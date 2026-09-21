//! 顶部菜单栏(阶段 5:AI 规范 9 项,副文档 06)。
//!
//! 三条纪律:
//! 1. **菜单结构是数据** —— 标题取 [`shortcuts::MENU_TITLES`]、条目取
//!    [`shortcuts::MENUS`],本文件只按注册表渲染,不另立第二份清单;
//! 2. **键位文本一律查注册表**(`shortcuts::key_text_for`),禁止在 label 里手写;
//! 3. **未落地项置灰 + 悬停提示**(`shortcuts::planned_reason`),
//!    绝不出现"点了没反应"的项(`design/06 §七`)。

use crate::shortcuts;

use super::VellumApp;

impl VellumApp {
    pub(crate) fn top_menu(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                // 点击的菜单项先收集,菜单全部渲染完再派发(避免借用冲突)。
                let mut fired: Option<&'static str> = None;

                for (idx, title) in shortcuts::MENU_TITLES.iter().enumerate() {
                    ui.menu_button(*title, |ui| {
                        self.menu_section(ui, idx, &mut fired);
                    });
                }

                if let Some(id) = fired {
                    self.run_command(id, false, false);
                }
            });
        });
    }

    /// 渲染第 `idx` 个菜单的条目(与 [`shortcuts::MENUS`] 同序)。
    fn menu_section(&mut self, ui: &mut egui::Ui, idx: usize, fired: &mut Option<&'static str>) {
        let Some(items) = shortcuts::MENUS.get(idx) else {
            return;
        };
        for item in *items {
            // ── 编辑:撤销/重做显示"会撤销什么",并按可撤销性置灰 ──
            if item.id == "edit.undo" || item.id == "edit.redo" {
                let (label, enabled) = match item.id {
                    "edit.undo" => (
                        self.undo.undo_label().unwrap_or("").to_string(),
                        self.undo.can_undo(),
                    ),
                    _ => (
                        self.undo.redo_label().unwrap_or("").to_string(),
                        self.undo.can_redo(),
                    ),
                };
                if menu_item_button_with(ui, item, &label, enabled).clicked() {
                    *fired = Some(item.id);
                    ui.close();
                }
                continue;
            }
            // ── 视图:四个画面开关以复选呈现(状态仍由命令派发统一改写)──
            match item.id {
                "view.toggle_grid"
                | "view.toggle_smart_guides"
                | "view.outline"
                | "view.toggle_theme" => {
                    let mut cur = match item.id {
                        "view.toggle_grid" => self.grid_on,
                        "view.toggle_smart_guides" => self.smart_guides_on,
                        "view.toggle_theme" => !self.theme_dark,
                        _ => self.outline_mode,
                    };
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut cur, item.label).changed() {
                            *fired = Some(item.id);
                        }
                        if let Some(k) = shortcuts::key_text_for(item.id) {
                            ui.weak(k);
                        }
                    });
                }
                _ => {
                    // 未落地项:置灰 + 悬停即见「计划于 vX」
                    let planned = shortcuts::planned_reason(item.id).is_none();
                    if menu_item_button_with(ui, item, "", planned).clicked() {
                        *fired = Some(item.id);
                        ui.close();
                    }
                }
            }
        }
    }
}

/// 菜单项按钮:标签 + 自动查表得到的键位文本;`extra` 为附在标签后的补充文本
/// (如"撤销"后面的会撤销什么)。禁用时若该项有计划说明,悬停给出原因。
fn menu_item_button_with(
    ui: &mut egui::Ui,
    item: &shortcuts::MenuItem,
    extra: &str,
    enabled: bool,
) -> egui::Response {
    let label = if extra.is_empty() {
        item.label.to_string()
    } else {
        format!("{} {}", item.label, extra)
    };
    let btn = match shortcuts::key_text_for(item.id) {
        Some(k) => egui::Button::new(label).shortcut_text(k),
        None => egui::Button::new(label),
    };
    let resp = ui.add_enabled(enabled, btn);
    match shortcuts::planned_reason(item.id) {
        Some(reason) if !enabled => resp.on_disabled_hover_text(reason),
        _ => resp,
    }
}
