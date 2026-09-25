//! 智能参考线吸附:对齐兄弟 / 画板 / 等距 / 等尺寸(屏幕空间阈值),
//! 以及父级到画板的坐标偏移换算。
//!
//! 06-1 自 `canvas_input.rs` 按职责拆出(纯搬移,零行为变化)。

use vb_doc::model::{Geom, NodeKind};

use crate::app::VellumApp;

impl VellumApp {
    /// 移动节点父级到所属画板之间的累计 geom 偏移(父级即画板时为 0)。
    /// 用于把父相对坐标换算成画板本地坐标。
    pub(super) fn parent_offset_in_artboard(
        &self,
        parent: Option<vb_doc::model::NodeId>,
        artboard: Option<vb_doc::model::NodeId>,
    ) -> (f64, f64) {
        let (mut ox, mut oy) = (0.0f64, 0.0f64);
        let Some(mut cur) = parent else {
            return (ox, oy);
        };
        while let Some(n) = self.doc.nodes.get(cur) {
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
    /// 帧纪律(G2/G3 修复):移动盒 geom 是父相对坐标,兄弟 bbox 与画板
    /// 边是画板本地坐标 —— 先把移动盒换算进画板本地帧再比较(否则组内
    /// 拖动吸附整体偏移一个组偏移量);参考线统一加画板世界原点输出
    /// (否则第 2+ 画板上的线错位一个画板偏移)。
    pub(super) fn smart_snap(
        &self,
        moving: vb_doc::model::NodeId,
        parent: Option<vb_doc::model::NodeId>,
        artboard: Option<vb_doc::model::NodeId>,
        g: &Geom,
        tol: f64,
    ) -> (f64, f64, Vec<[f64; 4]>) {
        let (off_x, off_y) = self.parent_offset_in_artboard(parent, artboard);
        let (abx, aby) = artboard
            .and_then(|ab| self.doc.nodes.get(ab))
            .map(|n| (n.geom.x, n.geom.y))
            .unwrap_or((0.0, 0.0));
        let mut xs: Vec<(f64, f64, f64)> = Vec::new(); // (候选 x, 线 y0, 线 y1)
        let mut ys: Vec<(f64, f64, f64)> = Vec::new(); // (候选 y, 线 x0, 线 x1)
                                                       // 兄弟完整 bbox(P4.1 间距/尺寸类用)
        let mut sib_rects: Vec<(f64, f64, f64, f64)> = Vec::new(); // (x0,y0,x1,y1)

        // 画板边/中心
        if let Some(ab) = artboard {
            if let Some(n) = self.doc.nodes.get(ab) {
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
            if let Some(pn) = self.doc.nodes.get(pid) {
                for &c in &pn.children {
                    if c == moving {
                        continue;
                    }
                    // 隐藏/锁定对象不可见不可选,也不得吸走拖动(与拾取口径一致)
                    if self
                        .doc
                        .nodes
                        .get(c)
                        .map(|n| n.hidden || n.locked)
                        .unwrap_or(true)
                    {
                        continue;
                    }
                    if let Some(bb) = vb_tools::abs_bbox(&self.doc, c) {
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
}
