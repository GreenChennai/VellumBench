//! 键位方案编辑器 GUI(阶段 5 / 05-4-A2;台账 09-L)。
//!
//! 「编辑 → 键盘快捷键…」:命令列表(按菜单分组)+ 当前键位 +
//! 冲突检测(重复绑定红标;应用时检测到冲突**拒绝**并给中文原因)+
//! 录制新键(点「修改」后按下一个组合键;Esc 取消)+ 还原单行 /
//! 恢复默认方案。数据与叠加解析在 [`crate::keymap`](`keymap.json`,
//! 与 recent.json 同目录同范式);本文件只摸 `VellumApp` 会话字段。
//!
//! 门禁:静态注册表自检(`shortcuts.rs` 门禁测试)不受影响 —— 用户
//! 方案是**覆盖层**;加载/保存的冲突判定与运行时解析共用
//! [`crate::keymap::combo_conflicts`] / [`crate::keymap::resolve_effective`]。

use egui::Key;

use crate::keymap::{self, KeyBinding};

use super::VellumApp;

/// 编辑器会话态(`VellumApp.keymap_editor`;行表在打开时构建)。
#[derive(Default)]
pub(crate) struct KeymapEditor {
    /// 命令行表:(组名, 命令 id)。「编辑 → 键盘快捷键…」打开时重建。
    pub rows: Vec<(&'static str, &'static str)>,
    /// 组过滤选择(下标进 [`Self::group_names`];0 = 全部,末位 = 其他)。
    pub group_sel: usize,
    /// 关键字过滤(标签 / id 子串,大小写不敏感)。
    pub filter: String,
    /// 录制中的行(rows 下标);Esc 或录到主键后退出录制态。
    pub recording: Option<usize>,
    /// 录制发起行的命令 id(待应用组合键归属哪一行)。
    pub pending_row: Option<String>,
    /// 录到的待应用组合键(与 `pending_row` 配对展示)。
    pub pending: Option<(Key, bool, bool, bool)>,
    /// 冲突/保存错误(红字展示;下一次成功应用后清除)。
    pub error: Option<String>,
}

impl KeymapEditor {
    /// 组名表(过滤下拉;「全部」+ 各菜单 + 「其他」)。
    pub fn group_names() -> Vec<&'static str> {
        let mut v = vec!["全部"];
        v.extend(crate::shortcuts::MENU_TITLES.iter().copied());
        v.push("其他");
        v
    }

    /// 「其他」组在组名表中的下标。
    pub fn others_index() -> usize {
        Self::group_names().len() - 1
    }

    /// 构建行表:全部已实现命令,按菜单分组;不在任何菜单的命令
    /// (工具/主页/命令面板等)归「其他」。打开窗口时调用一次。
    pub fn rebuild_rows(&mut self) {
        self.rows.clear();
        let mut seen: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
        for menu in crate::shortcuts::MENUS {
            for item in *menu {
                if seen.insert(item.id) {
                    self.rows
                        .push((crate::shortcuts::menu_group_of(item.id), item.id));
                }
            }
        }
        for id in crate::shortcuts::IMPLEMENTED_IDS {
            if seen.insert(id) {
                self.rows.push((crate::shortcuts::menu_group_of(id), id));
            }
        }
    }

    /// 过滤后的行(组选择 + 关键字;关键字对中文标签与 id 生效)。
    pub fn filtered(&self) -> Vec<(&'static str, &'static str)> {
        let groups = Self::group_names();
        let sel = self.group_sel;
        let others = Self::others_index();
        let q = self.filter.trim().to_lowercase();
        self.rows
            .iter()
            .copied()
            .filter(|&(group, id)| {
                let in_menu = crate::shortcuts::MENUS
                    .iter()
                    .flat_map(|m| m.iter())
                    .any(|i| i.id == id);
                let group_hit = sel == 0
                    || (sel == others && !in_menu)
                    || (sel > 0 && sel < others && group == groups[sel]);
                let text_hit = q.is_empty()
                    || id.to_lowercase().contains(&q)
                    || crate::shortcuts::command_label(id)
                        .map(|l| l.to_lowercase().contains(&q))
                        .unwrap_or(false);
                group_hit && text_hit
            })
            .collect()
    }

    /// 测试别名:过滤后的行(与 [`Self::filtered`] 同义)。
    #[cfg(test)]
    pub fn visible_rows(&self) -> Vec<(&'static str, &'static str)> {
        self.filtered()
    }
}

impl VellumApp {
    /// 「编辑 → 键盘快捷键…」窗口(09-L)。
    pub(crate) fn show_keymap_window(&mut self, ui: &mut egui::Ui) {
        if !self.keymap_open {
            return;
        }
        if self.keymap_editor.rows.is_empty() {
            self.keymap_editor.rebuild_rows();
        }
        // 录制态:捕获下一帧按键事件(纯修饰键等待;Esc 取消)
        if self.keymap_editor.recording.is_some() {
            let hit: Option<(Key, bool, bool, bool)> = ui.ctx().input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        modifiers,
                        ..
                    } => Some((
                        *key,
                        modifiers.ctrl || modifiers.command,
                        modifiers.shift,
                        modifiers.alt,
                    )),
                    _ => None,
                })
            });
            if let Some((k, c, s, a)) = hit {
                if k == Key::Escape && !c && !s && !a {
                    // Esc:取消录制(不关窗口)
                    self.keymap_editor.recording = None;
                    self.keymap_editor.pending = None;
                    self.keymap_editor.pending_row = None;
                } else if !keymap::is_modifier_key(k) {
                    // 主键落下:进入「待应用」态,由发起行确认
                    self.keymap_editor.pending = Some((k, c, s, a));
                    self.keymap_editor.recording = None;
                }
            }
        }
        let mut open = true;
        let mut action: Option<KeymapAction> = None;
        egui::Window::new("键盘快捷键")
            .open(&mut open)
            .collapsible(false)
            .default_size([580.0, 460.0])
            .show(ui.ctx(), |ui| {
                // 过滤行:组下拉 + 关键字
                let groups = KeymapEditor::group_names();
                ui.horizontal(|ui| {
                    ui.label("分组");
                    let sel = self.keymap_editor.group_sel;
                    egui::ComboBox::from_id_salt("vb-keymap-group")
                        .selected_text(groups.get(sel).copied().unwrap_or("全部"))
                        .show_ui(ui, |ui| {
                            for (i, g) in groups.iter().enumerate() {
                                ui.selectable_value(&mut self.keymap_editor.group_sel, i, *g);
                            }
                        });
                    ui.label("搜索");
                    ui.add_sized(
                        [150.0, vb_ui::theme::row_height(ui.ctx())],
                        egui::TextEdit::singleline(&mut self.keymap_editor.filter)
                            .hint_text("命令名 / id…"),
                    );
                });
                ui.separator();
                // 录制提示(录制态置顶,避免被列表推出视野)
                if let Some(row) = self.keymap_editor.recording {
                    let label = self
                        .keymap_editor
                        .rows
                        .get(row)
                        .and_then(|(_, id)| crate::shortcuts::command_label(id))
                        .unwrap_or("");
                    ui.colored_label(
                        vb_ui::theme::tokens(ui.ctx()).warn,
                        format!("正在录制「{label}」:请按下新的组合键(Esc 取消)"),
                    );
                    ui.separator();
                }
                // 命令行表(过滤后;行内动作收集后统一执行)
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let rows = self.keymap_editor.filtered();
                    for (group, id) in rows {
                        self.keymap_row(ui, group, id, &mut action);
                    }
                });
                ui.separator();
                // 冲突拒绝原因(红字)
                if let Some(err) = &self.keymap_editor.error {
                    ui.colored_label(vb_ui::theme::tokens(ui.ctx()).danger, err);
                }
                ui.horizontal(|ui| {
                    if ui.button("恢复默认方案").clicked() {
                        action = Some(KeymapAction::ResetAll);
                    }
                    ui.weak(match keymap::keymap_path() {
                        Some(p) => format!("方案文件:{}", p.display()),
                        None => "方案文件:不可用(未找到配置目录)".to_string(),
                    });
                });
            });
        // 动作统一收口(渲染后执行,避免借用冲突)
        match action {
            Some(KeymapAction::Apply { id, combo }) => self.keymap_apply(id, combo),
            Some(KeymapAction::Clear { id }) => self.keymap_clear(id),
            Some(KeymapAction::ResetAll) => self.keymap_reset_all(),
            None => {}
        }
        if !open {
            // 关窗:录制/待应用态一并作废
            self.keymap_open = false;
            self.keymap_editor.recording = None;
            self.keymap_editor.pending = None;
            self.keymap_editor.pending_row = None;
        }
    }

    /// 单行:组名 · 命令名(冲突红标)+ 当前键位 + 修改 / 还原。
    fn keymap_row(
        &mut self,
        ui: &mut egui::Ui,
        group: &'static str,
        id: &'static str,
        action: &mut Option<KeymapAction>,
    ) {
        let label = crate::shortcuts::command_label(id).unwrap_or(id);
        let current = self.menu_key_text(id).unwrap_or_else(|| "—".into());
        // 冲突红标:当前组合键在有效键位集内有其它命令占用
        let combo = keymap::parse_combo(&current);
        let conflict_ids = combo
            .as_ref()
            .map(|c| keymap::combo_conflicts(&self.keymap, &self.keymap_live, c, id))
            .unwrap_or_default();
        let overridden = keymap::is_overridden(&self.keymap, id);
        let pending_here = self.keymap_editor.pending.is_some()
            && self.keymap_editor.pending_row.as_deref() == Some(id);
        let t = vb_ui::theme::tokens(ui.ctx());
        ui.horizontal(|ui| {
            let text = if conflict_ids.is_empty() {
                egui::RichText::new(format!("{group} · {label}")).size(12.0)
            } else {
                egui::RichText::new(format!(
                    "{group} · {label}(与 {} 冲突)",
                    conflict_ids
                        .iter()
                        .map(|c| crate::shortcuts::command_label(c).unwrap_or(c))
                        .collect::<Vec<_>>()
                        .join("、")
                ))
                .size(12.0)
                .color(t.danger)
            };
            ui.label(text);
            // 当前键位(冲突行红底标 ⚠)
            let key_text = if conflict_ids.is_empty() {
                current.clone()
            } else {
                format!("{current} ⚠")
            };
            ui.colored_label(
                if conflict_ids.is_empty() {
                    t.text_2
                } else {
                    t.danger
                },
                key_text,
            );
            if pending_here {
                if let Some((k, c, s, a)) = self.keymap_editor.pending {
                    ui.colored_label(t.warn, keymap::combo_text(k, c, s, a));
                    if ui.small_button("应用").clicked() {
                        *action = Some(KeymapAction::Apply {
                            id: id.to_string(),
                            combo: keymap::combo_text(k, c, s, a),
                        });
                    }
                    if ui.small_button("取消").clicked() {
                        self.keymap_editor.pending = None;
                        self.keymap_editor.pending_row = None;
                    }
                }
            } else if ui.small_button("修改").clicked() {
                // 进入录制态(清旧 pending 与错误)
                self.keymap_editor.pending = None;
                self.keymap_editor.pending_row = Some(id.to_string());
                self.keymap_editor.error = None;
                self.keymap_editor.recording = self
                    .keymap_editor
                    .rows
                    .iter()
                    .position(|(_, rid)| *rid == id);
            }
            if overridden && ui.small_button("还原").clicked() {
                *action = Some(KeymapAction::Clear { id: id.to_string() });
            }
        });
    }

    /// 应用一条覆盖(冲突 → 拒绝并给中文原因;成功 → 重建有效集 + 落盘)。
    pub(crate) fn keymap_apply(&mut self, id: String, combo: String) {
        let Some((k, c, s, a)) = keymap::parse_combo(&combo) else {
            self.keymap_editor.error = Some(format!("无法识别组合键「{combo}」"));
            return;
        };
        let hits = keymap::combo_conflicts(&self.keymap, &self.keymap_live, &(k, c, s, a), &id);
        if !hits.is_empty() {
            let names: Vec<&str> = hits
                .iter()
                .map(|h| crate::shortcuts::command_label(h).unwrap_or(h))
                .collect();
            self.keymap_editor.error = Some(format!(
                "已拒绝:「{combo}」已被 {} 使用(同一组合键只能绑定一个命令)",
                names.join("、")
            ));
            return;
        }
        self.keymap_editor.error = None;
        self.keymap.bindings.retain(|b| b.id != id);
        self.keymap.bindings.push(KeyBinding {
            id: id.clone(),
            combo,
        });
        self.keymap_reload_live();
        let label = crate::shortcuts::command_label(&id).unwrap_or(&id);
        self.say(format!(
            "键位已更新:{label} → {}",
            self.menu_key_text(&id).unwrap_or_default()
        ));
    }

    /// 还原单行默认(移除该命令的覆盖;落盘)。
    pub(crate) fn keymap_clear(&mut self, id: String) {
        self.keymap.bindings.retain(|b| b.id != id);
        self.keymap_reload_live();
        self.keymap_editor.error = None;
        let label = crate::shortcuts::command_label(&id).unwrap_or(&id);
        self.say(format!(
            "「{label}」已还原默认键位({})",
            crate::shortcuts::key_text_for(&id).unwrap_or_else(|| "无绑定".into())
        ));
    }

    /// 恢复默认方案(清空全部覆盖;落盘)。
    pub(crate) fn keymap_reset_all(&mut self) {
        self.keymap.bindings.clear();
        self.keymap_reload_live();
        self.keymap_editor.error = None;
        self.say("键位方案已恢复默认(所有自定义覆盖已清除)");
    }

    /// 重建有效键位集并落盘(apply/clear/reset 的共同尾部)。
    fn keymap_reload_live(&mut self) {
        let (live, warns) = keymap::build_live(&self.keymap);
        for w in warns {
            log::warn!("{w}");
        }
        self.keymap_live = live;
        if let Err(e) = keymap::save(&self.keymap) {
            self.toast_warn(e);
        }
    }
}

/// 行内动作(渲染后统一执行,避免 `&mut self` 嵌套借用)。
enum KeymapAction {
    Apply { id: String, combo: String },
    Clear { id: String },
    ResetAll,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;

    /// 行表构建:全部已实现命令都在表内(按菜单分组 + 「其他」收尾),
    /// 组过滤语义:按菜单名过滤 / 「其他」只收非菜单命令;关键字可搜中文标签。
    #[test]
    fn rows_cover_all_implemented_and_group_filter_works() {
        let mut ed = KeymapEditor::default();
        ed.rebuild_rows();
        let ids: std::collections::HashSet<&str> = ed.rows.iter().map(|(_, id)| *id).collect();
        let mut all: std::collections::HashSet<&str> =
            crate::shortcuts::IMPLEMENTED_IDS.iter().copied().collect();
        all.extend(
            crate::shortcuts::MENUS
                .iter()
                .flat_map(|m| m.iter())
                .map(|i| i.id),
        );
        assert_eq!(ids, all, "行表必须覆盖全部命令(菜单 + 其他)");
        // 「文件」组过滤
        ed.group_sel = 1; // 0=全部,1=文件
        let visible = ed.visible_rows().len();
        assert!(visible > 0 && visible < ed.rows.len(), "文件组应是子集");
        // 「其他」组:只收非菜单命令(如 tool.* / home.*)
        ed.group_sel = KeymapEditor::others_index();
        let others = ed.visible_rows();
        assert!(
            others.iter().all(|(_, id)| !crate::shortcuts::MENUS
                .iter()
                .flat_map(|m| m.iter())
                .any(|i| i.id == *id)),
            "「其他」组不得含菜单命令"
        );
        assert!(
            others.iter().any(|(_, id)| *id == "tool.select"),
            "工具命令应归「其他」组"
        );
        // 关键字过滤(中文标签)
        ed.group_sel = 0;
        ed.filter = "撤销".into();
        assert!(!ed.visible_rows().is_empty(), "中文标签必须可搜");
    }

    /// 编辑器动作链:应用覆盖 → 冲突拒绝 → 运行时解析(新键活/旧键死)→
    /// 还原单行 → 恢复默认方案。`VB_KEYMAP` 指临时文件,验证落盘不落真实配置目录。
    #[test]
    fn apply_reject_conflicts_and_reset_flow() {
        let _env = crate::ENV_LOCK.lock();
        let file =
            std::env::temp_dir().join(format!("vb-keymap-dialog-{}.json", std::process::id()));
        unsafe {
            std::env::set_var("VB_KEYMAP", &file);
        }
        let _ = std::fs::remove_file(&file);
        let mut app = app_fresh(None);
        // 默认:无覆盖
        assert!(app.keymap.bindings.is_empty());
        // ① 应用:撤销 → Ctrl+P
        app.keymap_apply("edit.undo".into(), "Ctrl+P".into());
        assert!(app.keymap_editor.error.is_none());
        assert_eq!(
            app.menu_key_text("edit.undo").as_deref(),
            Some("Ctrl+P"),
            "覆盖后的键位文本必须生效(菜单同步)"
        );
        assert!(file.exists(), "应用即落盘");
        // ② 应用冲突:全选(Ctrl+A)→ Ctrl+P 已被撤销占用 → 拒绝
        app.keymap_apply("edit.select_all".into(), "Ctrl+P".into());
        let err = app
            .keymap_editor
            .error
            .clone()
            .expect("冲突必须给出拒绝原因");
        assert!(err.contains("已拒绝") && err.contains("撤销"), "{err}");
        assert!(
            !keymap::is_overridden(&app.keymap, "edit.select_all"),
            "被拒绝的绑定不得写入"
        );
        // ③ 运行时解析:Ctrl+P = 撤销;Ctrl+Z(旧键位)死亡
        let (id, _) =
            keymap::resolve_effective(&app.keymap, &app.keymap_live, Key::P, true, false, false)
                .expect("新键位必须可解析");
        assert_eq!(id, "edit.undo");
        assert!(
            keymap::resolve_effective(&app.keymap, &app.keymap_live, Key::Z, true, false, false)
                .is_none(),
            "被重绑命令的旧键位必须失效"
        );
        // ④ 还原单行:撤销回 Ctrl+Z
        app.keymap_clear("edit.undo".into());
        let (id, _) =
            keymap::resolve_effective(&app.keymap, &app.keymap_live, Key::Z, true, false, false)
                .expect("还原后旧键位必须恢复");
        assert_eq!(id, "edit.undo");
        // ⑤ 再应用一条后「恢复默认方案」:覆盖清空且落盘
        // Ctrl+Alt+P 空闲(Ctrl+Y 已被轮廓模式占用,会被冲突拒绝)
        app.keymap_apply("edit.redo".into(), "Ctrl+Alt+P".into());
        assert!(!app.keymap.bindings.is_empty());
        app.keymap_reset_all();
        assert!(app.keymap.bindings.is_empty());
        let (back, _) = keymap::load_from(&file);
        assert!(back.bindings.is_empty(), "恢复默认必须落盘");
        let _ = std::fs::remove_file(&file);
        unsafe { std::env::remove_var("VB_KEYMAP") };
    }
}
