# ADR-0015: GUI 宿主用 eframe(egui-wgpu),画布经 Vello 离屏纹理合成

- 状态:已接受(2026-09-09,v0.1 实现裁定)
- 背景:设计文档 05 篇要求 winit + egui-wgpu + 手写帧循环,画布与 UI 共享单 wgpu device。05 篇 §11 同时给出两个合成方案:画布直渲 surface + UI 第二 pass(v0.1 建议)vs 画布离屏纹理由 UI 合成(v0.4 建议)。
- 决策:用 **eframe** 作宿主(其内部即 winit + egui-wgpu + 同一 device),画布每帧渲入 Vello 离屏纹理,注册为 egui `TextureId` 由中央面板绘制,UI 面板直接叠加。
- 理由:等价于文档 05 §11 的方案 B(UI 合成画布),省去手写 surface/resize/event 路由约 800 行样板;单 device 约束(文档红线)仍满足;输入路由天然由 egui 处理(面板优先,未消费才给画布)。
- 后果:每帧多一次纹理拷贝(对 2D 编辑器可忽略);若后续需要 Mailbox 低延迟呈现再下沉到手写循环,场景编码层(`vb_render`)不变。
