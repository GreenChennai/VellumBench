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
        Self {
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
        }
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

/// 节点所属画板(沿 parent 链上溯);节点自身是画板时返回自身。
pub fn artboard_of(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut cur = Some(id);
    while let Some(cid) = cur {
        let n = doc.nodes.get(cid)?;
        if matches!(n.kind, NodeKind::Artboard) {
            return Some(cid);
        }
        cur = n.parent;
    }
    None
}

/// 节点的**世界坐标** bbox(画板原点 + abs_bbox;画板自身 geom 即世界坐标)。
/// 拾取/框选/叠加层绘制与相机(世界系)交互时必须用这一口径。
pub fn abs_bbox_world(doc: &Document, id: NodeId) -> Option<Rect> {
    let bb = abs_bbox(doc, id)?;
    // 画板自身的 geom 就是世界坐标,不再叠加原点
    if doc
        .nodes
        .get(id)
        .is_some_and(|n| matches!(n.kind, NodeKind::Artboard))
    {
        return Some(bb);
    }
    let origin = artboard_of(doc, id).map(|ab| doc.artboard_origin(ab));
    match origin {
        Some((ox, oy)) => Some(Rect::new(bb.x0 + ox, bb.y0 + oy, bb.x1 + ox, bb.y1 + oy)),
        None => Some(bb),
    }
}

/// 拾取:z 序从顶向下(children 末位在最上层),命中容器时优先深入子级。
/// `(wx, wy)` 是**世界坐标**(相机系);内部换算成画板本地再比较。
pub fn hit_test(doc: &Document, artboard: NodeId, wx: f64, wy: f64) -> Option<NodeId> {
    let ab = doc.nodes.get(artboard)?;
    let (ox, oy) = doc.artboard_origin(artboard);
    hit_children(doc, &ab.children, wx - ox, wy - oy)
}

fn hit_children(doc: &Document, ids: &[NodeId], wx: f64, wy: f64) -> Option<NodeId> {
    for &id in ids.iter().rev() {
        let Some(n) = doc.nodes.get(id) else { continue };
        if n.hidden || n.locked || n.tag == "#text" {
            continue;
        }
        let Some(bb) = abs_bbox(doc, id) else {
            continue;
        };
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

/// 子树拾取:在 `root` 的直接子级中命中(局部坐标 = root 的 abs 坐标系)。
/// 隔离模式用:root = 隔离组,lx/ly = 画板本地坐标(与 abs_bbox 同帧)。
pub fn hit_test_root(doc: &Document, root: NodeId, lx: f64, ly: f64) -> Option<NodeId> {
    let n = doc.nodes.get(root)?;
    hit_children(doc, &n.children, lx, ly)
}

/// 子树框选:`rect_local` 与 `root` 子级的 abs bbox(同一局部帧)求交。
pub fn marquee_select_root(doc: &Document, root: NodeId, rect_local: Rect) -> Vec<NodeId> {
    let mut out = Vec::new();
    let Some(n) = doc.nodes.get(root) else {
        return out;
    };
    for &c in &n.children {
        collect_intersect(doc, c, rect_local, &mut out);
    }
    out
}

/// 框选:**相交即选中**(AI 语义,设计文档 02 篇 §5.1)。
/// `rect` 是**世界坐标**;只选中相交的**顶层**对象(命中父级不再深入,
/// 否则组与子孙同时入选,删除/编组/对齐都会连锁出错)。
pub fn marquee_select(doc: &Document, artboard: NodeId, rect: Rect) -> Vec<NodeId> {
    let mut out = Vec::new();
    let Some(ab) = doc.nodes.get(artboard) else {
        return out;
    };
    let (ox, oy) = doc.artboard_origin(artboard);
    let local = Rect::new(rect.x0 - ox, rect.y0 - oy, rect.x1 - ox, rect.y1 - oy);
    for &c in &ab.children {
        collect_intersect(doc, c, local, &mut out);
    }
    out
}

fn collect_intersect(doc: &Document, id: NodeId, rect: Rect, out: &mut Vec<NodeId>) {
    let Some(n) = doc.nodes.get(id) else { return };
    if n.hidden || n.locked || n.tag == "#text" {
        return;
    }
    let Some(bb) = abs_bbox(doc, id) else { return };
    let inter = bb.intersect(rect);
    if inter.width() > 0.0 && inter.height() > 0.0 {
        out.push(id);
        return; // 顶层语义:父级已命中,不再把子孙一并选中
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
    let mut w = (x1 - x0).abs();
    let mut h = (y1 - y0).abs();
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

    #[allow(dead_code)]
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
        let mut cam = Camera {
            pan_x: 40.0,
            pan_y: 0.0,
            zoom: 1.0,
        };
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

    /// 多画板:拾取/框选必须用世界坐标(此前第 2+ 块画板点不中、框选错乱)。
    #[test]
    fn hit_and_marquee_use_world_coords() {
        let mut doc = Document::new("t", "zh-CN");
        // 第二块画板放在 y=980
        let ab2 = doc.new_artboard("画板 2", 800.0, 600.0);
        doc.nodes.get_mut(ab2).unwrap().geom.y = 980.0;
        let box_id = add_box(&mut doc, ab2, 100.0, 50.0, 100.0, 50.0);

        // 画板本地 (100+50) + 画板原点 980 = 世界 (150, 1030)
        assert_eq!(
            hit_test(&doc, ab2, 150.0, 1030.0),
            Some(box_id),
            "世界坐标点选必须命中第二画板上的对象"
        );
        assert_eq!(hit_test(&doc, ab2, 150.0, 50.0), None, "本地坐标不得命中");

        let r = Rect::new(140.0, 1020.0, 260.0, 1140.0);
        let hits = marquee_select(&doc, ab2, r);
        assert_eq!(hits, vec![box_id], "世界坐标框选必须命中");
        // 第一画板范围内框选不得命中第二画板对象
        let r_local = Rect::new(140.0, 20.0, 260.0, 140.0);
        assert!(marquee_select(&doc, ab2, r_local).is_empty());
    }

    /// 框选顶层语义:命中组后不再把组内子孙一并选中。
    #[test]
    fn marquee_selects_top_level_only() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let gsid = doc.alloc_sid();
        let mut g = Node::new(NodeKind::Group, "组", gsid);
        g.geom = Geom {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 300.0,
        };
        let gid = doc.nodes.insert(g);
        doc.nodes.get_mut(gid).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(gid);
        let child = add_box(&mut doc, gid, 10.0, 10.0, 80.0, 40.0);

        let hits = marquee_select(&doc, ab, Rect::new(0.0, 0.0, 320.0, 320.0));
        assert_eq!(hits, vec![gid], "只选组本身,不得连子孙一起选");
        assert!(!hits.contains(&child));
    }

    /// abs_bbox_world = 画板原点 + 本地 bbox;画板自身 geom 即世界坐标。
    #[test]
    fn abs_bbox_world_adds_artboard_origin() {
        let mut doc = Document::new("t", "zh-CN");
        let ab2 = doc.new_artboard("画板 2", 800.0, 600.0);
        doc.nodes.get_mut(ab2).unwrap().geom.y = 980.0;
        let id = add_box(&mut doc, ab2, 100.0, 50.0, 100.0, 50.0);
        let bb = abs_bbox_world(&doc, id).unwrap();
        assert_eq!((bb.x0, bb.y0), (100.0, 1030.0));
        let ab_bb = abs_bbox_world(&doc, ab2).unwrap();
        assert_eq!((ab_bb.x0, ab_bb.y0), (0.0, 980.0));
        assert_eq!(artboard_of(&doc, id), Some(ab2));
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
