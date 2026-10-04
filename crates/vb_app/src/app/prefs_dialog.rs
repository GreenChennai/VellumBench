//! 首选项九分类对话框(阶段 5 / 05-4-A2;台账 09-L)。
//!
//! 信息架构对齐 AI「首选项」九分类(分册 07 §3 参照表):
//! ①常规 ②文字 ③单位与标尺 ④参考线与网格 ⑤智能参考线(吸附)
//! ⑥画板 ⑦性能 ⑧外观 ⑨数据。
//!
//! **收编原则**:既有散落的设置项(工具箱未支持工具、铅笔保真度、
//! 自动保存间隔、UI 缩放、提示条、主题……)**原入口保留兼容**
//! (编辑/视图菜单的命令不动),首选项提供集中入口;两处读写同一批
//! 会话字段,经 `workspace_dirty` → `save_workspace` 统一落
//! `workspace.json`(serde-default 追加字段,旧文件兼容)。
//!
//! **生效时机(每页标明)**:全部为**即时生效** —— 拉动/勾选即改会话
//! 状态,下一存活帧自动落盘;「恢复默认」把九类全部回默认值并同样落盘。
//!
//! 渲染层(本文件)只摸 `VellumApp` 会话字段;纯数据判定在
//! `dock_layout::normalize`(越界拉回)与各命令分支。

use vb_ui::theme;

use super::VellumApp;
use crate::i18n::Lang;

/// 九分类页签(标题与顺序固定;`prefs_tab` 存下标)。
pub(crate) const PREF_PAGES: [&str; 9] = [
    "常规",         // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    "文字",         // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    "单位与标尺",   // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    "参考线与网格", // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    "智能参考线",   // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    "画板",         // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    "性能",         // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    "外观",         // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    "数据",         // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
];

impl VellumApp {
    /// 「编辑 → 首选项…」对话框(09-L)。
    pub(crate) fn show_preferences_window(&mut self, ui: &mut egui::Ui) {
        if !self.prefs_open {
            return;
        }
        let mut open = true;
        egui::Window::new(vb_session::i18n::t("ui-app-dialogs-002"))
            .open(&mut open)
            .collapsible(false)
            .default_size([520.0, 380.0])
            .show(ui.ctx(), |ui| {
                // 页签行(九分类;两行放得下,不滚动)
                let tab = &mut self.prefs_tab;
                ui.horizontal_wrapped(|ui| {
                    for (i, label) in PREF_PAGES.iter().enumerate() {
                        if ui.selectable_label(*tab == i, *label).clicked() {
                            *tab = i;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| match self.prefs_tab {
                    0 => self.prefs_page_general(ui),
                    1 => self.prefs_page_type(ui),
                    2 => self.prefs_page_units(ui),
                    3 => self.prefs_page_grid(ui),
                    4 => self.prefs_page_smart_guides(ui),
                    5 => self.prefs_page_artboard(ui),
                    6 => self.prefs_page_performance(ui),
                    7 => self.prefs_page_appearance(ui),
                    _ => self.prefs_page_data(ui),
                });
                ui.separator();
                ui.horizontal(|ui| {
                    // 恢复默认:九类全部回默认值(即时生效 + 落盘)
                    if ui
                        .button(vb_session::i18n::t("ui-app-prefs-dialog-004"))
                        .clicked()
                    {
                        self.prefs_restore_defaults();
                    }
                    ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-005"));
                });
            });
        self.prefs_open = open;
    }

    // ─────────────────────────── 九分类页 ───────────────────────────

    /// ①常规:工具行为(收编「设置 → 显示未支持工具」「设置 → 铅笔保真度」)
    /// + 界面语言(05-7 i18n 骨架)。
    fn prefs_page_general(&mut self, ui: &mut egui::Ui) {
        // H-1:动效总开关(视图菜单「界面动效」同款;持久化 workspace.json)
        let mut motion = self.motion_enabled;
        if ui
            .checkbox(&mut motion, vb_session::i18n::t("ui-app-prefs-dialog-006"))
            .changed()
        {
            self.motion_enabled = motion;
            self.save_workspace();
            if let Some(tx) = &self.shell_tx {
                let _ = tx.send(crate::shell::ShellRequest::MotionChanged(motion));
            }
        }
        // §8.3.5:密度档(图层行等列表行高 24/28;默认 comfortable)
        let mut compact = self.density_compact;
        if ui
            .checkbox(&mut compact, vb_session::i18n::t("ui-app-prefs-dialog-007"))
            .changed()
        {
            self.density_compact = compact;
            self.save_workspace();
        }
        let mut show_tools = self.show_all_tools;
        if ui
            .checkbox(
                &mut show_tools,
                vb_session::i18n::t("ui-app-prefs-dialog-008"),
            )
            .changed()
        {
            self.show_all_tools = show_tools;
        }
        let mut fidelity = self.pencil_fidelity;
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-app-prefs-dialog-009"));
            ui.add(
                egui::DragValue::new(&mut fidelity)
                    .range(1.0..=20.0)
                    .suffix(" px"),
            );
            ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-010"));
        });
        if (fidelity - self.pencil_fidelity).abs() > f64::EPSILON {
            self.pencil_fidelity = fidelity;
        }
        ui.add_space(6.0);
        // 05-7:界面语言(中文 / English;下一帧生效,持久化 workspace.json)
        {
            let cur = crate::i18n::lang();
            let mut sel = cur;
            ui.horizontal(|ui| {
                ui.label(crate::i18n::t("prefs.ui-language"));
                if ui
                    .selectable_label(cur == Lang::Zh, vb_session::i18n::t("ui-common-chinese"))
                    .clicked()
                {
                    sel = Lang::Zh;
                }
                if ui.selectable_label(cur == Lang::En, "English").clicked() {
                    sel = Lang::En;
                }
            });
            if sel != cur {
                crate::i18n::set_lang(sel);
                self.say(vb_session::i18n::t_args(
                    "ui-app-prefs-dialog-011",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from((sel.code()).to_string()),
                    )],
                ));
            }
            ui.weak(crate::i18n::t("prefs.ui-language-help"));
        }
        ui.add_space(6.0);
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-012"));
    }

    /// ②文字:新建文本默认样式(字符面板「默认样式」区同源)。
    fn prefs_page_type(&mut self, ui: &mut egui::Ui) {
        let mut size = self.text_default.font_size.unwrap_or(24.0);
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-app-prefs-dialog-013"));
            ui.add(
                egui::DragValue::new(&mut size)
                    .range(4.0..=200.0)
                    .suffix(" px"),
            );
        });
        if (size - self.text_default.font_size.unwrap_or(24.0)).abs() > f64::EPSILON {
            self.text_default.font_size = Some(size);
        }
        // 默认颜色(color_edit 即时生效;解析失败的存量值不动)
        if let Some(c) = self
            .text_default
            .color
            .clone()
            .and_then(|v| vb_common::color::parse_color(&v))
        {
            let mut col = egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
            ui.horizontal(|ui| {
                ui.label(vb_session::i18n::t("ui-app-prefs-dialog-014"));
                if ui.color_edit_button_srgba(&mut col).changed() {
                    let [r, g, b, a] = col.to_array();
                    self.text_default.color =
                        Some(vb_common::Rgba::new(r, g, b, a).to_shortest_hex());
                }
            });
        }
        // 默认字体(空 = 继承;文档已有字体见字符面板「文档已有」快选)
        let mut family = self.text_default.font_family.clone().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-app-prefs-dialog-015"));
            if ui
                .add_sized(
                    [180.0, theme::row_height(ui.ctx())],
                    egui::TextEdit::singleline(&mut family)
                        .hint_text(vb_session::i18n::t("ui-app-prefs-dialog-016")),
                )
                .changed()
            {
                self.text_default.font_family = if family.trim().is_empty() {
                    None
                } else {
                    Some(family.trim().to_string())
                };
            }
        });
        ui.add_space(6.0);
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-017"));
    }

    /// ③单位与标尺:HTML/CSS 唯一长度单位是 px(诚实标注,不给假选项);
    /// 标尺默认显隐在此收编。
    fn prefs_page_units(&mut self, ui: &mut egui::Ui) {
        ui.label(vb_session::i18n::t("ui-app-prefs-dialog-018"));
        let mut rulers = self.rulers_on;
        if ui
            .checkbox(
                &mut rulers,
                vb_session::i18n::t("ui-menu-view-toggle_rulers"),
            )
            .changed()
        {
            self.rulers_on = rulers;
        }
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-020"));
    }

    /// ④参考线与网格:网格基础间距(画布网格分级基数)+ 默认显隐收编。
    fn prefs_page_grid(&mut self, ui: &mut egui::Ui) {
        let mut size = self.grid_size;
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-app-prefs-dialog-021"));
            ui.add(
                egui::DragValue::new(&mut size)
                    .range(2.0..=256.0)
                    .suffix(" px"),
            );
            ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-022"));
        });
        if (size - self.grid_size).abs() > f64::EPSILON {
            self.grid_size = size;
        }
        let mut grid_on = self.grid_on;
        if ui
            .checkbox(
                &mut grid_on,
                vb_session::i18n::t("ui-menu-view-toggle_grid"),
            )
            .changed()
        {
            self.grid_on = grid_on;
        }
        let mut guides = self.guides_visible;
        if ui
            .checkbox(
                &mut guides,
                vb_session::i18n::t("ui-menu-view-toggle_guides"),
            )
            .changed()
        {
            self.guides_visible = guides;
        }
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-024"));
    }

    /// ⑤智能参考线(吸附):默认开关收编。
    fn prefs_page_smart_guides(&mut self, ui: &mut egui::Ui) {
        let mut on = self.smart_guides_on;
        if ui
            .checkbox(&mut on, vb_session::i18n::t("ui-app-prefs-dialog-025"))
            .changed()
        {
            self.smart_guides_on = on;
            if !on {
                self.smart_guides.clear();
            }
        }
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-026"));
    }

    /// ⑥画板:新画板默认预设(画板面板「+ 新建」与控制面板「+画板」取此尺寸)。
    fn prefs_page_artboard(&mut self, ui: &mut egui::Ui) {
        let presets = super::panels::artboards::AB_PRESETS;
        let idx = self.artboard_preset.min(presets.len() - 1);
        ui.label(vb_session::i18n::t("ui-app-prefs-dialog-027"));
        ui.horizontal_wrapped(|ui| {
            for (i, (name, w, h)) in presets.iter().enumerate() {
                if ui
                    .selectable_label(idx == i, format!("{name}({w}×{h})"))
                    .clicked()
                {
                    self.artboard_preset = i;
                }
            }
        });
        ui.add_space(6.0);
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-028"));
    }

    /// ⑦性能:渲染后端**只读**展示(诚实标注 —— 本版本无切换开关,
    /// 不给假选项;调试数据仍在「视图 → 开发者统计」)。
    fn prefs_page_performance(&mut self, ui: &mut egui::Ui) {
        ui.label(vb_session::i18n::t("ui-app-prefs-dialog-029"));
        match self.gpu.is_some() {
            true => ui.label(vb_session::i18n::t("ui-app-prefs-dialog-030")),
            false => ui.colored_label(
                theme::tokens(ui.ctx()).warn,
                vb_session::i18n::t("ui-app-prefs-dialog-031"),
            ),
        };
        ui.add_space(6.0);
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-032"));
    }

    /// ⑧外观:主题 / UI 缩放 / 提示条(全部收编既有项;原菜单入口保留)。
    fn prefs_page_appearance(&mut self, ui: &mut egui::Ui) {
        let mut dark = self.theme_dark;
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-common-theme"));
            if ui
                .selectable_label(dark, vb_session::i18n::t("ui-common-dark"))
                .clicked()
            {
                dark = true;
            }
            if ui
                .selectable_label(!dark, vb_session::i18n::t("ui-common-light"))
                .clicked()
            {
                dark = false;
            }
        });
        if dark != self.theme_dark {
            self.set_theme_dark(dark);
            // 阶段 2(02-3-5):主题广播,主页与所有窗口跟随
            if let Some(tx) = &self.shell_tx {
                let _ = tx.send(crate::shell::ShellRequest::ThemeChanged(dark));
            }
        }
        let mut scale = self.ui_scale;
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-common-ui-scale"));
            ui.add(
                egui::DragValue::new(&mut scale)
                    .speed(0.01)
                    .range(0.5..=3.0)
                    .suffix("×"),
            );
            ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-033"));
        });
        if (scale - self.ui_scale).abs() > f32::EPSILON && scale.is_finite() {
            self.ui_scale = scale;
        }
        let mut hints = self.hints;
        if ui
            .checkbox(&mut hints, vb_session::i18n::t("ui-app-prefs-dialog-034"))
            .changed()
        {
            self.hints = hints;
        }
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-035"));
    }

    /// ⑨数据:自动保存间隔(档位)+ 快照保留数(收编 07-A 既有项)。
    fn prefs_page_data(&mut self, ui: &mut egui::Ui) {
        let mut interval = self.autosave_interval_secs;
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-app-prefs-dialog-036"));
            let steps = crate::autosave::INTERVAL_STEPS;
            let pos = steps
                .iter()
                .position(|&s| s == interval)
                .unwrap_or_else(|| {
                    steps
                        .iter()
                        .position(|&s| s >= crate::autosave::DEFAULT_INTERVAL_SECS)
                        .unwrap_or(2)
                });
            for (i, &step) in steps.iter().enumerate() {
                let txt = if step == 0 {
                    vb_session::i18n::t("ui-common-off").to_string()
                } else {
                    format!("{step}s")
                };
                if ui.selectable_label(pos == i, txt).clicked() {
                    interval = step;
                }
            }
        });
        if interval != self.autosave_interval_secs {
            self.autosave_interval_secs = interval;
            self.say(match interval {
                0 => vb_session::i18n::t("ui-app-prefs-dialog-037"),
                s => vb_session::i18n::t_args(
                    "ui-app-prefs-dialog-038",
                    &[("s", vb_session::i18n::FluentValue::from((s).to_string()))],
                ),
            });
        }
        let mut keep = self.autosave_keep;
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-app-prefs-dialog-039"));
            ui.add(
                egui::DragValue::new(&mut keep)
                    .range(1..=3)
                    .suffix(vb_session::i18n::t("ui-app-prefs-dialog-040")),
            );
            ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-041"));
        });
        if keep != self.autosave_keep {
            self.autosave_keep = keep;
        }
        ui.add_space(6.0);
        ui.weak(vb_session::i18n::t("ui-app-prefs-dialog-042"));
    }

    /// 「恢复默认」:九类全部回默认值(即时生效;落盘走脏检查)。
    ///
    /// 只动**首选项**(主题/网格/文本默认/画板预设/数据),不动布局
    /// (停靠位/面板坞归「工作区」,见窗口菜单)。
    pub(crate) fn prefs_restore_defaults(&mut self) {
        let d = super::dock_layout::WorkspaceConfig::default();
        self.show_all_tools = d.show_all_tools;
        self.pencil_fidelity = d.pencil_fidelity;
        self.text_default = vb_doc::model::SegStyle {
            font_size: Some(d.text_default_size),
            color: Some(d.text_default_color.clone()),
            font_family: None,
            ..vb_doc::model::SegStyle::default()
        };
        self.rulers_on = d.rulers_default;
        self.grid_on = d.grid_default;
        self.guides_visible = d.guides_default;
        self.smart_guides_on = d.smart_guides_default;
        self.smart_guides.clear();
        self.grid_size = d.grid_size;
        self.artboard_preset = d.artboard_preset;
        self.set_theme_dark(d.theme_dark);
        if let Some(tx) = &self.shell_tx {
            let _ = tx.send(crate::shell::ShellRequest::ThemeChanged(d.theme_dark));
        }
        self.ui_scale = d.ui_scale;
        self.hints = d.hints;
        // §8.3.5:密度回 comfortable
        self.density_compact = d.density_compact;
        self.autosave_interval_secs = d.autosave_interval_secs;
        self.autosave_keep = d.autosave_keep;
        // 05-7:界面语言回默认(中文)
        crate::i18n::set_lang(Lang::from_code(&d.ui_lang));
        self.say(vb_session::i18n::t("ui-app-prefs-dialog-043"));
    }
}

#[cfg(test)]
mod tests {
    use crate::app::assemble::tests::app_fresh;
    use crate::app::dock_layout::WorkspaceConfig;

    /// 09-L 门禁:恢复默认 —— 九类字段全部回默认值且与会话状态一致。
    #[test]
    fn prefs_restore_defaults_resets_all_nine_categories() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        // 逐类改走默认值
        app.show_all_tools = true;
        app.pencil_fidelity = 12.0;
        app.text_default.font_size = Some(48.0);
        app.text_default.font_family = Some("宋体".into());
        app.rulers_on = false;
        app.grid_on = false;
        app.guides_visible = false;
        app.smart_guides_on = false;
        app.grid_size = 128.0;
        app.artboard_preset = 5;
        app.theme_dark = false;
        app.ui_scale = 1.5;
        app.hints = false;
        app.autosave_interval_secs = 0;
        app.autosave_keep = 1;

        app.prefs_restore_defaults();

        let d = WorkspaceConfig::default();
        assert_eq!(app.show_all_tools, d.show_all_tools, "①常规");
        assert_eq!(app.pencil_fidelity, d.pencil_fidelity, "①常规");
        assert_eq!(
            app.text_default.font_size,
            Some(d.text_default_size),
            "②文字"
        );
        assert_eq!(app.text_default.font_family, None, "②文字");
        assert_eq!(app.rulers_on, d.rulers_default, "③单位与标尺");
        assert_eq!(app.grid_on, d.grid_default, "④参考线与网格");
        assert_eq!(app.guides_visible, d.guides_default, "④参考线与网格");
        assert_eq!(app.smart_guides_on, d.smart_guides_default, "⑤智能参考线");
        assert_eq!(app.grid_size, d.grid_size, "④参考线与网格");
        assert_eq!(app.artboard_preset, d.artboard_preset, "⑥画板");
        assert_eq!(app.theme_dark, d.theme_dark, "⑧外观");
        assert_eq!(app.ui_scale, d.ui_scale, "⑧外观");
        assert_eq!(app.hints, d.hints, "⑧外观");
        assert_eq!(
            app.autosave_interval_secs, d.autosave_interval_secs,
            "⑨数据"
        );
        assert_eq!(app.autosave_keep, d.autosave_keep, "⑨数据");
    }

    /// 09-L 门禁:工作区配置与会话状态的双向一致 ——
    /// `workspace_config` 必须把九类会话值投影进持久化结构
    /// (改会话 → 配置变化 → `workspace_dirty` 翻真 → 自动落盘链路)。
    #[test]
    fn prefs_session_projects_into_workspace_config() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        assert!(!app.workspace_dirty(), "初始不脏");
        app.grid_size = 96.0;
        app.rulers_on = false;
        app.autosave_keep = 2;
        app.artboard_preset = 3;
        app.theme_dark = false;
        assert!(app.workspace_dirty(), "改首选项必须置脏(下一帧自动落盘)");
        let cfg = app.workspace_config();
        assert_eq!(cfg.grid_size, 96.0);
        assert!(!cfg.rulers_default);
        assert_eq!(cfg.autosave_keep, 2);
        assert_eq!(cfg.artboard_preset, 3);
        assert!(!cfg.theme_dark);
        // 文本默认样式投影(字号/颜色/字体)
        app.text_default.font_size = Some(32.0);
        let cfg = app.workspace_config();
        assert_eq!(cfg.text_default_size, 32.0);
    }

    /// 05-7 门禁:界面语言进工作区配置(切 En → 置脏 + 投影 "en"),
    /// 恢复默认回中文。全局语言态经 ENV_LOCK 串行化,结束还原中文。
    #[test]
    fn prefs_language_persists_and_restores() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        crate::i18n::set_lang(crate::i18n::Lang::En);
        assert!(app.workspace_dirty(), "切语言必须置脏(下一帧落盘)");
        assert_eq!(app.workspace_config().ui_lang, "en");
        app.prefs_restore_defaults();
        assert_eq!(crate::i18n::lang(), crate::i18n::Lang::Zh, "恢复默认回中文");
    }
}
