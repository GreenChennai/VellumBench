//! `vellum-sable` — VellumBench 新宿主(R0,22 篇迭代方案 §4 R0 交付 1)。
//!
//! 启动流程(ADR-0033 语义):
//! - 无参数 → **启动器窗口**:只读 MRU 列表(缩略图占位框 + 名称 + 相对
//!   时间)+ 搜索过滤 + ↑↓/Enter 键盘导航 + 单击打开 + R 移除记录;
//! - `--project <目录>`(或位置参数)→ **直达项目窗口**,不过启动器;
//! - `--help` / `--version` 保留,脚本与 CI 用法不破坏。
//!
//! 打开项目动作(R1 口径):新开一个 Sable Dock 工作台(图层树 + 文档画布),
//! 窗口标题 = 项目名,并把所选路径打印到 stdout(冒烟断言用)。
//! 打开即记录 MRU(02-2-2:置顶 + 落盘;持久化格式见 `vb_session::mru`)。
//!
//! 启动器纯逻辑(过滤/排序/导航状态机)在 [`vb_shell::launcher`](lib),
//! 本文件只做绘制与动作执行;MRU 数据 100% 来自 `vb_session::mru`(G-UI7:
//! 壳不持真相,禁依赖 vb_app)。
//!
//! 生命周期:窗口关闭即退出(gpui 默认:最后一个窗口关闭即结束 run 循环);
//! panic 不静默吞 —— 开窗失败 `expect` fail-fast,与 cutforge 同口径。

use std::path::{Path, PathBuf};
use std::rc::Rc;

mod export;

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
use sable::widgets::prelude::{h_flex, v_flex, Button, ButtonSize, ButtonVariant};
use sable::widgets::theme::theme;
use sable::widgets::tokens::FONT_SIZE_BODY;
use vb_platform::gpui_backend::RfdDialog;
use vb_platform::{DarkModeProbe as _, FileDialog as _, FileFilter};
use vb_session::i18n::{t, t_args};

use vb_kit::animation_panel::ProjectAnimationPanel;
use vb_kit::canvas_panel::CanvasPanel;
use vb_kit::inspector_panel::ProjectInspectorPanel;
use vb_kit::layers_panel::ProjectLayersPanel;
use vb_kit::tokens::inject_vb_theme;
use vb_session::mru::{self, RecentItem, RecentStore};
use vb_session::project::ProjectSession;
use vb_shell::launcher::{view_indices, LauncherModel};

/// 面板坞折叠规则(纯函数;ADR-0050 裁决二,规则表与单测在模块内)。
/// R0 只落规则+锁测,接进 DockArea 事件流(窗口 resize → 折叠切换)属
/// R2 面板批次首项,故暂有 dead_code 允许,接线时移除。
#[allow(dead_code)]
mod dock_rules;

/// 窗口/dock 标题取词(G-UI3:渲染路径禁裸文案)。
///
/// R0 已知限制:sable `SablePanel::create` 与 `WindowOptions` 标题收
/// `&'static str`,故取词结果 leak 成 'static —— 标题随窗口构建一次性
/// 定格,热切换语言不改已开面板标题;R2 面板批次接 DockArea 状态重建时
/// 一并收口(届时面板标题可重建)。
fn title(key: &str) -> &'static str {
    Box::leak(t(key).into_boxed_str())
}

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
                eprintln!("{}", t("ui-shell-project-dir-missing"));
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
    fn usage_text() -> String {
        t("ui-shell-usage")
    }
    let mut args = std::env::args().skip(1);
    let mut project: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Launch::Help(usage_text()),
            "--version" | "-V" => {
                return Launch::Version(format!("vellum-sable {}", env!("CARGO_PKG_VERSION")))
            }
            "--project" => match args.next() {
                Some(p) if !p.trim().is_empty() => project = Some(PathBuf::from(p)),
                _ => return Launch::Error(t("ui-shell-project-arg-required")),
            },
            _ => {
                if let Some(p) = arg.strip_prefix("--project=") {
                    if p.trim().is_empty() {
                        return Launch::Error(t("ui-shell-project-arg-required"));
                    }
                    project = Some(PathBuf::from(p));
                } else if arg.starts_with('-') {
                    return Launch::Error(t_args(
                        "ui-shell-arg-unknown",
                        &[("arg", arg.as_str().into())],
                    ));
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
///
/// 硬骨头 #13 接线:gpui_component 主题档经 [`vb_platform::DarkModeProbe`]
/// 探测系统外观(壳不再自己摸 OS),探测语义在 vb_platform::gpui_backend。
fn init_chrome(cx: &mut App) {
    sable::dock::init(cx);
    let dark = vb_platform::gpui_backend::GpuiDarkMode::new(cx).is_dark_mode();
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );
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
    let options = window_options(title("ui-shell-launcher-title"), 720., 560., cx);
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
    let project = ProjectSession::open(dir).map_err(|error| error.to_string());
    if let Err(error) = &project {
        eprintln!("项目导入失败: {error}");
    }

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
    cx.open_window(options, move |window, cx| {
        let shell = ShellApp::new(window, cx, project);
        cx.new(|cx| Root::new(shell, window, cx))
    })
    .expect("vellum-sable 项目窗口开窗失败:GPUI 平台层初始化异常(显卡驱动/显示服务)");
}

// ─────────────────────────── 启动器窗口视图 ───────────────────────────

/// 启动器文件命令快捷键;键值由 `commands.yaml` 管理并由测试锁定。
fn launcher_command_for_key(
    key: &str,
    platform: bool,
    shift: bool,
    alt: bool,
) -> Option<&'static str> {
    if !platform || shift || alt {
        return None;
    }
    if key.eq_ignore_ascii_case("n") {
        Some("file.new")
    } else if key.eq_ignore_ascii_case("o") {
        Some("file.open")
    } else {
        None
    }
}

fn launcher_shows_stale_badge(item: &RecentItem) -> bool {
    item.is_stale()
}

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
    new_project_button: Option<Entity<Button>>,
    open_project_button: Option<Entity<Button>>,
    _search_sub: Option<sable::gpui::Subscription>,
}

impl LauncherView {
    fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let (store, load_warning) = mru::load();
        let focus = cx.focus_handle();
        // 初始焦点落根视图:↑↓/Enter/R 无需先点窗口即可用
        window.focus(&focus);
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t("ui-shell-launcher-search-placeholder"))
        });
        let search_focus = search_input.read(cx).focus_handle(cx);
        let launcher = cx.new(|_| Self {
            model: LauncherModel::new(),
            store,
            search_input: search_input.clone(),
            focus,
            search_focus,
            load_warning,
            new_project_button: None,
            open_project_button: None,
            _search_sub: None,
        });
        // 启动器本身提供新建/打开入口;项目真实路径仍由 rfd OS 对话框选取。
        launcher.update(cx, |this, cx| {
            let new_project_click = cx.listener(|this, _event, _window, cx| {
                this.choose_new_project(cx);
            });
            this.new_project_button = Some(cx.new(|_| {
                Button::new("launcher-new-project", t("ui-shell-new-project"))
                    .variant(ButtonVariant::Primary)
                    .size(ButtonSize::Compact)
                    .on_press(new_project_click)
            }));
            let open_project_click = cx.listener(|this, _event, _window, cx| {
                this.choose_open_project(cx);
            });
            this.open_project_button = Some(cx.new(|_| {
                Button::new("launcher-open-project", t("ui-shell-open-project"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Compact)
                    .on_press(open_project_click)
            }));

            // 搜索词变更投影进状态机(选中回零);Enter 走键冒泡(见 render)
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
        let root_focused = self.focus.is_focused(window);
        let modifiers = ev.keystroke.modifiers;
        let plain = !modifiers.modified();
        let view = view_indices(&self.store, &self.model.search);

        // 命令 ID/key 来源见 commands.yaml: file.new=Ctrl+N, file.open=Ctrl+O。
        if !ev.is_held {
            match launcher_command_for_key(key, modifiers.platform, modifiers.shift, modifiers.alt)
            {
                Some("file.new") => {
                    self.choose_new_project(cx);
                    return;
                }
                Some("file.open") => {
                    self.choose_open_project(cx);
                    return;
                }
                _ => {}
            }
        }

        let launcher_focused = editing || root_focused;
        match key {
            "down" if launcher_focused && !editing => {
                self.model.select_next(view.len());
                cx.notify();
            }
            "up" if launcher_focused && !editing => {
                self.model.select_prev();
                cx.notify();
            }
            "enter" if plain && !ev.is_held && launcher_focused => self.open_selected(cx),
            "r" if plain && !ev.is_held && root_focused && !editing => self.remove_selected(cx),
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

    /// 新建项目:用户选择一个空目录,会话创建默认 HTML/CSS 文档并先安全落盘。
    fn choose_new_project(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = RfdDialog::new().pick_folder() else {
            return;
        };
        if let Err(error) = ProjectSession::new_project(&dir) {
            let error = error.to_string();
            self.load_warning = Some(t_args(
                "ui-shell-new-project-failed",
                &[("error", error.as_str().into())],
            ));
            cx.notify();
            return;
        }
        self.open_project(&dir, cx);
    }

    /// 打开目录项目:选择目录后交给同一 HTML/CSS 导入器验证。
    fn choose_open_project(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = RfdDialog::new().pick_folder() else {
            return;
        };
        self.open_project(&dir, cx);
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
        let stale = launcher_shows_stale_badge(item);
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
                            .children(stale.then(|| {
                                div()
                                    .text_size(px(11.0))
                                    .text_color(colors.warning)
                                    .child(t("ui-shell-launcher-invalid-path"))
                            })),
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
                            .child(t("ui-shell-launcher-empty")),
                    )
                    .child(
                        div()
                            .text_size(px(FONT_SIZE_BODY))
                            .text_color(colors.text_disabled)
                            .child(t("ui-shell-launcher-empty-hint")),
                    ),
            );
        } else if view.is_empty() {
            list = list.child(
                v_flex().size_full().items_center().justify_center().child(
                    div()
                        .text_size(px(FONT_SIZE_BODY))
                        .text_color(colors.text_secondary)
                        .child(t_args(
                            "ui-shell-launcher-no-match",
                            &[("query", self.model.search.as_str().into())],
                        )),
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
                    .children(self.new_project_button.clone())
                    .children(self.open_project_button.clone())
                    .child(Input::new(&self.search_input).w(px(220.0))),
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
                            .child(t("ui-shell-launcher-footer")),
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

// ─────────────────────────── 项目窗口(R0/R1 工作台) ───────────────────────────

/// 项目窗口根视图:全屏 dock 工作台(左坞 = 能力台账,中央 = 文档画布预览)。
struct ShellApp {
    dock: Entity<DockArea>,
    focus: FocusHandle,
}

impl ShellApp {
    fn new(
        window: &mut Window,
        cx: &mut App,
        project: Result<ProjectSession, String>,
    ) -> Entity<Self> {
        let focus = cx.focus_handle();
        let (session, error) = match project {
            Ok(project) => (Some(cx.new(|_| project)), None),
            Err(error) => (None, Some(error)),
        };
        let image_session = session.clone();
        let place_image: vb_kit::canvas_panel::PlaceImageAction = Rc::new(move |_window, app| {
            let Some(session) = image_session.as_ref() else {
                return Err(t("ui-canvas-no-project-session"));
            };
            let filters = [FileFilter::new(
                "Raster images",
                &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"],
            )];
            let mut dialog = RfdDialog::new();
            let Some(path) = dialog.pick_file(&t("ui-canvas-place-image-title"), &filters) else {
                return Ok(None);
            };
            session.update(app, |project, session_cx| {
                let result = project
                    .place_image_from_path(project.active_artboard_index(), &path)
                    .map(Some)
                    .map_err(|error| error.to_string());
                session_cx.notify();
                result
            })
        });
        let project_export_session = session.clone();
        let project_file: vb_kit::canvas_panel::ProjectFileAction =
            Rc::new(move |command_id, window, app| {
                if command_id == "file.export_dialog" {
                    return export::export_project(project_export_session.as_ref(), window, app);
                }
                let Some(dir) = RfdDialog::new().pick_folder() else {
                    return Ok(None);
                };
                match command_id {
                    "file.new" => ProjectSession::new_project(&dir).map(|_| ()),
                    "file.open" => ProjectSession::open(&dir).map(|_| ()),
                    _ => return Err(format!("unsupported project command: {command_id}")),
                }
                .map_err(|error| error.to_string())?;
                open_project_window(&dir, app);
                Ok(Some(t("ui-canvas-command-applied")))
            });

        let layers = SablePanel::create(
            title("ui-shell-panel-layers"),
            ProjectLayersPanel::new(session.clone(), error.clone(), cx).into(),
            cx,
        );
        let canvas = SablePanel::create(
            title("ui-shell-panel-canvas"),
            CanvasPanel::new(
                session.clone(),
                error.clone(),
                place_image,
                project_file,
                cx,
            )
            .into(),
            cx,
        );
        let inspector = SablePanel::create(
            title("ui-shell-panel-inspector"),
            ProjectInspectorPanel::new(session.clone(), error.clone(), cx).into(),
            cx,
        );
        let animation = SablePanel::create(
            title("ui-shell-panel-animation"),
            ProjectAnimationPanel::new(session, error, cx).into(),
            cx,
        );

        let dock = WorkspacePresets::build_workspace(
            "vellum-sable-r2",
            vec![layers],
            canvas,
            vec![inspector],
            window,
            cx,
        );
        dock.update(cx, |area, dock_cx| {
            let dock_area = dock_cx.entity().downgrade();
            area.set_bottom_dock(
                sable::dock::tab_group(vec![animation], &dock_area, window, dock_cx),
                Some(px(220.0)),
                true,
                window,
                dock_cx,
            );
        });

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

#[cfg(test)]
mod launcher_shortcut_tests {
    use super::{launcher_command_for_key, launcher_shows_stale_badge};
    use vb_session::mru::RecentItem;

    const COMMANDS: &str = include_str!("../../../commands.yaml");

    fn catalog_entry(id: &str) -> &str {
        let marker = format!("- id: {id}\n");
        let start = COMMANDS
            .find(&marker)
            .unwrap_or_else(|| panic!("command catalog missing {id}"))
            + marker.len();
        let remaining = &COMMANDS[start..];
        let end = remaining.find("\n- id: ").unwrap_or(remaining.len());
        &remaining[..end]
    }

    #[test]
    fn launcher_file_shortcuts_match_command_catalog() {
        assert_eq!(
            launcher_command_for_key("n", true, false, false),
            Some("file.new")
        );
        assert_eq!(
            launcher_command_for_key("O", true, false, false),
            Some("file.open")
        );
        assert_eq!(launcher_command_for_key("n", false, false, false), None);
        assert_eq!(launcher_command_for_key("n", true, true, false), None);
        assert_eq!(launcher_command_for_key("o", true, false, true), None);
        assert!(catalog_entry("file.new").contains("keys: { win: \"Ctrl+N\" }"));
        assert!(catalog_entry("file.open").contains("keys: { win: \"Ctrl+O\" }"));
    }

    #[test]
    fn stale_badge_tracks_real_directory_state() {
        let current = std::env::current_dir().expect("working directory exists");
        let existing = RecentItem {
            path: current.to_string_lossy().into_owned(),
            name: "existing".into(),
            last_opened: 0,
            pinned: false,
            thumb: None,
        };
        assert!(!launcher_shows_stale_badge(&existing));

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let missing_path = std::env::temp_dir().join(format!(
            "vellumb-stale-badge-missing-{}-{nonce}",
            std::process::id()
        ));
        assert!(!missing_path.is_dir());
        let missing = RecentItem {
            path: missing_path.to_string_lossy().into_owned(),
            name: "missing".into(),
            last_opened: 0,
            pinned: false,
            thumb: None,
        };
        assert!(launcher_shows_stale_badge(&missing));
    }
}
