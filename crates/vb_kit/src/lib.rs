//! `vb_kit` — sable 面板与控件(22 篇迭代方案 §3.1:29+ 面板按 sable 规格
//! 重写的落点;VB 专属控件、动画接线、文案投影都在这里)。
//!
//! R0/R1 接线范围(22 篇 §4):
//! - [`tokens`]:`vb-ui-tokens.json` → sable `ColorTokens` 注入 + 全量数值
//!   镜像(JSON 是唯一真相,同步由门禁测试把关);
//! - [`capabilities_panel`]:能力台账面板 —— 「面板 = 纯投影」模式的首个
//!   示范:数据 100% 来自 `vb_session::capabilities`,面板零自有台账。
//! - [`canvas_panel`]:项目画布与基础编辑命令,消费 `vb_session` 投影出的
//!   `DrawList`;保真差在界面显式提示,文档仍只由会话持有。
//! - [`layers_panel`]:稳定 SID 图层树只读投影,选择写回同一个 `ProjectSession`。
//! - [`inspector_panel`]:Sable 属性行/数值框经会话命令编辑单选几何。
//! - [`route_gpui`]:从 R0 上屏 spike 抽出的 GPUI 图元适配,生产画布与 spike 共用。
//! - [`command_palette`]:从根目录 `commands.yaml` 投影新宿主已支持命令至 Sable 搜索面板。

//! # 依赖纪律(G-UI7 壳纯度,`tests/shell_purity.rs` 把关)
//!
//! 本 crate **禁依赖**文档模型/窑炉两个实体 crate 与旧 egui 栈;对文档语义
//! 的一切消费须经 `vb_session` 投影。吸附/几何/动画求值不在此手写第二份
//! (22 篇 §3.3 壳不持真相)。若新增 `src/*.rs` 触碰红线,门禁测试直接红。
//!
//! hex 颜色字面量只允许出现在 [`tokens`](crate::tokens)(G-UI2 雏形)。

pub mod animation_panel;
pub mod canvas_panel;
pub mod capabilities_panel;
pub mod command_palette;
pub mod inspector_panel;
pub mod layers_panel;
pub mod route_gpui;
pub mod tokens;
