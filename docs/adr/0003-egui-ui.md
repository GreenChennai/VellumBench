# ADR-0003: UI 用 egui,而非 Qt / Tauri / 自绘

- 状态:已接受(源自设计文档 13 篇)
- 候选:A) PySide6/Qt;B) Tauri(Web 前端);C) 自绘 immediate-mode;**D) egui**
- 决策:**D**
- 理由:A 与 Rust 渲染核心跨语言、两个 GPU 上下文;B DOM 与 GPU 合成开销;C 工作量巨大;D 与画布共享同一 wgpu device、单窗口同帧合成、Rust 原生、工具型表单面板够用。
- 后果:复杂自定义控件(图层树拖拽、渐变编辑器)需自绘;AI 风格主题自行实现。
