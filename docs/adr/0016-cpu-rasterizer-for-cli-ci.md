# ADR-0016: CLI/CI 的光栅化走独立 CPU 引擎(`vb_render::cpu`),GUI 走 GPU(Vello)

- 状态:已接受(2026-09-09,v0.1 实现裁定)
- 背景:设计文档 10 篇要求渲染快照用 `vello_cpu`(CI 无 GPU 可跑);08 篇要求 CLI `shot`/`export` 平均往返 < 3s 且可在无头环境工作。
- 决策:`vb_render::cpu` 提供无 GPU 光栅化引擎(vello_cpu 优先;若其 API 不满足渐变/圆角/描边需求则以 tiny-skia 实现,内部以 `CpuRenderer` 封装,调用方无感),CLI 导出与 CI 快照共用;GUI 画布与 GUI 内导出走 Vello(wgpu)。
- 理由:无头环境(服务器/CI/Agent 批量)不该依赖驱动与窗口系统;CPU 引擎同时是渲染快照基线的稳定来源。
- 后果:CPU 与 GPU 引擎存在视觉差异面(渐变抖动、抗锯齿),由「校对视图」(v0.5)量化;两引擎共享同一个"文档 → 绘制指令"编码层,差异面被压缩到光栅层。
