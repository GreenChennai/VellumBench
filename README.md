# Vellum Bench · 绘台

> 用 Adobe Illustrator 的操作心智,编辑 100% 标准 HTML/CSS 文档;GPU(Vulkan) 原生渲染;Agent 可直接读写;可导出 PNG / PDF / GIF / MP4。

**HTML 是文档格式,不是编译产物。** 任何人和任何 Agent 都能继续改输出文件。Vellum Bench 把「Agent 出草稿 → 人类像画矢量图一样精修 → HTML 回到 Agent 继续迭代」变成一个无限循环。

## 状态(v0.1.0 骨架版)

按 [路线图 0→1](docs/design/11-路线图-0到1.md) 推进,当前完成:

| 模块 | 状态 |
|---|---|
| 工程骨架(12 crate workspace,依赖方向受控) | ✅ |
| `vb_doc` 场景图 + 命令模式 Undo/Redo | ✅ |
| `vb_css` L1 白名单属性表 + 值规范化 | ✅ |
| `vb_html` 忠实解析 + canonical 序列化(往返) | ✅ |
| HTML 导入 → 场景图 → 导出(幂等) | ✅ |
| `vellum-cli` Agent CLI(tree/find/get/patch/export) | ✅ |
| CPU 原生 PNG 导出(离屏,无 GPU 依赖) | ✅ |
| `vb_app` GUI:Vulkan(wgpu)+Vello 画布 + egui 面板 | ✅ |
| 选择/矩形工具、Alt 复制、Shift 约束、再次变换 | ✅ |
| Parley 中文文本渲染(v0.2) | ⏳ |
| 钢笔/渐变批注者/智能参考线(v0.3-0.4) | ⏳ |
| WPI 浏览器真值导出(v0.5+) | ⏳ |
| MCP Server(v0.6) | ⏳ |

## 构建

```bash
cargo build --release
# GUI(Vulkan/wgpu,无独显自动回退)
target/release/vellumbench.exe
# Agent CLI(无头,可进 CI)
target/release/vellum-cli.exe --doc examples/landing/index.html tree --json
```

## 快速体验(Agent 闭环)

```bash
vellum-cli --doc examples/landing/index.html outline --json
vellum-cli --doc examples/landing/index.html find --name "主标题" --json
vellum-cli --doc examples/landing/index.html patch ops.json   # set_text / set_style / move ...
vellum-cli --doc examples/landing/index.html export --artboard hero --format png --scale 2 --out hero@2x.png
```

Agent 改 HTML → 用户在 GUI 里接着画;用户保存 → Agent 读增量。**双向通畅,永不锁定**。

## 设计文档(灵魂,先读)

| 文档 | 内容 |
|---|---|
| [docs/design/00](docs/design/00-需求拷问与产品定义.md) | 为什么存在、做什么、不做什么 |
| [docs/design/01](docs/design/01-核心概念与AI术语对照.md) | Illustrator 概念 ↔ HTML 对照(灵魂) |
| [docs/design/04](docs/design/04-文档模型与HTML序列化.md) | 场景图与序列化(灵魂) |
| [CONTEXT.md](CONTEXT.md) | 术语表(单一真相) |
| [docs/adr/](docs/adr/) | 架构决策记录 |

## 三条不可妥协的原则

1. **HTML 是文档格式,不是编译产物** — 输出干净、可读、可 diff。
2. **Illustrator 心智不可妥协** — 快捷键、修饰键、术语零学习成本。
3. **Agent 是一等公民** — 用户能做的 Agent 都能做;元素稳定可寻址(`data-vb-id`)。

## License

MIT
