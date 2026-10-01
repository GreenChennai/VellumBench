---
description: 自动开发流水线:拆单 → 并发实现 → 测试/UI/评审门 → 发布
argument-hint: <目标>
---

按当前项目的 saved workflow「dev-loop」执行一轮完整自动迭代。

用 CreateWorkflow 工具以 saved 方式启动:`saved: { name: "dev-loop", args: { goal: "<用户输入的目标>$ARGUMENTS", maxRounds: 3 } }`(goal 用用户原话;若为空则先问一句要做什么)。

启动后向用户简要说明:流水线已开始,包含哪些阶段,预计的子 Agent 数量;运行结果出来后如实转述门禁结论,不要粉饰未通过的门。
