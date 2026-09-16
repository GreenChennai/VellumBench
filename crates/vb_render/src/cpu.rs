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

    let base = Transform::from_scale(scale, scale);
    for item in &list.items {
        let tf = if item.rot.abs() > 1e-9 {
            let [x, y, w, h] = item.rect;
            let (cx, cy) = ((x + w / 2.0) * scale as f64, (y + h / 2.0) * scale as f64);
            let rot = Transform::from_rotate(item.rot as f32);
            let to_c = Transform::from_translate(cx as f32, cy as f32);
            let from_c = Transform::from_translate(-cx as f32, -cy as f32);
            base.post_concat(to_c).post_concat(rot).post_concat(from_c)
        } else {
            base
        };
        draw_item(&mut pixmap, item, scale, tf, project_dir, &mut warnings);
        if let Some(f) = &item.filter {
            if !f.is_identity() {
                apply_filter_region(&mut pixmap, item, scale, f);
            }
        }
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
    // clip-path(L2):inset 快路径直接缩矩形;圆/椭圆/多边形走离屏蒙版
    if let Some(clip) = &item.clip {
        match clip {
            crate::encode::ClipDef::Inset(t, r, b, l) => {
                let mut trimmed = item.clone();
                trimmed.clip = None;
                trimmed.rect = [x + l, y + t, (iw - l - r).max(0.0), (ih - t - b).max(0.0)];
                draw_item(pixmap, &trimmed, scale, tf, project_dir, warnings);
                return;
            }
            _ => {
                apply_clip_mask(pixmap, item, scale, tf, project_dir, warnings);
                return;
            }
        }
    }
    // P4 矢量路径:kurbo BezPath -> tiny_skia Path
    if let Some(kpath) = &item.path {
        if let Some(tp) = kurbo_to_skia_path(kpath, item.rect[0], item.rect[1], item.opacity, item)
        {
            let mut fill_col = [0.5f32, 0.5, 0.5, 1.0];
            if let Some(crate::encode::FillDef::Solid(col)) = &item.fill {
                fill_col = *col;
            }
            let mut paint = Paint {
                anti_alias: true,
                ..Paint::default()
            };
            paint.set_color(with_alpha(fill_col, item.opacity));
            pixmap.fill_path(&tp, &paint, FillRule::Winding, tf, None);
            if let Some(b) = &item.border {
                paint.set_color(with_alpha(b.color, item.opacity));
                pixmap.stroke_path(
                    &tp,
                    &paint,
                    &Stroke {
                        width: b.width.max(1.0) as f32,
                        ..Stroke::default()
                    },
                    tf,
                    None,
                );
            }
        }
    }

    let text_hint: Option<&crate::encode::TextHint> = match item.kind {
        DrawKind::Text => item.label.as_ref(),
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
            if let Some(bmp) = &item.image {
                // 已挂载位图:直接走 RGBA,不再读文件(B3)
                draw_bitmap_rgba(pixmap, bmp, x, y, iw, ih, scale, item.rot, item.opacity);
                drew = true;
            }
            if !drew && !src.is_empty() {
                if let Some(dir) = project_dir {
                    let p = dir.join(&src);
                    if p.is_file() {
                        match draw_bitmap(pixmap, &p, x, y, iw, ih, scale, item.rot, item.opacity) {
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
        DrawKind::Box | DrawKind::Text | DrawKind::VectorPath => {
            if let Some(fill) = &item.fill {
                if let Some(shape) = shape_for(item, x, y, iw, ih) {
                    match fill {
                        FillDef::Solid(c) => fill_color(pixmap, &shape, *c, item.opacity, tf),
                        FillDef::LinearGradient { angle_css, stops } => {
                            let (sp, ep) = gradient_line(*angle_css, iw, ih);
                            // shader 坐标与路径同处 pre-transform 空间:局部
                            // 线段必须平移到节点原点。径向分支历来如此;线性
                            // 分支此前漏加,非原点节点渐变被 Pad 成纯色。
                            let start = tiny_skia::Point::from_xy(sp.x + x as f32, sp.y + y as f32);
                            let end = tiny_skia::Point::from_xy(ep.x + x as f32, ep.y + y as f32);
                            match tiny_skia::LinearGradient::new(
                                start,
                                end,
                                to_skia_stops(stops, item.opacity),
                                SpreadMode::Pad,
                                Transform::identity(),
                            ) {
                                Some(shader) => fill_shader(pixmap, &shape, shader, tf),
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
                                to_skia_stops(stops, item.opacity),
                                SpreadMode::Pad,
                                Transform::identity(),
                            ) {
                                Some(shader) => fill_shader(pixmap, &shape, shader, tf),
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
            // C4 真文本:fontique/注册表选字 + swash 整形/轮廓;
            // 硬行分行整形(避免控制字符打乱字形映射),贪心断行、
            // 字重选字、字距、富文本段逐字取色
            if let Some(t) = text_hint {
                let max_w = iw.max(1.0) as f32;
                let line_h = if t.line_height > 0.0 {
                    t.line_height
                } else {
                    t.font_size * 1.32
                };
                let ls = t.letter_spacing as f32;
                let hard_lines = crate::text::layout_text_lines(
                    &t.text,
                    &t.font_family,
                    t.font_size as f32,
                    t.weight,
                    max_w,
                    ls,
                );
                let mut visual = 0usize;
                let mut byte_base = 0usize;
                for (hard, run, lines) in &hard_lines {
                    for line in lines {
                        if line.is_empty() {
                            continue;
                        }
                        let line_x0 = run.glyphs[line[0]].x as f64;
                        let baseline = y + run.ascent as f64 + visual as f64 * line_h;
                        for &gi in line {
                            let g = &run.glyphs[gi];
                            // 行内字符序 → 全文字节偏移(富文本段按字节区间)
                            let b = byte_base
                                + hard.chars().take(gi).map(|c| c.len_utf8()).sum::<usize>();
                            let color = t
                                .segments
                                .iter()
                                .find(|sg| b >= sg.start && b < sg.end)
                                .and_then(|sg| sg.color)
                                .unwrap_or(t.color);
                            if let Some(gpath) = crate::text::glyph_outline(
                                &run.font_data,
                                run.font_index,
                                t.font_size as f32,
                                g.id,
                            ) {
                                let gx = x + (g.x as f64 + gi as f64 * ls as f64) - line_x0;
                                if let Some(sk) = kurbo_to_skia_path(
                                    &gpath,
                                    gx,
                                    baseline - g.y as f64,
                                    item.opacity,
                                    item,
                                ) {
                                    fill_color(pixmap, &sk, color, item.opacity, tf);
                                }
                            }
                        }
                        visual += 1;
                    }
                    byte_base += hard.len() + 1; // +1 = 换行符
                }
                if visual == 0 {
                    warnings.push(format!("文本「{}」字体解析失败,以占位条渲染", t.text));
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

fn fill_shader(pixmap: &mut Pixmap, path: &SkPath, shader: Shader, tf: Transform) {
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

#[allow(clippy::too_many_arguments)]
fn draw_bitmap(
    pixmap: &mut Pixmap,
    path: &std::path::Path,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    scale: f32,
    rot_deg: f64,
    opacity: f32,
) -> Result<(), String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let rgba = img.to_rgba8();
    draw_bitmap_rgba(
        pixmap,
        &crate::encode::BitmapData {
            width: rgba.width(),
            height: rgba.height(),
            rgba: std::sync::Arc::new(rgba.into_raw()),
        },
        x,
        y,
        w,
        h,
        scale,
        rot_deg,
        opacity,
    );
    Ok(())
}

/// 把 RGBA 位图绘制到设备坐标(B3 核心;文件路径与 GPU 挂载共用)。
#[allow(clippy::too_many_arguments)]
fn draw_bitmap_rgba(
    pixmap: &mut Pixmap,
    bmp: &crate::encode::BitmapData,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    scale: f32,
    rot_deg: f64,
    opacity: f32,
) {
    let dw = ((w * scale as f64).round() as u32).max(1);
    let dh = ((h * scale as f64).round() as u32).max(1);
    let src = image::RgbaImage::from_raw(bmp.width, bmp.height, (*bmp.rgba).clone())
        .unwrap_or_else(|| image::RgbaImage::new(dw, dh));
    let resized = image::imageops::resize(&src, dw, dh, image::imageops::FilterType::Triangle);
    let Some(mut pm) = Pixmap::new(dw, dh) else {
        return;
    };
    pm.data_mut().copy_from_slice(resized.as_raw());
    // 位置必须乘 scale(设备坐标);旋转绕缩放后矩形中心,与其他绘制
    // 项同帧(此前旋转与不透明度均被忽略,且 @2x 落点减半)
    let mut tf = Transform::from_translate((x * scale as f64) as f32, (y * scale as f64) as f32);
    if rot_deg.abs() > 1e-9 {
        let cx = ((x + w / 2.0) * scale as f64) as f32;
        let cy = ((y + h / 2.0) * scale as f64) as f32;
        tf = Transform::from_translate(cx, cy)
            .post_concat(Transform::from_rotate(rot_deg as f32))
            .post_concat(Transform::from_translate(-cx, -cy))
            .post_concat(tf);
    }
    let paint = tiny_skia::PixmapPaint {
        opacity: opacity.clamp(0.0, 1.0),
        ..tiny_skia::PixmapPaint::default()
    };
    pixmap.draw_pixmap(0, 0, pm.as_ref(), &paint, tf, None);
}

/// 色标 → tiny-skia 色标。节点 opacity 乘进每档 alpha(与 GPU 端一致,
/// 此前 CPU 渐变完全忽略节点不透明度,三端各一种结果)。
fn to_skia_stops(
    stops: &[crate::encode::GradientStop],
    opacity: f32,
) -> Vec<tiny_skia::GradientStop> {
    let opacity = opacity.clamp(0.0, 1.0);
    stops
        .iter()
        .filter_map(|s| {
            Color::from_rgba(s.color[0], s.color[1], s.color[2], s.color[3] * opacity)
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

/// kurbo BezPath → tiny_skia Path(平移到 item 的锚点位置)。
pub fn kurbo_to_skia_path(
    src: &vb_common::geom::BezPath,
    ox: f64,
    oy: f64,
    opacity: f32,
    _item: &DrawItem,
) -> Option<SkPath> {
    let _ = opacity;
    let mut pb = PathBuilder::new();
    for el in src.elements() {
        use vb_common::geom::PathEl;
        match el {
            PathEl::MoveTo(p) => pb.move_to((p.x + ox) as f32, (p.y + oy) as f32),
            PathEl::LineTo(p) => pb.line_to((p.x + ox) as f32, (p.y + oy) as f32),
            PathEl::QuadTo(c, p) => pb.quad_to(
                (c.x + ox) as f32,
                (c.y + oy) as f32,
                (p.x + ox) as f32,
                (p.y + oy) as f32,
            ),
            PathEl::CurveTo(c1, c2, p) => pb.cubic_to(
                (c1.x + ox) as f32,
                (c1.y + oy) as f32,
                (c2.x + ox) as f32,
                (c2.y + oy) as f32,
                (p.x + ox) as f32,
                (p.y + oy) as f32,
            ),
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

/// 圆/椭圆/多边形 clip:离屏渲染该项 → 以裁剪形状 alpha 为蒙版相乘 → 合成。
fn apply_clip_mask(
    pixmap: &mut Pixmap,
    item: &DrawItem,
    scale: f32,
    tf: Transform,
    project_dir: Option<&std::path::Path>,
    warnings: &mut Vec<String>,
) {
    let [x, y, w, h] = item.rect;
    let bx = ((x * scale as f64).floor() as i32 - 2).max(0);
    let by = ((y * scale as f64).floor() as i32 - 2).max(0);
    let bw = ((w * scale as f64).ceil() as u32 + 4).min(pixmap.width().saturating_sub(bx as u32));
    let bh = ((h * scale as f64).ceil() as u32 + 4).min(pixmap.height().saturating_sub(by as u32));
    if bw == 0 || bh == 0 {
        return;
    }
    let Some(mut sub) = Pixmap::new(bw, bh) else {
        return;
    };
    let shift = Transform::from_translate(-(bx as f32), -(by as f32));
    let mut trimmed = item.clone();
    trimmed.clip = None;
    draw_item(
        &mut sub,
        &trimmed,
        scale,
        tf.post_concat(shift),
        project_dir,
        warnings,
    );
    let Some(mut mask) = Pixmap::new(bw, bh) else {
        return;
    };
    let white = Paint {
        anti_alias: true,
        shader: tiny_skia::Shader::SolidColor(Color::WHITE),
        ..Paint::default()
    };
    if let Some(shape) = clip_shape_path(item, scale) {
        let local = Transform::from_translate(bx as f32, by as f32)
            .invert()
            .unwrap_or_default();
        mask.fill_path(&shape, &white, FillRule::Winding, local, None);
    }
    let mask_px = mask.pixels();
    let sub_px = sub.pixels_mut();
    for (i, px) in sub_px.iter_mut().enumerate() {
        let m = mask_px[i].alpha();
        if m == 255 {
            continue;
        }
        let c = px.demultiply();
        let a = (c.alpha() as u32 * m as u32 / 255).min(255) as u8;
        *px = tiny_skia::ColorU8::from_rgba(c.red(), c.green(), c.blue(), a).premultiply();
    }
    pixmap.draw_pixmap(
        bx,
        by,
        sub.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

/// 裁剪形状(画布坐标 skia path)。
fn clip_shape_path(item: &DrawItem, scale: f32) -> Option<SkPath> {
    let [x, y, w, h] = item.rect;
    let sc = scale as f64;
    let mut pb = PathBuilder::new();
    match &item.clip {
        Some(crate::encode::ClipDef::Circle(cx, cy, r)) => {
            pb.push_circle((cx * sc) as f32, (cy * sc) as f32, (*r * sc) as f32);
        }
        Some(crate::encode::ClipDef::Ellipse(cx, cy, rx, ry)) => {
            let rect = tiny_skia::Rect::from_ltrb(
                ((cx - rx) * sc) as f32,
                ((cy - ry) * sc) as f32,
                ((cx + rx) * sc) as f32,
                ((cy + ry) * sc) as f32,
            )?;
            pb.push_oval(rect);
        }
        Some(crate::encode::ClipDef::Polygon(pts)) => {
            for (i, (px, py)) in pts.iter().enumerate() {
                let px = (px * sc) as f32;
                let py = (py * sc) as f32;
                if i == 0 {
                    pb.move_to(px, py);
                } else {
                    pb.line_to(px, py);
                }
            }
            pb.close();
        }
        Some(crate::encode::ClipDef::Inset(t, r, b, l)) => {
            let rect = tiny_skia::Rect::from_ltrb(
                ((x + l) * sc) as f32,
                ((y + t) * sc) as f32,
                ((x + w - r) * sc) as f32,
                ((y + h - b) * sc) as f32,
            )?;
            pb.push_rect(rect);
        }
        None => return None,
    }
    pb.finish()
}

/// filter(L3)区域后处理:模糊 + 亮度 + 饱和度(项 bbox 外扩模糊半径)。
fn apply_filter_region(
    pixmap: &mut Pixmap,
    item: &DrawItem,
    scale: f32,
    f: &crate::encode::FilterDef,
) {
    let [x, y, w, h] = item.rect;
    let margin = (f.blur * scale as f64 * 2.0).ceil() as i32;
    let bw = pixmap.width() as i32;
    let bh = pixmap.height() as i32;
    if bw < 2 || bh < 2 {
        return;
    }
    let x0 = ((x * scale as f64).floor() as i32 - margin).clamp(0, bw - 1);
    let y0 = ((y * scale as f64).floor() as i32 - margin).clamp(0, bh - 1);
    let x1 = (((x + w) * scale as f64).ceil() as i32 + margin).clamp(x0 + 1, bw);
    let y1 = (((y + h) * scale as f64).ceil() as i32 + margin).clamp(y0 + 1, bh);
    let rw = (x1 - x0) as u32;
    let rh = (y1 - y0) as u32;
    if rw == 0 || rh == 0 {
        return;
    }
    // 区域像素 → 直 alpha RGBA
    let mut crop = image::RgbaImage::new(rw, rh);
    for dy in 0..rh {
        for dx in 0..rw {
            if let Some(px) = pixmap.pixel((x0 + dx as i32) as u32, (y0 + dy as i32) as u32) {
                let c = px.demultiply();
                crop.put_pixel(
                    dx,
                    dy,
                    image::Rgba([c.red(), c.green(), c.blue(), c.alpha()]),
                );
            }
        }
    }
    let mut worked = image::DynamicImage::ImageRgba8(crop);
    if f.blur > 0.0 {
        worked = worked.blur(((f.blur * scale as f64) as f32).max(0.5));
    }
    let need_sat = (f.saturate - 1.0).abs() > 1e-3;
    let need_bri = (f.brightness - 1.0).abs() > 1e-3;
    if need_sat || need_bri {
        let sat: f32 = f.saturate as f32;
        let bri: f32 = f.brightness as f32;
        if let Some(img) = worked.as_mut_rgba8() {
            for p in img.pixels_mut() {
                let mut rgb = [p[0], p[1], p[2]];
                if need_sat {
                    let l =
                        0.2126 * rgb[0] as f32 + 0.7152 * rgb[1] as f32 + 0.0722 * rgb[2] as f32;
                    #[allow(clippy::needless_range_loop)]
                    for i in 0..3 {
                        let v = l + (rgb[i] as f32 - l) * sat;
                        rgb[i] = v.clamp(0.0, 255.0) as u8;
                    }
                }
                if need_bri {
                    for ch in rgb.iter_mut().take(3) {
                        *ch = ((*ch as f32) * bri).clamp(0.0, 255.0) as u8;
                    }
                }
                p[0] = rgb[0];
                p[1] = rgb[1];
                p[2] = rgb[2];
            }
        }
    }
    let out = worked.to_rgba8();
    let pw = pixmap.width();
    for dy in 0..rh {
        for dx in 0..rw {
            let p = out.get_pixel(dx, dy);
            let c = tiny_skia::ColorU8::from_rgba(p[0], p[1], p[2], p[3]).premultiply();
            let gx = (x0 + dx as i32) as u32;
            let gy = (y0 + dy as i32) as u32;
            let idx = (gy * pw + gx) as usize;
            let total = (pw * pixmap.height()) as usize;
            if idx < total {
                pixmap.pixels_mut()[idx] = c;
            }
        }
    }
}
