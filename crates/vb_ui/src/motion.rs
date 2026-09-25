//! 一次性入场动效(H-1):对话框/浮层出现时的位移入场、Tab 内容切换。
//!
//! ## 为什么不用 `animate_value_with_time`
//!
//! egui 的动画 API 是「状态趋近」语义(布尔/数值朝目标过渡),而对话框
//! 打开是**一次性事件** —— 没有一个持续存在的状态可以趋近。这里用
//! egui 持久数据记「本次打开的起始时刻」,按流逝时间算缓动进度;
//! 「隔了 >0.5s 又被调用」即视为重新打开,重置起点。
//!
//! ## 与总开关的关系
//!
//! [`theme::motion_enabled`] 为 false 时本模块全部直通(alpha = 1.0、
//! 零位移),连起始时刻都不写 —— 关心动效的用户零开销。
//!
//! ## 两台实测警告( egui 0.35 的坑,勿回退)
//!
//! 1. **不能用 `Painter::multiply_opacity` 做整窗淡入**:对 Window 内容
//!    painter 乘上 <1 的透明度后,该窗口的**内容在后续所有 pass 里都不
//!    再绘制**(窗框在、内容永久空白,与 alpha 是否回到 1 无关)——
//!    逐层下传的透明度与窗口图形层状态相互污染。入场动效因此只保留
//!    布局位移;透明度交给 toast 的 Frame 级淡出(Frame 自身着色,
//!    不经 painter 透明度,实测无此问题)。
//! 2. **子块必须用 `scope_builder` 而不是 `new_child`**:后者不向父级
//!    回写 min_rect,Window 会把自己收缩成一条标题栏(内容画了但窗口
//!    尺寸为零)。

use std::time::{Duration, Instant};

use egui::{Id, Ui, UiBuilder};

use crate::theme;

/// 「重新打开」的判定间隔:两次调用相隔超过它 → 重置起始时刻。
const REOPEN_GAP: Duration = Duration::from_millis(500);

/// 每个入场点的记忆(egui 持久数据;临时数据两帧不访问会被清,
/// 持久数据只在长期不用时清 —— 两种情况都由 REOPEN_GAP 兜底)。
#[derive(Debug, Clone, Copy)]
struct EnterState {
    start: Instant,
    last: Instant,
}

/// 平滑缓动(smoothstep):两端导数为 0,起步与收尾都不突兀。
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 当前入场进度 alpha(0..1;动效关闭时恒 1)。
///
/// `secs` = 全程时长(如 `theme::motion::STATE` = 120ms)。
pub fn enter_alpha(ctx: &egui::Context, id: Id, secs: f32) -> f32 {
    if !theme::motion_enabled(ctx) {
        return 1.0;
    }
    let now = Instant::now();
    let elapsed = ctx.data_mut(|d| {
        let st = d.get_temp::<EnterState>(id);
        let (start, fresh) = match st {
            Some(s) if now.duration_since(s.last) < REOPEN_GAP => (s.start, false),
            _ => (now, true),
        };
        d.insert_temp(id, EnterState { start, last: now });
        if fresh {
            Duration::ZERO
        } else {
            now.duration_since(start)
        }
    });
    let secs = if secs > 0.0 { secs } else { 0.001 };
    let t = elapsed.as_secs_f32() / secs;
    // 动画期间(含到位后的一小段缓冲)主动要帧:空闲态(无输入无动画)
    // 下不补帧会卡在中途;且 Window 的布局随位移收敛逐帧变化,会触发
    // sizing pass —— 必须续帧到布局稳定后的正常绘制 pass 把画面接住。
    if elapsed.as_secs_f32() < secs + 0.25 {
        ctx.request_repaint_after(Duration::from_millis(16));
    }
    ease(t)
}

/// 把 `body` 的绘制包上一层入场位移(`slide_px` 像素,自下而上就位):
/// 对话框 120ms + 4px,浮层/Tab 内容 80ms + 2px。
pub fn fade_slide(ui: &mut Ui, id: Id, secs: f32, slide_px: f32, body: impl FnOnce(&mut Ui)) {
    let a = enter_alpha(ui.ctx(), id, secs);
    // 每帧走**同一条布局路径**(scope 子块):入场期与稳定期的结构一致,
    // 避免 Window 的 sizing 记忆在不同路径之间反复横跳(实测会撑高窗口)。
    let dy = (1.0 - a) * slide_px;
    ui.scope_builder(UiBuilder::new().id_salt(id.with("fade")), |inner| {
        inner.add_space(dy);
        body(inner);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缓动两端精确命中,中点平滑,越界钳制。
    #[test]
    fn ease_hits_ends_and_clamps() {
        assert!((ease(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((ease(1.0) - 1.0).abs() < f32::EPSILON);
        assert!(
            (ease(0.5) - 0.5).abs() < f32::EPSILON,
            "smoothstep 中点对称"
        );
        assert!(ease(0.25) < 0.25, "前段慢(缓入)");
        assert!(ease(0.75) > 0.75, "后段慢(缓出)");
        assert_eq!(ease(-1.0), 0.0);
        assert_eq!(ease(2.0), 1.0);
    }

    /// egui 临时数据跨 pass 存活(enter_alpha 的起始时刻依赖此语义)。
    #[test]
    fn temp_data_survives_across_passes() {
        let ctx = egui::Context::default();
        let id = Id::new("probe");
        ctx.begin_pass(egui::RawInput::default());
        let fresh = ctx.data_mut(|d| {
            let st = d.get_temp::<EnterState>(id);
            d.insert_temp(
                id,
                EnterState {
                    start: Instant::now(),
                    last: Instant::now(),
                },
            );
            st.is_none()
        });
        let _ = ctx.end_pass();
        ctx.begin_pass(egui::RawInput::default());
        let missing = ctx.data_mut(|d| d.get_temp::<EnterState>(id).is_none());
        let _ = ctx.end_pass();
        assert!(fresh, "首次调用必须是新入场");
        assert!(!missing, "临时数据必须跨 pass 存活(起始时刻依赖)");
    }

    /// 动效关闭时 `enter_alpha` 直通(alpha 恒 1)且不写起始时刻。
    #[test]
    fn kill_switch_short_circuits_enter_alpha() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        theme::set_motion_enabled(&ctx, false);
        let id = Id::new("t");
        assert_eq!(enter_alpha(&ctx, id, 0.12), 1.0);
        assert!(
            ctx.data(|d| d.get_temp::<EnterState>(id).is_none()),
            "关闭态不得写起始时刻(零开销)"
        );
    }
}
