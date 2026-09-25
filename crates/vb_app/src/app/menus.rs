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
                // 动态动作 = 自定义工作区切换("preset:<名>";X-7)
                let mut dyn_fired: Option<String> = None;

                for (idx, title) in shortcuts::MENU_TITLES.iter().enumerate() {
                    ui.menu_button(*title, |ui| {
                        self.menu_section(ui, idx, &mut fired);
                        // X-7:窗口菜单在静态项之后动态列出用户工作区预设
                        // (保存于 workspace.json workspace_presets,可切换;
                        //  删除走「窗口 → 新建工作区…」对话框)
                        if *title == "窗口" && !self.workspace_presets().is_empty() {
                            ui.separator();
                            ui.label(egui::RichText::new("自定义工作区").size(11.0).weak());
                            let names: Vec<String> = self
                                .workspace_presets()
                                .iter()
                                .map(|p| p.name.clone())
                                .collect();
                            for name in names {
                                if ui.button(format!("工作区 → {name}")).clicked() {
                                    dyn_fired = Some(format!("preset:{name}"));
                                    ui.close();
                                }
                            }
                        }
                    });
                }

                if let Some(id) = fired {
                    self.run_command(id, false, false);
                }
                if let Some(dyn_cmd) = dyn_fired {
                    if let Some(name) = dyn_cmd.strip_prefix("preset:") {
                        self.workspace_preset_apply(name);
                    }
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
                let key_text = self.menu_key_text(item.id);
                if menu_item_button_with(ui, item, &label, enabled, key_text).clicked() {
                    *fired = Some(item.id);
                    ui.close();
                }
                continue;
            }
            // ── 视图:画面/界面开关以复选呈现(状态仍由命令派发统一改写)──
            match item.id {
                "view.toggle_grid"
                | "view.toggle_smart_guides"
                | "view.outline"
                | "view.toggle_theme"
                // 04-4:开发者统计(默认关)与提示条(默认开)
                | "view.developer_stats"
                | "view.toggle_hints"
                // 04-6:显示未支持工具(默认关,design/06 §二)
                | "edit.toggle_unsupported_tools" => {
                    let mut cur = match item.id {
                        "view.toggle_grid" => self.grid_on,
                        "view.toggle_smart_guides" => self.smart_guides_on,
                        "view.toggle_theme" => !self.theme_dark,
                        "view.developer_stats" => self.dev_stats,
                        "view.toggle_hints" => self.hints,
                        "edit.toggle_unsupported_tools" => self.show_all_tools,
                        _ => self.outline_mode,
                    };
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut cur, item.label).changed() {
                            *fired = Some(item.id);
                        }
                        // 键位文本查有效键位集(用户方案覆盖优先,05-4-A2)
                        if let Some(k) = self.menu_key_text(item.id) {
                            ui.weak(k);
                        }
                    });
                }
                _ => {
                    // 未落地项:置灰 + 悬停即见「计划于 vX」
                    let planned = shortcuts::planned_reason(item.id).is_none();
                    // 键位文本查有效键位集(用户方案覆盖优先,05-4-A2)
                    let key_text = self.menu_key_text(item.id);
                    if menu_item_button_with(ui, item, "", planned, key_text).clicked() {
                        *fired = Some(item.id);
                        ui.close();
                    }
                }
            }
        }
    }
}

/// 菜单项按钮:标签 + 键位文本(调用方传入 —— 查**有效键位集**,用户
/// 方案覆盖优先,05-4-A2);`extra` 为附在标签后的补充文本
/// (如"撤销"后面的会撤销什么)。禁用时若该项有计划说明,悬停给出原因。
fn menu_item_button_with(
    ui: &mut egui::Ui,
    item: &shortcuts::MenuItem,
    extra: &str,
    enabled: bool,
    key_text: Option<String>,
) -> egui::Response {
    let label = if extra.is_empty() {
        item.label.to_string()
    } else {
        format!("{} {}", item.label, extra)
    };
    let btn = match key_text {
        Some(k) => egui::Button::new(label).shortcut_text(k),
        None => egui::Button::new(label),
    };
    let resp = ui.add_enabled(enabled, btn);
    match shortcuts::planned_reason(item.id) {
        Some(reason) if !enabled => resp.on_disabled_hover_text(reason),
        // 路径查找器各项悬停即见输出语义(05-3 / X-1)
        _ => match shortcuts::pathfinder_tip(item.id) {
            Some(tip) => resp.on_hover_text(tip),
            None => resp,
        },
    }
}
