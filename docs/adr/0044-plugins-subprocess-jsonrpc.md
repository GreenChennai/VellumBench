# ADR-0044: 插件 = 子进程 + stdio JSON-RPC + manifest 权限,不引 WASM 运行时

- 状态:已接受(阶段 5-10 / 05-10;兑付迭代附录 ADR-VB-L12,主文档 W12)
- 背景:插件化需要 ABI 与权限模型;WASM 运行时(wasmtime/wasmer)与
  「最小依赖」纪律冲突,内置脚本引擎则引入第二语言栈。
- 决策:
  1. 插件为**外部子进程**,经 stdio 换行分隔 **JSON-RPC 2.0** 与宿主通信
     —— 与 `vellum-mcp`(vb_agent)同构,任意语言可写;
  2. `plugin.json` manifest 严格 schema(未知字段拒绝)声明命令权限;
     默认零权限 + 首次启用授权弹窗 + `plugins.json` 持久化;越权拒绝并记录;
  3. 面板 UI 为**只读投影 + 受控元件**(按钮 = 回发通知),不给插件直接
     改内存文档的通道;文档访问走只读投影,导出器只落用户选的目录;
  4. 崩溃隔离:独立进程 + 状态机(Stopped/Starting/Running/Crashed/
     Unauthorized)+ 超时杀 + DROP 兜底 + 日志环。
- 落地:`crates/vb_plugin/`(protocol / process / host / manifest / auth /
  projection)、`crates/vb_app/src/app/plugins.rs`(GUI 会话态)、
  `plugins/example-stats`(仓库自带示例插件)、台账 09-J。
- 取舍:插件不能零拷贝访问内存,面板交互受限(受控元件);换来零新
  依赖、天然进程隔离、任意语言可写。
- 被否决替代:(a) WASM 插件运行时(重依赖);(b) 内置脚本引擎
  (第二语言栈 + 沙箱自研风险)。
