//! PostScript 家族:EPS 3.0 + Ai(PDF 兼容流 + Illustrator 头)。
//!
//! EPS:DOCN 头 + BoundingBox/HiResBoundingBox + Level 2 操作符,
//! 文本 show(印刷可编辑)。
//! Ai:.ai 公开格式即 PDF(artboard ADR-0008 实测结论)—— 写 PDF 兼容流
//! + Adobe 私有注释头;不伪造 PGF 私有数据流(仅 Illustrator 能写)。

use vb_render::encode::{DrawItem, DrawKind, FillDef};

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::pdf::{escape_pdf_string, fnum, winansi_escaped, write_pdf};

/// EPS 3.0 字节流。
pub fn write_eps(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    let w = ctx.logical_w;
    let h = ctx.logical_h;
    let (w_r, h_r) = (fnum(w), fnum(h));
    let mut s = String::with_capacity(32 * 1024);
    s.push_str("%!PS-Adobe-3.0 EPSF-3.0\n");
    s.push_str("%%Creator: Kiln/VellumBench\n");
    s.push_str(&format!(
        "%%Title: ({})\n",
        escape_pdf_string(&ctx.artboard_name)
    ));
    s.push_str(&format!(
        "%%BoundingBox: 0 0 {} {}\n",
        w.ceil() as i64,
        h.ceil() as i64
    ));
    s.push_str(&format!("%%HiResBoundingBox: 0 0 {w_r} {h_r}\n"));
    s.push_str("%%DocumentData: Clean7Bit\n");
    s.push_str("%%LanguageLevel: 2\n");
    s.push_str("%%Pages: 1\n");
    s.push_str("%%EndComments\n");
    s.push_str("%%Page: 1 1\n");
    s.push_str("gsave\n");
    if !ctx.transparent {
        let bg = ctx.list.background;
        s.push_str(&format!(
            "{} {} {} setrgbcolor newpath 0 0 moveto {w_r} 0 lineto {w_r} {h_r} lineto 0 {h_r} lineto closepath fill\n",
            fnum(bg[0] as f64), fnum(bg[1] as f64), fnum(bg[2] as f64)
        ));
    }
    for item in &ctx.list.items {
        draw_item_ps(&mut s, item, h);
    }
    s.push_str("grestore\nshowpage\n%%EOF\n");
    Ok(s.into_bytes())
}

/// Ai:PDF 兼容流 + Illustrator 私有头。
pub fn write_ai(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    let mut bytes = write_pdf(
        ctx,
        "Adobe Illustrator(R) 24.0 (Kiln compatible PDF stream)",
    )?;
    // Illustrator 兼容注释(插在 PDF 头二进制注释行前,不影响解析)
    let head: &[u8] = b"%AI9_PrivateDataBegin\n%%AI8_CreatorVersion: 24.0.0\n%AI5_FileFormat 9.0\n";
    if bytes.starts_with(b"%PDF-1.7\n") {
        let mut with_head = Vec::with_capacity(bytes.len() + head.len());
        with_head.extend_from_slice(&bytes[..9]);
        with_head.extend_from_slice(head);
        with_head.extend_from_slice(&bytes[9..]);
        bytes = with_head;
    }
    Ok(bytes)
}

/// 文本 -> PS 字形轮廓(PS 用户空间 Y 向上;字形路径 Y 向下取负写出)。
#[allow(unused_variables)]
#[allow(clippy::too_many_arguments)]
fn outline_text_ps(
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
    let baseline_ps = page_h - (y + half_lead + run.ascent as f64);
    for g in &run.glyphs {
        if let Some(path) =
            vb_render::text::glyph_outline(&run.font_data, run.font_index, font_size as f32, g.id)
        {
            s.push_str("gsave\n");
            s.push_str(&format!(
                "{} {} translate\n",
                fnum(x + g.x as f64),
                fnum(baseline_ps)
            ));
            s.push_str(&kurbo_path_ops_ps(&path));
            s.push_str("fill\ngrestore\n");
        }
    }
}

/// kurbo BezPath -> PS path 操作符(Y 取负)。
fn kurbo_path_ops_ps(path: &vb_common::geom::BezPath) -> String {
    use vb_common::geom::PathEl;
    let mut out = String::with_capacity(256);
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                out.push_str(&format!("{} {} moveto\n", fnum(p.x), fnum(-p.y)));
            }
            PathEl::LineTo(p) => {
                out.push_str(&format!("{} {} lineto\n", fnum(p.x), fnum(-p.y)));
            }
            PathEl::QuadTo(c, p) => {
                let c1x = c.x * 2.0 / 3.0 + p.x / 3.0;
                let c1y = c.y * 2.0 / 3.0 + p.y / 3.0;
                let c2x = p.x * 2.0 / 3.0 + c.x / 3.0;
                let c2y = p.y * 2.0 / 3.0 + c.y / 3.0;
                out.push_str(&format!(
                    "{} {} {} {} {} {} curveto\n",
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
                    "{} {} {} {} {} {} curveto\n",
                    fnum(c1.x),
                    fnum(-c1.y),
                    fnum(c2.x),
                    fnum(-c2.y),
                    fnum(p.x),
                    fnum(-p.y)
                ));
            }
            PathEl::ClosePath => out.push_str("closepath\n"),
        }
    }
    out
}
fn draw_item_ps(s: &mut String, item: &DrawItem, page_h: f64) {
    let [x, y, w, h] = item.rect;
    let py = page_h - y - h;
    s.push_str("gsave\n");
    if item.rot.abs() > 1e-9 {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let pcy = page_h - cy;
        s.push_str(&format!(
            "{} {} translate {} rotate {} {} translate\n",
            fnum(cx),
            fnum(pcy),
            fnum(-item.rot),
            fnum(-cx),
            fnum(cy)
        ));
    }
    if let Some(fill) = &item.fill {
        match fill {
            FillDef::Solid(c) => {
                s.push_str(&format!(
                    "{} {} {} setrgbcolor\n",
                    fnum(c[0] as f64),
                    fnum(c[1] as f64),
                    fnum(c[2] as f64)
                ));
                ps_path(s, item, x, py, w, h);
                s.push_str("fill\n");
            }
            FillDef::LinearGradient { stops, .. } | FillDef::RadialGradient { stops, .. } => {
                let mid = stops.get(stops.len() / 2).or_else(|| stops.first());
                if let Some(st) = mid {
                    s.push_str(&format!(
                        "{} {} {} setrgbcolor\n",
                        fnum(st.color[0] as f64),
                        fnum(st.color[1] as f64),
                        fnum(st.color[2] as f64)
                    ));
                    ps_path(s, item, x, py, w, h);
                    s.push_str("fill\n");
                }
            }
        }
    }
    if let Some(border) = &item.border {
        s.push_str(&format!(
            "{} {} {} setrgbcolor {} setlinewidth\n",
            fnum(border.color[0] as f64),
            fnum(border.color[1] as f64),
            fnum(border.color[2] as f64),
            fnum(border.width)
        ));
        ps_path(s, item, x, py, w, h);
        s.push_str("stroke\n");
    }
    if let Some(label) = &item.label {
        let c = label.color;
        s.push_str(&format!(
            "{} {} {} setrgbcolor\n",
            fnum(c[0] as f64),
            fnum(c[1] as f64),
            fnum(c[2] as f64)
        ));
        let has_cjk = label.text.chars().any(|ch| {
            let cp = ch as u32;
            !(0x20..0x7f).contains(&cp) && !(0xa0..0xff).contains(&cp)
        });
        if has_cjk {
            // CJK:字形轮廓矢量填充(与 PDF 分支同源 swash 整形)
            outline_text_ps(
                s,
                &label.text,
                &label.font_family,
                label.font_size,
                x,
                y,
                h,
                page_h,
            );
        } else {
            let font = if label.weight_bold {
                "Helvetica-Bold"
            } else {
                "Helvetica"
            };
            s.push_str(&format!(
                "/{font} findfont {} scalefont setfont\n",
                fnum(label.font_size)
            ));
            let baseline = page_h - (y + h * 0.78);
            s.push_str(&format!("{} {} moveto\n", fnum(x), fnum(baseline)));
            s.push_str(&format!(
                "({}) show\n",
                escape_pdf_string(&winansi_escaped(&label.text))
            ));
        }
    }
    if item.kind == DrawKind::Image {
        s.push_str("0.8 0.8 0.8 setrgbcolor 0.6 setlinewidth newpath\n");
        s.push_str(&format!(
            "{} {} moveto {} 0 rlineto 0 {} rlineto {} 0 rlineto closepath stroke\n",
            fnum(x),
            fnum(py),
            fnum(w),
            fnum(h),
            fnum(-w)
        ));
    }
    s.push_str("grestore\n");
}

fn ps_path(s: &mut String, item: &DrawItem, x: f64, py: f64, w: f64, h: f64) {
    s.push_str("newpath\n");
    if item.ellipse {
        ellipse_path(s, x, py, w, h);
    } else if item.radii.iter().any(|r| *r > 0.0) {
        let r = item.radii[0].clamp(0.0, w.min(h) / 2.0);
        let k = 0.5523;
        // 圆角矩形:moveto/lineto/curveto(每 curveto 恰 6 坐标)
        s.push_str(&format!(
            "{} {} moveto {} {} lineto {} {} {} {} {} {} curveto {} {} lineto {} {} {} {} {} {} curveto {} {} lineto {} {} {} {} {} {} curveto {} {} lineto {} {} {} {} {} {} curveto closepath\n",
            fnum(x + r), fnum(py),
            fnum(x + w - r), fnum(py),
            fnum(x + w - k * r), fnum(py), fnum(x + w), fnum(py + k * r), fnum(x + w), fnum(py + r),
            fnum(x + w), fnum(py + h - r),
            fnum(x + w), fnum(py + h - k * r), fnum(x + w - k * r), fnum(py + h), fnum(x + w - r), fnum(py + h),
            fnum(x + r), fnum(py + h),
            fnum(x + k * r), fnum(py + h), fnum(x), fnum(py + h - k * r), fnum(x), fnum(py + h - r),
            fnum(x), fnum(py + r),
            fnum(x), fnum(py + k * r), fnum(x + k * r), fnum(py), fnum(x + r), fnum(py)
        ));
    } else {
        s.push_str(&format!(
            "{} {} moveto {} 0 rlineto 0 {} rlineto {} 0 rlineto closepath\n",
            fnum(x),
            fnum(py),
            fnum(w),
            fnum(h),
            fnum(-w)
        ));
    }
}

fn ellipse_path(s: &mut String, x: f64, py: f64, w: f64, h: f64) {
    let k = 0.5523;
    let (cx, cy) = (x + w / 2.0, py + h / 2.0);
    let (rx, ry) = (w / 2.0, h / 2.0);
    s.push_str(&format!(
        "{} {} moveto {} {} {} {} {} {} curveto {} {} {} {} {} {} curveto {} {} {} {} {} {} curveto {} {} {} {} {} {} curveto closepath\n",
        fnum(cx + rx), fnum(cy),
        fnum(cx + rx), fnum(cy + k * ry), fnum(cx + k * rx), fnum(cy + ry), fnum(cx), fnum(cy + ry),
        fnum(cx - k * rx), fnum(cy + ry), fnum(cx - rx), fnum(cy + k * ry), fnum(cx - rx), fnum(cy),
        fnum(cx - rx), fnum(cy - k * ry), fnum(cx - k * rx), fnum(cy - ry), fnum(cx), fnum(cy - ry),
        fnum(cx + k * rx), fnum(cy - ry), fnum(cx + rx), fnum(cy - k * ry), fnum(cx + rx), fnum(cy)
    ));
}
