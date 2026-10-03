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
//! # R0 会话态批次(22 篇 §4 R0 交付 4)
//!
//! - [`capabilities`]:能力台账**数据模型**自 `vb_app::capabilities` 整体下沉;
//!   `vb_app` 侧 re-export,既有代码与测试行为不变。
//! - [`snap`]:**吸附引擎**自 `vb_app::app::canvas_input::snap` 整体下沉
//!   (22 篇 §3.3:吸附/几何求值单源,新旧宿主共用,不许手写第二份);
//!   旧宿主只剩 `VellumApp::smart_snap` 薄转发,行为逐字节一致。
//! - [`selection`] / [`tools`]:**转正** —— 骨架对照旧宿主现实重塑后
//!   (`SelectionState` = 有序 sid 列表;`ToolId` 即旧宿主 `Tool`),
//!   成为 `VellumApp.selection` / `VellumApp.tool` 的**实际字段类型**。
//! - [`command`]:命令**信封**最小实体(稳定 id + 请求/回执线格式);
//!   `run_command` 分发体仍在旧宿主,搬家属 R1(模块文档写明边界)。
//! - [`i18n`]:占位模块(i18n 地基在另一分支落 `vb_common`,合并后此处
//!   re-export,本批不实现)。
//!
//! 纯度门禁:本 crate 源码不得出现任何 UI 栈的路径表达,依赖白名单
//! (vb_common / vb_doc / vb_tools / serde 系)由 `tests/purity.rs` 锁定。

pub mod capabilities;
pub mod command;
pub mod i18n;
pub mod selection;
pub mod snap;
pub mod tools;

pub use capabilities::{CapStatus, Capability, CapabilityUi, CAPABILITIES};
