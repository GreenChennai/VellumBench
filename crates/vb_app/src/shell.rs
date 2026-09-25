//! 外壳(阶段 2 / 副文档 02-1、02-5):启动流程 + 多窗口管理。
//!
//! **架构**:顶层 `eframe::App` 只是一个"外壳"——它持有主页
//! ([`crate::launcher::LauncherUi`])与若干项目窗口(每窗口一份独立
//! `VellumApp`:独立 Document / 撤销栈 / 选中态 / 面板布局,02-5-1),
//! 用 `ctx.show_viewport_immediate` 并存渲染(02-1-2 / 02-5-2)。
//!
//! **根视口约束**:eframe 的根视口在启动时固定且不可更换——
//! - 无参数启动 → 根视口 = 主页(1024×680);
//! - `--project` 启动 → 根视口 = 第一个项目窗口(1680×1000),主页不出现。
//!
//! 因此"关闭根窗口 = 退出进程"(其余窗口一并关闭,有未保存修改时先确认);
//! 子项目窗口可独立关闭,互不影响(02-5 验收)。eframe 多 viewport 不做
//! "隐藏后唤出",只做"并存 + 关闭"(副文档 02 §6 风险)。
//!
//! **共享配置写竞争**(02-5-3):`recent.json` 只有外壳一个写入者;
//! `workspace.json` 仍由各窗口写(停靠偏好随窗口),**最后写入胜**,
//! 写入均为原子替换,不存在半截文件。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};

use egui::{ViewportBuilder, ViewportCommand, ViewportId};
use vb_doc::model::Document;

use crate::launcher::LauncherUi;
use crate::new_project::{create_from_template, create_project, NewProjectSpec};
use crate::recent::{self, RecentStore};
use crate::VellumApp;

// ─────────────────────────── 启动参数(02-1-1 / 02-1-4) ───────────────────────────

/// 启动模式(argv 解析结果)。
#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    /// 无参数 → 打开启动主页。
    Home,
    /// `--project <dir>` / 位置参数 `<dir>` / `<file.html>` → 直开项目窗口。
    Project(PathBuf),
    /// `--project <dir> --canvas-shot <out.png> --artboard <sid>`(03-3,隐藏:
    /// 打开项目 → 固定标称相机 → 窗口增长探针 → 画布纹理读回落盘 → 自动退出)。
    /// 供 `tools/canvas_parity.ps1` 取"画布侧 PNG",不是用户功能。
    Shot {
        project: PathBuf,
        out: PathBuf,
        artboard: String,
    },
    /// `--help` / `-h`:帮助文本(stdout,退出码 0)。
    Help(String),
    /// `--version` / `-V`:版本文本(stdout,退出码 0)。
    Version(String),
    /// 参数错误(stderr,退出码 2;脚本调用要大声失败,不静默回退)。
    Error(String),
}

impl Launch {
    /// 是否直达项目窗口(02-1-1:不出现主页)。
    pub fn is_direct_project(&self) -> bool {
        matches!(self, Launch::Project(_) | Launch::Shot { .. })
    }
}

/// 解析命令行(`argv[0]` = 程序名,忽略)。
pub fn parse_launch(argv: &[String]) -> Launch {
    // 03-3:--canvas-shot / --artboard 可出现在 --project 之前或之后,
    // 因此不再对 --project 提前返回,统一收集后装配
    let mut project: Option<PathBuf> = None;
    let mut shot_out: Option<PathBuf> = None;
    let mut shot_ab: Option<String> = None;
    let mut it = argv.iter().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => return Launch::Help(help_text()),
            "--version" | "-V" => {
                return Launch::Version(format!(
                    "Vellum Bench v{}(绘制 HTML 的台面)\n",
                    env!("CARGO_PKG_VERSION")
                ));
            }
            "--project" | "-p" => match it.next() {
                Some(v) if !v.trim().is_empty() => project = Some(PathBuf::from(v)),
                _ => {
                    return Launch::Error(
                        "错误:--project 需要一个项目目录(或 index.html)参数\n\
                         用法:vellumbench --project <目录>\n\
                         详见:vellumbench --help"
                            .into(),
                    );
                }
            },
            "--canvas-shot" => match it.next() {
                Some(v) if !v.trim().is_empty() => shot_out = Some(PathBuf::from(v)),
                _ => {
                    return Launch::Error(
                        "错误:--canvas-shot 需要一个输出 PNG 路径(需与 --project、\
                         --artboard 同用)\n\
                         用法:vellumbench --project <目录> --canvas-shot <out.png> \
                         --artboard <sid|名称|序号>"
                            .into(),
                    );
                }
            },
            "--artboard" => match it.next() {
                Some(v) if !v.trim().is_empty() => shot_ab = Some(v.to_string()),
                _ => {
                    return Launch::Error(
                        "错误:--artboard 需要画板 sid / 名称 / 序号\n\
                         用法:vellumbench --project <目录> --canvas-shot <out.png> \
                         --artboard <sid|名称|序号>"
                            .into(),
                    );
                }
            },
            other if other.starts_with('-') => {
                // 未知开关:忽略并继续(双击/快捷方式带的参数不该挡住启动)
                log::warn!("忽略未知参数:{other}(--help 查看用法)");
            }
            other => project = Some(PathBuf::from(other)),
        }
    }
    match (project, shot_out, shot_ab) {
        (Some(p), Some(out), Some(ab)) => Launch::Shot {
            project: p,
            out,
            artboard: ab,
        },
        (Some(p), None, None) => Launch::Project(p),
        (None, Some(_), _) | (None, _, Some(_)) => Launch::Error(
            "错误:--canvas-shot / --artboard 必须与 --project 同用\n\
             用法:vellumbench --project <目录> --canvas-shot <out.png> \
             --artboard <sid|名称|序号>"
                .into(),
        ),
        (Some(_), Some(_), None) | (Some(_), None, Some(_)) => Launch::Error(
            "错误:--canvas-shot 与 --artboard 必须同时提供\n\
             用法:vellumbench --project <目录> --canvas-shot <out.png> \
             --artboard <sid|名称|序号>"
                .into(),
        ),
        (None, None, None) => Launch::Home,
    }
}

fn help_text() -> String {
    format!(
        "Vellum Bench v{} · 绘台 — 用 AI 心智编辑 100% 标准 HTML/CSS 文档\n\
         \n\
         用法:\n\
         \x20 vellumbench                      打开启动主页(最近项目 / 新建 / 模板)\n\
         \x20 vellumbench --project <目录>      直接打开项目窗口(跳过主页)\n\
         \x20 vellumbench <目录|index.html>     同上(位置参数)\n\
         \n\
         选项:\n\
         \x20 -h, --help     显示本帮助\n\
         \x20 -V, --version  显示版本号\n\
         \n\
         示例:\n\
         \x20 vellumbench --project examples\\landing\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// 任意路径 → 项目目录:`index.html`/任意 `.html` 文件取其父目录,其余原样。
pub fn resolve_project_dir(p: &Path) -> PathBuf {
    if p.is_file() {
        p.parent()
            .map(|d| d.to_path_buf())
            .unwrap_or_else(|| p.to_path_buf())
    } else {
        p.to_path_buf()
    }
}

// ─────────────────────────── 外壳请求(单写入者通道) ───────────────────────────

/// 项目窗口 → 外壳的请求(项目窗口自身不做任何全局配置写盘)。
pub enum ShellRequest {
    /// 打开目录为项目窗口(已打开则聚焦,02-5-5)。
    OpenProject(PathBuf),
    /// 新建项目(02-4):生成最小合法项目 → 新窗口。
    CreateProject(NewProjectSpec),
    /// 从模板新建(02-4-4):整目录复制 → 新窗口。
    CreateFromTemplate {
        template: String,
        location: PathBuf,
        name: String,
    },
    /// 打开主页(`--project` 模式下主页是子视口;home 模式下聚焦根)。
    ShowHome,
    /// 关闭某个项目窗口(有未保存修改时外壳会弹确认,02-5-6)。
    CloseWindow(ViewportId),
    /// 记录"最近项目"(02-2-2:成功打开/保存时由窗口发起)。
    TouchRecent(PathBuf),
    /// 从最近列表移除(02-2-3)。
    RemoveRecent(PathBuf),
    /// 固定/取消固定(02-2-3)。
    TogglePinRecent(PathBuf),
    /// 一键恢复上次会话(02-6-2)。
    RestoreSession,
    /// 主题变化广播(多窗口 + 主页跟随)。
    ThemeChanged(bool),
    /// H-1 动效总开关广播(主页跟随窗口侧的切换;进程级生效)。
    MotionChanged(bool),
    /// 后台缩略图生成完毕(02-2-5;不阻塞打开)。
    ThumbReady { dir: PathBuf, thumb: PathBuf },
    /// 退出整个应用(保存会话后自然关闭根视口)。
    QuitAll,
}

// ─────────────────────────── 项目窗口 ───────────────────────────

/// 外壳管理的一个项目窗口。
struct ProjectWindow {
    id: ViewportId,
    app: VellumApp,
    /// 关闭确认对话框进行中(02-5-6:有未保存改动 → 保存/丢弃/取消)。
    confirm_close: bool,
    /// 已确认关闭(下一帧不再 show → 窗口消失)。
    wants_close: bool,
}

impl ProjectWindow {
    fn project_dir(&self) -> Option<&Path> {
        self.app.project_dir.as_deref()
    }
}

/// 窗口标题(02-5-4 / 07-C):`<项目名> — Vellum Bench`,脏时 `<项目名>*`。
/// `*` 跟在项目名后(与 07-1 定夺文案一致);Ctrl+S 成功后 `*` 消失
/// (`saved_rev` 对齐 → `is_dirty()` 翻假 → 标题每帧重算自动刷新)。
pub fn window_title(app: &VellumApp) -> String {
    let star = if app.is_dirty() { "*" } else { "" };
    format!("{}{star} — Vellum Bench", app.display_name())
}

// ─────────────────────────── 外壳 ───────────────────────────

/// H-7:推迟打开的两相位状态。
#[derive(Debug, Clone, PartialEq)]
enum PendingOpen {
    /// 本帧受理(占位浮层随本帧画出并呈现)。
    Armed(PathBuf),
    /// 下一帧执行(阻塞期间屏幕上仍是上一帧的占位画面)。
    Fire(PathBuf),
}

pub struct ShellApp {
    tx: Sender<ShellRequest>,
    rx: Receiver<ShellRequest>,
    /// 当前主题(true = 深色;主页与所有窗口跟随,单一真相在外壳)。
    theme_dark: bool,
    /// 最近项目(单写入者:只有外壳改它并落盘)。
    recent: RecentStore,
    home: LauncherUi,
    /// 根视口是否为主页(= 无参数启动)。
    home_at_root: bool,
    /// 主页视口 id(project 模式下主页是子视口)。
    home_id: ViewportId,
    /// 根项目窗口(`--project` 启动时 Some)。
    root: Option<ProjectWindow>,
    /// 子项目窗口。
    wins: Vec<ProjectWindow>,
    /// project 模式下主页子视口是否打开。
    home_visible: bool,
    /// 新窗口 id 发号器。
    next_id: u64,
    /// 进行中的缩略图后台任务(路径键去重)。
    thumb_jobs: HashSet<String>,
    /// 退出确认对话框进行中(根窗口/主页关闭且还有其它窗口或未保存改动)。
    exit_confirm: bool,
    /// 用户已确认退出 → 不再取消根视口的关闭请求(自然退出进程)。
    force_quit: bool,
    /// 待聚焦的视口(新建/聚焦窗口在其 viewport 真正渲染的那一帧发命令才可靠)。
    pending_focus: Option<ViewportId>,
    /// H-7:推迟一帧的打开动作(主页先画「正在打开…」占位,下一帧再做
    /// 阻塞的导入 + 开窗)。Armed = 本帧刚受理;Fire = 下一帧执行。
    pending_open: Option<PendingOpen>,
    /// H-1 动效总开关(主页侧;由 workspace.json 初始化、窗口广播更新)。
    motion_enabled: bool,
    /// 已同步到 egui 的主题(None=尚未同步;与 VellumApp.frame 同款,
    /// B4 主题单一真相 —— 主页此前跟随系统主题,深色机器上会画成浅色)。
    theme_synced: Option<bool>,
    /// 06-3:idle / 冷启动探针(VB_FPS_LOG=1;与 VellumApp 的同族钩子互补 ——
    /// 主页根视口由外壳渲染,项目窗口由 VellumApp 渲染,两边各报各的)。
    probe_born: Option<std::time::Instant>,
    /// 06-3:探针当前窗口内已发生的帧数。
    probe_frames: u32,
    /// 已发送过的根窗口标题(避免每帧重复发 Title 命令)。
    root_title_sent: Option<String>,
}

impl ShellApp {
    /// eframe 入口(`main.rs` 调用;根视口构建器按启动模式选择)。
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        // 字体只装一次(全局 ctx;子视口共享,项目窗口不再各自安装)
        let fonts_report = vb_ui::fonts::install(&cc.egui_ctx);
        log::info!("字体安装: {}", fonts_report.summary());

        let (recent, recent_warn) = recent::load();
        if let Some(w) = &recent_warn {
            log::warn!("{w}");
        }
        // 05-4-A2(首选项「外观」页):主题持久化 —— workspace.json 的
        // theme_dark 作为壳的启动主题;VB_THEME 环境变量仍优先(07-I 双主题
        // 截图夹具不受影响)。
        let (ws_cfg, _) = crate::app::dock_layout::load();
        let theme_from_env = std::env::var("VB_THEME").ok().map(|v| v != "light");
        // 02-5-3 写竞争策略:workspace.json 各窗口写(最后写入胜 + 原子替换),
        // recent.json 由外壳单点写 —— 启动时把策略写进日志,不静默。
        log::info!(
            "多窗口配置写策略:recent.json 单写入者(外壳);workspace.json 最后写入胜(原子替换)"
        );

        let (tx, rx) = std::sync::mpsc::channel();
        let home_at_root = !launch.is_direct_project();
        let mut shell = ShellApp {
            tx: tx.clone(),
            rx,
            // 默认深色;VB_THEME=light 环境变量整壳切浅色(07-I 双主题截图
            // 夹具,与 VB_UI_SCALE / VB_WORKSPACE 同族,不进 UI 面);
            // 无环境变量时用首选项持久值(05-4-A2)
            theme_dark: theme_from_env.unwrap_or(ws_cfg.theme_dark),
            recent,
            home: LauncherUi::new(home_at_root),
            home_at_root,
            home_id: ViewportId::from_hash_of("vb-home"),
            root: None,
            wins: Vec::new(),
            home_visible: false,
            next_id: 1,
            thumb_jobs: HashSet::new(),
            exit_confirm: false,
            force_quit: false,
            pending_focus: None,
            pending_open: None,
            // H-1:动效总开关随 workspace.json 初始化(主页侧与窗口侧同源)
            motion_enabled: ws_cfg.motion_enabled,
            theme_synced: None,
            root_title_sent: None,
            // 06-3:VB_FPS_LOG=1 探针(未设置时零开销)
            probe_born: std::env::var("VB_FPS_LOG")
                .is_ok()
                .then(std::time::Instant::now),
            probe_frames: 0,
        };

        // 项目直开(Project)与门禁出图(Shot)共用同一条开窗路径
        let proj = match &launch {
            Launch::Project(p) => Some(p.clone()),
            Launch::Shot { project, .. } => Some(project.clone()),
            _ => None,
        };
        if let Some(p) = proj {
            let dir = resolve_project_dir(&p);
            let mut app = match VellumApp::try_open_project(&cc.egui_ctx, &dir) {
                Ok(app) => app,
                Err(e) => {
                    // 兼容旧行为:打开失败不崩,退化为新建文档(脚本侧有 stderr)
                    eprintln!("打开失败:{e};使用新建文档");
                    VellumApp::build(&cc.egui_ctx, None)
                }
            };
            // 03-3:门禁出图模式(隐藏参数)——配置采样后由 tick 自动执行
            if let Launch::Shot { out, artboard, .. } = &launch {
                app.canvas_shot = Some(crate::canvas_shot::CanvasShotCfg::new(
                    out.clone(),
                    artboard.clone(),
                ));
            }
            // 根项目窗口同样接入外壳通道(02-5):「打开项目…」开新窗口而非就地
            // 替换;file.close = 触发退出确认;file.home 可唤出主页;app.quit 先同步
            // 会话;保存也记最近列表。
            app.shell_tx = Some(tx.clone());
            app.viewport_id = ViewportId::ROOT;
            app.theme_dark = shell.theme_dark;
            app.theme_synced = None;
            shell.root = Some(ProjectWindow {
                id: ViewportId::ROOT,
                app,
                confirm_close: false,
                wants_close: false,
            });
            if dir.is_dir() {
                shell.recent.touch(&dir);
                shell.save_recent();
                shell.sync_session();
                shell.spawn_thumb(&dir);
            }
        }
        shell
    }

    // ---------- 帧内流程 ----------

    fn drain_requests(&mut self, ctx: &egui::Context) {
        while let Ok(req) = self.rx.try_recv() {
            match req {
                ShellRequest::OpenProject(dir) => self.defer_or_open(ctx, &dir),
                ShellRequest::CreateProject(spec) => match create_project(&spec) {
                    Ok(dir) => self.open_project(ctx, &dir),
                    Err(e) => self.broadcast_error(format!("新建项目失败:{e}")),
                },
                ShellRequest::CreateFromTemplate {
                    template,
                    location,
                    name,
                } => match create_from_template(&template, &location, &name) {
                    Ok(dir) => self.open_project(ctx, &dir),
                    Err(e) => self.broadcast_error(format!("模板新建失败:{e}")),
                },
                ShellRequest::ShowHome => self.show_home(),
                ShellRequest::CloseWindow(id) => self.request_close(ctx, id),
                ShellRequest::TouchRecent(dir) => {
                    self.recent.touch(&dir);
                    self.save_recent();
                }
                ShellRequest::RemoveRecent(dir) => {
                    self.recent.remove(&dir);
                    self.save_recent();
                }
                ShellRequest::TogglePinRecent(dir) => {
                    self.recent.toggle_pin(&dir);
                    self.save_recent();
                }
                ShellRequest::RestoreSession => {
                    // 02-6-2:一键重开上次会话;失效路径不静默(逐条报错)
                    for dir in self.recent.session.clone() {
                        if !Path::new(&dir).is_dir() {
                            self.broadcast_error(format!("路径已失效,无法恢复:{dir}"));
                            continue;
                        }
                        self.open_project(ctx, Path::new(&dir));
                    }
                }
                ShellRequest::ThemeChanged(dark) => self.broadcast_theme(dark),
                ShellRequest::MotionChanged(enabled) => {
                    self.motion_enabled = enabled;
                }
                ShellRequest::ThumbReady { dir, thumb } => {
                    self.thumb_jobs.remove(&recent::path_key(&dir));
                    self.recent.set_thumb(&dir, &thumb);
                    self.save_recent();
                }
                ShellRequest::QuitAll => {
                    // 07-C:退出进程与关闭根窗口同语义 —— 有未保存改动
                    // 或还有其它窗口时先走退出确认(此前"显式退出不确认"
                    // 会让脏改动绕过 07-C 铁律;确认框里仍可「直接退出」)。
                    // 无牵挂:同步会话后自然关闭。
                    self.begin_exit(ctx);
                }
            }
        }
    }

    /// H-7:打开请求的受理分流 —— 主页可见且目标未开时**推迟一帧**,
    /// 让「正在打开…」占位先呈现;其余(已开聚焦 / 无主页)即时执行。
    fn defer_or_open(&mut self, _ctx: &egui::Context, raw: &Path) {
        let dir = resolve_project_dir(raw);
        let home_visible = self.home_at_root || self.home_visible;
        if home_visible && !self.is_project_open(&dir) {
            let name = crate::recent::display_name(&dir);
            self.home.opening = Some(name);
            self.pending_open = Some(PendingOpen::Armed(dir));
            return;
        }
        self.open_project(_ctx, &dir);
    }

    /// 项目是否已有窗口(含根窗口;键比较同 `open_project`)。
    fn is_project_open(&self, dir: &Path) -> bool {
        let key = recent::path_key(dir);
        self.wins.iter().any(|w| {
            w.project_dir()
                .map(recent::path_key)
                .is_some_and(|k| k == key)
        }) || self
            .root
            .as_ref()
            .and_then(|r| r.project_dir())
            .map(recent::path_key)
            .is_some_and(|k| k == key)
    }

    /// 单一打开入口(副文档 02 §6:--project 与新流程统一收敛到这里):
    /// 已打开 → 聚焦;否则导入 → 新窗口 → 记最近 + 会话 + 后台缩略图。
    fn open_project(&mut self, ctx: &egui::Context, raw: &Path) {
        let dir = resolve_project_dir(raw);
        if !dir.is_dir() {
            self.broadcast_error(format!("路径不存在:{}", dir.display()));
            return;
        }
        let key = recent::path_key(&dir);
        // 02-5-5:同项目重复打开 → 聚焦已有窗口,不重复开
        if let Some(w) = self
            .wins
            .iter()
            .find(|w| w.project_dir().map(recent::path_key).as_deref() == Some(&key))
        {
            let id = w.id;
            self.recent.touch(&dir);
            self.save_recent();
            self.pending_focus = Some(id);
            return;
        }
        if let Some(root) = &self.root {
            if root.project_dir().map(recent::path_key).as_deref() == Some(&key) {
                self.recent.touch(&dir);
                self.save_recent();
                self.pending_focus = Some(ViewportId::ROOT);
                return;
            }
        }

        match VellumApp::try_open_project(ctx, &dir) {
            Ok(mut app) => {
                let id = ViewportId::from_hash_of(format!("vb-proj-{}", self.next_id));
                self.next_id += 1;
                app.shell_tx = Some(self.tx.clone());
                app.viewport_id = id;
                app.theme_dark = self.theme_dark;
                app.theme_synced = None; // 强制向 egui 同步一次
                self.wins.push(ProjectWindow {
                    id,
                    app,
                    confirm_close: false,
                    wants_close: false,
                });
                self.recent.touch(&dir);
                self.save_recent();
                self.sync_session();
                self.spawn_thumb(&dir);
                // 视口尚未创建,Focus 命令推迟到它渲染的那一帧
                self.pending_focus = Some(id);
            }
            Err(e) => {
                log::warn!("打开项目失败:{e}");
                self.broadcast_error(format!("打开失败:{e}"));
            }
        }
    }

    fn show_home(&mut self) {
        if self.home_at_root {
            self.pending_focus = Some(ViewportId::ROOT);
        } else {
            self.home_visible = true;
            self.home.open = true;
            self.pending_focus = Some(self.home_id);
        }
    }

    /// 关闭请求(来自窗口的 file.close 命令):干净窗口直接关,脏窗口走确认。
    fn request_close(&mut self, ctx: &egui::Context, id: ViewportId) {
        if let Some(w) = self.wins.iter_mut().find(|w| w.id == id) {
            if w.app.is_dirty() {
                w.confirm_close = true;
            } else {
                w.wants_close = true;
                ctx.send_viewport_cmd_to(id, ViewportCommand::CancelClose);
            }
            return;
        }
        let is_root = self
            .root
            .as_ref()
            .map(|r| r.id == id)
            .unwrap_or(id == ViewportId::ROOT);
        if is_root {
            self.begin_exit(ctx);
        }
    }

    /// 根视口关闭(主页或根项目窗口)= 退出进程;有别的窗口或未保存改动时先确认。
    fn begin_exit(&mut self, _ctx: &egui::Context) {
        // 06-4 注入验证实测:03-3 出图通道(--canvas-shot)是门禁无头通道,
        // 采完必须直达退出 —— 若走 07-C 确认框,门禁进程会挂在对话框上
        // (主页子视口使 child_count > 0,必触发确认)。
        let shot_mode = self
            .root
            .as_ref()
            .is_some_and(|r| r.app.canvas_shot.is_some());
        if !shot_mode && (self.child_count() > 0 || self.any_dirty()) {
            self.exit_confirm = true;
        } else {
            // 无牵挂:同步会话后关闭。QuitAll 没有附带 OS 关闭事件,
            // force_quit 只是放行标志 —— 必须主动发 Close,根视口才会
            // 真正退出(与退出确认框「直接退出」同款;canvas-shot 门禁
            // 通道实测踩过:缺这行进程永退)。
            self.sync_session();
            self.save_recent();
            self.force_quit = true;
            _ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
        }
    }

    fn child_count(&self) -> usize {
        self.wins.len()
            + if self.home_visible && !self.home_at_root {
                1
            } else {
                0
            }
    }

    fn any_dirty(&self) -> bool {
        self.root
            .as_ref()
            .map(|r| r.app.is_dirty())
            .unwrap_or(false)
            || self.wins.iter().any(|w| w.app.is_dirty())
    }

    /// 保存所有脏窗口(退出确认里的「保存全部并退出」)。
    fn save_all_dirty(&mut self) {
        if let Some(root) = &mut self.root {
            if root.app.is_dirty() {
                root.app.save_project();
            }
        }
        for w in &mut self.wins {
            if w.app.is_dirty() {
                w.app.save_project();
            }
        }
    }

    /// 把当前打开的项目集合写进 recent.json 的 session 字段(02-6-1)。
    fn sync_session(&mut self) {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(root) = &self.root {
            if let Some(d) = root.project_dir() {
                dirs.push(d.to_path_buf());
            }
        }
        for w in &self.wins {
            if let Some(d) = w.project_dir() {
                dirs.push(d.to_path_buf());
            }
        }
        self.recent.set_session(&dirs);
        self.save_recent();
    }

    fn save_recent(&mut self) {
        if let Err(e) = self.recent.save() {
            log::warn!("{e}");
        }
    }

    /// 错误就近可见:主页开着就显示在主页状态行;同时必进日志(不静默)。
    fn broadcast_error(&mut self, msg: String) {
        log::warn!("{msg}");
        self.home.message = Some((msg, std::time::Instant::now()));
    }

    fn broadcast_theme(&mut self, dark: bool) {
        self.theme_dark = dark;
        self.theme_synced = None; // 强制下一帧向 egui 重新同步
        if let Some(root) = &mut self.root {
            root.app.set_theme_dark(dark);
        }
        for w in &mut self.wins {
            w.app.set_theme_dark(dark);
        }
    }

    // ---------- 缩略图(02-2-5:后台生成,失败占位,不阻塞打开) ----------

    fn spawn_thumb(&mut self, dir: &Path) {
        let key = recent::path_key(dir);
        if self.thumb_jobs.contains(&key) {
            return;
        }
        let out = dir.join(".vb-cache").join("thumb.png");
        if out.is_file() {
            // 已有缩略图:只补链接(缓存可安全删除,删除后走重新生成)
            self.recent.set_thumb(dir, &out);
            self.save_recent();
            return;
        }
        // 从刚打开的窗口取文档快照(NodeId 只在克隆出的 arena 内使用)
        let win = self
            .wins
            .iter()
            .find(|w| w.project_dir().map(recent::path_key).as_deref() == Some(&key))
            .or_else(|| {
                self.root
                    .as_ref()
                    .filter(|r| r.project_dir().map(recent::path_key).as_deref() == Some(&key))
            });
        let Some(win) = win else { return };
        let doc: Document = win.app.doc.clone();
        let Some(&ab) = doc.artboards.first() else {
            return;
        };
        let w = doc.nodes.get(ab).map(|n| n.geom.w).unwrap_or(1000.0);
        self.thumb_jobs.insert(key);
        let tx = self.tx.clone();
        let dir = dir.to_path_buf();
        // 取第一个画板,长边压到 ~480px;CPU 渲染在后台线程,失败只留占位
        std::thread::spawn(move || {
            let scale = (480.0 / w.max(1.0)).clamp(0.05, 1.0) as f32;
            if let Some(parent) = out.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match vb_export::export_artboard_png(&doc, ab, scale, false, Some(&dir)) {
                Ok((png, _warnings)) => {
                    if std::fs::write(&out, &png).is_ok() {
                        let _ = tx.send(ShellRequest::ThumbReady {
                            dir: dir.clone(),
                            thumb: out.clone(),
                        });
                    } else {
                        log::warn!("缩略图写入失败:{}", out.display());
                    }
                }
                Err(e) => log::warn!("缩略图生成失败({e});主页将显示占位图"),
            }
        });
    }
}

impl eframe::App for ShellApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // 06-3:VB_FPS_LOG=1 探针(主页根视口侧)。首帧一行「home first-frame」
        // = 冷启动到主页可见(tools/bench.ps1 -Boot 取此数);此后 ≥5s 汇一次
        // 窗口帧数 —— 空闲休眠时静默(无帧无代码),静默即 idle 节流证据。
        if let Some(t0) = self.probe_born {
            self.probe_frames += 1;
            if self.probe_frames == 1 {
                eprintln!("[fps-log] home first-frame {} ms", t0.elapsed().as_millis());
            }
            let win = t0.elapsed().as_secs_f32();
            if win >= 5.0 {
                let n = self.probe_frames;
                self.probe_frames = 0;
                self.probe_born = Some(std::time::Instant::now());
                eprintln!(
                    "[fps-log] home 窗口 {win:.1}s 内 {n} 帧(均 {:.1} fps)",
                    n as f32 / win.max(0.001)
                );
            }
        }
        // 主题单一真相在外壳(B4,与 frame.rs 同口径):theme_dark 是唯一
        // 来源,变化时同步进 egui 偏好 —— 否则主页 ctx.theme() 跟随系统
        // (浅色系统的主页会被画成浅色,与深色令牌错位)。
        if self.theme_synced != Some(self.theme_dark) {
            ctx.set_theme(if self.theme_dark {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            });
            self.theme_synced = Some(self.theme_dark);
        }
        // 主题单一真相在外壳(主页与窗口共用;vb_ui::theme 幂等)。
        // H-1:带动效总开关 —— 主页侧的过渡/toast 同样可关。
        vb_ui::theme::apply_ex(&ctx, self.theme_dark, 1.0, self.motion_enabled);

        self.drain_requests(&ctx);

        // ── H-7:上一帧受理的打开动作,此刻执行(阻塞期间屏幕上
        // 仍是上一帧呈现的「正在打开…」占位,不再是白屏/假死)──
        if let Some(PendingOpen::Fire(dir)) = self.pending_open.clone() {
            self.pending_open = None;
            self.open_project(&ctx, &dir);
            self.home.opening = None;
        }

        // ── 根视口内容 ──
        if self.home_at_root {
            self.render_home_root(ui, &ctx);
        } else if self.root.is_some() {
            // 根项目窗口:直接渲染进根视口(不是子视口)
            handle_drops(ui, &self.tx);
            if let Some(root) = &mut self.root {
                eframe::App::ui(&mut root.app, ui, frame);
            }
            self.handle_root_window_close(&ctx);
            if self.exit_confirm {
                self.render_exit_confirm(ui);
            }
            // 根窗口标题动态更新(02-5-4)
            let title = self.root.as_ref().map(|r| window_title(&r.app));
            if let Some(title) = title {
                if self.root_title_sent.as_deref() != Some(title.as_str()) {
                    ctx.send_viewport_cmd_to(
                        ViewportId::ROOT,
                        ViewportCommand::Title(title.clone()),
                    );
                    self.root_title_sent = Some(title);
                }
            }
        }

        // ── 子项目窗口(immediate viewport:每帧都要调用,停调即关闭) ──
        let tx = self.tx.clone();
        let mut i = 0;
        while i < self.wins.len() {
            let win = &mut self.wins[i];
            let ProjectWindow {
                id,
                app,
                confirm_close,
                wants_close,
            } = win;
            let builder = ViewportBuilder::default()
                .with_title(window_title(app))
                .with_inner_size([1400.0, 900.0])
                .with_min_inner_size([1024.0, 640.0])
                .with_icon(app_icon());
            let frame_ref: &mut eframe::Frame = &mut *frame;
            let tx2 = tx.clone();
            ctx.show_viewport_immediate(*id, builder, |ui, _class| {
                handle_drops(ui, &tx2);
                eframe::App::ui(app, ui, frame_ref);
                // 02-5-6 关闭处理:干净 → 直接关;脏 → 保存/丢弃/取消
                let wctx = ui.ctx();
                let close_requested = wctx.input(|i| i.viewport().close_requested());
                if close_requested && !*confirm_close && !*wants_close {
                    if app.is_dirty() {
                        wctx.send_viewport_cmd(ViewportCommand::CancelClose);
                        *confirm_close = true;
                    } else {
                        *wants_close = true;
                    }
                }
                if *confirm_close {
                    wctx.send_viewport_cmd(ViewportCommand::CancelClose);
                    let mut action = 0u8; // 1=保存并关闭 2=不保存 3=取消
                    egui::Window::new(format!("关闭前保存? — {}", app.display_name()))
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(wctx, |ui| {
                            ui.label(format!("「{}」有未保存的修改。", app.display_name()));
                            // U-7:统一按钮规格 —— 主按钮「保存并关闭」右下
                            let primary = vb_ui::components::dialog_footer_btn3(
                                ui,
                                "保存并关闭",
                                "不保存",
                                "取消",
                            );
                            match primary {
                                (true, _, _) => action = 1,
                                (_, true, _) => action = 2,
                                (_, _, true) => action = 3,
                                _ => {}
                            }
                            // Esc = 取消(U-7 键位统一;不与文本框冲突)
                            if wctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                                action = 3;
                            }
                        });
                    match action {
                        // 保存被取消(未选目录等)→ 留在窗口,不算已关闭
                        1 => {
                            if app.save_project() {
                                *wants_close = true;
                                *confirm_close = false;
                            }
                        }
                        2 => {
                            *wants_close = true;
                            *confirm_close = false;
                        }
                        3 => *confirm_close = false,
                        _ => {}
                    }
                }
            });
            if self.wins[i].wants_close {
                let closed = self.wins.remove(i);
                // 02-2-2:关闭也是"最近项目"的一次写入时机(刷新 last_opened)
                if let Some(d) = closed.project_dir() {
                    self.recent.touch(d);
                    // 07-A:正常关闭 = 会话结束 → 清理该项目快照
                    // (异常退出才留残影;「不保存」关闭同样丢弃,快照只救崩溃)
                    crate::autosave::clear(d);
                }
                self.sync_session();
            } else {
                i += 1;
            }
        }

        // ── project 模式下的主页子视口(可关闭:关闭只是隐藏主页,不退进程) ──
        if !self.home_at_root && self.home_visible {
            let items = self.recent.sorted();
            let session_len = self.recent.session.len();
            let home = &mut self.home;
            let tx2 = tx.clone();
            let mut close_home = false;
            let builder = ViewportBuilder::default()
                .with_title("Vellum Bench — 主页")
                .with_inner_size([1024.0, 680.0])
                .with_min_inner_size([920.0, 600.0])
                .with_icon(home_icon());
            ctx.show_viewport_immediate(self.home_id, builder, |ui, _class| {
                handle_drops(ui, &tx2);
                home.ui(ui, &items, session_len, &tx2);
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    // 不再调用 show_viewport_immediate → 窗口自然消失(02-1"并存+关闭")
                    close_home = true;
                }
            });
            if close_home {
                self.home_visible = false;
                self.home.open = false;
            }
        }

        // ── 待聚焦(视口创建后的下一帧发命令才可靠) ──
        if let Some(id) = self.pending_focus.take() {
            ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
        }

        // ── H-7:Armed → Fire(本帧已渲染占位,下一帧执行打开) ──
        if let Some(PendingOpen::Armed(dir)) = self.pending_open.clone() {
            self.pending_open = Some(PendingOpen::Fire(dir));
        }
    }
}

// ---------- 根视口的两种内容 ----------

impl ShellApp {
    /// 主页 = 根视口(home 启动模式)。
    fn render_home_root(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 拖拽目录/HTML 到主页 = 打开为项目窗口(02-1-3;主页与项目窗口都收)
        handle_drops(ui, &self.tx);
        // 关闭主页 = 退出进程:还有项目窗口或未保存改动时先确认(02-5-6 精神)
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.force_quit && !self.exit_confirm {
            self.begin_exit(ctx);
        }
        let items = self.recent.sorted();
        let session_len = self.recent.session.len();
        let home = &mut self.home;
        home.ui(ui, &items, session_len, &self.tx);
        if self.exit_confirm {
            self.render_exit_confirm(ui);
        }
    }

    /// 根项目窗口的关闭(`--project` 模式)= 退出进程。
    fn handle_root_window_close(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.force_quit && !self.exit_confirm {
            self.begin_exit(ctx);
        }
    }

    /// 退出确认(02-5-6):保存全部并退出 / 直接退出 / 取消。
    fn render_exit_confirm(&mut self, ui: &mut egui::Ui) {
        // 确认期间压制根视口关闭,直到用户做出选择
        ui.ctx()
            .send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::CancelClose);
        let dirty = self.any_dirty();
        let n = self.child_count();
        let mut action = 0u8; // 1=退出 2=保存全部并退出 3=取消
        egui::Window::new("退出 Vellum Bench?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                if n > 0 {
                    ui.label(format!(
                        "关闭{}将同时关闭其余 {n} 个窗口。",
                        if self.home_at_root {
                            "主页"
                        } else {
                            "主窗口"
                        }
                    ));
                } else {
                    ui.label(format!(
                        "关闭{}将退出 Vellum Bench。",
                        if self.home_at_root {
                            "主页"
                        } else {
                            "主窗口"
                        }
                    ));
                }
                if dirty {
                    ui.colored_label(
                        // 数据丢失属告警级,走主题 danger 令牌(同状态栏「外部已改动(未采用)」口径)
                        vb_ui::theme::tokens(ui.ctx()).danger,
                        "有未保存的修改,直接退出将丢失。",
                    );
                }
                // U-7:统一按钮规格 —— 主按钮右下(有未保存改动时 =
                // 「保存全部并退出」,否则 = 「直接退出」)
                let (a1, a2, a3) = if dirty {
                    vb_ui::components::dialog_footer_btn3(ui, "保存全部并退出", "直接退出", "取消")
                } else {
                    vb_ui::components::dialog_footer_btn3(ui, "直接退出", "", "取消")
                };
                if a1 {
                    action = if dirty { 2 } else { 1 };
                }
                if a2 && dirty {
                    action = 1;
                }
                if a3 {
                    action = 3;
                }
                // Esc = 取消(U-7 键位统一)
                if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                    action = 3;
                }
            });
        match action {
            1 => {
                self.sync_session();
                self.save_recent();
                self.force_quit = true;
                ui.ctx()
                    .send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
            }
            2 => {
                self.save_all_dirty();
                self.sync_session();
                self.save_recent();
                self.force_quit = true;
                ui.ctx()
                    .send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
            }
            3 => self.exit_confirm = false,
            _ => {}
        }
    }
}

// ---------- 子视口内的拖拽 ----------

/// 目录 / HTML 拖到窗口 → 打开为项目窗口(02-1-3)。
/// 只认目录与 `.html`/`.htm` 文件;其它类型忽略并记日志(不误开其父目录)。
fn handle_drops(ui: &mut egui::Ui, tx: &Sender<ShellRequest>) {
    let dropped = ui.ctx().input(|i| i.raw.dropped_files.clone());
    let (open, ignored) = classify_drops(&dropped);
    for p in open {
        let _ = tx.send(ShellRequest::OpenProject(p));
    }
    for p in ignored {
        log::warn!("忽略拖入文件(仅支持项目目录/HTML):{}", p.display());
    }
}

/// 拖入文件的打开判定(07-F 核验抽出的纯函数,单测直打):
/// 返回 (应打开的路径, 应忽略的路径)。判定规则 = 目录(须存在)或
/// `.html`/`.htm` 扩展名 → 打开(`resolve_project_dir` 会把 HTML 归到
/// 其父目录);无路径或其它扩展名 → 忽略,不误开。
/// 扩展名**大小写不敏感**(Windows 文件系统语义,`PAGE.HTML` 同样放行)。
fn classify_drops(
    files: &[egui::DroppedFile],
) -> (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    let mut open = Vec::new();
    let mut ignored = Vec::new();
    for f in files {
        let Some(p) = f.path.clone() else { continue };
        let is_html = p
            .extension()
            .map(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
            .unwrap_or(false);
        if p.is_dir() || is_html {
            open.push(p);
        } else {
            ignored.push(p);
        }
    }
    (open, ignored)
}

/// 项目窗口图标:橙色实心圆点(main.rs 原实现上移)。
pub fn app_icon() -> egui::IconData {
    let (w, h) = (16u32, 16u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 7.5;
            let inside = dx * dx + dy * dy <= 36.0;
            let (r, g, b, a) = if inside {
                (255, 90, 31, 255)
            } else {
                (0, 0, 0, 0)
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    egui::IconData {
        width: w,
        height: h,
        rgba,
    }
}

/// 主页图标:橙色空心圆环(与项目窗口的实心圆点区分;02-1-2 图标不同)。
pub fn home_icon() -> egui::IconData {
    let (w, h) = (16u32, 16u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 7.5;
            let d2 = dx * dx + dy * dy;
            let ring = (14.0..=42.0).contains(&d2);
            let (r, g, b, a) = if ring {
                (255, 90, 31, 255)
            } else {
                (0, 0, 0, 0)
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    egui::IconData {
        width: w,
        height: h,
        rgba,
    }
}

/// 组装 eframe 启动(`main.rs` 调用):根视口构建器按启动模式选择(02-1-2)。
pub fn run_native(launch: Launch) -> eframe::Result<()> {
    // 项目窗口(含 03-3 出图模式,同一构建器)与主页兜底两种根视口
    let proj = match &launch {
        Launch::Project(p) => Some(p.clone()),
        Launch::Shot { project, .. } => Some(project.clone()),
        _ => None,
    };
    // 冒烟夹具:VB_WINDOW_SIZE=<宽>x<高> 指定初始窗宽(第四轮 U-2 的
    // 「<1600 次级坞折叠」截图取证用;与 VB_UI_SCALE / VB_THEME 同族,
    // 不进 UI 面)。非法值忽略,回默认尺寸。
    let env_size = std::env::var("VB_WINDOW_SIZE").ok().and_then(|v| {
        let v = v.trim().to_lowercase();
        let (w, h) = v.split_once('x')?;
        Some([w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?])
    });
    let viewport = if let Some(p) = &proj {
        let dir = resolve_project_dir(p);
        let name = dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "未命名".into());
        ViewportBuilder::default()
            .with_inner_size(env_size.unwrap_or([1680.0, 1000.0]))
            .with_min_inner_size([1024.0, 640.0])
            .with_title(format!("{name} — Vellum Bench"))
            .with_icon(app_icon())
    } else {
        // 主页/帮助/版式错误分支不会真的进 GUI(main.rs 先行返回),给主页构建器兜底
        ViewportBuilder::default()
            .with_inner_size([1024.0, 680.0])
            .with_min_inner_size([920.0, 600.0])
            .with_title("Vellum Bench")
            .with_icon(home_icon())
    };
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "Vellum Bench",
        options,
        Box::new(move |cc| Ok(Box::new(ShellApp::new(cc, launch)))),
    )
}

// ─────────────────────────── 单测 ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        std::iter::once("vellumbench.exe")
            .chain(args.iter().copied())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn no_args_opens_home() {
        assert_eq!(parse_launch(&argv(&[])), Launch::Home);
    }

    #[test]
    fn project_flag_takes_directory() {
        let l = parse_launch(&argv(&["--project", r"examples\landing"]));
        assert!(l.is_direct_project());
        assert_eq!(l, Launch::Project(PathBuf::from(r"examples\landing")));
        let l = parse_launch(&argv(&["-p", "/tmp/proj"]));
        assert_eq!(l, Launch::Project(PathBuf::from("/tmp/proj")));
    }

    #[test]
    fn positional_dir_or_html_goes_direct() {
        let l = parse_launch(&argv(&[r"examples\landing"]));
        assert_eq!(l, Launch::Project(PathBuf::from(r"examples\landing")));
        let l = parse_launch(&argv(&["page.html"]));
        assert!(l.is_direct_project(), "HTML 文件也要直达项目窗口");
    }

    #[test]
    fn help_and_version_are_kept() {
        match parse_launch(&argv(&["--help"])) {
            Launch::Help(t) => {
                assert!(t.contains("--project"), "帮助必须提到 --project");
                assert!(t.contains("用法"));
            }
            other => panic!("--help 应返回 Help,得到 {other:?}"),
        }
        match parse_launch(&argv(&["-h"])) {
            Launch::Help(_) => {}
            other => panic!("-h 应返回 Help,得到 {other:?}"),
        }
        match parse_launch(&argv(&["--version"])) {
            Launch::Version(t) => assert!(t.contains("Vellum Bench")),
            other => panic!("--version 应返回 Version,得到 {other:?}"),
        }
    }

    #[test]
    fn project_without_value_is_a_loud_error() {
        match parse_launch(&argv(&["--project"])) {
            Launch::Error(t) => assert!(t.contains("--project"), "错误信息要可读:{t}"),
            other => panic!("--project 缺值应报错,得到 {other:?}"),
        }
    }

    #[test]
    fn unknown_flags_do_not_block_launch() {
        assert_eq!(parse_launch(&argv(&["--whatever"])), Launch::Home);
    }

    #[test]
    fn html_file_resolves_to_parent_dir() {
        // 文件必须真实存在才走"取父目录"分支
        let base = std::env::temp_dir().join("vb-shell-x");
        let _ = std::fs::create_dir_all(&base);
        let f = base.join("index.html");
        std::fs::write(&f, b"<html></html>").unwrap();
        let dir = resolve_project_dir(&f);
        assert_eq!(dir, base, "index.html → 其父目录");
        let d = resolve_project_dir(&std::env::temp_dir());
        assert_eq!(d, std::env::temp_dir(), "目录原样通过");
        // 不存在的路径原样返回(由打开流程显式报"路径不存在",不静默)
        let ghost = std::env::temp_dir().join("vb-ghost-xyz").join("index.html");
        assert_eq!(resolve_project_dir(&ghost), ghost);
        let _ = std::fs::remove_file(&f);
    }

    // ── 07-F 核验:拖拽打开的判定层(目录与 .html 都能进打开流程) ──

    fn drop_file(p: Option<std::path::PathBuf>) -> egui::DroppedFile {
        egui::DroppedFile {
            path: p,
            ..egui::DroppedFile::default()
        }
    }

    /// 目录与 .html/.htm 拖入 = 打开;.txt 等其它类型与无路径拖入 = 忽略。
    #[test]
    fn drops_accept_dirs_and_html_only() {
        let base = std::env::temp_dir().join(format!("vb-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let dir = base.join("proj");
        std::fs::create_dir_all(&dir).unwrap();
        let page = base.join("page.htm");
        std::fs::write(&page, b"<html></html>").unwrap();
        let upper = base.join("UPPER.HTML");
        std::fs::write(&upper, b"<html></html>").unwrap();

        let dropped = vec![
            drop_file(Some(dir.clone())),        // 目录 → 开
            drop_file(Some(page.clone())),       // .htm → 开
            drop_file(Some(upper.clone())),      // .HTML(大写)→ 开
            drop_file(Some(base.join("x.txt"))), // 其它扩展名 → 忽略
            drop_file(None),                     // 无真实路径(内存拖放)→ 静默跳过
        ];
        let (open, ignored) = classify_drops(&dropped);
        assert_eq!(open, vec![dir, page, upper], "目录与 html 变体都放行");
        assert_eq!(ignored.len(), 1, "非 html 文件被忽略(无路径拖入不入任一列)");

        // 空拖放 = 无动作
        let (open, ignored) = classify_drops(&[]);
        assert!(open.is_empty() && ignored.is_empty());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
