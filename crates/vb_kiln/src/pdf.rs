//! Kiln PDF 写入器(自研 PDF 1.7 子集,零外部依赖)。
//!
//! 算法资产(源自 artboard 管线实测经验):
//! - 真实文本(Type1 WinAnsi + Tj):PDF 阅读器/Illustrator 可选中改字
//! - OCG 可选内容组:节点级图层,Acrobat/浏览器可开关
//! - 内容流操作符:m/l/c/re + rg/RG + cm 旋转
//! - Y 轴翻转:画布 Y 向下 → PDF 用户空间 Y 向上

use std::time::Instant;

use vb_render::encode::{DrawItem, DrawKind, FillDef};

use crate::context::ExportContext;
use crate::error::{KilnError, KilnResult};
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct PdfWriter;

impl FormatWriter for PdfWriter {
    fn format(&self) -> Format {
        Format::Pdf
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let warnings = common_warnings(ctx, Format::Pdf);
        let bytes = write_pdf(ctx, "Kiln PDF")?;
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = 1;
        let latin = LATIN_FALLBACK_COUNT.load(Ordering::Relaxed);
        if latin > 0 {
            r.warnings
                .push(crate::error::KilnWarning::UnembeddedLatinText);
        }
        Ok(r)
    }
}

/// PDF 写入核心(Ai 格式复用,仅 Producer 元数据不同)。
/// WinAnsi 兜底计数(导出内累计,write 时转警告)。
static LATIN_FALLBACK_COUNT: AtomicUsize = AtomicUsize::new(0);

pub const AI_HEAD: &str = "%%AI8_CreatorVersion: 28.0.0\n%%Creator: Kiln/VellumBench\n";

pub fn write_pdf(ctx: &ExportContext, producer: &str) -> KilnResult<Vec<u8>> {
    write_pdf_head(ctx, producer, "")
}

/// 同 [`write_pdf`],但把 `head` 注释行写在 PDF 头部**之内**。
/// AI 头必须在这里写:此前是「生成完 PDF 再往首行后插注释」,插入的 57 字节
/// 让全表 xref 偏移整体错位——严格解析器读到错位的目录对象,pikepdf 修复
/// 后会把「第二个 Catalog」当根(A4 双面合并只出 1 个画板即此因)。
pub fn write_pdf_head(ctx: &ExportContext, producer: &str, head: &str) -> KilnResult<Vec<u8>> {
    LATIN_FALLBACK_COUNT.store(0, Ordering::Relaxed);
    let w = ctx.logical_w;
    let h = ctx.logical_h;

    // 内容流 + CJK 字体使用收集(M3:CID 真文本)
    let mut usage = CjkUsage::new();
    let _probe = render_content_stream(ctx, &mut usage, false);
    finalize_cjk_fonts(&mut usage);
    // 两遍渲染间清除图形资源(图像/透明度/渐变 Pattern),防止 pass1+pass2
    // 重复导致资源编号错位和对象数翻倍。pass2 重新收集(remap=true 时正式发射)。
    usage.images.clear();
    usage.opacities.clear();
    usage.patterns.clear();
    usage.im_counter = 0;
    usage.gs_counter = 0;
    usage.pat_counter = 0;
    let content = render_content_stream(ctx, &mut usage, true);
    let content_z = content.into_bytes();

    let layer_names = crate::writer::collect_layers(&ctx.list);
    let n_layers = layer_names.len().min(64);

    // 对象布局:1 Catalog 2 Pages 3 Page 4 Content 5 F1 6 F2
    // 8.. OCG(n_layers 个);其后每 CJK 字体 5 个对象
    // (Type0 / CIDFontType2 / FontDescriptor / FontFile2 / ToUnicode)
    let ocg_start = 7u32;
    let font_base_start = ocg_start + n_layers.max(1) as u32;
    let n_cjk_font_objs = usage.fonts.len() as u32 * 5;
    let im_base_start = font_base_start + n_cjk_font_objs;
    let sm_base_start = im_base_start + usage.images.len() as u32;
    // 仅非全不透明图像才发 SMask 对象;基址必须按实际数量计,否则透明度
    // 对象编号错位(曾按图像总数预留 → gs 基址偏移 → ExtGState 引用错对象)
    let n_smasks = usage
        .images
        .iter()
        .filter(|(_, d, _, _)| d.chunks_exact(4).any(|px| px[3] != 255))
        .count() as u32;
    let gs_base_start = sm_base_start + n_smasks;
    let pat_base_start = gs_base_start + usage.opacities.len() as u32;
    let mut objects: Vec<Obj> = Vec::new();

    let catalog_id = 1u32;
    let pages_id = 2u32;
    let page_id = 3u32;
    let content_id = 4usize;
    let font_id = 5u32;
    let font_bold_id = 6u32;

    // 1 Catalog
    let mut catalog = format!("/Type /Catalog /Pages {pages_id} 0 R");
    if n_layers > 0 {
        let ocgs: Vec<String> = (0..n_layers)
            .map(|i| format!("{} 0 R", ocg_start + i as u32))
            .collect();
        catalog.push_str(&format!(
            " /OCProperties << /OCGs [{}] /D << /ON [{}] /Order [{}] >> >>",
            ocgs.join(" "),
            ocgs.join(" "),
            ocgs.join(" ")
        ));
    }
    objects.push(Obj::Dict(catalog));

    // 2 Pages
    objects.push(Obj::Dict(format!(
        "/Type /Pages /Kids [{page_id} 0 R] /Count 1"
    )));

    // 3 Page(资源字典含 WinAnsi + CJK CID 字体 + Shading + XObject + ExtGState)
    let mut font_res = format!("/F1 {font_id} 0 R /F2 {font_bold_id} 0 R");
    for (i, f) in usage.fonts.iter().enumerate() {
        let base = font_base_start + (i as u32) * 5;
        font_res.push_str(&format!(" /{} {} 0 R", f.resource, base));
    }
    let mut page = format!(
        "/Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {} {}] /Contents {content_id} 0 R /Resources << /Font << {} >>",
        fnum(w),
        fnum(h),
        font_res
    );
    // F2: XObject 资源(图像,含栅格化渐变)
    if !usage.images.is_empty() {
        let ims: Vec<String> = usage
            .images
            .iter()
            .enumerate()
            .map(|(i, (res, _, _, _))| format!("/{} {} 0 R", res, im_base_start + i as u32))
            .collect();
        page.push_str(&format!(" /XObject << {} >>", ims.join(" ")));
    }
    // F4': 矢量渐变 Pattern 资源(21 篇 G1)
    if !usage.patterns.is_empty() {
        let pats: Vec<String> = usage
            .patterns
            .iter()
            .enumerate()
            .map(|(i, p)| format!("/{} {} 0 R", p.resource, pat_base_start + i as u32))
            .collect();
        page.push_str(&format!(" /Pattern << {} >>", pats.join(" ")));
    }
    // F3: ExtGState 资源
    if n_layers > 0 {
        let props: Vec<String> = (0..n_layers)
            .map(|i| format!("/MC{i} {} 0 R", ocg_start + i as u32))
            .collect();
        page.push_str(&format!(" /Properties << {} >>", props.join(" ")));
    }
    page.push_str(" >>");
    objects.push(Obj::Dict(page));

    // 4 Content(stream)
    objects.push(Obj::Stream {
        dict: String::new(),
        data: content_z,
    });

    // 5/6 WinAnsi 字体
    objects.push(Obj::Dict(
        "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding".to_string(),
    ));
    objects.push(Obj::Dict(
        "/Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding"
            .to_string(),
    ));

    // 7.. OCG 对象
    for name in layer_names.iter().take(n_layers) {
        objects.push(Obj::Dict(format!(
            "/Type /OCG /Name {} /Intent [/View /Design] /Usage << /CreatorInfo << /Creator (Kiln) >> >>",
            pdf_text_string(name)
        )));
    }

    // CJK 字体对象组(Type0/CID/Descriptor/File2/ToUnicode)
    for f in &usage.fonts {
        let base = (font_base_start
            + (usage
                .fonts
                .iter()
                .position(|x| std::ptr::eq(x, f))
                .unwrap_or(0) as u32)
                * 5) as usize;
        let cid_id = base + 1;
        let fd_id = base + 2;
        let ff_id = base + 3;
        let tu_id = base + 4;
        let base_font = sanitize_font_name(&format!(
            "{}-{}-{}",
            f.name,
            f.weight,
            f.resource.trim_start_matches('/')
        ));

        // Type0
        objects.push(Obj::Dict(format!(
            "/Type /Font /Subtype /Type0 /BaseFont /{base_font} /Encoding /Identity-H /DescendantFonts [{cid_id} 0 R] /ToUnicode {tu_id} 0 R"
        )));
        // CIDFontType2
        let w_entries: Vec<String> = f
            .glyphs
            .iter()
            .map(|(cid, (_, w1000))| format!("{cid} [{w1000}]"))
            .collect();
        objects.push(Obj::Dict(format!(
            "/Type /Font /Subtype /CIDFontType2 /BaseFont /{base_font} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {fd_id} 0 R /DW 1000 /W [{}] /CIDToGIDMap /Identity",
            w_entries.join(" ")
        )));
        // FontDescriptor
        objects.push(Obj::Dict(format!(
            "/Type /FontDescriptor /FontName /{base_font} /Flags 4 /FontBBox [-1000 -300 2200 1200] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight 700 /StemV 80 /FontFile2 {ff_id} 0 R",
            fnum(f.ascent),
            fnum(f.descent)
        )));
        // FontFile2(子集化 TTF;失败回退全量)
        objects.push(Obj::Stream {
            dict: format!("/Length1 {}", f.subset_len1),
            data: f.subset.clone(),
        });
        // ToUnicode(规范 CMap:codespacerange/bfchar 均需数量前缀)
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Adobe-Identity-UCS def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
",
        );
        let pairs: Vec<String> = f
            .glyphs
            .iter()
            .map(|(cid, (ch, _))| {
                let mut uni_buf = [0u16; 4];
                let uni: Vec<String> = (*ch)
                    .encode_utf16(&mut uni_buf)
                    .iter()
                    .map(|u| format!("{u:04X}"))
                    .collect();
                format!("<{cid:04X}> <{}>", uni.join(""))
            })
            .collect();
        cmap.push_str(&format!(
            "{} beginbfchar
{}
endbfchar
",
            pairs.len(),
            pairs.join(
                "
"
            )
        ));
        cmap.push_str(
            "endcmap
CMapName currentdict /CMap defineresource pop
end
end",
        );
        objects.push(Obj::Stream {
            dict: String::new(),
            data: cmap.into_bytes(),
        });
    }

    // F2: Image XObject(FlateDecode RGB;非全不透明时加 SMask 设备灰度)。
    // 对象布局要求图像对象连续、其后 SMask 对象连续,故两趟发射。
    let mut smask_objs: Vec<Obj> = Vec::new();
    let mut smask_seq: usize = 0;
    for (_res, data, iw, ih) in usage.images.iter() {
        use std::io::Write as _;
        let mut rgb = Vec::with_capacity(data.len() / 4 * 3);
        let mut alpha = Vec::with_capacity(data.len() / 4);
        let mut uniform_opaque = true;
        for px in data.chunks_exact(4) {
            rgb.extend_from_slice(&[px[0], px[1], px[2]]);
            alpha.push(px[3]);
            if px[3] != 255 {
                uniform_opaque = false;
            }
        }
        let mut smask_ref = String::new();
        if !uniform_opaque {
            let mut enc =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(&alpha)
                .map_err(|e| KilnError::Encode(e.to_string()))?;
            let sm = enc.finish().map_err(|e| KilnError::Encode(e.to_string()))?;
            smask_objs.push(Obj::Stream {
                dict: format!(
                    "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode",
                    iw, ih
                ),
                data: sm,
            });
            smask_ref = format!(" /SMask {} 0 R", sm_base_start + smask_seq as u32);
            smask_seq += 1;
        }
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&rgb)
            .map_err(|e| KilnError::Encode(e.to_string()))?;
        let compressed = enc.finish().map_err(|e| KilnError::Encode(e.to_string()))?;
        objects.push(Obj::Stream {
            dict: format!(
                "/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode{}",
                iw, ih, smask_ref
            ),
            data: compressed,
        });
    }
    objects.extend(smask_objs);

    // F3: ExtGState 对象(透明度)
    for (res, alpha) in &usage.opacities {
        let _ = res;
        objects.push(Obj::Dict(format!(
            "/Type /ExtGState /ca {} /CA {}",
            fnum(*alpha),
            fnum(*alpha)
        )));
    }

    // F4': 矢量渐变 Pattern 对象(G1;Function 内嵌字典,chrome 同构)
    for p in &usage.patterns {
        objects.push(Obj::Dict(pattern_dict(p)));
    }

    // F4: 无 Shading/Pattern 对象 —— 渐变一律栅格位图(pdfium 不渲染
    // sh/PatternType 2;Illustrator 对非常规 shading 组合报「未知的阴影类型」)

    // 组装
    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    out.extend_from_slice(head.as_bytes());
    let mut offsets: Vec<u64> = Vec::with_capacity(objects.len());
    for (idx, obj) in objects.iter().enumerate() {
        offsets.push(out.len() as u64);
        out.extend_from_slice(format!("{} 0 obj\n", idx + 1).as_bytes());
        match obj {
            Obj::Stream { dict, data } => {
                if dict.is_empty() {
                    out.extend_from_slice(
                        format!("<< /Length {} >>\nstream\n", data.len()).as_bytes(),
                    );
                } else {
                    out.extend_from_slice(
                        format!("<< {dict} /Length {} >>\nstream\n", data.len()).as_bytes(),
                    );
                }
                out.extend_from_slice(data);
                out.extend_from_slice(b"\nendstream\nendobj\n");
            }
            Obj::Dict(d) => {
                out.extend_from_slice(format!("<< {d} >>").as_bytes());
                out.extend_from_slice(b"\nendobj\n");
            }
        }
    }
    let xref_at = out.len() as u64;
    let count = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {count}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {count} /Root {catalog_id} 0 R /Producer ({}) /Creator (Kiln/VellumBench) >>\nstartxref\n{xref_at}\n%%EOF\n",
            escape_pdf_string(producer)
        )
        .as_bytes(),
    );
    Ok(out)
}

/// PDF 间接对象(字典或流)。
enum Obj {
    Dict(String),
    Stream { dict: String, data: Vec<u8> },
}

/// 渐变色标 → PDF Function 字典(chrome 同构):
/// 2 档 = FunctionType 2;≥3 档 = FunctionType 3 stitching(内档位为 Bounds)。
fn gradient_function_dict(stops: &[(f32, [f32; 3])]) -> String {
    let rgb = |c: [f32; 3]| {
        format!(
            "{} {} {}",
            fnum(c[0] as f64),
            fnum(c[1] as f64),
            fnum(c[2] as f64)
        )
    };
    let type2 = |a: [f32; 3], b: [f32; 3]| {
        format!(
            "<< /FunctionType 2 /Domain [0 1] /N 1 /C0 [{}] /C1 [{}] >>",
            rgb(a),
            rgb(b)
        )
    };
    if stops.len() <= 2 {
        let c0 = stops.first().map(|s| s.1).unwrap_or([0.0; 3]);
        let c1 = stops.last().map(|s| s.1).unwrap_or(c0);
        return type2(c0, c1);
    }
    let bounds: Vec<String> = stops[1..stops.len() - 1]
        .iter()
        .map(|(p, _)| fnum(*p as f64))
        .collect();
    let funcs: Vec<String> = stops.windows(2).map(|w| type2(w[0].1, w[1].1)).collect();
    let encode: Vec<&str> = funcs.iter().map(|_| "0 1").collect();
    format!(
        "<< /FunctionType 3 /Domain [0 1] /Bounds [{}] /Encode [{}] /Functions [{}] >>",
        bounds.join(" "),
        encode.join(" "),
        funcs.join(" ")
    )
}

/// Pattern 字典(PatternType 2 + Shading;coords 为用户空间,Matrix 省略 = 单位阵)。
fn pattern_dict(p: &GradPattern) -> String {
    let coords: Vec<String> = p.coords.iter().map(|v| fnum(*v)).collect();
    format!(
        "/Type /Pattern /PatternType 2 /Shading << /ShadingType {} /ColorSpace /DeviceRGB /Coords [{}] /Extend [true true] /Function {} >>",
        p.shading_type,
        coords.join(" "),
        gradient_function_dict(&p.stops)
    )
}

/// 单个 CJK 字体的使用记录(内容流渲染期收集)。
pub struct CjkFont {
    pub name: String,
    pub weight: u16,
    pub resource: String,
    /// 旧 gid → 子集内新 cid
    pub remap: std::collections::HashMap<u16, u16>,
    /// cid(gid) → (unicode, 宽度/1000)
    pub glyphs: std::collections::BTreeMap<u16, (char, f64)>,
    pub ascent: f64,
    pub descent: f64,
    pub subset: Vec<u8>,
    pub subset_len1: usize,
}

/// PDF 文本串:非 ASCII 时用 UTF-16BE 十六进制串(OCG /Name 等需严格
/// PDFDocEncoding/UTF-16 语义;裸 UTF-8 会被严格解析器拒绝)。
fn pdf_text_string(s: &str) -> String {
    if s.chars().all(|c| (c as u32) < 0x80) && !s.contains(['(', ')']) {
        format!("({s})")
    } else {
        let mut hex = String::from("<FEFF");
        for u in s.encode_utf16() {
            hex.push_str(&format!("{u:04X}"));
        }
        hex.push('>');
        hex
    }
}

fn sanitize_font_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '+')
        .collect();
    if cleaned.is_empty() {
        "Font".to_string()
    } else {
        cleaned
    }
}

/// 矢量渐变 Pattern(21 篇 G1:结构复刻 chrome printToPDF 产物——
/// PatternType 2 + ShadingType 2/3 + FunctionType 2/3,pdfium/Illustrator
/// 双端渲染已由 chrome 产物背书)。
pub struct GradPattern {
    pub resource: String,
    /// 2 = axial(线性),3 = radial(径向)。
    pub shading_type: u32,
    /// 用户空间坐标(axial 4 个;radial 6 个)。
    pub coords: Vec<f64>,
    /// (pos, rgb)——alpha 一致性由调用侧保证,经 ExtGState ca 承担。
    pub stops: Vec<(f32, [f32; 3])>,
}

/// 全部 CJK 字体使用。
#[derive(Default)]
pub struct CjkUsage {
    pub fonts: Vec<CjkFont>,
    /// 图像资源(F2):resource → (FlateDecode RGBA 数据, 宽, 高;A 通道发 SMask)
    pub images: Vec<(String, Vec<u8>, u32, u32)>,
    /// 透明度资源(F3):resource → alpha
    pub opacities: Vec<(String, f64)>,
    /// 矢量渐变 Pattern(G1)。
    pub patterns: Vec<GradPattern>,
    im_counter: u32,
    gs_counter: u32,
    pat_counter: u32,
}

impl CjkUsage {
    pub fn new() -> Self {
        CjkUsage {
            fonts: Vec::new(),
            images: Vec::new(),
            opacities: Vec::new(),
            patterns: Vec::new(),
            im_counter: 0,
            gs_counter: 0,
            pat_counter: 0,
        }
    }

    fn image_for(&mut self, data: Vec<u8>, w: u32, h: u32) -> String {
        self.im_counter += 1;
        let res = format!("Im{}", self.im_counter);
        self.images.push((res.clone(), data, w, h));
        res
    }

    /// 矢量渐变 Pattern 登记(G1;resource 命名空间独立于图像)。
    fn gradient_pattern_for(
        &mut self,
        shading_type: u32,
        coords: Vec<f64>,
        stops: Vec<(f32, [f32; 3])>,
    ) -> String {
        self.pat_counter += 1;
        let res = format!("GP{}", self.pat_counter);
        self.patterns.push(GradPattern {
            resource: res.clone(),
            shading_type,
            coords,
            stops,
        });
        res
    }

    fn opaque_gs(&mut self) -> String {
        if let Some((res, _)) = self.opacities.iter().find(|(_, a)| (*a - 1.0).abs() < 1e-9) {
            return res.clone();
        }
        self.gs_counter += 1;
        let res = format!("GS{}", self.gs_counter);
        self.opacities.push((res.clone(), 1.0));
        res
    }

    /// 透明度资源按值去重:emit_gs 现在每项都显式设置 ca(防前项残留),
    /// 不去重会让 ExtGState 对象数按绘制项线性膨胀。
    fn opacity_for(&mut self, alpha: f64) -> String {
        let a = (alpha.clamp(0.001, 0.999) * 1000.0).round() / 1000.0;
        if let Some((res, _)) = self.opacities.iter().find(|(_, x)| (*x - a).abs() < 1e-9) {
            return res.clone();
        }
        self.gs_counter += 1;
        let res = format!("GS{}", self.gs_counter);
        self.opacities.push((res.clone(), a));
        res
    }

    fn resource_for(&mut self, family: &str, weight: u16) -> String {
        let name = family.trim().trim_matches('"').to_string();
        if let Some(f) = self
            .fonts
            .iter_mut()
            .find(|f| f.name == name && f.weight == weight)
        {
            return f.resource.clone();
        }
        let resource = format!("CF{}", self.fonts.len() + 1);
        self.fonts.push(CjkFont {
            name,
            weight,
            resource: resource.clone(),
            remap: Default::default(),
            glyphs: Default::default(),
            ascent: 800.0,
            descent: -200.0,
            subset: Vec::new(),
            subset_len1: 0,
        });
        resource
    }

    fn record(
        &mut self,
        family: &str,
        weight: u16,
        gid: u16,
        ch: char,
        advance_px: f64,
        font_size: f64,
    ) {
        if let Some(f) = self
            .fonts
            .iter_mut()
            .find(|f| f.name == family.trim().trim_matches('"') && f.weight == weight)
        {
            let w1000 = if font_size > 0.0 {
                (advance_px / font_size * 1000.0).round()
            } else {
                1000.0
            };
            f.glyphs.entry(gid).or_insert((ch, w1000));
        }
    }

    fn set_metrics(&mut self, family: &str, weight: u16, ascent: f64, descent: f64) {
        if let Some(f) = self
            .fonts
            .iter_mut()
            .find(|f| f.name == family.trim().trim_matches('"') && f.weight == weight)
        {
            f.ascent = ascent;
            f.descent = descent;
        }
    }

    fn remap_cid(&self, family: &str, weight: u16, gid: u16) -> u16 {
        self.fonts
            .iter()
            .find(|f| f.name == family.trim().trim_matches('"') && f.weight == weight)
            .and_then(|f| f.remap.get(&gid).copied())
            .unwrap_or(gid)
    }
}

/// 字体子集化 + 写回(subsetter 失败回退全量)。
fn finalize_cjk_fonts(usage: &mut CjkUsage) {
    for f in usage.fonts.iter_mut() {
        let Some((data, index)) = vb_render::text::font_data_for(&f.name, f.weight) else {
            continue;
        };
        // 子集化(缩减文件体积);subsetter 的 GID 排序经 ToUnicode 交叉验证一致。
        // 变量字体(NotoSerifSC-VF 等)必须**实例化到目标 wght**:PDF 不支持
        // 变量字体,嵌入原始 VF 会让 Illustrator/pdfium 取默认实例(≈400)——
        // `font-weight:900` 的大标题在 AI 里变细体(实测 A4 标题)。
        let gids: Vec<u16> = f.glyphs.keys().copied().collect();
        let remapper = subsetter::GlyphRemapper::new_from_glyphs_sorted(&gids);
        let coords = [(subsetter::Tag::new(b"wght"), f.weight as f32)];
        let instanced =
            subsetter::subset_with_variations(&data, index as u32, &coords, &remapper).ok();
        let sub_result = match instanced {
            Some(s) if !s.is_empty() => Ok(s),
            _ => subsetter::subset(&data, index as u32, &remapper),
        };
        match sub_result {
            Ok(sub) if !sub.is_empty() && sub.len() < data.len() => {
                f.subset_len1 = sub.len();
                f.subset = sub;
            }
            _ => {
                f.subset_len1 = data.len();
                f.subset = (*data).clone();
            }
        }
        // 重映射 cid 表
        let old: Vec<(u16, (char, f64))> = f.glyphs.iter().map(|(k, v)| (*k, *v)).collect();
        let mut new_map = std::collections::BTreeMap::new();
        for (gid, info) in old {
            let new_cid = remapper.get(gid).unwrap_or(0);
            f.remap.insert(gid, new_cid);
            new_map.insert(new_cid, info);
        }
        f.glyphs = new_map;
    }
}

/// 内容流:DrawItem → PDF 操作符(Y 翻转;文本 Tj;OCG BDC/EMC)。
fn render_content_stream(ctx: &ExportContext, usage: &mut CjkUsage, remap: bool) -> String {
    let h = ctx.logical_h;
    let mut s = String::with_capacity(32 * 1024);
    if !ctx.transparent {
        let bg = ctx.list.background;
        s.push_str(&format!(
            "q {} {} {} rg 0 0 {} {} re f Q
",
            fnum(bg[0]),
            fnum(bg[1]),
            fnum(bg[2]),
            fnum(ctx.logical_w),
            fnum(ctx.logical_h)
        ));
    }
    let n_layers = ctx.list.items.iter().map(|i| i.layer).max().unwrap_or(0) as usize + 1;
    for item in ctx.list.items.iter() {
        // ADR-0021:项级 OCG 标记(/Properties 已注册 /MC{i};Illustrator
        // 图层面板按此分组,G4/pdfium 门禁盯文本提取回归)
        if std::env::var("KILN_NO_BDC").is_err() {
            s.push_str(&format!(
                "/OC /MC{} BDC
",
                (item.layer as usize).min(n_layers - 1)
            ));
        }
        draw_item_pdf(&mut s, item, h, usage, remap);
        s.push_str(
            "EMC
",
        );
    }
    s
}

fn draw_item_pdf(s: &mut String, item: &DrawItem, page_h: f64, usage: &mut CjkUsage, remap: bool) {
    let [x, y, w, h] = item.rect;
    let py = page_h - y - h;
    let rotated = item.rot.abs() > 1e-9;
    let has_clip = item.clip.is_some();

    // ---- 图形状态隔离:q/Q 用于旋转(cm)和/或裁剪(W n)----
    let need_q = rotated || has_clip;
    if need_q {
        s.push_str("q\n");
    }
    if rotated {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let rad = item.rot.to_radians();
        let (sn, cs) = (rad.sin(), rad.cos());
        let pcy = page_h - cy;
        s.push_str(&format!(
            "1 0 0 1 {} {} cm\n{} {} {} {} 0 0 cm\n1 0 0 1 {} {} cm\n",
            fnum(cx),
            fnum(pcy),
            fnum(cs),
            fnum(sn),
            fnum(-sn),
            fnum(cs),
            fnum(-cx),
            fnum(cy - page_h)
        ));
    }
    if has_clip {
        emit_clip_path(s, item.clip.as_ref().unwrap(), x, y, w, h, py, page_h);
    }

    // ---- 填充 / 描边 ----
    // 透明度 = 项 opacity × 颜色 alpha,按部分各自发一次 ExtGState;
    // 渐变填充的半透明烘进位图 SMask(与 CPU 栅格 to_skia_stops 一致)。
    if item.fill.is_some() || item.border.is_some() {
        if let Some(fill) = &item.fill {
            match fill {
                FillDef::Solid(c) => {
                    // F5: filter 色彩调整(brightness/saturate)
                    let cc = apply_filter_color(*c, item.filter.as_ref());
                    emit_gs(s, usage, item.opacity as f64 * cc[3].clamp(0.0, 1.0) as f64);
                    s.push_str(&format!(
                        "{} {} {} rg\n",
                        fnum(cc[0]),
                        fnum(cc[1]),
                        fnum(cc[2])
                    ));
                    path_ops(s, item, x, py, w, h);
                    s.push_str("f\n");
                }
                FillDef::LinearGradient { angle_css, stops } => {
                    let stops = apply_filter_stops(stops, item.filter.as_ref());
                    // G1(21 篇):色标 alpha 一致 → 真矢量 Pattern(chrome 结构,
                    // pdfium/AI 双端背书,可编辑、无位图/无栅格化色带);
                    // 混合 alpha → 位图 + SMask 兜底(逐像素 alpha 只有位图能表达)
                    let alpha0 = stops[0].color[3] * item.opacity;
                    let uniform = stops
                        .iter()
                        .all(|s| (s.color[3] * item.opacity - alpha0).abs() < 1e-3);
                    if uniform {
                        let rad = (*angle_css).to_radians();
                        let dx = rad.sin() as f32;
                        let dy = -(rad.cos()) as f32;
                        let l = (w as f32 * dx.abs()) + (h as f32 * dy.abs());
                        let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
                        let (sx, sy) = (cx - dx * l / 2.0, cy - dy * l / 2.0);
                        let (ex, ey) = (cx + dx * l / 2.0, cy + dy * l / 2.0);
                        let coords = vec![
                            x + sx as f64,
                            page_h - (y + sy as f64),
                            x + ex as f64,
                            page_h - (y + ey as f64),
                        ];
                        let rgb: Vec<(f32, [f32; 3])> = stops
                            .iter()
                            .map(|s| (s.pos, [s.color[0], s.color[1], s.color[2]]))
                            .collect();
                        let res = usage.gradient_pattern_for(2, coords, rgb);
                        emit_gs(s, usage, alpha0 as f64);
                        // q/Q 隔离 /Pattern cs 对图形状态色彩的污染;
                        // 形状由 path_ops 原生表达(圆角/椭圆),零 W n
                        s.push_str("q /Pattern cs\n");
                        s.push_str(&format!("/{} scn\n", res));
                        path_ops(s, item, x, py, w, h);
                        s.push_str("f\nQ\n");
                    } else {
                        // 渐变一律栅格位图:pdfium/Chrome 的 PDF 打印也不会产生
                        // Shading 对象,自研写 shading 会踩两个坑——pdfium 不渲染
                        // sh/PatternType2(对拍直接变成空白),Illustrator 对非常规
                        // shading 组合报「未知的阴影类型」。位图路线两边都稳。
                        // 圆角/椭圆遮罩烘入 alpha 通道,因此不发 W n(零剪切蒙版)。
                        {
                            let (mut data, gw, gh) =
                                gradient_bitmap_linear(*angle_css, &stops, w, h, item.opacity);
                            bake_shape_mask(
                                &mut data,
                                gw,
                                gh,
                                w,
                                h,
                                shape_radius(item),
                                item.ellipse,
                            );
                            let im_res = usage.image_for(data, gw, gh);
                            // 前项 Solid 的低 ca 会泄漏到本 Do(渐变 alpha 已烘进位图):
                            // 位图渐变必须以不透明 ca 绘制
                            let gs_reset = usage.opaque_gs();
                            s.push_str(&format!(
                                "q /{gs_reset} gs {} 0 0 {} {} {} cm /{} Do Q\n",
                                fnum(w),
                                fnum(h),
                                fnum(x),
                                fnum(py),
                                im_res
                            ));
                        }
                    }
                }
                FillDef::RadialGradient {
                    cx: gcx,
                    cy: gcy,
                    stops,
                } => {
                    let stops = apply_filter_stops(stops, item.filter.as_ref());
                    let alpha0 = stops[0].color[3] * item.opacity;
                    let uniform = stops
                        .iter()
                        .all(|s| (s.color[3] * item.opacity - alpha0).abs() < 1e-3);
                    if uniform {
                        // 径向:圆心项内比例坐标,半径 = √(w²+h²)/2(与 CPU 栅格一致)
                        let ccx = w * (*gcx as f64);
                        let ccy = h * (*gcy as f64);
                        let r = (w * w + h * h).sqrt() / 2.0;
                        let cpx = x + ccx;
                        let cpy = page_h - (y + ccy);
                        let coords = vec![cpx, cpy, 0.0, cpx, cpy, r];
                        let rgb: Vec<(f32, [f32; 3])> = stops
                            .iter()
                            .map(|s| (s.pos, [s.color[0], s.color[1], s.color[2]]))
                            .collect();
                        let res = usage.gradient_pattern_for(3, coords, rgb);
                        emit_gs(s, usage, alpha0 as f64);
                        s.push_str("q /Pattern cs\n");
                        s.push_str(&format!("/{} scn\n", res));
                        path_ops(s, item, x, py, w, h);
                        s.push_str("f\nQ\n");
                    } else {
                        {
                            let (mut data, gw, gh) =
                                gradient_bitmap_radial(*gcx, *gcy, &stops, w, h, item.opacity);
                            bake_shape_mask(
                                &mut data,
                                gw,
                                gh,
                                w,
                                h,
                                shape_radius(item),
                                item.ellipse,
                            );
                            let im_res = usage.image_for(data, gw, gh);
                            let gs_reset = usage.opaque_gs();
                            s.push_str(&format!(
                                "q /{gs_reset} gs {} 0 0 {} {} {} cm /{} Do Q\n",
                                fnum(w),
                                fnum(h),
                                fnum(x),
                                fnum(py),
                                im_res
                            ));
                        }
                    }
                }
            }
        }
        if let Some(border) = &item.border {
            emit_gs(
                s,
                usage,
                item.opacity as f64 * border.color[3].clamp(0.0, 1.0) as f64,
            );
            s.push_str(&format!(
                "{} {} {} RG {} w\n",
                fnum(border.color[0]),
                fnum(border.color[1]),
                fnum(border.color[2]),
                fnum(border.width)
            ));
            path_ops(s, item, x, py, w, h);
            s.push_str("S\n");
        }
    }

    // ---- 文本(CID 真文本 / 轮廓兜底 / WinAnsi 兜底)----
    if let Some(label) = &item.label {
        emit_gs(
            s,
            usage,
            item.opacity as f64 * label.color[3].clamp(0.0, 1.0) as f64,
        );
        let has_cjk = label.text.chars().any(|ch| {
            let cp = ch as u32;
            !(0x20..0x7f).contains(&cp) && !(0xa0..0xff).contains(&cp)
        });
        let line_h = if label.line_height > 0.0 {
            label.line_height
        } else {
            label.font_size * 1.32
        };
        let max_w = w.max(1.0) as f32;
        let ls = label.letter_spacing as f32;
        let seg_ranges: Vec<(usize, usize)> =
            label.segments.iter().map(|sg| (sg.start, sg.end)).collect();
        let font_ok = vb_render::text::font_data_for(&label.font_family, label.weight).is_some();

        if font_ok {
            // T2(21 篇):分段携带字号/字重/字体时逐段整形——旧口径在基础
            // 字体整行整形后才切片,段只换色,重点字被统一成正文。
            let styled = label.segments.iter().any(|sg| {
                sg.font_size.is_some() || sg.bold.is_some() || !sg.font_family.is_empty()
            });
            if styled {
                let spans: Vec<vb_render::text::StyleSpan> = label
                    .segments
                    .iter()
                    .map(|sg| vb_render::text::StyleSpan {
                        start: sg.start,
                        end: sg.end,
                        color: sg.color,
                        bold: sg.bold,
                        font_size: sg.font_size,
                        font_family: sg.font_family.clone(),
                    })
                    .collect();
                vb_render::text::for_each_styled_line(
                    &label.text,
                    &label.font_family,
                    label.font_size as f32,
                    label.weight,
                    max_w,
                    ls,
                    &spans,
                    |vi, parts| {
                        // 行基线 = 行盒顶 + 行内最大 ascent(浏览器行盒语义)
                        let asc = parts.iter().map(|p| p.ascent).fold(0.0f32, f32::max);
                        let baseline_art = y + asc as f64 + vi as f64 * line_h;
                        for part in parts {
                            let res = usage.resource_for(&part.font_family, part.weight);
                            usage.set_metrics(
                                &part.font_family,
                                part.weight,
                                part.ascent as f64 / (part.font_size.max(1.0) as f64) * 1000.0,
                                -(part.descent as f64) / (part.font_size.max(1.0) as f64) * 1000.0,
                            );
                            let color = part
                                .seg
                                .and_then(|i| label.segments.get(i))
                                .and_then(|sg| sg.color)
                                .unwrap_or(label.color);
                            s.push_str("BT\n");
                            s.push_str(&format!("/{} {} Tf\n", res, fnum(part.font_size as f64)));
                            s.push_str(&format!("{} Tc\n", fnum(ls as f64)));
                            s.push_str(&format!(
                                "{} {} {} rg\n",
                                fnum(color[0]),
                                fnum(color[1]),
                                fnum(color[2])
                            ));
                            s.push_str(&format!(
                                "1 0 0 1 {} {} Tm\n",
                                fnum(x + part.x),
                                fnum(page_h - baseline_art)
                            ));
                            let mut hexes = Vec::with_capacity(part.gids.len());
                            for (k, &gid) in part.gids.iter().enumerate() {
                                if !remap {
                                    if let Some(ch) = part.text.chars().nth(k) {
                                        usage.record(
                                            &part.font_family,
                                            part.weight,
                                            gid,
                                            ch,
                                            part.advances[k] as f64,
                                            part.font_size as f64,
                                        );
                                    }
                                }
                                let cid = if remap {
                                    usage.remap_cid(&part.font_family, part.weight, gid)
                                } else {
                                    gid
                                };
                                hexes.push(format!("{cid:04X}"));
                            }
                            s.push_str(&format!("<{}> Tj\n", hexes.join("")));
                            s.push_str("ET\n");
                        }
                    },
                );
            } else {
                let resource = usage.resource_for(&label.font_family, label.weight);
                vb_render::text::for_each_visual_line(
                    &label.text,
                    &label.font_family,
                    label.font_size as f32,
                    label.weight,
                    max_w,
                    ls,
                    |vi, hard, run, line, byte_base| {
                        usage.set_metrics(
                            &label.font_family,
                            label.weight,
                            run.ascent as f64 / label.font_size.max(1.0) * 1000.0,
                            -(run.descent as f64) / label.font_size.max(1.0) * 1000.0,
                        );
                        let baseline_art = y + run.ascent as f64 + vi as f64 * line_h;
                        let parts = vb_render::text::split_line_segments(
                            hard,
                            line,
                            run,
                            byte_base,
                            &seg_ranges,
                            ls,
                        );
                        s.push_str("BT\n");
                        s.push_str(&format!("/{resource} {} Tf\n", fnum(label.font_size)));
                        // 字距用 PDF 原生 Tc 表达:段落内部的字距此前只在段起点
                        // 一次性偏移,段内字形按自然 advance 排布 → 行内逐字漂移
                        // (A4 大标题字距 .02em,行末累计偏 ~19px)。Tc 与 CPU 栅格
                        // 的 `g.x + 行内字形序 × ls` 逐字形等价。
                        s.push_str(&format!("{} Tc\n", fnum(ls as f64)));
                        for part in parts {
                            let color = part
                                .seg
                                .and_then(|i| label.segments.get(i))
                                .and_then(|sg| sg.color)
                                .unwrap_or(label.color);
                            s.push_str(&format!(
                                "{} {} {} rg\n",
                                fnum(color[0]),
                                fnum(color[1]),
                                fnum(color[2])
                            ));
                            // 旋转/裁剪由外层 q + cm 统一处理,这里只做平移
                            s.push_str(&format!(
                                "1 0 0 1 {} {} Tm\n",
                                fnum(x + part.x),
                                fnum(page_h - baseline_art)
                            ));
                            let mut hexes = Vec::with_capacity(part.gids.len());
                            for (k, &gid) in part.gids.iter().enumerate() {
                                if !remap {
                                    if let Some(ch) = part.text.chars().nth(k) {
                                        usage.record(
                                            &label.font_family,
                                            label.weight,
                                            gid,
                                            ch,
                                            part.advances[k] as f64,
                                            label.font_size,
                                        );
                                    }
                                }
                                let cid = if remap {
                                    usage.remap_cid(&label.font_family, label.weight, gid)
                                } else {
                                    gid
                                };
                                hexes.push(format!("{cid:04X}"));
                            }
                            s.push_str(&format!("<{}> Tj\n", hexes.join("")));
                        }
                        s.push_str("ET\n");
                    },
                );
            }
        } else if has_cjk {
            s.push_str(&format!(
                "{} {} {} rg\n",
                fnum(label.color[0]),
                fnum(label.color[1]),
                fnum(label.color[2])
            ));
            vb_render::text::for_each_visual_line(
                &label.text,
                &label.font_family,
                label.font_size as f32,
                label.weight,
                max_w,
                ls,
                |vi, hard, run, line, _bb| {
                    let s0: usize = hard.chars().take(line[0]).map(|ch| ch.len_utf8()).sum();
                    let last = *line.last().expect("nonempty");
                    let s1: usize = hard.chars().take(last + 1).map(|ch| ch.len_utf8()).sum();
                    let sub = &hard[s0..s1];
                    outline_text_pdf(
                        s,
                        sub,
                        &label.font_family,
                        label.font_size,
                        label.weight,
                        x,
                        y + vi as f64 * line_h,
                        h,
                        page_h,
                        ls as f64,
                        line[0],
                    );
                    let _ = run;
                },
            );
        } else {
            // S2(19 篇 §2.4):无任何字体数据可嵌入时的拉丁兜底——
            // 未嵌入 Helvetica 有换机替换风险,计数并在报告中显式声明
            LATIN_FALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
            let fref = if label.weight >= 600 { "/F2" } else { "/F1" };
            vb_render::text::for_each_visual_line(
                &label.text,
                &label.font_family,
                label.font_size as f32,
                label.weight,
                max_w,
                ls,
                |vi, hard, run, line, byte_base| {
                    let asc = run.ascent as f64;
                    let baseline = page_h - (y + asc + vi as f64 * line_h);
                    s.push_str("BT\n");
                    s.push_str(&format!("{fref} {} Tf\n", fnum(label.font_size)));
                    s.push_str(&format!("{} Tc\n", fnum(ls as f64)));
                    let parts = vb_render::text::split_line_segments(
                        hard,
                        line,
                        run,
                        byte_base,
                        &seg_ranges,
                        ls,
                    );
                    for part in parts {
                        let color = part
                            .seg
                            .and_then(|i| label.segments.get(i))
                            .and_then(|sg| sg.color)
                            .unwrap_or(label.color);
                        s.push_str(&format!(
                            "{} {} {} rg\n",
                            fnum(color[0]),
                            fnum(color[1]),
                            fnum(color[2])
                        ));
                        s.push_str(&format!(
                            "1 0 0 1 {} {} Tm\n",
                            fnum(x + part.x),
                            fnum(baseline)
                        ));
                        let text = winansi_escaped(part.text);
                        s.push_str(&format!("({text}) Tj\n"));
                    }
                    s.push_str("ET\n");
                },
            );
        }
    }

    // ---- 图像(XObject RGBA 嵌入;旋转由外层 q/cm 统一处理)----
    if item.kind == DrawKind::Image {
        // 位图缺失时不画占位,与 CPU 栅格的跳过行为一致
        if let Some(bmp) = &item.image {
            let im_res = usage.image_for(bmp.rgba.to_vec(), bmp.width, bmp.height);
            emit_gs(s, usage, item.opacity as f64);
            s.push_str(&format!(
                "q {} 0 0 {} {} {} cm /{} Do Q\n",
                fnum(w),
                fnum(h),
                fnum(x),
                fnum(py),
                im_res
            ));
        }
    }

    if need_q {
        s.push_str("Q\n");
    }
}

/// F3: 发射 ExtGState 透明度(按值去重;每项都显式设置)。
///
/// 关键:此前只在 alpha<1 时设置,**不还原 ca=1**。PDF 的 ca 是图形状态,
/// 上一个半透明项之后所有不透明项都会被连带画淡——A4 标题整块褪色即此因
/// (PDF 里的 ca 泄漏;CPU 栅格逐项独立,不会暴露)。
fn emit_gs(s: &mut String, usage: &mut CjkUsage, alpha: f64) {
    let a = alpha.clamp(0.0, 1.0);
    let gs_res = if a >= 0.999 {
        usage.opaque_gs()
    } else {
        usage.opacity_for(a)
    };
    s.push_str(&format!("/{} gs\n", gs_res));
}

/// F5: 应用 brightness/saturate 调整到颜色。
fn apply_filter_color(mut c: [f32; 4], filter: Option<&vb_render::encode::FilterDef>) -> [f32; 4] {
    if let Some(f) = filter {
        if (f.brightness - 1.0).abs() > 1e-3 {
            for ch in c.iter_mut().take(3) {
                *ch = ((*ch as f32) * f.brightness as f32).clamp(0.0, 1.0);
            }
        }
        if (f.saturate - 1.0).abs() > 1e-3 {
            let l = 0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32;
            for ch in c.iter_mut().take(3) {
                *ch = (l + (*ch as f32 - l) * f.saturate as f32).clamp(0.0, 1.0);
            }
        }
    }
    c
}

/// F5: 应用 filter 到渐变 stop 色彩。
fn apply_filter_stops(
    stops: &[vb_render::encode::GradientStop],
    filter: Option<&vb_render::encode::FilterDef>,
) -> Vec<vb_render::encode::GradientStop> {
    let Some(f) = filter else {
        return stops.to_vec();
    };
    stops
        .iter()
        .map(|s| {
            let mut c = s.color;
            if (f.brightness - 1.0).abs() > 1e-3 {
                for ch in c.iter_mut().take(3) {
                    *ch = ((*ch as f32) * f.brightness as f32).clamp(0.0, 1.0);
                }
            }
            if (f.saturate - 1.0).abs() > 1e-3 {
                let l = 0.2126 * c[0] as f32 + 0.7152 * c[1] as f32 + 0.0722 * c[2] as f32;
                for ch in c.iter_mut().take(3) {
                    *ch = (l + (*ch as f32 - l) * f.saturate as f32).clamp(0.0, 1.0);
                }
            }
            vb_render::encode::GradientStop {
                pos: s.pos,
                color: c,
            }
        })
        .collect()
}

/// F4: 发射 clip path(W n)。坐标从项局部转换到 PDF 用户空间(Y 翻转)。
fn emit_clip_path(
    s: &mut String,
    clip: &vb_render::encode::ClipDef,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    py: f64,
    page_h: f64,
) {
    let pdf_y = |local_y: f64| py + h - local_y;
    match clip {
        vb_render::encode::ClipDef::Inset(t, r, b, l) => {
            let cl = x + l;
            let cb = pdf_y(h - b);
            let cw = w - l - r;
            let ch = h - t - b;
            s.push_str(&format!(
                "{} {} {} {} re W n\n",
                fnum(cl),
                fnum(cb),
                fnum(cw.max(0.0)),
                fnum(ch.max(0.0))
            ));
        }
        vb_render::encode::ClipDef::Circle(cx, cy, r) => {
            let pdf_cx = x + cx;
            let pdf_cy = pdf_y(*cy);
            let k = 0.5523;
            let rx = r;
            let ry = r;
            s.push_str(&format!("{} {} m\n", fnum(pdf_cx + rx), fnum(pdf_cy)));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx + rx),
                fnum(pdf_cy + k * ry),
                fnum(pdf_cx + k * rx),
                fnum(pdf_cy + ry),
                fnum(pdf_cx),
                fnum(pdf_cy + ry)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx - k * rx),
                fnum(pdf_cy + ry),
                fnum(pdf_cx - rx),
                fnum(pdf_cy + k * ry),
                fnum(pdf_cx - rx),
                fnum(pdf_cy)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx - rx),
                fnum(pdf_cy - k * ry),
                fnum(pdf_cx - k * rx),
                fnum(pdf_cy - ry),
                fnum(pdf_cx),
                fnum(pdf_cy - ry)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx + k * rx),
                fnum(pdf_cy - ry),
                fnum(pdf_cx + rx),
                fnum(pdf_cy - k * ry),
                fnum(pdf_cx + rx),
                fnum(pdf_cy)
            ));
            s.push_str("h\nW n\n");
        }
        vb_render::encode::ClipDef::Ellipse(cx, cy, rx, ry) => {
            let pdf_cx = x + cx;
            let pdf_cy = pdf_y(*cy);
            let k = 0.5523;
            s.push_str(&format!("{} {} m\n", fnum(pdf_cx + rx), fnum(pdf_cy)));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx + rx),
                fnum(pdf_cy + k * ry),
                fnum(pdf_cx + k * rx),
                fnum(pdf_cy + ry),
                fnum(pdf_cx),
                fnum(pdf_cy + ry)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx - k * rx),
                fnum(pdf_cy + ry),
                fnum(pdf_cx - rx),
                fnum(pdf_cy + k * ry),
                fnum(pdf_cx - rx),
                fnum(pdf_cy)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx - rx),
                fnum(pdf_cy - k * ry),
                fnum(pdf_cx - k * rx),
                fnum(pdf_cy - ry),
                fnum(pdf_cx),
                fnum(pdf_cy - ry)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx + k * rx),
                fnum(pdf_cy - ry),
                fnum(pdf_cx + rx),
                fnum(pdf_cy - k * ry),
                fnum(pdf_cx + rx),
                fnum(pdf_cy)
            ));
            s.push_str("h\nW n\n");
        }
        vb_render::encode::ClipDef::Polygon(pts) => {
            for (i, (pt_x, pt_y)) in pts.iter().enumerate() {
                let pdf_x = x + pt_x;
                let pdf_py = pdf_y(*pt_y);
                if i == 0 {
                    s.push_str(&format!("{} {} m\n", fnum(pdf_x), fnum(pdf_py)));
                } else {
                    s.push_str(&format!("{} {} l\n", fnum(pdf_x), fnum(pdf_py)));
                }
            }
            s.push_str("h\nW n\n");
        }
    }
}

/// F1: 线性渐变 → PDF axial shading 字典体。
/// 形状半径(圆角):>0.75px 才当作圆角,亚像素圆角(1px 细线)按矩形处理,
/// 避免为无视觉意义的小圆角多留一份遮罩。
fn shape_radius(item: &DrawItem) -> f64 {
    if item.ellipse {
        return 0.0;
    }
    let r = item.radii.iter().cloned().fold(0.0f64, f64::max);
    if r > 0.75 {
        r
    } else {
        0.0
    }
}

/// 把圆角/椭圆的覆盖度烘进渐变位图的 alpha 通道,替代 PDF 剪切路径。
///
/// Illustrator 把内容流里每个 `W n` 解释成「剪切蒙版」:逐元素自带蒙版正是
/// 用户最反感的体验。位图渐变本就栅格化,形状边缘直接烘进 alpha 即可,
/// 视觉等价(像素中心采样 + 1px 覆盖度软边)而零 `W n`。
fn bake_shape_mask(data: &mut [u8], gw: u32, gh: u32, w: f64, h: f64, radius: f64, ellipse: bool) {
    if !ellipse && radius <= 0.75 {
        return;
    }
    if w <= 0.0 || h <= 0.0 || gw == 0 || gh == 0 {
        return;
    }
    let (cx, cy) = (w / 2.0, h / 2.0);
    // 圆角矩形的内缩半宽/半高
    let ix = (cx - radius).max(0.0);
    let iy = (cy - radius).max(0.0);
    let target = (gw * gh) as usize;
    for gy in 0..gh {
        let py = (gy as f64 + 0.5) * h / gh as f64;
        for gx in 0..gw {
            let idx = (gy * gw + gx) as usize;
            if idx >= target {
                break;
            }
            let px = (gx as f64 + 0.5) * w / gw as f64;
            // 有符号距离(px,内部为负)
            let dist = if ellipse {
                let nx = (px - cx) / cx.max(1e-6);
                let ny = (py - cy) / cy.max(1e-6);
                (nx * nx + ny * ny).sqrt() * cx.min(cy) - cx.min(cy)
            } else {
                let dx = ((px - cx).abs() - ix).max(0.0);
                let dy = ((py - cy).abs() - iy).max(0.0);
                (dx * dx + dy * dy).sqrt() - radius
            };
            // 1px 软边近似抗锯齿
            let cov = (0.5 - dist).clamp(0.0, 1.0);
            if cov >= 1.0 {
                continue;
            }
            let a = idx * 4 + 3;
            if a < data.len() {
                data[a] = (data[a] as f64 * cov).round() as u8;
            }
        }
    }
}

/// 渐变栅格化网格尺寸:长边封顶 256(位图拉伸 + 渲染器平滑,与逐像素
/// 插值的差异低于评分阈值),短边至少 2。
fn gradient_grid_size(w: f64, h: f64) -> (u32, u32) {
    let max = w.max(h).max(1.0);
    if max <= 256.0 {
        (w.max(2.0) as u32, h.max(2.0) as u32)
    } else {
        let s = 256.0 / max;
        (
            ((w * s).ceil() as u32).max(2),
            ((h * s).ceil() as u32).max(2),
        )
    }
}

/// 取 stop 序列在位置 t(0..1,Pad 展开)处的颜色并写入 RGBA 字节
/// (alpha = stop alpha × 项不透明度,对齐 CPU 栅格 to_skia_stops)。
fn push_stop_color(
    data: &mut Vec<u8>,
    stops: &[vb_render::encode::GradientStop],
    t: f32,
    opacity: f32,
) {
    let t = t.clamp(0.0, 1.0);
    let first = &stops[0];
    let last = &stops[stops.len() - 1];
    let c = if t <= first.pos {
        first.color
    } else if t >= last.pos {
        last.color
    } else {
        let mut c = last.color;
        for pair in stops.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if t >= a.pos && t <= b.pos {
                let span = (b.pos - a.pos).max(1e-6);
                let k = (t - a.pos) / span;
                // RGBA 四通道全插值(alpha 漏插曾让半透明渐变整体变不透明)
                for i in 0..4 {
                    c[i] = a.color[i] + (b.color[i] - a.color[i]) * k;
                }
                break;
            }
        }
        c
    };
    data.push((c[0].clamp(0.0, 1.0) * 255.0) as u8);
    data.push((c[1].clamp(0.0, 1.0) * 255.0) as u8);
    data.push((c[2].clamp(0.0, 1.0) * 255.0) as u8);
    data.push((c[3].clamp(0.0, 1.0) * opacity.clamp(0.0, 1.0) * 255.0) as u8);
}

/// 线性渐变 → RGB 位图。数学与 vb_render::cpu::gradient_line +
/// tiny-skia Pad 展开完全一致(CSS 角度,Y 向下局部坐标)。
fn gradient_bitmap_linear(
    angle_css: f64,
    stops: &[vb_render::encode::GradientStop],
    w: f64,
    h: f64,
    opacity: f32,
) -> (Vec<u8>, u32, u32) {
    let (gw, gh) = gradient_grid_size(w, h);
    let rad = angle_css.to_radians();
    let dx = rad.sin() as f32;
    let dy = -(rad.cos()) as f32;
    let l = (w as f32 * dx.abs()) + (h as f32 * dy.abs());
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let sx = cx - dx * l / 2.0;
    let sy = cy - dy * l / 2.0;
    let mut data = Vec::with_capacity(gw as usize * gh as usize * 4);
    for gy in 0..gh {
        let py = (gy as f64 + 0.5) * h / gh as f64;
        for gx in 0..gw {
            let px = (gx as f64 + 0.5) * w / gw as f64;
            let t = if l.abs() < 1e-6 {
                0.0
            } else {
                ((px as f32 - sx) * dx + (py as f32 - sy) * dy) / l
            };
            push_stop_color(&mut data, stops, t, opacity);
        }
    }
    (data, gw, gh)
}

/// 径向渐变 → RGB 位图。圆心为项内比例坐标,半径 = √(w²+h²)/2,
/// 与 vb_render::cpu 径向分支一致。
fn gradient_bitmap_radial(
    cxf: f32,
    cyf: f32,
    stops: &[vb_render::encode::GradientStop],
    w: f64,
    h: f64,
    opacity: f32,
) -> (Vec<u8>, u32, u32) {
    let (gw, gh) = gradient_grid_size(w, h);
    let ccx = (w * cxf as f64) as f32;
    let ccy = (h * cyf as f64) as f32;
    let radius = (((w * w + h * h).sqrt()) / 2.0) as f32;
    let mut data = Vec::with_capacity(gw as usize * gh as usize * 4);
    for gy in 0..gh {
        let py = (gy as f64 + 0.5) * h / gh as f64;
        for gx in 0..gw {
            let px = (gx as f64 + 0.5) * w / gw as f64;
            let d = ((px as f32 - ccx).powi(2) + (py as f32 - ccy).powi(2)).sqrt();
            push_stop_color(&mut data, stops, d / radius.max(1e-6), opacity);
        }
    }
    (data, gw, gh)
}

fn path_ops(s: &mut String, item: &DrawItem, x: f64, py: f64, w: f64, h: f64) {
    if item.ellipse {
        ellipse_path(s, x, py, w, h);
    } else if item.radii.iter().any(|r| *r > 0.0) {
        let r = item.radii[0].clamp(0.0, w.min(h) / 2.0);
        let k = 0.5523;
        // 圆角矩形:m + 4×(l|c) + h;每 c 恰 6 坐标(两控制点 + 终点)。
        // 控制点公式 = 角圆心 + k*r 切向(S1 修复:曾写 x+w-k*r,
        // 等效 k 随 r 增大被压扁,r=w/2 的正圆变成棱圆)
        // 右上:角心 (x+w-r, py+r) → 起 (x+w-r,py) 终 (x+w,py+r)
        let c_tr1 = (x + w - r + k * r, py);
        let c_tr2 = (x + w, py + r - k * r);
        // 右下:角心 (x+w-r, py+h-r) → 起 (x+w,py+h-r) 终 (x+w-r,py+h)
        let c_br1 = (x + w, py + h - r + k * r);
        let c_br2 = (x + w - r + k * r, py + h);
        // 左下:角心 (x+r, py+h-r) → 起 (x+r,py+h) 终 (x,py+h-r)
        let c_bl1 = (x + r - k * r, py + h);
        let c_bl2 = (x, py + h - r + k * r);
        // 左上:角心 (x+r, py+r) → 起 (x,py+r) 终 (x+r,py)
        let c_tl1 = (x, py + r - k * r);
        let c_tl2 = (x + r - k * r, py);
        s.push_str(&format!(
            "{} {} m {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c h\n",
            fnum(x + r), fnum(py),                                     // m 起点
            fnum(x + w - r), fnum(py),                                 // l 顶边
            fnum(c_tr1.0), fnum(c_tr1.1), fnum(c_tr2.0), fnum(c_tr2.1), fnum(x + w), fnum(py + r),   // c 右上
            fnum(x + w), fnum(py + h - r),                             // l 右边
            fnum(c_br1.0), fnum(c_br1.1), fnum(c_br2.0), fnum(c_br2.1), fnum(x + w - r), fnum(py + h), // c 右下
            fnum(x + r), fnum(py + h),                                 // l 底边
            fnum(c_bl1.0), fnum(c_bl1.1), fnum(c_bl2.0), fnum(c_bl2.1), fnum(x), fnum(py + h - r), // c 左下
            fnum(x), fnum(py + r),                                     // l 左边
            fnum(c_tl1.0), fnum(c_tl1.1), fnum(c_tl2.0), fnum(c_tl2.1), fnum(x + r), fnum(py)      // c 左上
        ));
    } else {
        s.push_str(&format!(
            "{} {} {} {} re\n",
            fnum(x),
            fnum(py),
            fnum(w),
            fnum(h)
        ));
    }
}

fn ellipse_path(s: &mut String, x: f64, py: f64, w: f64, h: f64) {
    let k = 0.5523;
    let (cx, cy) = (x + w / 2.0, py + h / 2.0);
    let (rx, ry) = (w / 2.0, h / 2.0);
    // 椭圆:m 右点 + 4×c(每 c 恰 6 坐标)
    s.push_str(&format!(
        "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c h\n",
        fnum(cx + rx), fnum(cy),                                                          // m 右点
        fnum(cx + rx), fnum(cy + k * ry), fnum(cx + k * rx), fnum(cy + ry), fnum(cx), fnum(cy + ry),  // c 上
        fnum(cx - k * rx), fnum(cy + ry), fnum(cx - rx), fnum(cy + k * ry), fnum(cx - rx), fnum(cy),  // c 左
        fnum(cx - rx), fnum(cy - k * ry), fnum(cx - k * rx), fnum(cy - ry), fnum(cx), fnum(cy - ry),  // c 下
        fnum(cx + k * rx), fnum(cy - ry), fnum(cx + rx), fnum(cy - k * ry), fnum(cx + rx), fnum(cy)   // c 右
    ));
}
/// 非 WinAnsi 字符降级(智能引号映射,其余 → '?';计数进 report)。
/// 文本 → 字形轮廓 PDF 路径(画板本地坐标,已含 Y 翻转)。
///
/// 用 vb_render::text 的 fontique+swash 管线:shape_text 得到字形与
/// advance,glyph_outline 取轮廓,Y 翻转后以 f 填充。与 CPU 光栅
/// 同一整形源,PDF 内视觉与 PNG 一致。
#[allow(unused_variables)]
#[allow(clippy::too_many_arguments)]
fn outline_text_pdf(
    s: &mut String,
    text: &str,
    font_family: &str,
    font_size: f64,
    weight: u16,
    x: f64,
    y: f64,
    box_h: f64,
    page_h: f64,
    letter_spacing: f64,
    idx_base: usize,
) {
    let Some(run) =
        vb_render::text::shape_text_weighted(text, font_family, font_size as f32, weight)
    else {
        return;
    };
    // 基线:盒顶 + 半行距 + ascent(浏览器 normal line-height 1.14 语义)
    let half_lead = 0.0 * font_size;
    let baseline_pdf = page_h - (y + half_lead + run.ascent as f64);
    for (i, g) in run.glyphs.iter().enumerate() {
        if let Some(path) = vb_render::text::glyph_outline_weighted(
            &run.font_data,
            run.font_index,
            font_size as f32,
            g.id,
            weight,
        ) {
            // 字距与 CPU 栅格同式:字形自然 x + 行内字形序 × ls
            let gx = x + g.x as f64 + (idx_base + i) as f64 * letter_spacing;
            s.push_str("q\n");
            s.push_str(&format!("1 0 0 1 {} {} cm\n", fnum(gx), fnum(baseline_pdf)));
            s.push_str(&kurbo_path_ops_pdf(&path));
            s.push_str("f\nQ\n");
        }
    }
}

/// kurbo BezPath -> PDF path 操作符(Y 翻转:字形坐标画布向下 -> PDF 向上)。
fn kurbo_path_ops_pdf(path: &vb_common::geom::BezPath) -> String {
    use vb_common::geom::PathEl;
    let mut out = String::with_capacity(256);
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                out.push_str(&format!("{} {} m\n", fnum(p.x), fnum(-p.y)));
            }
            PathEl::LineTo(p) => {
                out.push_str(&format!("{} {} l\n", fnum(p.x), fnum(-p.y)));
            }
            PathEl::QuadTo(c, p) => {
                // PDF 无二次贝塞尔:升为三次
                // c1 = p0 + 2/3(c-p0), c2 = p1 + 2/3(c-p1);此处以 c,p 近似
                let c1x = c.x * 2.0 / 3.0 + p.x / 3.0;
                let c1y = c.y * 2.0 / 3.0 + p.y / 3.0;
                let c2x = p.x * 2.0 / 3.0 + c.x / 3.0;
                let c2y = p.y * 2.0 / 3.0 + c.y / 3.0;
                out.push_str(&format!(
                    "{} {} {} {} {} {} c\n",
                    fnum(c1x),
                    fnum(-c1y),
                    fnum(c2x),
                    fnum(-c2y),
                    fnum(p.x),
                    fnum(-p.y)
                ));
            }
            PathEl::CurveTo(c1, c2, p) => {
                out.push_str(&format!(
                    "{} {} {} {} {} {} c\n",
                    fnum(c1.x),
                    fnum(-c1.y),
                    fnum(c2.x),
                    fnum(-c2.y),
                    fnum(p.x),
                    fnum(-p.y)
                ));
            }
            PathEl::ClosePath => out.push_str("h\n"),
        }
    }
    out
}
pub fn winansi_escaped(t: &str) -> String {
    t.chars()
        .map(|c| {
            let cp = c as u32;
            if (0x20..0x7f).contains(&cp) || (0xa0..0xff).contains(&cp) {
                c
            } else if c == '\u{201C}' || c == '\u{201D}' {
                '"'
            } else if c == '\u{2018}' || c == '\u{2019}' {
                '\''
            } else if c == '\u{2014}' {
                '-'
            } else if c == '\u{2026}' {
                '.'
            } else {
                '?'
            }
        })
        .collect()
}

pub fn escape_pdf_string(t: &str) -> String {
    t.replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

/// f32/f64 统一数值格式化(消双侧调用点的类型摩擦)。
pub trait FnumVal {
    fn val(self) -> f64;
}
impl FnumVal for f64 {
    fn val(self) -> f64 {
        self
    }
}
impl FnumVal for f32 {
    fn val(self) -> f64 {
        self as f64
    }
}

pub fn fnum<V: FnumVal>(v: V) -> String {
    let r = (v.val() * 1000.0).round() / 1000.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

/// F1: 线性渐变 → PDF axial shading 字典体。
/// 角度为 CSS 语义(0=to top, 90=to right, 180=to bottom),坐标已 Y 翻转。
///
/// 保留但默认不走:实测 pdfium(Acrobat/Chrome 同源内核)不渲染 `sh` 与
/// `PatternType 2`,对拍会变成空白;Illustrator 对非常规 shading 组合报
/// 「未知的阴影类型」。当前渐变一律走栅格位图(见 `gradient_bitmap_*`),
/// 本函数留作后续「矢量渐变可选项」的落点(design/20 §7 carry-forward)。
#[allow(dead_code)]
fn build_axial_shading(
    angle_css: f64,
    stops: &[vb_render::encode::GradientStop],
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    page_h: f64,
) -> String {
    let rad = angle_css.to_radians();
    // CSS 渐变方向向量(屏幕坐标 Y-down): (sin θ, -cos θ)
    // PDF 坐标系 (Y-up): (sin θ, cos θ)
    let dx = rad.sin();
    let dy = rad.cos();
    let cx = x + w / 2.0;
    let cy_pdf = page_h - (y + h / 2.0);
    // 渐变线长度(覆盖整个盒子)
    let len = (w * dx.abs() + h * dy.abs()).max(1.0);
    let x0 = cx - dx * len / 2.0;
    let y0 = cy_pdf - dy * len / 2.0;
    let x1 = cx + dx * len / 2.0;
    let y1 = cy_pdf + dy * len / 2.0;

    if stops.len() == 2 {
        let c0 = &stops[0].color;
        let c1 = &stops[1].color;
        format!(
            "/ShadingType 2 /ColorSpace /DeviceRGB /Coords [{} {} {} {}] \
             /Function << /FunctionType 2 /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >> \
             /Extend [true true]",
            fnum(x0),
            fnum(y0),
            fnum(x1),
            fnum(y1),
            fnum(c0[0] as f64),
            fnum(c0[1] as f64),
            fnum(c0[2] as f64),
            fnum(c1[0] as f64),
            fnum(c1[1] as f64),
            fnum(c1[2] as f64),
        )
    } else {
        // 多 stop: FunctionType 3 stitching
        let mut funcs = Vec::new();
        let mut bounds = Vec::new();
        let mut encode = Vec::new();
        for w in stops.windows(2) {
            let c0 = &w[0].color;
            let c1 = &w[1].color;
            funcs.push(format!(
                "<< /FunctionType 2 /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >>",
                fnum(c0[0] as f64),
                fnum(c0[1] as f64),
                fnum(c0[2] as f64),
                fnum(c1[0] as f64),
                fnum(c1[1] as f64),
                fnum(c1[2] as f64),
            ));
            if w[0].pos > 0.0 && w[0].pos < 1.0 {
                bounds.push(fnum(w[0].pos as f64));
            }
            encode.push("0 1".to_string());
        }
        let coords = format!("[{} {} {} {}]", fnum(x0), fnum(y0), fnum(x1), fnum(y1));
        format!(
            "/ShadingType 2 /ColorSpace /DeviceRGB /Coords {} \
             /Function << /FunctionType 3 /Domain [0 1] /Functions [{}] /Bounds [{}] /Encode [{}] >> \
             /Extend [true true]",
            coords, funcs.join(" "), bounds.join(" "), encode.join(" ")
        )
    }
}

/// F1: 径向渐变 → PDF radial shading 字典体(同 [`build_axial_shading`]:保留待用)。
#[allow(dead_code)]
fn build_radial_shading(
    stops: &[vb_render::encode::GradientStop],
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    page_h: f64,
) -> String {
    let cx = x + w / 2.0;
    let cy_pdf = page_h - (y + h / 2.0);
    let r = (w.max(h) / 2.0).max(1.0);
    let coords = format!(
        "[{} {} 0 {} {} {}]",
        fnum(cx),
        fnum(cy_pdf),
        fnum(cx),
        fnum(cy_pdf),
        fnum(r)
    );

    if stops.len() == 2 {
        let c0 = &stops[0].color;
        let c1 = &stops[1].color;
        format!(
            "/ShadingType 3 /ColorSpace /DeviceRGB /Coords {} \
             /Function << /FunctionType 2 /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >> \
             /Extend [true true]",
            coords,
            fnum(c0[0] as f64),
            fnum(c0[1] as f64),
            fnum(c0[2] as f64),
            fnum(c1[0] as f64),
            fnum(c1[1] as f64),
            fnum(c1[2] as f64),
        )
    } else {
        // 多 stop: stitching function
        let mut funcs = Vec::new();
        let mut bounds = Vec::new();
        let mut encode = Vec::new();
        for w in stops.windows(2) {
            let c0 = &w[0].color;
            let c1 = &w[1].color;
            funcs.push(format!(
                "<< /FunctionType 2 /C0 [{} {} {}] /C1 [{} {} {}] /N 1 >>",
                fnum(c0[0] as f64),
                fnum(c0[1] as f64),
                fnum(c0[2] as f64),
                fnum(c1[0] as f64),
                fnum(c1[1] as f64),
                fnum(c1[2] as f64),
            ));
            if w[0].pos > 0.0 && w[0].pos < 1.0 {
                bounds.push(fnum(w[0].pos as f64));
            }
            encode.push("0 1".to_string());
        }
        format!(
            "/ShadingType 3 /ColorSpace /DeviceRGB /Coords {} \
             /Function << /FunctionType 3 /Domain [0 1] /Functions [{}] /Bounds [{}] /Encode [{}] >> \
             /Extend [true true]",
            coords, funcs.join(" "), bounds.join(" "), encode.join(" ")
        )
    }
}

/// 按输入 PDF 的 xref 表精确切出每个对象体。
///
/// 早期版本按 `endstream`/`endobj` 关键字流式扫描切分:Flate 二进制流里
/// 出现同名字节即误切,合并产物 pdfium 直接报 Data format error(多画板
/// 只剩 1 页)。xref 是权威偏移,按它切片不会出错。
fn split_pdf_objects(pdf: &[u8]) -> KilnResult<std::collections::BTreeMap<u32, Vec<u8>>> {
    use std::collections::BTreeMap;
    let err = |m: &str| KilnError::Encode(format!("页合并解析失败: {m}"));
    let sx = pdf
        .windows(9)
        .rposition(|w| w == &b"startxref"[..])
        .ok_or_else(|| err("无 startxref"))?;
    let tail = String::from_utf8_lossy(&pdf[sx + 9..]).to_string();
    let xref_at: usize = tail
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| err("startxref 非法"))?;
    if xref_at >= pdf.len() || !pdf[xref_at..].starts_with(b"xref") {
        return Err(err("非经典 xref 表"));
    }
    let mut p = xref_at;
    let nl = |from: usize| -> Option<usize> {
        pdf[from..]
            .iter()
            .position(|&c| c == b'\n')
            .map(|v| from + v)
    };
    p = nl(p).ok_or_else(|| err("xref 头未结束"))? + 1;
    let mut entries: Vec<(u32, usize)> = Vec::new();
    loop {
        if pdf[p..].starts_with(b"trailer") {
            break;
        }
        let he = nl(p).ok_or_else(|| err("xref 子段头未结束"))?;
        let header = String::from_utf8_lossy(&pdf[p..he]).to_string();
        let mut it = header.split_whitespace();
        let start: u32 = it
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| err("子段头非法"))?;
        let count: u32 = it
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| err("子段头非法"))?;
        p = he + 1;
        for i in 0..count {
            if p + 20 > pdf.len() {
                break;
            }
            let ent = &pdf[p..p + 20];
            if ent[17] == b'n' {
                let off: usize = String::from_utf8_lossy(&ent[..10])
                    .trim()
                    .parse()
                    .unwrap_or(0);
                entries.push((start + i, off));
            }
            p += 20;
        }
    }
    entries.sort_by_key(|(_, off)| *off);
    let mut out: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for i in 0..entries.len() {
        let (id, off) = entries[i];
        let end = if i + 1 < entries.len() {
            entries[i + 1].1
        } else {
            xref_at
        };
        if off >= end || end > pdf.len() {
            continue;
        }
        let chunk = &pdf[off..end];
        let Some(ob) = chunk.windows(3).position(|w| w == &b"obj"[..]) else {
            continue;
        };
        let mut b0 = ob + 3;
        while b0 < chunk.len() && matches!(chunk[b0], b'\n' | b'\r' | b' ') {
            b0 += 1;
        }
        let mut b1 = chunk.len();
        if let Some(rel) = chunk.windows(6).rposition(|w| w == &b"endobj"[..]) {
            b1 = rel;
        }
        while b1 > b0 && matches!(chunk[b1 - 1], b'\n' | b'\r' | b' ') {
            b1 -= 1;
        }
        out.insert(id, chunk[b0..b1].to_vec());
    }
    if out.is_empty() {
        return Err(err("对象为空"));
    }
    Ok(out)
}

/// 页合并器(design/20 N4):把 N 个单页 PDF(本写入器产物,结构已知)
/// 合并为多页 PDF。各页字体/图像对象独立保留(文件略大,正确性优先;
/// 跨页资源去重列 carry-forward)。用于 AI 多画板(Illustrator:页=画板)。
pub fn merge_pdf_pages(pages: &[Vec<u8>]) -> KilnResult<Vec<u8>> {
    merge_pdf_pages_head(pages, "")
}

/// 同 [`merge_pdf_pages`],并把 `head` 注释写入头部之内(见 [`write_pdf_head`])。
pub fn merge_pdf_pages_head(pages: &[Vec<u8>], head: &str) -> KilnResult<Vec<u8>> {
    use std::collections::BTreeMap;
    if pages.len() <= 1 {
        return Ok(pages.first().cloned().unwrap_or_default());
    }
    // 每页:解析对象 {id: bytes},记录页面对象 id 与 MediaBox
    let mut all_objs: Vec<BTreeMap<u32, Vec<u8>>> = Vec::new();
    let mut page_obj_ids: Vec<u32> = Vec::new();
    let mut offsets: Vec<u32> = Vec::new(); // 每输入的对象 id 偏移
                                            // 每页的 OCG(层)对象:local id + /Name 原始 token(多画板合并后按名去重，
                                            // 让整册只有「背景/内容」两层，而不是每页各出一套)
    let mut page_ocg_ids: Vec<Vec<u32>> = Vec::new();
    let mut page_ocg_names: Vec<Vec<String>> = Vec::new();
    for (pi, pdf) in pages.iter().enumerate() {
        let objs = split_pdf_objects(pdf)?;
        if std::env::var("KILN_DEBUG_MERGE").is_ok() {
            eprintln!(
                "[merge] 输入{pi}: {} 个对象, ids={:?}",
                objs.len(),
                objs.keys().take(6).collect::<Vec<_>>()
            );
        }
        if objs.is_empty() {
            return Err(KilnError::Encode("页合并:输入 PDF 对象解析为空".into()));
        }
        // 页面对象 = /Type /Page 且非 /Pages
        let pid = objs
            .iter()
            .find(|(_, body)| {
                let b = String::from_utf8_lossy(body);
                b.contains("/Type /Page") && !b.contains("/Type /Pages")
            })
            .map(|(id, _)| *id)
            .ok_or_else(|| KilnError::Encode("页合并:未找到页面对象".into()))?;
        page_obj_ids.push(pid);
        // 本页 OCG 清单:Catalog 的 /OCGs [a 0 R b 0 R ...]
        let mut ocg_local: Vec<u32> = Vec::new();
        if let Some((_, cat)) = objs
            .iter()
            .find(|(_, b)| String::from_utf8_lossy(b).contains("/Type /Catalog"))
        {
            let cat = String::from_utf8_lossy(cat).to_string();
            if let Some(s) = cat.find("/OCGs [") {
                let rest = &cat[s + "/OCGs [".len()..];
                if let Some(e) = rest.find(']') {
                    ocg_local = parse_ref_list(&rest[..e]);
                }
            }
        }
        let mut ocg_names: Vec<String> = Vec::new();
        for id in &ocg_local {
            let name = objs
                .get(id)
                .map(|b| String::from_utf8_lossy(b).to_string())
                .and_then(|b| extract_name_token(&b))
                .unwrap_or_default();
            ocg_names.push(name);
        }
        page_ocg_ids.push(ocg_local);
        page_ocg_names.push(ocg_names);
        offsets.push(if pi == 0 { 0 } else { 0 }); // 先占位,稍后累计
        all_objs.push(objs);
    }
    // 偏移:前 i 个输入的最大对象号累计
    let mut acc = 0u32;
    for (i, objs) in all_objs.iter().enumerate() {
        offsets[i] = acc;
        let max_id = objs.keys().next_back().copied().unwrap_or(0);
        acc += max_id;
    }
    // 重编号 + 引用改写:只改 dict 段(到 stream 关键字为止)——
    // Flate 流是二进制,整 body 扫描会把随机字节误当引用破坏流
    // OCG 去重:按 /Name 首次出现者为准(整册合并为一套「背景/内容」)
    let ocg_global = |pi: usize, k: usize| -> (u32, String) {
        let global = page_ocg_ids[pi][k] + offsets[pi];
        let name = page_ocg_names
            .get(pi)
            .and_then(|v| v.get(k))
            .cloned()
            .unwrap_or_default();
        (global, name)
    };
    let mut name_to_global: BTreeMap<String, u32> = BTreeMap::new();
    for pi in 0..page_ocg_ids.len() {
        for k in 0..page_ocg_ids[pi].len() {
            let (global, name) = ocg_global(pi, k);
            name_to_global.entry(name).or_insert(global);
        }
    }
    let mut canonical_ocgs: Vec<u32> = Vec::new();
    for pi in 0..page_ocg_ids.len() {
        for k in 0..page_ocg_ids[pi].len() {
            let (global, name) = ocg_global(pi, k);
            let canon = name_to_global.get(&name).copied().unwrap_or(global);
            if !canonical_ocgs.contains(&canon) {
                canonical_ocgs.push(canon);
            }
        }
    }

    let mut out_objs: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for (pi, objs) in all_objs.iter().enumerate() {
        let off = offsets[pi];
        for (id, body) in objs {
            let new_id = id + off;
            let mut rewritten = rewrite_obj_body(body, off);
            // 页面 /Properties 的 OCG 引用改指整册唯一的那一套
            let is_page = {
                let head = head_of(&rewritten);
                head.contains("/Type /Page") && !head.contains("/Type /Pages")
            };
            if is_page {
                let map: Vec<(u32, u32)> = page_ocg_ids[pi]
                    .iter()
                    .enumerate()
                    .map(|(k, id)| {
                        let global = id + off;
                        let name = page_ocg_names
                            .get(pi)
                            .and_then(|v| v.get(k))
                            .cloned()
                            .unwrap_or_default();
                        (global, name_to_global.get(&name).copied().unwrap_or(global))
                    })
                    .filter(|(a, b)| a != b)
                    .collect();
                if !map.is_empty() {
                    remap_properties_ocg(&mut rewritten, &map);
                }
            }
            out_objs.insert(new_id, rewritten);
        }
    }
    // 新 Catalog(1)与 Pages(2);原输入的 Catalog/Pages 对象弃用
    let total = out_objs.keys().next_back().copied().unwrap_or(2);
    let kids: Vec<String> = page_obj_ids
        .iter()
        .enumerate()
        .map(|(pi, id)| format!("{} 0 R", id + offsets[pi]))
        .collect();
    if std::env::var("KILN_DEBUG_MERGE").is_ok() {
        eprintln!(
            "[merge] page_obj_ids={page_obj_ids:?} offsets={offsets:?} kids={kids:?} out_objs={} 个",
            out_objs.len()
        );
    }
    let pages_obj = format!(
        "/Type /Pages /Kids [{}] /Count {}",
        kids.join(" "),
        pages.len()
    );
    // Catalog 保留 OCProperties:整册共用一套 OCG(背景/内容),多画板也
    // 只有两个图层,而不是「每页各一套双图层」
    let mut catalog_obj = String::from("/Type /Catalog /Pages 2 0 R");
    if !canonical_ocgs.is_empty() {
        let refs: Vec<String> = canonical_ocgs.iter().map(|i| format!("{i} 0 R")).collect();
        let list = refs.join(" ");
        catalog_obj.push_str(&format!(
            " /OCProperties << /OCGs [{list}] /D << /ON [{list}] /Order [{list}] >> >>"
        ));
    }
    // 页面对象的 /Parent 全部改指 2(原指各自 Pages,已随偏移错位——统一改写)
    // 合并写入器按「对象体原样」写盘,不像 Obj::Dict 那样自动包 `<< >>`——
    // 漏掉时 /Root 不是字典,qpdf/pdfium 直接拒绝(pikepdf: unable to find
    // /Root dictionary;pdfium: Data format error)
    out_objs.insert(1, format!("<< {catalog_obj} >>").into_bytes());
    out_objs.insert(2, format!("<< {pages_obj} >>").into_bytes());

    // 组装输出
    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    out.extend_from_slice(head.as_bytes());
    let mut xref_offsets: BTreeMap<u32, u64> = BTreeMap::new();
    for (id, body) in &out_objs {
        xref_offsets.insert(*id, out.len() as u64);
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let max_id = out_objs.keys().next_back().copied().unwrap_or(2);
    let startxref = out.len() as u64;
    out.extend_from_slice(format!("xref\n0 {}\n", max_id + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for id in 1..=max_id {
        match xref_offsets.get(&id) {
            Some(off) => out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    // trailer 必须是字典:少了 `<< >>` 时 qpdf/pdfium 报「expected trailer
    // dictionary / Data format error」——多画板文件直接打不开
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{startxref}\n%%EOF\n",
            max_id + 1
        )
        .as_bytes(),
    );
    Ok(out)
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// 对象体的 dict 段(到 stream 关键字为止)。
fn head_of(body: &[u8]) -> String {
    let end = find_sub(body, b"stream").unwrap_or(body.len());
    String::from_utf8_lossy(&body[..end]).to_string()
}

/// 解析 `/OCGs [a 0 R b 0 R]` 花括号内的引用列表。
fn parse_ref_list(inner: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let bytes = inner.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if inner[i..].starts_with(" 0 R") {
                if let Ok(n) = inner[start..i].parse::<u32>() {
                    out.push(n);
                }
                i += 4;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// 取 OCG 对象的 `/Name` 原始 token(含括号/尖括号),用于按名去重。
fn extract_name_token(body: &str) -> Option<String> {
    let i = body.find("/Name ")? + "/Name ".len();
    let rest = &body[i..];
    let first = rest.chars().next()?;
    match first {
        '(' => rest.find(')').map(|e| rest[..=e].to_string()),
        '<' => rest.find('>').map(|e| rest[..=e].to_string()),
        _ => {
            let end = rest
                .find(|c: char| c == '/' || c == '>' || c.is_whitespace())
                .unwrap_or(rest.len());
            Some(rest[..end].to_string())
        }
    }
}

/// 页面 /Properties 字典里的 OCG 引用改指整册唯一对象。
fn remap_properties_ocg(body: &mut Vec<u8>, map: &[(u32, u32)]) {
    let head_end = find_sub(body, b"stream").unwrap_or(body.len());
    let head = String::from_utf8_lossy(&body[..head_end]).to_string();
    let Some(ps) = head.find("/Properties") else {
        return;
    };
    let Some(open_rel) = head[ps..].find("<<") else {
        return;
    };
    let open = ps + open_rel;
    let Some(close_rel) = head[open..].find(">>") else {
        return;
    };
    let close = open + close_rel + 2;
    let mut section = head[open..close].to_string();
    for (from, to) in map {
        section = section.replace(&format!("{from} 0 R"), &format!("{to} 0 R"));
    }
    let new_head = format!("{}{}{}", &head[..open], section, &head[close..]);
    let mut out = new_head.into_bytes();
    if head_end < body.len() {
        out.extend_from_slice(&body[head_end..]);
    }
    *body = out;
}

/// 对象体引用改写:dict 段(至 stream 止)加偏移;流数据原样保留。
/// 页面对象的 /Parent 一律改指新 Pages(对象 2)。
fn rewrite_obj_body(body: &[u8], off: u32) -> Vec<u8> {
    let stream_at = find_sub(body, b"stream");
    let head_end = match stream_at {
        Some(i) => i,
        None => body.len(),
    };
    let head = String::from_utf8_lossy(&body[..head_end]).to_string();
    let is_page = head.contains("/Type /Page") && !head.contains("/Type /Pages");
    let mut head_rw = String::with_capacity(head.len());
    let bytes = head.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let num = &head[start..i];
            if head[i..].starts_with(" 0 R") {
                let n: u32 = num.parse().unwrap_or(0);
                head_rw.push_str(&format!("{} 0 R", n + off));
                i += 4;
            } else {
                head_rw.push_str(num);
            }
        } else {
            head_rw.push(bytes[i] as char);
            i += 1;
        }
    }
    if is_page {
        // /Parent <旧> 0 R → /Parent 2 0 R(改写后的值可能已偏移,二次替换)
        if let Some(p0) = head_rw.find("/Parent ") {
            if let Some(rel) = head_rw[p0..].find("0 R") {
                let seg_end = p0 + rel + 3;
                head_rw.replace_range(p0..seg_end, "/Parent 2 0 R");
            }
        }
    }
    let mut out = head_rw.into_bytes();
    if let Some(i) = stream_at {
        out.extend_from_slice(&body[i..]);
    }
    out
}
