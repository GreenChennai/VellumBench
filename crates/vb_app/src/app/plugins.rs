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

use super::panel_dock;
use super::VellumApp;

/// 授权弹窗编辑态(`VellumApp.plugin_auth`;Some = 弹窗开着)。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PluginAuthState {
    pub plugin_id: String,
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
            return Err("插件不得调用插件管理入口(防递归与自我授权)".into());
        }
        if !crate::shortcuts::is_implemented(command) {
            return Err(format!("命令 {command} 不是宿主已注册命令"));
        }
        self.0.run_command(command, false, false);
        Ok(json!({"ok": true, "command": command, "via": plugin}))
    }

    /// 文档只读投影(结构摘要;**只读,不发写捷径**)。
    fn doc_projection(&self) -> Result<Value, String> {
        Ok(vb_plugin::projection::build(&self.0.doc))
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
            return Err("当前文档没有画板,无法导出".into());
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
                std::fs::write(&file, &png)
                    .map_err(|e| format!("写 {} 失败:{e}", file.display()))?;
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
                std::fs::write(&file, &svg)
                    .map_err(|e| format!("写 {} 失败:{e}", file.display()))?;
                Ok(json!({
                    "ok": true, "export": export_id, "format": "svg",
                    "out": file.display().to_string(),
                    "bytes": svg.len(),
                }))
            }
            other => Err(format!("导出格式 {other} 不受宿主支持(仅 png/svg)")),
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
        let mut open = true;
        egui::Window::new("插件管理")
            .open(&mut open)
            .collapsible(false)
            .default_size([660.0, 460.0])
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label("插件 = 外部进程(stdio JSON-RPC);默认零权限,首次启用需授权。");
                });
                ui.horizontal(|ui| {
                    if ui.button("安装…(选择 plugin.json)").clicked() {
                        if let Some(file) = rfd::FileDialog::new()
                            .add_filter("VellumBench 插件清单", &["json"])
                            .pick_file()
                        {
                            let dir = file
                                .parent()
                                .map(Path::to_path_buf)
                                .unwrap_or_else(|| PathBuf::from("."));
                            match self
                                .plugin_host
                                .install_dir(&dir, &crate::shortcuts::is_implemented)
                            {
                                Ok(id) => {
                                    self.toast_warn(format!("插件已安装:{id}(启用前需授权)"));
                                }
                                Err(e) => self.toast_error(format!("安装失败:{e}")),
                            }
                        }
                    }
                    if ui.button("重载已登记插件").clicked() {
                        self.plugin_host
                            .reload_installed(&crate::shortcuts::is_implemented);
                        self.say("插件注册表已重载(运行中的插件会被停止)");
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
                        egui::Color32::from_rgb(230, 120, 120),
                        format!("装载失败 {dir}:{err}"),
                    );
                }
                ui.separator();
                // 插件行(快照先行,渲染中可变更结构)
                let infos = self.plugin_host.list();
                if infos.is_empty() {
                    ui.label("尚未安装任何插件。仓库自带示例:plugins/example-stats(统计元素)。");
                }
                for info in infos {
                    self.plugin_manager_row(ui, &info);
                    ui.separator();
                }
                ui.weak(format!(
                    "授权文件:{}(与 recent.json 同范式;撤销授权即停用)",
                    self.plugin_host
                        .auth_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "(未配置,本会话不持久化)".into())
                ));
            });
        self.plugins_mgr_open = open;
    }

    /// 单个插件行:状态徽标 + 启用开关 + 重启/日志/卸载。
    fn plugin_manager_row(&mut self, ui: &mut egui::Ui, info: &PluginInfo) {
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
                            Ok(()) => self.say(format!("插件 {id}:启动中…")),
                            Err(e) => self.toast_error(format!("启动失败:{e}")),
                        }
                    } else {
                        // 首次启用授权弹窗(05-10-3:拒绝 = 不启用)
                        self.plugin_auth = Some(PluginAuthState { plugin_id: id });
                    }
                } else if !check && enabled {
                    self.plugin_host.stop(&id);
                    self.say(format!("插件 {id}:已停止"));
                }
            }
            // 状态徽标
            let badge = match info.state {
                PluginState::Running => egui::Color32::from_rgb(120, 200, 120),
                PluginState::Starting => egui::Color32::from_rgb(200, 190, 120),
                PluginState::Crashed => egui::Color32::from_rgb(230, 120, 120),
                PluginState::Unauthorized => egui::Color32::from_rgb(200, 160, 120),
                PluginState::Stopped => egui::Color32::GRAY,
            };
            ui.colored_label(badge, format!("[{}]", info.state.label()));
            ui.weak(format!("v{}", info.version));
            // 重启(崩溃后 / 运行中)
            if matches!(info.state, PluginState::Crashed | PluginState::Running)
                && ui.button("重启").clicked()
            {
                let id = info.id.clone();
                match self.plugin_host.restart(&id) {
                    Ok(()) => self.say(format!("插件 {id}:重启中…")),
                    Err(e) => self.toast_error(format!("重启失败:{e}")),
                }
            }
            // 日志环展开
            let log_id = info.id.clone();
            let mut expanded = self.plugin_logs_open.contains(&log_id);
            if ui.toggle_value(&mut expanded, "日志").changed() {
                if expanded {
                    self.plugin_logs_open.insert(log_id);
                } else {
                    self.plugin_logs_open.remove(&log_id);
                }
            }
            // 卸载(停进程 + 清授权登记)
            if ui.button("卸载").clicked() {
                let dir = info.dir.clone();
                self.plugin_host.uninstall(&dir);
                self.say(format!("插件 {} 已卸载", info.id));
            }
        });
        ui.horizontal(|ui| {
            ui.weak(info.dir.display().to_string());
            if !info.authorized {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 160, 120),
                    "未授权(启用时需在弹窗确认 manifest 权限)",
                );
            }
        });
        if let Some(note) = (!info.note.is_empty()).then(|| info.note.clone()) {
            ui.colored_label(egui::Color32::from_rgb(230, 120, 120), note);
        }
        // 权限清单摘要(授权闸门的可见性)
        if !info.manifest.commands.is_empty() {
            ui.weak(format!("命令权限:{}", info.manifest.commands.join(", ")));
        } else {
            ui.weak("命令权限:无(零权限)");
        }
        // 日志环(最近 N 条,时序)
        if self.plugin_logs_open.contains(&info.id) {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.weak(format!("日志(最近 {LOG_CAP} 条,新在下):"));
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for l in self.plugin_host.logs(&info.id) {
                            let color = match l.level.as_str() {
                                "error" => egui::Color32::from_rgb(230, 120, 120),
                                "warn" => egui::Color32::from_rgb(220, 180, 110),
                                _ => egui::Color32::PLACEHOLDER,
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
        let Some(auth) = self.plugin_auth.clone() else {
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
        let mut open = true;
        egui::Window::new(format!("启用插件「{}」前请授权", info.name))
            .open(&mut open)
            .collapsible(false)
            .default_width(480.0)
            .show(ui.ctx(), |ui| {
                ui.label(format!("{} v{}(id:{})", info.name, info.version, info.id));
                ui.weak(info.dir.display().to_string());
                ui.separator();
                ui.strong("该插件声明的全部权限:");
                if info.manifest.commands.is_empty() {
                    ui.label("· 宿主命令:无(零权限插件)");
                } else {
                    for c in &info.manifest.commands {
                        let label = crate::shortcuts::CMD_LABELS
                            .iter()
                            .find(|(id, _)| id == c)
                            .map(|(_, l)| *l)
                            .unwrap_or(c);
                        ui.label(format!("· 调用宿主命令「{c}」({label})"));
                    }
                }
                for p in &info.manifest.panels {
                    ui.label(format!("· 注册面板「{}」(受控 UI,禁任意代码)", p.title));
                }
                for e in &info.manifest.exports {
                    ui.label(format!(
                        "· 注册导出动作「{}」(输出只落你选择的目录)",
                        e.title
                    ));
                }
                ui.separator();
                ui.weak("授权后插件只能调用上列命令;越权调用会被拒绝并记录。");
                ui.weak("插件是独立进程:崩溃 / 超时只影响它自己,宿主可一键重启。");
                ui.weak("插件没有直改文档文件的通道,修改文档只能经宿主命令(可撤销)。");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("启用(授权)").clicked() {
                        let id = info.id.clone();
                        match self.plugin_host.authorize(&id) {
                            Ok(()) => match self.plugin_host.start(&id) {
                                Ok(()) => {
                                    self.say(format!("插件 {id}:已授权并启动"));
                                    // 有面板 → 顺手打开插件坞面板(可见反馈)
                                    if !info.manifest.panels.is_empty() {
                                        self.plugins_panel_open = true;
                                        self.sec_focus(panel_dock::SecPanel::Plugins);
                                    }
                                }
                                Err(e) => self.toast_error(format!("已授权但启动失败:{e}")),
                            },
                            Err(e) => self.toast_error(format!("授权持久化失败:{e}")),
                        }
                        self.plugin_auth = None;
                    }
                    if ui.button("取消").clicked() {
                        // 拒绝 = 不启用(状态保持未授权)
                        self.plugin_host.deny(&info.id);
                        self.say(format!("插件 {}:未授权,保持停用", info.id));
                        self.plugin_auth = None;
                    }
                });
            });
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
            ui.label("尚无运行中的插件面板。");
            ui.weak("在「编辑 → 插件管理…」安装并启用插件(首次启用需授权)。");
            ui.add_space(8.0);
            if ui.button("打开插件管理…").clicked() {
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
        ui.weak(format!("{name}(Running;只读投影 + 受控 UI)"));
        ui.separator();
        // 导出动作(05-10-4 ④):输出只落用户选择的目录
        if let Some(info) = self.plugin_host.list().into_iter().find(|p| p.id == id) {
            for e in &info.manifest.exports {
                ui.horizontal(|ui| {
                    if ui.button(format!("导出:{}…", e.title)).clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            let res = self.with_host_services(|host, services| {
                                host.run_export(services, &id, &e.id, Some(&dir))
                            });
                            match res {
                                Ok(v) => self.say(format!(
                                    "导出完成:{}",
                                    v.get("out").and_then(|x| x.as_str()).unwrap_or("")
                                )),
                                Err(err) => self.toast_error(format!("导出失败:{err}")),
                            }
                        } else {
                            self.toast_warn("导出已取消:未选择目录");
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
                    ui.weak("等待插件提交面板内容…(点插件面板按钮,或在管理窗口重启插件)");
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
                            Ok(()) => self.say(format!("已提交输入到插件 {pid}")),
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
                        self.say(format!("插件已装载:{id}(启用前需授权)"));
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
                });
                self.plugins_mgr_open = true;
            }
        }
    }
}
