//! `vb_ui` — 设计令牌、字体、图标、组件与光标系统。
//!
//! P2 视觉重制（设计文档 14 篇 §3）的地基层：
//! - [`theme`]：深/浅双主题设计令牌 + egui 样式注入
//! - [`fonts`]：Inter → MiSans → 系统 CJK 五族字体 fallback 链
//! - [`icons`]：Lucide 图标语义名映射（iconflow，pack-lucide）
//! - [`components`]：ToolButton / NumField / ColorField / SectionHeader /
//!   PanelTabs / LayerRow 组件 + 文本助手(S1-b:NumField scrubby+表达式、
//!   ColorField 取色器浮窗)
//! - [`expr`]：数值框数学表达式解析器(纯函数,02-6-1)
//! - [`dock`]：面板坞折叠规则与宽度钳制(纯函数,02-1)
//! - [`toast`]：可堆叠通知(错误可复制,02-6-6)
//! - [`motion`]：一次性入场动效(对话框/Tab 淡入,H-1;总开关在 [`theme`])
//! - [`gradient`]：渐变结构化模型 + **自绘色标条**(阶段 4 / 05-2,V4 决策)
//! - [`cursor`]：工具/手柄 → 系统光标映射
//!
//! 依赖方向：vb_ui 不依赖 vb_app/vb_doc,可被 vb_app 与 vb_agent 复用
//! (颜色解析借 vb_common,是既有底层设施)。

pub mod components;
pub mod cursor;
pub mod dock;
pub mod expr;
pub mod fonts;
pub mod gradient;
pub mod icons;
pub mod motion;
pub mod theme;
pub mod toast;

pub use components::{
    caption, dialog_footer, dialog_footer_btn3, icon_button, key_badge_text, label, mono, strong,
    ColorField, ColorFieldResponse, LayerRow, LayerRowResponse, NumField, NumFieldResponse,
    PanelTabs, SectionHeader, TabsResponse, ToolButton,
};
pub use gradient::{gradient_bar, GradKind, Gradient, GradientBarResponse, Stop};
pub use toast::{Toast, ToastHost, ToastKind};
