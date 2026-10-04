//! 插件宿主 UI(09-J,05-10-3/4/6):插件管理窗口、「插件」次级坞面板、
//! 首次启用授权弹窗与逐帧 poll。
//!
//! 职责切分:插件注册表 / 权限闸门 / 状态机 / 子进程管理在 `vb_plugin`
//! crate(可独立门禁测试);本模块只做三件事:
//! 1. 实现 [`HostServices`] 端口 —— 把 `runCommand` / 文档只读投影 /
//!    导出动作接到 `VellumApp`(05-10-4);
//! 2. 每帧 [`VellumApp::plugin_poll`](Self::plugin_poll) —— 泵插件请求
//!    与崩溃检测(**mem::take 防双借**:插件回调执行命令时宿主不在借用中);
//! 3. 渲染:管理窗口(状态/版本/启用开关/授权/日志/重启)+ 插件坞面板
//!    (受控 UI 描述)+ 授权弹窗(列 manifest 全部权限,确认才启用)。

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use vb_plugin::host::{HostServices, PluginInfo, PluginState, Widget, LOG_CAP};
// PLG-09:文件对话框经 vb_platform trait(方法解析需要 trait 在作用域)
use vb_platform::FileDialog as _;

use super::panel_dock;
use super::VellumApp;

/// 授权弹窗编辑态(`VellumApp.plugin_auth`;Some = 弹窗开着)。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PluginAuthState {
    pub plugin_id: String,
    /// PLG-01:用户已阅读「原生进程」告知并显式勾选(未勾选 = 启用按钮
    /// 不可点;勾选凭证经 `NativeProcessConsent` 传给 vb_plugin 既有告知
    /// 合同 `authorize_with_consent`,None/未勾选在宿主侧二次拒绝)。
    pub consent_checked: bool,
}

// ─────────────────────────── HostServices 端口 ───────────────────────────

/// `vb_plugin` 的宿主服务端口实现(借 &mut VellumApp;只在 poll/导出的
/// 短暂窗口内存在,宿主已被 mem::take 取出,无双借)。
pub(crate) struct PluginServices<'a>(pub(crate) &'a mut VellumApp);

impl HostServices for PluginServices<'_> {
    /// 白名单内的宿主命令执行(可撤销、与 UI 同一命令路径)。
    fn run_command(&mut self, plugin: &str, command: &str) -> Result<Value, String> {
        // 防御:插件管理入口不得被插件调用(防递归 / 防插件自我授权)
        if matches!(command, "edit.plugins" | "view.toggle_plugins_panel") {
            return Err(vb_session::i18n::t("ui-app-plugins-001"));
        }
        if !crate::shortcuts::is_implemented(command) {
            return Err(vb_session::i18n::t_args(
                "ui-app-plugins-002",
                &[(
                    "command",
                    vb_session::i18n::FluentValue::from((command).to_string()),
                )],
            ));
        }
        self.0.run_command(command, false, false);
        Ok(json!({"ok": true, "command": command, "via": plugin}))
    }

    /// 文档只读投影(结构摘要;**只读,不发写捷径**)。
    ///
    /// PLG-05 起为降级路径;常规路径经 [`Self::doc_snapshot`] + 后台线程。
    fn doc_projection(&self) -> Result<Value, String> {
        Ok(vb_plugin::projection::build(&self.0.doc))
    }

    /// PLG-05:UI 线程只付 arena 克隆(独立快照,不与 UI 状态共享可变
    /// 内部);投影 JSON 构建由 vb_plugin 投影线程完成,结果下一帧 poll 收。
    fn doc_snapshot(&self) -> Option<vb_doc::model::Document> {
        Some(self.0.doc.clone())
    }

    /// 导出动作(05-10-4 ④):宿主执行,输出只落**用户选择的目录**。
    fn run_export(
        &mut self,
        plugin: &str,
        export_id: &str,
        format: &str,
        dir: &Path,
    ) -> Result<Value, String> {
        let app = &mut *self.0;
        let Some(ab) = app.active_artboard() else {
            return Err(vb_session::i18n::t("ui-app-plugins-003"));
        };
        let name = app.active_artboard_name();
        let stem = sanitize_file_stem(&format!("{plugin}-{name}-2x"));
        match format {
            "png" => {
                let (png, warnings) = vb_export::export_artboard_png(
                    &app.doc,
                    ab,
                    2.0,
                    false,
                    app.project_dir.as_deref(),
                )?;
                let file = dir.join(format!("{stem}.png"));
                std::fs::write(&file, &png).map_err(|e| {
                    vb_session::i18n::t_args(
                        "ui-app-plugins-004",
                        &[
                            (
                                "a1",
                                vb_session::i18n::FluentValue::from((file.display()).to_string()),
                            ),
                            ("e", vb_session::i18n::FluentValue::from((e).to_string())),
                        ],
                    )
                })?;
                Ok(json!({
                    "ok": true, "export": export_id, "format": "png",
                    "out": file.display().to_string(),
                    "bytes": png.len(), "warnings": warnings.len(),
                }))
            }
            "svg" => {
                let svg = vb_export::export_artboard_svg(
                    &app.doc,
                    ab,
                    2,
                    false,
                    app.project_dir.as_deref(),
                )?;
                let file = dir.join(format!("{stem}.svg"));
                std::fs::write(&file, &svg).map_err(|e| {
                    vb_session::i18n::t_args(
                        "ui-app-plugins-004",
                        &[
                            (
                                "a1",
                                vb_session::i18n::FluentValue::from((file.display()).to_string()),
                            ),
                            ("e", vb_session::i18n::FluentValue::from((e).to_string())),
                        ],
                    )
                })?;
                Ok(json!({
                    "ok": true, "export": export_id, "format": "svg",
                    "out": file.display().to_string(),
                    "bytes": svg.len(),
                }))
            }
            other => Err(vb_session::i18n::t_args(
                "ui-app-plugins-005",
                &[(
                    "other",
                    vb_session::i18n::FluentValue::from((other).to_string()),
                )],
            )),
        }
    }
}

/// 文件名净化(画板名可能含 Windows 非法字符;非法字符换下划线)。
fn sanitize_file_stem(s: &str) -> String {
    let bad = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
    let cleaned: String = s
        .chars()
        .map(|c| if bad.contains(&c) { '_' } else { c })
        .collect();
    cleaned.trim().to_string()
}

// ─────────────────────────── 每帧 poll ───────────────────────────

impl VellumApp {
    /// 每帧泵插件事件(请求 / 通知 / 崩溃检测)。**零插件时零开销**。
    pub(crate) fn plugin_poll(&mut self) {
        // mem::take:插件请求的 runCommand 会执行宿主命令(要 &mut self),
        // 宿主本体必须先离开 self,否则双借。
        let mut host = std::mem::take(&mut self.plugin_host);
        {
            let mut services = PluginServices(&mut *self);
            host.poll(&mut services);
        }
        self.plugin_host = host;
    }

    /// 执行一次「宿主服务 + 插件」的复合操作(run_export 用;UI 线程)。
    fn with_host_services<R>(
        &mut self,
        f: impl FnOnce(&mut vb_plugin::host::PluginHost, &mut PluginServices<'_>) -> R,
    ) -> R {
        let mut host = std::mem::take(&mut self.plugin_host);
        let r = {
            let mut services = PluginServices(&mut *self);
            f(&mut host, &mut services)
        };
        self.plugin_host = host;
        r
    }
}

// ─────────────────────────── 插件管理窗口 ───────────────────────────

impl VellumApp {
    /// 「编辑 → 插件管理…」窗口(05-10-3/5:列表 + 启用开关 + 授权 +
    /// 日志 + 重启 + 安装/卸载)。
    pub(crate) fn show_plugins_window(&mut self, ui: &mut egui::Ui) {
        if !self.plugins_mgr_open {
            return;
        }
        // 状态/日志色一律走主题令牌(theme.rs 是全仓唯一颜色字面量文件)
        let c_error = vb_ui::theme::Tokens::get(self.theme_dark).danger;
        let mut open = true;
        egui::Window::new(vb_session::i18n::t("ui-app-plugins-006"))
            .open(&mut open)
            .collapsible(false)
            .default_size([660.0, 460.0])
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(vb_session::i18n::t("ui-app-plugins-007"));
                    ui.weak(vb_session::i18n::t("ui-app-plugins-008"));
                });
                ui.horizontal(|ui| {
                    if ui
                        .button(vb_session::i18n::t("ui-app-plugins-009"))
                        .clicked()
                    {
                        // PLG-09:文件对话框经 vb_platform trait(面板不再
                        // 各自 import rfd;与 launcher 同一 seam)
                        let mut dialog = vb_platform::egui_backend::RfdDialog::new();
                        if let Some(file) = dialog.pick_file(
                            &vb_session::i18n::t("ui-app-plugins-010"),
                            &[vb_platform::FileFilter {
                                name: "VellumBench 插件清单", // vb-literal-ok: FileFilter.name 为 &str 平台接缝类型,fn 化留后续
                                extensions: &["json"],
                            }],
                        ) {
                            let dir = file
                                .parent()
                                .map(Path::to_path_buf)
                                .unwrap_or_else(|| PathBuf::from("."));
                            match self
                                .plugin_host
                                .install_dir(&dir, &crate::shortcuts::is_implemented)
                            {
                                Ok(id) => {
                                    self.toast_warn(vb_session::i18n::t_args(
                                        "ui-app-plugins-012",
                                        &[(
                                            "id",
                                            vb_session::i18n::FluentValue::from((id).to_string()),
                                        )],
                                    ));
                                }
                                Err(e) => self.toast_error(vb_session::i18n::t_args(
                                    "ui-app-plugins-013",
                                    &[("e", vb_session::i18n::FluentValue::from((e).to_string()))],
                                )),
                            }
                        }
                    }
                    if ui
                        .button(vb_session::i18n::t("ui-app-plugins-014"))
                        .clicked()
                    {
                        self.plugin_host
                            .reload_installed(&crate::shortcuts::is_implemented);
                        self.say(vb_session::i18n::t("ui-app-plugins-015"));
                    }
                });
                // 装载失败条目(不阻断其他插件)
                let errors: Vec<(String, String)> = self
                    .plugin_host
                    .install_errors()
                    .iter()
                    .map(|e| (e.dir.display().to_string(), e.error.clone()))
                    .collect();
                for (dir, err) in &errors {
                    ui.colored_label(
                        c_error,
                        vb_session::i18n::t_args(
                            "ui-app-plugins-016",
                            &[
                                (
                                    "dir",
                                    vb_session::i18n::FluentValue::from((dir).to_string()),
                                ),
                                (
                                    "err",
                                    vb_session::i18n::FluentValue::from((err).to_string()),
                                ),
                            ],
                        ),
                    );
                }
                ui.separator();
                // 插件行(快照先行,渲染中可变更结构)
                let infos = self.plugin_host.list();
                if infos.is_empty() {
                    ui.label(vb_session::i18n::t("ui-app-plugins-017"));
                }
                for info in infos {
                    self.plugin_manager_row(ui, &info);
                    ui.separator();
                }
                ui.weak(vb_session::i18n::t_args(
                    "ui-app-plugins-018",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            (self
                                .plugin_host
                                .auth_path()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|| vb_session::i18n::t("ui-app-plugins-019")))
                            .to_string(),
                        ),
                    )],
                ));
            });
        self.plugins_mgr_open = open;
    }

    /// 单个插件行:状态徽标 + 启用开关 + 重启/日志/卸载。
    fn plugin_manager_row(&mut self, ui: &mut egui::Ui, info: &PluginInfo) {
        // 状态/日志色一律走主题令牌(theme.rs 是全仓唯一颜色字面量文件)
        let t = vb_ui::theme::Tokens::get(self.theme_dark);
        let (c_error, c_ok, c_warn, c_muted) = (t.danger, t.success, t.warn, t.text_3);
        ui.horizontal(|ui| {
            // 启用开关:勾选 = 启动(未授权 → 先弹授权窗);取消 = 停止
            let enabled = matches!(info.state, PluginState::Starting | PluginState::Running);
            let mut check = enabled;
            if ui
                .checkbox(&mut check, format!("{}({})", info.name, info.id))
                .changed()
            {
                let id = info.id.clone();
                if check && !enabled {
                    if info.authorized {
                        match self.plugin_host.start(&id) {
                            Ok(()) => self.say(vb_session::i18n::t_args(
                                "ui-app-plugins-020",
                                &[("id", vb_session::i18n::FluentValue::from((id).to_string()))],
                            )),
                            Err(e) => self.toast_error(vb_session::i18n::t_args(
                                "ui-app-plugins-021",
                                &[("e", vb_session::i18n::FluentValue::from((e).to_string()))],
                            )),
                        }
                    } else {
                        // 首次启用授权弹窗(05-10-3:拒绝 = 不启用)
                        self.plugin_auth = Some(PluginAuthState {
                            plugin_id: id,
                            consent_checked: false,
                        });
                    }
                } else if !check && enabled {
                    self.plugin_host.stop(&id);
                    self.say(vb_session::i18n::t_args(
                        "ui-app-plugins-022",
                        &[("id", vb_session::i18n::FluentValue::from((id).to_string()))],
                    ));
                }
            }
            // 状态徽标
            let badge = match info.state {
                PluginState::Running => c_ok,
                PluginState::Starting => c_warn,
                PluginState::Crashed => c_error,
                PluginState::Unauthorized => c_warn,
                PluginState::Stopped => c_muted,
            };
            ui.colored_label(badge, format!("[{}]", info.state.label()));
            ui.weak(format!("v{}", info.version));
            // 重启(崩溃后 / 运行中)
            if matches!(info.state, PluginState::Crashed | PluginState::Running)
                && ui
                    .button(vb_session::i18n::t("ui-common-restart"))
                    .clicked()
            {
                let id = info.id.clone();
                match self.plugin_host.restart(&id) {
                    Ok(()) => self.say(vb_session::i18n::t_args(
                        "ui-app-plugins-023",
                        &[("id", vb_session::i18n::FluentValue::from((id).to_string()))],
                    )),
                    Err(e) => self.toast_error(vb_session::i18n::t_args(
                        "ui-app-plugins-024",
                        &[("e", vb_session::i18n::FluentValue::from((e).to_string()))],
                    )),
                }
            }
            // 日志环展开
            let log_id = info.id.clone();
            let mut expanded = self.plugin_logs_open.contains(&log_id);
            if ui
                .toggle_value(&mut expanded, vb_session::i18n::t("ui-common-log"))
                .changed()
            {
                if expanded {
                    self.plugin_logs_open.insert(log_id);
                } else {
                    self.plugin_logs_open.remove(&log_id);
                }
            }
            // 卸载(停进程 + 清授权登记)
            if ui
                .button(vb_session::i18n::t("ui-common-uninstall"))
                .clicked()
            {
                let dir = info.dir.clone();
                self.plugin_host.uninstall(&dir);
                self.say(vb_session::i18n::t_args(
                    "ui-app-plugins-025",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from((info.id).to_string()),
                    )],
                ));
            }
        });
        ui.horizontal(|ui| {
            ui.weak(info.dir.display().to_string());
            if !info.authorized {
                ui.colored_label(c_warn, vb_session::i18n::t("ui-app-plugins-026"));
            }
        });
        if let Some(note) = (!info.note.is_empty()).then(|| info.note.clone()) {
            ui.colored_label(c_error, note);
        }
        // 权限清单摘要(授权闸门的可见性)
        if !info.manifest.commands.is_empty() {
            ui.weak(vb_session::i18n::t_args(
                "ui-app-plugins-027",
                &[(
                    "a1",
                    vb_session::i18n::FluentValue::from(
                        (info.manifest.commands.join(", ")).to_string(),
                    ),
                )],
            ));
        } else {
            ui.weak(vb_session::i18n::t("ui-app-plugins-028"));
        }
        // 日志环(最近 N 条,时序)
        if self.plugin_logs_open.contains(&info.id) {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.weak(vb_session::i18n::t_args(
                    "ui-app-plugins-029",
                    &[(
                        "LOG_CAP",
                        vb_session::i18n::FluentValue::from((LOG_CAP).to_string()),
                    )],
                ));
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for l in self.plugin_host.logs(&info.id) {
                            let color = match l.level.as_str() {
                                "error" => c_error,
                                "warn" => c_warn,
                                _ => c_muted,
                            };
                            ui.colored_label(color, format!("[{}] {}", l.level, l.text));
                        }
                    });
            });
        }
    }
}

// ─────────────────────────── 授权弹窗(05-10-3)───────────────────────────

impl VellumApp {
    /// 首次启用授权弹窗:列出 manifest 声明的**全部**权限;
    /// 确认 → 授权持久化 + 启动;取消 → 不启用(状态保持未授权)。
    pub(crate) fn show_plugin_auth_window(&mut self, ui: &mut egui::Ui) {
        let Some(mut auth) = self.plugin_auth.clone() else {
            return;
        };
        let Some(info) = self
            .plugin_host
            .list()
            .into_iter()
            .find(|p| p.id == auth.plugin_id)
        else {
            self.plugin_auth = None;
            return;
        };
        // 状态/告警色一律走主题令牌(theme.rs 是全仓唯一颜色字面量文件)
        let c_error = vb_ui::theme::Tokens::get(self.theme_dark).danger;
        let mut open = true;
        egui::Window::new(vb_session::i18n::t_args(
            "ui-app-plugins-030",
            &[(
                "a1",
                vb_session::i18n::FluentValue::from((info.name).to_string()),
            )],
        ))
        .open(&mut open)
        .collapsible(false)
        .default_width(480.0)
        .show(ui.ctx(), |ui| {
            ui.label(format!("{} v{}(id:{})", info.name, info.version, info.id));
            ui.weak(info.dir.display().to_string());
            ui.separator();
            ui.strong(vb_session::i18n::t("ui-app-plugins-031"));
            if info.manifest.commands.is_empty() {
                ui.label(vb_session::i18n::t("ui-app-plugins-032"));
            } else {
                for c in &info.manifest.commands {
                    let label = crate::shortcuts::CMD_LABELS
                        .iter()
                        .find(|(id, _)| id == c)
                        .map(|(_, l)| *l)
                        .unwrap_or(c);
                    ui.label(vb_session::i18n::t_args(
                        "ui-app-plugins-033",
                        &[
                            ("c", vb_session::i18n::FluentValue::from((c).to_string())),
                            (
                                "label",
                                vb_session::i18n::FluentValue::from((label).to_string()),
                            ),
                        ],
                    ));
                }
            }
            for p in &info.manifest.panels {
                ui.label(vb_session::i18n::t_args(
                    "ui-app-plugins-034",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from((p.title).to_string()),
                    )],
                ));
            }
            for e in &info.manifest.exports {
                ui.label(vb_session::i18n::t_args(
                    "ui-app-plugins-035",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from((e.title).to_string()),
                    )],
                ));
            }
            ui.separator();
            // PLG-01(P0):原生进程权限告知 —— 插件是普通用户权限进程,
            // 白名单只约束诚实插件;安装/首次启用必须让用户知情并显式接受。
            ui.colored_label(c_error, vb_session::i18n::t("ui-app-plugins-036"));
            ui.add(
                egui::Label::new(
                    egui::RichText::new(vb_plugin::NATIVE_PROCESS_DISCLOSURE).strong(),
                )
                .wrap(),
            );
            ui.separator();
            ui.weak(vb_session::i18n::t("ui-app-plugins-037"));
            ui.weak(vb_session::i18n::t("ui-app-plugins-038"));
            ui.weak(vb_session::i18n::t("ui-app-plugins-039"));
            ui.separator();
            // 强制勾选:未勾选时「启用(授权)」不可点(凭证在此产生)
            let mut consent = auth.consent_checked;
            ui.checkbox(&mut consent, vb_session::i18n::t("ui-app-plugins-040"));
            auth.consent_checked = consent;
            let consent_ok =
                vb_plugin::NativeProcessConsent::from_dialog_checkbox(auth.consent_checked);
            ui.horizontal(|ui| {
                let enable = ui.add_enabled(
                    consent_ok.is_some(),
                    egui::Button::new(vb_session::i18n::t("ui-app-plugins-041")),
                );
                if enable.clicked() {
                    let id = info.id.clone();
                    // 勾选凭证进入 vb_plugin 既有告知合同(未勾选在
                    // 宿主侧同样拒绝 —— 双重闸门,防 UI 侧漏检)
                    match self.plugin_host.authorize_with_consent(&id, consent_ok) {
                        Ok(()) => match self.plugin_host.start(&id) {
                            Ok(()) => {
                                self.say(vb_session::i18n::t_args(
                                    "ui-app-plugins-042",
                                    &[(
                                        "id",
                                        vb_session::i18n::FluentValue::from((id).to_string()),
                                    )],
                                ));
                                // 有面板 → 顺手打开插件坞面板(可见反馈)
                                if !info.manifest.panels.is_empty() {
                                    self.plugins_panel_open = true;
                                    self.sec_focus(panel_dock::SecPanel::Plugins);
                                }
                            }
                            Err(e) => self.toast_error(vb_session::i18n::t_args(
                                "ui-app-plugins-043",
                                &[("e", vb_session::i18n::FluentValue::from((e).to_string()))],
                            )),
                        },
                        Err(e) => self.toast_error(vb_session::i18n::t_args(
                            "ui-app-plugins-044",
                            &[("e", vb_session::i18n::FluentValue::from((e).to_string()))],
                        )),
                    }
                    self.plugin_auth = None;
                }
                if ui.button(vb_session::i18n::t("ui-common-cancel")).clicked() {
                    // 拒绝 = 不启用(状态保持未授权)
                    self.plugin_host.deny(&info.id);
                    self.say(vb_session::i18n::t_args(
                        "ui-app-plugins-045",
                        &[(
                            "a1",
                            vb_session::i18n::FluentValue::from((info.id).to_string()),
                        )],
                    ));
                    self.plugin_auth = None;
                }
            });
        });
        // PLG-01:勾选态回写(auth 是本帧克隆;闭包借用已随 show 结束)
        if let Some(pa) = &mut self.plugin_auth {
            pa.consent_checked = auth.consent_checked;
        }
        // 点窗体外/× 关闭 = 取消(拒绝;按钮路径已清 plugin_auth,不会重入)
        if !open && self.plugin_auth.is_some() {
            self.plugin_host.deny(&auth.plugin_id);
            self.plugin_auth = None;
        }
    }
}

// ─────────────────────────── 插件坞面板(05-10-4 ③)───────────────────────────

impl VellumApp {
    /// 「插件」次级坞面板:Running 插件注册的受控 UI(有限元件),
    /// 按钮点击 = 向插件发通知(禁任意代码);导出按钮 = 用户选目录后
    /// 由宿主执行导出。
    pub(crate) fn plugins_panel_body(&mut self, ui: &mut egui::Ui) {
        let running = self.plugin_host.running_with_panels();
        if running.is_empty() {
            ui.label(vb_session::i18n::t("ui-app-plugins-046"));
            ui.weak(vb_session::i18n::t("ui-app-plugins-047"));
            ui.add_space(8.0);
            if ui
                .button(vb_session::i18n::t("ui-app-plugins-048"))
                .clicked()
            {
                self.run_command("edit.plugins", false, false);
            }
            return;
        }
        // 多插件子 Tab(单个插件时不画选择条)
        let mut sel = self.plugin_panel_sel.min(running.len() - 1);
        if running.len() > 1 {
            let labels: Vec<&str> = running.iter().map(|(_, n, _)| n.as_str()).collect();
            if vb_ui::components::PanelTabs::new(&labels, &mut sel).ui(ui) {
                self.plugin_panel_sel = sel;
            }
        }
        let (id, name, panels) = &running[sel];
        let id = id.clone();
        let name = name.clone();
        let panels = panels.clone();
        ui.weak(vb_session::i18n::t_args(
            "ui-app-plugins-049",
            &[(
                "name",
                vb_session::i18n::FluentValue::from((name).to_string()),
            )],
        ));
        ui.separator();
        // 导出动作(05-10-4 ④):输出只落用户选择的目录
        if let Some(info) = self.plugin_host.list().into_iter().find(|p| p.id == id) {
            for e in &info.manifest.exports {
                ui.horizontal(|ui| {
                    if ui
                        .button(vb_session::i18n::t_args(
                            "ui-app-plugins-050",
                            &[(
                                "a1",
                                vb_session::i18n::FluentValue::from((e.title).to_string()),
                            )],
                        ))
                        .clicked()
                    {
                        // PLG-09:目录选择同样经 vb_platform trait
                        let mut dialog = vb_platform::egui_backend::RfdDialog::new();
                        if let Some(dir) = dialog.pick_folder() {
                            let res = self.with_host_services(|host, services| {
                                host.run_export(services, &id, &e.id, Some(&dir))
                            });
                            match res {
                                Ok(v) => self.say(vb_session::i18n::t_args(
                                    "ui-app-plugins-051",
                                    &[(
                                        "a1",
                                        vb_session::i18n::FluentValue::from(
                                            (v.get("out").and_then(|x| x.as_str()).unwrap_or(""))
                                                .to_string(),
                                        ),
                                    )],
                                )),
                                Err(err) => self.toast_error(vb_session::i18n::t_args(
                                    "ui-app-plugins-052",
                                    &[(
                                        "err",
                                        vb_session::i18n::FluentValue::from((err).to_string()),
                                    )],
                                )),
                            }
                        } else {
                            self.toast_warn(vb_session::i18n::t("ui-app-plugins-053"));
                        }
                    }
                });
            }
            if !info.manifest.exports.is_empty() {
                ui.separator();
            }
        }
        // 各面板(受控 UI 描述)
        for panel in &panels {
            ui.strong(&panel.title);
            match self.plugin_host.panel_ui(&id, &panel.id) {
                None => {
                    ui.weak(vb_session::i18n::t("ui-app-plugins-054"));
                }
                Some(desc) => {
                    self.render_plugin_widgets(ui, &id, &panel.id, &desc.widgets);
                }
            }
            ui.add_space(6.0);
        }
    }

    /// 受控元件渲染(text/metric/button/input;未知元件已在宿主侧过滤)。
    fn render_plugin_widgets(
        &mut self,
        ui: &mut egui::Ui,
        plugin_id: &str,
        panel_id: &str,
        widgets: &[Widget],
    ) {
        for w in widgets {
            match w {
                Widget::Text { text } => {
                    // ui.add(Label) 才吃 wrap;ui.label 只收 WidgetText
                    ui.add(egui::Label::new(text.as_str()).wrap());
                }
                Widget::Metric { label, value } => {
                    ui.horizontal(|ui| {
                        ui.weak(format!("{label}:"));
                        ui.strong(value);
                    });
                }
                Widget::Button { action, label } => {
                    let (pid, plid, act) =
                        (plugin_id.to_string(), panel_id.to_string(), action.clone());
                    if ui.button(label.as_str()).clicked() {
                        // 按钮点击 = 向插件发通知(宿主不解释 action 语义)
                        match self.plugin_host.send_button(&pid, &plid, &act) {
                            Ok(()) => {}
                            Err(e) => self.toast_error(e),
                        }
                    }
                }
                Widget::Input {
                    id,
                    placeholder,
                    value,
                } => {
                    let key = format!("{plugin_id}/{panel_id}/{id}");
                    let mut text = self
                        .plugin_input_buf
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| value.clone());
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .hint_text(placeholder.as_str())
                            .desired_width(f32::INFINITY),
                    );
                    if resp.changed() {
                        self.plugin_input_buf.insert(key.clone(), text.clone());
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let (pid, plid, iid) =
                            (plugin_id.to_string(), panel_id.to_string(), id.clone());
                        match self.plugin_host.send_input(&pid, &plid, &iid, &text) {
                            Ok(()) => self.say(vb_session::i18n::t_args(
                                "ui-app-plugins-055",
                                &[(
                                    "pid",
                                    vb_session::i18n::FluentValue::from((pid).to_string()),
                                )],
                            )),
                            Err(e) => self.toast_error(e),
                        }
                    }
                }
            }
        }
    }
}

// ─────────────────────────── 装载与冒烟夹具 ───────────────────────────

impl VellumApp {
    /// 启动时装载已登记插件 + 告警(construct 末尾调用)。
    pub(crate) fn plugin_load_installed(&mut self) {
        self.plugin_host
            .reload_installed(&crate::shortcuts::is_implemented);
        for e in self.plugin_host.install_errors() {
            log::warn!("插件装载失败({}):{}", e.dir.display(), e.error);
        }
    }

    /// 09-J 冒烟夹具(不进 UI 面,与 VB_PROOFREAD_AUTOSTART 同族):
    /// - `VB_PLUGIN_LOAD=<目录>[;<目录>…]` 装载指定插件目录(不自动启用);
    /// - `VB_PLUGIN_SMOKE=1` 装载仓库自带 `plugins/example-stats`(从仓库
    ///   根启动时按相对路径找)并**自动授权 + 启动 + 打开管理窗与插件
    ///   面板**(自动授权仅限本夹具;正常 UI 流程必须人工确认)。
    pub(crate) fn plugin_smoke_fixture(&mut self) {
        if let Ok(load) = std::env::var("VB_PLUGIN_LOAD") {
            for d in load.split(';').map(str::trim).filter(|s| !s.is_empty()) {
                let dir = PathBuf::from(d);
                match self
                    .plugin_host
                    .install_dir(&dir, &crate::shortcuts::is_implemented)
                {
                    Ok(id) => {
                        self.say(vb_session::i18n::t_args(
                            "ui-app-plugins-056",
                            &[("id", vb_session::i18n::FluentValue::from((id).to_string()))],
                        ));
                        self.plugins_mgr_open = true;
                    }
                    Err(e) => eprintln!("VB_PLUGIN_LOAD:装载失败({d}):{e}"),
                }
            }
        }
        if std::env::var("VB_PLUGIN_SMOKE").is_ok() {
            const EXAMPLE: &str = "example-stats";
            if self.plugin_host.list().iter().all(|p| p.id != EXAMPLE) {
                let cand = PathBuf::from("plugins/example-stats");
                if cand.join("plugin.json").is_file() {
                    match self
                        .plugin_host
                        .install_dir(&cand, &crate::shortcuts::is_implemented)
                    {
                        Ok(_) => log::info!("VB_PLUGIN_SMOKE:已装载仓库示例插件"),
                        Err(e) => eprintln!("VB_PLUGIN_SMOKE:示例装载失败:{e}"),
                    }
                } else {
                    eprintln!("VB_PLUGIN_SMOKE:找不到 plugins/example-stats(请从仓库根启动)");
                }
            }
            let infos = self.plugin_host.list();
            if let Some(info) = infos.iter().find(|p| p.id == EXAMPLE) {
                if !info.authorized {
                    // 冒烟夹具专用:跳过人工授权窗(正常 UI 必须人工确认)
                    if let Err(e) = self.plugin_host.authorize(EXAMPLE) {
                        eprintln!("VB_PLUGIN_SMOKE:授权失败:{e}");
                    }
                }
                let st = info.state;
                if matches!(
                    st,
                    PluginState::Stopped | PluginState::Unauthorized | PluginState::Crashed
                ) {
                    if let Err(e) = self.plugin_host.start(EXAMPLE) {
                        eprintln!("VB_PLUGIN_SMOKE:启动失败:{e}");
                    }
                }
            }
            self.plugins_mgr_open = true;
            self.plugins_panel_open = true;
            self.panels_hidden = false;
            self.sec_focus(panel_dock::SecPanel::Plugins);
            log::info!("VB_PLUGIN_SMOKE:插件管理窗 + 插件面板已打开,示例启动中(冒烟夹具)");
        }
        // 授权弹窗冒烟夹具(与 VB_PREFS_TAB 同族,不进 UI 面):配合
        // VB_PLUGIN_LOAD(装载但未授权),VB_PLUGIN_AUTH_PROMPT=<插件 id>
        // 直接打开该插件的首次启用授权弹窗,供截图取证;正常 UI 流程仍由
        // 启用开关触发,不受本夹具影响。
        if let Ok(pid) = std::env::var("VB_PLUGIN_AUTH_PROMPT") {
            let pid = pid.trim();
            if !pid.is_empty() && self.plugin_host.list().iter().any(|p| p.id == pid) {
                self.plugin_auth = Some(PluginAuthState {
                    plugin_id: pid.to_string(),
                    consent_checked: false,
                });
                self.plugins_mgr_open = true;
            }
        }
    }
}
