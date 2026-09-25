//! 「帮助 → 能力台账」窗口(副文档 09-3 的用户可见面)。
//!
//! 台账数据在 [`crate::capabilities`](crate::capabilities)(单一真相);
//! 本文件只把它渲染出来:按状态分组,「计划」项一律带「计划于 vX」说明,
//! 并列出相关命令 ID —— **让用户当场能看到什么做了、什么没做、怎么触发**,
//! 而不是靠文档口径(`design/06 §七`:不允许点了没反应,也不允许含糊)。

use vb_ui::components::caption;

use crate::app::VellumApp;
use crate::capabilities::{CapStatus, CAPABILITIES};

impl VellumApp {
    pub(crate) fn show_capabilities_window(&mut self, ui: &mut egui::Ui) {
        if !self.capabilities_ui.open {
            return;
        }
        let mut open = true;
        let t = vb_ui::theme::Tokens::get(self.theme_dark);
        egui::Window::new("能力台账")
            .open(&mut open)
            .collapsible(false)
            .default_width(460.0)
            .max_height(560.0)
            .show(ui.ctx(), |ui| {
                let n_done = CAPABILITIES
                    .iter()
                    .filter(|c| c.status == CapStatus::Done)
                    .count();
                let n_partial = CAPABILITIES
                    .iter()
                    .filter(|c| matches!(c.status, CapStatus::Partial(_)))
                    .count();
                let n_dropped = CAPABILITIES
                    .iter()
                    .filter(|c| matches!(c.status, CapStatus::Dropped(_)))
                    .count();
                let n_planned = CAPABILITIES.len() - n_done - n_partial - n_dropped;
                ui.label(caption(
                    ui,
                    &format!(
                        "共 {} 条:已落地 {n_done} · 部分 {n_partial} · 计划 {n_planned} · 不做 {n_dropped}。\
                         本表是「还有哪些没做」的单一真相。",
                        CAPABILITIES.len()
                    ),
                ));
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for c in CAPABILITIES {
                        ui.horizontal(|ui| {
                            let color = match c.status {
                                CapStatus::Done => t.text,
                                CapStatus::Partial(_) => t.accent,
                                CapStatus::Planned(_) => t.text_3,
                                // 05-1 三态收敛:「不做」要一眼可辨且带理由
                                CapStatus::Dropped(_) => t.text_3,
                            };
                            ui.label(
                                egui::RichText::new(format!("[{}]", c.status.badge()))
                                    .color(color)
                                    .monospace(),
                            );
                            ui.label(egui::RichText::new(format!("{} {}", c.id, c.name)).strong());
                        });
                        let note = c.status.note();
                        if !note.is_empty() {
                            ui.label(caption(ui, &format!("    {note}")));
                        }
                        if !c.commands.is_empty() {
                            let ids = c.commands.join(" · ");
                            ui.label(caption(ui, &format!("    命令:{ids}")));
                        }
                        ui.add_space(4.0);
                    }
                });
            });
        self.capabilities_ui.open = open;
    }
}
