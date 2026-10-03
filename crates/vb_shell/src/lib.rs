//! `vb_shell` — VellumBench 新宿主(R0,22 篇迭代方案 §4 R0 交付 1)。
//!
//! 本 lib 只放**启动器纯逻辑**(过滤 / 排序 / 键盘导航状态机,零 UI 依赖,
//! 单测直测);GPUI 视图与窗口编排都在 bin(`src/main.rs`,二进制
//! `vellum-sable`)。
//!
//! 依赖纪律(G-UI7 壳纯度):vb_shell 禁依赖 `vb_app`(旧 egui 宿主);
//! 对 MRU 数据的消费一律经 `vb_session::mru` 投影。

pub mod launcher;
