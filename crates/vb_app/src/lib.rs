//! `vb_app` — Vellum Bench 桌面应用(ADR-0015:eframe 宿主 + Vello 画布纹理合成)。

pub mod app;
/// 能力台账(副文档 09-3:能力/状态/命令 ID 的**单一真相**)。
pub mod capabilities;
pub mod shortcuts;

pub use app::{Tool, VellumApp};
