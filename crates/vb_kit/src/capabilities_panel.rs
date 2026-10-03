//! 能力台账面板(R0 最小可用版;22 篇 §4 R0 交付 2 的示范面板)。
//!
//! 走通「面板 = 纯投影」模式:数据 100% 来自
//! [`vb_session::capabilities::CAPABILITIES`](单一真相),本面板**零自有
//! 台账**;状态只有两个投影参数(状态过滤 + 仅 Agent 可复现),没有对任何
//! 文档模型/宿主的反向依赖。R6 的「台账 UI 升级」(直达命令)在此基础上做。
//!
//! 组件形态:gpui-component `Button`(过滤 chips + Agent 过滤开关)+
//! `Label`(条目名/去向),行高走令牌 [`layout::ROW_HEIGHT_LIST`](24)。

use sable::gpui::{
    div, hsla, px, App, AppContext as _, ClickEvent, Context, Entity, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use sable::gpui_component::button::Button;
use sable::gpui_component::label::Label;
use sable::gpui_component::Selectable;
use sable::widgets::prelude::{h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::tokens::ColorTokens;
use vb_session::capabilities::{CapStatus, Capability, CAPABILITIES};

use crate::tokens::{font_size, layout, radius, space};

/// 状态过滤档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapFilter {
    All,
    Done,
    Partial,
    Dropped,
}

/// 台账面板状态(纯投影参数)。
pub struct CapabilitiesPanel {
    filter: CapFilter,
    /// 只看「Agent 可复现」(有命令入口;R6 升级为过滤列 + 直达命令)。
    agent_only: bool,
}

impl CapabilitiesPanel {
    pub fn new(cx: &mut App) -> Entity<Self> {
        cx.new(|_| Self {
            filter: CapFilter::All,
            agent_only: false,
        })
    }

    /// 过滤谓词(纯函数)。
    fn matches(&self, c: &Capability) -> bool {
        let status_ok = match self.filter {
            CapFilter::All => true,
            CapFilter::Done => matches!(c.status, CapStatus::Done),
            CapFilter::Partial => matches!(c.status, CapStatus::Partial(_)),
            CapFilter::Dropped => matches!(c.status, CapStatus::Dropped(_)),
        };
        status_ok && (!self.agent_only || c.agent_reproducible())
    }

    fn filter_chip(
        &self,
        id: &'static str,
        label: &'static str,
        value: CapFilter,
        weak: &sable::gpui::WeakEntity<Self>,
    ) -> Button {
        let weak = weak.clone();
        Button::new(id)
            .label(label)
            .compact()
            .selected(self.filter == value)
            .on_click(move |_: &ClickEvent, _, cx: &mut App| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |panel, cx| {
                        panel.filter = value;
                        cx.notify();
                    });
                }
            })
    }

    fn agent_chip(&self, weak: &sable::gpui::WeakEntity<Self>) -> Button {
        let weak = weak.clone();
        Button::new("cap-filter-agent")
            .label("仅 Agent 可复现")
            .compact()
            .selected(self.agent_only)
            .on_click(move |_: &ClickEvent, _, cx: &mut App| {
                if let Some(panel) = weak.upgrade() {
                    panel.update(cx, |panel, cx| {
                        panel.agent_only = !panel.agent_only;
                        cx.notify();
                    });
                }
            })
    }
}

/// 三态徽章:(底色, 文字色)。语义色映射 —— Done=success / Partial=warn /
/// Dropped=danger;`Planned` 被「no_dangling_planned」门禁保证为空,仍给
/// 兜底色(次级文字色),徽章文案用 [`CapStatus::badge`]。
fn badge_colors(status: CapStatus, c: &ColorTokens) -> (Hsla, Hsla) {
    let base = match status {
        CapStatus::Done => c.success,
        CapStatus::Partial(_) => c.warning,
        CapStatus::Dropped(_) => c.danger,
        CapStatus::Planned(_) => c.text_secondary,
    };
    // 同色低透明底(品红参考线式的语义色淡底;不经 hex,直接降 alpha)
    (hsla(base.h, base.s, base.l, 0.16), base)
}

/// 台账计数(全量,不受过滤影响)。
fn counts() -> (usize, usize, usize) {
    CAPABILITIES
        .iter()
        .fold((0, 0, 0), |(done, partial, dropped), c| match c.status {
            CapStatus::Done => (done + 1, partial, dropped),
            CapStatus::Partial(_) => (done, partial + 1, dropped),
            CapStatus::Dropped(_) => (done, partial, dropped + 1),
            CapStatus::Planned(_) => (done, partial, dropped),
        })
}

/// 一条台账目项(徽章 + 编号 + 名称 + Agent 列;Partial/Dropped 加去向行)。
fn capability_row(cap: &'static Capability, c: &ColorTokens) -> sable::gpui::AnyElement {
    let (badge_bg, badge_fg) = badge_colors(cap.status, c);
    let (agent_text, agent_color) = if cap.agent_reproducible() {
        (format!("可复现 ×{}", cap.commands.len()), c.success)
    } else {
        ("—".to_string(), c.text_disabled)
    };

    let mut row = v_flex()
        .px(px(space::S2))
        .py(px(3.0))
        .rounded(px(radius::SM))
        .hover(|s| s.bg(c.surface_3))
        .child(
            h_flex()
                .gap(px(space::S2))
                .items_center()
                .min_h(px(layout::ROW_HEIGHT_LIST))
                .child(
                    // 三态徽章
                    div_badge(CapStatus::badge(cap.status), badge_bg, badge_fg),
                )
                .child(
                    div()
                        .w(px(34.0))
                        .flex_shrink_0()
                        .text_size(px(font_size::CAPTION))
                        .text_color(c.text_secondary)
                        .child(cap.id),
                )
                .child(
                    Label::new(cap.name)
                        .flex_1()
                        .text_size(px(font_size::LABEL))
                        .text_color(c.text_primary)
                        .truncate(),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(font_size::CAPTION))
                        .text_color(agent_color)
                        .child(agent_text),
                ),
        );

    let note = cap.status.note();
    if !note.is_empty() {
        // 去向行:Partial 必写「缺哪一半/留后续」,Dropped 必写理由与替代
        row = row.child(
            div()
                .pl(px(space::S6))
                .text_size(px(font_size::CAPTION))
                .text_color(c.text_disabled)
                .child(note)
                .truncate(),
        );
    }
    row.into_any_element()
}

/// 徽章小胶囊(div 形态,色经语义映射取自主题,零 hex)。
fn div_badge(text: &'static str, bg: Hsla, fg: Hsla) -> sable::gpui::Div {
    sable::gpui::div()
        .px(px(5.0))
        .py(px(1.0))
        .rounded(px(radius::SM))
        .flex_shrink_0()
        .bg(bg)
        .text_size(px(font_size::CAPTION))
        .text_color(fg)
        .child(text)
}

impl Render for CapabilitiesPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = theme(cx).colors;
        let weak = cx.entity().downgrade();

        let (done, partial, dropped) = counts();
        let total = CAPABILITIES.len();
        let visible: Vec<&'static Capability> = CAPABILITIES
            .iter()
            .filter(|cap| self.matches(cap))
            .collect();

        // ── 过滤行:三态 chips + Agent 开关 + 计数 ──
        let toolbar = v_flex()
            .flex_shrink_0()
            .gap(px(space::S2))
            .px(px(space::S3))
            .py(px(space::S2))
            .border_b_1()
            .border_color(c.border_subtle)
            .child(
                h_flex()
                    .gap(px(2.0))
                    .flex_wrap()
                    .child(self.filter_chip("cap-filter-all", "全部", CapFilter::All, &weak))
                    .child(self.filter_chip("cap-filter-done", "已落地", CapFilter::Done, &weak))
                    .child(self.filter_chip(
                        "cap-filter-partial",
                        "部分",
                        CapFilter::Partial,
                        &weak,
                    ))
                    .child(self.filter_chip(
                        "cap-filter-dropped",
                        "不做",
                        CapFilter::Dropped,
                        &weak,
                    ))
                    .child(self.agent_chip(&weak)),
            )
            .child(
                div()
                    .text_size(px(font_size::CAPTION))
                    .text_color(c.text_secondary)
                    .child(format!(
                        "共 {total} 条 · 已落地 {done} · 部分 {partial} · 不做 {dropped}"
                    )),
            );

        // ── 条目列表 ──
        let list = if visible.is_empty() {
            v_flex()
                .items_center()
                .justify_center()
                .gap(px(space::S2))
                .child(
                    div()
                        .text_size(px(font_size::LABEL))
                        .text_color(c.text_secondary)
                        .child("无匹配条目(当前过滤组合)"),
                )
                .into_any_element()
        } else {
            let mut list = v_flex().gap(px(1.0));
            for cap in visible {
                list = list.child(capability_row(cap, &c));
            }
            list.into_any_element()
        };

        // ── 页脚:数据源声明(纯投影的自我声明)──
        let footer = div()
            .flex_shrink_0()
            .px(px(space::S3))
            .py(px(space::S2))
            .border_t_1()
            .border_color(c.border_subtle)
            .text_size(px(font_size::CAPTION))
            .text_color(c.text_disabled)
            .child("数据源:vb_session::capabilities(单一真相) · 三态:已落地 / 部分+去向 / 不做");

        v_flex()
            .id("capabilities-panel")
            .size_full()
            .bg(c.surface_1)
            .text_color(c.text_primary)
            .child(toolbar)
            .child(
                div()
                    .id("capabilities-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(space::S2))
                    .py(px(space::S2))
                    .child(list),
            )
            .child(footer)
    }
}
