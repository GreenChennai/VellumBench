//! `vb_tools` — 工具层的纯几何/拾取函数(设计文档 06 篇)。
//!
//! v0.1 裁定(范围控制,关联 ADR-0017 的同类取舍):完整 `Tool` trait 状态机
//! 随 v0.3「像 AI 的编辑体验」落地;本 crate 现在提供工具与宿主共用的
//! **无副作用纯函数** —— 拾取、框选、约束、预览几何 —— 并以单元测试锁定
//! AI 行为语义(相交即选中、Shift 约束、Alt 从中心等)。

use vb_common::geom::Rect;
use vb_doc::model::{Document, Geom, NodeId, NodeKind};

/// 相机:screen = world * zoom + pan(canvas 本地像素)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub pan_x: f64,
    pub pan_y: f64,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self { pan_x: 0.0, pan_y: 0.0, zoom: 1.0 }
    }
}

impl Camera {
    pub fn world_to_screen(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.zoom + self.pan_x, y * self.zoom + self.pan_y)
    }

    pub fn screen_to_world(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.pan_x) / self.zoom, (y - self.pan_y) / self.zoom)
    }

    /// 以光标为中心缩放(zoom 保持光标下的世界点不动)。
    pub fn zoom_at(&mut self, screen_x: f64, screen_y: f64, factor: f64) {
        let (wx, wy) = self.screen_to_world(screen_x, screen_y);
        let new_zoom = (self.zoom * factor).clamp(0.01, 64.0);
        let applied = new_zoom / self.zoom;
        self.pan_x = screen_x - wx * applied;
        self.pan_y = screen_y - wy * applied;
        self.zoom = new_zoom;
    }
}

/// 节点在画板坐标系中的绝对 bbox(沿祖先链累加 x/y,止于画板)。
pub fn abs_bbox(doc: &Document, id: NodeId) -> Option<Rect> {
    let n = doc.nodes.get(id)?;
    let mut x = n.geom.x;
    let mut y = n.geom.y;
    let mut p = n.parent;
    while let Some(pid) = p {
        let pn = doc.nodes.get(pid)?;
        if matches!(pn.kind, NodeKind::Artboard) {
            break; // 坐标相对画板,画板自身原点不再累加
        }
        if !matches!(pn.kind, NodeKind::Layer) {
            x += pn.geom.x;
            y += pn.geom.y;
        }
        p = pn.parent;
    }
    Some(Rect::new(x, y, x + n.geom.w, y + n.geom.h))
}

/// 拾取:z 序从顶向下(children 末位在最上层),命中容器时优先深入子级。
pub fn hit_test(doc: &Document, artboard: NodeId, wx: f64, wy: f64) -> Option<NodeId> {
    let ab = doc.nodes.get(artboard)?;
    hit_children(doc, &ab.children, wx, wy)
}

fn hit_children(doc: &Document, ids: &[NodeId], wx: f64, wy: f64) -> Option<NodeId> {
    for &id in ids.iter().rev() {
        let Some(n) = doc.nodes.get(id) else { continue };
        if n.hidden || n.locked {
            continue;
        }
        let Some(bb) = abs_bbox(doc, id) else { continue };
        if !bb.contains((wx, wy)) {
            continue;
        }
        if !n.children.is_empty() {
            if let Some(hit) = hit_children(doc, &n.children, wx, wy) {
                return Some(hit);
            }
        }
        return Some(id);
    }
    None
}

/// 框选:**相交即选中**(AI 语义,设计文档 02 篇 §5.1)。
pub fn marquee_select(doc: &Document, artboard: NodeId, rect: Rect) -> Vec<NodeId> {
    let mut out = Vec::new();
    let Some(ab) = doc.nodes.get(artboard) else { return out };
    for &c in &ab.children {
        collect_intersect(doc, c, rect, &mut out);
    }
    out
}

fn collect_intersect(doc: &Document, id: NodeId, rect: Rect, out: &mut Vec<NodeId>) {
    let Some(n) = doc.nodes.get(id) else { return };
    if n.hidden || n.locked {
        return;
    }
    let Some(bb) = abs_bbox(doc, id) else { return };
    let inter = bb.intersect(rect);
    if inter.width() > 0.0 && inter.height() > 0.0 {
        out.push(id);
    }
    for &c in &n.children {
        collect_intersect(doc, c, rect, out);
    }
}

/// Shift 约束:锁定更接近的轴(移动方向 45° 约束,与 AI 一致)。
pub fn constrain_axis(dx: f64, dy: f64, shift: bool) -> (f64, f64) {
    if !shift {
        return (dx, dy);
    }
    if dx.abs() >= dy.abs() {
        (dx, 0.0)
    } else {
        (0.0, dy)
    }
}

/// 矩形/椭圆工具拖拽:起点 → 当前点;`shift`=正方形/正圆,`alt`=从中心。
pub fn drag_rect_geom(sx: f64, sy: f64, cx: f64, cy: f64, shift: bool, alt: bool) -> Geom {
    let (mut x0, mut y0) = (sx, sy);
    let (mut x1, mut y1) = (cx, cy);
    let (mut w) = (x1 - x0).abs();
    let (mut h) = (y1 - y0).abs();
    if shift {
        let s = w.max(h);
        w = s;
        h = s;
    }
    if alt {
        // 从中心:起点即中心
        let half_w = w;
        let half_h = h;
        w *= 2.0;
        h *= 2.0;
        x0 = sx - half_w;
        y0 = sy - half_h;
        let _ = x1;
        let _ = y1;
        x1 = sx + half_w;
        y1 = sy + half_h;
    } else {
        // 归一化到左上角
        if x1 < x0 {
            std::mem::swap(&mut x0, &mut x1);
        }
        if y1 < y0 {
            std::mem::swap(&mut y0, &mut y1);
        }
        // shift 时按拖拽方向对齐正方形
        if shift {
            if cx < sx {
                x0 = sx - w;
            }
            if cy < sy {
                y0 = sy - h;
            }
        }
    }
    let _ = (x1, y1);
    Geom { x: x0, y: y0, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::model::{Document, Node, NodeKind};

    fn add_box(doc: &mut Document, parent: NodeId, x: f64, y: f64, w: f64, h: f64) -> NodeId {
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, "盒", sid);
        n.geom = Geom { x, y, w, h };
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        id
    }

    #[test]
    fn camera_zoom_at_keeps_cursor_point() {
        let mut cam = Camera { pan_x: 40.0, pan_y: 0.0, zoom: 1.0 };
        let (sx, sy) = (300.0, 200.0);
        let (wx, wy) = cam.screen_to_world(sx, sy);
        cam.zoom_at(sx, sy, 2.0);
        let (wx2, wy2) = cam.screen_to_world(sx, sy);
        assert!((wx - wx2).abs() < 1e-9);
        assert!((wy - wy2).abs() < 1e-9);
        assert_eq!(cam.zoom, 2.0);
    }

    #[test]
    fn constrain_axis_locks_major_axis() {
        assert_eq!(constrain_axis(30.0, 4.0, true), (30.0, 0.0));
        assert_eq!(constrain_axis(2.0, 50.0, true), (0.0, 50.0));
        assert_eq!(constrain_axis(30.0, 4.0, false), (30.0, 4.0));
    }

    #[test]
    fn drag_rect_shift_and_alt() {
        // Shift:正方形,面积取大轴
        let g = drag_rect_geom(0.0, 0.0, 100.0, 40.0, true, false);
        assert_eq!((g.x, g.y, g.w, g.h), (0.0, 0.0, 100.0, 100.0));
        // Alt:从中心
        let g = drag_rect_geom(100.0, 100.0, 140.0, 120.0, false, true);
        assert_eq!((g.x, g.y, g.w, g.h), (60.0, 80.0, 80.0, 40.0));
        // 反向拖拽归一化
        let g = drag_rect_geom(100.0, 100.0, 40.0, 60.0, false, false);
        assert_eq!((g.x, g.y, g.w, g.h), (40.0, 60.0, 60.0, 40.0));
    }
}
