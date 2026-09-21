//! 可堆叠 toast 通知(02-6-6;design/14 §3.5 组件表 #9)。
//!
//! 替代单行 footer 中「错误 / 告警」类反馈:footer 仍保留常规状态提示,
//! 错误与告警走 toast —— **可见更久、可复制文本**(Agent 场景要把报错
//! 原文粘给 AI/日志,单行 footer 一闪而过或被截断都不可接受)。
//!
//! 规则:
//! - 右下角堆叠(状态栏上方),新的在上;自动消退
//!   (信息/成功 2.5s / 告警 5s / 错误 10s —— 错误要留够「读 + 复制」
//!   的时间,不按 2s 一刀切);
//! - 每条可手动关闭;错误/告警文本可选中,并带一键复制按钮;
//! - 纯逻辑(入队 / 按到期修剪)与绘制分离,逻辑可单测。

use std::time::{Duration, Instant};

use egui::{Align2, Context, Vec2};

use crate::{fonts, icons, theme};

/// 通知类别(决定图标、主色与存活时长)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    /// 常规信息。
    Info,
    /// 成功(已保存 / 已导出)。
    Success,
    /// 告警(操作被拒绝、参数无效等,可继续工作)。
    Warn,
    /// 错误(命令失败、解析失败;**可复制**)。
    Error,
}

impl ToastKind {
    /// 存活时长(自动消退)。
    pub fn ttl(self) -> Duration {
        match self {
            Self::Info | Self::Success => Duration::from_millis(2500),
            Self::Warn => Duration::from_millis(5000),
            Self::Error => Duration::from_millis(10_000),
        }
    }

    fn icon(self) -> icons::Name {
        match self {
            Self::Info => icons::Name::Info,
            Self::Success => icons::Name::Check,
            Self::Warn | Self::Error => icons::Name::Alert,
        }
    }

    fn color(self, t: &theme::Tokens) -> Color32 {
        match self {
            Self::Info => t.text_2,
            Self::Success => t.success,
            Self::Warn => t.warn,
            Self::Error => t.danger,
        }
    }
}

use egui::Color32;

/// 一条通知。字段 `pub(crate)`:宿主只经 [`ToastHost::push`] 入队。
#[derive(Debug, Clone)]
pub struct Toast {
    pub(crate) kind: ToastKind,
    pub(crate) text: String,
    pub(crate) born: Instant,
}

impl Toast {
    /// 是否到期(纯函数,`prune_at` 的判定核心)。
    fn expired(&self, now: Instant) -> bool {
        match now.checked_duration_since(self.born) {
            Some(elapsed) => elapsed >= self.kind.ttl(),
            // born 在未来(时钟回拨等)视为未到期
            None => false,
        }
    }
}

/// toast 队列宿主。`vb_app` 持有一个,每帧 `prune` + `show`。
#[derive(Default)]
pub struct ToastHost {
    items: Vec<Toast>,
}

impl ToastHost {
    /// 入队一条(现在时刻)。
    pub fn push(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.push_at(kind, text, Instant::now());
    }

    /// 入队(显式时刻;测试注入用)。
    pub fn push_at(&mut self, kind: ToastKind, text: impl Into<String>, now: Instant) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        // 未过期的同文本去重:命令失败可能逐帧重复触发,不刷屏
        if self
            .items
            .iter()
            .any(|t| t.text == text && t.kind == kind && !t.expired(now))
        {
            return;
        }
        self.items.push(Toast {
            kind,
            text,
            born: now,
        });
        // 上限 6 条:再多的旧消息让位(错误会重新发生,不丢「有过错误」这个事实)
        let excess = self.items.len().saturating_sub(6);
        self.items.drain(0..excess);
    }

    /// 修剪到期条目。
    pub fn prune(&mut self) {
        self.prune_at(Instant::now());
    }

    /// 修剪(显式时刻;测试注入用)。
    pub fn prune_at(&mut self, now: Instant) {
        self.items.retain(|t| !t.expired(now));
    }

    /// 队列是否为空(宿主可据此跳过绘制)。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 当前条数(测试与调试)。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 画在视口右下角(状态栏上方)。每帧调用;内部自行修剪。
    pub fn show(&mut self, ctx: &Context) {
        self.prune();
        if self.items.is_empty() {
            return;
        }
        let t = theme::tokens(ctx);
        let mut closed: Vec<usize> = Vec::new();
        let mut copy_text: Option<String> = None;
        let now = Instant::now();

        egui::Area::new(egui::Id::new("vb_toasts"))
            .order(egui::Order::Foreground)
            .anchor(
                Align2::RIGHT_BOTTOM,
                Vec2::new(
                    -theme::space::S6,
                    -(theme::space::STATUS_BAR_HEIGHT + theme::space::S4),
                ),
            )
            .show(ctx, |ui| {
                ui.set_max_width(360.0);
                ui.set_min_width(200.0);
                ui.spacing_mut().item_spacing.y = theme::space::S1;
                // push 追加在尾部(最新),锚定右下 → 逆序绘制让最新在最上
                for (i, toast) in self.items.iter().enumerate().rev() {
                    let elapsed = now
                        .checked_duration_since(toast.born)
                        .unwrap_or(Duration::ZERO);
                    let remaining = toast.kind.ttl().saturating_sub(elapsed);
                    // 末 400ms 整卡淡出
                    let fade = (remaining.as_secs_f32() / 0.4).clamp(0.0, 1.0);
                    let kind_col = toast.kind.color(&t);
                    let copyable = matches!(toast.kind, ToastKind::Error | ToastKind::Warn);
                    egui::Frame::new()
                        .fill(t.bg_raised)
                        .stroke(egui::Stroke::new(
                            theme::stroke::HAIRLINE,
                            kind_col.gamma_multiply(0.6),
                        ))
                        .corner_radius(theme::radius::lg())
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .multiply_with_opacity(fade)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(icons::rich(toast.kind.icon(), 14.0).color(kind_col));
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&toast.text)
                                            .font(fonts::font(12.0, fonts::Weight::Regular))
                                            .color(t.text),
                                    )
                                    .selectable(copyable),
                                );
                                if copyable
                                    && crate::components::icon_button(
                                        ui,
                                        icons::Name::Copy,
                                        "复制文本",
                                    )
                                    .clicked()
                                {
                                    copy_text = Some(toast.text.clone());
                                }
                                if crate::components::icon_button(ui, icons::Name::Close, "关闭")
                                    .clicked()
                                {
                                    closed.push(i);
                                }
                            });
                        });
                }
            });

        if let Some(text) = copy_text {
            ctx.copy_text(text);
        }
        // 从高到低删,避免索引位移
        closed.sort_unstable();
        closed.dedup();
        for i in closed.into_iter().rev() {
            if i < self.items.len() {
                self.items.remove(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(offset_ms: i64) -> Instant {
        let now = Instant::now();
        if offset_ms >= 0 {
            now + Duration::from_millis(offset_ms as u64)
        } else {
            now - Duration::from_millis((-offset_ms) as u64)
        }
    }

    /// 自动消退:信息 2.5s / 告警 5s / 错误 10s,到期即修剪。
    #[test]
    fn prune_expires_by_kind() {
        let mut h = ToastHost::default();
        let born = t(0);
        h.push_at(ToastKind::Info, "i", born);
        h.push_at(ToastKind::Warn, "w", born);
        h.push_at(ToastKind::Error, "e", born);
        assert_eq!(h.len(), 3);

        h.prune_at(born + Duration::from_millis(2499));
        assert_eq!(h.len(), 3);
        h.prune_at(born + Duration::from_millis(2500));
        assert_eq!(h.len(), 2, "信息在 2.5s 到期");
        h.prune_at(born + Duration::from_millis(5000));
        assert_eq!(h.len(), 1, "告警在 5s 到期");
        h.prune_at(born + Duration::from_millis(9999));
        assert_eq!(h.len(), 1);
        h.prune_at(born + Duration::from_millis(10_000));
        assert!(h.is_empty(), "错误在 10s 到期");
    }

    /// 同文本未过期去重(逐帧重复的报错不刷屏);过期后允许再次入队。
    #[test]
    fn dedups_unduplicated_while_alive() {
        let mut h = ToastHost::default();
        let born = t(0);
        h.push_at(ToastKind::Error, "命令失败:x", born);
        h.push_at(
            ToastKind::Error,
            "命令失败:x",
            born + Duration::from_secs(1),
        );
        assert_eq!(h.len(), 1, "存活期内同文本不重复");
        h.push_at(ToastKind::Error, "命令失败:y", born);
        assert_eq!(h.len(), 2, "不同文本照常入队");
        h.prune_at(born + ToastKind::Error.ttl());
        h.push_at(
            ToastKind::Error,
            "命令失败:x",
            born + ToastKind::Error.ttl(),
        );
        assert_eq!(h.len(), 1, "过期后同文本可再次入队");
    }

    /// 空文本不入队;超过 6 条裁最旧。
    #[test]
    fn rejects_empty_and_caps_at_six() {
        let mut h = ToastHost::default();
        h.push_at(ToastKind::Info, "", t(0));
        assert!(h.is_empty());
        let born = t(0);
        for i in 0..8 {
            h.push_at(ToastKind::Info, format!("m{i}"), born);
        }
        assert_eq!(h.len(), 6, "队列上限 6 条");
        assert_eq!(h.items[0].text, "m2", "裁掉的是最旧的");
        assert_eq!(h.items[5].text, "m7");
    }
}
