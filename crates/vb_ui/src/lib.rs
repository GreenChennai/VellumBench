//! `vb_ui` — 设计令牌、字体、图标、组件与光标系统。
//!
//! P2 视觉重制（设计文档 14 篇 §3）的地基层：
//! - [`theme`]：深/浅双主题设计令牌 + egui 样式注入
//! - [`fonts`]：Inter → MiSans → 系统 CJK 五族字体 fallback 链
//! - [`icons`]：Lucide 图标语义名映射（iconflow，pack-lucide）
//! - [`components`]：ToolButton / NumField / ColorField / SectionHeader /
//!   PanelTabs / LayerRow 六个组件 + 文本助手
//! - [`cursor`]：工具/手柄 → 系统光标映射
//!
//! 依赖方向：vb_ui 不依赖 vb_app，可被 vb_app 与 vb_agent 复用。

pub mod components;
pub mod cursor;
pub mod fonts;
pub mod icons;
pub mod theme;

pub use components::{
    caption, icon_button, label, mono, strong, ColorField, LayerRow, LayerRowResponse, NumField,
    PanelTabs, SectionHeader, ToolButton,
};
