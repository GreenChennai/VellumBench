//! 浏览器车道等待取消(硬骨头 #3 收口,R0):静态快照导出的分片轮询。
//!
//! ## 语义(与 `vb_kiln::cancel` 三态对齐)
//!
//! - **协作式分片**:CDP 长等待(截图 180s / printToPDF 300s / beginFrame
//!   60s / navigate 35s)不再一把堵死——等待循环按 **≤100ms 泵片**推进,
//!   每片之间查一次取消探针([`CancelProbe`])。取消延迟上界 = 一个泵片 +
//!   当前步骤收尾(<1s,硬骨头 #3 完成定义「任意阶段取消 <1s 生效」)。
//! - **三态**:取消错误串以 [`WAIT_CANCELLED_PREFIX`] 开头,与真失败可判;
//!   `vb_kiln::cancel::LANE_CANCELLED_PREFIX` **就是本常量的别名**(编译期
//!   单源,两 crate 字面量不得漂移),下游 `is_lane_cancelled` 原样可用。
//! - **收割**:取消路径上 `BrowserProcess` 照常随作用域 Drop(kill_tree 整树
//!   收割 + 临时 user-data 清理),不留半截浏览器进程;产物只在完整成功后
//!   由调用方 `write_atomic` 落盘,取消不写任何输出文件。
//! - **迟到响应**:取消瞬点后迟到的 CDP 响应留在 `Cdp.responses` 表(下次
//!   call 用新 id,永不误配),随连接关闭释放——一次取消至多滞留一条,
//!   有界且无害。

use std::sync::Arc;

/// 车道取消错误前缀(vb_kiln::cancel::LANE_CANCELLED_PREFIX 的编译期单源)。
pub const WAIT_CANCELLED_PREFIX: &str = "导出已取消";

/// 取消探针:调用方注入的「是否已请求取消」谓词(vb_kiln 用 CancelToken
/// 实现,本 crate 不依赖 vb_kiln——依赖方向 vb_kiln → vb_browser)。
pub type CancelProbe = Arc<dyn Fn() -> bool + Send + Sync>;

/// 构造车道取消错误(带位置标注,便于日志定位取消生效点)。
pub fn wait_cancelled(what: &str) -> String {
    format!("{}(浏览器等待中止:{what})", WAIT_CANCELLED_PREFIX)
}

/// 判定错误串是否为取消(区别于真失败)。
pub fn is_wait_cancelled(err: &str) -> bool {
    err.starts_with(WAIT_CANCELLED_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_is_prefix_discriminable() {
        let e = wait_cancelled("Page.captureScreenshot 等待响应");
        assert!(is_wait_cancelled(&e), "{e}");
        assert!(e.contains("Page.captureScreenshot"), "位置标注应保留:{e}");
        assert!(!is_wait_cancelled("Page.navigate 传输失败: 连接断开"));
        assert!(!is_wait_cancelled("截图数据解码失败: bad base64"));
    }

    #[test]
    fn probe_closure_reports_cancel() {
        let hit = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let probe: CancelProbe = {
            let hit = hit.clone();
            Arc::new(move || hit.load(std::sync::atomic::Ordering::Relaxed))
        };
        assert!(!probe());
        hit.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(probe());
    }
}
