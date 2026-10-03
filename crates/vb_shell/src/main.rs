//! `vellum-sable` — VellumBench 新宿主预览壳(22 篇迭代方案 §4 R0 交付 1
//! 的最小形态;旧 `vellumbench.exe`(egui)原样保留,双宿主并行)。
//!
//! 组装:`Application::new().run` → `sable::dock::init` → gpui-component
//! chrome 固定深色 → [`vb_kit::tokens::inject_vb_theme`] 灌 VB 调色板 →
//! 单窗口 dock 工作台(左:能力台账 / 中:画布(R1) 占位)。
//!
//! 生命周期:窗口关闭即退出(gpui 默认:最后一个窗口关闭即结束 run 循环);
//! panic 不静默吞 —— 开窗失败 `expect` fail-fast,与 cutforge 同口径。

use sable::dock::{SablePanel, WorkspacePresets};
use sable::gpui::{
    div, px, size, App, AppContext as _, Application, Bounds, Context, Entity, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Render, Styled as _,
    Window, WindowBounds, WindowOptions,
};
use sable::gpui_component::dock::DockArea;
use sable::gpui_component::theme::Theme;
use sable::gpui_component::{Root, ThemeMode};
use sable::widgets::prelude::{h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::tokens::FONT_SIZE_BODY;
use vb_session::i18n::t;

use vb_kit::capabilities_panel::CapabilitiesPanel;
use vb_kit::tokens::inject_vb_theme;

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
    Application::new().run(|cx: &mut App| {
        // 两套主题全局各自初始化(与 cutforge 同口径):sable::dock::init
        // 已含 gpui_component::init 与 sable theme::init,只调后者会丢 sable
        // 主题(启动 panic)。
        sable::dock::init(cx);
        // gpui-component 面板 chrome( DockArea/tab/输入框)固定深色
        Theme::change(ThemeMode::Dark, None, cx);
        // VB 调色板灌进 sable tokens(JSON 单一真相的投影;tokens_sync2 门禁)
        inject_vb_theme(cx);

        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(sable::gpui::TitlebarOptions {
                title: Some(title("ui-shell-window-title").into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(options, |window, cx| {
            let shell = ShellApp::new(window, cx);
            cx.new(|cx| Root::new(shell, window, cx))
        })
        .expect("vellum-sable 开窗失败:GPUI 平台层初始化异常(显卡驱动/显示服务)");

        cx.activate(true);
    });
}

/// 根视图:全屏 dock 工作台(左坞 = 能力台账,中央 = 画布(R1) 占位)。
struct ShellApp {
    dock: Entity<DockArea>,
    focus: FocusHandle,
}

impl ShellApp {
    fn new(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let focus = cx.focus_handle();

        // 左坞:能力台账(「面板 = 纯投影」示范;数据在 vb_session)
        let capabilities = SablePanel::create(
            title("ui-shell-panel-capabilities"),
            CapabilitiesPanel::new(cx).into(),
            cx,
        );
        // 中央:画布占位(R1 接管;R0 先立窗口骨架与 dock 布局)
        let canvas = SablePanel::create(
            title("ui-shell-panel-canvas"),
            cx.new(|_| CanvasPlaceholder).into(),
            cx,
        );

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
                        .child(t("ui-shell-canvas-placeholder")),
                ),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(colors.text_disabled)
                    .child(t("ui-shell-canvas-subtitle")),
            )
    }
}
