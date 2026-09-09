//! SVG 原生导出:DrawList → SVG(真实文本;渐变/圆角/旋转/透明度)。
//!
//! 文本以真实 `<text>` 输出(HTML 是源格式,SVG 同理,ADR-0010)。

use std::fmt::Write as _;

use vb_render::encode::{DrawItem, DrawKind, DrawList, FillDef};

pub fn render_svg(list: &DrawList, scale: u32) -> String {
    let w = list.w * scale as f64;
    let h = list.h * scale as f64;
    let mut out = String::with_capacity(64 * 1024);
    let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#
    );
    let _ = writeln!(out, "<defs>");

    // 渐变收集到 defs
    let mut defs = String::new();
    for (i, item) in list.items.iter().enumerate() {
        if let Some(FillDef::LinearGradient { angle_css, stops }) = &item.fill {
            let (sp, ep) = vb_render::cpu::gradient_line(*angle_css, item.rect[2], item.rect[3]);
            let (sx, sy, ex, ey) = (sp.x as u32, sp.y as u32, ep.x as u32, ep.y as u32);
            let _ = write!(
                defs,
                r#"  <linearGradient id="g{i}" x1="{sx}" y1="{sy}" x2="{ex}" y2="{ey}">"#
            );
            defs.push('\n');
            for s in stops {
                let (r, g, b) = to_255(s.color);
                let _ = write!(
                    defs,
                    r#"    <stop offset="{}" stop-color="rgb({r},{g},{b})" stop-opacity="{}"/>"#,
                    s.pos, s.color[3]
                );
                defs.push('\n');
            }
            let _ = writeln!(defs, "  </linearGradient>");
        }
        if let Some(FillDef::RadialGradient { cx, cy, stops }) = &item.fill {
            let _ = write!(
                defs,
                r#"  <radialGradient id="g{i}" cx="{}" cy="{}" r="0.7">"#,
                item.rect[2] * *cx as f64,
                item.rect[3] * *cy as f64
            );
            defs.push('\n');
            for s in stops {
                let (r, g, b) = to_255(s.color);
                let _ = write!(
                    defs,
                    r#"    <stop offset="{}" stop-color="rgb({r},{g},{b})" stop-opacity="{}"/>"#,
                    s.pos, s.color[3]
                );
                defs.push('\n');
            }
            let _ = writeln!(defs, "  </radialGradient>");
        }
    }
    out.push_str(&defs);
    out.push_str("</defs>\n");

    if !list.background.iter().all(|c| *c >= 0.999) || list.background[3] < 1.0 {
        // 画板有自定义背景色时画底色矩形
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
            format!(r#"fill="url(#g{i})""#)
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
    let (x, y, w, h) = (x * s, y * s, w * s, h * s);

    // 旋转 + 缩放统一 transform
    let mut tf = String::new();
    if (item.rot.abs() > 1e-9) || scale != 1.0 {
        let cx = x + w / 2.0;
        let cy = y + h / 2.0;
        let rot = if item.rot.abs() > 1e-9 {
            format!("rotate({} {cx} {cy}) ", item.rot)
        } else {
            String::new()
        };
        let sc = if scale != 1.0 {
            format!("scale({scale}) ")
        } else {
            String::new()
        };
        tf = format!(r#" transform="{}{}""#, rot, sc);
    }

    match item.kind {
        DrawKind::Text => {
            if let Some(t) = &item.label {
                let (r, g, b) = to_255(t.color);
                let weight = if t.weight_bold { "600" } else { "400" };
                let _ = write!(
                    out,
                    r#"<text x="{}" y="{}" font-size="{}" font-weight="{weight}" fill="rgb({r},{g},{b})" fill-opacity="{}"{tf}>{}</text>"#,
                    x,
                    y + t.font_size * s,
                    t.font_size * s,
                    t.color[3] * item.opacity,
                    escape_xml(&t.text),
                );
                out.push('\n');
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
            let _ = write!(
                out,
                r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none" stroke="rgb(230,77,77)" stroke-width="2"{tf}/><!-- image: {:?} -->"#,
                item.src
            );
            out.push('\n');
        }
        DrawKind::Box => {
            let fa = fill_attr(item, i);
            let radius = if item.ellipse {
                String::new()
            } else {
                let r = item.radii[0].min(w.min(h) / 2.0) * s;
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
                    let rr = item.radii[0].min(w.min(h) / 2.0) * s;
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

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
