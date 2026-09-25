//! 控制面板(S1-c 02-2,AI 招牌)+ 面板元数据。
//!
//! 职责分三层(ADR-VB-U04:**工具只声明,不各画各的**):
//!
//! 1. **Spec(纯数据)**:[`ControlPanelSpec`] 描述"当前工具态应有哪些
//!    字段"(标签 / 控件种类 / 写回命令);由纯函数 [`spec_for`] 按
//!    `design/03 §三` 的八工具态字段表生成 —— 无 egui、无文档依赖,
//!    门 1 单测直接断言字段集合;
//! 2. **共享写回构建器(纯函数)**:[`geom_field_cmds`] /
//!    [`style_prop_cmds`] / [`attr_cmds`] / [`rotation_cmds`] /
//!    [`rename_cmds`] / 渐变系列 —— 面板控件提交时调它构造文档命令,
//!    门 2 文档状态级测试走同一条路径(`UndoStack::push` + 断言
//!    Document/CSS),保证"面板上每个控件真的写文档";
//! 3. **渲染器**:[`VellumApp::control_bar`] 菜单栏下 40px 通栏,统一
//!    消费 spec(按字段 id 取值 / 提交),右侧固定区:文档标题(改名,
//!    经 `SetMetaTitle`)、画板切换下拉、缩放下拉。
//!
//! **不做"点了没反应"**:design/03 字段表里暂无命令支撑的项(倾斜、
//! 参考点九宫格、色标编辑、浏览器校对按钮等)**不进 spec**,登记为
//! 阶段 2/4/5/8 遗留项(见 02c 报告);提示性条目用 `Hint`(非交互,
//! 不属于"点了没反应")。
//! 06-1 按职责拆分(纯搬移,零行为变化):字段规格在 `spec`,共享写回
//! 构建器在 `writes`,渲染器在 `render`/`editors`,门禁测试在 `tests`;
//! 对外路径经下方 `pub use` 保持不变。

mod editors;
mod render;
mod spec;
mod writes;

#[cfg(test)]
mod tests;

pub use spec::{spec_for, ControlPanelSpec, CtlCtx, CtlField, CtlKind, CtlWrite, FIXED_FIELDS};
pub use writes::{
    attr_cmds, build_gradient, combine, geom_axes_cmd, geom_field_cmds, gradient_angle_cmds,
    gradient_kind_cmds, gradient_reverse_cmds, parse_gradient, rename_cmds, rotation_cmds,
    rotation_deg_of, style_prop_cmds, style_prop_remove_cmds, GeomAxis, GradKind,
};
