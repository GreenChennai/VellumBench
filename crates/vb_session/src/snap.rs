//! 智能参考线吸附引擎:对齐兄弟 / 画板 / 等距 / 等尺寸(屏幕空间阈值),
//! 以及父级到画板的坐标偏移换算。**纯函数,零 UI 依赖** —— 输入文档快照
//! 与几何,输出吸附后坐标与参考线段,不持任何会话/宿主状态。
//!
//! # 迁移出处(22 篇 §3.3「吸附引擎在 vb_session」)
//!
//! 原体在 `vb_app::app::canvas_input::snap`(06-1 自 `canvas_input.rs`
//! 按职责拆出);本批整体下沉为本模块的**自由函数**:`impl VellumApp`
//! 的两个方法改为薄转发,调用点(`select.rs`)不动,行为逐字节一致。
//! 吸附/几何求值单源纪律:新宿主(vb_kit/vb_shell)不得手写第二份。
//!
//! 依赖注记:引擎需要沿文档祖先链遍历(`vb_doc`)并复用画板本地
//! bbox 口径(`vb_tools::abs_bbox`,**复用而非抄写**,防第二份几何)。
//! `vb_doc ← vb_session` 是 22 篇 §3.1 的既定依赖方向;vb_tools 与
//! vb_doc 同为无 UI 的核心 crate。
//!
//! 帧纪律(G2/G3 修复,见 [`smart_snap`]):移动盒 geom 是父相对坐标,
//! 兄弟 bbox 与画板边是画板本地坐标 —— 引擎内部先换算进画板本地帧
//! 再比较;参考线统一加画板世界原点输出。

use vb_doc::model::{Document, Geom, NodeKind};

/// 移动节点父级到所属画板之间的累计 geom 偏移(父级即画板时为 0)。
/// 用于把父相对坐标换算成画板本地坐标。
pub fn parent_offset_in_artboard(
    doc: &Document,
    parent: Option<vb_doc::model::NodeId>,
    artboard: Option<vb_doc::model::NodeId>,
) -> (f64, f64) {
    let (mut ox, mut oy) = (0.0f64, 0.0f64);
    let Some(mut cur) = parent else {
        return (ox, oy);
    };
    while let Some(n) = doc.nodes.get(cur) {
        if Some(cur) == artboard || matches!(n.kind, NodeKind::Artboard) {
            return (ox, oy);
        }
        ox += n.geom.x;
        oy += n.geom.y;
        match n.parent {
            Some(p) => cur = p,
            None => return (ox, oy),
        }
    }
    (ox, oy)
}

/// 智能参考线:移动中的对象边/中心 对齐 兄弟边/中心 或 画板边/中心。
/// 返回 (吸附后 x, 吸附后 y, 参考线段[世界坐标])。
///
/// `tol` 是**屏幕空间阈值换算到世界坐标**后的容差(调用方以
/// `屏幕像素 / 相机 zoom` 传入,如旧宿主的 `6.0 / camera.zoom`),
/// 本引擎不做任何相机假设。
///
/// 帧纪律(G2/G3 修复):移动盒 geom 是父相对坐标,兄弟 bbox 与画板
/// 边是画板本地坐标 —— 先把移动盒换算进画板本地帧再比较(否则组内
/// 拖动吸附整体偏移一个组偏移量);参考线统一加画板世界原点输出
/// (否则第 2+ 画板上的线错位一个画板偏移)。
pub fn smart_snap(
    doc: &Document,
    moving: vb_doc::model::NodeId,
    parent: Option<vb_doc::model::NodeId>,
    artboard: Option<vb_doc::model::NodeId>,
    g: &Geom,
    tol: f64,
) -> (f64, f64, Vec<[f64; 4]>) {
    let (off_x, off_y) = parent_offset_in_artboard(doc, parent, artboard);
    let (abx, aby) = artboard
        .and_then(|ab| doc.nodes.get(ab))
        .map(|n| (n.geom.x, n.geom.y))
        .unwrap_or((0.0, 0.0));
    let mut xs: Vec<(f64, f64, f64)> = Vec::new(); // (候选 x, 线 y0, 线 y1)
    let mut ys: Vec<(f64, f64, f64)> = Vec::new(); // (候选 y, 线 x0, 线 x1)
                                                   // 兄弟完整 bbox(P4.1 间距/尺寸类用)
    let mut sib_rects: Vec<(f64, f64, f64, f64)> = Vec::new(); // (x0,y0,x1,y1)

    // 画板边/中心
    if let Some(ab) = artboard {
        if let Some(n) = doc.nodes.get(ab) {
            let (ax, ay, aw, ah) = (0.0, 0.0, n.geom.w, n.geom.h);
            xs.push((ax, ay, ay + ah));
            xs.push((ax + aw / 2.0, ay, ay + ah));
            xs.push((ax + aw, ay, ay + ah));
            ys.push((ay, ax, ax + aw));
            ys.push((ay + ah / 2.0, ax, ax + aw));
            ys.push((ay + ah, ax, ax + aw));
        }
    }
    // 兄弟节点
    if let Some(pid) = parent {
        if let Some(pn) = doc.nodes.get(pid) {
            for &c in &pn.children {
                if c == moving {
                    continue;
                }
                // 隐藏/锁定对象不可见不可选,也不得吸走拖动(与拾取口径一致)
                if doc
                    .nodes
                    .get(c)
                    .map(|n| n.hidden || n.locked)
                    .unwrap_or(true)
                {
                    continue;
                }
                if let Some(bb) = vb_tools::abs_bbox(doc, c) {
                    let (bx0, by0, bx1, by1) = (bb.x0, bb.y0, bb.x1, bb.y1);
                    sib_rects.push((bx0, by0, bx1, by1));
                    xs.push((bx0, by0, by1));
                    xs.push(((bx0 + bx1) / 2.0, by0, by1));
                    xs.push((bx1, by0, by1));
                    ys.push((by0, bx0, bx1));
                    ys.push(((by0 + by1) / 2.0, bx0, bx1));
                    ys.push((by1, bx0, bx1));
                }
            }
        }
    }

    let mut lines: Vec<[f64; 4]> = Vec::new();
    let mx = [g.x + off_x, g.x + off_x + g.w / 2.0, g.x + off_x + g.w];
    let my = [g.y + off_y, g.y + off_y + g.h / 2.0, g.y + off_y + g.h];

    let mut best_x: Option<(f64, f64)> = None; // (delta, 候选)
    let mut best_line_x: Option<[f64; 4]> = None;
    for e in mx {
        for (cand, ly0, ly1) in &xs {
            let d = (e - cand).abs();
            if d <= tol && best_x.map(|(bd, _)| d < bd).unwrap_or(true) {
                best_x = Some((d, *cand));
                // 只保留当前最优候选的参考线(此前每个容差内候选都画一条)
                best_line_x = Some([
                    *cand + abx,
                    *ly0 - 12.0 + aby,
                    *cand + abx,
                    *ly1 + 12.0 + aby,
                ]);
            }
        }
    }
    if let Some(l) = best_line_x {
        lines.push(l);
    }
    let mut best_y: Option<(f64, f64)> = None;
    let mut best_line_y: Option<[f64; 4]> = None;
    for e in my {
        for (cand, lx0, lx1) in &ys {
            let d = (e - cand).abs();
            if d <= tol && best_y.map(|(bd, _)| d < bd).unwrap_or(true) {
                best_y = Some((d, *cand));
                best_line_y = Some([
                    *lx0 - 12.0 + abx,
                    *cand + aby,
                    *lx1 + 12.0 + abx,
                    *cand + aby,
                ]);
            }
        }
    }
    if let Some(l) = best_line_y {
        lines.push(l);
    }
    let nx = best_x.map(|(_, c)| {
        // 对齐的是哪条边?吸附到候选后保持原相对关系:取移动后最接近候选的那条边
        // (c 与 cur 同在画板本地帧,delta 是平移量,帧无关)
        let cur = [g.x + off_x, g.x + off_x + g.w / 2.0, g.x + off_x + g.w]
            .iter()
            .copied()
            .min_by(|a, b| {
                (*a - c)
                    .abs()
                    .partial_cmp(&(*b - c).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(c);
        g.x + (c - cur)
    });
    let ny = best_y.map(|(_, c)| {
        let cur = [g.y + off_y, g.y + off_y + g.h / 2.0, g.y + off_y + g.h]
            .iter()
            .copied()
            .min_by(|a, b| {
                (*a - c)
                    .abs()
                    .partial_cmp(&(*b - c).abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(c);
        g.y + (c - cur)
    });

    let mut nx = nx.unwrap_or(g.x);
    let mut ny = ny.unwrap_or(g.y);

    // ── P4.1 间距类:移动边与某兄弟形成"与既有兄弟对间距相等"的布局时吸附。
    // 仅在坐标轴对齐类未命中时尝试(对齐优先)。
    if best_x.is_none() && sib_rects.len() >= 2 {
        let mut sibs: Vec<(f64, f64, f64, f64)> = sib_rects.clone();
        sibs.sort_by(|a, b| a.2.total_cmp(&b.2));
        let mut gaps: Vec<f64> = Vec::new();
        for w in sibs.windows(2) {
            let gp = w[1].0 - w[0].2;
            if gp > 0.0 {
                gaps.push(gp);
            }
        }
        let mut best: Option<(f64, f64, f64, f64)> = None; // (delta, cand_x, y0, y1)
        for (ax0, ay0, ax1, ay1) in &sib_rects {
            for gp in &gaps {
                // 放在兄弟右侧:移动盒左缘 = 兄弟右缘 + gp
                let cand = ax1 + gp;
                let d = (g.x + off_x - cand).abs();
                if d <= tol && best.as_ref().map(|(bd, ..)| d < *bd).unwrap_or(true) {
                    best = Some((d, cand, *ay0, *ay1));
                }
                // 放在兄弟左侧:移动盒左缘 = 兄弟左缘 - gp - 移动盒宽
                let cand2 = ax0 - gp - g.w;
                let d2 = (g.x + off_x - cand2).abs();
                if d2 <= tol && best.as_ref().map(|(bd, ..)| d2 < *bd).unwrap_or(true) {
                    best = Some((d2, cand2, *ay0, *ay1));
                }
            }
        }
        if let Some((_, cand, ly0, ly1)) = best {
            nx = cand - off_x;
            // 间距参考线:横跨两盒中点的水平测量线(世界坐标)
            let mid_y = ly0 + (ly1 - ly0) / 2.0;
            lines.push([cand + abx, mid_y + aby, cand + g.w + abx, mid_y + aby]);
        }
    }
    if best_y.is_none() && sib_rects.len() >= 2 {
        let mut vrects: Vec<(f64, f64, f64, f64)> = sib_rects.clone();
        vrects.sort_by(|a, b| a.3.total_cmp(&b.3));
        let mut gaps: Vec<f64> = Vec::new();
        for w in vrects.windows(2) {
            let gp = w[1].1 - w[0].3;
            if gp > 0.0 {
                gaps.push(gp);
            }
        }
        let mut best: Option<(f64, f64, f64, f64)> = None;
        for (bx0, by0, bx1, by1) in &vrects {
            for gp in &gaps {
                let cand = *by1 + gp;
                let d = (g.y + off_y - cand).abs();
                if d <= tol && best.as_ref().map(|(bd, ..)| d < *bd).unwrap_or(true) {
                    best = Some((d, cand, *bx0, *bx1));
                }
                let cand2 = by0 - gp - g.h;
                let d2 = (g.y + off_y - cand2).abs();
                if d2 <= tol && best.as_ref().map(|(bd, ..)| d2 < *bd).unwrap_or(true) {
                    best = Some((d2, cand2, *bx0, *bx1));
                }
            }
        }
        if let Some((_, cand, lx0, lx1)) = best {
            ny = cand - off_y;
            let mid_x = lx0 + (lx1 - lx0) / 2.0;
            lines.push([mid_x + abx, cand + aby, mid_x + abx, cand + g.h + aby]);
        }
    }

    // ── P4.1 尺寸相等类:宽(或高)与某兄弟一致时轻微吸附(仅拖动,不改尺寸) ──
    // 移动时若宽恰等于某兄弟宽,沿该兄弟左缘对齐提示;此处以参考线表达。
    for (bx0, by0, bx1, by1) in &sib_rects {
        let dw = (*bx1 - *bx0 - g.w).abs();
        let dh = (*by1 - *by0 - g.h).abs();
        if dw <= tol * 0.5 {
            lines.push([*bx0 + abx, *by0 - 8.0 + aby, *bx0 + abx, *by1 + 8.0 + aby]);
        }
        if dh <= tol * 0.5 {
            lines.push([*bx0 - 8.0, *by0, *bx1 + 8.0, *by0]);
        }
    }

    (nx, ny, lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::model::{Node, NodeId};

    /// 测试节点:直接挂到 parent 下(parent 是画板或编组)。
    fn add_box(doc: &mut Document, parent: NodeId, x: f64, y: f64, w: f64, h: f64) -> NodeId {
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, "盒", sid);
        n.geom = Geom { x, y, w, h };
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        id
    }

    /// 有序编组(geom 即偏移),供组内帧纪律用例使用。
    fn add_group(doc: &mut Document, parent: NodeId, x: f64, y: f64) -> NodeId {
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Group, "组", sid);
        n.geom = Geom {
            x,
            y,
            w: 0.0,
            h: 0.0,
        };
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        id
    }

    #[test]
    fn parent_offset_zero_when_parent_is_artboard_or_none() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let m = add_box(&mut doc, ab, 10.0, 20.0, 40.0, 30.0);
        assert_eq!(
            parent_offset_in_artboard(&doc, Some(ab), Some(ab)),
            (0.0, 0.0)
        );
        assert_eq!(parent_offset_in_artboard(&doc, None, Some(ab)), (0.0, 0.0));
        // 父级是画板时,smart_snap 不得移动坐标(画板本地帧 = 父相对帧)
        let g = Geom {
            x: 10.0,
            y: 20.0,
            w: 40.0,
            h: 30.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (10.0, 20.0), "无候选时原样返回");
        assert!(lines.is_empty());
    }

    #[test]
    fn parent_offset_accumulates_group_chain() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let g1 = add_group(&mut doc, ab, 10.0, 20.0);
        let g2 = add_group(&mut doc, g1, 100.0, 200.0);
        assert_eq!(
            parent_offset_in_artboard(&doc, Some(g2), Some(ab)),
            (110.0, 220.0)
        );
        // 未指定画板:沿父链上溯到第一个画板祖先为止,语义不变
        assert_eq!(
            parent_offset_in_artboard(&doc, Some(g2), None),
            (110.0, 220.0)
        );
        // artboard 是链中更上层节点:走到它即止
        assert_eq!(
            parent_offset_in_artboard(&doc, Some(g1), Some(g1)),
            (0.0, 0.0)
        );
    }

    #[test]
    fn aligns_to_sibling_left_edge_within_tolerance() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let _sib = add_box(&mut doc, ab, 100.0, 100.0, 100.0, 50.0);
        // 尺寸 50×40:避开与兄弟等高触发 P4.1 等尺寸提示线(下同,不赘)
        let m = add_box(&mut doc, ab, 104.0, 200.0, 50.0, 40.0);
        let g = Geom {
            x: 104.0,
            y: 200.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        // 左缘 104 → 吸附兄弟左缘 100(d=4);y 无候选原样
        assert_eq!((nx, ny), (100.0, 200.0));
        // 参考线:竖线 x=100,跨兄弟 y 范围外扩 12(世界坐标,画板原点 0,0)
        assert_eq!(lines, vec![[100.0, 88.0, 100.0, 162.0]]);
    }

    #[test]
    fn exact_tolerance_boundary_snaps_but_one_pixel_more_does_not() {
        // 每个子情形独立文档:拖动手势一次只有一个移动对象,前一个用例的
        // 移动盒不得成为后一个用例的兄弟候选
        let build = |mx: f64, my: f64| {
            let mut doc = Document::new("t", "zh-CN");
            let ab = doc.artboards[0];
            let _sib = add_box(&mut doc, ab, 100.0, 100.0, 100.0, 50.0);
            let m = add_box(&mut doc, ab, mx, my, 50.0, 40.0);
            (doc, ab, m)
        };
        // d = 6.0 == tol:边界即吸附(`d <= tol`)
        let (doc, ab, m) = build(106.0, 300.0);
        let g = Geom {
            x: 106.0,
            y: 300.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (100.0, 300.0));
        assert_eq!(lines.len(), 1);
        // d = 7.0 > tol:不吸
        let (doc, ab, m) = build(107.0, 400.0);
        let g = Geom {
            x: 107.0,
            y: 400.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx2, ny2, lines2) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx2, ny2), (107.0, 400.0));
        assert!(lines2.is_empty());
        // tol = 0:恰好重合也吸(参考线仍发出)
        let (doc, ab, m) = build(100.0, 500.0);
        let g = Geom {
            x: 100.0,
            y: 500.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx3, _, lines3) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 0.0);
        assert_eq!(nx3, 100.0);
        assert_eq!(lines3.len(), 1);
    }

    #[test]
    fn closest_candidate_wins_and_single_line_per_axis() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let _a = add_box(&mut doc, ab, 100.0, 100.0, 100.0, 50.0);
        let _b = add_box(&mut doc, ab, 103.0, 300.0, 100.0, 50.0);
        // 移动左缘 105:A 左缘 d=5,B 左缘 d=2 → 取更近的 B;每轴只画一条线。
        // (y=470:避开 B.y1+水平间距 150 = 500 的垂直等距测量线巧合命中)
        let m = add_box(&mut doc, ab, 105.0, 470.0, 50.0, 40.0);
        let g = Geom {
            x: 105.0,
            y: 470.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx, _, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!(nx, 103.0);
        let vlines: Vec<[f64; 4]> = lines.iter().filter(|l| l[0] == l[2]).copied().collect();
        assert_eq!(
            vlines,
            vec![[103.0, 288.0, 103.0, 362.0]],
            "只保留最优候选的参考线"
        );
    }

    #[test]
    fn center_and_right_edge_alignment() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let _sib = add_box(&mut doc, ab, 100.0, 100.0, 100.0, 50.0);
        // 移动盒中心 150(= 兄弟中心)精确重合:吸附但坐标不变,线仍发出
        let m = add_box(&mut doc, ab, 125.0, 300.0, 50.0, 40.0);
        let g = Geom {
            x: 125.0,
            y: 300.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (125.0, 300.0));
        assert_eq!(lines, vec![[150.0, 88.0, 150.0, 162.0]]);
        // 右缘对齐:移动右缘 203 距兄弟右缘 200 为 d=3 → 吸
        // (第二个移动盒在前一个的调用之后加入,不参与前者的兄弟集合)
        let m2 = add_box(&mut doc, ab, 153.0, 500.0, 50.0, 40.0);
        let g2 = Geom {
            x: 153.0,
            y: 500.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx2, _, _) = smart_snap(&doc, m2, Some(ab), Some(ab), &g2, 6.0);
        assert_eq!(nx2, 150.0);
    }

    #[test]
    fn artboard_edges_and_center_are_candidates() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0]; // 1440×900 at (0,0)
        let m = add_box(&mut doc, ab, -3.0, 200.0, 50.0, 50.0);
        let g = Geom {
            x: -3.0,
            y: 200.0,
            w: 50.0,
            h: 50.0,
        };
        // 左缘 -3 → 吸画板左缘 0;y 远离画板三线,不吸
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (0.0, 200.0));
        assert_eq!(
            lines,
            vec![[0.0, -12.0, 0.0, 912.0]],
            "画板左缘线跨全高外扩 12"
        );
        // 画板中心 720 亦是候选:左缘 722 距中心 d=2,左缘即最近对齐边 → 左缘吸到 720
        let m2 = add_box(&mut doc, ab, 722.0, 200.0, 50.0, 50.0);
        let g2 = Geom {
            x: 722.0,
            y: 200.0,
            w: 50.0,
            h: 50.0,
        };
        let (nx2, _, _) = smart_snap(&doc, m2, Some(ab), Some(ab), &g2, 6.0);
        assert_eq!(nx2, 720.0);
    }

    #[test]
    fn hidden_and_locked_siblings_do_not_snap() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let mut hid = Node::new(NodeKind::Box, "隐藏", doc.alloc_sid());
        hid.geom = Geom {
            x: 100.0,
            y: 100.0,
            w: 100.0,
            h: 50.0,
        };
        hid.hidden = true;
        let hid_id = doc.nodes.insert(hid);
        doc.nodes.get_mut(hid_id).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(hid_id);
        let mut lock = Node::new(NodeKind::Box, "锁定", doc.alloc_sid());
        lock.geom = Geom {
            x: 300.0,
            y: 100.0,
            w: 100.0,
            h: 50.0,
        };
        lock.locked = true;
        let lock_id = doc.nodes.insert(lock);
        doc.nodes.get_mut(lock_id).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(lock_id);

        // 移动盒左缘 103(距隐藏兄弟 d=3):不得被不可见/锁定对象吸走
        let m = add_box(&mut doc, ab, 103.0, 200.0, 50.0, 50.0);
        let g = Geom {
            x: 103.0,
            y: 200.0,
            w: 50.0,
            h: 50.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (103.0, 200.0), "隐藏/锁定兄弟不得吸走拖动");
        assert!(lines.is_empty());
    }

    #[test]
    fn equal_spacing_snaps_only_when_alignment_misses() {
        // 两个子情形各自独立文档(同一手势内移动对象不互为兄弟)
        let build = |mx: f64, my: f64| {
            let mut doc = Document::new("t", "zh-CN");
            let ab = doc.artboards[0];
            let _a = add_box(&mut doc, ab, 0.0, 0.0, 60.0, 50.0);
            let _b = add_box(&mut doc, ab, 140.0, 0.0, 80.0, 40.0);
            let m = add_box(&mut doc, ab, mx, my, 40.0, 30.0);
            (doc, ab, m)
        };
        // 间距 gp = B.x0 - A.x1 = 140 - 60 = 80;间距候选:B 右缘 220+80=300。
        // 对齐候选最右 220 → 移动左缘 296 时对齐全 miss(d≥76),间距命中(d=4)。
        let (doc, ab, m) = build(296.0, 100.0);
        let g = Geom {
            x: 296.0,
            y: 100.0,
            w: 40.0,
            h: 30.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (300.0, 100.0), "等距:移动左缘 = 兄弟右缘 + gp");
        // 测量线:跨被贴兄弟(B)y 中点 20 的水平线段
        assert_eq!(lines, vec![[300.0, 20.0, 340.0, 20.0]]);

        // 对齐优先:移动左缘 141 时对齐命中 B 左缘 140,间距分支不进(无测量线)
        let (doc, ab, m) = build(141.0, 200.0);
        let g = Geom {
            x: 141.0,
            y: 200.0,
            w: 40.0,
            h: 30.0,
        };
        let (nx2, _, lines2) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!(nx2, 140.0);
        assert_eq!(
            lines2,
            vec![[140.0, -12.0, 140.0, 52.0]],
            "只有对齐线,无间距测量线"
        );
    }

    #[test]
    fn vertical_equal_spacing_snaps() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let _a = add_box(&mut doc, ab, 0.0, 0.0, 60.0, 50.0);
        let _b = add_box(&mut doc, ab, 0.0, 140.0, 80.0, 40.0);
        // 垂直间距 gp = B.y0 - A.y1 = 140 - 50 = 90;候选:B 下缘 180+90=270。
        // 移动盒 x 放 200 避开水平对齐;尺寸 50×45 避开两兄弟触发等尺寸提示线。
        let m = add_box(&mut doc, ab, 200.0, 267.0, 50.0, 45.0);
        let g = Geom {
            x: 200.0,
            y: 267.0,
            w: 50.0,
            h: 45.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (200.0, 270.0), "等距:移动上缘 = 兄弟下缘 + gp");
        // 测量线:跨被贴兄弟(B)x 中点 40 的竖直线段,长度 = 移动盒高
        assert_eq!(lines, vec![[40.0, 270.0, 40.0, 315.0]]);
    }

    #[test]
    fn equal_size_emits_hint_lines_without_moving() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let _sib = add_box(&mut doc, ab, 100.0, 100.0, 100.0, 50.0);
        // 等宽(dw=0 ≤ tol/2):沿兄弟左缘发提示线;等高(dh=30 > 3)不发
        let m = add_box(&mut doc, ab, 400.0, 400.0, 100.0, 20.0);
        let g = Geom {
            x: 400.0,
            y: 400.0,
            w: 100.0,
            h: 20.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (400.0, 400.0), "尺寸类只提示不吸附坐标");
        assert_eq!(lines, vec![[100.0, 92.0, 100.0, 158.0]]);
        // 等宽且等高:两条提示线(竖线沿左缘 + 横线沿上缘)
        let m2 = add_box(&mut doc, ab, 400.0, 300.0, 100.0, 50.0);
        let g2 = Geom {
            x: 400.0,
            y: 300.0,
            w: 100.0,
            h: 50.0,
        };
        let (_, _, lines2) = smart_snap(&doc, m2, Some(ab), Some(ab), &g2, 6.0);
        assert!(lines2.contains(&[100.0, 92.0, 100.0, 158.0]), "{lines2:?}");
        assert!(lines2.contains(&[92.0, 100.0, 208.0, 100.0]), "{lines2:?}");
    }

    #[test]
    fn group_frame_conversion() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let grp = add_group(&mut doc, ab, 10.0, 20.0);
        let _sib = add_box(&mut doc, grp, 100.0, 100.0, 100.0, 50.0);
        let m = add_box(&mut doc, grp, 104.0, 200.0, 50.0, 40.0);
        // G2/G3 帧纪律:移动盒 104 是组内坐标(画板本地 114),兄弟画板本地
        // 左缘是 110 —— d=4 吸附后组内坐标应回到 100,不多扣/少扣组偏移。
        let g = Geom {
            x: 104.0,
            y: 200.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(grp), Some(ab), &g, 6.0);
        assert_eq!((nx, ny), (100.0, 200.0), "吸附量是帧无关的平移");
        // 兄弟画板本地 bbox = (110,120,210,170),线跨其 y 范围外扩 12
        assert_eq!(lines, vec![[110.0, 108.0, 110.0, 182.0]]);
    }

    #[test]
    fn guides_shift_by_second_artboard_world_origin() {
        let mut doc = Document::new("t", "zh-CN");
        let ab2 = doc.new_artboard("画板 2", 800.0, 600.0);
        doc.nodes.get_mut(ab2).unwrap().geom = Geom {
            x: 50.0,
            y: 980.0,
            w: 800.0,
            h: 600.0,
        };
        let _sib = add_box(&mut doc, ab2, 100.0, 100.0, 100.0, 50.0);
        let m = add_box(&mut doc, ab2, 104.0, 200.0, 50.0, 40.0);
        let g = Geom {
            x: 104.0,
            y: 200.0,
            w: 50.0,
            h: 40.0,
        };
        let (nx, ny, lines) = smart_snap(&doc, m, Some(ab2), Some(ab2), &g, 6.0);
        assert_eq!((nx, ny), (100.0, 200.0));
        // 参考线 = 画板本地候选 + 画板世界原点(50, 980):否则第二画板线错位
        assert_eq!(lines, vec![[150.0, 1068.0, 150.0, 1142.0]]);
    }

    #[test]
    fn no_parent_no_artboard_is_noop() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        let m = add_box(&mut doc, ab, 42.0, 42.0, 50.0, 50.0);
        // 无父无画板:无候选来源,原样返回(容忍悬空 id,不 panic)
        let g = Geom {
            x: 42.0,
            y: 42.0,
            w: 50.0,
            h: 50.0,
        };
        let phantom: Option<NodeId> = None;
        let (nx, ny, lines) = smart_snap(&doc, m, phantom, phantom, &g, 6.0);
        assert_eq!((nx, ny), (42.0, 42.0));
        assert!(lines.is_empty());
        // 父级除自身外无子:同样无候选(画板三线离得远)
        let empty_grp = add_group(&mut doc, ab, 0.0, 0.0);
        let m2 = add_box(&mut doc, empty_grp, 5000.0, 5000.0, 10.0, 10.0);
        let g2 = Geom {
            x: 5000.0,
            y: 5000.0,
            w: 10.0,
            h: 10.0,
        };
        let (nx2, ny2, lines2) = smart_snap(&doc, m2, Some(empty_grp), Some(ab), &g2, 6.0);
        assert_eq!((nx2, ny2), (5000.0, 5000.0));
        assert!(lines2.is_empty());
    }
}
