//! 撤销历史面板(阶段 7 / 07-D;副文档 07 §2.1「做(轻)」)。
//!
//! 数据面 = [`vb_doc::undo::UndoStack`] 的只读访问器(命令 label 本就是
//! 中文注册表文案,直接展示);点击历史项 = 连续 undo/redo 到对应深度:
//! - 跳到**更早**状态且存在重做尾 → 按定夺「跳转即丢弃并给确认」
//!   (确认后跳转 + `clear_redo`;取消则不动);
//! - 跳到重做区(前进)= 逐条 redo,无丢弃语义;
//! - 当前位置(撤销栈顶)高亮;撤销/重做按钮与栈同步(状态本就派生自栈)。
//!
//! 面板停靠在次级坞「变换」组(变换 | 对齐 | 历史),开关真值
//! `history_open`,命令 `view.toggle_history_panel`。

use vb_doc::commands::Command;

use super::VellumApp;

/// H-8 置灰动作按钮的点击结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionClick {
    /// 无点击。
    None,
    /// 可用态点击(执行动作)。
    Fire,
    /// 禁用态点击(给 toast 说明原因)。
    Disabled,
}

/// H-8:置灰但可点的动作按钮 —— 可用时正常执行;不可用时点击
/// 返回 [`ActionClick::Disabled`],由调用方发 toast 说明原因
/// (绝不「点了没反应」,与工具箱「未支持工具」同一纪律)。
fn disabled_action_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    reason: &str,
) -> ActionClick {
    let t = vb_ui::theme::tokens(ui.ctx());
    let text = egui::RichText::new(label).color(if enabled { t.text } else { t.text_3 });
    let resp = ui.add(egui::Button::new(text));
    let resp = resp.on_hover_text(reason);
    if resp.clicked() {
        if enabled {
            ActionClick::Fire
        } else {
            ActionClick::Disabled
        }
    } else {
        ActionClick::None
    }
}

/// 历史列表上限(最近 N 步;撤销深度无上限,列表只展示最近 50)。
pub const HISTORY_MAX: usize = 50;

/// 一行历史条目(渲染前折算好的展示模型;纯函数便于测试)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRow {
    /// 命令中文名(`Command::label()`;MultiResult 的名来自其 op 字段)。
    pub label: String,
    /// `true` = 已发生区(撤销栈);`false` = 重做区(尚未发生)。
    pub done: bool,
    /// 点击后应到达的撤销深度(undo_len 目标值)。
    pub target_depth: usize,
    /// 当前位置行(撤销栈顶)。
    pub current: bool,
}

/// 折算历史列表(纯函数):撤销栈(旧 → 新,只取最近 [`HISTORY_MAX`] 步)
/// + 当前位置 + 重做区(按**应用顺序**,即下一个重做在最上)。
///
/// 重做行的 `target_depth` = 前进后应到达的撤销深度(点击转成 redo 次数)。
pub fn history_rows(undo: &[Command], redo: &[Command]) -> Vec<HistoryRow> {
    let undo_n = undo.len();
    let skip = undo_n.saturating_sub(HISTORY_MAX);
    let mut rows: Vec<HistoryRow> = undo[skip..]
        .iter()
        .enumerate()
        .map(|(i, c)| HistoryRow {
            label: c.label().to_string(),
            done: true,
            target_depth: skip + i + 1,
            current: skip + i + 1 == undo_n,
        })
        .collect();
    // 重做区:栈顶(下一个重做)先展示;第 j 行 = 连续 redo j+1 步
    for (j, c) in redo.iter().rev().enumerate() {
        rows.push(HistoryRow {
            label: c.label().to_string(),
            done: false,
            target_depth: undo_n + j + 1,
            current: false,
        });
    }
    rows
}

impl VellumApp {
    /// 历史面板正文(次级坞「历史」Tab;`panel_dock::sec_panel_body` 转发)。
    pub(crate) fn history_panel_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let can_undo = self.undo.can_undo();
            let can_redo = self.undo.can_redo();
            // H-8:禁用不是「点了没反应」—— 按钮保持可点、置灰呈呈现,
            // 点击给 toast 说明原因(与工具箱「未支持工具」同一纪律)。
            match disabled_action_button(ui, "⟲ 撤销", can_undo, "没有可撤销的操作") {
                ActionClick::Fire => self.run_command("edit.undo", false, false),
                ActionClick::Disabled => self.toast_warn("没有可撤销的操作(先做一次编辑)"),
                ActionClick::None => {}
            }
            match disabled_action_button(ui, "⟳ 重做", can_redo, "没有可重做的操作") {
                ActionClick::Fire => self.run_command("edit.redo", false, false),
                ActionClick::Disabled => self.toast_warn("没有可重做的操作(先撤销一步)"),
                ActionClick::None => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} 步 / 待重做 {}",
                        self.undo.undo_len(),
                        self.undo.redo_len()
                    ))
                    .size(12.0),
                );
            });
        });
        ui.separator();

        let rows = history_rows(self.undo.undo_slice(), self.undo.redo_slice());
        if rows.is_empty() {
            // U-5:历史空态 = 统一「图标 + 一句短话 + 动作按钮」
            // (与属性/图层/资产/时间轴同一模式)
            let t = vb_ui::theme::tokens(ui.ctx());
            ui.add_space(vb_ui::theme::space::S3);
            ui.horizontal(|ui| {
                ui.add_space(vb_ui::theme::space::S2);
                ui.label(vb_ui::icons::rich(vb_ui::icons::Name::History, 18.0).color(t.text_3));
                ui.label("还没有可撤销的操作 —— 画一笔就有了。");
            });
            ui.add_space(vb_ui::theme::space::S2);
            ui.horizontal_wrapped(|ui| {
                for (id, icon, tip) in [
                    (
                        "tool.rect",
                        vb_ui::icons::Name::ToolRect,
                        "矩形工具(M):拖框新建",
                    ),
                    (
                        "tool.text",
                        vb_ui::icons::Name::ToolText,
                        "文字工具(T):单击点文本",
                    ),
                ] {
                    if vb_ui::components::icon_button(ui, icon, tip).clicked() {
                        self.run_command(id, false, false);
                    }
                }
            });
            return;
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            // 点击先收集,滚动区渲染完再执行(借用 + 状态一致)
            let mut jump_undo: Option<usize> = None; // 回退目标深度
            let mut redo_n: usize = 0; // 前进重做步数
            let undo_len = self.undo.undo_len();
            for row in &rows {
                let text = if row.current {
                    format!("▶ {}", row.label)
                } else if row.done {
                    format!("  {}", row.label)
                } else {
                    format!("○ {}", row.label)
                };
                let resp = ui.selectable_label(row.current, text);
                if resp.clicked() {
                    if row.done && row.target_depth < undo_len {
                        if self.undo.redo_len() > 0 {
                            // 回退且存在重做尾:丢弃须确认(定夺文案)
                            self.jump_confirm = Some((row.target_depth, self.undo.redo_len()));
                        } else {
                            jump_undo = Some(row.target_depth);
                        }
                    } else if !row.done && row.target_depth > undo_len {
                        redo_n = row.target_depth - undo_len;
                    }
                }
            }
            if let Some(depth) = jump_undo {
                self.history_jump_to(depth);
            }
            if redo_n > 0 {
                self.history_redo_n(redo_n);
            }
        });
    }

    /// 跳转确认窗(回退且需丢弃重做尾时出现;`jump_confirm` 有值即渲染)。
    pub(crate) fn show_history_jump_confirm(&mut self, ui: &mut egui::Ui) {
        let Some((depth, redo_n)) = self.jump_confirm else {
            return;
        };
        let mut action = 0u8; // 1=确认 2=取消
        egui::Window::new("跳转历史?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "跳转到该状态将丢弃其后的 {redo_n} 步重做记录(不可恢复)。"
                ));
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("跳转并丢弃").clicked() {
                        action = 1;
                    }
                    if ui.button("取消").clicked() {
                        action = 2;
                    }
                });
            });
        match action {
            1 => {
                self.jump_confirm = None;
                // 顺序关键:**先跳转、后清空** —— 回退的每一步 undo 会把
                // 命令推进重做栈,先清空会被跳转自身重新填上(丢弃失效)
                self.history_jump_to(depth);
                self.undo.clear_redo();
            }
            2 => self.jump_confirm = None,
            _ => {}
        }
    }

    /// 回退到指定撤销深度(连续 undo;选区/悬空引用清理与菜单撤销同款收敛)。
    pub(crate) fn history_jump_to(&mut self, depth: usize) {
        let mut n = 0usize;
        while self.undo.undo_len() > depth {
            match self.undo.undo(&mut self.doc) {
                Ok(_) => n += 1,
                Err(e) => {
                    self.toast_error(format!("历史跳转中断:{e}"));
                    break;
                }
            }
        }
        self.after_history_move(format!("已回退 {n} 步(深度 {depth})"));
    }

    /// 前进 `n` 步重做(历史重do区点击;无丢弃语义)。
    pub(crate) fn history_redo_n(&mut self, n: usize) {
        let mut done = 0usize;
        for _ in 0..n {
            match self.undo.redo(&mut self.doc) {
                Ok(Some(_)) => done += 1,
                Ok(None) => break,
                Err(e) => {
                    self.toast_error(format!("历史跳转中断:{e}"));
                    break;
                }
            }
        }
        self.after_history_move(format!("已重做 {done} 步"));
    }

    /// 跳转后的公共收敛:悬空引用清理 + 状态提示(与撤销命令同款纪律)。
    fn after_history_move(&mut self, msg: String) {
        self.selection.retain(|s| self.doc.find_by_sid(s).is_some());
        self.isolate_stack
            .retain(|id| self.doc.nodes.get(*id).is_some());
        self.editing_text = None;
        self.ds_vertex = None;
        self.say(msg);
    }
}

// ─────────────────────── 单测(历史跳转状态机) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::dock_layout::WorkspaceConfig;

    fn app_fresh() -> VellumApp {
        let file =
            std::env::temp_dir().join(format!("vb-history-gate-{}.json", std::process::id()));
        unsafe {
            std::env::set_var("VB_WORKSPACE", &file);
        }
        let ctx = egui::Context::default();
        VellumApp::construct(
            &ctx,
            WorkspaceConfig::default(),
            [0, 1, 2, 3],
            None,
            vb_doc::model::Document::new_default(),
            None,
        )
    }

    /// 三次重命名:A → B → C(每条独立 undo)。
    /// 关闭合并(`merging_enabled`):否则同目标 Rename 在 500ms 合并窗口内
    /// 并成一条,历史列表就没有三行了(生产语义正确,测试要的是分步)。
    fn rename_thrice(app: &mut VellumApp) -> String {
        let ab = app.doc.artboards[0];
        let sid = app.doc.nodes.get(ab).unwrap().sid.as_str().to_string();
        app.undo.merging_enabled = false;
        for name in ["B", "C", "D"] {
            app.exec(Command::Rename {
                sid: sid.clone(),
                new: name.into(),
                old: None,
            });
        }
        app.undo.merging_enabled = true;
        sid
    }

    #[test]
    fn history_rows_reflect_stack_order_and_current() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh();
        rename_thrice(&mut app);
        let rows = history_rows(app.undo.undo_slice(), app.undo.redo_slice());
        assert_eq!(rows.len(), 3, "三次编辑 = 三行历史");
        assert_eq!(rows[0].label, "重命名");
        assert!(rows[0].done && !rows[0].current);
        assert!(rows[2].current, "栈顶 = 当前位置");
        assert_eq!(rows[0].target_depth, 1);
        assert_eq!(rows[2].target_depth, 3);
        // 上限截断:只保留最近 HISTORY_MAX 行
        app.undo.merging_enabled = false;
        for _ in 0..(HISTORY_MAX + 5) {
            let ab = app.doc.artboards[0];
            let sid = app.doc.nodes.get(ab).unwrap().sid.as_str().to_string();
            app.exec(Command::Rename {
                sid,
                new: "x".into(),
                old: None,
            });
        }
        app.undo.merging_enabled = true;
        let rows = history_rows(app.undo.undo_slice(), app.undo.redo_slice());
        assert_eq!(rows.len(), HISTORY_MAX, "历史列表必须截到最近 N 步");
        assert!(rows.last().unwrap().current);
    }

    #[test]
    fn jump_backward_keeps_redo_tail_and_forward_consumes_it() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh();
        let sid = rename_thrice(&mut app);
        assert_eq!(app.undo.undo_len(), 3);
        assert_eq!(app.undo.redo_len(), 0);

        // 回退到深度 1:重做尾保留(可撤销的撤销)
        app.history_jump_to(1);
        assert_eq!(app.undo.undo_len(), 1);
        assert_eq!(app.undo.redo_len(), 2);
        let ab = app.doc.find_by_sid(&sid).unwrap();
        assert_eq!(
            app.doc.nodes.get(ab).unwrap().name,
            "B",
            "深度 1 = 第一次重命名后"
        );

        // 前进两步:重做尾被消费,回到栈顶
        app.history_redo_n(2);
        assert_eq!(app.undo.undo_len(), 3);
        assert_eq!(app.undo.redo_len(), 0);
        let ab = app.doc.find_by_sid(&sid).unwrap();
        assert_eq!(app.doc.nodes.get(ab).unwrap().name, "D");

        // 再次回退 → 丢弃重做尾(确认语义的第二半:clear_redo + 跳转)
        app.history_jump_to(1);
        app.undo.clear_redo();
        assert_eq!(app.undo.redo_len(), 0, "丢弃后不得还能重做");
        assert_eq!(app.undo.undo_len(), 1);
    }

    #[test]
    fn jump_confirm_is_armed_only_for_backward_with_redo_tail() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh();
        rename_thrice(&mut app);
        // 造一个重做尾
        app.history_jump_to(2);
        assert_eq!(app.undo.redo_len(), 1);
        // 模拟面板点击"深度 1"(回退且 redo 尾存在)→ 状态机要求确认
        let rows = history_rows(app.undo.undo_slice(), app.undo.redo_slice());
        let target = rows.iter().find(|r| r.done && r.target_depth == 1).unwrap();
        assert!(target.target_depth < app.undo.undo_len() && app.undo.redo_len() > 0);
        // 确认路径:先跳转、后清空(顺序反了会被跳转自身的 undo 重新填上)
        app.history_jump_to(target.target_depth);
        app.undo.clear_redo();
        assert_eq!(app.undo.undo_len(), 1);
        assert_eq!(app.undo.redo_len(), 0);
        // 重做区行的折算(前进无确认语义):再造两条编辑 → 回退一步,
        // 重做区两行的 target_depth 必须是"前进后深度",且不落在已发生区
        app.undo.merging_enabled = false;
        let ab = app.doc.artboards[0];
        let sid = app.doc.nodes.get(ab).unwrap().sid.as_str().to_string();
        for name in ["E", "F"] {
            app.exec(Command::Rename {
                sid: sid.clone(),
                new: name.into(),
                old: None,
            });
        }
        app.undo.merging_enabled = true;
        app.history_jump_to(2);
        assert_eq!(app.undo.redo_len(), 1, "深度 3 → 2 只回退一步");
        let rows = history_rows(app.undo.undo_slice(), app.undo.redo_slice());
        let redo_rows: Vec<_> = rows.iter().filter(|r| !r.done).collect();
        assert_eq!(redo_rows.len(), 1, "重做区一行");
        assert_eq!(redo_rows[0].target_depth, 3, "下一个重做 = 前进后的深度");
    }
}
