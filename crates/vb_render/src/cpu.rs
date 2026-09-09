//! CPU 光栅化引擎(tiny-skia,ADR-0016):CLI 导出 / CI 渲染快照用,无 GPU 依赖。
//!
//! 视觉范围 v0.1:纯色/线性/径向渐变填充、圆角矩形、椭圆、描边、透明度、位图;
//! 文本按 ADR-0017 画占位条;冻结块画灰色占位框 —— 两者均产生 warnings。

use tiny_skia::{
    Color, FillRule, Paint, Path as SkPath, PathBuilder, Pixmap, Shader, SpreadMode, Stroke,
    Transform,
};

use crate::encode::{DrawItem, DrawKind, DrawList, FillDef};

const KAPPA: f32 = 0.552_284_8;

pub struct CpuRenderResult {
    pub png: Vec<u8>,
    pub warnings: Vec<String>,
}

/// 渲染 DrawList → PNG。`project_dir` 用于解析相对路径图片(缺失时占位 + warning)。
pub fn render_png(
    list: &DrawList,
    scale: f32,
    transparent: bool,
    project_dir: Option<&std::path::Path>,
) -> Result<CpuRenderResult, String> {
    let scale = scale.max(0.01);
    let w = (list.w * scale as f64).round().max(1.0) as u32;
    let h = (list.h * scale as f64).round().max(1.0) as u32;
    let mut pixmap = Pixmap::new(w, h).ok_or("画布尺寸非法")?;
    let mut warnings = Vec::new();

    if !transparent {
        let [r, g, b, a] = list.background;
        pixmap.fill(Color::from_rgba(r, g, b, a).unwrap_or(Color::WHITE));
    }

    let tf = Transform::from_scale(scale, scale);
    for item in &list.items {
        draw_item(&mut pixmap, item, scale, tf, project_dir, &mut warnings);
    }

    let png = pixmap
        .encode_png()
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    Ok(CpuRenderResult { png, warnings })
}

fn draw_item(
    pixmap: &mut Pixmap,
    item: &DrawItem,
    scale: f32,
    tf: Transform,
    project_dir: Option<&std::path::Path>,
    warnings: &mut Vec<String>,
) {
    let [x, y, iw, ih] = item.rect;
    if iw <= 0.0 || ih <= 0.0 {
        return;
    }
    let text_hint: Option<&crate::encode::TextHint> = match item.kind {
        DrawKind::Text => {
            // ADR-0017:占位条(叠加在节点自身填充之上,见下方 Box|Text 臂)
            if let Some(t) = &item.label {
                warnings.push(format!(
                    "文本「{}」以占位条渲染(原生文本管线 v0.2 接入)",
                    t.text
                ));
            }
            item.label.as_ref()
        }
        _ => None,
    };
    match item.kind {
        DrawKind::FrozenPlaceholder => {
            warnings.push("冻结块以灰色占位渲染(浏览器引擎导出可获真值)".to_string());
            if let Some(path) =
                rect_path(x, y, iw, ih, item.radii.map(|r| r.min(32.0)), item.ellipse)
            {
                fill_color(pixmap, &path, [0.85, 0.83, 0.8, 1.0], item.opacity, tf);
            }
        }
        DrawKind::Image => {
            let src = item.src.clone().unwrap_or_default();
            let mut drew = false;
            if !src.is_empty() {
                if let Some(dir) = project_dir {
                    let p = dir.join(&src);
                    if p.is_file() {
                        match draw_bitmap(pixmap, &p, x, y, iw, ih, scale) {
                            Ok(()) => drew = true,
                            Err(e) => warnings.push(format!("图片解码失败 {src}: {e}")),
                        }
                    }
                }
            }
            if !drew {
                warnings.push(format!("图片缺失或未解码:{src}(画红框占位)"));
                if let Some(path) = rect_path(x + 1.0, y + 1.0, iw - 2.0, ih - 2.0, [0.0; 4], false)
                {
                    stroke_color(pixmap, &path, [0.9, 0.3, 0.3, 1.0], 2.0, item.opacity, tf);
                }
            }
        }
        DrawKind::Box | DrawKind::Text => {
            if let Some(fill) = &item.fill {
                if let Some(shape) = shape_for(item, x, y, iw, ih) {
                    match fill {
                        FillDef::Solid(c) => fill_color(pixmap, &shape, *c, item.opacity, tf),
                        FillDef::LinearGradient { angle_css, stops } => {
                            let (start, end) = gradient_line(*angle_css, iw, ih);
                            match tiny_skia::LinearGradient::new(
                                start,
                                end,
                                to_skia_stops(stops),
                                SpreadMode::Pad,
                                Transform::identity(),
                            ) {
                                Some(shader) => {
                                    fill_shader(pixmap, &shape, shader, item.opacity, tf)
                                }
                                None => warnings.push("线性渐变非法(已跳过)".into()),
                            }
                        }
                        FillDef::RadialGradient { cx, cy, stops } => {
                            let center = tiny_skia::Point::from_xy(
                                (x + iw * *cx as f64) as f32,
                                (y + ih * *cy as f64) as f32,
                            );
                            let radius = (iw * iw + ih * ih).sqrt() as f32 / 2.0;
                            match tiny_skia::RadialGradient::new(
                                center,
                                0.0,
                                center,
                                radius,
                                to_skia_stops(stops),
                                SpreadMode::Pad,
                                Transform::identity(),
                            ) {
                                Some(shader) => {
                                    fill_shader(pixmap, &shape, shader, item.opacity, tf)
                                }
                                None => warnings.push("径向渐变非法(已跳过)".into()),
                            }
                        }
                    }
                }
            }
            if let Some(border) = &item.border {
                let bw = border.width.max(1.0);
                if let Some(shape) = shape_for(
                    item,
                    x + bw / 2.0,
                    y + bw / 2.0,
                    (iw - bw).max(1.0),
                    (ih - bw).max(1.0),
                ) {
                    stroke_color(pixmap, &shape, border.color, bw as f32, item.opacity, tf);
                }
            }
            // 文本占位条(ADR-0017):画在自身填充之上
            if let Some(t) = text_hint {
                let bar_h = (t.font_size * 0.62).min(ih).max(4.0);
                if let Some(path) = rect_path(
                    x,
                    y + (ih - bar_h).min(ih) * 0.25,
                    (t.font_size * 0.55 * t.text.chars().count() as f64).min(iw),
                    bar_h,
                    [2.0; 4],
                    false,
                ) {
                    fill_color(pixmap, &path, t.color, item.opacity * 0.9, tf);
                }
            }
        }
    }
}

fn shape_for(item: &DrawItem, x: f64, y: f64, w: f64, h: f64) -> Option<SkPath> {
    rect_path(
        x,
        y,
        w,
        h,
        item.radii.map(|r| r.min(w.min(h) / 2.0)),
        item.ellipse,
    )
}

/// 圆角矩形 / 椭圆路径。
fn rect_path(x: f64, y: f64, w: f64, h: f64, radii: [f64; 4], ellipse: bool) -> Option<SkPath> {
    let (x, y, w, h) = (x as f32, y as f32, w as f32, h as f32);
    let mut pb = PathBuilder::new();
    if ellipse {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let (ox, oy) = (w / 2.0, h / 2.0);
        let (mx, my) = (ox * KAPPA, oy * KAPPA);
        pb.move_to(cx + ox, cy);
        pb.cubic_to(cx + ox, cy + my, cx + mx, cy + oy, cx, cy + oy);
        pb.cubic_to(cx - mx, cy + oy, cx - ox, cy + my, cx - ox, cy);
        pb.cubic_to(cx - ox, cy - my, cx - mx, cy - oy, cx, cy - oy);
        pb.cubic_to(cx + mx, cy - oy, cx + ox, cy - my, cx + ox, cy);
        pb.close();
    } else {
        let [tl, tr, br, bl] = radii.map(|r| (r as f32).clamp(0.0, w.min(h) / 2.0));
        pb.move_to(x + tl, y);
        pb.line_to(x + w - tr, y);
        if tr > 0.0 {
            // 右上角:cubic (x+w-tr, y) → (x+w, y+tr)
            let k = KAPPA * tr;
            pb.cubic_to(x + w - tr + k, y, x + w, y + tr - k, x + w, y + tr);
        }
        pb.line_to(x + w, y + h - br);
        if br > 0.0 {
            let k = KAPPA * br;
            pb.cubic_to(
                x + w,
                y + h - br + k,
                x + w - br + k,
                y + h,
                x + w - br,
                y + h,
            );
        }
        pb.line_to(x + bl, y + h);
        if bl > 0.0 {
            let k = KAPPA * bl;
            pb.cubic_to(x + bl - k, y + h, x, y + h - bl + k, x, y + h - bl);
        }
        pb.line_to(x, y + tl);
        if tl > 0.0 {
            let k = KAPPA * tl;
            pb.cubic_to(x, y + tl - k, x + tl - k, y, x + tl, y);
        }
        pb.close();
    }
    pb.finish()
}

fn fill_color(pixmap: &mut Pixmap, path: &SkPath, color: [f32; 4], opacity: f32, tf: Transform) {
    let mut paint = Paint::default();
    paint.set_color(with_alpha(color, opacity));
    paint.anti_alias = true;
    pixmap.fill_path(path, &paint, FillRule::Winding, tf, None);
}

/// 颜色 × 不透明度(tiny-skia 0.12 无 Paint::opacity,乘进 alpha)。
fn with_alpha(color: [f32; 4], opacity: f32) -> Color {
    Color::from_rgba(
        color[0],
        color[1],
        color[2],
        color[3] * opacity.clamp(0.0, 1.0),
    )
    .unwrap_or(Color::BLACK)
}

fn fill_shader(pixmap: &mut Pixmap, path: &SkPath, shader: Shader, _opacity: f32, tf: Transform) {
    let paint = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    pixmap.fill_path(path, &paint, FillRule::Winding, tf, None);
}

fn stroke_color(
    pixmap: &mut Pixmap,
    path: &SkPath,
    color: [f32; 4],
    width: f32,
    opacity: f32,
    tf: Transform,
) {
    let mut paint = Paint::default();
    paint.set_color(with_alpha(color, opacity));
    paint.anti_alias = true;
    pixmap.stroke_path(
        path,
        &paint,
        &Stroke {
            width,
            ..Stroke::default()
        },
        tf,
        None,
    );
}

fn draw_bitmap(
    pixmap: &mut Pixmap,
    path: &std::path::Path,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    scale: f32,
) -> Result<(), String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    let dw = ((w * scale as f64).round() as u32).max(1);
    let dh = ((h * scale as f64).round() as u32).max(1);
    let resized = image::imageops::resize(&rgba, dw, dh, image::imageops::FilterType::Triangle);
    let Some(mut pm) = Pixmap::new(dw, dh) else {
        return Err("tiny-skia 位图构建失败".into());
    };
    pm.data_mut().copy_from_slice(resized.as_raw());
    pixmap.draw_pixmap(
        0,
        0,
        pm.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::from_translate(x as f32, y as f32),
        None,
    );
    Ok(())
}

fn to_skia_stops(stops: &[crate::encode::GradientStop]) -> Vec<tiny_skia::GradientStop> {
    stops
        .iter()
        .filter_map(|s| {
            Color::from_rgba(s.color[0], s.color[1], s.color[2], s.color[3])
                .map(|c| tiny_skia::GradientStop::new(s.pos.clamp(0.0, 1.0), c))
        })
        .collect()
}

/// CSS 渐变角 → (start, end) 点(CSS:0deg 向上,顺时针为正;长度覆盖盒子)。
pub fn gradient_line(angle_css_deg: f64, w: f64, h: f64) -> (tiny_skia::Point, tiny_skia::Point) {
    let rad = angle_css_deg.to_radians();
    let dx = rad.sin() as f32;
    let dy = -(rad.cos()) as f32;
    let l = (w as f32 * dx.abs()) + (h as f32 * dy.abs());
    let cx = (w / 2.0) as f32;
    let cy = (h / 2.0) as f32;
    (
        tiny_skia::Point::from_xy(cx - dx * l / 2.0, cy - dy * l / 2.0),
        tiny_skia::Point::from_xy(cx + dx * l / 2.0, cy + dy * l / 2.0),
    )
}
