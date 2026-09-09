# ADR-0008: Undo 用命令模式,而非快照;不提前引入 CRDT

- 状态:已接受(源自设计文档 13 篇)
- 决策:`Command{apply, revert}` + ChangeSet;Undo 栈内存上限 200MB,溢出写盘。v3 再评估 CRDT。
- 理由:命令模式同时满足 Undo、Agent 回放、崩溃恢复、事务;过早引入 CRDT 会污染文档模型设计。
- 后果:v3 做协作时需为 CRDT 改造(已预留:`sid` 稳定 id 天然适合 CRDT 键)。
