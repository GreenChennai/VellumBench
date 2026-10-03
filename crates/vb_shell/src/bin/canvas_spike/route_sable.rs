//! 路线 (b):vb_render DrawList → sable-paint `PaintSink` 翻译层。
//!
//! 上层只面向 [`sable_paint::sink::PaintSink`] 编码,GPU(VelloSink)与
//! CPU(VelloCpuSink)两后端共用同一份翻译(降级 = 换 Sink,翻译零改动)
//! —— 这正是 sable 手册宣示的架构红利,本模块同时验证它对 vb 文档域
//! DrawList 的覆盖度。
//!
//! 文本:走 sable-canvas 真文本管线(parley 布局缓存 + skrifa 字形轮廓 →
//! BezPath → sink.fill);CJK 覆盖由 fontique 系统字体回退保证。

use sable::canvas::text_glyphs;
use sable::core::scene::{GradientStop as SableStop, Paint, Rgba8, StrokeStyle};
use sable::kurbo::{self, Affine, PathEl};
use sable::paint::sink::PaintSink;
use vb_render::encode::{DrawItem, DrawList, FillDef};

fn rgba(c: [f32; 4]) -> Rgba8 {
    let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [to_u8(c[0]), to_u8(c[1]), to_u8(c[2]), to_u8(c[3])]
}

fn stops_of(stops: &[vb_render::encode::GradientStop], opacity: f32) -> Vec<SableStop> {
    stops
        .iter()
        .map(|s| SableStop {
            offset: s.pos.clamp(0.0, 1.0),
            color: rgba([
                s.color[0],
                s.color[1],
                s.color[2],
                s.color[3] * opacity.clamp(0.0, 1.0),
            ]),
        })
        .collect()
}

/// CSS 渐变线(镜像 vb_render::cpu::gradient_line 的角度约定,
/// 免引 tiny-skia:0° = 向上,顺时针;返回画板本地绝对坐标)。
fn gradient_line(angle_css: f64, x: f64, y: f64, w: f64, h: f64) -> ([f64; 2], [f64; 2]) {
    let rad = angle_css.to_radians();
    let dx = rad.sin();
    let dy = -rad.cos();
    let l = w * dx.abs() + h * dy.abs();
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    (
        [cx - dx * l / 2.0, cy - dy * l / 2.0],
        [cx + dx * l / 2.0, cy + dy * l / 2.0],
    )
}

/// 把 DrawList 编进任意 PaintSink(坐标:画板本地 × `viewport` 仿射)。
///
/// `cache_hits`/`cache_misses` 回传文本布局缓存命中(观测面,可传 &mut 0)。
pub fn translate_into(
    list: &DrawList,
    viewport: Affine,
    sink: &mut dyn PaintSink,
    cache_hits: &mut u64,
    cache_misses: &mut u64,
) {
    let mut path = kurbo::BezPath::new();
    for item in &list.items {
        let [x, y, w, h] = item.rect;
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        // 节点变换:旋转绕矩形中心(与 gpu.rs item_tf 同口径)。
        let node_tf = if item.rot.abs() > 1e-9 {
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            Affine::translate((cx, cy))
                * Affine::rotate(item.rot.to_radians())
                * Affine::translate((-cx, -cy))
        } else {
            Affine::IDENTITY
        };
        let total = viewport * node_tf;

        if let Some(hint) = &item.label {
            if item.kind == vb_render::encode::DrawKind::Text {
                let layout = text_glyphs::layout_text(&hint.text, hint.font_size, rgba(hint.color));
                let color = rgba([
                    hint.color[0],
                    hint.color[1],
                    hint.color[2],
                    hint.color[3] * item.opacity.clamp(0.0, 1.0),
                ]);
                text_glyphs::draw_text(
                    sink,
                    &layout,
                    total * Affine::translate((x, y)),
                    color,
                    cache_hits,
                    cache_misses,
                );
                continue;
            }
        }

        shape_path(item, &mut path);
        let fill_paint: Option<Paint> = match &item.fill {
            Some(FillDef::Solid(c)) => Some(Paint::Solid(rgba(*c))),
            Some(FillDef::LinearGradient { angle_css, stops }) => {
                let (start, end) = gradient_line(*angle_css, x, y, w, h);
                Some(Paint::LinearGradient {
                    start,
                    end,
                    stops: stops_of(stops, item.opacity),
                })
            }
            Some(FillDef::RadialGradient { cx, cy, stops }) => {
                let radius = ((w * w + h * h) as f32).sqrt() as f64 / 2.0;
                Some(Paint::RadialGradient {
                    center: [x + w * *cx as f64, y + h * *cy as f64],
                    radius,
                    stops: stops_of(stops, item.opacity),
                })
            }
            None => None,
        };
        if let Some(paint) = fill_paint {
            sink.fill_with_opacity(&paint, item.opacity as f64, total, &path);
        }
        if let Some(border) = &item.border {
            if border.width > 0.0 {
                sink.stroke(
                    &StrokeStyle {
                        paint: Paint::Solid(rgba(border.color)),
                        width: border.width.max(1.0),
                    },
                    total,
                    &path,
                );
            }
        }
    }
}

/// 几何 → kurbo 路径(清空复用传入缓冲,避免每项一次堆分配)。
fn shape_path(item: &DrawItem, path: &mut kurbo::BezPath) {
    path.truncate(0);
    let [x, y, w, h] = item.rect;
    if let Some(src) = &item.path {
        for el in src.elements() {
            match el {
                PathEl::MoveTo(p) => path.move_to((p.x + x, p.y + y)),
                PathEl::LineTo(p) => path.line_to((p.x + x, p.y + y)),
                PathEl::QuadTo(c, p) => path.quad_to((c.x + x, c.y + y), (p.x + x, p.y + y)),
                PathEl::CurveTo(c1, c2, p) => path.curve_to(
                    (c1.x + x, c1.y + y),
                    (c2.x + x, c2.y + y),
                    (p.x + x, p.y + y),
                ),
                PathEl::ClosePath => path.close_path(),
            }
        }
        return;
    }
    if item.ellipse {
        path.move_to((x + w, y + h / 2.0));
        path.curve_to(
            (x + w, y + h * 0.2238),
            (x + w * 0.7762, y),
            (x + w / 2.0, y),
        );
        path.curve_to((x + w * 0.2238, y), (x, y + h * 0.2238), (x, y + h / 2.0));
        path.curve_to(
            (x, y + h * 0.7762),
            (x + w * 0.2238, y + h),
            (x + w / 2.0, y + h),
        );
        path.curve_to(
            (x + w * 0.7762, y + h),
            (x + w, y + h * 0.7762),
            (x + w, y + h / 2.0),
        );
        path.close_path();
        return;
    }
    // 矩形(圆角半径逐角钳制,与 gpu.rs shape_of 同口径)。
    let r: [f64; 4] = item.radii.map(|r| r.clamp(0.0, w.min(h) / 2.0));
    path.move_to((x + r[0], y));
    path.line_to((x + w - r[1], y));
    if r[1] > 0.0 {
        path.curve_to(
            (x + w - r[1] * K, y),
            (x + w, y + r[1] * K),
            (x + w, y + r[1]),
        );
    }
    path.line_to((x + w, y + h - r[2]));
    if r[2] > 0.0 {
        path.curve_to(
            (x + w, y + h - r[2] * K),
            (x + w - r[2] * K, y + h),
            (x + w - r[2], y + h),
        );
    }
    path.line_to((x + r[3], y + h));
    if r[3] > 0.0 {
        path.curve_to(
            (x + r[3] * K, y + h),
            (x, y + h - r[3] * K),
            (x, y + h - r[3]),
        );
    }
    path.line_to((x, y + r[0]));
    if r[0] > 0.0 {
        path.curve_to((x, y + r[0] * K), (x + r[0] * K, y), (x + r[0], y));
    }
    path.close_path();
}

/// 圆角 kappa(四次贝塞尔近似 90° 圆弧)。
const K: f64 = 0.5522847498307936;
