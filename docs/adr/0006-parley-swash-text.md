# ADR-0006: 文本用 Parley + Swash(保留回退位)

- 状态:已接受(源自设计文档 13 篇);实现排期 v0.2
- 决策:主用 Parley(布局/整形)+ Swash(光栅)+ 自建图集;抽象 `trait TextShaper`。
- 理由:Rust 原生、与 Vello 同源、支持中文与复杂脚本。
- 后果:若质量/性能不达标,回退路径:`fontdue`/`skia-safe` CPU 光栅进图集(接口已抽象,改动局部)。
- v0.1 说明:文本管线未接入前,画布内文本对象用 egui 近似绘制(见 ADR-0017);HTML 导出不受影响(文本是真实 HTML)。
