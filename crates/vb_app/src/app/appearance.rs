//! 外观面板(`⇧F6`,05-1)与描边面板(`^F10`,05-3)+ 效果写回(05-6)。
//!
//! 结构与控制面板同纪律(ADR-VB-U04):**纯函数模型/编解码/构建器 + 门禁
//! 测试**,渲染层只做「投影 → 构建命令 → [`VellumApp::exec`]/[`num_commit`]」,
//! 不在面板私存文档状态。
//!
//! # 外观条目模型与 CSS 落盘机制(05-1 核心设计)
//!
//! AI 的「外观」是条目列表:填充/描边/效果可多条、可排序、可禁用;而 CSS
//! 天然单背景/双边框。本模块的裁定(详见 `05a` 报告):
//!
//! 1. **模型真值** = 元素属性 `data-vb-appearance`(紧凑 JSON,经
//!    `SetAttrs` 落盘;导入/导出对未知 `data-*` 属性原样往返,故条目的
//!    顺序/眼睛/混合模式/冻结字段**无损**穿越 HTML 管线);
//! 2. **渲染真值** = 按映射表从启用条目**编译**出的 CSS 声明
//!    (经 `SetStyle` 落盘):多条填充 → `background-image` 逗号多层叠加
//!    (纯色层以同色双 stop 渐变承载,像素恒等)、多条阴影 → `box-shadow`
//!    逗号分段、多个模糊 → `filter` 空格串联;
//! 3. **接管集 `own`** 记录「哪些属性组已由模型接管」:编译只重写接管组
//!    (清空 = 移除声明),未接管的手写声明(如属性面板直改的
//!    `border-radius`)绝不动 —— 不静默丢弃、不双写打架;
//! 4. **无损裁定**:接管组内启用条目编译 ⇄ 解码恒等(单元测试锁);
//!    无 CSS 落点的字段(描边箭头、条目级混合对描边/效果、跨类效果顺序)
//!    保存在模型中并如实标注「未落盘」。
//!
//! 解码优先级:有 `data-vb-appearance` → 纯 JSON(模型无损);无 → 从
//! CSS 启发式解码(导入既有文件;只认领可完整表达的声明)。
//!
//! 命令路径:每次条目操作 = `Compound[SetStyle(重编译), SetAttrs(模型)]`
//! 一条 undo;`merge_target` 已放行同目标 `SetStyle+SetAttrs` 混合
//! Compound,NumField 会话合并(拖数值只产生一条 undo)照常生效。

//! 06-1 按职责拆分(纯搬移,零行为变化):条目模型与目标类型在 `model`,
//! 效果↔CSS 片段映射在 `segments`,编解码在 `codec`,命令构建器与不支持
//! 登记在 `commands`,渲染在 `ui`(描边面板在 `ui::stroke`),测试在 `tests`;
//! 对外路径经下方 `pub use` 保持不变。

mod codec;
mod commands;
mod model;
mod segments;
mod stroke;
mod ui;

#[cfg(test)]
mod tests;

pub use codec::*;
pub use commands::*;
pub use model::*;
pub use segments::*;
