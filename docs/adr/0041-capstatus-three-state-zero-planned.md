# ADR-0041: 能力台账三态收敛 —— Planned 归零,Partial 写明缺口,Dropped 仅限根本冲突

- 状态:已接受(阶段 5 / 05-1;用户确认"连四件大件也做";兑付迭代附录
  ADR-VB-L09;阶段 6 补 Planned=0 硬门禁)
- 背景:台账一度 14 条 `Planned`,均为"计划于 v2/v2.x/v3" —— **没有交付
  日期的承诺本身就是技术债**,且台账是 README / 窗口 / 门禁的单一真相,
  悬空状态会污染全部下游口径。
- 决策:
  1. `CapStatus` 收敛为 `Done` / `Partial(缺哪一半+去向)` / `Dropped(理由)`;
     `Planned` 保留为枚举值但**门禁断言计数 = 0**(`no_dangling_planned`),
     合法出路只有:本轮做掉 → `Done`;缺半 → `Partial`(说明缺哪一半、
     去向,这是**合法态**);根本冲突 → `Dropped`;
  2. `Dropped` 门槛极高:**仅限**与 HTML / 产品定位根本冲突者(现两条:
     实时上色族 / 云端账号),且门禁锁死三条 —— 理由非空且不得含"计划于"、
     数量 ≤2、UI 无任何命令入口(命令注册表与快捷键不得出现);
  3. 台账里列出的每个命令 id 必须是已注册命令(既有门禁承接),
     状态改动必须与代码同 commit。
- 落地:`crates/vb_app/src/capabilities.rs`(枚举 + 台账 + 四条门禁测试:
  `no_dangling_planned` / `partial_items_explain_the_gap` /
  `dropped_items_are_limited_and_reasoned` / `dropped_items_have_no_command_entry`);
  README「已落地 / 明确不做 / 未落地」三段与台账同源。
- 取舍:承认"不做"可能显得能力缩水;换来口径诚实、无悬空承诺。
- 被否决替代:继续挂 `Planned`(悬空);把大件一律 `Dropped`(被用户否决,
  四件大件 09-H/I/J/K 本轮实际落地)。
