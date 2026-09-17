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
        Ok(r)
    }
}

/// PDF 写入核心(Ai 格式复用,仅 Producer 元数据不同)。
pub fn write_pdf(ctx: &ExportContext, producer: &str) -> KilnResult<Vec<u8>> {
    let w = ctx.logical_w;
    let h = ctx.logical_h;

    // 内容流 + CJK 字体使用收集(M3:CID 真文本)
    let mut usage = CjkUsage::new();
    let _probe = render_content_stream(ctx, &mut usage, false);
    finalize_cjk_fonts(&mut usage);
    // 两遍渲染间清除图形资源(图像/透明度),防止 pass1+pass2 重复
    // 导致资源编号错位和对象数翻倍。pass2 重新收集(remap=true 时正式发射)。
    usage.images.clear();
    usage.opacities.clear();
    usage.im_counter = 0;
    usage.gs_counter = 0;
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
    // F3: ExtGState 资源
    if !usage.opacities.is_empty() {
        let gss: Vec<String> = usage
            .opacities
            .iter()
            .enumerate()
            .map(|(i, (res, _))| format!("/{} {} 0 R", res, gs_base_start + i as u32))
            .collect();
        page.push_str(&format!(" /ExtGState << {} >>", gss.join(" ")));
    }
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
        objects.push(Obj::Dict(format!(
            "/Type /ExtGState /ca {} /CA {}",
            fnum(*alpha),
            fnum(*alpha)
        )));
    }

    // 组装
    let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
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

/// 全部 CJK 字体使用。
#[derive(Default)]
pub struct CjkUsage {
    pub fonts: Vec<CjkFont>,
    /// 图像资源(F2):resource → (FlateDecode RGBA 数据, 宽, 高;A 通道发 SMask)
    pub images: Vec<(String, Vec<u8>, u32, u32)>,
    /// 透明度资源(F3):resource → alpha
    pub opacities: Vec<(String, f64)>,
    im_counter: u32,
    gs_counter: u32,
}

impl CjkUsage {
    pub fn new() -> Self {
        CjkUsage {
            fonts: Vec::new(),
            images: Vec::new(),
            opacities: Vec::new(),
            im_counter: 0,
            gs_counter: 0,
        }
    }

    fn image_for(&mut self, data: Vec<u8>, w: u32, h: u32) -> String {
        self.im_counter += 1;
        let res = format!("Im{}", self.im_counter);
        self.images.push((res.clone(), data, w, h));
        res
    }

    fn opacity_for(&mut self, alpha: f64) -> String {
        self.gs_counter += 1;
        let res = format!("GS{}", self.gs_counter);
        self.opacities.push((res.clone(), alpha));
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
        // 子集化(缩减文件体积);subsetter 的 GID 排序经 ToUnicode 交叉验证一致
        let gids: Vec<u16> = f.glyphs.keys().copied().collect();
        let remapper = subsetter::GlyphRemapper::new_from_glyphs_sorted(&gids);
        match subsetter::subset(&data, index as u32, &remapper) {
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
    for item in ctx.list.items.iter() {
        // OCG 逐项标记暂缓(qpdf/PDFium 对未注册 MC 资源的解析分歧会破坏
        // 文本提取;图层功能待 OCG 语义修证后重开,见流程表 M3.4)
        draw_item_pdf(&mut s, item, h, usage, remap);
    }
    s
}

fn draw_item_pdf(
    s: &mut String,
    item: &DrawItem,
    page_h: f64,
    usage: &mut CjkUsage,
    remap: bool,
) {
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
                    // pdfium 等渲染器不绘制 PDF shading:渐变按 CPU 栅格同款
                    // 数学降采样为位图嵌入并裁剪到项形状,任意渲染器结果一致。
                    let stops = apply_filter_stops(stops, item.filter.as_ref());
                    let (data, gw, gh) =
                        gradient_bitmap_linear(*angle_css, &stops, w, h, item.opacity);
                    let im_res = usage.image_for(data, gw, gh);
                    s.push_str("q\n");
                    path_ops(s, item, x, py, w, h);
                    s.push_str("W n\n");
                    s.push_str(&format!(
                        "{} 0 0 {} {} {} cm /{} Do\nQ\n",
                        fnum(w),
                        fnum(h),
                        fnum(x),
                        fnum(py),
                        im_res
                    ));
                }
                FillDef::RadialGradient {
                    cx: gcx,
                    cy: gcy,
                    stops,
                } => {
                    let stops = apply_filter_stops(stops, item.filter.as_ref());
                    let (data, gw, gh) =
                        gradient_bitmap_radial(*gcx, *gcy, &stops, w, h, item.opacity);
                    let im_res = usage.image_for(data, gw, gh);
                    s.push_str("q\n");
                    path_ops(s, item, x, py, w, h);
                    s.push_str("W n\n");
                    s.push_str(&format!(
                        "{} 0 0 {} {} {} cm /{} Do\nQ\n",
                        fnum(w),
                        fnum(h),
                        fnum(x),
                        fnum(py),
                        im_res
                    ));
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
        emit_gs(s, usage, item.opacity as f64 * label.color[3].clamp(0.0, 1.0) as f64);
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
            let resource = usage.resource_for(&label.font_family, label.weight);
            vb_render::text::for_each_visual_line(
                &label.text, &label.font_family, label.font_size as f32,
                label.weight, max_w, ls,
                |vi, hard, run, line, byte_base| {
                    usage.set_metrics(
                        &label.font_family, label.weight,
                        run.ascent as f64 / label.font_size.max(1.0) * 1000.0,
                        -(run.descent as f64) / label.font_size.max(1.0) * 1000.0,
                    );
                    let baseline_art = y + run.ascent as f64 + vi as f64 * line_h;
                    let parts = vb_render::text::split_line_segments(
                        hard, line, run, byte_base, &seg_ranges, ls,
                    );
                    s.push_str("BT\n");
                    s.push_str(&format!("/{resource} {} Tf\n", fnum(label.font_size)));
                    for part in parts {
                        let color = part
                            .seg
                            .and_then(|i| label.segments.get(i))
                            .and_then(|sg| sg.color)
                            .unwrap_or(label.color);
                        s.push_str(&format!(
                            "{} {} {} rg\n",
                            fnum(color[0]), fnum(color[1]), fnum(color[2])
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
                                        &label.font_family, label.weight,
                                        gid, ch,
                                        part.advances[k] as f64, label.font_size,
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
        } else if has_cjk {
            s.push_str(&format!(
                "{} {} {} rg\n",
                fnum(label.color[0]), fnum(label.color[1]), fnum(label.color[2])
            ));
            vb_render::text::for_each_visual_line(
                &label.text, &label.font_family, label.font_size as f32,
                label.weight, max_w, ls,
                |vi, hard, run, line, _bb| {
                    let s0: usize = hard.chars().take(line[0]).map(|ch| ch.len_utf8()).sum();
                    let last = *line.last().expect("nonempty");
                    let s1: usize = hard.chars().take(last + 1).map(|ch| ch.len_utf8()).sum();
                    let sub = &hard[s0..s1];
                    outline_text_pdf(
                        s, sub, &label.font_family, label.font_size, label.weight,
                        x, y + vi as f64 * line_h, h, page_h,
                    );
                    let _ = run;
                },
            );
        } else {
            let fref = if label.weight >= 600 { "/F2" } else { "/F1" };
            vb_render::text::for_each_visual_line(
                &label.text, &label.font_family, label.font_size as f32,
                label.weight, max_w, ls,
                |vi, hard, run, line, byte_base| {
                    let asc = run.ascent as f64;
                    let baseline = page_h - (y + asc + vi as f64 * line_h);
                    s.push_str("BT\n");
                    s.push_str(&format!("{fref} {} Tf\n", fnum(label.font_size)));
                    let parts = vb_render::text::split_line_segments(
                        hard, line, run, byte_base, &seg_ranges, ls,
                    );
                    for part in parts {
                        let color = part
                            .seg
                            .and_then(|i| label.segments.get(i))
                            .and_then(|sg| sg.color)
                            .unwrap_or(label.color);
                        s.push_str(&format!(
                            "{} {} {} rg\n",
                            fnum(color[0]), fnum(color[1]), fnum(color[2])
                        ));
                        s.push_str(&format!(
                            "1 0 0 1 {} {} Tm\n",
                            fnum(x + part.x), fnum(baseline)
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
            s.push_str(&format!(
                "q {} 0 0 {} {} {} cm /{} Do Q\n",
                fnum(w), fnum(h), fnum(x), fnum(py), im_res
            ));
        }
    }

    if need_q {
        s.push_str("Q\n");
    }
}

/// F3: 按需发射 ExtGState 透明度(alpha<1 才发;每次设置替换而非叠加)。
fn emit_gs(s: &mut String, usage: &mut CjkUsage, alpha: f64) {
    if (0.0..1.0).contains(&alpha) {
        let gs_res = usage.opacity_for(alpha.clamp(0.001, 0.999));
        s.push_str(&format!("/{} gs\n", gs_res));
    }
}

/// F5: 应用 brightness/saturate 调整到颜色。
fn apply_filter_color(
    mut c: [f32; 4],
    filter: Option<&vb_render::encode::FilterDef>,
) -> [f32; 4] {
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
            vb_render::encode::GradientStop { pos: s.pos, color: c }
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
                fnum(cl), fnum(cb), fnum(cw.max(0.0)), fnum(ch.max(0.0))
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
                fnum(pdf_cx + rx), fnum(pdf_cy + k * ry),
                fnum(pdf_cx + k * rx), fnum(pdf_cy + ry),
                fnum(pdf_cx), fnum(pdf_cy + ry)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx - k * rx), fnum(pdf_cy + ry),
                fnum(pdf_cx - rx), fnum(pdf_cy + k * ry),
                fnum(pdf_cx - rx), fnum(pdf_cy)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx - rx), fnum(pdf_cy - k * ry),
                fnum(pdf_cx - k * rx), fnum(pdf_cy - ry),
                fnum(pdf_cx), fnum(pdf_cy - ry)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx + k * rx), fnum(pdf_cy - ry),
                fnum(pdf_cx + rx), fnum(pdf_cy - k * ry),
                fnum(pdf_cx + rx), fnum(pdf_cy)
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
                fnum(pdf_cx + rx), fnum(pdf_cy + k * ry),
                fnum(pdf_cx + k * rx), fnum(pdf_cy + ry),
                fnum(pdf_cx), fnum(pdf_cy + ry)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx - k * rx), fnum(pdf_cy + ry),
                fnum(pdf_cx - rx), fnum(pdf_cy + k * ry),
                fnum(pdf_cx - rx), fnum(pdf_cy)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx - rx), fnum(pdf_cy - k * ry),
                fnum(pdf_cx - k * rx), fnum(pdf_cy - ry),
                fnum(pdf_cx), fnum(pdf_cy - ry)
            ));
            s.push_str(&format!(
                "{} {} {} {} {} {} c\n",
                fnum(pdf_cx + k * rx), fnum(pdf_cy - ry),
                fnum(pdf_cx + rx), fnum(pdf_cy - k * ry),
                fnum(pdf_cx + rx), fnum(pdf_cy)
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
        // 圆角矩形:m + 4×(l|c) + h;每个 c 恰 6 坐标(两控制点 + 终点)
        s.push_str(&format!(
            "{} {} m {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c {} {} l {} {} {} {} {} {} c h\n",
            fnum(x + r), fnum(py),                                                         // m 起点
            fnum(x + w - r), fnum(py),                                                     // l 顶边
            fnum(x + w - k * r), fnum(py), fnum(x + w), fnum(py + k * r), fnum(x + w), fnum(py + r), // c 右上
            fnum(x + w), fnum(py + h - r),                                                 // l 右边
            fnum(x + w), fnum(py + h - k * r), fnum(x + w - k * r), fnum(py + h), fnum(x + w - r), fnum(py + h), // c 右下
            fnum(x + r), fnum(py + h),                                                     // l 底边
            fnum(x + k * r), fnum(py + h), fnum(x), fnum(py + h - k * r), fnum(x), fnum(py + h - r), // c 左下
            fnum(x), fnum(py + r),                                                         // l 左边
            fnum(x), fnum(py + k * r), fnum(x + k * r), fnum(py), fnum(x + r), fnum(py)    // c 左上
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
) {
    let Some(run) =
        vb_render::text::shape_text_weighted(text, font_family, font_size as f32, weight)
    else {
        return;
    };
    // 基线:盒顶 + 半行距 + ascent(浏览器 normal line-height 1.14 语义)
    let half_lead = 0.0 * font_size;
    let baseline_pdf = page_h - (y + half_lead + run.ascent as f64);
    for g in &run.glyphs {
        if let Some(path) =
            vb_render::text::glyph_outline(&run.font_data, run.font_index, font_size as f32, g.id)
        {
            s.push_str("q\n");
            s.push_str(&format!(
                "1 0 0 1 {} {} cm\n",
                fnum(x + g.x as f64),
                fnum(baseline_pdf)
            ));
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
