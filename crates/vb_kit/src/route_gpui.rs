//! 路线 (c):`vb_render::DrawList` → GPUI 原生绘制原语翻译层。
//!
//! 这是从 R0 canvas spike 提升的共享渲染适配;画布视图与 spike 共用同一
//! 翻译路径。GPUI 原生 Path 逐帧 CPU tessellation,渐变/裁剪/滤镜有明确
//! 保真差,由上层画布提示,不可宣称为完整文档渲染器。

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use sable::gpui::{
    fill, font, linear_color_stop, linear_gradient, px, size, App, Background, Bounds, ColorSpace,
    Corners, Edges, Hsla, PaintQuad, Path, PathBuilder, Pixels, Point, Rgba, ShapedLine, TextRun,
    Window, WindowTextSystem,
};
use sable::kurbo::PathEl;
use vb_render::encode::{BitmapData, DrawItem, DrawKind, DrawList, FillDef};

/// 一帧的 GPUI 原语(构建于 render 期,paint 期零几何计算)。
pub enum Prim {
    Quad(PaintQuad),
    /// Path 的着色随图元携带。
    Path(Path<Pixels>, Hsla),
    /// 已解码位图;资源转换经 Sable 的唯一 RGBA→RenderImage 桥。
    Image {
        bounds: Bounds<Pixels>,
        corners: Corners<Pixels>,
        image: Arc<sable::gpui::RenderImage>,
    },
    Text {
        origin: Point<Pixels>,
        line: Box<ShapedLine>,
        line_height: Pixels,
    },
}

/// 画布级 RenderImage 缓存,避免每次视口重绘都重建纹理帧。
#[derive(Default)]
pub(crate) struct ImageCache {
    entries: HashMap<usize, CachedImage>,
    bytes: usize,
}

struct CachedImage {
    rgba: Arc<Vec<u8>>,
    width: u32,
    height: u32,
    image: Arc<sable::gpui::RenderImage>,
}

impl ImageCache {
    const MAX_ENTRIES: usize = 32;
    const MAX_BYTES: usize = 256 * 1024 * 1024;

    pub(crate) fn get_or_create(
        &mut self,
        bitmap: &BitmapData,
    ) -> Option<Arc<sable::gpui::RenderImage>> {
        // BitmapData's Arc is stable for the lifetime of a DrawList. Holding it
        // in CachedImage prevents pointer reuse, without retaining long data: URLs.
        let key = Arc::as_ptr(&bitmap.rgba) as usize;
        if let Some(cached) = self.entries.get(&key) {
            if cached.width == bitmap.width && cached.height == bitmap.height {
                return Some(cached.image.clone());
            }
        }
        if let Some(previous) = self.entries.remove(&key) {
            self.bytes = self.bytes.saturating_sub(previous.rgba.len());
        }
        let image = sable::canvas::gpui_element::rgba_to_render_image(
            bitmap.rgba.as_ref().clone(),
            bitmap.width,
            bitmap.height,
        )?;
        let byte_len = bitmap.rgba.len();
        if byte_len <= Self::MAX_BYTES {
            if self.entries.len() >= Self::MAX_ENTRIES
                || self.bytes.saturating_add(byte_len) > Self::MAX_BYTES
            {
                self.entries.clear();
                self.bytes = 0;
            }
            self.bytes = self.bytes.saturating_add(byte_len);
            self.entries.insert(
                key,
                CachedImage {
                    rgba: bitmap.rgba.clone(),
                    width: bitmap.width,
                    height: bitmap.height,
                    image: image.clone(),
                },
            );
        }
        Some(image)
    }
}

/// 文档坐标到 GPUI 视口的屏幕变换。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasTransform {
    /// 文档缩放比例。
    pub zoom: f64,
    /// 视口宽高,单位为逻辑像素。
    pub viewport: [f64; 2],
    /// 视口窗口原点,单位为逻辑像素。
    pub viewport_origin: [f64; 2],
    /// 用户平移,单位为屏幕逻辑像素。
    pub pan_offset: [f64; 2],
}

fn hsla(c: [f32; 4]) -> Hsla {
    Hsla::from(Rgba {
        r: c[0].clamp(0.0, 1.0),
        g: c[1].clamp(0.0, 1.0),
        b: c[2].clamp(0.0, 1.0),
        a: c[3].clamp(0.0, 1.0),
    })
}

fn mul_alpha(c: [f32; 4], opacity: f32) -> [f32; 4] {
    [c[0], c[1], c[2], c[3] * opacity]
}

/// 用 Sable token accent 画交互中的矩形轮廓;只产出临时图元,不进入文档。
pub fn draft_rectangle(
    list: &DrawList,
    rect: [f64; 4],
    zoom: f64,
    viewport: [f64; 2],
    viewport_origin: [f64; 2],
    pan_offset: [f64; 2],
    color: Hsla,
) -> Option<Prim> {
    if !zoom.is_finite()
        || zoom <= 0.0
        || viewport
            .iter()
            .chain([list.w, list.h].iter())
            .any(|value| !value.is_finite() || *value <= 0.0)
        || rect.iter().any(|value| !value.is_finite())
        || rect[2] <= 0.0
        || rect[3] <= 0.0
    {
        return None;
    }

    let [x, y, width, height] = rect;
    let [viewport_width, viewport_height] = viewport;
    let [origin_x, origin_y] = viewport_origin;
    let [pan_x, pan_y] = pan_offset;
    let sx = |value| axis_position_with_pan(value, zoom, viewport_width, list.w, origin_x, pan_x);
    let sy = |value| axis_position_with_pan(value, zoom, viewport_height, list.h, origin_y, pan_y);
    let mut path = PathBuilder::stroke(px(1.5));
    path.move_to(Point { x: sx(x), y: sy(y) });
    path.line_to(Point {
        x: sx(x + width),
        y: sy(y),
    });
    path.line_to(Point {
        x: sx(x + width),
        y: sy(y + height),
    });
    path.line_to(Point {
        x: sx(x),
        y: sy(y + height),
    });
    path.close();
    path.build().ok().map(|path| Prim::Path(path, color))
}

/// 用 Sable token accent 画交互中的椭圆轮廓;只产出临时图元,不进入文档。
pub fn draft_ellipse(
    list: &DrawList,
    rect: [f64; 4],
    zoom: f64,
    viewport: [f64; 2],
    viewport_origin: [f64; 2],
    pan_offset: [f64; 2],
    color: Hsla,
) -> Option<Prim> {
    if !zoom.is_finite()
        || zoom <= 0.0
        || viewport
            .iter()
            .chain([list.w, list.h].iter())
            .any(|value| !value.is_finite() || *value <= 0.0)
        || rect.iter().any(|value| !value.is_finite())
        || rect[2] <= 0.0
        || rect[3] <= 0.0
    {
        return None;
    }

    let [x, y, width, height] = rect;
    let [viewport_width, viewport_height] = viewport;
    let [origin_x, origin_y] = viewport_origin;
    let [pan_x, pan_y] = pan_offset;
    let origin = Point {
        x: axis_position_with_pan(x, zoom, viewport_width, list.w, origin_x, pan_x),
        y: axis_position_with_pan(y, zoom, viewport_height, list.h, origin_y, pan_y),
    };
    let scaled_width = width * zoom;
    let scaled_height = height * zoom;
    if !scaled_width.is_finite()
        || !scaled_height.is_finite()
        || scaled_width > f64::from(f32::MAX)
        || scaled_height > f64::from(f32::MAX)
    {
        return None;
    }

    let mut path = PathBuilder::stroke(px(1.5));
    push_ellipse(
        &mut path,
        origin,
        px(scaled_width as f32),
        px(scaled_height as f32),
    );
    path.build().ok().map(|path| Prim::Path(path, color))
}

fn axis_position_with_pan(
    value: f64,
    zoom: f64,
    viewport_size: f64,
    document_size: f64,
    origin: f64,
    pan_offset: f64,
) -> Pixels {
    let centered_offset = (viewport_size - document_size * zoom) / 2.0;
    px((value * zoom + centered_offset + origin + pan_offset) as f32)
}

/// 翻译整张 DrawList,视口原点默认为窗口左上角(0, 0)。
///
/// 返回(图元序列,翻译耗时)。当前无视口剔除;复杂效果的降级由
/// `canvas_panel::preview_limitation_count` 统计并向用户提示。
pub fn translate(
    list: &DrawList,
    zoom: f64,
    viewport: [f64; 2],
    text_system: &WindowTextSystem,
) -> (Vec<Prim>, Duration) {
    translate_at(list, zoom, viewport, [0.0, 0.0], text_system)
}

/// 翻译到位于窗口非零原点的 GPUI 视口(如 Dock 中央画布面板)。
pub fn translate_at(
    list: &DrawList,
    zoom: f64,
    viewport: [f64; 2],
    viewport_origin: [f64; 2],
    text_system: &WindowTextSystem,
) -> (Vec<Prim>, Duration) {
    translate_with_pan(
        list,
        zoom,
        viewport,
        viewport_origin,
        [0.0, 0.0],
        text_system,
    )
}

/// 翻译到指定原点并叠加用户平移量(屏幕像素)。
pub fn translate_with_pan(
    list: &DrawList,
    zoom: f64,
    viewport: [f64; 2],
    viewport_origin: [f64; 2],
    pan_offset: [f64; 2],
    text_system: &WindowTextSystem,
) -> (Vec<Prim>, Duration) {
    translate_with_selection(
        list,
        CanvasTransform {
            zoom,
            viewport,
            viewport_origin,
            pan_offset,
        },
        &[],
        [0.0, 0.0],
        text_system,
    )
}

fn offset_for_sid(selected_sids: &HashSet<&str>, sid: &str, selected_offset: [f64; 2]) -> [f64; 2] {
    if selected_sids.contains(sid) {
        selected_offset
    } else {
        [0.0, 0.0]
    }
}

/// 渲染选中节点的拖动预览,只改变临时画布变换,不复制/修改 DrawList。
pub fn translate_with_selection(
    list: &DrawList,
    transform: CanvasTransform,
    selected_sids: &[String],
    selected_offset: [f64; 2],
    text_system: &WindowTextSystem,
) -> (Vec<Prim>, Duration) {
    translate_with_cache(
        list,
        transform,
        selected_sids,
        selected_offset,
        &mut ImageCache::default(),
        text_system,
    )
}

pub(crate) fn translate_with_cache(
    list: &DrawList,
    transform: CanvasTransform,
    selected_sids: &[String],
    selected_offset: [f64; 2],
    image_cache: &mut ImageCache,
    text_system: &WindowTextSystem,
) -> (Vec<Prim>, Duration) {
    let t0 = std::time::Instant::now();
    let CanvasTransform {
        zoom,
        viewport,
        viewport_origin,
        pan_offset,
    } = transform;
    let [vw, vh] = viewport;
    let [origin_x, origin_y] = viewport_origin;
    let [pan_x, pan_y] = pan_offset;
    let selected_sids: HashSet<&str> = selected_sids.iter().map(String::as_str).collect();
    let selected_offset = if selected_offset.iter().all(|value| value.is_finite()) {
        selected_offset
    } else {
        [0.0, 0.0]
    };

    let mut prims: Vec<Prim> = Vec::with_capacity(list.items.len());
    for item in &list.items {
        let offset = offset_for_sid(&selected_sids, item.sid.as_str(), selected_offset);
        let sx = |x: f64| axis_position_with_pan(x + offset[0], zoom, vw, list.w, origin_x, pan_x);
        let sy = |y: f64| axis_position_with_pan(y + offset[1], zoom, vh, list.h, origin_y, pan_y);
        let [x, y, w, h] = item.rect;
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let opacity = item.opacity.clamp(0.0, 1.0);

        if item.kind == DrawKind::Image {
            if let Some(bitmap) = &item.image {
                if let Some(image) = image_cache.get_or_create(bitmap) {
                    let bounds = Bounds::new(
                        Point { x: sx(x), y: sy(y) },
                        size(px((w * zoom) as f32), px((h * zoom) as f32)),
                    );
                    let radii = item
                        .radii
                        .map(|radius| px((radius.clamp(0.0, w.min(h) / 2.0) * zoom) as f32));
                    prims.push(Prim::Image {
                        bounds,
                        corners: Corners {
                            top_left: radii[0],
                            top_right: radii[1],
                            bottom_right: radii[2],
                            bottom_left: radii[3],
                        },
                        image,
                    });
                }
            }
            continue;
        }

        if item.kind == DrawKind::Text {
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

        if item.path.is_some() {
            let solid = match &item.fill {
                Some(FillDef::Solid(c)) => Some(hsla(mul_alpha(*c, opacity))),
                Some(FillDef::LinearGradient { stops, .. }) => {
                    stops.first().map(|s| hsla(mul_alpha(s.color, opacity)))
                }
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

fn grid_spacing_for_zoom(zoom: f64) -> f64 {
    if !zoom.is_finite() || zoom <= 0.0 {
        return 20.0;
    }
    let zoom = zoom.clamp(0.05, 8.0);
    let mut spacing = 20.0;
    while spacing * zoom < 12.0 {
        spacing *= 2.0;
    }
    while spacing * zoom > 48.0 {
        spacing *= 0.5;
    }
    spacing
}

fn grid_axis_positions(
    world_zero_screen: f64,
    viewport_start: f64,
    viewport_length: f64,
    step_screen: f64,
) -> Vec<f64> {
    if !world_zero_screen.is_finite()
        || !viewport_start.is_finite()
        || !viewport_length.is_finite()
        || !step_screen.is_finite()
        || viewport_length <= 0.0
        || step_screen <= 0.0
    {
        return Vec::new();
    }
    let first = ((viewport_start - world_zero_screen) / step_screen).ceil();
    let last = ((viewport_start + viewport_length - world_zero_screen) / step_screen).floor();
    if !first.is_finite() || !last.is_finite() || last < first || last - first > 4096.0 {
        return Vec::new();
    }
    (first as i64..=last as i64)
        .map(|index| world_zero_screen + index as f64 * step_screen)
        .collect()
}

/// Draw a document-origin-aligned adaptive grid behind the projected artwork.
pub fn grid_primitives(list: &DrawList, transform: CanvasTransform, color: Hsla) -> Vec<Prim> {
    let CanvasTransform {
        zoom,
        viewport,
        viewport_origin,
        pan_offset,
    } = transform;
    let [viewport_width, viewport_height] = viewport;
    let [origin_x, origin_y] = viewport_origin;
    if !list.w.is_finite()
        || !list.h.is_finite()
        || list.w <= 0.0
        || list.h <= 0.0
        || !zoom.is_finite()
        || zoom <= 0.0
        || !viewport_width.is_finite()
        || !viewport_height.is_finite()
        || viewport_width <= 0.0
        || viewport_height <= 0.0
    {
        return Vec::new();
    }

    let step_world = grid_spacing_for_zoom(zoom);
    let step_screen = step_world * zoom;
    let world_zero_x = f64::from(axis_position_with_pan(
        0.0,
        zoom,
        viewport_width,
        list.w,
        origin_x,
        pan_offset[0],
    ));
    let world_zero_y = f64::from(axis_position_with_pan(
        0.0,
        zoom,
        viewport_height,
        list.h,
        origin_y,
        pan_offset[1],
    ));
    let vertical = grid_axis_positions(world_zero_x, origin_x, viewport_width, step_screen);
    let horizontal = grid_axis_positions(world_zero_y, origin_y, viewport_height, step_screen);
    if vertical.is_empty() && horizontal.is_empty() {
        return Vec::new();
    }

    let mut builder = PathBuilder::stroke(px(1.0));
    for x in vertical {
        builder.move_to(Point {
            x: px(x as f32),
            y: px(origin_y as f32),
        });
        builder.line_to(Point {
            x: px(x as f32),
            y: px((origin_y + viewport_height) as f32),
        });
    }
    for y in horizontal {
        builder.move_to(Point {
            x: px(origin_x as f32),
            y: px(y as f32),
        });
        builder.line_to(Point {
            x: px((origin_x + viewport_width) as f32),
            y: px(y as f32),
        });
    }
    builder
        .build()
        .map_or_else(|_| Vec::new(), |path| vec![Prim::Path(path, color)])
}

/// 将画板本地吸附参考线转换成固定屏幕像素宽度的 GPUI 描边。
pub fn snap_guide_primitives(
    list: &DrawList,
    transform: CanvasTransform,
    guides: &[[f64; 4]],
    color: Hsla,
) -> Vec<Prim> {
    let CanvasTransform {
        zoom,
        viewport,
        viewport_origin,
        pan_offset,
    } = transform;
    let [viewport_width, viewport_height] = viewport;
    let [origin_x, origin_y] = viewport_origin;
    if guides.is_empty()
        || !zoom.is_finite()
        || zoom <= 0.0
        || !list.w.is_finite()
        || !list.h.is_finite()
        || !viewport_width.is_finite()
        || !viewport_height.is_finite()
        || viewport_width <= 0.0
        || viewport_height <= 0.0
    {
        return Vec::new();
    }

    let screen_x =
        |x: f64| axis_position_with_pan(x, zoom, viewport_width, list.w, origin_x, pan_offset[0]);
    let screen_y =
        |y: f64| axis_position_with_pan(y, zoom, viewport_height, list.h, origin_y, pan_offset[1]);
    let mut builder = PathBuilder::stroke(px(1.0));
    let mut added = false;
    for [x0, y0, x1, y1] in guides {
        if [x0, y0, x1, y1].iter().any(|value| !value.is_finite()) {
            continue;
        }
        builder.move_to(Point {
            x: screen_x(*x0),
            y: screen_y(*y0),
        });
        builder.line_to(Point {
            x: screen_x(*x1),
            y: screen_y(*y1),
        });
        added = true;
    }
    if !added {
        return Vec::new();
    }
    builder
        .build()
        .map_or_else(|_| Vec::new(), |path| vec![Prim::Path(path, color)])
}

/// 构造与原语几何一致的选择描边(固定屏幕像素宽度)。
pub fn selection_outline(
    list: &DrawList,
    item: &DrawItem,
    zoom: f64,
    viewport: [f64; 2],
    viewport_origin: [f64; 2],
    pan_offset: [f64; 2],
    color: Hsla,
) -> Option<Prim> {
    selection_outline_with_offset(
        list,
        item,
        CanvasTransform {
            zoom,
            viewport,
            viewport_origin,
            pan_offset,
        },
        [0.0, 0.0],
        color,
    )
}

/// 构造带临时文档平移偏移的选择描边。
pub fn selection_outline_with_offset(
    list: &DrawList,
    item: &DrawItem,
    transform: CanvasTransform,
    object_offset: [f64; 2],
    color: Hsla,
) -> Option<Prim> {
    let CanvasTransform {
        zoom,
        viewport,
        viewport_origin,
        pan_offset,
    } = transform;
    let [vw, vh] = viewport;
    let [origin_x, origin_y] = viewport_origin;
    let [pan_x, pan_y] = pan_offset;
    let object_offset = if object_offset.iter().all(|value| value.is_finite()) {
        object_offset
    } else {
        [0.0, 0.0]
    };
    let sx =
        |x: f64| axis_position_with_pan(x + object_offset[0], zoom, vw, list.w, origin_x, pan_x);
    let sy =
        |y: f64| axis_position_with_pan(y + object_offset[1], zoom, vh, list.h, origin_y, pan_y);
    let [x, y, w, h] = item.rect;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }

    let mut builder = PathBuilder::stroke(px(2.0));
    if item.path.is_some() {
        push_item_path(&mut builder, item, &sx, &sy);
    } else if item.rot.abs() > 1e-9 {
        let (cx, cy) = (x + w / 2.0, y + h / 2.0);
        let points: Vec<Point<Pixels>> = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]
            .map(|(px, py)| rotate_about(px, py, item.rot.to_radians(), cx, cy))
            .iter()
            .map(|&(px, py)| Point {
                x: sx(px),
                y: sy(py),
            })
            .collect();
        builder.add_polygon(&points, true);
    } else if item.ellipse {
        push_ellipse(
            &mut builder,
            Point { x: sx(x), y: sy(y) },
            px((w * zoom) as f32),
            px((h * zoom) as f32),
        );
    } else {
        let points = [
            Point { x: sx(x), y: sy(y) },
            Point {
                x: sx(x + w),
                y: sy(y),
            },
            Point {
                x: sx(x + w),
                y: sy(y + h),
            },
            Point {
                x: sx(x),
                y: sy(y + h),
            },
        ];
        builder.add_polygon(&points, true);
    }
    builder.build().ok().map(|path| Prim::Path(path, color))
}

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
            linear_gradient(
                *angle_css as f32,
                linear_color_stop(
                    hsla(mul_alpha(first.color, opacity)),
                    first.pos.clamp(0.0, 1.0),
                ),
                linear_color_stop(
                    hsla(mul_alpha(last.color, opacity)),
                    last.pos.clamp(0.0, 1.0),
                ),
            )
            .color_space(ColorSpace::Srgb)
        }
        Some(FillDef::RadialGradient { .. }) => Background::from(hsla([0.5, 0.5, 0.5, opacity])),
        None => Background::from(hsla([0.0, 0.0, 0.0, 0.0])),
    }
}

fn rotate_about(ax: f64, ay: f64, rad: f64, cx: f64, cy: f64) -> (f64, f64) {
    let (dx, dy) = (ax - cx, ay - cy);
    let (s, c) = rad.sin_cos();
    (cx + dx * c - dy * s, cy + dx * s + dy * c)
}

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

/// paint 期按序提交图元(此处不再做几何计算)。
pub fn paint_prims(prims: &[Prim], window: &mut Window, cx: &mut App) {
    for prim in prims {
        match prim {
            Prim::Quad(quad) => window.paint_quad(quad.clone()),
            Prim::Path(path, color) => window.paint_path(path.clone(), *color),
            Prim::Image {
                bounds,
                corners,
                image,
            } => {
                let _ = window.paint_image(*bounds, *corners, image.clone(), 0, false);
            }
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

#[cfg(test)]
mod tests {
    use super::{
        axis_position_with_pan, draft_ellipse, draft_rectangle, grid_axis_positions,
        grid_primitives, grid_spacing_for_zoom, offset_for_sid, snap_guide_primitives,
        CanvasTransform, ImageCache, Prim,
    };
    use std::collections::HashSet;
    use std::sync::Arc;

    use sable::gpui::{Hsla, Rgba};
    use vb_render::encode::{BitmapData, DrawList};

    #[test]
    fn image_cache_reuses_converted_frames_and_skips_large_data_url_keys() {
        let rgba = Arc::new(vec![255, 0, 0, 255]);
        let bitmap = BitmapData {
            width: 1,
            height: 1,
            rgba,
        };
        let mut cache = ImageCache::default();
        let first = cache
            .get_or_create(&bitmap)
            .expect("valid RGBA becomes a GPUI image");
        let second = cache
            .get_or_create(&bitmap)
            .expect("cached GPUI image exists");
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.entries.len(), 1);

        let distinct = BitmapData {
            width: 1,
            height: 1,
            rgba: Arc::new(vec![0, 255, 0, 255]),
        };
        cache
            .get_or_create(&distinct)
            .expect("distinct embedded bitmap renders");
        assert_eq!(cache.entries.len(), 2);
    }

    #[test]
    fn adaptive_grid_spacing_remains_legible_across_zoom_levels() {
        for zoom in [0.05, 0.25, 1.0, 8.0] {
            let screen_spacing = grid_spacing_for_zoom(zoom) * zoom;
            assert!((12.0..=48.0).contains(&screen_spacing));
        }
    }

    #[test]
    fn grid_spacing_clamps_extreme_positive_zoom_before_search() {
        assert_eq!(grid_spacing_for_zoom(f64::MIN_POSITIVE), 320.0);
        assert_eq!(grid_spacing_for_zoom(f64::MAX), 5.0);
    }

    #[test]
    fn grid_lines_stay_aligned_to_document_origin_after_pan() {
        assert_eq!(
            grid_axis_positions(45.0, 0.0, 100.0, 20.0),
            vec![5.0, 25.0, 45.0, 65.0, 85.0]
        );
        assert_eq!(
            grid_axis_positions(45.0, 0.0, 100.0, 0.0),
            Vec::<f64>::new()
        );
    }

    #[test]
    fn snap_guides_render_as_path_and_ignore_invalid_segments() {
        let list = DrawList {
            w: 100.0,
            h: 80.0,
            background: [0.0; 4],
            items: Vec::new(),
        };
        let transform = CanvasTransform {
            zoom: 1.0,
            viewport: [160.0, 120.0],
            viewport_origin: [10.0, 20.0],
            pan_offset: [0.0, 0.0],
        };
        let color = Hsla::from(Rgba {
            r: 0.2,
            g: 0.8,
            b: 1.0,
            a: 0.9,
        });
        let guides = [[50.0, 0.0, 50.0, 80.0], [f64::NAN, 0.0, 1.0, 1.0]];

        let prims = snap_guide_primitives(&list, transform, &guides, color);
        assert!(matches!(prims.as_slice(), [Prim::Path(_, _)]));
        assert!(snap_guide_primitives(
            &list,
            CanvasTransform {
                zoom: 0.0,
                ..transform
            },
            &guides,
            color,
        )
        .is_empty());
    }

    #[test]
    fn grid_primitives_emit_one_world_aligned_path_and_reject_invalid_zoom() {
        let list = DrawList {
            w: 100.0,
            h: 80.0,
            background: [0.0; 4],
            items: Vec::new(),
        };
        let transform = CanvasTransform {
            zoom: 1.0,
            viewport: [160.0, 120.0],
            viewport_origin: [10.0, 20.0],
            pan_offset: [0.0, 0.0],
        };
        let color = Hsla::from(Rgba {
            r: 0.3,
            g: 0.4,
            b: 0.5,
            a: 0.25,
        });
        let prims = grid_primitives(&list, transform, color);
        assert!(matches!(prims.as_slice(), [Prim::Path(_, _)]));
        assert!(grid_primitives(
            &list,
            CanvasTransform {
                zoom: 0.0,
                ..transform
            },
            color,
        )
        .is_empty());
        for zoom in [f64::MIN_POSITIVE, f64::MAX] {
            assert!(
                grid_primitives(&list, CanvasTransform { zoom, ..transform }, color,).is_empty()
            );
        }
    }

    #[test]
    fn selection_preview_offsets_only_selected_stable_ids() {
        let selected = HashSet::from(["rect-a", "rect-b"]);
        assert_eq!(
            offset_for_sid(&selected, "rect-a", [12.0, -7.0]),
            [12.0, -7.0]
        );
        assert_eq!(
            offset_for_sid(&selected, "rect-b", [12.0, -7.0]),
            [12.0, -7.0]
        );
        assert_eq!(
            offset_for_sid(&selected, "untouched", [12.0, -7.0]),
            [0.0, 0.0]
        );
    }

    #[test]
    fn draft_rectangle_uses_token_color_and_rejects_invalid_geometry() {
        let list = DrawList {
            w: 100.0,
            h: 100.0,
            background: [0.0; 4],
            items: Vec::new(),
        };
        let color = Hsla::from(Rgba {
            r: 0.31,
            g: 0.62,
            b: 1.0,
            a: 1.0,
        });
        assert!(draft_rectangle(
            &list,
            [0.0, 0.0, 0.0, 20.0],
            1.0,
            [300.0, 200.0],
            [500.0, 100.0],
            [0.0, 0.0],
            color,
        )
        .is_none());
        assert!(matches!(
            draft_rectangle(
                &list,
                [10.0, 10.0, 20.0, 20.0],
                1.0,
                [300.0, 200.0],
                [500.0, 100.0],
                [0.0, 0.0],
                color,
            ),
            Some(Prim::Path(_, _))
        ));
    }

    #[test]
    fn draft_ellipse_rejects_invalid_geometry_and_builds_outline() {
        let list = DrawList {
            w: 100.0,
            h: 100.0,
            background: [0.0; 4],
            items: Vec::new(),
        };
        let color = Hsla::from(Rgba {
            r: 0.31,
            g: 0.62,
            b: 1.0,
            a: 1.0,
        });
        let viewport = [300.0, 200.0];
        let origin = [500.0, 100.0];
        let pan = [0.0, 0.0];

        assert!(draft_ellipse(
            &list,
            [10.0, 10.0, 0.0, 20.0],
            1.0,
            viewport,
            origin,
            pan,
            color,
        )
        .is_none());
        assert!(matches!(
            draft_ellipse(
                &list,
                [10.0, 10.0, 20.0, 30.0],
                1.0,
                viewport,
                origin,
                pan,
                color,
            ),
            Some(Prim::Path(_, _))
        ));
    }

    #[test]
    fn centers_artboard_inside_nonzero_viewport_origin() {
        let centered = f64::from(axis_position_with_pan(0.0, 1.0, 300.0, 100.0, 0.0, 0.0));
        let shifted = f64::from(axis_position_with_pan(0.0, 1.0, 300.0, 100.0, 420.0, 0.0));

        assert!((centered - 100.0).abs() < 0.01);
        assert!((shifted - 520.0).abs() < 0.01);
        assert!((shifted - centered - 420.0).abs() < 0.01);
    }

    #[test]
    fn pan_offset_moves_artboard_in_screen_pixels() {
        let centered = f64::from(axis_position_with_pan(25.0, 2.0, 300.0, 100.0, 420.0, 0.0));
        let panned = f64::from(axis_position_with_pan(
            25.0, 2.0, 300.0, 100.0, 420.0, -36.0,
        ));

        assert!((centered - 520.0).abs() < 0.01);
        assert!((panned - (centered - 36.0)).abs() < 0.01);
    }
}
