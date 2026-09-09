//! `vb_common` — 全工作区共享的基础类型。
//!
//! 职责(依赖方向的根,不依赖任何其他 vb_* crate):
//! - 几何:重导出 `kurbo`(ADR-0002,Vello 原生几何库)
//! - 单位:内部一律 px;数值输出 ≤4 位小数去尾随 0(ADR-0007)
//! - 角度:内部 AI 语义(逆时针为正),仅序列化层取负(ADR-0007)
//! - `StableId`:落盘为 `data-vb-id` 的稳定短码,元素全生命周期不变
//! - 颜色:解析/最短 hex 规范化

pub mod color;
pub mod geom;
pub mod id;
pub mod units;

pub use color::Rgba;
pub use id::{SidAllocator, StableId};
