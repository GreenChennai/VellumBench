//! 图层 Tab · 拖放与右键菜单:落点高亮、落下命令执行、行上下文菜单。
//!
//! 06-1 自 `panels/layers.rs` 按职责拆出(纯搬移,零行为变化);
//! 落点语义纯函数(`drop_slot`/`move_cmds`)在 `model`。

use egui::Pos2;
use vb_doc::commands::Command;
use vb_ui::theme;

use super::model::*;
use crate::app::VellumApp;

impl VellumApp {
    /// 拖拽进行中:落点高亮 + 松手落下(02-4-1/2)。
    pub(super) fn layer_drag_overlay(&mut self, ui: &mut egui::Ui, zones: &[DropZone]) {
        if self.layer_drag.is_none() {
            return;
        }
        let pos = ui.input(|i| i.pointer.interact_pos());
        let target = self
            .layer_drag
            .as_ref()
            .and_then(|d| drop_slot(&self.doc, zones, pos, &d.sid));
        let t = theme::tokens(ui.ctx());
        match &target {
            Some(DropTarget::Into { rect, .. }) => {
                ui.painter()
                    .rect_filled(*rect, theme::radius::sm(), t.accent.gamma_multiply(0.22));
                ui.painter().rect_stroke(
                    *rect,
                    theme::radius::sm(),
                    egui::Stroke::new(1.5, t.accent),
                    egui::StrokeKind::Inside,
                );
            }
            Some(DropTarget::Edge { line_y, x0, x1, .. }) => {
                ui.painter().line_segment(
                    [Pos2::new(*x0, *line_y), Pos2::new(*x1, *line_y)],
                    egui::Stroke::new(2.0, t.accent),
                );
            }
            None => {}
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(drag) = self.layer_drag.take() {
                self.finish_layer_drop(&drag, target);
            }
        }
    }

    /// 右键菜单(02-4-4 / 07-L 补全):全部接真实命令路径;无支撑的项不出现。
    /// 规范项(design/03 §5.1 + 迭代 07-L):改名 / 复制 / 删除 / 编组 /
    /// 显隐 / 锁定 / 前后移,叠加既有隔离·锁定其他·隐藏其他·选择同类·
    /// 转换为编组。单节点导出与剪切蒙版仍无命令支撑,不放假按钮。
    pub(super) fn layer_context_menu(
        &mut self,
        _ui: &mut egui::Ui,
        resp: &egui::Response,
        r: &RowDesc,
    ) {
        resp.context_menu(|ui| {
            // ── 改名(与双击行内编辑同一入口)──
            if ui.button("改名").clicked() {
                self.editing_layer = Some(r.sid.clone());
                ui.close();
            }
            // ── 复制(克隆子树插到自身之后;走 Insert 命令,可撤销)──
            if !r.is_artboard && ui.button("复制").clicked() {
                let parent_sid = r.parent_sid.clone().unwrap_or_default();
                let mut doc = std::mem::take(&mut self.doc);
                let dup = dup_insert_cmd(&mut doc, &r.sid, &parent_sid, r.index + 1);
                self.doc = doc;
                if let Some((insert, new_sid)) = dup {
                    let mut cmds = vec![insert];
                    cmds.extend(rebase_for_new_parent(
                        &self.doc,
                        &r.sid,
                        &new_sid,
                        &parent_sid,
                    ));
                    self.exec(Command::Compound { cmds });
                    self.selection = vec![new_sid];
                    self.say("已复制(副本插到原对象之后;Ctrl+Z 可撤销)");
                }
                ui.close();
            }
            // ── 删除(画板行不删:保底至少一块画板的守卫在选区删除路径)──
            if !r.is_artboard && ui.button("删除").clicked() {
                self.selection = vec![r.sid.clone()];
                self.exec(Command::Delete {
                    target_sid: r.sid.clone(),
                    captured: None,
                });
                self.say("已删除(Ctrl+Z 撤销)");
                ui.close();
            }
            // ── 显隐 / 锁定(与行内眼睛/锁同一命令)──
            if ui.button(if r.hidden { "显示" } else { "隐藏" }).clicked() {
                self.exec(Command::SetFlags {
                    sid: r.sid.clone(),
                    hidden: Some(!r.hidden),
                    locked: None,
                    old: None,
                });
                ui.close();
            }
            if ui.button(if r.locked { "解锁" } else { "锁定" }).clicked() {
                self.exec(Command::SetFlags {
                    sid: r.sid.clone(),
                    hidden: None,
                    locked: Some(!r.locked),
                    old: None,
                });
                ui.close();
            }
            // ── 前后移(与行内 ↑ ↓ 同一命令路径)──
            if ui.button("前移一层").clicked() {
                self.reorder(&r.sid, 1);
                ui.close();
            }
            if ui.button("后移一层").clicked() {
                self.reorder(&r.sid, -1);
                ui.close();
            }
            ui.separator();
            // ── 编组(多选同父;走既有 group_selection 命令路径)──
            if self.selection.len() >= 2 && ui.button("编组(Ctrl+G)").clicked() {
                self.group_selection();
                ui.close();
            }
            // 隔离(仅容器;复用既有隔离模式栈)
            if r.container && ui.button("隔离(进入隔离模式)").clicked() {
                if let Some(nid) = self.doc.find_by_sid(&r.sid) {
                    self.isolate_stack.push(nid);
                    self.selection = vec![r.sid.clone()];
                    self.say(format!("已进入隔离模式:{}(Esc 退出)", r.name));
                }
                ui.close();
            }
            if ui.button("锁定其他").clicked() {
                let others = others_of(&self.doc, &r.sid);
                if others.is_empty() {
                    self.say("锁定其他:没有其他对象");
                } else {
                    let n = others.len();
                    let cmds = others
                        .into_iter()
                        .map(|sid| Command::SetFlags {
                            sid,
                            hidden: None,
                            locked: Some(true),
                            old: None,
                        })
                        .collect();
                    self.exec(Command::Compound { cmds });
                    self.say(format!("已锁定其他 {n} 个对象"));
                }
                ui.close();
            }
            if ui.button("隐藏其他").clicked() {
                let others = others_of(&self.doc, &r.sid);
                if others.is_empty() {
                    self.say("隐藏其他:没有其他对象");
                } else {
                    let n = others.len();
                    let cmds = others
                        .into_iter()
                        .map(|sid| Command::SetFlags {
                            sid,
                            hidden: Some(true),
                            locked: None,
                            old: None,
                        })
                        .collect();
                    self.exec(Command::Compound { cmds });
                    self.selection = vec![r.sid.clone()];
                    self.say(format!("已隐藏其他 {n} 个对象"));
                }
                ui.close();
            }
            if ui.button("选择同类").clicked() {
                let sids = same_kind_sids(&self.doc, &r.sid);
                let n = sids.len();
                self.selection = sids;
                self.say(format!("已选择同类 {n} 个对象"));
                ui.close();
            }
            if !r.is_artboard && ui.button("转换为编组").clicked() {
                let mut doc = std::mem::take(&mut self.doc);
                let cmd = wrap_in_group_cmd(&mut doc, &r.sid);
                let gsid = cmd.as_ref().and_then(|c| match c {
                    Command::Group { group_sid, .. } => Some(group_sid.clone()),
                    _ => None,
                });
                self.doc = doc;
                match cmd {
                    Some(c) => {
                        self.exec(c);
                        if let Some(g) = gsid {
                            self.selection = vec![g];
                        }
                        self.say("已转换为编组");
                    }
                    None => self.say("转换为编组:该对象不支持"),
                }
                ui.close();
            }
        });
    }

    /// 拖拽落下(02-4-1/2):按落点语义构造命令并执行。
    pub(super) fn finish_layer_drop(&mut self, drag: &LayerDrag, target: Option<DropTarget>) {
        let Some(target) = target else {
            return;
        };
        let (parent_sid, index) = match &target {
            DropTarget::Into { parent_sid, .. } => {
                let len = self
                    .doc
                    .find_by_sid(parent_sid)
                    .map(|pid| self.doc.nodes.get(pid).unwrap().children.len())
                    .unwrap_or(0);
                (parent_sid.clone(), len)
            }
            DropTarget::Edge {
                parent_sid, index, ..
            } => (parent_sid.clone(), *index),
        };
        if drag.dup {
            // Alt = 复制:克隆子树插入目标槽位(+ 跨父重定基)
            let mut doc = std::mem::take(&mut self.doc);
            let dup = dup_insert_cmd(&mut doc, &drag.sid, &parent_sid, index);
            self.doc = doc;
            if let Some((insert, new_sid)) = dup {
                let mut cmds = vec![insert];
                cmds.extend(rebase_for_new_parent(
                    &self.doc,
                    &drag.sid,
                    &new_sid,
                    &parent_sid,
                ));
                self.exec(Command::Compound { cmds });
                self.selection = vec![new_sid];
                self.say("已复制到目标位置(Alt+拖拽)");
            }
        } else {
            let cmds = move_cmds(&self.doc, &drag.sid, &parent_sid, index);
            let moved = match cmds.len() {
                0 => false,
                1 => {
                    self.exec(cmds.into_iter().next().unwrap());
                    true
                }
                _ => {
                    self.exec(Command::Compound { cmds });
                    true
                }
            };
            if moved {
                self.say(if matches!(target, DropTarget::Into { .. }) {
                    "已移入目标位置(世界位置保持)"
                } else {
                    "已调整层序"
                });
            }
            self.selection = vec![drag.sid.clone()];
        }
    }
}
