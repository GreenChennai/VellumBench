//! `vb_agent` 库:patch 操作解析/校验/事务(设计文档 08 篇 §五)。
//!
//! 所有 op 最终落到 `vb_doc::commands::Command` —— 与 GUI 菜单/快捷键同一条
//! 命令路径,天然获得可撤销、可回放、一致性(08 篇 §十)。

pub mod patch;

pub use patch::{apply_patch, PatchError, PatchOp, PatchOutcome, PatchRequest};
