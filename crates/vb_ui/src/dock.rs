//! 面板坞布局规则(02-1:可折叠 + Tab 分组)。
//!
//! **规则只写在这里一份**:`vb_app` 每帧照本模块的纯函数决定画展开坞
//! 还是 40px 图标条。纯函数无 egui 依赖,可完整单测 —— 折叠与否是
//! "窗口宽度 + 用户偏好"的纯函数,同一输入必然同一结果。
//!
//! ## 折叠规则(写明边界,测试逐条对应)
//!
//! | 窗口宽度 | 用户偏好(展开) | 结果 |
//! |---|---|---|
//! | < 1200 | 任意 | **强制折叠**(图标条;主动折叠,无横向滚动条) |
//! | ≥ 1200 | 未折叠 | 展开 |
//! | ≥ 1200 | 折叠 | 折叠(用户折的尊重) |
//!
//! 手动展开**不能**覆盖 <1200 的强制折叠 —— 窄窗口下展开 280px 坞
//! 会挤压画布到不可用;此时图标条点击只切换 Tab,不展开。

use crate::theme::space::COLLAPSE_BELOW;

/// 折叠态图标条宽度。
pub const ICON_RAIL_WIDTH: f32 = 40.0;
/// 展开宽度可拖下限。
pub const DOCK_MIN: f32 = 240.0;
/// 展开宽度可拖上限。
pub const DOCK_MAX: f32 = 420.0;

/// 面板坞此刻应否折叠(纯函数;规则表见模块注释)。
///
/// `window_width` = 视口宽度;`user_collapsed` = 用户折叠偏好
/// (折叠按钮 / F7 折叠写入;<1200 强制折叠不回写偏好 —— 窗口拉宽
/// 后自动恢复展开,不需要用户再点一次)。
pub fn should_collapse(window_width: f32, user_collapsed: bool) -> bool {
    window_width < COLLAPSE_BELOW || user_collapsed
}

/// 图标条上的点击是否允许展开(<1200 强制折叠时只切 Tab 不展开)。
pub fn rail_click_can_expand(window_width: f32) -> bool {
    window_width >= COLLAPSE_BELOW
}

/// 展开宽度钳制(可拖 240–420;默认 [`DOCK_WIDTH`] = 280)。
pub fn clamp_width(w: f32) -> f32 {
    w.clamp(DOCK_MIN, DOCK_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 规则表逐行:<1200 强制折叠,用户展开无效。
    #[test]
    fn below_threshold_always_collapses() {
        assert!(should_collapse(1199.0, false));
        assert!(
            should_collapse(1199.0, true),
            "手动展开不覆盖 <1200 强制折叠"
        );
        assert!(should_collapse(0.0, false));
        assert!(!rail_click_can_expand(1199.0), "窄窗口图标条点击只切 Tab");
    }

    /// 规则表逐行:≥1200 由用户偏好决定。
    #[test]
    fn at_or_above_threshold_follows_user() {
        assert!(
            !should_collapse(1200.0, false),
            "1200 本身不折叠(边界含等号)"
        );
        assert!(should_collapse(1200.0, true));
        assert!(!should_collapse(1920.0, false));
        assert!(should_collapse(1920.0, true));
        assert!(rail_click_can_expand(1200.0));
    }

    /// 宽度钳制:默认 280,可拖 240–420,越界收回。
    #[test]
    fn width_clamps_to_drag_range() {
        assert_eq!(clamp_width(280.0), 280.0);
        assert_eq!(clamp_width(100.0), DOCK_MIN);
        assert_eq!(clamp_width(9999.0), DOCK_MAX);
        assert_eq!(clamp_width(-5.0), DOCK_MIN);
        assert_eq!(DOCK_MIN, 240.0);
        assert_eq!(DOCK_MAX, 420.0);
        assert_eq!(crate::theme::space::DOCK_WIDTH, 280.0);
    }
}
