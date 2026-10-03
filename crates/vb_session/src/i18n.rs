//! i18n 取词入口(R0 合并批:骨架 × i18n 地基两分支的交界线)。
//!
//! 22 篇 §4 R0 交付 7:实现本体在 [`vb_common::i18n`](Fluent 底座 + 双语
//! catalog + 204 条命令标签)。此处 re-export,让 `vb_kit`/`vb_shell` 的界面
//! 文案统一经 `vb_session::i18n::t(key)` 取——依赖图上 UI 侧只需要认识
//! vb_session,不直接 import vb_common 的取词内部。
//!
//! 新宿主从第一行代码起禁裸文案(G-UI3;全量双语收口在 R7)。

/// 取词 API 条目级再导出:UI 侧统一经 `vb_session::i18n::t(key)` 取词
/// (本模块文档声明的路径;只再导出条目而非整个模块,`vb_session::i18n`
/// 即取词门面,不产生 `i18n::i18n` 双层路径)。
pub use vb_common::i18n::{init, language, set_language, t, t_args, try_t, try_t_args, Lang};

/// `t_args` 实参类型再导出:UI 侧(`vb_kit`/`vb_shell`)按依赖纪律只认识
/// `vb_session`,不必为拼一条插值文案直依赖 `fluent`(vb_common 已是
/// 唯一的 fluent 消费者)。
pub use fluent::FluentValue;
