---
description: 只跑 UI 质量闭环:审计 → 修复 → 复检,直到界面合格
argument-hint: [范围,如"设置页"或留空全项目]
---

按当前项目的 saved workflow「fix-ui」执行 UI 修复闭环。

用 CreateWorkflow 工具以 saved 方式启动:`saved: { name: "fix-ui", args: { scope: "<$ARGUMENTS 或空串>", maxRounds: 2 } }`。

启动后向用户说明:会先静态审计再启动应用实测,发现问题按 P0/P1/P2 修复并独立复检;结果出来后如实转述,剩余问题列清楚。
