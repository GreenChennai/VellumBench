//! 对象编辑命令实现(S1-a 自 app.rs 机械搬移,零行为变化):
//! 层序/对齐/分布/编组/剪贴板/钢笔与矢量顶点/路径查找器/再次变换,
//! 以及形状/文本/画板创建、吸管、渐变、剪刀等工具落地逻辑。

use egui::Key;
use vb_doc::commands::Command;
use vb_doc::model::{Geom, NodeKind};

use super::{re_sid_tree, Tool, VellumApp};

impl VellumApp {
    /// 按层序调整当前选区(`Mod+[`/`]` 与菜单共用)。
    pub(crate) fn reorder_selection(&mut self, delta: i32) {
        let sids = self.selection.clone();
        for s in sids {
            self.reorder(&s, delta);
        }
    }

    pub(crate) fn arrow_nudge(&mut self, key: Key, _ctrl: bool, shift: bool) {
        let step = if shift { 10.0 } else { 1.0 };
        let (dx, dy) = match key {
            Key::ArrowLeft => (-step, 0.0),
            Key::ArrowRight => (step, 0.0),
            Key::ArrowUp => (0.0, -step),
            Key::ArrowDown => (0.0, step),
            _ => return,
        };
        if self.selection.is_empty() {
            self.camera.pan_x += dx;
            self.camera.pan_y += dy;
            return;
        }
        let sids = self.selection.clone();
        for sid in sids {
            if let Some(nid) = self.doc.find_by_sid(&sid) {
                let mut g = self.doc.nodes.get(nid).unwrap().geom;
                g.x += dx;
                g.y += dy;
                self.exec(Command::SetGeom {
                    sid: sid.clone(),
                    new: g,
                    old: None,
                    old_declared: None,
                });
            }
        }
        self.last_move_delta = Some((dx, dy));
    }

    pub(crate) fn delete_selection(&mut self) {
        // 预检:选中含画板且会删到不足一块时提前拦截(命令层有硬守卫,
        // 这里保住选区并给出可读提示,而不是吃掉选区后逐条报错)
        let artboards_in_selection = self
            .selection
            .iter()
            .filter(|sid| self.is_artboard_sid(sid))
            .count();
        if artboards_in_selection >= self.doc.artboards.len() && artboards_in_selection > 0 {
            self.status = "至少保留一块画板".into();
            return;
        }
        let sids = std::mem::take(&mut self.selection);
        for sid in sids {
            self.exec(Command::Delete {
                target_sid: sid,
                captured: None,
            });
        }
        self.status = "已删除(Ctrl+Z 撤销)".into();
    }

    /// 多选拖动(B4):除主对象外的其余选中对象及其起始几何。
    pub(crate) fn selection_others(&self, primary_sid: &str) -> Vec<(String, Geom)> {
        self.selection
            .iter()
            .filter(|s| s.as_str() != primary_sid)
            .filter_map(|s| {
                self.doc
                    .find_by_sid(s)
                    .and_then(|id| self.doc.nodes.get(id))
                    .map(|n| (s.clone(), n.geom))
            })
            .collect()
    }

    fn is_artboard_sid(&self, sid: &str) -> bool {
        self.doc.find_by_sid(sid).is_some_and(
            |id| matches!(self.doc.nodes.get(id), Some(n) if matches!(n.kind, NodeKind::Artboard)),
        )
    }

    pub(crate) fn group_selection(&mut self) {
        if self.selection.len() < 2 {
            self.status = "编组需要至少 2 个对象".into();
            return;
        }
        let group_sid = self.doc.alloc_sid();
        let members = self.selection.clone();
        self.exec(Command::Group {
            member_sids: members,
            name: format!("编组 {}", group_sid.as_str()),
            group_sid: group_sid.as_str().to_string(),
            old_slots: None,
        });
        self.selection = vec![group_sid.as_str().to_string()];
        self.status = "已编组(Ctrl+G)".into();
    }

    pub(crate) fn ungroup_selection(&mut self) {
        // 多选解散全部组(AI 行为);散开后重选原成员
        let groups: Vec<String> = self
            .selection
            .iter()
            .filter(|sid| {
                self.doc
                    .find_by_sid(sid)
                    .and_then(|nid| self.doc.nodes.get(nid))
                    .is_some_and(|n| matches!(n.kind, NodeKind::Group))
            })
            .cloned()
            .collect();
        if groups.is_empty() {
            self.status = "取消编组:选中对象里没有编组".into();
            return;
        }
        let mut members: Vec<String> = Vec::new();
        for g in &groups {
            if let Some(gid) = self.doc.find_by_sid(g) {
                for c in &self.doc.nodes.get(gid).unwrap().children {
                    members.push(self.doc.nodes.get(*c).unwrap().sid.as_str().to_string());
                }
            }
        }
        for g in groups {
            self.exec(Command::Ungroup {
                group_sid: g,
                captured: None,
            });
        }
        self.selection = members;
        self.status = "已取消编组(Ctrl+Shift+G)".into();
    }

    /// 剪贴板:复制所选子树(非破坏;NodeTree::from_document 只克隆)。
    pub(crate) fn clipboard_copy(&mut self) {
        if self.selection.is_empty() {
            self.status = "剪贴板:未选中对象".into();
            return;
        }
        let mut buf = Vec::new();
        for sid in &self.selection {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                if let Some(n) = self.doc.nodes.get(nid) {
                    let parent_sid = n
                        .parent
                        .and_then(|p| self.doc.nodes.get(p))
                        .map(|p| p.sid.as_str().to_string())
                        .unwrap_or_default();
                    if let Some(tree) = vb_doc::model::NodeTree::from_document(&self.doc, nid) {
                        buf.push((parent_sid, tree));
                    }
                }
            }
        }
        self.clipboard = buf;
        self.paste_offset = 0;
        self.status = format!("已复制 {} 个对象", self.clipboard.len());
    }

    /// 剪贴板:粘贴。`in_place` = 原坐标(AI 的贴在前面);否则按 16px 递增偏移。
    pub(crate) fn clipboard_paste(&mut self, in_place: bool) {
        if self.clipboard.is_empty() {
            self.status = "剪贴板为空".into();
            return;
        }
        let offset = if in_place { 0 } else { self.paste_offset };
        if !in_place {
            // 就地粘贴不消耗偏移预算(否则后续普通粘贴多跳 16px)
            self.paste_offset += 1;
        }
        let dx = (offset * 16) as f64;
        let dy = (offset * 16) as f64;

        // 深拷贝出命令序列(整批一个 undo 条目)
        let entries = self.clipboard.clone();
        let mut cmds: Vec<Command> = Vec::new();
        let mut pasted_sids: Vec<String> = Vec::new();
        for (parent_sid, tree) in &entries {
            let mut tree = tree.clone();
            re_sid_tree(&mut tree, &mut self.doc);
            if !in_place {
                tree.node.geom.x += dx;
                tree.node.geom.y += dy;
            }
            pasted_sids.push(tree.node.sid.as_str().to_string());
            cmds.push(Command::Insert {
                parent_sid: parent_sid.clone(),
                index: usize::MAX,
                tree,
            });
        }
        self.exec(Command::Compound { cmds });
        self.selection = pasted_sids;
        self.status = format!(
            "已粘贴 {} 个对象{}",
            self.clipboard.len(),
            if in_place { "(就地)" } else { "" }
        );
    }

    /// 分布(P3.8 尾巴):≥3 个选中时,让相邻对象间距相等。
    /// `horizontal` = 水平分布;否则垂直。
    pub(crate) fn distribute_selection(&mut self, horizontal: bool) {
        let mut items: Vec<(String, Geom)> = Vec::new();
        for sid in &self.selection {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                if let Some(n) = self.doc.nodes.get(nid) {
                    items.push((sid.clone(), n.geom));
                }
            }
        }
        if items.len() < 3 {
            self.status = "分布需要至少 3 个对象".into();
            return;
        }
        // 按前缘排序(左→右或上→下):游标按前缘推进,排序也必须按前缘,
        // 否则尺寸悬殊时右缘序 ≠ 前缘序,分布结果互相穿越
        if horizontal {
            items.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        } else {
            items.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
        }
        let first = items.first().unwrap().1;
        let last = items.last().unwrap().1;
        // 首尾不动,中间等间距
        let (total_span, _size_sum): (f64, f64) = if horizontal {
            let span = (last.x + last.w) - first.x - items.iter().map(|(_, g)| g.w).sum::<f64>();
            (span, items.iter().map(|(_, g)| g.w).sum())
        } else {
            let span = (last.y + last.h) - first.y - items.iter().map(|(_, g)| g.h).sum::<f64>();
            (span, items.iter().map(|(_, g)| g.h).sum())
        };
        let n = items.len();
        if n < 2 {
            return;
        }
        let gap = total_span / (n - 1) as f64;
        let mut cmds: Vec<Command> = Vec::new();
        let mut cursor = if horizontal { first.x } else { first.y };
        for (i, (sid, g)) in items.iter().enumerate() {
            if i == 0 || i == n - 1 {
                // 首尾不动,但仍推进游标
                cursor += if horizontal { g.w + gap } else { g.h + gap };
                continue;
            }
            let mut ng = *g;
            if horizontal {
                ng.x = cursor.round();
                cursor += ng.w + gap;
            } else {
                ng.y = cursor.round();
                cursor += ng.h + gap;
            }
            cmds.push(Command::SetGeom {
                sid: sid.clone(),
                new: ng,
                old: None,
                old_declared: None,
            });
        }
        let count = cmds.len();
        let axis = if horizontal { "水平" } else { "垂直" };
        if count > 0 {
            self.exec(Command::Compound { cmds });
            self.status = format!("已{}分布 {count} 个对象(间距 {gap:.0}px)", axis);
        }
    }

    /// 钢笔结束:closed = 闭合路径;否则开放路径(仅描边)。
    /// 锚点 → BezPath(直线段;平滑手柄属 P4 后半)。
    pub(crate) fn finish_pen(&mut self, closed: bool) {
        let pts = std::mem::take(&mut self.pen_points);
        if pts.len() < 2 {
            return;
        }
        // 平滑点(拖出出手柄)→ 三次贝塞尔;单柄 → 二次;无柄 → 直线(06 篇 §5.3)
        let mut path = vb_common::geom::BezPath::new();
        path.move_to(vb_common::geom::Point::new(
            pts[0].anchor.0,
            pts[0].anchor.1,
        ));
        let n = pts.len();
        let segs = if closed { n } else { n - 1 };
        for i in 1..=segs {
            let prev = &pts[i - 1];
            let cur = &pts[i % n];
            let end = vb_common::geom::Point::new(cur.anchor.0, cur.anchor.1);
            let c1 = prev.h_out.map(|(x, y)| vb_common::geom::Point::new(x, y));
            let c2 = cur.h_in().map(|(x, y)| vb_common::geom::Point::new(x, y));
            match (c1, c2) {
                (Some(a), Some(b)) => path.curve_to(a, b, end),
                (Some(a), None) | (None, Some(a)) => path.quad_to(a, end),
                (None, None) => path.line_to(end),
            }
        }
        if closed {
            path.close_path();
        }
        self.create_vector_node(path, closed);
    }

    /// 由路径创建矢量节点(P4.5)。
    fn create_vector_node(&mut self, path: vb_common::geom::BezPath, closed: bool) {
        use kurbo::Shape;
        let (minx, miny, maxx, maxy) = {
            let bb = path.bounding_box();
            (bb.x0, bb.y0, bb.x1, bb.y1)
        };
        let sid = self.doc.alloc_sid();
        let mut n = vb_doc::model::Node::new(
            NodeKind::Vector { path: path.clone() },
            format!("路径 {}", sid.as_str()),
            sid.clone(),
        );
        n.tag = "svg".into();
        n.geom = Geom {
            x: minx.round(),
            y: miny.round(),
            w: (maxx - minx).ceil().max(1.0),
            h: (maxy - miny).ceil().max(1.0),
        };
        // 路径以节点原点为基准:平移到 geom.x/y
        let mut shifted = vb_common::geom::BezPath::new();
        for el in &path.elements().to_vec() {
            use vb_common::geom::PathEl;
            match &el {
                PathEl::MoveTo(p) => {
                    shifted.move_to(*p - vb_common::geom::Vec2::new(n.geom.x, n.geom.y))
                }
                PathEl::LineTo(p) => {
                    shifted.line_to(*p - vb_common::geom::Vec2::new(n.geom.x, n.geom.y))
                }
                PathEl::QuadTo(c, p) => shifted.quad_to(
                    *c - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                    *p - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                ),
                PathEl::CurveTo(c1, c2, p) => shifted.curve_to(
                    *c1 - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                    *c2 - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                    *p - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                ),
                PathEl::ClosePath => shifted.close_path(),
            }
        }
        n.kind = NodeKind::Vector { path: shifted };
        if closed {
            n.style.push(vb_css::Decl {
                prop: "fill".into(),
                // vb-token-ok: 钢笔闭合形状的默认填充(文档内容,非 UI 皮肤,同下方直线描边)
                value: "#d4d4d4".into(),
                important: false,
            });
        }
        n.style.push(vb_css::Decl {
            prop: "stroke".into(),
            // vb-token-ok: 钢笔路径的默认描边(文档内容,非 UI 皮肤)
            value: "#1a1a1a".into(),
            important: false,
        });
        n.style.push(vb_css::Decl {
            prop: "stroke-width".into(),
            value: "1.5px".into(),
            important: false,
        });
        let parent = self.insert_target(n.geom.x, n.geom.y);
        let (dx, dy) = self.world_to_parent_local(parent, 0.0, 0.0);
        n.geom.x += dx;
        n.geom.y += dy;
        let parent_sid = self.doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        let ab_len = self.doc.nodes.get(parent).unwrap().children.len();
        let tree = vb_doc::model::NodeTree {
            node: n,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid,
            index: ab_len,
            tree,
        });
        self.selection = vec![sid.as_str().to_string()];
    }

    /// 直接选择/剪刀共用的顶点枚举:返回 (元素序号, 世界坐标)。
    /// 锚点元素 = MoveTo/LineTo/QuadTo/CurveTo 的终点(平滑钢笔会产出曲线)。
    fn path_anchor_points(
        &self,
        nid: vb_doc::model::NodeId,
    ) -> Vec<(usize, vb_common::geom::Point)> {
        use vb_common::geom::PathEl;
        let Some(n) = self.doc.nodes.get(nid) else {
            return vec![];
        };
        let NodeKind::Vector { path } = &n.kind else {
            return vec![];
        };
        let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
            return vec![];
        };
        path.elements()
            .iter()
            .enumerate()
            .filter_map(|(i, el)| {
                let p = match el {
                    PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
                    PathEl::QuadTo(_, p) => *p,
                    PathEl::CurveTo(_, _, p) => *p,
                    _ => return None,
                };
                Some((i, vb_common::geom::Point::new(bb.x0 + p.x, bb.y0 + p.y)))
            })
            .collect()
    }

    /// 直接选择:命中检测 — 找光标附近矢量节点的顶点。返回 (sid, 顶点序号)。
    pub(crate) fn find_vector_vertex(&self, wx: f64, wy: f64, tol: f64) -> Option<(String, usize)> {
        for &ab in &self.doc.artboards {
            let mut ids = Vec::new();
            self.doc.subtree(ab, &mut ids);
            for id in ids {
                let Some(n) = self.doc.nodes.get(id) else {
                    continue;
                };
                if n.hidden || n.locked {
                    continue;
                }
                if matches!(n.kind, NodeKind::Vector { .. }) {
                    // 单个节点取不到 bbox 只跳过该节点(此前 `?` 会放弃整棵树)
                    for (i, p) in self.path_anchor_points(id) {
                        if (p.x - wx).hypot(p.y - wy) <= tol {
                            return Some((n.sid.as_str().to_string(), i));
                        }
                    }
                }
            }
        }
        None
    }

    /// 取矢量节点的顶点绝对坐标(直接选择渲染/拖拽用)。
    pub(crate) fn vector_vertices(&self, sid: &str) -> Vec<(usize, f64, f64)> {
        let Some(nid) = self.doc.find_by_sid(sid) else {
            return vec![];
        };
        let Some(n) = self.doc.nodes.get(nid) else {
            return vec![];
        };
        let NodeKind::Vector { path } = &n.kind else {
            return vec![];
        };
        let bb = vb_tools::abs_bbox_world(&self.doc, nid).unwrap_or(vb_common::geom::Rect::ZERO);
        let els: Vec<vb_common::geom::PathEl> = path.elements().to_vec();
        els.iter()
            .enumerate()
            .filter_map(|(i, el)| {
                use vb_common::geom::PathEl;
                let p = match el {
                    PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
                    PathEl::QuadTo(_, p) => *p,
                    PathEl::CurveTo(_, _, p) => *p,
                    _ => return None,
                };
                Some((i, bb.x0 + p.x, bb.y0 + p.y))
            })
            .collect()
    }

    /// 对齐(P3.8):多选 → 在选择包围盒内对齐;单选 → 对齐所属画板。
    /// 路径查找器(C1):对选中的两个矢量路径执行布尔运算。
    /// AI 语义:减去顶层 = z 序在上者减去在下者;联集/交集/差集与顺序无关。
    pub(crate) fn path_boolean(&mut self, op: vb_tools::boolean::BooleanOp) {
        if self.selection.len() != 2 {
            self.status = "路径查找器:需要恰好选中 2 个对象".into();
            return;
        }
        let (sid_a, sid_b) = (self.selection[0].clone(), self.selection[1].clone());
        let (id_a, id_b) = match (self.doc.find_by_sid(&sid_a), self.doc.find_by_sid(&sid_b)) {
            (Some(a), Some(b)) => (a, b),
            _ => return,
        };
        for id in [id_a, id_b] {
            if !matches!(
                self.doc.nodes.get(id).map(|n| &n.kind),
                Some(NodeKind::Vector { .. })
            ) {
                self.status = "路径查找器:只支持矢量路径(钢笔创建的形状)".into();
                return;
            }
        }
        // 减法族(减去顶层 / 减去后方对象):z 序在上者为 lhs(被减数)
        let (lhs, rhs) = if matches!(
            op,
            vb_tools::boolean::BooleanOp::Subtract | vb_tools::boolean::BooleanOp::SubtractBack
        ) {
            let za = z_order(&self.doc, id_a);
            let zb = z_order(&self.doc, id_b);
            if za >= zb {
                (sid_a, sid_b)
            } else {
                (sid_b, sid_a)
            }
        } else {
            (sid_a.clone(), sid_b.clone())
        };
        let lhs_id = self.doc.find_by_sid(&lhs).unwrap();
        let rhs_id = self.doc.find_by_sid(&rhs).unwrap();
        let (new_path, new_geom) =
            match vb_tools::boolean::path_boolean_nodes(&self.doc, op, lhs_id, rhs_id) {
                Ok(v) => v,
                Err(e) => {
                    self.status = format!("路径查找器:{e}");
                    return;
                }
            };
        self.exec(Command::PathBoolean {
            op: op.as_str().into(),
            lhs_sid: lhs.clone(),
            rhs_sid: rhs.clone(),
            new_path,
            new_geom,
            captured: None,
        });
        self.selection = vec![lhs];
        self.status = format!("路径查找器:{}", op.as_str());
    }

    pub(crate) fn align_selection(&mut self, mode: &str) {
        use vb_tools::align::AbsBox;
        if self.selection.is_empty() {
            self.status = "对齐:未选中对象".into();
            return;
        }
        // (sid, 当前几何, 绝对 bbox)
        let mut items: Vec<(String, Geom, vb_common::geom::Rect)> = Vec::new();
        for sid in &self.selection {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                if let Some(bb) = vb_tools::abs_bbox(&self.doc, nid) {
                    if let Some(n) = self.doc.nodes.get(nid) {
                        items.push((sid.clone(), n.geom, bb));
                    }
                }
            }
        }
        if items.is_empty() {
            return;
        }
        // 目标框由**纯函数**给出(阶段 2 / 03-5「对齐到」三选一):
        // 选区(多选=公共包围盒;单选回退画板)/ 关键对象(最后选中者)/ 画板。
        let Some(target) = super::align_panel::align_target_box(&self.doc, &items, self.align_to)
        else {
            self.status = "对齐:找不到目标框".into();
            return;
        };
        let Some(m) = vb_tools::align::AlignMode::parse(mode) else {
            self.status = format!("对齐:未知模式 {mode}");
            return;
        };

        let mut cmds: Vec<Command> = Vec::new();
        for (sid, g, bb) in &items {
            // 位移在**绝对系**里算:平移与参照系无关,可直接作用于父相对 geom
            let (dx, dy) = vb_tools::align::aligned_delta(m, &AbsBox::from_rect(*bb), &target);
            if dx.abs() < 0.5 && dy.abs() < 0.5 {
                continue;
            }
            cmds.push(Command::SetGeom {
                sid: sid.clone(),
                new: Geom {
                    x: g.x + dx,
                    y: g.y + dy,
                    w: g.w,
                    h: g.h,
                },
                old: None,
                old_declared: None,
            });
        }
        if cmds.is_empty() {
            self.status = "对齐:无需移动".into();
            return;
        }
        let n = cmds.len();
        self.exec(Command::Compound { cmds });
        self.status = format!("已对齐 {n} 个对象({mode})");
    }

    /// 层序调整:delta=+1 前移一层(z 序升),-1 后移;front/back 用 ±10000
    pub(crate) fn reorder(&mut self, sid: &str, delta: i32) {
        let Some(nid) = self.doc.find_by_sid(sid) else {
            return;
        };
        let Some(parent) = self.doc.nodes.get(nid).and_then(|n| n.parent) else {
            return;
        };
        let len = self.doc.nodes.get(parent).unwrap().children.len();
        let cur = self
            .doc
            .nodes
            .get(parent)
            .unwrap()
            .children
            .iter()
            .position(|&c| c == nid)
            .unwrap_or(0);
        let new_index = if delta.abs() >= 10000 {
            if delta > 0 {
                len - 1
            } else {
                0
            }
        } else {
            (cur as i32 + delta).clamp(0, len as i32 - 1) as usize
        };
        if new_index == cur {
            return;
        }
        let parent_sid = self.doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        self.exec(Command::Move {
            sid: sid.to_string(),
            new_parent_sid: parent_sid,
            new_index,
            old: None,
        });
    }

    /// 再次变换 `Mod+D`(副文档 03-4-2):重放**上一次变换**。
    ///
    /// 上一次变换由三条路径写入 `last_transform`:
    /// 画布拖动结束(位移 / 缩放 / 旋转)、变换数值面板提交。
    /// 几何重放走纯函数 `transform_panel::replay`(缩放以参考点为轴心),
    /// 旋转落到 `transform` 声明(与旋转手柄同一条命令)。
    pub(crate) fn transform_again(&mut self) {
        let Some(d) = self.last_transform else {
            self.status = "没有可再次的变换(先移动/缩放/旋转一次)".into();
            return;
        };
        if self.selection.is_empty() {
            self.status = "再次变换:先选中对象".into();
            return;
        }
        let rp = self.transform_ref;
        let mut n = 0;
        for sid in self.selection.clone() {
            let Some(nid) = self.doc.find_by_sid(&sid) else {
                continue;
            };
            let Some(node) = self.doc.nodes.get(nid) else {
                continue;
            };
            let g0 = node.geom;
            let tf = node.style_get("transform").unwrap_or("").to_string();
            let (angle, sx, sy) = super::transform_panel::parse_transform(&tf);
            let (g1, a1) = super::transform_panel::replay(g0, angle, &d, rp);
            self.exec(Command::SetGeom {
                sid: sid.clone(),
                new: g1,
                old: None,
                old_declared: None,
            });
            // 旋转增量非零(或原本就有倾斜要保留)时才动 transform 声明
            let keep_skew = sx.abs() > 1e-9 || sy.abs() > 1e-9;
            if (a1 - angle).abs() > 1e-9 || keep_skew {
                if let Some(cmd) =
                    super::transform_panel::set_transform_cmd(&self.doc, &sid, a1, sx, sy)
                {
                    self.exec(cmd);
                }
            }
            n += 1;
        }
        self.status = format!(
            "再次变换(Ctrl+D):{} 个对象(位移 {:.0},{:.0} · 缩放 ×{:.2}/×{:.2} · 旋转 {:.1}°)",
            n, d.dx, d.dy, d.kx, d.ky, d.d_angle
        );
    }
}

impl VellumApp {
    /// 在指定几何处创建 Box 形状(矩形/椭圆由当前工具决定),可撤销。
    pub(crate) fn create_shape(&mut self, mut g: Geom) {
        // 直线工具:创建 2px 高的细长色条(HTML 中即一条水平线;斜线待 P4 矢量路径)
        let is_line = self.tool == Tool::Line;
        if is_line {
            g.h = 2.0;
        }
        let sid = self.doc.alloc_sid();
        let name = if is_line { "直线" } else { "矩形" };
        let mut n = vb_doc::model::Node::new(
            NodeKind::Box,
            format!("{} {}", name, sid.as_str()),
            sid.clone(),
        );
        n.geom = g;
        if self.tool == Tool::Ellipse {
            n.style.push(vb_css::Decl {
                prop: "border-radius".into(),
                value: "50%".into(),
                important: false,
            });
        }
        if is_line {
            n.style.push(vb_css::Decl {
                prop: "background-color".into(),
                value: "#1a1a1a".into(), // vb-token-ok: 直线是文档内容
                important: false,
            });
        } else {
            n.style.push(vb_css::Decl {
                prop: "background-color".into(),
                value: "#d4d4d4".into(), // vb-token-ok: 新建形状默认填充(文档内容,非 UI 皮肤)
                important: false,
            });
            n.style.push(vb_css::Decl {
                prop: "border".into(),
                value: "1px solid #1a1a1a".into(),
                important: false,
            });
        }
        let parent = self.insert_target(g.x, g.y);
        let (lx, ly) = self.world_to_parent_local(parent, g.x, g.y);
        n.geom.x = lx;
        n.geom.y = ly;
        let parent_sid = self.doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        let plen = self.doc.nodes.get(parent).unwrap().children.len();
        let tree = vb_doc::model::NodeTree {
            node: n,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid,
            index: plen,
            tree,
        });
        self.selection = vec![sid.as_str().to_string()];
        self.status = "已创建对象".into();
    }

    /// 文字工具:点文本(area=None)/ 区域文本(拖框)。
    /// 创建后立即进入编辑(AI 行为)。
    /// 04-3-3:模式取 `text_mode_pending`(Shift+T 循环;拖框恒为区域),
    /// 样式继承字符面板「新建文本默认样式」(替换旧写死 24px/黑)。
    pub(crate) fn create_text_node(&mut self, point: (f64, f64), area: Option<Geom>) {
        let parent = self.insert_target(point.0, point.1);
        let (lx, ly) = self.world_to_parent_local(parent, point.0, point.1);
        let sid = self.doc.alloc_sid();
        // 显式拖框 = 区域文本;否则按待用模式(Shift+T 循环的落点)
        let (mode, g) = match area {
            Some(mut a) => {
                a.x = lx;
                a.y = ly;
                (vb_doc::model::TextMode::Area, a)
            }
            None => {
                let w = match self.text_mode_pending {
                    vb_doc::model::TextMode::Point => 200.0,
                    vb_doc::model::TextMode::Area => 320.0,
                };
                let h = match self.text_mode_pending {
                    vb_doc::model::TextMode::Point => 36.0,
                    vb_doc::model::TextMode::Area => 120.0,
                };
                (self.text_mode_pending, Geom { x: lx, y: ly, w, h })
            }
        };
        let mut n = vb_doc::model::Node::new(
            NodeKind::Text {
                text: "双击编辑文本".into(),
                mode,
                segments: Vec::new(),
            },
            format!("文本 {}", sid.as_str()),
            sid.clone(),
        );
        n.tag = "p".into();
        n.geom = g;
        // 默认样式 = 字符面板当前默认(全部落白名单;不再写死 24px/黑)
        let d = self.text_default.clone();
        n.style.push(vb_css::Decl {
            prop: "font-size".into(),
            value: format!(
                "{}px",
                vb_common::units::fmt_num(d.font_size.unwrap_or(24.0))
            ),
            important: false,
        });
        if let Some(c) = &d.color {
            n.style.push(vb_css::Decl {
                prop: "color".into(),
                // 默认 #1a1a1a 来自面板默认样式(文档内容,非 UI 皮肤)
                value: c.clone(),
                important: false,
            });
        }
        if d.bold == Some(true) {
            n.style.push(vb_css::Decl {
                prop: "font-weight".into(),
                value: "700".into(),
                important: false,
            });
        }
        if d.italic == Some(true) {
            n.style.push(vb_css::Decl {
                prop: "font-style".into(),
                value: "italic".into(),
                important: false,
            });
        }
        if let Some(ff) = &d.font_family {
            n.style.push(vb_css::Decl {
                prop: "font-family".into(),
                value: ff.clone(),
                important: false,
            });
        }
        if let Some(lh) = d.line_height {
            n.style.push(vb_css::Decl {
                prop: "line-height".into(),
                value: format!("{}px", vb_common::units::fmt_num(lh)),
                important: false,
            });
        }
        if let Some(ls) = d.letter_spacing {
            n.style.push(vb_css::Decl {
                prop: "letter-spacing".into(),
                value: format!("{}px", vb_common::units::fmt_num(ls)),
                important: false,
            });
        }
        let parent_sid = self.doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        let plen = self.doc.nodes.get(parent).unwrap().children.len();
        let tree = vb_doc::model::NodeTree {
            node: n,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid,
            index: plen,
            tree,
        });
        self.selection = vec![sid.as_str().to_string()];
        self.editing_text = Some(sid.as_str().to_string());
        self.status = if area.is_some() {
            "已创建区域文本(拖框宽度即换行宽度)".into()
        } else {
            "已创建点文本(输入内容,Ctrl+Enter 提交)".into()
        };
    }

    /// 画板工具:在世界坐标处新建画板(画板 geom 即世界坐标)。
    pub(crate) fn create_artboard(&mut self, g: Geom) {
        let count = self.doc.artboards.len();
        let sid = self.doc.alloc_sid();
        let mut n = vb_doc::model::Node::new(
            NodeKind::Artboard,
            format!("画板 {}", count + 1),
            sid.clone(),
        );
        n.geom = g;
        let root_sid = self
            .doc
            .nodes
            .get(self.doc.root)
            .unwrap()
            .sid
            .as_str()
            .to_string();
        let tree = vb_doc::model::NodeTree {
            node: n,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid: root_sid,
            index: usize::MAX,
            tree,
        });
        self.selection = vec![sid.as_str().to_string()];
        self.status = format!("已新建画板 {}({}×{})", count + 1, g.w as i64, g.h as i64);
    }

    /// 新建默认画板(S1-c 控制面板「+画板」与画板面板「+ 新建」共用;
    /// 纵向堆到现有画板最下方,经 Insert 命令入 undo 栈)。
    pub(crate) fn add_default_artboard(&mut self) {
        let name = format!("画板 {}", self.doc.artboards.len() + 1);
        let sid = self.doc.alloc_sid();
        let mut n = vb_doc::model::Node::new(NodeKind::Artboard, name.clone(), sid.clone());
        n.geom = Geom {
            x: 0.0,
            y: 0.0,
            w: 1440.0,
            h: 900.0,
        };
        // 纵向堆到最下方
        n.geom.y = self
            .doc
            .artboards
            .iter()
            .filter_map(|&a| self.doc.nodes.get(a).map(|n| n.geom.y + n.geom.h))
            .fold(0.0f64, f64::max)
            + 80.0;
        let root_sid = self
            .doc
            .nodes
            .get(self.doc.root)
            .unwrap()
            .sid
            .as_str()
            .to_string();
        let tree = vb_doc::model::NodeTree {
            node: n,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid: root_sid,
            index: usize::MAX,
            tree,
        });
        self.say(format!("已新建 {name}(Shift+O 画板工具)"));
    }

    /// 直接选择锚点改位(数值框 / 画布拖拽共用的命令构造):
    /// 把第 `vi` 个元素的终点设为节点本地坐标 `local`,返回 SetVector
    /// 命令(调用方决定 exec / 入提交会话)。
    pub(crate) fn build_anchor_cmd(
        &self,
        sid: &str,
        vi: usize,
        local: (f64, f64),
    ) -> Option<Command> {
        use vb_common::geom::PathEl;
        let nid = self.doc.find_by_sid(sid)?;
        let els: Vec<PathEl> = match &self.doc.nodes.get(nid)?.kind {
            NodeKind::Vector { path } => path.elements().to_vec(),
            _ => return None,
        };
        let new_pt = vb_common::geom::Point::new(local.0, local.1);
        let mut els = els;
        els[vi] = match els[vi] {
            PathEl::MoveTo(_) => PathEl::MoveTo(new_pt),
            PathEl::LineTo(_) => PathEl::LineTo(new_pt),
            PathEl::QuadTo(c, _) => PathEl::QuadTo(c, new_pt),
            PathEl::CurveTo(c1, c2, _) => PathEl::CurveTo(c1, c2, new_pt),
            other => other,
        };
        let mut np = vb_common::geom::BezPath::new();
        for el in els {
            np.push(el);
        }
        Some(Command::SetVector {
            sid: sid.to_string(),
            new: np,
            old: None,
        })
    }

    /// 吸管:命中对象取填充色应用到选区;Alt = 吸取全部样式替换。
    pub(crate) fn eyedropper_pick(&mut self, alt: bool) {
        let (wx, wy) = self.cursor_world;
        let Some(nid) = self.pick_at_world(wx, wy) else {
            self.status = "吸管:未命中对象".into();
            return;
        };
        let src = self.doc.nodes.get(nid).unwrap();
        if alt {
            // 全部样式:整份替换(目标无声明才允许空)
            let style = src.style.clone();
            if style.is_empty() {
                self.status = "吸管:目标没有样式可吸取".into();
                return;
            }
            let targets = self.selection.clone();
            if targets.is_empty() {
                self.status = format!("已吸取 {} 条样式(先选中对象再点应用)", style.len());
                return;
            }
            for sid in targets {
                self.exec(Command::SetStyle {
                    sid,
                    new: style.clone(),
                    old: None,
                });
            }
            self.status = format!("已应用全部样式({} 条声明)", style.len());
        } else {
            let Some(color) = src.fill_color() else {
                self.status = "吸管:目标没有填充色(Alt 可吸全部样式)".into();
                return;
            };
            let hex = color.to_shortest_hex();
            let targets = self.selection.clone();
            if targets.is_empty() {
                self.status = format!("已取色 {hex}(先选中对象再点应用)");
                return;
            }
            for sid in targets {
                let Some(t) = self.doc.find_by_sid(&sid) else {
                    continue;
                };
                let mut style = self.doc.nodes.get(t).unwrap().style.clone();
                if let Some(d) = style.iter_mut().find(|d| d.prop == "background-color") {
                    d.value = hex.clone();
                } else {
                    style.push(vb_css::Decl {
                        prop: "background-color".into(),
                        value: hex.clone(),
                        important: false,
                    });
                }
                self.exec(Command::SetStyle {
                    sid,
                    new: style,
                    old: None,
                });
            }
            self.status = format!("已应用填充 {hex}");
        }
    }

    /// 渐变工具:对选区写入 `linear-gradient(<角度>deg, <原填充> 0%, #ffffff 100%)`。
    /// 起色标 = 对象现有填充(无则默认灰),止色标 = 白;逐帧 Compound 应用,
    /// 合并窗口内整次拖动为一条 undo。
    pub(crate) fn apply_gradient_to_selection(&mut self, angle: f64) {
        let targets = self.selection.clone();
        let mut cmds = Vec::new();
        for sid in targets {
            let Some(t) = self.doc.find_by_sid(&sid) else {
                continue;
            };
            let n = self.doc.nodes.get(t).unwrap();
            let c1 = n
                .fill_color()
                .map(|c| c.to_shortest_hex())
                .unwrap_or_else(|| "#d4d4d4".into()); // vb-token-ok: 文档内容色
            let value = format!("linear-gradient({angle:.0}deg, {c1} 0%, #ffffff 100%)");
            let mut style = n.style.clone();
            if let Some(d) = style.iter_mut().find(|d| d.prop == "background-image") {
                d.value = value;
            } else {
                style.push(vb_css::Decl {
                    prop: "background-image".into(),
                    value,
                    important: false,
                });
            }
            cmds.push(Command::SetStyle {
                sid,
                new: style,
                old: None,
            });
        }
        if !cmds.is_empty() {
            self.exec(Command::Compound { cmds });
        }
    }

    /// 剪刀:在命中锚点处剪开矢量路径。
    /// 闭路 → 开口(锚点处断开,Z 变显式线段);开路 → 分段为两个节点。
    pub(crate) fn scissors_cut(&mut self, wx: f64, wy: f64, tol: f64) {
        use vb_common::geom::PathEl;
        let Some((sid, idx)) = self.find_vector_vertex(wx, wy, tol) else {
            self.status = "剪刀:请在矢量路径的锚点上单击".into();
            return;
        };
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            return;
        };
        let els: Vec<PathEl> = match self.doc.nodes.get(nid).unwrap().kind {
            NodeKind::Vector { ref path } => path.elements().to_vec(),
            _ => return,
        };
        let local_end = |e: &PathEl| -> vb_common::geom::Point {
            match e {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
                PathEl::QuadTo(_, p) => *p,
                PathEl::CurveTo(_, _, p) => *p,
                PathEl::ClosePath => vb_common::geom::Point::new(f64::NAN, f64::NAN),
            }
        };
        let closed = matches!(els.last(), Some(PathEl::ClosePath));
        if closed {
            // 旋转元素序列使命中锚点成为起点;原 M 降级为 L;去 Z。
            // 环 a0→…→a_k→…→a0 在 a_k 处断开:全部边保留,首尾都是 a_k
            let start_pt = local_end(&els[idx]);
            let m0 = local_end(&els[0]);
            let mut new_els: Vec<PathEl> = vec![PathEl::MoveTo(start_pt)];
            if idx + 1 < els.len() - 1 {
                new_els.extend_from_slice(&els[idx + 1..els.len() - 1]);
            }
            new_els.push(PathEl::LineTo(m0));
            if idx >= 1 {
                new_els.extend_from_slice(&els[1..=idx]);
            }
            let mut np = vb_common::geom::BezPath::new();
            for e in new_els {
                np.push(e);
            }
            self.exec(Command::SetVector {
                sid,
                new: np,
                old: None,
            });
            self.status = "剪刀:已剪开(路径开放,填充按隐式闭合渲染)".into();
        } else {
            // 开路分段:[0..=idx] 与 [idx..end];端点处无需剪
            if idx == 0 || idx >= els.len() - 1 {
                self.status = "剪刀:锚点已在路径端点,无需剪".into();
                return;
            }
            let split_pt = local_end(&els[idx]);
            let mut first = vb_common::geom::BezPath::new();
            for e in &els[..=idx] {
                first.push(*e);
            }
            let mut second = vb_common::geom::BezPath::new();
            second.push(PathEl::MoveTo(split_pt));
            for e in &els[idx + 1..] {
                second.push(*e);
            }
            // 第二段 = 克隆节点(新 sid)+ 替换路径;第一段 = SetVector
            let n = self.doc.nodes.get(nid).unwrap().clone();
            let parent_sid = n
                .parent
                .and_then(|p| self.doc.nodes.get(p))
                .map(|p| p.sid.as_str().to_string())
                .unwrap_or_default();
            let mut second_node = n.clone();
            second_node.sid = self.doc.alloc_sid();
            second_node.name = format!("{} 2", n.name);
            second_node.kind = NodeKind::Vector { path: second };
            let new_sid = second_node.sid.as_str().to_string();
            let tree = vb_doc::model::NodeTree {
                node: second_node,
                children: vec![],
            };
            self.exec(Command::Compound {
                cmds: vec![
                    Command::SetVector {
                        sid: sid.clone(),
                        new: first,
                        old: None,
                    },
                    Command::Insert {
                        parent_sid,
                        index: usize::MAX,
                        tree,
                    },
                ],
            });
            self.selection = vec![sid, new_sid];
            self.status = "剪刀:已剪开为两段(两段均已选中)".into();
        }
    }
}

/// 节点在父级 children 中的 z 序(同父;不同父时按 id 序兜底)。
fn z_order(doc: &vb_doc::model::Document, id: vb_doc::model::NodeId) -> usize {
    doc.nodes
        .get(id)
        .and_then(|n| n.parent)
        .and_then(|p| doc.nodes.get(p))
        .map(|p| {
            p.children
                .iter()
                .position(|&c| c == id)
                .unwrap_or(usize::MAX)
        })
        .unwrap_or(usize::MAX)
}
