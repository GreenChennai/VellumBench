---
description: 发布门:门禁 + 评审 + 冒烟,全绿才打版本 tag
argument-hint: [版本号,如 v1.2.0,留空自动 bump]
---

按当前项目的 saved workflow「ship」执行发布门。

用 CreateWorkflow 工具以 saved 方式启动:`saved: { name: "ship", args: { version: "<$ARGUMENTS 或空串>", skipReview: false } }`。

启动后向用户说明:会跑全量测试/构建、ocr+独立评审、按 RELEASE.md 冒烟,全绿才打 tag;被拦下时如实说明是哪道门、为什么。
