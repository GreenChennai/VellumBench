//! 选择工具族:直接选择(A)顶点编辑、双击(隔离/文本编辑)、点选 /
//! 编组选择 / 框选拖拽起手、MoveObj 拖动(智能参考线 + 多选整体位移)
//! 与松手收束。
//!
//! 06-1 自 `canvas_input.rs` 按工具族拆出(纯搬移,零行为变化):
//! 阶段管线与调用顺序见 `mod.rs`。

use vb_doc::commands::Command;
use vb_doc::model::{Geom, NodeKind};

use super::Drag;
use crate::app::{re_sid_tree, Tool, VellumApp};

impl VellumApp {
    /// 直接选择单击(不消费)/ 拖拽锚点(原语义:均不拦截后续事件)。
    pub(super) fn direct_select_click_and_drag(
        &mut self,
        response: &egui::Response,
        rect: egui::Rect,
    ) {
        // 直接选择单击(A):命中矢量顶点 → 选中节点并记录待拖(P4.6)
        if response.clicked() && self.tool == Tool::DirectSelect {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.ds_vertex = self.find_vector_vertex(wx, wy, 8.0 / self.camera.zoom);
                if let Some((sid, _)) = &self.ds_vertex {
                    self.selection = vec![sid.clone()];
                }
            }
        }

        // 直接选择(A):拖拽锚点 → SetVector 命令入 undo 栈(P4.6 接线)。
        // 连续拖拽落在 500ms 合并窗口内 = 一条 undo 条目(与数值框同策略)
        if self.tool == Tool::DirectSelect && response.dragged() {
            if let (Some(p0), Some((sid, vi))) =
                (response.interact_pointer_pos(), self.ds_vertex.as_ref())
            {
                let p = p0 - rect.min;
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                let (sid, vi) = (sid.clone(), *vi);
                if let Some(nid) = self.doc.find_by_sid(&sid) {
                    // 顶点存节点本地坐标:新本地 = 光标世界 - 节点世界原点
                    // (命令构造与控制面板锚点数值框共用 build_anchor_cmd,S1-c)
                    if let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) {
                        if let Some(cmd) = self.build_anchor_cmd(&sid, vi, (wx - bb.x0, wy - bb.y0))
                        {
                            self.exec(cmd);
                        }
                    }
                }
            }
        }
    }

    /// 双击:进入文本编辑 / 编组进入隔离。返回 true = 本帧后续处理跳过。
    pub(super) fn double_click_edit(
        &mut self,
        response: &egui::Response,
        rect: egui::Rect,
    ) -> bool {
        // 双击:进入文本编辑(AI:双击文本进入编辑)/编组进入隔离
        if response.double_clicked() && self.tool == Tool::Select {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                if let Some(nid) = self.pick_at_world(wx, wy) {
                    let n = self.doc.nodes.get(nid).unwrap();
                    // P4.3 隔离模式:双击编组进入(06 篇 §4.3;嵌套逐层进栈)
                    if matches!(n.kind, NodeKind::Group) {
                        self.isolate_stack.push(nid);
                        self.selection.clear();
                        let crumbs: Vec<String> = self
                            .isolate_stack
                            .iter()
                            .filter_map(|id| self.doc.nodes.get(*id).map(|n| n.name.clone()))
                            .collect();
                        self.status = format!("隔离模式:{}(Esc 退出)", crumbs.join(" / "));
                        return true;
                    }
                    if matches!(n.kind, NodeKind::Text { .. }) {
                        // 04-2(design/06 §3.6):区域文本溢出时双击右下角
                        // 角点 = 自动扩高(几何命令),不进编辑
                        let corner_hit = match (&n.kind, vb_tools::abs_bbox_world(&self.doc, nid)) {
                            (
                                NodeKind::Text {
                                    mode: vb_doc::model::TextMode::Area,
                                    ..
                                },
                                Some(bb),
                            ) => {
                                let (cx, cy) = self.camera.world_to_screen(bb.x1, bb.y1);
                                ((pl.x - cx as f32).hypot(pl.y - cy as f32) as f64) <= 14.0
                                    && crate::app::panels::charpara::area_overflow_px(
                                        &self.doc, nid,
                                    ) > 0.0
                            }
                            _ => false,
                        };
                        if corner_hit {
                            let sid = n.sid.as_str().to_string();
                            if let Some(cmd) =
                                crate::app::panels::charpara::area_fit_height_cmd(&self.doc, &sid)
                            {
                                self.exec(cmd);
                                self.status = "区域文本已自动扩高(SetGeom,可撤销)".into();
                            }
                            return true;
                        }
                        self.editing_text = Some(n.sid.as_str().to_string());
                        self.status =
                            format!("编辑文本:{}(Ctrl+Enter/Esc 提交,再按 Esc 放弃)", n.name);
                    }
                }
            }
        }
        false
    }

    /// 编组选择拖拽起手(命中 → 最近 Group 祖先;无祖先退化为普通选择)。
    pub(super) fn drag_begin_group_select(&mut self, p: egui::Vec2, wx: f64, wy: f64, shift: bool) {
        // 编组选择:命中对象 → 选中其所在编组(最近 Group 祖先);
        // 无编组祖先(顶层对象)时退化为普通选择
        let sid = self.pick_at_world(wx, wy).map(|nid| {
            let mut cur = Some(nid);
            let mut group = None;
            while let Some(c) = cur {
                let n = self.doc.nodes.get(c).unwrap();
                if matches!(n.kind, NodeKind::Group) {
                    group = Some(c);
                    break;
                }
                cur = n.parent;
            }
            self.doc
                .nodes
                .get(group.unwrap_or(nid))
                .unwrap()
                .sid
                .as_str()
                .to_string()
        });
        if let Some(sid) = sid {
            if !self.selection.contains(&sid) {
                if shift {
                    self.selection.push(sid.clone());
                } else {
                    self.selection = vec![sid.clone()];
                }
            }
            let nid = self.doc.find_by_sid(&sid).unwrap();
            let g = self.doc.nodes.get(nid).unwrap().geom;
            let others = self.selection_others(&sid);
            self.drag = Drag::MoveObj {
                sid,
                start_geom: g,
                grab_dx: wx - g.x,
                grab_dy: wy - g.y,
                moved: false,
                others,
            };
        } else {
            self.drag = Drag::Marquee { start: p, cur: p };
            if !shift {
                self.selection.clear();
            }
        }
    }

    /// 选择工具拖拽起手:点选 / Alt 复制 / 框选。
    pub(super) fn drag_begin_select(
        &mut self,
        p: egui::Vec2,
        wx: f64,
        wy: f64,
        alt: bool,
        shift: bool,
    ) {
        let hit = self
            .pick_at_world(wx, wy)
            .map(|nid| self.doc.nodes.get(nid).unwrap().sid.as_str().to_string());
        if let Some(sid) = hit {
            if !self.selection.contains(&sid) {
                if shift {
                    self.selection.push(sid.clone());
                } else {
                    self.selection = vec![sid.clone()];
                }
            }
            // Alt = 复制并拖动(AI 招牌);走 Insert 命令入 undo 栈,
            // 裸 clone_subtree 产生的克隆体永远撤销不掉
            let drag_sid = if alt {
                let nid = self.doc.find_by_sid(&sid).unwrap();
                match self.doc.nodes.get(nid).unwrap().parent {
                    Some(parent_id) => {
                        let fallback = vb_doc::model::NodeTree {
                            node: self.doc.nodes.get(nid).unwrap().clone(),
                            children: vec![],
                        };
                        let mut tree = vb_doc::model::NodeTree::from_document(&self.doc, nid)
                            .unwrap_or(fallback);
                        re_sid_tree(&mut tree, &mut self.doc);
                        let new_sid = tree.node.sid.as_str().to_string();
                        let parent_sid = self
                            .doc
                            .nodes
                            .get(parent_id)
                            .unwrap()
                            .sid
                            .as_str()
                            .to_string();
                        self.exec(Command::Insert {
                            parent_sid,
                            index: usize::MAX,
                            tree,
                        });
                        self.selection = vec![new_sid.clone()];
                        new_sid
                    }
                    None => sid.clone(),
                }
            } else {
                sid.clone()
            };
            let nid = self.doc.find_by_sid(&drag_sid).unwrap();
            let g = self.doc.nodes.get(nid).unwrap().geom;
            let others = self.selection_others(&drag_sid);
            self.drag = Drag::MoveObj {
                sid: drag_sid,
                start_geom: g,
                grab_dx: wx - g.x,
                grab_dy: wy - g.y,
                moved: false,
                others,
            };
        } else {
            self.drag = Drag::Marquee { start: p, cur: p };
            if !shift {
                self.selection.clear();
            }
        }
    }

    /// 拖拽持续:MoveObj 移动 + 智能参考线 + 多选整体位移(**不消费**,继续到松手)。
    pub(super) fn drag_move_object(&mut self, p: egui::Vec2, shift: bool, ctrl: bool) {
        let drag_update: Option<(String, Geom)> = match &mut self.drag {
            Drag::Marquee { cur, .. } | Drag::Create { cur, .. } | Drag::ZoomRegion { cur, .. } => {
                *cur = p;
                None
            }
            Drag::MoveObj {
                sid,
                start_geom,
                grab_dx,
                grab_dy,
                ..
            } => {
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                let mut nx = wx - *grab_dx;
                let mut ny = wy - *grab_dy;
                let (dx, dy) =
                    vb_tools::constrain_axis(nx - start_geom.x, ny - start_geom.y, shift);
                nx = (start_geom.x + dx).round();
                ny = (start_geom.y + dy).round();
                Some((
                    sid.clone(),
                    Geom {
                        x: nx,
                        y: ny,
                        w: start_geom.w,
                        h: start_geom.h,
                    },
                ))
            }
            _ => None,
        };
        if let Some((sid, g)) = drag_update {
            // 智能参考线:对齐兄弟/画板(屏幕空间 6px 阈值);拖动中 Mod 临时禁用(P3.10)
            let mut g = g;
            if self.smart_guides_on && !ctrl {
                if let Some(nid) = self.doc.find_by_sid(&sid) {
                    let parent = self.doc.nodes.get(nid).unwrap().parent;
                    // 移动对象所属画板直接沿父链上溯(坐标探测在多画板
                    // 且对象位于负坐标时会找错画板)
                    let ab = vb_tools::artboard_of(&self.doc, nid);
                    let (sx, sy, lines) =
                        self.smart_snap(nid, parent, ab, &g, 6.0 / self.camera.zoom);
                    g.x = sx;
                    g.y = sy;
                    self.smart_guides = lines;
                    // 角度类参考线(C2 第五类):拖动方向接近 45° 倍数
                    // 时,过拖拽起点的世界向导线(仅提示,不吸附)
                    if let Drag::MoveObj {
                        grab_dx,
                        grab_dy,
                        start_geom,
                        ..
                    } = &self.drag
                    {
                        let ox = start_geom.x + grab_dx;
                        let oy = start_geom.y + grab_dy;
                        let (vx, vy) = (g.x + grab_dx - ox, g.y + grab_dy - oy);
                        let len = vx.hypot(vy);
                        if len > 12.0 / self.camera.zoom {
                            let ang = vy.atan2(vx).to_degrees().rem_euclid(180.0);
                            for t in [0.0f64, 45.0, 90.0, 135.0] {
                                if (t - ang).abs() < 3.0 {
                                    let rad = t.to_radians();
                                    let (dx, dy) = (rad.cos(), rad.sin());
                                    let l = 4000.0;
                                    self.smart_guides.push([
                                        ox - dx * l,
                                        oy - dy * l,
                                        ox + dx * l,
                                        oy + dy * l,
                                    ]);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            // 多选:其余成员随主对象整体位移(含吸附量),
            // 合成一条纯 SetGeom Compound(合并键同族 → 整段拖拽一条 undo)
            let (others, start_geom) = match &self.drag {
                Drag::MoveObj {
                    others, start_geom, ..
                } => (others.clone(), *start_geom),
                _ => (Vec::new(), g),
            };
            if others.is_empty() {
                self.exec(Command::SetGeom {
                    sid: sid.clone(),
                    new: g,
                    old: None,
                    old_declared: None,
                });
            } else {
                let dx = g.x - start_geom.x;
                let dy = g.y - start_geom.y;
                let mut cmds = vec![Command::SetGeom {
                    sid: sid.clone(),
                    new: g,
                    old: None,
                    old_declared: None,
                }];
                for (osid, og) in others {
                    cmds.push(Command::SetGeom {
                        sid: osid,
                        new: Geom {
                            x: og.x + dx,
                            y: og.y + dy,
                            w: og.w,
                            h: og.h,
                        },
                        old: None,
                        old_declared: None,
                    });
                }
                self.exec(Command::Compound { cmds });
            }
            let mut delta_recorded: Option<(f64, f64)> = None;
            if let Drag::MoveObj {
                moved, start_geom, ..
            } = &mut self.drag
            {
                *moved = true;
                self.last_move_delta = Some((g.x - start_geom.x, g.y - start_geom.y));
                delta_recorded = Some((g.x - start_geom.x, g.y - start_geom.y));
            }
            // 阶段 2(03-4-2):拖动也记进"上一次变换"
            // (借用结束后再调用,避免 `&mut self.drag` 与 `&mut self` 冲突)
            if let Some((dx, dy)) = delta_recorded {
                self.remember_transform(crate::app::transform_panel::TransformDelta::translate(
                    dx, dy,
                ));
            }
        }
    }

    /// 框选松手:相交即选中(落在拖拽起点所在画板)。
    pub(super) fn end_marquee(&mut self, start: egui::Vec2, cur: egui::Vec2, shift: bool) {
        // 相交即选中(AI);框选落在拖拽起点所在画板(此前硬编码
        // artboards.first(),多画板文档在其它画板框选错乱)
        let (x0, y0) = self
            .camera
            .screen_to_world(start.x.min(cur.x) as f64, start.y.min(cur.y) as f64);
        let (x1, y1) = self
            .camera
            .screen_to_world(start.x.max(cur.x) as f64, start.y.max(cur.y) as f64);
        let r = vb_common::geom::rect_xywh(x0, y0, x1 - x0, y1 - y0);
        let (swx, swy) = self.camera.screen_to_world(start.x as f64, start.y as f64);
        let ab = self
            .artboard_at_world(swx, swy)
            .or_else(|| self.doc.artboards.first().copied());
        if let Some(ab) = ab {
            let hits = match self.isolate_top() {
                Some(iso) => {
                    let (ox, oy) = self.doc.artboard_origin(ab);
                    let lr = vb_common::geom::Rect::new(r.x0 - ox, r.y0 - oy, r.x1 - ox, r.y1 - oy);
                    vb_tools::marquee_select_root(&self.doc, iso, lr)
                }
                None => vb_tools::marquee_select(&self.doc, ab, r),
            };
            let mut sids: Vec<String> = hits
                .into_iter()
                .map(|id| self.doc.nodes.get(id).unwrap().sid.as_str().to_string())
                .collect();
            if shift {
                for s in &self.selection {
                    if !sids.contains(s) {
                        sids.push(s.clone());
                    }
                }
            }
            self.selection = sids;
            if !self.selection.is_empty() {
                self.status = format!("框选 {} 个对象", self.selection.len());
            }
        }
    }

    /// 移动松手:记「上一次位移」(Mod+D 重放用)。
    pub(super) fn end_move_obj(
        &mut self,
        sid: String,
        moved: bool,
        start_geom: vb_doc::model::Geom,
    ) {
        if moved {
            if let Some(nid) = self.doc.find_by_sid(&sid) {
                let g = self.doc.nodes.get(nid).unwrap().geom;
                self.last_move_delta = Some((g.x - start_geom.x, g.y - start_geom.y));
            }
        }
    }
}
