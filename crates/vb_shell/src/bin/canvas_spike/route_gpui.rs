//! 路线 (c):vb_render DrawList → gpui 原生绘制原语翻译层(不经 vello)。
//!
//! 落点(gpui 0.2.2 三类原生图元,源码已核实):
//! - 矩形 → `Window::paint_quad`(`gpui::fill`,无 tessellation 快路径);
//! - 渐变 → `Background::LinearGradient`(gpui 0.2.2 **只有 2 停靠点**,
//!   多停靠点取首尾近似 —— 有意保留的保真差,交 ADR 记录);
//! - 椭圆/旋转矩形/贝塞尔 → `PathBuilder`(SVG 语义;`build()` 内做 lyon
//!   CPU tessellation,**逐帧、主线程、无跨帧缓存** —— gpui Path 是
//!   immediate-mode 图元,这是路线 (c) 的主要 CPU 成本,由 overlay 的
//!   translate_ms 直接量化);
//! - 文本 → `WindowTextSystem::shape_line`(DirectWrite 塑形 + 行布局
//!   缓存,缓存键含 font_size;CJK 走系统字体回退)。

use std::time::Duration;

use sable::gpui::{
    fill, font, linear_color_stop, linear_gradient, px, size, App, Background, Bounds, ColorSpace,
    Corners, Edges, Hsla, PaintQuad, Path, PathBuilder, Pixels, Point, Rgba, ShapedLine, TextRun,
    Window, WindowTextSystem,
};
use sable::kurbo::PathEl;
use vb_render::encode::{DrawItem, DrawList, FillDef};

/// 一帧的 gpui 原语(构建于 render 期,paint 期零几何计算)。
pub enum Prim {
    Quad(PaintQuad),
    /// Path 的着色随图元携带(`Path::color` 是 pub(crate),提交时经
    /// `Window::paint_path(path, color)` 注入)。
    Path(Path<Pixels>, Hsla),
    Text {
        origin: Point<Pixels>,
        /// Box 化:ShapedLine 内含 SmallVec<[DecorationRun; 32]>,裸放会把
        /// 枚举撑到 KB 级(clippy large-variant)。
        line: Box<ShapedLine>,
        line_height: Pixels,
    },
}

fn hsla(c: [f32; 4]) -> Hsla {
    Hsla::from(Rgba {
        r: c[0].clamp(0.0, 1.0),
        g: c[1].clamp(0.0, 1.0),
        b: c[2].clamp(0.0, 1.0),
        a: c[3].clamp(0.0, 1.0),
    })
}

/// alpha 乘子(节点不透明度,与 sable fill_with_opacity 同数学)。
fn mul_alpha(c: [f32; 4], opacity: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * opacity]
}

/// 翻译整张 DrawList(世界坐标 × `zoom` + 画板居中 pan → 逻辑像素)。
///
/// 返回(图元序列,翻译耗时)。**无视口剔除** —— 与 (a)/(b) 全量渲染
/// 同口径,通道对比才公平(剔除是通道无关的后续优化)。
pub fn translate(
    list: &DrawList,
    zoom: f64,
    viewport: [f64; 2],
    text_system: &WindowTextSystem,
) -> (Vec<Prim>, Duration) {
    let t0 = std::time::Instant::now();
    let [vw, vh] = viewport;
    let pan_x = (vw - list.w * zoom) / 2.0;
    let pan_y = (vh - list.h * zoom) / 2.0;
    let sx = |x: f64| px((x * zoom + pan_x) as f32);
    let sy = |y: f64| px((y * zoom + pan_y) as f32);

    let mut prims: Vec<Prim> = Vec::with_capacity(list.items.len());
    for item in &list.items {
        let [x, y, w, h] = item.rect;
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let opacity = item.opacity.clamp(0.0, 1.0);

        // —— 文本:shape_line(DirectWrite;行布局缓存,缩放后重塑一次)——
        if item.kind == vb_render::encode::DrawKind::Text {
            if let Some(hint) = &item.label {
                let mut f = font(".SystemUIFont");
                if hint.weight_bold {
                    f = f.bold();
                }
                let run = TextRun {
                    len: hint.text.len(),
                    font: f,
                    color: hsla(mul_alpha(hint.color, opacity)),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let line = text_system.shape_line(
                    hint.text.clone().into(),
                    px(hint.font_size as f32 * zoom as f32),
                    &[run],
                    None,
                );
                prims.push(Prim::Text {
                    origin: Point { x: sx(x), y: sy(y) },
                    line: Box::new(line),
                    line_height: px(hint.line_height as f32 * zoom as f32),
                });
            }
            continue;
        }

        // —— 贝塞尔路径项(fill + stroke,PathBuilder 构建期 tessellate)——
        if item.path.is_some() {
            let solid = match &item.fill {
                Some(FillDef::Solid(c)) => Some(hsla(mul_alpha(*c, opacity))),
                Some(FillDef::LinearGradient { stops, .. }) => stops.first().map(|s| {
                    // Path 只有单色:取首停靠点(保真差,ADR 记录)。
                    hsla(mul_alpha(s.color, opacity))
                }),
                _ => None,
            };
            if let Some(color) = solid {
                let mut pb = PathBuilder::fill();
                push_item_path(&mut pb, item, &sx, &sy);
                if let Ok(path) = pb.build() {
                    prims.push(Prim::Path(path, color));
                }
            }
            if let Some(border) = &item.border {
                if border.width > 0.0 {
                    let mut pb = PathBuilder::stroke(px((border.width.max(1.0) * zoom) as f32));
                    push_item_path(&mut pb, item, &sx, &sy);
                    if let Ok(path) = pb.build() {
                        prims.push(Prim::Path(path, hsla(mul_alpha(border.color, opacity))));
                    }
                }
            }
            continue;
        }

        // —— 旋转矩形:quad 无旋转 → 4 角多边形走 Path ——
        if item.rot.abs() > 1e-9 {
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
                .map(|(ax, ay)| rotate_about(ax, ay, item.rot.to_radians(), cx, cy));
            if let Some(color) = solid_fill_of(item, opacity) {
                let pts: Vec<Point<Pixels>> = corners
                    .iter()
                    .map(|&(ax, ay)| Point {
                        x: sx(ax),
                        y: sy(ay),
                    })
                    .collect();
                let mut pb = PathBuilder::fill();
                pb.add_polygon(&pts, true);
                if let Ok(path) = pb.build() {
                    prims.push(Prim::Path(path, color));
                }
            }
            if let Some(border) = &item.border {
                if border.width > 0.0 {
                    let pts: Vec<Point<Pixels>> = corners
                        .iter()
                        .map(|&(ax, ay)| Point {
                            x: sx(ax),
                            y: sy(ay),
                        })
                        .collect();
                    let mut pb = PathBuilder::stroke(px((border.width.max(1.0) * zoom) as f32));
                    pb.add_polygon(&pts, true);
                    if let Ok(path) = pb.build() {
                        prims.push(Prim::Path(path, hsla(mul_alpha(border.color, opacity))));
                    }
                }
            }
            continue;
        }

        let bounds = Bounds::new(
            Point { x: sx(x), y: sy(y) },
            size(px((w * zoom) as f32), px((h * zoom) as f32)),
        );

        // —— 椭圆:四段三次贝塞尔(kappa),PathBuilder 构建 ——
        if item.ellipse {
            if let Some(color) = solid_fill_of(item, opacity) {
                let mut pb = PathBuilder::fill();
                push_ellipse(
                    &mut pb,
                    Point { x: sx(x), y: sy(y) },
                    px((w * zoom) as f32),
                    px((h * zoom) as f32),
                );
                if let Ok(path) = pb.build() {
                    prims.push(Prim::Path(path, color));
                }
            }
            continue;
        }

        // —— 矩形:paint_quad 快路径(圆角 → Corners;渐变 → 2 停靠点近似)——
        let radii = item
            .radii
            .map(|r| px((r.clamp(0.0, w.min(h) / 2.0) * zoom) as f32));
        let mut quad = fill(bounds, background_of(item, opacity)).corner_radii(Corners {
            top_left: radii[0],
            top_right: radii[1],
            bottom_right: radii[2],
            bottom_left: radii[3],
        });
        if let Some(border) = &item.border {
            if border.width > 0.0 {
                quad = quad
                    .border_widths(Edges::all(px((border.width.max(1.0) * zoom) as f32)))
                    .border_color(hsla(mul_alpha(border.color, opacity)));
            }
        }
        prims.push(Prim::Quad(quad));
    }
    (prims, t0.elapsed())
}

/// 实心填充色(Path 用:单色;渐变取首停靠点,保真差见模块注释)。
fn solid_fill_of(item: &DrawItem, opacity: f32) -> Option<Hsla> {
    match &item.fill {
        Some(FillDef::Solid(c)) => Some(hsla(mul_alpha(*c, opacity))),
        Some(FillDef::LinearGradient { stops, .. }) => {
            stops.first().map(|s| hsla(mul_alpha(s.color, opacity)))
        }
        Some(FillDef::RadialGradient { .. }) => Some(hsla([0.5, 0.5, 0.5, opacity])),
        None => None,
    }
}

/// quad 背景:实心直映;渐变取首尾停靠点(gpui 0.2.2 只有 2 停靠点);
/// 径向渐变以中灰实心诚实降级(ADR 记录)。
fn background_of(item: &DrawItem, opacity: f32) -> Background {
    match &item.fill {
        Some(FillDef::Solid(c)) => Background::from(hsla(mul_alpha(*c, opacity))),
        Some(FillDef::LinearGradient { angle_css, stops }) => {
            let first = stops
                .first()
                .cloned()
                .unwrap_or(vb_render::encode::GradientStop {
                    pos: 0.0,
                    color: [0.0, 0.0, 0.0, 1.0],
                });
            let last = stops.last().cloned().unwrap_or_else(|| first.clone());
            let bg = linear_gradient(
                *angle_css as f32,
                linear_color_stop(
                    hsla(mul_alpha(first.color, opacity)),
                    first.pos.clamp(0.0, 1.0),
                ),
                linear_color_stop(
                    hsla(mul_alpha(last.color, opacity)),
                    last.pos.clamp(0.0, 1.0),
                ),
            );
            // 与 (a)/(b) 的 sRGB 插值对齐(gpui 默认 Oklab 插值)。
            bg.color_space(ColorSpace::Srgb)
        }
        Some(FillDef::RadialGradient { .. }) => Background::from(hsla([0.5, 0.5, 0.5, opacity])),
        None => Background::from(hsla([0.0, 0.0, 0.0, 0.0])),
    }
}

/// 绕中心旋转(与 gpu.rs item_tf 同口径)。
fn rotate_about(ax: f64, ay: f64, rad: f64, cx: f64, cy: f64) -> (f64, f64) {
    let (dx, dy) = (ax - cx, ay - cy);
    let (s, c) = rad.sin_cos();
    (cx + dx * c - dy * s, cy + dx * s + dy * c)
}

/// 椭圆 = 四段三次贝塞尔(kappa 切柄),构建期 lyon tessellation。
fn push_ellipse(pb: &mut PathBuilder, origin: Point<Pixels>, w: Pixels, h: Pixels) {
    const K: f32 = 0.552_284_5;
    let (rx, ry) = (w / 2.0, h / 2.0);
    let c = Point {
        x: origin.x + rx,
        y: origin.y + ry,
    };
    let (kx, ky) = (rx * K, ry * K);
    let pt = |x: Pixels, y: Pixels| Point { x, y };
    pb.move_to(pt(c.x + rx, c.y));
    // 四段闭合:右→下→左→上(控制柄 = kappa × 半径)。
    pb.cubic_bezier_to(
        pt(c.x + rx, c.y + ky),
        pt(c.x + kx, c.y + ry),
        pt(c.x, c.y + ry),
    );
    pb.cubic_bezier_to(
        pt(c.x - kx, c.y + ry),
        pt(c.x - rx, c.y + ky),
        pt(c.x - rx, c.y),
    );
    pb.cubic_bezier_to(
        pt(c.x - rx, c.y - ky),
        pt(c.x - kx, c.y - ry),
        pt(c.x, c.y - ry),
    );
    pb.cubic_bezier_to(
        pt(c.x + kx, c.y - ry),
        pt(c.x + rx, c.y - ky),
        pt(c.x + rx, c.y),
    );
    pb.close();
}

/// 贝塞尔路径项 → PathBuilder(元素坐标 + 矩形锚点平移)。
pub fn push_item_path(
    pb: &mut PathBuilder,
    item: &DrawItem,
    sx: &impl Fn(f64) -> Pixels,
    sy: &impl Fn(f64) -> Pixels,
) {
    if let Some(src) = &item.path {
        let [x, y, _, _] = item.rect;
        let pt = |ax: f64, ay: f64| Point {
            x: sx(ax + x),
            y: sy(ay + y),
        };
        for el in src.elements() {
            match el {
                PathEl::MoveTo(p) => pb.move_to(pt(p.x, p.y)),
                PathEl::LineTo(p) => pb.line_to(pt(p.x, p.y)),
                PathEl::QuadTo(c, p) => pb.curve_to(pt(p.x, p.y), pt(c.x, c.y)),
                PathEl::CurveTo(c1, c2, p) => {
                    pb.cubic_bezier_to(pt(p.x, p.y), pt(c1.x, c1.y), pt(c2.x, c2.y))
                }
                PathEl::ClosePath => pb.close(),
            }
        }
    }
}

/// paint 期:按序提交图元(此处不再做几何计算)。
pub fn paint_prims(prims: &[Prim], window: &mut Window, cx: &mut App) {
    for prim in prims {
        match prim {
            Prim::Quad(quad) => window.paint_quad(quad.clone()),
            Prim::Path(path, color) => window.paint_path(path.clone(), *color),
            Prim::Text {
                origin,
                line,
                line_height,
            } => {
                let _ = line.paint(*origin, *line_height, window, cx);
            }
        }
    }
}
