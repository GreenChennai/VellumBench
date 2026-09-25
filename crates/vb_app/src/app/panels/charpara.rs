//! 字符面板 `Ctrl+T` 与段落面板 `Ctrl+Alt+T`(04 阶段目标③,design/03 §5.10)。
//!
//! 结构与控制面板同纪律(ADR-VB-U04):**纯函数构建器 + 门禁测试** ——
//! 面板控件只做「投影 → 构建命令 → 经 [`VellumApp::exec`]/[`num_commit`]
//! 入 undo」,不在面板里私存文档状态(值全部回读 `SegStyle` / `Node.style`)。
//!
//! 作用域规则(副文档 04-1-2):
//! - 节点**存在段内 run**(`segments` 非空)→ 字段作用于全部 run(`SegStyle`,
//!   经 `SetSegs` 命令,可撤销);「整段转 run」按钮把全文包成单 run 以支持
//!   创建 run;「移除全部 run」回到整段。
//! - 否则作用于**整段**(`Node.style` 白名单声明,经 `SetStyle`)。
//! - 基线偏移只有 run 落点(`vertical-align` 对块级元素无意义)→ 整段置灰;
//!   语言 / 抗锯齿只有节点级落点(`lang` 属性 / `-webkit-font-smoothing`)
//!   → run 作用域置灰。
//!
//! **不放假控件**(design/03 §5.10 字段逐条裁定见 `04a` 报告白名单处置表):
//! 字偶距 / 垂直缩放 / 水平缩放 / 字符旋转无对称的 CSS 往返落点 → 冻结登记
//! (caption 说明),绝不做「点了没反应」的假输入框。
//!
//! 画布文字仍为 egui 近似(ADR-0017,诚实标注);溢出估算与自动扩高用
//! 导出同款量测(`vb_render::text`,真字形引擎),不受近似影响。
//! 06-1 按职责拆分(纯搬移,零行为变化):纯函数在 `model`,字段清单与
//! 规格在 `fields`,渲染在 `char`/`para`,测试在 `tests`;
//! 对外路径经下方 `pub use` 保持不变。

mod char;
mod fields;
mod model;
mod para;

#[cfg(test)]
mod tests;

pub use model::*;
