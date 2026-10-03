//! `vellum-sable` — VellumBench 新宿主(R0,22 篇迭代方案 §4 R0 交付 1)。
//!
//! 启动流程(ADR-0033 语义):
//! - 无参数 → **启动器窗口**:只读 MRU 列表(缩略图占位框 + 名称 + 相对
//!   时间)+ 搜索过滤 + ↑↓/Enter 键盘导航 + 单击打开 + R 移除记录;
//! - `--project <目录>`(或位置参数)→ **直达项目窗口**,不过启动器;
//! - `--help` / `--version` 保留,脚本与 CI 用法不破坏。
//!
//! 打开项目动作(R0 口径):新开一个与骨架等价的项目窗口(能力台账 +
//! 画布占位),窗口标题 = 项目名,并把所选路径打印到 stdout(冒烟断言用)。
//! 打开即记录 MRU(02-2-2:置顶 + 落盘;持久化格式见 `vb_session::mru`)。
//!
//! 启动器纯逻辑(过滤/排序/导航状态机)在 [`vb_shell::launcher`](lib),
//! 本文件只做绘制与动作执行;MRU 数据 100% 来自 `vb_session::mru`(G-UI7:
//! 壳不持真相,禁依赖 vb_app)。
//!
//! 生命周期:窗口关闭即退出(gpui 默认:最后一个窗口关闭即结束 run 循环);
//! panic 不静默吞 —— 开窗失败 `expect` fail-fast,与 cutforge 同口径。

use std::path::{Path, PathBuf};

use sable::dock::{SablePanel, WorkspacePresets};
use sable::gpui::{
    div, px, size, App, AppContext as _, Application, Bounds, Context, Entity, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, WindowBounds,
    WindowOptions,
};
use sable::gpui_component::dock::DockArea;
use sable::gpui_component::input::{Input, InputEvent, InputState};
use sable::gpui_component::theme::Theme;
use sable::gpui_component::{Root, ThemeMode};
use sable::widgets::prelude::{h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::tokens::FONT_SIZE_BODY;

use vb_kit::capabilities_panel::CapabilitiesPanel;
use vb_kit::tokens::inject_vb_theme;
use vb_session::mru::{self, RecentStore};
use vb_shell::launcher::{view_indices, LauncherModel};

fn main() {
    match parse_launch() {
        Launch::Help(text) | Launch::Version(text) => println!("{text}"),
        Launch::Error(text) => {
            eprintln!("{text}");
            std::process::exit(2);
        }
        Launch::Launcher => Application::new().run(|cx: &mut App| {
            init_chrome(cx);
            open_launcher_window(cx);
            cx.activate(true);
        }),
        Launch::Project(dir) => {
            if !dir.is_dir() {
                eprintln!("vellum-sable: 项目目录不存在:{}", dir.display());
                std::process::exit(2);
            }
            Application::new().run(move |cx: &mut App| {
                init_chrome(cx);
                open_project_window(&dir, cx);
                cx.activate(true);
            });
        }
    }
}

// ─────────────────────────── 命令行(ADR-0033) ───────────────────────────

/// 启动模式。`--project` / 位置参数直达项目窗口;缺省 = 启动器窗口。
enum Launch {
    Launcher,
    Project(PathBuf),
    Help(String),
    Version(String),
    Error(String),
}

fn parse_launch() -> Launch {
    const HELP: &str = "vellum-sable — VellumBench 新宿主预览壳(R0)\n\
         用法:\n  \
         vellum-sable                    先开启动器窗口(只读最近项目列表)\n  \
         vellum-sable --project <目录>   直达项目窗口(跳过启动器,ADR-0033)\n  \
         vellum-sable <项目目录>         同 --project(位置参数)\n  \
         vellum-sable --help | --version";
    let mut args = std::env::args().skip(1);
    let mut project: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Launch::Help(HELP.into()),
            "--version" | "-V" => {
                return Launch::Version(format!("vellum-sable {}", env!("CARGO_PKG_VERSION")))
            }
            "--project" => match args.next() {
                Some(p) if !p.trim().is_empty() => project = Some(PathBuf::from(p)),
                _ => return Launch::Error("vellum-sable: --project 需要一个项目目录参数".into()),
            },
            _ => {
                if let Some(p) = arg.strip_prefix("--project=") {
                    if p.trim().is_empty() {
                        return Launch::Error(
                            "vellum-sable: --project 需要一个项目目录参数".into(),
                        );
                    }
                    project = Some(PathBuf::from(p));
                } else if arg.starts_with('-') {
                    return Launch::Error(format!("vellum-sable: 未知参数 {arg}(--help 查看用法)"));
                } else {
                    // 位置参数:项目目录(ADR-0033;拖拽/关联文件 R2 接入)
                    project = Some(PathBuf::from(arg));
                }
            }
        }
    }
    match project {
        Some(p) => Launch::Project(p),
        None => Launch::Launcher,
    }
}

// ─────────────────────────── 窗口编排 ───────────────────────────

/// 两套主题全局各自初始化(与 cutforge 同口径):sable::dock::init 已含
/// gpui_component::init 与 sable theme::init,只调后者会丢 sable 主题;
/// gpui-component 面板 chrome 固定深色;VB 调色板灌进 sable tokens
/// (JSON 单一真相的投影,tokens_sync2 门禁)。
fn init_chrome(cx: &mut App) {
    sable::dock::init(cx);
    Theme::change(ThemeMode::Dark, None, cx);
    inject_vb_theme(cx);
}

fn window_options(title: impl Into<SharedString>, w: f32, h: f32, cx: &mut App) -> WindowOptions {
    let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(sable::gpui::TitlebarOptions {
            title: Some(title.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// 启动器窗口(缺省入口;ADR-0033「主页窗口」的新宿主形态)。
fn open_launcher_window(cx: &mut App) {
    let options = window_options("VellumBench · 启动器", 720., 560., cx);
    cx.open_window(options, |window, cx| {
        let launcher = LauncherView::new(window, cx);
        cx.new(|cx| Root::new(launcher, window, cx))
    })
    .expect("vellum-sable 启动器开窗失败:GPUI 平台层初始化异常(显卡驱动/显示服务)");
}

/// 打开项目窗口(R0 口径:与骨架等价的工作台;窗口标题 = 项目名)。
///
/// 打开即记录 MRU(02-2-2)+ 落盘(失败如实上 stderr,不静默);所选路径
/// 打印到 stdout(`opened project: <规范化路径>`,冒烟可断言)。
fn open_project_window(dir: &Path, cx: &mut App) {
    let (mut store, warn) = mru::load();
    if let Some(w) = warn {
        eprintln!("recent.json 告警:{w}");
    }
    store.touch(dir);
    if let Err(e) = store.save() {
        eprintln!("{e}");
    }

    let name = mru::display_name(dir);
    println!("opened project: {}", mru::normalize_path(dir));

    let options = window_options(name, 1280., 800., cx);
    cx.open_window(options, |window, cx| {
        let shell = ShellApp::new(window, cx);
        cx.new(|cx| Root::new(shell, window, cx))
    })
    .expect("vellum-sable 项目窗口开窗失败:GPUI 平台层初始化异常(显卡驱动/显示服务)");
}

// ─────────────────────────── 启动器窗口视图 ───────────────────────────

/// 启动器窗口(22 篇 §4 R0 交付 1:只读 MRU 列表)。
///
/// 状态与语义全部投影自 [`vb_shell::launcher`](纯函数状态机)与
/// [`vb_session::mru`](存储),本视图只做绘制与动作执行(打开 / 移除 /
/// 落盘)。缩略图为占位框(R2 接真实缓存);R 键 = 从列表移除记录
/// (只删记录,不动磁盘)。
struct LauncherView {
    model: LauncherModel,
    store: RecentStore,
    search_input: Entity<InputState>,
    /// 根视图焦点(键盘导航的挂点:on_key_down 沿焦点链冒泡到根)。
    focus: FocusHandle,
    /// 搜索框焦点句柄克隆(键盘派发的 editing 判定,与旧宿主同语义:
    /// 搜索框聚焦时 ↑↓/R 不做导航/移除)。
    search_focus: FocusHandle,
    load_warning: Option<String>,
    _search_sub: Option<sable::gpui::Subscription>,
}

impl LauncherView {
    fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let (store, load_warning) = mru::load();
        let focus = cx.focus_handle();
        // 初始焦点落根视图:↑↓/Enter/R 无需先点窗口即可用
        window.focus(&focus);
        let search_input = cx.new(|cx| InputState::new(window, cx).placeholder("搜索名称或路径…"));
        let search_focus = search_input.read(cx).focus_handle(cx);
        let launcher = cx.new(|_| Self {
            model: LauncherModel::new(),
            store,
            search_input: search_input.clone(),
            focus,
            search_focus,
            load_warning,
            _search_sub: None,
        });
        // 搜索词变更投影进状态机(选中回零);Enter 走键冒泡(见 render)
        launcher.update(cx, |this, cx| {
            let sub = cx.subscribe(
                &this.search_input,
                |this, _: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>| {
                    if matches!(event, InputEvent::Change) {
                        let q = this.search_input.read(cx).value().to_string();
                        this.model.set_search(q);
                        cx.notify();
                    }
                },
            );
            this._search_sub = Some(sub);
        });
        launcher
    }

    /// 键盘导航(旧宿主 02-3-4 语义平移):↑↓ 移动、Enter 打开选中
    /// (搜索框聚焦时 Enter 仍打开,与旧宿主一致)、R 移除记录。
    /// 挂在根视图 on_key_down(焦点链冒泡),编辑豁免在此判定。
    fn handle_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let editing = self.search_focus.is_focused(window);
        let plain = !ev.keystroke.modifiers.modified();
        let view = view_indices(&self.store, &self.model.search);
        match key {
            "down" if !editing => {
                self.model.select_next(view.len());
                cx.notify();
            }
            "up" if !editing => {
                self.model.select_prev();
                cx.notify();
            }
            "enter" if plain && !ev.is_held => self.open_selected(cx),
            "r" if plain && !ev.is_held && !editing => self.remove_selected(cx),
            _ => {}
        }
    }

    /// Enter / 「打开」:打开当前选中项(空表/越界 = 无动作)。
    fn open_selected(&mut self, cx: &mut Context<Self>) {
        let view = view_indices(&self.store, &self.model.search);
        if let Some(item) = self.model.selected_item(&view, &self.store) {
            let dir = PathBuf::from(item.path.clone());
            self.open_project(&dir, cx);
        }
    }

    /// 打开项目:记录 MRU + 落盘 + 新开项目窗口(见 `open_project_window`)。
    fn open_project(&mut self, dir: &Path, cx: &mut Context<Self>) {
        self.store.touch(dir);
        if let Err(e) = self.store.save() {
            eprintln!("{e}");
        }
        cx.notify();
        open_project_window(dir, cx);
    }

    /// R / 「移除记录」(02-2-3:只删记录,不动磁盘;不静默丢 —— 有落盘,
    /// 失败上 stderr)。
    fn remove_selected(&mut self, cx: &mut Context<Self>) {
        let view = view_indices(&self.store, &self.model.search);
        let Some(item) = self.model.selected_item(&view, &self.store) else {
            return;
        };
        let dir = PathBuf::from(item.path.clone());
        self.store.remove(&dir);
        if let Err(e) = self.store.save() {
            eprintln!("{e}");
        }
        self.model.clamp(view.len().saturating_sub(1));
        cx.notify();
    }

    /// 一行 MRU 卡片(缩略图占位框 96×54 + 名称/路径 + 相对时间)。
    /// 失效目录置灰仍列出(02-2-4:不静默丢),不可打开、可移除。
    fn row(&self, row: usize, idx: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let item = &self.store.items[idx];
        let stale = item.is_stale();
        let selected = row == self.model.selected;
        let name = if item.pinned {
            format!("★ {}", item.name)
        } else {
            item.name.clone()
        };
        let path = item.path.clone();
        let path_for_click = path.clone();
        let when = mru::relative_time(item.last_opened, mru::now_secs());
        let initial = item.name.chars().next().unwrap_or('?').to_string();
        let colors = theme(cx).colors;
        let (name_color, text_color) = if stale {
            (colors.text_disabled, colors.text_disabled)
        } else {
            (colors.text_primary, colors.text_secondary)
        };
        let (bg, border) = if selected {
            (colors.accent_muted, colors.accent)
        } else {
            (colors.surface_1, colors.border_subtle)
        };
        let hover_bg = colors.surface_3;

        h_flex()
            .id(("mru-row", idx))
            .w_full()
            .p(px(6.0))
            .gap(px(10.0))
            .items_center()
            .rounded(px(6.0))
            .bg(bg)
            .border_1()
            .border_color(border)
            .hover(move |s| s.bg(hover_bg))
            .on_click(
                cx.listener(move |this, _: &sable::gpui::ClickEvent, _, cx| {
                    this.model.selected = row;
                    if !stale {
                        let dir = PathBuf::from(path_for_click.clone());
                        this.open_project(&dir, cx);
                    }
                    cx.notify();
                }),
            )
            // 缩略图占位框(R2 接真实缩略图缓存;先证明布局与命中区)
            .child(
                div()
                    .w(px(96.0))
                    .h(px(54.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .bg(colors.surface_2)
                    .border_1()
                    .border_color(colors.border_subtle)
                    .text_size(px(22.0))
                    .text_color(colors.text_disabled)
                    .child(initial),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.0))
                    .child(
                        h_flex()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_size(px(FONT_SIZE_BODY))
                                    .font_weight(sable::gpui::FontWeight::MEDIUM)
                                    .text_color(name_color)
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
                                    .text_color(colors.warning)
                                    .child("路径已失效".to_string()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(text_color)
                            .truncate()
                            .child(path),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(colors.text_disabled)
                    .child(when),
            )
    }
}

impl Render for LauncherView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = view_indices(&self.store, &self.model.search);
        self.model.clamp(view.len());

        let colors = theme(cx).colors;

        // 中央列表(可滚轮;空存储/空过滤各有空态)
        let mut list = v_flex()
            .id("mru-list")
            .flex_1()
            .overflow_y_scroll()
            .px(px(16.0))
            .gap(px(6.0));
        if self.store.items.is_empty() {
            list = list.child(
                v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap(px(6.0))
                    .child(
                        div()
                            .text_size(px(16.0))
                            .text_color(colors.text_primary)
                            .child("还没有项目"),
                    )
                    .child(
                        div()
                            .text_size(px(FONT_SIZE_BODY))
                            .text_color(colors.text_disabled)
                            .child("用 vellum-sable --project <目录> 打开第一个项目;打开后自动进入最近列表"),
                    ),
            );
        } else if view.is_empty() {
            list = list.child(
                v_flex().size_full().items_center().justify_center().child(
                    div()
                        .text_size(px(FONT_SIZE_BODY))
                        .text_color(colors.text_secondary)
                        .child(format!("没有匹配「{}」的项目", self.model.search)),
                ),
            );
        } else {
            for (row, &idx) in view.iter().enumerate() {
                list = list.child(self.row(row, idx, cx));
            }
        }

        // 根视图挂键盘:↑↓ 导航、Enter 打开、R 移除(02-3-4 语义平移)。
        // on_key_down 沿焦点链冒泡——根视图 track_focus 且持焦点;搜索框
        // (子树)聚焦时事件同样冒泡到这里,编辑豁免在 handle_key 内判定。
        v_flex()
            .id("launcher-root")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                this.handle_key(ev, window, cx);
            }))
            .size_full()
            .bg(colors.surface_0)
            .text_color(colors.text_primary)
            // 顶部:标识 + 版本 + 搜索
            .child(
                h_flex()
                    .w_full()
                    .px(px(16.0))
                    .pt(px(12.0))
                    .pb(px(8.0))
                    .gap(px(10.0))
                    .items_center()
                    .child(
                        div()
                            .text_size(px(16.0))
                            .font_weight(sable::gpui::FontWeight::MEDIUM)
                            .child("VellumBench"),
                    )
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.text_disabled)
                            .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(div().flex_1())
                    .child(Input::new(&self.search_input).w(px(280.0))),
            )
            .child(list)
            // 底部:键位提示 + 读取告警(损坏回退等,如实展示不静默)
            .child(
                h_flex()
                    .w_full()
                    .px(px(16.0))
                    .py(px(8.0))
                    .gap(px(12.0))
                    .items_center()
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.text_disabled)
                            .child("↑↓ 选择 · Enter 打开 · R 移除记录 · 单击行打开"),
                    )
                    .child(div().flex_1())
                    .children(self.load_warning.clone().map(|w| {
                        div()
                            .text_size(px(11.0))
                            .text_color(colors.warning)
                            .truncate()
                            .child(w)
                    })),
            )
    }
}

// ─────────────────────────── 项目窗口(R0 骨架,原样保留) ───────────────────────────

/// 项目窗口根视图:全屏 dock 工作台(左坞 = 能力台账,中央 = 画布(R1) 占位)。
struct ShellApp {
    dock: Entity<DockArea>,
    focus: FocusHandle,
}

impl ShellApp {
    fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let focus = cx.focus_handle();

        // 左坞:能力台账(「面板 = 纯投影」示范;数据在 vb_session)
        let capabilities = SablePanel::create("能力台账", CapabilitiesPanel::new(cx).into(), cx);
        // 中央:画布占位(R1 接管;R0 先立窗口骨架与 dock 布局)
        let canvas = SablePanel::create("画布(R1)", cx.new(|_| CanvasPlaceholder).into(), cx);

        let dock = WorkspacePresets::build_workspace(
            "vellum-sable",
            vec![capabilities],
            canvas,
            vec![],
            window,
            cx,
        );

        cx.new(|_| Self { dock, focus })
    }
}

impl Focusable for ShellApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ShellApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        div()
            .id("vellum-sable-root")
            .size_full()
            .bg(colors.surface_0)
            .text_color(colors.text_primary)
            .track_focus(&self.focus)
            .child(self.dock.clone())
    }
}

/// 画布占位视图(R1 换血;R0 只证明窗口/主题/dock 通道可用)。
struct CanvasPlaceholder;

impl Render for CanvasPlaceholder {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .bg(colors.surface_0)
            .child(
                h_flex().child(
                    div()
                        .text_size(px(FONT_SIZE_BODY))
                        .text_color(colors.text_secondary)
                        .child("画布(R1)— 画布上屏通道按 ADR-0047 裁定后接管"),
                ),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(colors.text_disabled)
                    .child("R0 预览宿主:窗口 / 主题 / dock 布局骨架"),
            )
    }
}
