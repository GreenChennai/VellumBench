//! SVG 原生导出:DrawList → SVG(真实文本;渐变/圆角/旋转/透明度)。
//!
//! 文本以真实 `<text>` 输出(HTML 是源格式,SVG 同理,ADR-0010)。

use std::fmt::Write as _;

use vb_render::encode::{DrawItem, DrawKind, DrawList, FillDef};

pub fn render_svg(list: &DrawList, scale: u32, transparent: bool) -> String {
    let w = list.w * scale as f64;
    let h = list.h * scale as f64;
    let mut out = String::with_capacity(64 * 1024);
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#
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
                r#"  <linearGradient id="g{i}" gradientUnits="userSpaceOnUse" x1="{sx}" y1="{sy}" x2="{ex}" y2="{ey}">"#
            );
            defs.push('\n');
            for st in stops {
                let (r, g, b) = to_255(st.color);
                let _ = write!(
                    defs,
                    r#"    <stop offset="{}" stop-color="rgb({r},{g},{b})" stop-opacity="{}"/>"#,
                    st.pos, st.color[3]
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
                r#"  <radialGradient id="g{i}" gradientUnits="userSpaceOnUse" cx="{ccx}" cy="{ccy}" r="{r}">"#
            );
            defs.push('\n');
            for st in stops {
                let (r2, g2, b2) = to_255(st.color);
                let _ = write!(
                    defs,
                    r#"    <stop offset="{}" stop-color="rgb({r2},{g2},{b2})" stop-opacity="{}"/>"#,
                    st.pos, st.color[3]
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
            r#"<rect x="0" y="0" width="{w}" height="{h}" fill="rgb({r},{g},{b})"/>"#
        );
    }

    for (i, item) in list.items.iter().enumerate() {
        write_item(&mut out, i, item, scale as f64);
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
                c[3] * item.opacity
            )
        }
        Some(FillDef::LinearGradient { .. }) | Some(FillDef::RadialGradient { .. }) => {
            // 节点 opacity:渐变端此前完全不输出,CPU/GPU 端都乘进色标
            format!(r#"fill="url(#g{i})" fill-opacity="{}""#, item.opacity)
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
        tf = format!(r#" transform="rotate({} {cx} {cy})""#, item.rot);
    }

    // P4 矢量路径 → <path d>
    if let Some(kpath) = &item.path {
        let mut d = String::new();
        for el in kpath.elements() {
            use vb_common::geom::PathEl;
            match el {
                PathEl::MoveTo(p) => {
                    let _ = write!(
                        d,
                        "M{} {} ",
                        (p.x + item.rect[0]) * scale,
                        (p.y + item.rect[1]) * scale
                    );
                }
                PathEl::LineTo(p) => {
                    let _ = write!(
                        d,
                        "L{} {} ",
                        (p.x + item.rect[0]) * scale,
                        (p.y + item.rect[1]) * scale
                    );
                }
                PathEl::QuadTo(c, p) => {
                    let _ = write!(
                        d,
                        "Q{} {} {} {} ",
                        (c.x + item.rect[0]) * scale,
                        (c.y + item.rect[1]) * scale,
                        (p.x + item.rect[0]) * scale,
                        (p.y + item.rect[1]) * scale
                    );
                }
                PathEl::CurveTo(c1, c2, p) => {
                    let _ = write!(
                        d,
                        "C{} {} {} {} {} {} ",
                        (c1.x + item.rect[0]) * scale,
                        (c1.y + item.rect[1]) * scale,
                        (c2.x + item.rect[0]) * scale,
                        (c2.y + item.rect[1]) * scale,
                        (p.x + item.rect[0]) * scale,
                        (p.y + item.rect[1]) * scale
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
                    b.width.max(1.0),
                    b.color[3] * item.opacity
                )
            }
            None => r#"stroke="none""#.to_string(),
        };
        let _ = writeln!(out, r#"<path d="{d}" {fa} {stroke}{tf}/>"#);
        return;
    }

    if item.kind == DrawKind::VectorPath {
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
                let max_w = (item.rect[2] * s as f64).max(1.0) as f32;
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
                        let _ = write!(
                            out,
                            r#"<text x="{x}" y="{baseline}" font-size="{}" font-weight="{}" letter-spacing="{ls}" fill="rgb({br},{bg2},{bb})" fill-opacity="{}"{tf}>"#,
                            t.font_size * s,
                            t.weight,
                            t.color[3] * item.opacity,
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
                                        r#"<tspan fill="rgb({r},{g},{b})" font-weight="{fw}" font-size="{fs}">{}</tspan>"#,
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
                r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="8" fill="rgb(217,212,204)" fill-opacity="{}"{tf}/><!-- 冻结块:原 HTML 保留于 index.html -->"#,
                item.opacity
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
                        r#"<image x="{x}" y="{y}" width="{w}" height="{h}" opacity="{}" preserveAspectRatio="none" href="{href}"{tf}/><!-- image: {:?} -->"#,
                        item.opacity, item.src
                    );
                    out.push('\n');
                    return;
                }
            }
            let _ = write!(
                out,
                r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none" stroke="rgb(230,77,77)" stroke-width="2"{tf}/><!-- image missing: {:?} -->"#,
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
                format!(r#" rx="{r}""#)
            };
            if item.ellipse {
                let _ = write!(
                    out,
                    r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" {fa}{tf}/>"#,
                    x + w / 2.0,
                    y + h / 2.0,
                    w / 2.0,
                    h / 2.0
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
                    r#"<rect x="{x}" y="{y}" width="{w}" height="{h}"{radius} {fa}{tf}/>"#
                );
            }
            out.push('\n');
            if let Some(b) = &item.border {
                let (r, g, b2) = to_255(b.color);
                let bw = b.width * s;
                if item.ellipse {
                    let _ = write!(
                        out,
                        r#"<ellipse cx="{}" cy="{}" rx="{}" ry="{}" fill="none" stroke="rgb({r},{g},{b2})" stroke-width="{bw}" stroke-opacity="{}"{tf}/>"#,
                        x + w / 2.0,
                        y + h / 2.0,
                        (w - bw) / 2.0,
                        (h - bw) / 2.0,
                        b.color[3] * item.opacity
                    );
                } else {
                    let rr = item.radii[0].min(item.rect[2].min(item.rect[3]) / 2.0) * s;
                    let _ = write!(
                        out,
                        r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{rr}" fill="none" stroke="rgb({r},{g},{b2})" stroke-width="{bw}" stroke-opacity="{}"{tf}/>"#,
                        x + bw / 2.0,
                        y + bw / 2.0,
                        w - bw,
                        h - bw,
                        b.color[3] * item.opacity
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
    let _ = write!(d, "M{} {} ", x + tl, y);
    let _ = write!(d, "L{} {} ", x + w - tr, y);
    if tr > 0.0 {
        let k = KAPPA * tr;
        let _ = write!(
            d,
            "C{} {} {} {} {} {} ",
            x + w - tr + k,
            y,
            x + w,
            y + tr - k,
            x + w,
            y + tr
        );
    }
    let _ = write!(d, "L{} {} ", x + w, y + h - br);
    if br > 0.0 {
        let k = KAPPA * br;
        let _ = write!(
            d,
            "C{} {} {} {} {} {} ",
            x + w,
            y + h - br + k,
            x + w - br + k,
            y + h,
            x + w - br,
            y + h
        );
    }
    let _ = write!(d, "L{} {} ", x + bl, y + h);
    if bl > 0.0 {
        let k = KAPPA * bl;
        let _ = write!(
            d,
            "C{} {} {} {} {} {} ",
            x + bl - k,
            y + h,
            x,
            y + h - bl + k,
            x,
            y + h - bl
        );
    }
    let _ = write!(d, "L{} {} ", x, y + tl);
    if tl > 0.0 {
        let k = KAPPA * tl;
        let _ = write!(
            d,
            "C{} {} {} {} {} {} ",
            x,
            y + tl - k,
            x + tl - k,
            y,
            x + tl,
            y
        );
    }
    d.push('Z');
    d
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
