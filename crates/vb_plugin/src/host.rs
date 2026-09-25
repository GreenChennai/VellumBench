//! 插件宿主(05-10-3/4/5):注册表、权限闸门、状态机、日志环与逐帧 poll。
//!
//! 状态机:`Unauthorized →(授权)→ Stopped →(启动)→ Starting →
//! Running;任何时刻进程退出/被杀 → Crashed(可重启);停止 → Stopped。
//!
//! 线程分工(单插件串行,05-10-1):
//! - **UI/调用线程**:install / grant / start(只 spawn,不阻塞)/ stop /
//!   poll(每帧;处理插件请求与通知、检测崩溃);
//! - **握手线程**(start 内部 spawn):发 `initialize` → 等响应(带超时)
//!   → 置 Running 并发 `event/started`;失败 → 置 Crashed 并杀进程;
//! - **子进程读线程**(process.rs):stdout/stderr 泵。
//!
//! 权限闸门(安全红线):插件请求 `runCommand` 时,宿主先查 manifest
//! 白名单;越权 → 应答 -32001 + 日志环 + log::warn,**命令不执行**。
//! 授权持久化在 `auth.rs`(plugins.json);manifest 命令清单与授权快照
//! 不一致时视为未授权(防"先授权 A 再把 manifest 改成 B")。

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::auth::AuthStore;
use crate::manifest::PluginManifest;
use crate::process::{Incoming, PluginProcess};
use crate::protocol::{self, RpcError};
use crate::DEFAULT_TIMEOUT_MS;

/// 日志环容量(每插件;面板可看最近 N 条)。
pub const LOG_CAP: usize = 200;
/// 受控 UI 描述的单面板元件上限(防失控刷屏)。
pub const WIDGET_CAP: usize = 64;

/// 插件状态机(05-10-5)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginState {
    /// 已停止(初始 / 用户停止)。
    Stopped,
    /// 启动中(握手进行)。
    Starting,
    /// 运行中(握手成功)。
    Running,
    /// 已崩溃 / 超时被杀 / 启动失败(可重启)。
    Crashed,
    /// 未授权(等待用户在授权弹窗确认;拒绝 = 不启用)。
    Unauthorized,
}

impl PluginState {
    /// 状态徽标文本(管理面板显示)。
    pub fn label(self) -> &'static str {
        match self {
            PluginState::Stopped => "已停止",
            PluginState::Starting => "启动中",
            PluginState::Running => "运行中",
            PluginState::Crashed => "已崩溃",
            PluginState::Unauthorized => "未授权",
        }
    }
}

/// 一条插件日志(日志环元素;宿主侧事件与插件 `log` 通知、stderr 共环)。
#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    /// Unix 秒。
    pub at_unix: i64,
    /// 级别:info / warn / error / plugin(插件自报)。
    pub level: String,
    /// 文本。
    pub text: String,
}

/// 受控 UI 元件(05-10-4 ③;**有限元件集,禁任意代码** —— 插件只能从
/// text/metric/button/input 里选,按钮点击 = 宿主向插件发通知)。
#[derive(Debug, Clone, PartialEq)]
pub enum Widget {
    /// 静态文本(≤2000 字符)。
    Text { text: String },
    /// 键值读数(统计类面板的主元件)。
    Metric { label: String, value: String },
    /// 按钮;点击 → 宿主发 `event/button {panel, action}` 给插件。
    Button { action: String, label: String },
    /// 单行输入;回车 → 宿主发 `event/input {panel, id, value}`。
    Input {
        id: String,
        placeholder: String,
        value: String,
    },
}

/// 一个已注册面板的受控 UI(插件经 `panel/setUI` 提交;宿主渲染)。
#[derive(Debug, Clone, PartialEq)]
pub struct PanelUi {
    /// 面板 id(必须在 manifest.panels 里声明过)。
    pub id: String,
    pub widgets: Vec<Widget>,
}

/// 宿主服务端口:宿主能力回填(05-10-4)。由 UI 层实现(vb_app 接
/// `run_command` / 文档投影 / 导出);测试实现为 mock。
pub trait HostServices {
    /// 执行宿主命令(**调用前宿主已过白名单闸门**)。
    fn run_command(&mut self, plugin: &str, command: &str) -> Result<Value, String>;
    /// 文档只读投影(结构摘要;无文档 → Err)。
    fn doc_projection(&self) -> Result<Value, String>;
    /// 执行导出动作(**dir 必须是用户在宿主 UI 里选择的目录**;输出只落
    /// 那里)。`format` 来自 manifest.exports 声明(宿主已校验白名单)。
    fn run_export(
        &mut self,
        plugin: &str,
        export_id: &str,
        format: &str,
        dir: &Path,
    ) -> Result<Value, String>;
}

/// 注册表条目内部共享态(与握手线程共享)。
struct EntryShared {
    state: Mutex<PluginState>,
    proc: Mutex<Option<Arc<PluginProcess>>>,
    logs: Mutex<VecDeque<LogEntry>>,
    panels: Mutex<BTreeMap<String, PanelUi>>,
    /// 崩溃/失败原因(管理面板显示)。
    note: Mutex<String>,
    /// 用户已请求停止(握手线程看到它就不再改写状态)。
    stop_requested: AtomicBool,
}

impl EntryShared {
    fn set_state(&self, s: PluginState) {
        if let Ok(mut g) = self.state.lock() {
            *g = s;
        }
    }

    fn state(&self) -> PluginState {
        self.state
            .lock()
            .map(|g| *g)
            .unwrap_or(PluginState::Crashed)
    }

    fn push_log(&self, level: &str, text: impl Into<String>) {
        let t: String = text.into();
        if let Ok(mut g) = self.logs.lock() {
            if g.len() >= LOG_CAP {
                g.pop_front();
            }
            g.push_back(LogEntry {
                at_unix: now_secs(),
                level: level.to_string(),
                text: t.clone(),
            });
        }
        // 调试日志同步走 log 门面(调试控制台可见;05-10-3)
        match level {
            "error" => log::error!("插件日志:{t}"),
            "warn" => log::warn!("插件日志:{t}"),
            _ => log::info!("插件日志:{t}"),
        }
    }
}

/// 一条已安装插件。
struct Entry {
    manifest: PluginManifest,
    dir: PathBuf,
    shared: Arc<EntryShared>,
}

/// 宿主对外的插件信息快照(管理面板渲染用)。
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
    pub state: PluginState,
    pub authorized: bool,
    /// 崩溃/失败原因(空 = 无)。
    pub note: String,
    pub manifest: PluginManifest,
}

/// 安装失败条目(管理面板列出 + 中文原因;不影响其他插件)。
#[derive(Debug, Clone)]
pub struct InstallError {
    pub dir: PathBuf,
    pub error: String,
}

/// 插件宿主。
pub struct PluginHost {
    entries: Vec<Entry>,
    errors: Vec<InstallError>,
    store: AuthStore,
    auth_path: Option<PathBuf>,
    /// 握手超时(默认 5s;测试调短)。
    handshake_timeout: Duration,
}

impl Default for PluginHost {
    fn default() -> Self {
        Self::new(crate::auth::default_auth_path())
    }
}

impl PluginHost {
    /// 构造(显式授权文件路径;`None` = 不持久化,测试用)。
    pub fn new(auth_path: Option<PathBuf>) -> Self {
        let store = match &auth_path {
            Some(p) => AuthStore::load_from(p).0,
            None => AuthStore::default(),
        };
        PluginHost {
            entries: Vec::new(),
            errors: Vec::new(),
            store,
            auth_path,
            handshake_timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
        }
    }

    /// 调整握手超时(测试用短值)。
    pub fn with_handshake_timeout(mut self, t: Duration) -> Self {
        self.handshake_timeout = t;
        self
    }

    /// 授权文件路径(管理面板"打开授权文件"用)。
    pub fn auth_path(&self) -> Option<&Path> {
        self.auth_path.as_deref()
    }

    // ---------- 安装登记 ----------

    /// 安装:校验目录内 plugin.json(严格 schema + 命令必须真实存在),
    /// 登记进授权文件。返回插件 id。
    ///
    /// `is_implemented`:宿主命令注册表判据 —— 白名单里的每条命令都必须
    /// 是已注册命令(否则插件声明的权限根本执行不了,等于说谎)。
    pub fn install_dir(
        &mut self,
        dir: &Path,
        is_implemented: &dyn Fn(&str) -> bool,
    ) -> Result<String, String> {
        if !dir.is_dir() {
            return Err(format!("插件目录不存在:{}", dir.display()));
        }
        let mf_path = dir.join("plugin.json");
        let manifest = PluginManifest::load_file(&mf_path)?;
        for cmd in &manifest.commands {
            if !is_implemented(cmd) {
                return Err(format!(
                    "manifest 请求的命令「{cmd}」不是宿主已注册命令(白名单只接受真实存在的命令)"
                ));
            }
        }
        // 同 id 只允许装一份(先卸旧的)
        if let Some(old) = self
            .entries
            .iter()
            .find(|e| e.manifest.id == manifest.id)
            .map(|e| e.dir.clone())
        {
            self.uninstall(&old);
        }
        let dir_canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        self.store.install(&dir_canon);
        self.load_one(&dir_canon, manifest);
        self.persist()?;
        Ok(self
            .entries
            .iter()
            .find(|e| e.dir == dir_canon)
            .map(|e| e.manifest.id.clone())
            .unwrap_or_default())
    }

    /// 启动时重载全部已登记插件(损坏条目进 `errors`,不阻断其他)。
    pub fn reload_installed(&mut self, is_implemented: &dyn Fn(&str) -> bool) {
        self.entries.clear();
        self.errors.clear();
        let dirs: Vec<PathBuf> = self
            .store
            .installed
            .iter()
            .map(|e| PathBuf::from(&e.dir))
            .collect();
        for dir in dirs {
            match PluginManifest::load_file(&dir.join("plugin.json")) {
                Ok(m) => {
                    let bad = m.commands.iter().find(|c| !is_implemented(c)).cloned();
                    match bad {
                        Some(c) => self.errors.push(InstallError {
                            dir,
                            error: format!("manifest 请求的命令「{c}」不是宿主已注册命令"),
                        }),
                        None => self.load_one(&dir, m),
                    }
                }
                Err(e) => self.errors.push(InstallError { dir, error: e }),
            }
        }
    }

    /// 装载一个条目(初始状态按授权态落:未授权 → Unauthorized)。
    fn load_one(&mut self, dir: &Path, manifest: PluginManifest) {
        let authorized = self.store.grant_matches(&manifest.id, &manifest.commands);
        let shared = Arc::new(EntryShared {
            state: Mutex::new(if authorized {
                PluginState::Stopped
            } else {
                PluginState::Unauthorized
            }),
            proc: Mutex::new(None),
            logs: Mutex::new(VecDeque::new()),
            panels: Mutex::new(BTreeMap::new()),
            note: Mutex::new(String::new()),
            stop_requested: AtomicBool::new(false),
        });
        shared.push_log(
            "info",
            format!(
                "已装载:{} v{}({} 条命令权限 / {} 个面板 / {} 个导出)",
                manifest.name,
                manifest.version,
                manifest.commands.len(),
                manifest.panels.len(),
                manifest.exports.len()
            ),
        );
        self.entries.push(Entry {
            manifest,
            dir: dir.to_path_buf(),
            shared,
        });
    }

    /// 卸载(停止进程 + 移出注册表 + 清授权)。
    pub fn uninstall(&mut self, dir: &Path) -> bool {
        let dir_canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let Some(pos) = self.entries.iter().position(|e| e.dir == dir_canon) else {
            return false;
        };
        let e = &self.entries[pos];
        self.stop_by_shared(&e.shared);
        let id = e.manifest.id.clone();
        self.entries.remove(pos);
        self.store.uninstall(&dir_canon);
        self.store.revoke(&id);
        let _ = self.persist();
        true
    }

    /// 持久化授权文件(失败 → Err,调用方给 toast;**不静默丢授权**)。
    fn persist(&self) -> Result<(), String> {
        match &self.auth_path {
            Some(p) => self.store.save_to(p),
            None => Ok(()),
        }
    }

    // ---------- 查询 ----------

    /// 全部插件信息快照(列表序 = 安装序)。
    pub fn list(&self) -> Vec<PluginInfo> {
        self.entries
            .iter()
            .map(|e| PluginInfo {
                id: e.manifest.id.clone(),
                name: e.manifest.name.clone(),
                version: e.manifest.version.clone(),
                dir: e.dir.clone(),
                state: e.shared.state(),
                authorized: self
                    .store
                    .grant_matches(&e.manifest.id, &e.manifest.commands),
                note: e.shared.note.lock().map(|g| g.clone()).unwrap_or_default(),
                manifest: e.manifest.clone(),
            })
            .collect()
    }

    /// 安装失败条目(管理面板红字区)。
    pub fn install_errors(&self) -> &[InstallError] {
        &self.errors
    }

    /// 日志环快照(最近 N 条,时序)。
    pub fn logs(&self, id: &str) -> Vec<LogEntry> {
        self.entries
            .iter()
            .find(|e| e.manifest.id == id)
            .map(|e| {
                e.shared
                    .logs
                    .lock()
                    .map(|g| g.iter().cloned().collect())
                    .unwrap_or_default()
            })
            .unwrap_or_default()
    }

    /// 某 Running 插件已注册面板的当前 UI。
    pub fn panel_ui(&self, id: &str, panel_id: &str) -> Option<PanelUi> {
        self.entries
            .iter()
            .find(|e| e.manifest.id == id)
            .and_then(|e| {
                e.shared
                    .panels
                    .lock()
                    .ok()
                    .and_then(|g| g.get(panel_id).cloned())
            })
    }

    /// 全部 Running 且声明了面板的插件(插件坞面板渲染用)。
    pub fn running_with_panels(&self) -> Vec<(String, String, Vec<crate::manifest::PanelDecl>)> {
        self.entries
            .iter()
            .filter(|e| e.shared.state() == PluginState::Running)
            .filter(|e| !e.manifest.panels.is_empty())
            .map(|e| {
                (
                    e.manifest.id.clone(),
                    e.manifest.name.clone(),
                    e.manifest.panels.clone(),
                )
            })
            .collect()
    }

    fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.manifest.id == id)
    }

    // ---------- 授权(05-10-3)----------

    /// 用户在授权弹窗点了「启用」:持久化授权(命令白名单快照)。
    /// 之后调用方再 `start`。授权文件写失败 → Err(授权绝不静默丢)。
    pub fn authorize(&mut self, id: &str) -> Result<(), String> {
        if !self.entries.iter().any(|e| e.manifest.id == id) {
            return Err(format!("插件 {id} 未安装"));
        }
        let cmds = self
            .entries
            .iter()
            .find(|e| e.manifest.id == id)
            .map(|e| e.manifest.commands.clone())
            .unwrap_or_default();
        self.store.grant(id, &cmds);
        if let Some(e) = self.entry(id) {
            e.shared
                .push_log("info", "用户已授权:manifest 权限清单确认");
        }
        self.persist()?;
        Ok(())
    }

    /// 用户点了「取消」:不启用(状态保持 Unauthorized;同时清掉可能
    /// 残留的旧授权)。
    pub fn deny(&mut self, id: &str) {
        self.store.revoke(id);
        if let Some(e) = self.entry(id) {
            e.shared.set_state(PluginState::Unauthorized);
            e.shared.push_log("info", "用户拒绝授权:插件保持未启用");
        }
        let _ = self.persist();
    }

    // ---------- 生命周期(05-10-5)----------

    /// 启动(默认无参数)。授权检查 → spawn → 握手线程。
    pub fn start(&mut self, id: &str) -> Result<(), String> {
        self.start_with_args(id, &[])
    }

    /// 启动(带参数;门禁测试用 `--probe/--crash/--hang` 夹具)。
    pub fn start_with_args(&mut self, id: &str, args: &[String]) -> Result<(), String> {
        let Some(e) = self.entry(id) else {
            return Err(format!("插件 {id} 未安装"));
        };
        let shared = e.shared.clone();
        if shared.state() == PluginState::Starting || shared.state() == PluginState::Running {
            return Err(format!("插件 {id} 已在运行"));
        }
        // 授权闸门:manifest 命令清单必须与授权快照一致
        if !self.store.grant_matches(id, &e.manifest.commands) {
            shared.set_state(PluginState::Unauthorized);
            shared.push_log("warn", "启动被拒:插件未授权(或 manifest 权限已变更)");
            return Err(format!("插件 {id} 未授权,先在授权弹窗确认"));
        }
        // 解析入口可执行
        let host_exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let exe = e.manifest.resolve_entry(&e.dir, host_exe_dir.as_deref());
        if !exe.is_file() {
            shared.set_state(PluginState::Crashed);
            let msg = format!("入口可执行不存在:{}", exe.display());
            shared.set_note(&msg);
            shared.push_log("error", msg.clone());
            return Err(msg);
        }
        shared.set_note("");
        shared.stop_requested.store(false, Ordering::SeqCst);
        shared.set_state(PluginState::Starting);
        shared.push_log("info", format!("启动:{}", exe.display()));
        let process = match PluginProcess::spawn(&exe, args) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                shared.set_state(PluginState::Crashed);
                shared.set_note(&e);
                shared.push_log("error", e.clone());
                return Err(e);
            }
        };
        *shared
            .proc
            .lock()
            .map_err(|_| "插件进程槽锁中毒".to_string())? = Some(process.clone());
        // 握手线程:initialize → Running → event/started
        let timeout = self.handshake_timeout;
        let manifest_name = e.manifest.name.clone();
        let plugin_id = e.manifest.id.clone();
        std::thread::Builder::new()
            .name(format!("vb-plugin-handshake-{id}"))
            .spawn(move || {
                match handshake(&process, &plugin_id, timeout) {
                    Ok(()) => {
                        shared.set_state(PluginState::Running);
                        shared.push_log("info", format!("「{manifest_name}」已就绪(Running)"));
                        let _ = process.send_notification(protocol::M_EVENT_STARTED, &json!({}));
                    }
                    Err(msg) => {
                        process.kill();
                        if shared.stop_requested.load(Ordering::SeqCst) {
                            // 用户已叫停:状态归 Stopped,不再标崩溃
                            shared.set_state(PluginState::Stopped);
                        } else {
                            let full = format!("握手失败:{msg}(进程已终止)");
                            shared.set_note(&msg);
                            shared.set_state(PluginState::Crashed);
                            shared.push_log("error", full);
                        }
                    }
                }
            })
            .map_err(|e| format!("握手线程启动失败:{e}"))?;
        Ok(())
    }

    /// 停止(礼貌通知 + 立即杀;幂等)。
    pub fn stop(&mut self, id: &str) {
        if let Some(e) = self.entry(id) {
            self.stop_by_shared(&e.shared);
        }
    }

    fn stop_by_shared(&self, shared: &Arc<EntryShared>) {
        shared.stop_requested.store(true, Ordering::SeqCst);
        let proc = shared.proc.lock().map(|g| g.clone()).ok().flatten();
        if let Some(p) = &proc {
            // 礼貌停机通知(不等回包):大多数插件收到后自行退出
            let _ = p.send_notification(protocol::M_SHUTDOWN, &json!({}));
        }
        if let Some(p) = &proc {
            p.kill();
        }
        shared.set_state(PluginState::Stopped);
        shared.push_log("info", "已停止");
    }

    /// 重启(崩溃后的按钮;先停再起)。
    pub fn restart(&mut self, id: &str) -> Result<(), String> {
        self.stop(id);
        self.start(id)
    }

    // ---------- 面板交互回流(05-10-4 ③)----------

    /// 面板按钮点击 → 通知插件。
    pub fn send_button(&self, id: &str, panel: &str, action: &str) -> Result<(), String> {
        let Some(e) = self.entry(id) else {
            return Err(format!("插件 {id} 未安装"));
        };
        let proc = e.shared.proc.lock().map(|g| g.clone()).ok().flatten();
        match proc {
            Some(p) if e.shared.state() == PluginState::Running => p.send_notification(
                protocol::M_EVENT_BUTTON,
                &json!({"panel": panel, "action": action}),
            ),
            _ => Err(format!("插件 {id} 未在运行,面板事件已丢弃")),
        }
    }

    /// 面板输入提交 → 通知插件。
    pub fn send_input(
        &self,
        id: &str,
        panel: &str,
        input_id: &str,
        value: &str,
    ) -> Result<(), String> {
        let Some(e) = self.entry(id) else {
            return Err(format!("插件 {id} 未安装"));
        };
        let proc = e.shared.proc.lock().map(|g| g.clone()).ok().flatten();
        match proc {
            Some(p) if e.shared.state() == PluginState::Running => p.send_notification(
                protocol::M_EVENT_INPUT,
                &json!({"panel": panel, "id": input_id, "value": value}),
            ),
            _ => Err(format!("插件 {id} 未在运行,输入已丢弃")),
        }
    }

    // ---------- 导出器(05-10-4 ④)----------

    /// 执行插件声明的导出动作。**dir 必须来自用户的目录选择**(UI 层用
    /// rfd 选;传 None = 用户还没选 → 拒绝)。宿主自己执行导出,插件
    /// 进程不接触输出目录。
    pub fn run_export(
        &mut self,
        services: &mut dyn HostServices,
        id: &str,
        export_id: &str,
        dir: Option<&Path>,
    ) -> Result<Value, String> {
        let Some(e) = self.entry(id) else {
            return Err(format!("插件 {id} 未安装"));
        };
        let Some(decl) = e.manifest.exports.iter().find(|x| x.id == export_id) else {
            return Err(format!(
                "插件 {id} 未声明导出动作「{export_id}」(manifest.exports)"
            ));
        };
        let Some(dir) = dir else {
            return Err("导出目录必须先由用户选择(插件不能自选输出位置)".into());
        };
        e.shared.push_log(
            "info",
            format!(
                "执行导出「{}」({})→ {}",
                decl.title,
                decl.format,
                dir.display()
            ),
        );
        services.run_export(id, export_id, &decl.format, dir)
    }

    // ---------- 逐帧 poll ----------

    /// 每帧调用:泵 stderr → 日志环;处理插件请求/通知;检测崩溃。
    /// **崩溃隔离**:本函数内的一切都不 panic;插件异常只落日志与状态。
    pub fn poll(&mut self, services: &mut dyn HostServices) {
        for e in &self.entries {
            let shared = &e.shared;
            // ① stderr → 日志环
            if let Some(proc) = shared.proc.lock().map(|g| g.clone()).ok().flatten() {
                let mut lines = Vec::new();
                proc.drain_stderr(&mut lines);
                for l in lines {
                    shared.push_log("plugin", l);
                }
                // ② 崩溃检测(Running 态进程死亡 → Crashed;Starting 由
                //    握手线程收尾)
                if shared.state() == PluginState::Running && !proc.is_alive() {
                    let code = proc.exit_code();
                    let msg = match code {
                        Some(c) => format!("插件进程已退出(退出码 {c})"),
                        None => "插件进程已退出".to_string(),
                    };
                    shared.set_note(&msg);
                    shared.set_state(PluginState::Crashed);
                    shared.push_log("error", msg);
                }
                // ③ 入站消息(请求 + 通知)
                let mut incoming = VecDeque::new();
                proc.drain_incoming(&mut incoming);
                for inc in incoming {
                    self.handle_incoming(e, &proc, services, inc);
                }
            }
        }
    }

    /// 处理一条插件入站消息(请求一律应答;通知一律不崩)。
    fn handle_incoming(
        &self,
        e: &Entry,
        proc: &Arc<PluginProcess>,
        services: &mut dyn HostServices,
        inc: Incoming,
    ) {
        match inc {
            Incoming::Request { id, method, params } => match method.as_str() {
                protocol::M_RUN_COMMAND => {
                    self.on_run_command(e, proc, services, id, &params);
                }
                protocol::M_DOC_PROJECTION => {
                    let Some(id) = id else {
                        e.shared
                            .push_log("warn", "doc/projection 以通知发出(缺 id),已忽略");
                        return;
                    };
                    match services.doc_projection() {
                        Ok(v) => proc.respond(&id, &v),
                        Err(msg) => proc.respond_error(
                            &id,
                            &RpcError::new(protocol::E_PROJECTION_UNAVAILABLE, msg),
                        ),
                    }
                }
                other => {
                    let msg = format!("未知方法:{other}");
                    if let Some(id) = id {
                        proc.respond_error(&id, &RpcError::new(protocol::E_METHOD_NOT_FOUND, &msg));
                    }
                    e.shared.push_log("warn", msg);
                }
            },
            Incoming::Notification { method, params } => match method.as_str() {
                protocol::M_PANEL_SET_UI => self.on_panel_set_ui(e, &params),
                protocol::M_LOG => {
                    let level = params
                        .get("level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("plugin")
                        .to_string();
                    let text = params
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !text.is_empty() {
                        e.shared.push_log(&level, format!("[插件] {text}"));
                    }
                }
                other => {
                    e.shared
                        .push_log("warn", format!("未知通知:{other}(已忽略)"));
                }
            },
        }
    }

    /// **权限闸门**(安全红线):白名单外命令 → -32001 + 记录,不执行。
    fn on_run_command(
        &self,
        e: &Entry,
        proc: &Arc<PluginProcess>,
        services: &mut dyn HostServices,
        id: Option<Value>,
        params: &Value,
    ) {
        let cmd = params.get("command").and_then(|v| v.as_str()).unwrap_or("");
        let Some(id) = id else {
            e.shared
                .push_log("warn", "runCommand 以通知发出(缺 id),已忽略");
            return;
        };
        if cmd.is_empty() {
            proc.respond_error(
                &id,
                &RpcError::new(protocol::E_INVALID_PARAMS, "缺少 command"),
            );
            return;
        }
        if !e.manifest.commands.iter().any(|c| c == cmd) {
            // 越权:拒绝 + 记录(内存日志环 + 调试日志)
            let msg = format!("越权调用被拒:「{cmd}」不在 manifest.commands 白名单内");
            proc.respond_error(
                &id,
                &RpcError::new(
                    protocol::E_PERMISSION_DENIED,
                    format!("权限拒绝:{msg}(已记录)"),
                ),
            );
            e.shared.push_log("warn", msg);
            return;
        }
        // 白名单内 → 宿主执行(可撤销、与 UI 同一命令路径)
        match services.run_command(&e.manifest.id, cmd) {
            Ok(v) => proc.respond(&id, &v),
            Err(msg) => {
                proc.respond_error(&id, &RpcError::new(protocol::E_COMMAND_FAILED, msg.clone()));
                e.shared
                    .push_log("warn", format!("命令「{cmd}」执行失败:{msg}"));
            }
        }
    }

    /// 受控 UI 描述落地(校验:面板必须声明过、元件必须可识别、数量封顶)。
    fn on_panel_set_ui(&self, e: &Entry, params: &Value) {
        let Some(panel) = params.get("panel").and_then(|v| v.as_str()) else {
            e.shared
                .push_log("warn", "panel/setUI 缺 panel 字段,已忽略");
            return;
        };
        if !e.manifest.panels.iter().any(|p| p.id == panel) {
            e.shared.push_log(
                "warn",
                format!("panel/setUI 指向未声明面板「{panel}」(manifest.panels),已忽略"),
            );
            return;
        }
        let Some(widgets_json) = params.get("widgets").and_then(|v| v.as_array()) else {
            e.shared
                .push_log("warn", "panel/setUI 缺 widgets 数组,已忽略");
            return;
        };
        if widgets_json.len() > WIDGET_CAP {
            e.shared.push_log(
                "warn",
                format!(
                    "panel/setUI 元件数 {} 超上限 {WIDGET_CAP},已整批拒绝",
                    widgets_json.len()
                ),
            );
            return;
        }
        let mut widgets = Vec::new();
        for (i, w) in widgets_json.iter().enumerate() {
            match parse_widget(w) {
                Ok(w) => widgets.push(w),
                Err(msg) => {
                    e.shared
                        .push_log("warn", format!("panel/setUI 第 {i} 个元件被拒:{msg}"));
                }
            }
        }
        if let Ok(mut g) = e.shared.panels.lock() {
            g.insert(
                panel.to_string(),
                PanelUi {
                    id: panel.to_string(),
                    widgets,
                },
            );
        }
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        // 退出兜底:所有子进程随宿主终止(05-10-5「退出时停止」)
        for e in &self.entries {
            self.stop_by_shared(&e.shared);
        }
    }
}

/// 握手:发 `initialize` 请求并等响应(05-10-1 的消息形态在此定死)。
fn handshake(proc: &PluginProcess, plugin_id: &str, timeout: Duration) -> Result<(), String> {
    let params = json!({
        "pluginId": plugin_id,
        "hostVersion": env!("CARGO_PKG_VERSION"),
        "capabilities": crate::HOST_CAPABILITIES,
    });
    let req_id = proc.send_request(protocol::M_INITIALIZE, &params)?;
    let result = proc.call(req_id, timeout)?;
    let result = result.map_err(|e| format!("插件拒绝握手({}:{})", e.code, e.message))?;
    // 宽松校验:回包须带 name(version/protocolVersion 缺省可容忍 ——
    // 严格校验留给 manifest,握手回包只当"活体证明")
    if result.get("name").and_then(|v| v.as_str()).is_none() {
        return Err("握手回包缺少 name 字段".into());
    }
    Ok(())
}

/// 单个受控元件解析(未知 kind / 超长文本 → Err,宿主跳过该元件)。
fn parse_widget(v: &Value) -> Result<Widget, String> {
    let obj = v.as_object().ok_or("元件必须是对象")?;
    let kind = obj
        .get("kind")
        .and_then(|k| k.as_str())
        .ok_or("元件缺 kind")?;
    let s = |key: &str| -> Result<String, String> {
        let v = obj.get(key).and_then(|x| x.as_str()).unwrap_or("");
        if v.chars().count() > 2000 {
            return Err(format!("字段 {key} 超长(>2000 字符)"));
        }
        Ok(v.to_string())
    };
    match kind {
        "text" => Ok(Widget::Text { text: s("text")? }),
        "metric" => Ok(Widget::Metric {
            label: s("label")?,
            value: s("value")?,
        }),
        "button" => {
            let action = s("action")?;
            if action.is_empty() {
                return Err("button 缺 action".into());
            }
            Ok(Widget::Button {
                action,
                label: s("label")?,
            })
        }
        "input" => {
            let id = s("id")?;
            if id.is_empty() {
                return Err("input 缺 id".into());
            }
            Ok(Widget::Input {
                id,
                placeholder: s("placeholder")?,
                value: s("value")?,
            })
        }
        other => Err(format!(
            "未知元件类型「{other}」(只支持 text/metric/button/input)"
        )),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl EntryShared {
    fn set_note(&self, msg: &str) {
        if let Ok(mut g) = self.note.lock() {
            *g = msg.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write_manifest(dir: &Path, commands: &[&str]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let cmds: Vec<String> = commands.iter().map(|c| format!("\"{c}\"")).collect();
        let text = format!(
            r#"{{"id":"gate-plug","name":"门禁插件","version":"0.1.0","entry":"whatever.exe","commands":[{}],"panels":[{{"id":"p1","title":"面板一"}}]}}"#,
            cmds.join(",")
        );
        let p = dir.join("plugin.json");
        std::fs::write(&p, text).unwrap();
        p
    }

    /// 权限闸门单测(05-10-7 ②的纯函数层):白名单内放行、白名单外拒绝。
    /// (进程级端到端在 tests/plugin_gates.rs。)
    #[test]
    fn permission_gate_denies_outside_whitelist() {
        let tmp = std::env::temp_dir().join(format!("vb-host-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mf_dir = tmp.join("plug");
        write_manifest(&mf_dir, &["edit.undo"]);

        let mut host = PluginHost::new(None);
        host.install_dir(&mf_dir, &|_| true).expect("安装成功");
        // 未授权 → 启动被拒
        assert!(host.start("gate-plug").is_err(), "未授权不得启动");
        assert_eq!(host.list()[0].state, PluginState::Unauthorized);
        // 授权后仍不启动进程(入口不存在),但授权检查通过到"入口不存在"
        host.authorize("gate-plug").unwrap();
        let err = host.start("gate-plug").unwrap_err();
        assert!(err.contains("入口可执行"), "{err}");
        assert_eq!(host.list()[0].state, PluginState::Crashed);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// install 的白名单必须指向真实命令。
    #[test]
    fn install_rejects_unknown_commands() {
        let tmp = std::env::temp_dir().join(format!("vb-host-gate2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mf_dir = tmp.join("plug");
        write_manifest(&mf_dir, &["no.such_command"]);
        let mut host = PluginHost::new(None);
        let err = host
            .install_dir(&mf_dir, &|id| id == "edit.undo")
            .unwrap_err();
        assert!(err.contains("no.such_command"), "{err}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 受控元件解析:text/metric/button/input 之外拒绝;超长拒绝。
    #[test]
    fn widget_parsing_is_strict() {
        assert!(matches!(
            parse_widget(&json!({"kind": "metric", "label": "节点", "value": "21"})),
            Ok(Widget::Metric { .. })
        ));
        assert!(
            parse_widget(&json!({"kind": "eval", "code": "os.system()"})).is_err(),
            "未知元件必须拒绝(禁任意代码)"
        );
        assert!(parse_widget(&json!({"kind": "button", "action": ""})).is_err());
        assert!(parse_widget(&json!({"kind": "text", "text": "x".repeat(3000)})).is_err());
    }

    /// 状态机标签与授权路径:deny 后保持 Unauthorized。
    #[test]
    fn deny_keeps_unauthorized() {
        let tmp = std::env::temp_dir().join(format!("vb-host-gate3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mf_dir = tmp.join("plug");
        write_manifest(&mf_dir, &[]);
        let mut host = PluginHost::new(None);
        host.install_dir(&mf_dir, &|_| true).unwrap();
        assert_eq!(host.list()[0].state, PluginState::Unauthorized);
        host.authorize("gate-plug").unwrap();
        // 授权后(入口不存在 → Crashed 但已越过授权)
        let _ = host.start("gate-plug");
        assert_eq!(host.list()[0].state, PluginState::Crashed);
        host.stop("gate-plug");
        assert_eq!(host.list()[0].state, PluginState::Stopped);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
