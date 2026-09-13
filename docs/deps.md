# 依赖登记(docs/deps.md;设计文档 09 篇 §二:每引入一个 crate 记一行)

| crate | 版本 | 用途 | 许可 | 备注 |
|---|---|---|---|---|
| kurbo | 0.13 | 几何(Vello 原生) | MIT/Apache-2.0 | |
| slotmap | 1 | 场景图 Arena | MIT/Apache-2.0/Zlib | |
| serde / serde_json | 1 | 序列化(.vbproj/patch/MCP) | MIT/Apache-2.0 | |
| thiserror | 2 | 错误定义 | MIT/Apache-2.0 | |
| html5ever | 0.35 | HTML 解析(忠实) | MIT/Apache-2.0 | 与 rcdom 版本对齐 |
| markup5ever_rcdom | 0.35 | 解析 DOM | MIT/Apache-2.0 | TTC/外调属性保真注意 |
| image | 0.25 | 位图解码/缩放(PNG 导入导出) | MIT/Apache-2.0 | |
| tiny-skia | 0.12 | CPU 光栅化(CLI/CI,ADR-0016) | BSD-3 | |
| vello | 0.10 | GPU 2D 渲染(wgpu 29) | MIT/Apache-2.0 | **锁 wgpu 29** |
| wgpu | 29 | GPU 后端(Vulkan 优先) | MIT/Apache-2.0 | 与 vello/egui-wgpu 对齐 |
| eframe / egui | 0.35 | GUI 宿主与 UI | MIT/Apache-2.0 | 0.36 用 wgpu30 与 vello 冲突,**锁 0.35** |
| rfd | 0.17 | 原生文件对话框 | MIT | |
| notify | 8 | 文件监听(v0.6 热重载) | MIT/Apache-2.0? CC0 | |
| clap | 4 | CLI 参数 | MIT/Apache-2.0 | |
| anyhow | 1 | bin 错误处理 | MIT/Apache-2.0 | |
| env_logger / log | 0.11/0.4 | 日志 | MIT/Apache-2.0 | |
| pollster | 0.4 | (预留)阻塞异步 | MIT/Apache-2.0 | |
| resvg / usvg | 0.48 | **仅 dev 依赖**(vb_export):SVG 参考栅格化,三端一致性门禁(门禁 10) | MIT/Apache-2.0 | 自带 tiny-skia 0.11 栈,不进 release 二进制 |
| base64 | 0.23 | vb_export:SVG 位图 data URL 嵌入(B3) | MIT/Apache-2.0 | image 在 vb_export 从 dev 提升为正式依赖(同一用途) |
| flo_curves | 0.8 | **仅 dev 依赖**(vb_tools 测试域):路径布尔 Spike(ADR-0012),批次 C 落地转正式依赖 | MIT/Apache-2.0 | |

目标:release 二进制 + 资源 < 120MB(当前 ~35MB,远低于预算)。
排期:taffy(v0.7 flex)、parley/swash(v0.2 文本)、rmcp 或自研(v0.6 已自研 stdio)、printpdf(v0.5 原生 PDF)。
