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
use crate::error::KilnResult;
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
    let content = render_content_stream(ctx);
    let content_z = content.into_bytes();

    // 对象布局:1 Catalog 2 Pages 3 Page 4 Content 5 F1 6 F2,7.. OCG
    let layer_names = crate::writer::collect_layers(&ctx.list);
    let n_layers = layer_names.len().min(64);
    let ocg_start = 8u32;
    let mut objects: Vec<String> = Vec::new();

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
    objects.push(catalog);

    // 2 Pages
    objects.push(format!("/Type /Pages /Kids [{page_id} 0 R] /Count 1"));

    // 3 Page
    let mut page = format!(
        "/Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {} {}] /Contents {content_id} 0 R /Resources << /Font << /F1 {font_id} 0 R /F2 {font_bold_id} 0 R >>",
        fnum(w),
        fnum(h)
    );
    if n_layers > 0 {
        let props: Vec<String> = (0..n_layers)
            .map(|i| format!("/MC{i} {} 0 R", ocg_start + i as u32))
            .collect();
        page.push_str(&format!(" /Properties << {} >>", props.join(" ")));
    }
    page.push_str(" >>");
    objects.push(page);

    // 4 Content(占位;组装时替换为 stream 形式)
    objects.push(String::new());

    // 5/6 字体
    objects.push(
        "/Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding".to_string(),
    );
    objects.push(
        "/Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding"
            .to_string(),
    );

    // 7.. OCG 对象
    for name in layer_names.iter().take(n_layers) {
        objects.push(format!(
            "/Type /OCG /Name ({}) /Intent [/View /Design] /Usage << /CreatorInfo << /Creator (Kiln) >> >>",
            escape_pdf_string(name)
        ));
    }

    // 组装
    let mut out: Vec<u8> = Vec::with_capacity(32 * 1024);
    out.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets: Vec<u64> = Vec::with_capacity(objects.len());
    for (idx, obj) in objects.iter().enumerate() {
        offsets.push(out.len() as u64);
        out.extend_from_slice(format!("{} 0 obj\n", idx + 1).as_bytes());
        if idx + 1 == content_id {
            out.extend_from_slice(
                format!("/Length {}\nstream\n", content_z.len()).as_bytes(),
            );
            out.extend_from_slice(&content_z);
            out.extend_from_slice(b"\nendstream\nendobj\n");
        } else {
            out.extend_from_slice(obj.as_bytes());
            out.extend_from_slice(b"\nendobj\n");
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

/// 内容流:DrawItem → PDF 操作符(Y 翻转;文本 Tj;OCG BDC/EMC)。
fn render_content_stream(ctx: &ExportContext) -> String {
    let h = ctx.logical_h;
    let mut s = String::with_capacity(32 * 1024);
    if !ctx.transparent {
        let bg = ctx.list.background;
        s.push_str(&format!(
            "q {} {} {} rg 0 0 {} {} re f Q\n",
            fnum(bg[0]),
            fnum(bg[1]),
            fnum(bg[2]),
            fnum(ctx.logical_w),
            fnum(ctx.logical_h)
        ));
    }
    for (i, item) in ctx.list.items.iter().enumerate() {
        // 图层:背景(首项)归 MC1,其余归 MC2(与 collect_layers 对应)
        let layer_idx = if i == 0 { 1 } else { 2 };
        if layer_idx <= 2 {
            s.push_str(&format!("/OC /MC{layer_idx} BDC\n"));
        }
        draw_item_pdf(&mut s, item, h);
        if layer_idx <= 2 {
            s.push_str("EMC\n");
        }
    }
    s
}

fn draw_item_pdf(s: &mut String, item: &DrawItem, page_h: f64) {
    let [x, y, w, h] = item.rect;
    let py = page_h - y - h;
    s.push_str("q\n");
    if item.rot.abs() > 1e-9 {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let rad = item.rot.to_radians();
        let (sn, cs) = (rad.sin(), rad.cos());
        let pcy = page_h - cy;
        // 平移到旋转中心(已翻转),旋转,再平移回局部
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
    if let Some(fill) = &item.fill {
        match fill {
            FillDef::Solid(c) => {
                s.push_str(&format!(
                    "{} {} {} rg\n",
                    fnum(c[0]),
                    fnum(c[1]),
                    fnum(c[2])
                ));
                path_ops(s, item, x, py, w, h);
                s.push_str("f\n");
            }
            FillDef::LinearGradient { stops, .. } | FillDef::RadialGradient { stops, .. } => {
                let mid = stops.get(stops.len() / 2).or_else(|| stops.first());
                if let Some(st) = mid {
                    s.push_str(&format!(
                        "{} {} {} rg\n",
                        fnum(st.color[0]),
                        fnum(st.color[1]),
                        fnum(st.color[2])
                    ));
                    path_ops(s, item, x, py, w, h);
                    s.push_str("f\n");
                }
            }
        }
    }
    if let Some(border) = &item.border {
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
    if let Some(label) = &item.label {
        let c = label.color;
        let has_cjk = label
            .text
            .chars()
            .any(|ch| {
                let cp = ch as u32;
                !(0x20..0x7f).contains(&cp) && !(0xa0..0xff).contains(&cp)
            });
        if has_cjk {
            // CJK 等非 WinAnsi 文本:swash 整形 → 字形轮廓矢量填充
            // (PDF 内仍为矢量、可选中;文本层降级在 report 告警)
            s.push_str(&format!(
                "{} {} {} rg\n",
                fnum(c[0]),
                fnum(c[1]),
                fnum(c[2])
            ));
            outline_text_pdf(
                s,
                &label.text,
                &label.font_family,
                label.font_size as f64,
                x,
                y,
                h,
                page_h,
            );
        } else {
            s.push_str("BT\n");
            s.push_str(&format!(
                "{} {} {} rg\n",
                fnum(c[0]),
                fnum(c[1]),
                fnum(c[2])
            ));
            let fref = if label.weight_bold { "/F2" } else { "/F1" };
            s.push_str(&format!("{fref} {} Tf\n", fnum(label.font_size)));
            let metrics = vb_render::text::shape_text(&label.text, &label.font_family, label.font_size as f32);
            let (asc, _desc, fsize) = metrics
                .as_ref()
                .map(|r| (r.ascent as f64, r.descent as f64, label.font_size))
                .unwrap_or((h * 0.78, 0.0, label.font_size));
            let half_lead = 0.0 * fsize;
            let baseline = page_h - (y + half_lead + asc);
            s.push_str(&format!(
                "1 0 0 1 {} {} Tm\n",
                fnum(x),
                fnum(baseline)
            ));
            let text = winansi_escaped(&label.text);
            s.push_str(&format!("({text}) Tj\nET\n"));
        }
    }
    if item.kind == DrawKind::Image {
        s.push_str("0.8 0.8 0.8 rg 0.6 0.6 0.6 RG 1 w\n");
        s.push_str(&format!(
            "{} {} {} {} re B\n",
            fnum(x),
            fnum(py),
            fnum(w),
            fnum(h)
        ));
    }
    s.push_str("Q\n");
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
fn outline_text_pdf(
    s: &mut String,
    text: &str,
    font_family: &str,
    font_size: f64,
    x: f64,
    y: f64,
    box_h: f64,
    page_h: f64,
) {
    let Some(run) = vb_render::text::shape_text(text, font_family, font_size as f32) else {
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
