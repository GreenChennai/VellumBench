//! `vb_render` — 渲染层。
//!
//! 结构(设计文档 05 篇 §十三,ADR-0016):
//! - `encode`:Document → 引擎中立绘制指令 `DrawList`(CPU/GPU 共用,单一来源)
//! - `cpu`:tiny-skia CPU 光栅化(CLI 导出 / CI 渲染快照,无 GPU 依赖)
//! - `gpu`:Vello(wgpu)场景构建(GUI 画布与原生导出)
//! - `text`:COUP-01(ADR-0053)起为 [`vb_textmeasure`] 的纯 re-export
//!   门面——文本量测/整形实现已上收为布局与渲染共同依赖的底层 crate,
//!   下游(`vb_kiln`/`vb_export`/`vb_web`/`vb_app`)调用路径零改动。

pub mod cpu;
pub mod encode;
pub mod gpu;

pub use vb_textmeasure as text;

pub use encode::{BorderDef, DrawItem, DrawKind, DrawList, FillDef, GradientStop};
