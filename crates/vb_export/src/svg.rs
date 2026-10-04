//! SVG 原生导出:DrawList → SVG(真实文本;渐变/圆角/旋转/透明度)。
//!
//! 文本以真实 `<text>` 输出(HTML 是源格式,SVG 同理,ADR-0010)。
//! 坐标精度与 PDF 车道同源 `vb_common::numfmt::fnum`(EXP-07):f64
//! 最短往返的浮点噪声(`123.45000000000002`)不再进入产物。

use std::fmt::Write as _;

use vb_common::numfmt::fnum;
use vb_render::encode::{DrawItem, DrawKind, DrawList, FillDef};

pub fn render_svg(list: &DrawList, scale: u32, transparent: bool) -> String {
    let w = list.w * scale as f64;
    let h = list.h * scale as f64;
    let mut out = String::with_capacity(64 * 1024);
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
        fnum(w),
        fnum(h),
        fnum(w),
        fnum(h)
    );
    let _ = writeln!(out, "<defs>");

    // 渐变收集到 defs。SVG 渐变默认 objectBoundingBox(坐标按包围盒比例
    // 解释),这里写的是像素,必须显式 userSpaceOnUse;坐标乘 scale 并加
    // 节点偏移,与 CPU/GPU 端的 shader 坐标同帧。
    let mut defs = String::new();
    let s = scale as f64;
    for (i, item) in list.items.iter().enumerate() {
        if let Some(FillDef::LinearGradient { angle_css, stops }) = &item.fill {
            let [x, y, iw, ih] = item.rect;
            let (sp, ep) = vb_render::cpu::gradient_line(*angle_css, iw, ih);
            let (sx, sy) = ((x + sp.x as f64) * s, (y + sp.y as f64) * s);
            let (ex, ey) = ((x + ep.x as f64) * s, (y + ep.y as f64) * s);
            let _ = write!(
                defs,
                r#"  <linearGradient id="g{i}" gradientUnits="userSpaceOnUse" x1="{}" y1="{}" x2="{}" y2="{}">"#,
                fnum(sx),
                fnum(sy),
                fnum(ex),
                fnum(ey)
            );
            defs.push('\n');
            for st in stops {
                let (r, g, b) = to_255(st.color);
                let _ = write!(
                    defs,
                    r#"    <stop offset="{}" stop-color="rgb({r},{g},{b})" stop-opacity="{}"/>"#,
                    fnum(st.pos),
                    fnum(st.color[3])
                );
                defs.push('\n');
            }
            let _ = writeln!(defs, "  </linearGradient>");
        }
        if let Some(FillDef::RadialGradient { cx, cy, stops }) = &item.fill {
            let [x, y, iw, ih] = item.rect;
            let ccx = (x + iw * *cx as f64) * s;
            let ccy = (y + ih * *cy as f64) * s;
            // 引擎两端(CPU/GPU)的半径公式:sqrt(w²+h²)/2
            let r = (iw * iw + ih * ih).sqrt() / 2.0 * s;
            let _ = write!(
                defs,
                r#"  <radialGradient id="g{i}" gradientUnits="userSpaceOnUse" cx="{}" cy="{}" r="{}">"#,
                fnum(ccx),
                fnum(ccy),
                fnum(r)
            );
            defs.push('\n');
            for st in stops {
                let (r2, g2, b2) = to_255(st.color);
                let _ = write!(
                    defs,
                    r#"    <stop offset="{}" stop-color="rgb({r2},{g2},{b2})" stop-opacity="{}"/>"#,
                    fnum(st.pos),
                    fnum(st.color[3])
                );
                defs.push('\n');
            }
            let _ = writeln!(defs, "  </radialGradient>");
        }
    }
    out.push_str(&defs);
    out.push_str("</defs>\n");

    if !transparent && (!list.background.iter().all(|c| *c >= 0.999) || list.background[3] < 1.0) {
        // 画板有自定义背景色时画底色矩形(透明导出跳过)
        let (r, g, b) = to_255(list.background);
        let _ = writeln!(
            out,
            r#"<rect x="0" y="0" width="{}" height="{}" fill="rgb({r},{g},{b})"/>"#,
            fnum(w),
            fnum(h)
        );
    }

    // 05-2(09-B 剪切蒙版):overflow 裁剪组 → `<g clip-path>` 分组;
    // 裁剪矩形(编码期已按嵌套交集折叠)收集进尾部的 `<clipPath>` defs。
    let mut clip_defs = String::new();
    let mut clip_ids: Vec<[f64; 4]> = Vec::new();
    let mut open_clip: Option<usize> = None;
    for (i, item) in list.items.iter().enumerate() {
        let cid = item
            .overflow_clip
            .filter(|[_, _, cw, ch]| *cw > 0.0 && *ch > 0.0)
            .map(|rect| {
                if let Some(pos) = clip_ids.iter().position(|r| *r == rect) {
                    return pos;
                }
                let id = clip_ids.len();
                let s = scale as f64;
                clip_defs.push_str(&format!(
                    r#"  <clipPath id="oc{id}"><rect x="{}" y="{}" width="{}" height="{}"/></clipPath>"#,
                    fnum(rect[0] * s),
                    fnum(rect[1] * s),
                    fnum(rect[2] * s),
                    fnum(rect[3] * s),
                ));
                clip_defs.push('\n');
                clip_ids.push(rect);
                id
            });
        if cid != open_clip {
            if open_clip.is_some() {
                out.push_str("</g>\n");
            }
            if let Some(id) = cid {
                let _ = writeln!(out, r#"<g clip-path="url(#oc{id})">"#);
                open_clip = cid;
            } else {
                open_clip = None;
            }
        }
        write_item(&mut out, i, item, scale as f64);
    }
    if open_clip.is_some() {
        out.push_str("</g>\n");
    }
    if !clip_defs.is_empty() {
        // 渐变 defs 已闭合:clipPath 以独立 defs 追加(SVG 合法且等价)
        let _ = writeln!(out, "<defs>\n{clip_defs}</defs>");
    }

    out.push_str("</svg>\n");
    out
}

fn to_255(c: [f32; 4]) -> (u32, u32, u32) {
    (
        (c[0] * 255.0).round() as u32,
        (c[1] * 255.0).round() as u32,
        (c[2] * 255.0).round() as u32,
    )
}

fn fill_attr(item: &DrawItem, i: usize) -> String {
    match &item.fill {
        Some(FillDef::Solid(c)) => {
            let (r, g, b) = to_255(*c);
            format!(
                r#"fill="rgb({r},{g},{b})" fill-opacity="{}""#,
                fnum(c[3] * item.opacity)
            )
        }
        Some(FillDef::LinearGradient { .. }) | Some(FillDef::RadialGradient { .. }) => {
            // 节点 opacity:渐变端此前完全不输出,CPU/GPU 端都乘进色标
            format!(r#"fill="url(#g{i})" fill-opacity="{}""#, fnum(item.opacity))
        }
        None => r#"fill="none""#.into(),
    }
}

fn write_item(out: &mut String, i: usize, item: &DrawItem, scale: f64) {
    let [x, y, w, h] = item.rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let s = scale;
    // 几何一次性乘 scale;不得再加 scale() transform(此前两者叠加,
    // scale≠1 时内容被放大 s²,只看得到左上角一块)
    let (x, y, w, h) = (x * s, y * s, w * s, h * s);

    // 旋转(绕缩放后的中心,与几何同帧)
    let mut tf = String::new();
    if item.rot.abs() > 1e-9 {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        tf = format!(
            r#" transform="rotate({} {} {})""#,
            fnum(item.rot),
            fnum(cx),
            fnum(cy)
        );
    }

    // P4 矢量路径 → <path d>(坐标统一 fnum,EXP-07)
    if let Some(kpath) = &item.path {
        let mut d = String::new();
        for el in kpath.elements() {
            use vb_common::geom::PathEl;
            match el {
                PathEl::MoveTo(p) => {
                    let _ = write!(
                        d,
                        "M{} {} ",
                        fnum((p.x + item.rect[0]) * scale),
                        fnum((p.y + item.rect[1]) * scale)
                    );
                }
                PathEl::LineTo(p) => {
                    let _ = write!(
                        d,
                        "L{} {} ",
                        fnum((p.x + item.rect[0]) * scale),
                        fnum((p.y + item.rect[1]) * scale)
                    );
                }
                PathEl::QuadTo(c, p) => {
                    let _ = write!(
                        d,
                        "Q{} {} {} {} ",
                        fnum((c.x + item.rect[0]) * scale),
                        fnum((c.y + item.rect[1]) * scale),
                        fnum((p.x + item.rect[0]) * scale),
                        fnum((p.y + item.rect[1]) * scale)
                    );
                }
                PathEl::CurveTo(c1, c2, p) => {
                    let _ = write!(
                        d,
                        "C{} {} {} {} {} {} ",
                        fnum((c1.x + item.rect[0]) * scale),
                        fnum((c1.y + item.rect[1]) * scale),
                        fnum((c2.x + item.rect[0]) * scale),
                        fnum((c2.y + item.rect[1]) * scale),
                        fnum((p.x + item.rect[0]) * scale),
                        fnum((p.y + item.rect[1]) * scale)
                    );
                }
                PathEl::ClosePath => d.push('Z'),
            }
        }
        let fa = fill_attr(item, i);
        // 描边色/宽与 CPU/GPU 端一致取 border(此前硬编码深灰)
        let stroke = match &item.border {
            Some(b) => {
                let (r, g, b2) = to_255(b.color);
                format!(
                    r#"stroke="rgb({r},{g},{b2})" stroke-width="{}" stroke-opacity="{}""#,
                    fnum(b.width.max(1.0)),
                    fnum(b.color[3] * item.opacity)
                )
            }
            None => r#"stroke="none""#.to_string(),
        };
        let _ = writeln!(out, r#"<path d="{d}" {fa} {stroke}{tf}/>"#);
        return;
    }
    #[allow(unreachable_patterns)]
    match item.kind {
        DrawKind::VectorPath | DrawKind::Text => {
            if let Some(t) = &item.label {
                let line_h = (if t.line_height > 0.0 {
                    t.line_height
                } else {
                    t.font_size * 1.32
                }) * s;
                let ls = t.letter_spacing * s;
                let max_w = (item.rect[2] * s).max(1.0) as f32;
                let seg_ranges: Vec<(usize, usize)> =
                    t.segments.iter().map(|sg| (sg.start, sg.end)).collect();
                let (br, bg2, bb) = to_255(t.color);
                vb_render::text::for_each_visual_line(
                    &t.text,
                    &t.font_family,
                    t.font_size as f32,
                    t.weight,
                    max_w,
                    ls as f32,
                    |vi, hard, run, line, byte_base| {
                        let baseline = y + run.ascent as f64 * s + vi as f64 * line_h;
                        // font-family 必须落盘:此前只有字号/字重,设计字体
                        // 全部丢成查看器默认字体(度量按原字体整形,换默认
                        // 字体后行宽错位)
                        let _ = write!(
                            out,
                            r#"<text x="{}" y="{}" font-size="{}" font-weight="{}" letter-spacing="{}" font-family="{}" fill="rgb({br},{bg2},{bb})" fill-opacity="{}"{tf}>"#,
                            fnum(x),
                            fnum(baseline),
                            fnum(t.font_size * s),
                            t.weight,
                            fnum(ls),
                            escape_xml(&t.font_family),
                            fnum(t.color[3] * item.opacity),
                        );
                        for part in vb_render::text::split_line_segments(
                            hard,
                            line,
                            run,
                            byte_base,
                            &seg_ranges,
                            ls as f32,
                        ) {
                            match part.seg.and_then(|i| t.segments.get(i)) {
                                Some(sg) => {
                                    let (r, g, b) = sg.color.map(to_255).unwrap_or((br, bg2, bb));
                                    let fw = sg
                                        .bold
                                        .map(|b| if b { 700 } else { 400 })
                                        .unwrap_or(t.weight);
                                    let fs = sg.font_size.map(|f| f * s).unwrap_or(t.font_size * s);
                                    let _ = write!(
                                        out,
                                        r#"<tspan fill="rgb({r},{g},{b})" font-weight="{fw}" font-size="{}">{}</tspan>"#,
                                        fnum(fs),
                                        escape_xml(part.text),
                                    );
                                }
                                None => out.push_str(&escape_xml(part.text)),
                            }
                        }
                        out.push_str("</text>");
                        out.push('\n');
                    },
                );
            }
        }
        DrawKind::FrozenPlaceholder => {
            let _ = write!(
                out,
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="8" fill="rgb(217,212,204)" fill-opacity="{}"{tf}/><!-- 冻结块:原 HTML 保留于 index.html -->"#,
                fnum(x),
                fnum(y),
                fnum(w),
                fnum(h),
                fnum(item.opacity)
            );
            out.push('\n');
        }
        DrawKind::Image => {
            if let Some(bmp) = &item.image {
                // 位图以 data URL 嵌入(B3):导出的独立 SVG 不再丢图;
                // PNG 载荷由原始 RGBA 现场编码(导出一次,不逐帧)
                if let Some(href) = bitmap_data_url(bmp) {
                    let _ = write!(
                        out,
                        r#"<image x="{}" y="{}" width="{}" height="{}" opacity="{}" preserveAspectRatio="none" href="{href}"{tf}/><!-- image: {:?} -->"#,
                        fnum(x),
                        fnum(y),
                        fnum(w),
                        fnum(h),
                        fnum(item.opacity),
                        item.src
                    );
                    out.push('\n');
                    return;
                }
            }
            let _ = write!(
                out,
                r#"<rect x="{}" y="{}" width="{}" height="{}" fill="none" stroke="rgb(230,77,77)" stroke-width="2"{tf}/><!-- image missing: {:?} -->"#,
                fnum(x),
                fnum(y),
                fnum(w),
                fnum(h),
                item.src
            );
            out.push('\n');
        }
        DrawKind::Box => {
            let fa = fill_attr(item, i);
            let radius = if item.ellipse {
                String::new()
            } else {
                // 先对未缩放尺寸取 clamp 再乘 scale(此前对已缩放尺寸二次
                // 乘 s,圆角被放大 s²)
                let r = item.radii[0].min(item.rect[2].min(item.rect[3]) / 2.0) * s;
                format!(r#" rx="{}""#, fnum(r))
            };
            if item.ellipse {
                let _ = write!(
                    out,
                    r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" {fa}{tf}/>"#,
                    fnum(x + w / 2.0),
                    fnum(y + h / 2.0),
                    fnum(w / 2.0),
                    fnum(h / 2.0)
                );
            } else if item.radii.iter().any(|r| *r > 0.0)
                && (item.radii[0] != item.radii[1]
                    || item.radii[1] != item.radii[2]
                    || item.radii[2] != item.radii[3])
            {
                // 四角异径:rect 的 rx 单值表达不了,与 CPU 端同构发 path
                let d = rounded_rect_path(x, y, w, h, item.radii, s);
                let _ = write!(out, r#"<path d="{d}" {fa}{tf}/>"#);
            } else {
                let _ = write!(
                    out,
                    r#"<rect x="{}" y="{}" width="{}" height="{}"{radius} {fa}{tf}/>"#,
                    fnum(x),
                    fnum(y),
                    fnum(w),
                    fnum(h)
                );
            }
            out.push('\n');
            if let Some(b) = &item.border {
                let (r, g, b2) = to_255(b.color);
                let bw = b.width * s;
                if item.ellipse {
                    let _ = write!(
                        out,
                        r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" fill="none" stroke="rgb({r},{g},{b2})" stroke-width="{}" stroke-opacity="{}"{tf}/>"#,
                        fnum(x + w / 2.0),
                        fnum(y + h / 2.0),
                        fnum((w - bw) / 2.0),
                        fnum((h - bw) / 2.0),
                        fnum(bw),
                        fnum(b.color[3] * item.opacity)
                    );
                } else {
                    let rr = item.radii[0].min(item.rect[2].min(item.rect[3]) / 2.0) * s;
                    let _ = write!(
                        out,
                        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}" fill="none" stroke="rgb({r},{g},{b2})" stroke-width="{}" stroke-opacity="{}"{tf}/>"#,
                        fnum(x + bw / 2.0),
                        fnum(y + bw / 2.0),
                        fnum(w - bw),
                        fnum(h - bw),
                        fnum(rr),
                        fnum(bw),
                        fnum(b.color[3] * item.opacity)
                    );
                }
                out.push('\n');
            }
        }
    }
}

/// RGBA 位图 → `data:image/png;base64,…`(导出一次性成本)。
fn bitmap_data_url(bmp: &vb_render::encode::BitmapData) -> Option<String> {
    use base64::Engine as _;
    let img = image::RgbaImage::from_raw(bmp.width, bmp.height, (*bmp.rgba).clone())?;
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png.get_ref());
    Some(format!("data:image/png;base64,{b64}"))
}

/// 四角异径圆角矩形 → SVG path d 串(与 cpu.rs rect_path 同一几何:
/// kappa 0.5522848 控制点;坐标已缩放)。
fn rounded_rect_path(x: f64, y: f64, w: f64, h: f64, radii: [f64; 4], s: f64) -> String {
    const KAPPA: f64 = 0.552_284_8;
    let clamp = |r: f64| r.min(w.min(h) / 2.0);
    let tl = clamp(radii[0]) * s;
    let tr = clamp(radii[1]) * s;
    let br = clamp(radii[2]) * s;
    let bl = clamp(radii[3]) * s;
    let mut d = String::new();
    let _ = write!(d, "M{} {} ", fnum(x + tl), fnum(y));
    let _ = write!(d, "L{} {} ", fnum(x + w - tr), fnum(y));
    if tr > 0.0 {
        let k = KAPPA * tr;
        let _ = write!(
            d,
            "C{} {} {} {} {} {} ",
            fnum(x + w - tr + k),
            fnum(y),
            fnum(x + w),
            fnum(y + tr - k),
            fnum(x + w),
            fnum(y + tr)
        );
    }
    let _ = write!(d, "L{} {} ", fnum(x + w), fnum(y + h - br));
    if br > 0.0 {
        let k = KAPPA * br;
        let _ = write!(
            d,
            "C{} {} {} {} {} {} ",
            fnum(x + w),
            fnum(y + h - br + k),
            fnum(x + w - br + k),
            fnum(y + h),
            fnum(x + w - br),
            fnum(y + h)
        );
    }
    let _ = write!(d, "L{} {} ", fnum(x + bl), fnum(y + h));
    if bl > 0.0 {
        let k = KAPPA * bl;
        let _ = write!(
            d,
            "C{} {} {} {} {} {} ",
            fnum(x + bl - k),
            fnum(y + h),
            fnum(x),
            fnum(y + h - bl + k),
            fnum(x),
            fnum(y + h - bl)
        );
    }
    let _ = write!(d, "L{} {} ", fnum(x), fnum(y + tl));
    if tl > 0.0 {
        let k = KAPPA * tl;
        let _ = write!(
            d,
            "C{} {} {} {} {} {} ",
            fnum(x),
            fnum(y + tl - k),
            fnum(x + tl - k),
            fnum(y),
            fnum(x + tl),
            fnum(y)
        );
    }
    d.push('Z');
    d
}

/// XML 转义(EXP-08):补双引号/单引号 —— `font-family="{}"` 等属性以
/// 双引号包裹,含 `"` 的字体名此前直接破坏 XML 结构;文本内容同理需要
/// 完整五元转义,不能只顾 `<` `>`。
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EXP-08:含双引号的属性值(font-family 等)不得破坏 XML 结构。
    #[test]
    fn escape_xml_handles_quotes() {
        assert_eq!(
            escape_xml(r#"Heiti "SC", 'Ming'"#),
            "Heiti &quot;SC&quot;, &apos;Ming&apos;"
        );
        assert_eq!(escape_xml(r#"<a & b>"#), "&lt;a &amp; b&gt;");
    }

    /// EXP-07:SVG 坐标与 PDF fnum 同精度(无浮点噪声尾巴)。
    #[test]
    fn fnum_matches_pdf_precision() {
        assert_eq!(fnum(123.45000000000002), "123.45");
        assert_eq!(fnum(0.1 + 0.2), "0.3");
        assert_eq!(fnum(100.0), "100");
    }
}
