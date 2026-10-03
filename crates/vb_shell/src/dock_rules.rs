//! 面板坞折叠规则(22 篇 §3.4 裁决二;ADR-0050,硬骨头 #23 的配套裁决)。
//!
//! **规则只写在这里一份**:后续每帧照本模块纯函数决定画展开坞还是 40px
//! 图标条。纯函数零宿主依赖,可完整单测 —— 折叠与否是"窗口宽度 + 用户
//! 偏好"的纯函数,同一输入必然同一结果(旧宿主 `vb_ui/dock.rs` 规则表
//! 语义在新宿主保留并复测,常量单一真相在
//! [`vb_kit::tokens::layout`](vb_kit::tokens::layout))。
//!
//! ## 折叠规则(写明边界,测试逐条对应)
//!
//! | 窗口宽度 | 用户偏好(折叠) | 结果 |
//! |---|---|---|
//! | < 1200 | 任意 | **强制折叠**(40px 图标条;无横向滚动条) |
//! | ≥ 1200 | 未折叠 | 展开 |
//! | ≥ 1200 | 折叠 | 折叠(用户折的尊重) |
//!
//! 手动展开**不能**覆盖 <1200 的强制折叠 —— 窄窗口下展开 280px 坞会挤压
//! 画布到不可用;此时图标条点击只切换 Tab,不展开。
//!
//! ## R0 口径(诚实声明)
//!
//! 本批只落**规则 + 单测**;把它接进 gpui-component DockArea 的事件流
//! (窗口 resize 监听 → 折叠态切换、图标条点击拦截)需要动 sable dock
//! 交互面,归 R2 面板批次首项(ADR-0050「后续工作」)。布局持久化
//! (DockAreaState + workspace.json v3,22 篇裁决二后半)同样在 R2,
//! 对应硬骨头 #10。

use vb_kit::tokens::layout::{COLLAPSE_BELOW, RIGHT_DOCK_MAX, RIGHT_DOCK_MIN};

/// 折叠态图标条宽度(= 令牌 `RIGHT_DOCK_COLLAPSED`;沿用旧 dock.rs 命名)。
/// R0 只落规则+单测(接线归 R2,见模块注释),别名当前仅测试引用——
/// cfg(test) 门控避免二进制构建的 unused_import 警告;R2 接线时去掉门控。
#[cfg(test)]
pub use vb_kit::tokens::layout::RIGHT_DOCK_COLLAPSED as ICON_RAIL_WIDTH;

/// 面板坞此刻应否折叠(纯函数;规则表见模块注释)。
///
/// `window_width` = 视口宽度;`user_collapsed` = 用户折叠偏好(折叠按钮
/// 写入;<1200 强制折叠**不回写**偏好 —— 窗口拉宽后自动恢复展开,不需要
/// 用户再点一次,与旧 dock.rs 同口径)。
pub fn should_collapse(window_width: f32, user_collapsed: bool) -> bool {
    window_width < COLLAPSE_BELOW || user_collapsed
}

/// 图标条上的点击是否允许展开(<1200 强制折叠时只切 Tab 不展开)。
pub fn rail_click_can_expand(window_width: f32) -> bool {
    window_width >= COLLAPSE_BELOW
}

/// 展开宽度钳制(可拖 [`RIGHT_DOCK_MIN`]–[`RIGHT_DOCK_MAX`];默认
/// [`vb_kit::tokens::layout::RIGHT_DOCK_DEFAULT`] = 280)。
pub fn clamp_width(w: f32) -> f32 {
    w.clamp(RIGHT_DOCK_MIN, RIGHT_DOCK_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 规则表逐行:<1200 强制折叠,用户展开无效(不覆盖强制折叠)。
    #[test]
    fn below_threshold_always_collapses() {
        assert!(should_collapse(1199.0, false));
        assert!(
            should_collapse(1199.0, true),
            "手动折叠偏好不影响窄窗强制折叠(两种输入都折叠)"
        );
        assert!(should_collapse(0.0, false));
        assert!(
            !rail_click_can_expand(1199.0),
            "窄窗口图标条点击只切 Tab,不展开"
        );
    }

    /// 规则表逐行:≥1200 由用户偏好决定;1200 本身不强制折叠(边界含等号)。
    #[test]
    fn at_or_above_threshold_follows_user() {
        assert!(
            !should_collapse(1200.0, false),
            "1200 本身不折叠(边界含等号)"
        );
        assert!(should_collapse(1200.0, true), "宽窗尊重用户折叠偏好");
        assert!(!should_collapse(1920.0, false));
        assert!(should_collapse(1920.0, true));
        assert!(rail_click_can_expand(1200.0));
        assert!(rail_click_can_expand(1920.0));
    }

    /// 宽度钳制:默认 280,可拖 240–420,越界收回(与旧 dock.rs 同表)。
    #[test]
    fn width_clamps_to_drag_range() {
        assert_eq!(clamp_width(280.0), 280.0);
        assert_eq!(clamp_width(100.0), RIGHT_DOCK_MIN);
        assert_eq!(clamp_width(9999.0), RIGHT_DOCK_MAX);
        assert_eq!(
            ICON_RAIL_WIDTH,
            vb_kit::tokens::layout::RIGHT_DOCK_COLLAPSED,
            "图标条 40px 单一真相"
        );
    }
}
