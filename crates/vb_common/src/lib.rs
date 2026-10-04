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
/// Fluent i18n 取词底座(R0 第 7 条;设计 22 §4):`i18n/zh.ftl` / `en.ftl`
/// 编译期内嵌,t/t_args/set_language,缺词回退链显式可见。
pub mod i18n;
pub mod id;
/// 文本小工具(BOM 剥除、percent 解码;跨 crate 单源)。
pub mod text;
/// 数值格式化单源(EXP-07):PDF/SVG 两车道共用坐标精度函数 `fnum`。
pub mod numfmt;
/// `transform` 平移分量解析(导入折算与导出补偿的**唯一口径**)。
pub mod transform;
pub mod units;

/// 树形结构统一递归深度上限(RB-02 / 审查 DOC-01 的单一真相):
/// HTML 解析/序列化、文档建树、子树立、calc 括号深度一律以本值为准,
/// 超限必须显式报错或告警,**不许静默截断**。512 对合法文档余量充足
/// (浏览器同款嵌套上限量级),对线程栈亦安全(512 帧远小于栈空间)。
pub const MAX_TREE_DEPTH: usize = 512;

pub use color::Rgba;
pub use id::{SidAllocator, StableId};
