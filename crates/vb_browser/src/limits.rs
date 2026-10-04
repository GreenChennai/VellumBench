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

/// CDP 长等待上限(EXP-12 集中):截图 180s / printToPDF 300s /
/// beginFrame 60s / 页面 enable 与 attach 30s。此前字面量散落
/// page.rs/print.rs 多处,调整一处漏一处。
pub const SCREENSHOT_WAIT: Duration = Duration::from_secs(180);
pub const PRINT_PDF_WAIT: Duration = Duration::from_secs(300);
pub const BEGIN_FRAME_WAIT: Duration = Duration::from_secs(60);
pub const ATTACH_ENABLE_WAIT: Duration = Duration::from_secs(30);

/// navigate 后 networkidle 静默判定的外层等待上限(EXP-12;内层静默窗
/// 见 [`NETWORK_IDLE_SILENCE_MS`])。
pub const NETWORK_IDLE_CAP: Duration = Duration::from_secs(3);

// `--max-wait` 语义(EXP-02):显式给出时作为浏览器车道**总 deadline**
// —— settle 预算与全部 CDP 长等待(截图/printToPDF/beginFrame)各自被
// min(默认上限, 剩余预算) 收口;预算耗尽的调用立即失败(错误串可判),
// 不再静默挂满 180s/300s。未给出(None)= 维持各阶段默认上限。

/// 临时产物命名(EXP-10 落盘点集中注册):一切落 %TEMP% 的工作目录/文件
/// **必须**经 [`temp_name`] 生成 —— `kiln-{kind}-{pid}-{nanos}-{seq}{ext}`。
/// 扫描回收按 `kiln-` 前缀命中 + 第二段解析存活 pid:pid 已死立即回收
/// (崩溃残留不再等 6h),解析不出 pid(历史/外来目录)退回 6h mtime 规则。
/// 新增落盘点 = 新 kind 字符串,命名约定即注册,无独立清单可漏。
pub fn temp_name(kind: &str, ext: &str) -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("kiln-{kind}-{}-{nanos}-{seq}{ext}", std::process::id())
}

/// 临时目录前缀(扫描命中范围;精确解析见 [`parse_temp_pid`])。
pub const TEMP_PREFIX: &str = "kiln-";

/// 历史目录(解析不出存活 pid)的 mtime 回收阈值。
pub const TEMP_LEGACY_MTIME_SECS: u64 = 6 * 3600;

/// 单次清扫最多探活的目录数(RB-05:一切循环有上限;异常环境下 %TEMP%
/// 被灌满时不把启动拖成分钟级)。
pub const TEMP_SWEEP_PROBE_MAX: usize = 64;

/// 从 `kiln-{kind}-{pid}-…` 解析落盘 pid(第二段须全数字)。
pub fn parse_temp_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(TEMP_PREFIX)?;
    let mut it = rest.split('-');
    let kind = it.next()?;
    if kind.is_empty() || kind.bytes().any(|b| b.is_ascii_digit()) {
        return None;
    }
    let pid = it.next()?;
    if pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    pid.parse().ok()
}

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
        assert_eq!(SCREENSHOT_WAIT, Duration::from_secs(180));
        assert_eq!(PRINT_PDF_WAIT, Duration::from_secs(300));
        assert_eq!(BEGIN_FRAME_WAIT, Duration::from_secs(60));
    }

    /// EXP-10:temp_name 产出可被 parse_temp_pid 还原本进程 pid。
    #[test]
    fn temp_name_roundtrips_pid() {
        let n = temp_name("browser", "");
        assert!(n.starts_with("kiln-browser-"), "{n}");
        assert_eq!(parse_temp_pid(&n), Some(std::process::id()), "{n}");
        let f = temp_name("wc", ".h264");
        assert!(f.ends_with(".h264") && f.starts_with("kiln-wc-"), "{f}");
        assert_eq!(parse_temp_pid(&f), Some(std::process::id()));
        // 唯一性(同 kind 同瞬间两次调用)
        assert_ne!(temp_name("anim", ""), temp_name("anim", ""));
    }

    /// EXP-10:外来/畸形名字解析为 None(退回 mtime 规则,宁留勿删)。
    #[test]
    fn parse_rejects_foreign_names() {
        assert_eq!(parse_temp_pid("not-kiln-123"), None);
        assert_eq!(parse_temp_pid("kiln-123-456"), None, "kind 含数字拒绝");
        assert_eq!(parse_temp_pid("kiln-browser-"), None);
        assert_eq!(parse_temp_pid("kiln-browser-x-1"), None);
        // 但合法的其他 kind 可解析(命名约定即注册,无清单可漏)
        assert_eq!(parse_temp_pid("kiln-somefuture-99-1-2"), Some(99));
    }
}
