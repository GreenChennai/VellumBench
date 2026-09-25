//! `vb_doc` — 单一真相 `Document` 与一切变更的唯一入口 `Command`(设计文档 09 篇)。
//!
//! 依赖方向:`vb_doc ──▶ vb_html / vb_css / vb_common`;不依赖任何 GPU/UI crate,
//! 保证可无头测试(设计文档 09 篇 §一)。
//!
//! - `model`:场景图(Arena + 稳定 sid)
//! - `commands`:可逆命令(apply/revert)+ ChangeSet
//! - `undo`:Undo/Redo 栈(合并策略:同 kind + 同 target + 500ms)
//! - `import`:项目目录(index.html + styles/*.css)→ Document
//! - `export`:Document → canonical HTML/CSS(与导入构成往返)

/// K4 协同会话合并层(05-11-4,ADR-0031):连续属性 LWW CRDT + 共享目录
/// 传输;结构性变更不走此层(命令 + 三向对比 + rev 乐观锁)。
pub mod collab;
pub mod commands;
pub mod export;
pub mod import;
pub mod model;
/// 05-8 符号 / 组件系统(模型口径、覆盖登记、主件同步与五个符号命令)。
pub mod symbol;
pub mod undo;

pub use commands::{ChangeSet, CmdKind, Command};
pub use model::{Document, Node, NodeKind, NodeSlot, OutputMode, TextMode};
pub use undo::UndoStack;

#[derive(Debug, thiserror::Error)]
pub enum VbError {
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("解析失败: {0}")]
    Parse(String),
    #[error("节点不存在: {0}")]
    NoSuchNode(String),
    #[error("冲突: {0}")]
    Conflict(String),
    #[error("不支持: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, VbError>;
