# ADR-0014: 命名前缀统一为 `vb-`,废弃设计文档中的 `vs-` / `vsm-`

- 状态:已接受(2026-09-09,首次实现时裁定)
- 背景:设计文档集内三种前缀并存:`vsm-artboard`(01 篇)、`vs-artboard` + `vs:artboard` 注释(04 篇示例)、`.vb-artboard`(04 篇 CSS)。
- 决策:**统一 `vb-`**。class:`vb-artboard` / `vb-layer` / `vb-group`;属性:`data-vb-id` / `data-vb-name` / `data-vb-slice`;注释锚点:`<!-- vb:artboard hero -->`。导入兼容旧前缀(`vs-`/`vsm-` 视为同义)。
- 理由:CLI 叫 `vellum-cli`、属性已是 `data-vb-*`、CSS 示例已是 `.vb-artboard`;`vb-` 占多数且与项目代号 Vellum **B**ench 一致。单一前缀是 diff 稳定与 Agent 寻址的前提。
- 后果:文档 01/04 中的旧前缀示例按本 ADR 解读;导入器永久的兼容分支(很小)。
