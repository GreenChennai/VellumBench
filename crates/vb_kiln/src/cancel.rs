//! 协作式取消(硬骨头 #6,R0):导出全链路的取消令牌与边界检查。
//!
//! ## 语义
//!
//! - **协作式**:取消不会打断任何阻塞调用(截图/ffmpeg/浏览器进程),
//!   只在管线预设的**边界检查点**生效:浏览器截图车道每帧边界、
//!   WebCodecs 车道每批边界、native 车道每分段(帧落盘/ffmpeg 调用)
//!   边界、GIF 内存车道每帧边界。检查点粒度 = 取消延迟上界。
//! - **三态**:成功 / 失败([`crate::KilnError`])/**取消**显式区分。
//!   native 车道直接返回 [`KilnError::Cancelled`];浏览器动画车道以
//!   `String` 报错,取消经 [`lane_cancelled`] 标记串区分
//!   ([`is_lane_cancelled`] 判定),下游(kiln-cli)据此给退出码 130。
//! - **不留半截产物**:最终产物只在完整成功后经 [`crate::write_atomic`]
//!   原子落盘;取消路径负责清扫临时/分段文件(anim 工作目录、WebCodecs
//!   sink、frames.rs 临时帧目录),与既有失败清扫同路径。
//! - **进程不泄漏**:取消时已拉起的 ffmpeg 子进程显式 kill + wait 收尸;
//!   浏览器实例靠 `vb_browser::BrowserProcess::Drop`(taskkill /T /F 探活
//!   路径)随 worker 线程退出整树收割。
//!
//! ## 用法
//!
//! ```no_run
//! use vb_kiln::cancel::CancelToken;
//! let token = CancelToken::new();
//! let seg = token.child();          // 分段任务拿派生句柄
//! let h = std::thread::spawn(move || {
//!     for frame in 0..1000 {
//!         if seg.is_cancelled() { break; } // 帧边界检查
//!         // …渲染一帧…
//!     }
//! });
//! token.cancel();                   // 任意线程触发,全部句柄可见
//! h.join().unwrap();
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::KilnError;

/// 协作式取消令牌:`Arc<AtomicBool>` 语义的共享旗标。
///
/// 克隆 / [`CancelToken::child`] 与父共享同一旗标——任意句柄 `cancel()`
/// 即对全部句柄立即可见(含已拉起的分段 worker);开销为一次原子读。
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
}

impl CancelToken {
    pub fn new() -> Self {
        CancelToken {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 是否已请求取消(边界检查点轮询)。
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Relaxed)
    }

    /// 请求取消:幂等,任意线程调用,对所有共享句柄立即生效。
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    /// 派生子句柄:与父共享同一取消旗标,便于把独立句柄值下发给分段
    /// 任务 / worker(父 `cancel()` 自动传播到全部 child)。若未来需要
    /// 「只取消某个分段」的细粒度语义,在此扩展层级链,调用方不变。
    pub fn child(&self) -> Self {
        self.clone()
    }
}

/// native 车道边界检查:已取消则返回 [`KilnError::Cancelled`]。
pub fn guard_kiln(token: &Option<CancelToken>, checkpoint: &str) -> Result<(), KilnError> {
    if let Some(t) = token {
        if t.is_cancelled() {
            return Err(KilnError::Cancelled);
        }
    }
    let _ = checkpoint; // 检查点标签仅调试期人读;保持签名稳定供日志扩展
    Ok(())
}

/// 浏览器动画车道(String 报错)边界检查:已取消则返回取消标记串。
pub fn guard_lane(token: &Option<CancelToken>, checkpoint: &str) -> Result<(), String> {
    if let Some(t) = token {
        if t.is_cancelled() {
            return Err(lane_cancelled(checkpoint));
        }
    }
    Ok(())
}

/// 车道取消标记串(anim 车道以 String 报错;取消与失败共用错误通道,
/// 经此前缀区分三态)。勿在别处硬编码同文案,一律经本模块构造/判定。
pub const LANE_CANCELLED_PREFIX: &str = "导出已取消";

/// 构造车道取消错误(带检查点标注,便于日志定位取消生效位置)。
pub fn lane_cancelled(checkpoint: &str) -> String {
    format!("{}(边界:{checkpoint})", LANE_CANCELLED_PREFIX)
}

/// 判定车道错误串是否为取消(区别于真失败)。
pub fn is_lane_cancelled(err: &str) -> bool {
    err.starts_with(LANE_CANCELLED_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_token_not_cancelled_and_cancel_is_idempotent() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled(), "cancel 必须幂等");
    }

    #[test]
    fn child_shares_flag_and_propagates() {
        let parent = CancelToken::new();
        let c1 = parent.child();
        let c2 = c1.child();
        assert!(!c1.is_cancelled() && !c2.is_cancelled());
        c2.cancel();
        assert!(parent.is_cancelled(), "child.cancel 必须传播到 parent");
        assert!(c1.is_cancelled());
        // 再派生也立即处于取消态
        assert!(c1.child().is_cancelled());
    }

    #[test]
    fn clone_is_same_flag_default_is_fresh() {
        let a = CancelToken::new();
        let b = a.clone();
        b.cancel();
        assert!(a.is_cancelled());
        assert!(!CancelToken::default().is_cancelled());
        assert!(!CancelToken::default().child().child().is_cancelled());
    }

    #[test]
    fn cancel_visible_across_threads() {
        let t = CancelToken::new();
        let child = t.child();
        let (tx, rx) = std::sync::mpsc::channel();
        let h = std::thread::spawn(move || {
            // 边界轮询:未取消时最多自旋到上限(防挂死测试)
            for _ in 0..10_000_000 {
                if child.is_cancelled() {
                    tx.send(true).unwrap();
                    return;
                }
                std::hint::spin_loop();
            }
            tx.send(false).unwrap();
        });
        t.cancel();
        assert!(rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap());
        h.join().unwrap();
    }

    #[test]
    fn guards_distinguish_cancelled_from_ok() {
        let none: Option<CancelToken> = None;
        assert!(guard_kiln(&none, "x").is_ok());
        assert!(guard_lane(&none, "x").is_ok());

        let t = CancelToken::new();
        let some = Some(t.child());
        t.cancel();
        assert!(matches!(
            guard_kiln(&some, "native 分段"),
            Err(KilnError::Cancelled)
        ));
        let lane_err = guard_lane(&some, "帧 12").unwrap_err();
        assert!(is_lane_cancelled(&lane_err), "{lane_err}");
        assert!(lane_err.contains("帧 12"), "检查点标注应保留:{lane_err}");
        assert!(!is_lane_cancelled("ffmpeg 编码失败: …"));
    }
}
