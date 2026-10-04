//! 车道配置常量单一入口(EXP-12 / RB-12):超时、阈值、上限、经验 sleep
//! 集中定义,命名即语义。此前 `15_000`/`1080`/`5s`/`130ms` 等散落三个
//! crate 十余处,调整一处漏一处即口径漂移。
//!
//! 约定:常量一律 `pub`;新增采集参数先进本模块再引用;环境变量覆盖钩子
//! (如 `VB_SETTLE_BUDGET_MS`)留在使用点并在此注明。

use std::time::Duration;

/// 采集边缘上限(px):宽×scale / 高×scale 任一超过即放弃单拍走分块,
/// 或把 DSF 压回 1(浏览器单帧位图上限,超限截图直接失败)。
pub const CAPTURE_MAX_EDGE_PX: u64 = 15_000;

/// 视口兜底宽(px):无显式 --width 且画板无声明尺寸时的历史口径(WPI)。
pub const VIEWPORT_FALLBACK_PX: u32 = 1080;

/// settle 视觉收敛总预算(硬上限):assets/滚动/定格/稳定探测全部计入,
/// 到顶即用当前帧导出并追加可观测 warning。环境变量 `VB_SETTLE_BUDGET_MS`
/// 可覆盖(测试/诊断钩子,见 capture::settle_budget)。
pub const SETTLE_BUDGET: Duration = Duration::from_secs(5);

/// 页内资源等待竞速上限(要素 5):fonts.ready 3s / img.complete 5s。
pub const ASSET_FONTS_CAP_MS: u64 = 3_000;
pub const ASSET_IMGS_CAP_MS: u64 = 5_000;

/// 资源等待后的末次重排/重绘窗(经验值;观察到的掉字均在此窗口内)。
pub const ASSET_WAIT_POST_DELAY_MS: u64 = 200;

/// networkidle 静默判定窗:500ms 无网络活动即视为空闲。
pub const NETWORK_IDLE_SILENCE_MS: u64 = 500;

/// 滚动触发 reveal:单步停留窗(IntersectionObserver/过渡触发)与步数上限。
pub const SCROLL_REVEAL_STEP_MS: u64 = 130;
pub const SCROLL_REVEAL_MAX_STEPS: u32 = 40;

/// 分块截图:滚动后等布局/合成稳定(经验值)。
pub const TILE_SETTLE_DELAY_MS: u64 = 400;

/// settle 稳定探针:相邻比对帧间隔与滚动/定格后的末次绘制窗。
pub const STABILITY_PROBE_INTERVAL_MS: u64 = 200;

/// 有限动画页宽限期(入场过渡走完后取当前帧;受 settle 预算截断)。
pub const INFINITE_ANIM_GRACE_MS: u64 = 3_000;

/// 画板取景后的重排等待窗(经验值)。
pub const ARTBOARD_REFLOW_DELAY_MS: u64 = 150;

/// 无画板取景时 artboard 分支前的固定等待(经验值)。
pub const BODY_MARGIN_RESET_DELAY_MS: u64 = 120;

#[cfg(test)]
mod tests {
    use super::*;

    /// EXP-12:常量集中后钉住关键值(误改口径即测试红)。
    #[test]
    fn pinned_values() {
        assert_eq!(CAPTURE_MAX_EDGE_PX, 15_000);
        assert_eq!(VIEWPORT_FALLBACK_PX, 1080);
        assert_eq!(SETTLE_BUDGET, Duration::from_secs(5));
        assert_eq!(ASSET_FONTS_CAP_MS, 3_000);
        assert_eq!(ASSET_IMGS_CAP_MS, 5_000);
        assert_eq!(SCROLL_REVEAL_MAX_STEPS, 40);
    }
}
