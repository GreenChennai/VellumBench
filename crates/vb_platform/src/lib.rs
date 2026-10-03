//! `vb_platform` — 平台服务 seam(硬骨头 #13,22 篇 §3.7,R0 实体化)。
//!
//! 目的:让 `vb_session` 与 `vb_kit` 里不再出现任何 `std::path` 之外的 OS
//! 直呼,Windows 特有逻辑(深色标题栏、Mica、感知 DPI)集中一处。宿主
//! (vb_app/egui、vb_shell/gpui)各给一套实现,面板与逻辑层只面向 trait。
//!
//! ## 六 trait(签名一览)
//!
//! | trait | 方法 | 宿主语义 |
//! |---|---|---|
//! | [`WindowHandle`] | `id` / `set_title` / `activate` | 窗口标识与前台化 |
//! | [`Clipboard`] | `set_text` / `text` | 系统(或 egui 命令通道)剪贴板 |
//! | [`FileDialog`] | `pick_folder` / `pick_file` / `save_file` | 目录/文件选取 |
//! | [`SystemCursor`] | `set_shape` | 画布工具随动光标 |
//! | [`DisplayInfo`] | `displays` | **每显示器** DPI/分辨率(画布清晰度预算) |
//! | [`DarkModeProbe`] | `is_dark_mode` | 系统深色外观探测(主题初值) |
//!
//! ## 实现
//!
//! - `null`(恒可用):内存态 fallback,供测试与 headless——全接口可走通,
//!   见 `tests/walkthrough.rs` 的真实用例(复制路径/工具光标/DPI 预算/主题初值)。
//! - feature `egui`:vb_app 宿主。egui Context 薄封装;剪贴板写走 egui
//!   命令通道、读走 arboard 真 OS 剪贴板(egui 0.35 不暴露系统剪贴板读);
//!   文件对话框复用 rfd(vb_app 现用件)。
//! - feature `gpui`:vb_shell 宿主。经 `sable::gpui`(版本与宿主同锁);
//!   gpui 0.2.2 缺失的能力(运行期改标题/缩放/取色光标)按
//!   [`PlatformError::Unsupported`] **诚实降级**,不静默假装成功(ADR-0046)。
//!
//! 依赖方向(22 篇 §3.1):本 crate 不依赖任何 vb 实体 crate;宿主 crate
//! (vb_app/vb_shell)依赖本 crate。vb_kit purity 门禁锁定 vb_kit 源码零
//! `windows::`/`winit::` 直引(完成定义)。
//!
//! 不用 `#[serde]`/` Clone` 除非必要;全部 trait 均对象安全(`dyn` 可派发)。

pub mod error;
pub mod null;
pub mod traits;
pub mod types;

#[cfg(feature = "egui")]
pub mod egui_backend;
#[cfg(feature = "gpui")]
pub mod gpui_backend;

pub use error::PlatformError;
pub use null::{NullClipboard, NullCursor, NullDarkMode, NullDialog, NullDisplay, NullWindow};
pub use traits::{Clipboard, DarkModeProbe, DisplayInfo, FileDialog, SystemCursor, WindowHandle};
pub use types::{CursorShape, DisplayMetrics, FileFilter};
