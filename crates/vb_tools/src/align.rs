//! 对齐几何(**唯一实现**)。
//!
//! 为什么必须只有一份:对齐是"把若干个节点的**绝对盒**对齐到同一个目标盒",
//! 而节点的 `geom` 是**父相对**坐标。若某处误把 `geom` 当画板本地坐标算
//! `min_x / 中心 / max_r`,它算出来的就是"不同参照系下的一堆数字"
//! —— 不仅对齐结果错(浏览器里也跳),重导入后几何还会每轮漂移一次,
//! 直接把 L1 幂等打穿。这条教训来自 2026-09-21 的总验收(判据 C)。
//!
//! 因此:**对齐目标盒与成员盒都用绝对盒(`abs_bbox`,画板本地系),
//! 位移用"绝对系差值"平移节点自身的 `geom`** —— 差值在任何参照系下都成立。
//!
//! 使用方:`vb_app`(对齐面板 / 对齐命令)与 `vb_agent`(patch 的 `align` op)。

use vb_common::geom::Rect;
use vb_doc::model::{Document, Geom, NodeId};

/// 对齐模式(6 键,名称与命令 ID 的后缀一致)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignMode {
    Left,
    HCenter,
    Right,
    Top,
    VCenter,
    Bottom,
}

impl AlignMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "left" => Some(Self::Left),
            "hcenter" => Some(Self::HCenter),
            "right" => Some(Self::Right),
            "top" => Some(Self::Top),
            "vcenter" => Some(Self::VCenter),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::HCenter => "hcenter",
            Self::Right => "right",
            Self::Top => "top",
            Self::VCenter => "vcenter",
            Self::Bottom => "bottom",
        }
    }

    /// 六种模式的可枚举全集(供 UI 按序渲染与门禁穷举)。
    pub const ALL: [AlignMode; 6] = [
        Self::Left,
        Self::HCenter,
        Self::Right,
        Self::Top,
        Self::VCenter,
        Self::Bottom,
    ];
}

/// 「对齐到」三选一(03-5-2)。
///
/// - `Selection`:成员盒的公共包围盒(AI 默认;单选时无意义);
/// - `KeyObject`:**最后选中**者(与画布/面板同口径,画布会给它加粗选中框);
/// - `Artboard`:所属画板的本地框 `(0,0,w,h)`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignTo {
    #[default]
    Selection,
    KeyObject,
    Artboard,
}

impl AlignTo {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "selection" | "selection_box" => Some(Self::Selection),
            "key" | "key_object" | "keyobject" => Some(Self::KeyObject),
            "artboard" => Some(Self::Artboard),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::KeyObject => "key_object",
            Self::Artboard => "artboard",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Selection => "选区",
            Self::KeyObject => "关键对象",
            Self::Artboard => "画板",
        }
    }

    pub const ALL: [AlignTo; 3] = [Self::Selection, Self::KeyObject, Self::Artboard];
}

/// 绝对包围盒(画板本地系;与 [`vb_common::geom::Rect`] 同一套数值)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbsBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl AbsBox {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn from_rect(r: Rect) -> Self {
        Self::new(r.x0, r.y0, r.x1, r.y1)
    }

    /// 节点的绝对盒(画板本地系);无几何 / 无画板归属 → `None`。
    pub fn of(doc: &Document, id: NodeId) -> Option<Self> {
        crate::abs_bbox(doc, id).map(Self::from_rect)
    }

    pub fn w(&self) -> f64 {
        self.x1 - self.x0
    }

    pub fn h(&self) -> f64 {
        self.y1 - self.y0
    }

    pub fn center_x(&self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }

    pub fn center_y(&self) -> f64 {
        (self.y0 + self.y1) / 2.0
    }

    /// 多个盒的并集(空 → `None`)。
    pub fn union(items: &[Self]) -> Option<Self> {
        let mut it = items.iter();
        let first = *it.next()?;
        Some(it.fold(first, |a, b| {
            Self::new(
                a.x0.min(b.x0),
                a.y0.min(b.y0),
                a.x1.max(b.x1),
                a.y1.max(b.y1),
            )
        }))
    }
}

/// 把 `bb` 对齐到 `target` 所需的位移 `(dx, dy)`,**在绝对系里算**。
///
/// 该位移对节点自身 `geom` 同样成立(平移不依赖参照系),直接 `geom.x += dx` 即可。
pub fn aligned_delta(mode: AlignMode, bb: &AbsBox, target: &AbsBox) -> (f64, f64) {
    let (dx, dy) = match mode {
        AlignMode::Left => (target.x0 - bb.x0, 0.0),
        AlignMode::HCenter => (target.center_x() - bb.center_x(), 0.0),
        AlignMode::Right => (target.x1 - bb.x1, 0.0),
        AlignMode::Top => (0.0, target.y0 - bb.y0),
        AlignMode::VCenter => (0.0, target.center_y() - bb.center_y()),
        AlignMode::Bottom => (0.0, target.y1 - bb.y1),
    };
    (dx, dy)
}

/// 便捷:算出对齐后的新 `geom`(只平移,尺寸不变)。
pub fn aligned_geom(mode: AlignMode, g: Geom, bb: &AbsBox, target: &AbsBox) -> Geom {
    let (dx, dy) = aligned_delta(mode, bb, target);
    Geom {
        x: g.x + dx,
        y: g.y + dy,
        w: g.w,
        h: g.h,
    }
}

/// 「分布间距」目标位置:首末不动,中间成员**间隙**均匀。
///
/// 返回与 `items` 同序的目标 x0(或 y0);不足 3 个 → `None`。
/// 排序按**视觉序**(位置从小到大),不按传入顺序。
pub fn space_targets(items: &[AbsBox], horizontal: bool) -> Option<Vec<f64>> {
    if items.len() < 3 {
        return None;
    }
    let start_of = |b: &AbsBox| if horizontal { b.x0 } else { b.y0 };
    let end_of = |b: &AbsBox| if horizontal { b.x1 } else { b.y1 };
    let size_of = |b: &AbsBox| if horizontal { b.w() } else { b.h() };

    let mut idx: Vec<usize> = (0..items.len()).collect();
    idx.sort_by(|&a, &b| start_of(&items[a]).total_cmp(&start_of(&items[b])));

    let first = idx[0];
    let last = idx[idx.len() - 1];
    let span = end_of(&items[last]) - start_of(&items[first]);
    let total: f64 = items.iter().map(size_of).sum();
    let gap = (span - total) / (items.len() - 1) as f64;

    let mut out = vec![0.0; items.len()];
    let mut cur = start_of(&items[first]);
    for &i in &idx {
        out[i] = cur;
        cur += size_of(&items[i]) + gap;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::model::{Document, Geom, Node, NodeKind};

    fn ab(x0: f64, y0: f64, x1: f64, y1: f64) -> AbsBox {
        AbsBox::new(x0, y0, x1, y1)
    }

    fn doc_with_nested() -> (Document, String, String, NodeId) {
        // 画板 ▸ 容器(offset 100,50)▸ 子(geom 10,10,20,20 → 绝对 110,60)
        let mut doc = Document::new_default();
        let ab_id = doc.artboards.first().copied().unwrap();
        doc.nodes.get_mut(ab_id).unwrap().geom = Geom {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        };
        let wrap_sid = doc.alloc_sid();
        let mut wrap = Node::new(NodeKind::Group, "容器", wrap_sid.clone());
        wrap.geom = Geom {
            x: 100.0,
            y: 50.0,
            w: 400.0,
            h: 300.0,
        };
        let wrap_id = doc.nodes.insert(wrap);
        doc.nodes.get_mut(wrap_id).unwrap().parent = Some(ab_id);
        doc.nodes.get_mut(ab_id).unwrap().children.push(wrap_id);

        let child_sid = doc.alloc_sid();
        let mut child = Node::new(NodeKind::Box, "子", child_sid.clone());
        child.geom = Geom {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
        };
        let child_id = doc.nodes.insert(child);
        doc.nodes.get_mut(child_id).unwrap().parent = Some(wrap_id);
        doc.nodes.get_mut(wrap_id).unwrap().children.push(child_id);
        (
            doc,
            wrap_sid.as_str().to_string(),
            child_sid.as_str().to_string(),
            ab_id,
        )
    }

    #[test]
    fn abs_box_of_nested_node_is_artboard_local() {
        let (doc, _w, c, _ab) = doc_with_nested();
        let id = doc.find_by_sid(&c).unwrap();
        let b = AbsBox::of(&doc, id).expect("应能取绝对盒");
        assert_eq!(b, ab(110.0, 60.0, 130.0, 80.0), "父偏移必须计入");
    }

    #[test]
    fn align_left_uses_absolute_frame() {
        let (doc, w, c, _ab) = doc_with_nested();
        let wid = doc.find_by_sid(&w).unwrap();
        let cid = doc.find_by_sid(&c).unwrap();
        let wb = AbsBox::of(&doc, wid).unwrap();
        let cb = AbsBox::of(&doc, cid).unwrap();
        let target = AbsBox::union(&[wb, cb]).unwrap();
        // 子对齐到并集左边(100)→ 位移 -10,而不是把 geom.x 直接写成 100
        let g = doc.nodes.get(cid).unwrap().geom;
        let ng = aligned_geom(AlignMode::Left, g, &cb, &target);
        assert_eq!(ng.x, 0.0, "父相对坐标里应为 10-10=0");
        assert_eq!(ng.w, 20.0);
    }

    #[test]
    fn six_modes_deltas() {
        let b = ab(10.0, 20.0, 30.0, 40.0); // 20×20,中心 (20,30)
        let t = ab(0.0, 0.0, 100.0, 200.0); // 中心 (50,100)
        for (m, want) in [
            (AlignMode::Left, (-10.0, 0.0)),
            (AlignMode::HCenter, (30.0, 0.0)),
            (AlignMode::Right, (70.0, 0.0)),
            (AlignMode::Top, (0.0, -20.0)),
            (AlignMode::VCenter, (0.0, 70.0)),
            (AlignMode::Bottom, (0.0, 160.0)),
        ] {
            let got = aligned_delta(m, &b, &t);
            assert!(
                (got.0 - want.0).abs() < 1e-9 && (got.1 - want.1).abs() < 1e-9,
                "{m:?} 期望 {want:?} 实得 {got:?}"
            );
        }
        assert_eq!(AlignMode::ALL.len(), 6);
        for m in AlignMode::ALL {
            assert_eq!(
                AlignMode::parse(m.as_str()),
                Some(m),
                "parse/as_str 必须互逆"
            );
        }
        assert_eq!(AlignMode::parse("nope"), None);
    }

    #[test]
    fn union_is_none_for_empty() {
        assert_eq!(AbsBox::union(&[]), None);
        assert_eq!(
            AbsBox::union(&[ab(1.0, 2.0, 3.0, 4.0)]),
            Some(ab(1.0, 2.0, 3.0, 4.0))
        );
    }

    #[test]
    fn space_targets_equalize_gaps() {
        let items = [
            ab(0.0, 0.0, 20.0, 10.0),
            ab(40.0, 0.0, 60.0, 10.0),
            ab(100.0, 0.0, 120.0, 10.0),
        ];
        let t = space_targets(&items, true).expect("三块可分布");
        assert_eq!(t, vec![0.0, 50.0, 100.0], "首末不动,间隙各 30");
        assert_eq!(t[0], items[0].x0);
        assert_eq!(t[2], items[2].x0);
    }

    #[test]
    fn space_targets_sorts_by_visual_order_and_needs_three() {
        // 传入顺序打乱(100, 0, 40)→ 按视觉序 a(0) b(40) c(100) 分布
        let items = [
            ab(100.0, 0.0, 120.0, 10.0),
            ab(0.0, 0.0, 20.0, 10.0),
            ab(40.0, 0.0, 60.0, 10.0),
        ];
        assert_eq!(space_targets(&items, true).unwrap(), vec![100.0, 0.0, 50.0]);
        assert!(
            space_targets(&items[..2], true).is_none(),
            "不足三个 → 不分布"
        );
    }
}
