//! i18n 取词入口(R0 合并批:骨架 × i18n 地基两分支的交界线)。
//!
//! 22 篇 §4 R0 交付 7:实现本体在 [`vb_common::i18n`](Fluent 底座 + 双语
//! catalog + 204 条命令标签)。此处 re-export,让 `vb_kit`/`vb_shell` 的界面
//! 文案统一经 `vb_session::i18n::t(key)` 取——依赖图上 UI 侧只需要认识
//! vb_session,不直接 import vb_common 的取词内部。
//!
//! 新宿主从第一行代码起禁裸文案(G-UI3;全量双语收口在 R7)。

pub use vb_common::i18n;
