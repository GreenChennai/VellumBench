//! `vb_session` — 宿主无关会话态层(22 篇迭代方案 §3.1:从 `vb_app` 提取)。
//!
//! 定位:纯数据 + 纯函数,**零 UI 依赖**(不依赖 egui/GPUI 任何一侧)。
//! 依赖方向硬规则(22 篇 §3.1,进 CI):
//!
//! ```text
//! vb_doc ← vb_session ← vb_kit ← vb_shell
//! ```
//!
//! `vb_kit`/`vb_shell` 与文档模型内部类型的一切交互都必须经本 crate 投影 ——
//! 这是对旧宿主"面板摸 `VellumApp` 私有字段"耦合模式的总清算。
//!
//! # R0 骨架批次范围(22 篇 §4 R0 交付 4 的第一步)
//!
//! - [`capabilities`]:能力台账**数据模型**自 `vb_app::capabilities` 整体下沉;
//!   `vb_app` 侧 re-export,既有代码与测试行为不变。
//! - [`mru`]:最近项目(MRU)持久化自 `vb_app::recent` 整体下沉(含
//!   v0.14 旧文件向后兼容用例);`vb_app` 侧 re-export,行为零变化。
//! - [`selection`] / [`tools`]:选中态与工具状态机**骨架**(纯数据 + 纯函数)。
//!   **vb_app 全量迁移是后续批次,本批只立骨架** —— 画布手势、吸附引擎、
//!   命令分发、撤销栈门面等仍在旧宿主,按轮次迁入。
//! - [`i18n`]:占位模块(i18n 地基在另一分支落 `vb_common`,合并后此处
//!   re-export,本批不实现)。

pub mod capabilities;
pub mod i18n;
pub mod mru;
pub mod selection;
pub mod tools;

pub use capabilities::{CapStatus, Capability, CapabilityUi, CAPABILITIES};
