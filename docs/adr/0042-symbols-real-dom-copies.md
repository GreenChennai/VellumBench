# ADR-0042: 组件(符号)= 实例为真实 DOM 副本 + 主件同步,禁 JS 依赖

- 状态:已接受(阶段 5-8 / 05-8;兑付迭代附录 ADR-VB-L10,主文档 W12)
- 背景:AI 的"符号 / 组件"要落到"可读、可 diff、浏览器直接渲染"的 HTML 上;
  Web Components 方案需要 JS 才能渲染,与「导出零 JS、HTML 即真相」冲突。
- 决策:
  1. **主件原型**存文档内 `<div class="vb-symbol-defs" hidden>` 容器
     (原生 hidden 属性,零 JS),导出侧补写、导入侧回读;
  2. **实例 = 真实 DOM 副本**(不是 `<symbol>` 引用 / 不是运行时展开):
     覆盖字段写 `data-vb-symbol-overrides`,HTML 拿到手不经任何处理即可
     渲染、可 diff、可被任意 Agent 继续编辑;
  3. 主件同步(改主件 → 各实例跟进)走"一次操作多节点"事务
     (`Command::MultiResult`),单次撤销;创建 / 分离 / 重置覆盖 /
     替换主件定义 / 选择所有实例均有命令入口;
  4. 诚实边界:跨文档复用(组件库)留后续复议,台账 09-H = Partial 写明。
- 落地:`crates/vb_doc/src/symbol.rs`(模型)、`crates/vb_doc/src/export.rs`
  与 `import.rs`(vb-symbol-defs 双向)、`crates/vb_app/src/app/symbol_cmds.rs`
  (命令路径)、台账 09-H。
- 取舍:文档体积随实例数线性增长;换来零 JS、九格式导出不受影响、
  实例可独立微调。
- 被否决替代:(a) Web Components(需 JS,摧毁零依赖原则);(b) 导出时
  展开 `<use>` 引用(产物不可读、不可 diff)。
