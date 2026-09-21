//! 画布输入(S1-a 自 app.rs 机械搬移,零行为变化):
//! 指针/滚轮手势状态机(Drag 各变体)、智能参考线吸附、手柄/几何辅助函数。

use egui::{pos2, vec2, PointerButton, Rect};
use vb_doc::commands::Command;
use vb_doc::model::{Geom, NodeKind};
use vb_ui::cursor as vbcursor;

use super::{fmt_deg, parse_rotate_deg, re_sid_tree, set_style_prop, Drag, PenPt, Tool, VellumApp};

impl VellumApp {
    pub(crate) fn handle_canvas_input(
        &mut self,
        response: &egui::Response,
        ctx: egui::Context,
        rect: Rect,
    ) {
        // 缩放工具单击:放大 / Alt+单击缩小(02 篇 §5.6)
        if response.clicked() && self.tool == Tool::Zoom {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let alt_click = ctx.input(|i| i.modifiers.alt);
                let f = if alt_click { 1.0 / 1.25 } else { 1.25 };
                self.camera.zoom_at(pl.x as f64, pl.y as f64, f);
                self.status = format!("缩放 {}%", (self.camera.zoom * 100.0) as i64);
            }
            return;
        }
        // 吸管单击:取色/取样式应用到选区(06 篇 §5.4;Alt = 全部样式)
        if response.clicked() && self.tool == Tool::Eyedropper {
            let alt = ctx.input(|i| i.modifiers.alt);
            self.eyedropper_pick(alt);
            return;
        }
        // 渐变单击:Alt = 移除 background-image 恢复纯色(06 篇 §5.5)
        if response.clicked() && self.tool == Tool::Gradient {
            // 双击批注上的色标 → 选中该色标并打开渐变面板(05-2-3)
            if response.double_clicked() {
                if let Some(pp) = response.interact_pointer_pos() {
                    let pl = pp - rect.min;
                    if self.grad_annot_double_click((pl.x, pl.y)) {
                        return;
                    }
                }
            }
            let alt = ctx.input(|i| i.modifiers.alt);
            if alt {
                let targets = self.selection.clone();
                if targets.is_empty() {
                    self.status = "渐变:先选中对象".into();
                } else {
                    for sid in targets {
                        let Some(t) = self.doc.find_by_sid(&sid) else {
                            continue;
                        };
                        let mut style = self.doc.nodes.get(t).unwrap().style.clone();
                        let before = style.len();
                        style.retain(|d| d.prop != "background-image");
                        if style.len() != before {
                            self.exec(Command::SetStyle {
                                sid,
                                new: style,
                                old: None,
                            });
                        }
                    }
                    self.status = "已移除渐变(恢复纯色)".into();
                }
            } else if self.selection.is_empty() {
                self.status = "渐变:先选中对象,再拖动设定方向".into();
            }
            return;
        }
        // 剪刀单击:在矢量锚点处剪开(闭路开口 / 开路分段;06 篇 §5.3)
        if response.clicked() && self.tool == Tool::Scissors {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.scissors_cut(wx, wy, 8.0 / self.camera.zoom);
            }
            return;
        }
        // 单击创建(Rect/Ellipse/Line/Text/Artboard 工具下单击 = 默认尺寸;处理单帧合并的合成拖拽)
        if response.clicked()
            && matches!(
                self.tool,
                Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Text | Tool::Artboard
            )
        {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                match self.tool {
                    Tool::Text => {
                        self.create_text_node((wx.round(), wy.round()), None);
                    }
                    Tool::Artboard => {
                        self.create_artboard(Geom {
                            x: wx.round(),
                            y: wy.round(),
                            w: 1440.0,
                            h: 900.0,
                        });
                    }
                    _ => {
                        self.create_shape(Geom {
                            x: (wx - 60.0).round(),
                            y: (wy - 40.0).round(),
                            w: 120.0,
                            h: 80.0,
                        });
                        self.status = "已创建对象(单击默认尺寸)".into();
                    }
                }
                return;
            }
        }
        // 钢笔工具单击(P4.5):落锚点;靠近起点时闭合
        if response.clicked() && self.tool == Tool::Pen {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                let (wx, wy) = (wx.round(), wy.round());
                if let Some(first) = self.pen_points.first() {
                    let (x0, y0) = first.anchor;
                    if (wx - x0).hypot(wy - y0) <= 6.0 / self.camera.zoom
                        && self.pen_points.len() >= 3
                    {
                        self.finish_pen(true);
                        return;
                    }
                }
                self.pen_points.push(PenPt::corner(wx, wy));
                // (request_repaint 由 egui 输入事件自动触发)
            }
            return;
        }
        // 钢笔拖拽(06 篇 §5.3 平滑点):按下锚点后拖出出手柄,入柄镜像
        if response.drag_started() && self.tool == Tool::Pen {
            if let Some(p0) = response.interact_pointer_pos() {
                let pl = p0 - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.pen_points.push(PenPt::corner(wx.round(), wy.round()));
            }
            return;
        }
        if response.dragged() && self.tool == Tool::Pen && !self.pen_points.is_empty() {
            if let Some(p0) = response.interact_pointer_pos() {
                let pl = p0 - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                let last = self.pen_points.last_mut().unwrap();
                last.h_out = Some((wx, wy));
            }
            return;
        }
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
        // 光标世界坐标(指针先转画布本地)
        if let Some(p) = response.hover_pos() {
            let pl = p - rect.min;
            let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
            self.cursor_world = (wx, wy);
        }
        // Alt+滚轮 / 滚轮缩放与滚动
        let (scroll, alt_down) = ctx.input(|i| (i.smooth_scroll_delta, i.modifiers.alt));
        if response.hovered() && scroll.y != 0.0 {
            if alt_down {
                if let Some(p) = response.hover_pos() {
                    let pl = p - rect.min;
                    self.camera.zoom_at(
                        pl.x as f64,
                        pl.y as f64,
                        (-(scroll.y as f64) / 400.0).exp(),
                    );
                }
            } else {
                self.camera.pan_y += scroll.y as f64;
                self.camera.pan_x += scroll.x as f64;
            }
        }

        let mods = ctx.input(|i| i.modifiers);
        let alt = mods.alt;
        let shift = mods.shift;
        let ctrl = mods.ctrl || mods.command;
        let _ = alt_down;

        // 本帧参考线清空(绘制在 overlay)
        self.smart_guides.clear();

        // 平移:中键 或 Space+左键 或 抓手工具
        let pan_wanted = self.space_down || self.tool == Tool::Hand;
        if pan_wanted {
            ctx.set_cursor_icon(vbcursor::PAN);
        } else {
            // 工具光标映射(P2.8 尾巴):绘图类十字线、缩放放大镜
            match self.tool {
                Tool::Rect | Tool::Ellipse | Tool::Line => {
                    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                Tool::Zoom => ctx.set_cursor_icon(egui::CursorIcon::ZoomIn),
                // 直接选择:与「选择」必须**看得出不同**(07-3-5)。egui 没有
                // Illustrator 的空心箭头/锚点光标,`Cell`(方框十字)是最接近
                // 「锚点」语义的内置图标 —— 与 `cursor.rs` 的旋转光标同属
                // **已知妥协**,在 `cursor` 模块的测试里钉住。
                Tool::DirectSelect => ctx.set_cursor_icon(vbcursor::DIRECT_SELECT),
                _ => {}
            }
        }

        // 手柄悬停光标(P2.8):非平移态下,指针落在选中对象手柄上给方向光标,
        // 旋转圈在角外侧(见下方 Drag::Rotate 命中区)。
        if !pan_wanted && self.tool == Tool::Select {
            if let Some(p) = response.hover_pos() {
                'outer: for sid in &self.selection {
                    let Some(nid) = self.doc.find_by_sid(sid) else {
                        continue;
                    };
                    let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
                        continue;
                    };
                    let (sx, sy) = self.camera.world_to_screen(bb.x0, bb.y0);
                    let (ex, ey) = self.camera.world_to_screen(bb.x1, bb.y1);
                    let r = Rect::from_min_max(
                        pos2(sx as f32 + rect.min.x, sy as f32 + rect.min.y),
                        pos2(ex as f32 + rect.min.x, ey as f32 + rect.min.y),
                    );
                    if let Some(h) = hit_handle(p, r) {
                        ctx.set_cursor_icon(vbcursor::for_handle(h));
                        break 'outer;
                    }
                }
            }
        }

        if response.dragged_by(PointerButton::Middle)
            || (pan_wanted && response.dragged_by(PointerButton::Primary))
        {
            ctx.set_cursor_icon(vbcursor::PANNING);
            if let Drag::Pan { start_pan } = self.drag {
                self.camera.pan_x = start_pan.x as f64 + (response.drag_delta().x) as f64;
                self.camera.pan_y = start_pan.y as f64 + (response.drag_delta().y) as f64;
            } else if matches!(self.drag, Drag::None) {
                let d = response.drag_delta();
                self.camera.pan_x += d.x as f64;
                self.camera.pan_y += d.y as f64;
                self.drag = Drag::None;
            }
            return;
        }

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
                        return;
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
                            return;
                        }
                        self.editing_text = Some(n.sid.as_str().to_string());
                        self.status =
                            format!("编辑文本:{}(Ctrl+Enter/Esc 提交,再按 Esc 放弃)", n.name);
                    }
                }
            }
        }

        // 选中对象的屏幕 bbox(用于手柄/旋转命中)
        let sel_bbox_screen = self.selection.last().and_then(|sid| {
            let nid = self.doc.find_by_sid(sid)?;
            let bb = vb_tools::abs_bbox_world(&self.doc, nid)?;
            let (x0, y0) = self.camera.world_to_screen(bb.x0, bb.y0);
            let (x1, y1) = self.camera.world_to_screen(bb.x1, bb.y1);
            Some((
                Rect::from_min_max(
                    pos2(x0 as f32 + rect.min.x, y0 as f32 + rect.min.y),
                    pos2(x1 as f32 + rect.min.x, y1 as f32 + rect.min.y),
                ),
                sid.clone(),
            ))
        });

        if response.drag_started() {
            let Some(p0) = response.interact_pointer_pos() else {
                return;
            };
            let p = p0 - rect.min; // 画布本地

            // --- 0) 标尺参考线抓取(未锁定时优先级最高):从标尺拖出新建,
            //        在参考线 ±3px 内按下则拖动既有参考线(02 篇 §5.2) ---
            if self.tool == Tool::Select && !self.guides_locked {
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                const STRIP: f32 = 20.0;
                let in_top = p.y <= STRIP;
                let in_left = p.x <= STRIP;
                let near_line = self.guides.iter().position(|&(h, pos)| {
                    if h {
                        let (_, sy) = self.camera.world_to_screen(0.0, pos);
                        (p.y - sy as f32).abs() <= 3.0
                    } else {
                        let (sx, _) = self.camera.world_to_screen(pos, 0.0);
                        (p.x - sx as f32).abs() <= 3.0
                    }
                });
                if in_top || in_left || near_line.is_some() {
                    // 命中已有参考线(±3px)一律抓取,与指针在不在标尺条内
                    // 无关;否则水平参考线拖到画布中部后,方向 guard 不匹配
                    // 会凭空新建一条垂直线(G7)
                    let idx = match near_line {
                        Some(i) => i,
                        None if in_top => {
                            self.guides.push((true, wy));
                            self.guides.len() - 1
                        }
                        None => {
                            self.guides.push((false, wx));
                            self.guides.len() - 1
                        }
                    };
                    self.drag = Drag::Guide { idx };
                    return;
                }
            }

            // --- 1) 手柄/旋转命中(单选优先) ---
            if let (Some((bbox, sid)), true) = (sel_bbox_screen.clone(), self.tool == Tool::Select)
            {
                // 角外圈 → 旋转
                const RING: f32 = 14.0;
                let corners = [
                    bbox.left_top(),
                    bbox.right_top(),
                    bbox.right_bottom(),
                    bbox.left_bottom(),
                ];
                if let Some(nid) = self.doc.find_by_sid(&sid) {
                    let n = self.doc.nodes.get(nid).unwrap();
                    let cur_deg = n
                        .style_get("transform")
                        .and_then(parse_rotate_deg)
                        .unwrap_or(0.0);
                    for c in corners {
                        let d = (p - (c - rect.min)).length();
                        if d > 6.0 && d < RING + 6.0 {
                            let cx = bbox.center().x as f64;
                            let cy = bbox.center().y as f64;
                            let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                            let a0 = (wy - cy).atan2(wx - cx);
                            self.drag = Drag::Rotate {
                                sid,
                                center: (cx, cy),
                                start_angle: a0,
                                start_deg: cur_deg,
                                moved: false,
                            };
                            return;
                        }
                    }
                }
                // 8 手柄
                if let Some(h) = hit_handle(rect.min + p, bbox) {
                    let nid = self.doc.find_by_sid(&sid).unwrap();
                    let g = self.doc.nodes.get(nid).unwrap().geom;
                    self.drag = Drag::Resize {
                        sid,
                        start_geom: g,
                        handle: h,
                        start: p,
                        moved: false,
                    };
                    return;
                }
            }

            // --- 2) 常规:点选 / 框选 / 创建 ---
            let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
            match self.tool {
                Tool::Hand => {
                    self.drag = Drag::Pan {
                        start_pan: vec2(self.camera.pan_x as f32, self.camera.pan_y as f32),
                    };
                }
                Tool::GroupSelect => {
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
                Tool::Select => {
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
                                    let mut tree =
                                        vb_doc::model::NodeTree::from_document(&self.doc, nid)
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
                Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Text | Tool::Artboard => {
                    self.drag = Drag::Create { start: p, cur: p };
                }
                Tool::Zoom => {
                    self.drag = Drag::ZoomRegion { start: p, cur: p };
                }
                Tool::Pen => {
                    // 钢笔单击由 clicked() 处理;这里兜底防穿透
                }
                Tool::DirectSelect => {
                    // 直接选择:单击由 clicked() 处理(顶点命中)
                }
                Tool::Eyedropper => {
                    // 吸管:单击由 clicked() 处理(取色/取样式)
                }
                Tool::Gradient => {
                    // 渐变批注者:需要选区;拖动方向 = 渐变方向
                    if self.selection.is_empty() {
                        self.status = "渐变:先选中对象".into();
                    } else {
                        self.drag = Drag::GradientAnnotate {
                            start: (wx, wy),
                            end: (wx, wy),
                            angle: 0.0,
                        };
                    }
                }
                Tool::Scissors => {
                    // 剪刀:单击由 clicked() 处理(锚点剪开)
                }
            }
        }

        if response.dragged() {
            let Some(p0) = response.interact_pointer_pos() else {
                return;
            };
            let p = p0 - rect.min;

            // 参考线拖动:实时跟随光标(世界坐标)
            if let Drag::Guide { idx } = &self.drag {
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                if let Some(g) = self.guides.get_mut(*idx) {
                    g.1 = if g.0 { wy } else { wx };
                }
                return;
            }

            // 渐变拖动:方向 = 起点→光标;实时应用(合并窗口内一条 undo)
            if let Drag::GradientAnnotate { start, .. } = &self.drag {
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                let dx = wx - start.0;
                let dy = wy - start.1;
                if dx.hypot(dy) >= 2.0 {
                    // CSS 角度:0° = 向上,90° = 向右(Y 轴向下,故取 -dy)
                    let angle = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                    if let Drag::GradientAnnotate {
                        angle: slot, end, ..
                    } = &mut self.drag
                    {
                        *slot = angle;
                        *end = (wx, wy);
                    }
                    self.apply_gradient_to_selection(angle);
                }
                return;
            }

            // 缩放 / 旋转(它们自成状态,不与 MoveObj 共路)
            match &self.drag {
                Drag::Resize { .. } | Drag::Rotate { .. } => {}
                _ => {}
            }
            let resize_update: Option<(String, Geom)> = match &mut self.drag {
                Drag::Resize {
                    sid,
                    start_geom,
                    handle,
                    start,
                    ..
                } => {
                    let dx = (p.x - start.x) as f64 / self.camera.zoom;
                    let dy = (p.y - start.y) as f64 / self.camera.zoom;
                    Some((
                        sid.clone(),
                        resize_geom(*start_geom, *handle, dx, dy, shift, alt),
                    ))
                }
                _ => None,
            };
            if let Some((sid, g)) = resize_update {
                self.exec(Command::SetGeom {
                    sid,
                    new: g,
                    old: None,
                    old_declared: None,
                });
                if let Drag::Resize { moved, .. } = &mut self.drag {
                    *moved = true;
                }
                return;
            }
            if let Drag::Rotate {
                sid,
                center,
                start_angle,
                start_deg,
                ..
            } = &mut self.drag
            {
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                let a = (wy - center.1).atan2(wx - center.0);
                let mut deg = *start_deg + (a - *start_angle).to_degrees();
                if shift {
                    deg = (deg / 15.0).round() * 15.0;
                }
                deg = (deg * 10.0).round() / 10.0;
                let sid = sid.clone();
                if let Some(nid) = self.doc.find_by_sid(&sid) {
                    let mut style = self.doc.nodes.get(nid).unwrap().style.clone();
                    style =
                        set_style_prop(style, "transform", &format!("rotate({}deg)", fmt_deg(deg)));
                    self.exec(Command::SetStyle {
                        sid,
                        new: style,
                        old: None,
                    });
                    if let Drag::Rotate { moved, .. } = &mut self.drag {
                        *moved = true;
                    }
                }
                return;
            }

            // 移动 + 智能参考线
            let drag_update: Option<(String, Geom)> = match &mut self.drag {
                Drag::Marquee { cur, .. }
                | Drag::Create { cur, .. }
                | Drag::ZoomRegion { cur, .. } => {
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
                    self.remember_transform(super::transform_panel::TransformDelta::translate(
                        dx, dy,
                    ));
                }
            }
        }

        if response.drag_stopped() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::GradientAnnotate { start, end, angle } => {
                    // 批注保留(05-2-3):双击其上的色标可改色
                    self.gradient_annot = Some((start.0, start.1, end.0, end.1));
                    self.status = format!(
                        "线性渐变已应用 {angle:.0}°(起=原填充 → 止=#ffffff;双击色标改色;Alt+单击移除)"
                    );
                }
                Drag::Guide { idx } => {
                    // 松手在标尺条内/画布外 = 删除(AI 拖回标尺删参考线)
                    let inside = response.interact_pointer_pos().map(|pp| {
                        let pl = pp - rect.min;
                        pl.x > 20.0 && pl.y > 20.0 && rect.contains(pp)
                    });
                    if inside != Some(true) && idx < self.guides.len() {
                        self.guides.remove(idx);
                        self.status = "参考线已删除".into();
                    }
                }
                Drag::Marquee { start, cur } => {
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
                                let lr = vb_common::geom::Rect::new(
                                    r.x0 - ox,
                                    r.y0 - oy,
                                    r.x1 - ox,
                                    r.y1 - oy,
                                );
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
                Drag::Create { start, cur } => {
                    let (sx, sy) = self.camera.screen_to_world(start.x as f64, start.y as f64);
                    let (cx, cy) = self.camera.screen_to_world(cur.x as f64, cur.y as f64);
                    match self.tool {
                        Tool::Text => {
                            let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                            // 拖框 = 区域文本;几乎没拖 = 点文本(与 clicked 互斥,兜底)
                            if g.w < 24.0 && g.h < 24.0 {
                                self.create_text_node((sx.round(), sy.round()), None);
                            } else {
                                self.create_text_node(
                                    (sx.round(), sy.round()),
                                    Some(Geom {
                                        x: g.x.round(),
                                        y: g.y.round(),
                                        w: g.w.round().max(120.0),
                                        h: g.h.round().max(40.0),
                                    }),
                                );
                            }
                        }
                        Tool::Artboard => {
                            let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                            self.create_artboard(Geom {
                                x: g.x.round(),
                                y: g.y.round(),
                                w: g.w.round().max(80.0),
                                h: g.h.round().max(80.0),
                            });
                        }
                        _ => {
                            let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                            self.create_shape(g);
                        }
                    }
                }
                Drag::ZoomRegion { start, cur } => {
                    let (x0, y0) = self
                        .camera
                        .screen_to_world(start.x.min(cur.x) as f64, start.y.min(cur.y) as f64);
                    let (x1, y1) = self
                        .camera
                        .screen_to_world(start.x.max(cur.x) as f64, start.y.max(cur.y) as f64);
                    let rw = (x1 - x0).max(1.0);
                    let rh = (y1 - y0).max(1.0);
                    if let Some(r) = self.canvas_rect {
                        if rw > 1.0 && rh > 1.0 {
                            let zoom = ((r.width() as f64) / rw)
                                .min((r.height() as f64) / rh)
                                .clamp(0.01, 64.0);
                            self.camera.zoom = zoom;
                            // pan 使区域中心落在画布中心(screen 为画布本地坐标)
                            let ccx = (r.center().x - rect.min.x) as f64;
                            let ccy = (r.center().y - rect.min.y) as f64;
                            self.camera.pan_x = ccx - (x0 + rw / 2.0) * zoom;
                            self.camera.pan_y = ccy - (y0 + rh / 2.0) * zoom;
                            self.status = format!("缩放到区域 {}%", (zoom * 100.0) as i64);
                        }
                    }
                }
                Drag::MoveObj {
                    sid,
                    moved,
                    start_geom,
                    ..
                } => {
                    if moved {
                        if let Some(nid) = self.doc.find_by_sid(&sid) {
                            let g = self.doc.nodes.get(nid).unwrap().geom;
                            self.last_move_delta = Some((g.x - start_geom.x, g.y - start_geom.y));
                        }
                    }
                }
                // 阶段 2(03-4-2):把缩放/旋转记进"上一次变换",供 `Mod+D` 重放
                Drag::Resize {
                    sid,
                    moved,
                    start_geom,
                    ..
                } => {
                    if !moved {
                        return;
                    }
                    let Some(nid) = self.doc.find_by_sid(&sid) else {
                        return;
                    };
                    let g = self.doc.nodes.get(nid).unwrap().geom;
                    let kx = if start_geom.w.abs() > 1e-9 {
                        g.w / start_geom.w
                    } else {
                        1.0
                    };
                    let ky = if start_geom.h.abs() > 1e-9 {
                        g.h / start_geom.h
                    } else {
                        1.0
                    };
                    // 缩放可能同时改变位置(拖边/角):`replay` 先按比例缩放再位移
                    self.remember_transform(super::transform_panel::TransformDelta {
                        dx: g.x - start_geom.x,
                        dy: g.y - start_geom.y,
                        kx,
                        ky,
                        d_angle: 0.0,
                    });
                }
                Drag::Rotate {
                    sid,
                    moved,
                    start_deg,
                    ..
                } => {
                    if !moved {
                        return;
                    }
                    let Some(nid) = self.doc.find_by_sid(&sid) else {
                        return;
                    };
                    let cur = self
                        .doc
                        .nodes
                        .get(nid)
                        .and_then(|n| n.style_get("transform"))
                        .map(|t| super::transform_panel::parse_transform(t).0)
                        .unwrap_or(0.0);
                    self.remember_transform(super::transform_panel::TransformDelta {
                        d_angle: cur - start_deg,
                        ..super::transform_panel::TransformDelta::translate(0.0, 0.0)
                    });
                }
                _ => {}
            }
        }
    }

    /// 移动节点父级到所属画板之间的累计 geom 偏移(父级即画板时为 0)。
    /// 用于把父相对坐标换算成画板本地坐标。
    fn parent_offset_in_artboard(
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
    fn smart_snap(
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

/// 8 手柄命中:返回 0=NW 1=N 2=NE 3=E 4=SE 5=S 6=SW 7=W;未命中 None。
fn hit_handle(p: egui::Pos2, bbox: Rect) -> Option<u8> {
    const R: f32 = 6.0;
    let pts = [
        bbox.left_top(),
        (bbox.center().x, bbox.top()).into(),
        bbox.right_top(),
        (bbox.right(), bbox.center().y).into(),
        bbox.right_bottom(),
        (bbox.center().x, bbox.bottom()).into(),
        bbox.left_bottom(),
        (bbox.left(), bbox.center().y).into(),
    ];
    pts.iter()
        .position(|c| {
            let c: egui::Pos2 = *c;
            (p - c).length() <= R + 2.0
        })
        .map(|i| i as u8)
}

/// 缩放几何:handle 决定动哪条边;Shift 等比(角手柄);Alt 从中心。
fn resize_geom(g0: Geom, handle: u8, dx: f64, dy: f64, shift: bool, alt: bool) -> Geom {
    let (mut x0, mut y0) = (g0.x, g0.y);
    let (mut x1, mut y1) = (g0.x + g0.w, g0.y + g0.h);
    let west = matches!(handle, 6 | 7 | 0);
    let east = matches!(handle, 2..=4);
    let north = matches!(handle, 0..=2);
    let south = matches!(handle, 4..=6);
    let corner = matches!(handle, 0 | 2 | 4 | 6);

    if west {
        x0 += dx;
        if alt {
            x1 -= dx;
        }
    }
    if east {
        x1 += dx;
        if alt {
            x0 -= dx;
        }
    }
    if north {
        y0 += dy;
        if alt {
            y1 -= dy;
        }
    }
    if south {
        y1 += dy;
        if alt {
            y0 -= dy;
        }
    }
    if x1 < x0 {
        std::mem::swap(&mut x0, &mut x1);
    }
    if y1 < y0 {
        std::mem::swap(&mut y0, &mut y1);
    }
    let (mut w, mut h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));

    // Shift 等比(仅角手柄):以较大的轴比率回推另一轴
    if shift && corner && g0.w > 0.0 && g0.h > 0.0 {
        let ratio = g0.w / g0.h;
        let cx = (x0 + x1) / 2.0;
        let cy = (y0 + y1) / 2.0;
        if w / h > ratio {
            h = w / ratio;
        } else {
            w = h * ratio;
        }
        // 保持锚点:Alt=中心,否则固定对侧
        if alt {
            x0 = cx - w / 2.0;
            y0 = cy - h / 2.0;
        } else {
            // 固定对侧角
            let (ax, ay) = match handle {
                0 => (g0.x + g0.w, g0.y + g0.h),
                2 => (g0.x, g0.y + g0.h),
                4 => (g0.x, g0.y),
                _ => (g0.x + g0.w, g0.y),
            };
            x0 = if matches!(handle, 6 | 7 | 0) {
                ax - w
            } else {
                ax
            };
            y0 = if matches!(handle, 0..=2) { ay - h } else { ay };
        }
        x1 = x0 + w;
        y1 = y0 + h;
    }

    // 取整到像素
    Geom {
        x: x0.round(),
        y: y0.round(),
        w: (x1 - x0).round().max(1.0),
        h: (y1 - y0).round().max(1.0),
    }
}
