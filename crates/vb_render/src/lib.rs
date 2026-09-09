//! `vb_render` — 渲染层。
//!
//! 结构(设计文档 05 篇 §十三,ADR-0016):
//! - `encode`:Document → 引擎中立绘制指令 `DrawList`(CPU/GPU 共用,单一来源)
//! - `cpu`:tiny-skia CPU 光栅化(CLI 导出 / CI 渲染快照,无 GPU 依赖)
//! - `gpu`:Vello(wgpu)场景构建(GUI 画布与原生导出)

pub mod cpu;
pub mod encode;
pub mod gpu;

pub use encode::{BorderDef, DrawItem, DrawKind, DrawList, FillDef, GradientStop};
