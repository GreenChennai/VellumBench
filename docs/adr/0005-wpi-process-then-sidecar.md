# ADR-0005: WPI 用「进程 → 常驻 sidecar」集成,而非嵌入或重写

- 状态:已接受(源自设计文档 13 篇);实现排期 v0.5/v0.6
- 候选:A) PyO3 嵌入 Python;B) Rust 重写导出链路;**C) CLI 进程调用 → 常驻 sidecar(JSON-RPC)**
- 决策:**C**
- 理由:A 构建复杂度翻倍、GIL 与版本耦合;B 重写"浏览器行为经验知识"(reveal-on-scroll、动画冻结、PDF 分页)纯亏;C 隔离崩溃、可复用浏览器实例、WPI 独立演进。
- 后果:需给 WPI 加 `--selector`/`--clip`/`--json` 三参数(分支 `feat/vellum-bridge`);主程序必须在 WPI 缺失时正常工作(原生导出兜底)。
