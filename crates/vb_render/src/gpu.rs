//! GPU 场景构建(vello,ADR-0002/0015):DrawList → `vello::Scene`。
//!
//! GUI 画布与(未来的)GPU 原生导出共用本编码;文本与冻结块在画布上由
//! vb_app 的 egui 覆盖层近似绘制(ADR-0017),此处画占位形状。
//! 不透明度乘进画刷 alpha(vello 0.10 无独立 alpha 参数)。

use vello::kurbo::{Affine, Ellipse, Rect, RoundedRect, RoundedRectRadii};
use vello::peniko::{color::DynamicColor, Brush, Color, ColorStop, ColorStops, Gradient};
use vello::Scene;

use crate::encode::{DrawItem, DrawKind, DrawList, FillDef};

/// 把 DrawList 编码进 Scene(原点 = 画板左上角)。
pub fn encode_scene(scene: &mut Scene, list: &DrawList) {
    for item in &list.items {
        draw_item(scene, item);
    }
}

/// [f32;4] → peniko Color(alpha 已含不透明度)。
fn to_color(c: [f32; 4], opacity: f32) -> Color {
    Color::new([
        c[0].clamp(0.0, 1.0),
        c[1].clamp(0.0, 1.0),
        c[2].clamp(0.0, 1.0),
        (c[3] * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0),
    ])
}

fn to_stops(stops: &[crate::encode::GradientStop], opacity: f32) -> ColorStops {
    let mut out = ColorStops::new();
    for s in stops {
        out.push(ColorStop {
            offset: s.pos.clamp(0.0, 1.0),
            color: DynamicColor::from_alpha_color(to_color(s.color, opacity)),
        });
    }
    out
}

enum ShapeKind {
    Rect(RoundedRect),
    Ellipse(Ellipse),
}

fn shape_of(item: &DrawItem) -> ShapeKind {
    let [x, y, w, h] = item.rect;
    if item.ellipse {
        return ShapeKind::Ellipse(Ellipse::new(
            vello::kurbo::Point::new(x + w / 2.0, y + h / 2.0),
            (w / 2.0, h / 2.0),
            0.0,
        ));
    }
    let r = item.radii.map(|r| r.clamp(0.0, w.min(h) / 2.0));
    ShapeKind::Rect(RoundedRect::from_rect(
        Rect::new(x, y, x + w, y + h),
        RoundedRectRadii::new(r[0], r[1], r[2], r[3]),
    ))
}

fn item_tf(item: &DrawItem) -> Affine {
    let [x, y, w, h] = item.rect;
    if item.rot.abs() > 1e-9 {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        Affine::translate((cx, cy))
            * Affine::rotate(item.rot.to_radians())
            * Affine::translate((-cx, -cy))
    } else {
        Affine::IDENTITY
    }
}

fn draw_item(scene: &mut Scene, item: &DrawItem) {
    let [x, y, w, h] = item.rect;
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let tf = item_tf(item);
    // P4 77e291cf8def5f84:BezPath 4f1851486e3267d3(fill + stroke)
    if let Some(kpath) = &item.path {
        let mut vpath = vello::kurbo::BezPath::new();
        for el in kpath.elements() {
            match el {
                vb_common::geom::PathEl::MoveTo(p) => {
                    vpath.move_to(vello::kurbo::Point::new(p.x + x, p.y + y))
                }
                vb_common::geom::PathEl::LineTo(p) => {
                    vpath.line_to(vello::kurbo::Point::new(p.x + x, p.y + y))
                }
                vb_common::geom::PathEl::QuadTo(c, p) => vpath.quad_to(
                    vello::kurbo::Point::new(c.x + x, c.y + y),
                    vello::kurbo::Point::new(p.x + x, p.y + y),
                ),
                vb_common::geom::PathEl::CurveTo(c1, c2, p) => vpath.curve_to(
                    vello::kurbo::Point::new(c1.x + x, c1.y + y),
                    vello::kurbo::Point::new(c2.x + x, c2.y + y),
                    vello::kurbo::Point::new(p.x + x, p.y + y),
                ),
                vb_common::geom::PathEl::ClosePath => vpath.close_path(),
            }
        }
        let brush = match &item.fill {
            Some(FillDef::Solid(c)) => Brush::Solid(to_color(*c, item.opacity)),
            _ => Brush::Solid(to_color([0.5, 0.5, 0.5, 1.0], item.opacity)),
        };
        scene.fill(vello::peniko::Fill::NonZero, tf, &brush, None, &vpath);
        if let Some(b) = &item.border {
            let stroke = vello::kurbo::Stroke::new(b.width.max(1.0));
            let sbrush = Brush::Solid(to_color(b.color, item.opacity));
            scene.stroke(&stroke, tf, &sbrush, None, &vpath);
        }
        return;
    }
    if item.kind == DrawKind::Image {
        if let Some(bmp) = &item.image {
            // 真实位图(B3):画布与导出同源,不再画米色占位
            let peniko_img = vello::peniko::ImageData {
                data: vello::peniko::Blob::new(bmp.rgba.clone()),
                format: vello::peniko::ImageFormat::Rgba8,
                alpha_type: vello::peniko::ImageAlphaType::Alpha,
                width: bmp.width,
                height: bmp.height,
            };
            let brush = Brush::Image(vello::peniko::ImageBrush::new(peniko_img));
            // 路径 = 位图像素空间矩形,经 base 变换铺到节点矩形
            let rect = vello::kurbo::Rect::new(0.0, 0.0, bmp.width as f64, bmp.height as f64);
            let base = Affine::translate((x, y))
                * Affine::scale_non_uniform(w / bmp.width as f64, h / bmp.height as f64);
            scene.fill(vello::peniko::Fill::NonZero, tf * base, &brush, None, &rect);
            return;
        }
        let shape = shape_of(item);
        let brush = Brush::Solid(to_color([0.85, 0.83, 0.8, 1.0], item.opacity));
        fill_shape(scene, &shape, &brush, tf);
        return;
    }
    if item.kind == DrawKind::FrozenPlaceholder {
        let shape = shape_of(item);
        let brush = Brush::Solid(to_color([0.85, 0.83, 0.8, 1.0], item.opacity));
        fill_shape(scene, &shape, &brush, tf);
        return;
    }
    if item.kind == DrawKind::Text {
        // 画布文本由 egui 覆盖层近似绘制(ADR-0017)
        return;
    }
    let shape = shape_of(item);
    if let Some(fill) = &item.fill {
        let brush: Option<Brush> = match fill {
            FillDef::Solid(c) => Some(Brush::Solid(to_color(*c, item.opacity))),
            FillDef::LinearGradient { angle_css, stops } => {
                let (start_pt, end_pt) = crate::cpu::gradient_line(*angle_css, w, h);
                Some(Brush::Gradient(
                    Gradient::new_linear(
                        vello::kurbo::Point::new(start_pt.x as f64 + x, start_pt.y as f64 + y),
                        vello::kurbo::Point::new(end_pt.x as f64 + x, end_pt.y as f64 + y),
                    )
                    .with_stops(to_stops(stops, item.opacity)),
                ))
            }
            FillDef::RadialGradient { cx, cy, stops } => {
                let radius = ((w * w + h * h) as f32).sqrt() / 2.0;
                Some(Brush::Gradient(
                    Gradient::new_radial(
                        vello::kurbo::Point::new(x + w * *cx as f64, y + h * *cy as f64),
                        radius,
                    )
                    .with_stops(to_stops(stops, item.opacity)),
                ))
            }
        };
        if let Some(b) = brush {
            fill_shape(scene, &shape, &b, tf);
        }
    }
    if let Some(border) = &item.border {
        if border.width > 0.0 {
            let stroke = vello::kurbo::Stroke::new(border.width.max(1.0));
            let brush = Brush::Solid(to_color(border.color, item.opacity));
            match &shape {
                ShapeKind::Ellipse(e) => scene.stroke(&stroke, tf, &brush, None, e),
                ShapeKind::Rect(r) => scene.stroke(&stroke, tf, &brush, None, r),
            };
        }
    }
}

fn fill_shape(scene: &mut Scene, shape: &ShapeKind, brush: &Brush, tf: Affine) {
    match shape {
        ShapeKind::Ellipse(e) => scene.fill(vello::peniko::Fill::NonZero, tf, brush, None, e),
        ShapeKind::Rect(r) => scene.fill(vello::peniko::Fill::NonZero, tf, brush, None, r),
    };
}
