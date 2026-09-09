# ADR-0002: 渲染用 wgpu + Vello,而非裸 Vulkan / Skia

- 状态:已接受(源自设计文档 13 篇)
- 候选:A) 裸 Vulkan(ash);B) Skia(Vulkan 后端);**C) wgpu + Vello**;D) Qt Quick
- 决策:**C**
- 理由:A 需 5000+ 行样板且无跨平台回退;B 构建重、Rust 绑定滞后、无并行编码;D 与 UI 框架绑定且 2D 路径性能一般。C:Vulkan/Metal/DX12/GL 全覆盖、GPU 全量光栅 + 并行编码、与 egui 共享 device。
- 后果:依赖 linebender 生态(API 可能变化);文本需自行集成 Parley;`vello_cpu` 用于 CI 渲染快照(见 ADR-0016)。
