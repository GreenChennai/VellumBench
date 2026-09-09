# 08 · Agent 协作协议

> 原则：**用户能做的，Agent 都能做；Agent 能做的，用户都能做。**
> 不接任何外部大模型 API —— Agent 是外部驱动者（WorkBuddy / Claude / 自写脚本），本软件只提供**确定性工具接口**。

---

## 一、三种接入方式

| 方式 | 适用 | 版本 |
|---|---|---|
| **文件同步**（监听 + 热重载） | 最简单：Agent 直接写 `index.html`，软件自动重载 | v0.1 起 |
| **CLI** | 脚本化、批量、CI | v0.6 |
| **MCP Server** | 对话式 Agent（WorkBuddy / Claude Desktop） | v0.6 |

三者共用同一套 **命令 ID**（`commands.yaml`），与快捷键、菜单、Undo 栈完全同源 —— 这是 09 篇命令模式带来的红利。

---

## 二、方式一：文件同步（最简，先做）

```
Vellum Bench 打开 MyProject/index.html
  → 启动文件监听（notify crate，去抖 300ms）
  → 磁盘变更 → 三向合并（base/ours/theirs，见 04 篇 6.3）
  → 画布增量更新（保留视口、选区、撤销栈）
```

- 设置项：`自动重载外部修改` = 询问 / 自动采用 / 忽略（Agent 场景推荐「自动采用」）
- 冲突时按 04 篇规则处理；Agent 场景下建议设为「自动采用外部修改」
- **保存策略**：用户在 Vellum Bench 内 `Mod+S` → 回写 HTML；Agent 写文件 → 软件重载。双向通畅。

> 这一条就足以支撑「Agent 写网页 → 用户改」的闭环，v0.1 即可用，性价比最高。

---

## 三、方式二：CLI

```bash
vellum-cli --doc ./MyProject/index.html <command> [args] [--json]
```

| 命令 | 示例 |
|---|---|
| `open` / `save` / `close` | `vellum-cli --doc ./i.html save` |
| `tree` | `vellum-cli tree --depth 3 --json` |
| `find` | `vellum-cli find --name "主标题" --json` |
| `get` | `vellum-cli get a7f3c2 --style --json` |
| `patch` | `vellum-cli patch ops.json` |
| `export` | `vellum-cli export --artboard hero --format png --scale 2 --out hero.png` |
| `shot` | `vellum-cli shot --artboard hero --out preview.png` |
| `check` | `vellum-cli check --engine browser --report diff.json` |

**所有输出默认 `--json`**（Agent 友好）；无 `--json` 时输出人类可读表格。
**退出码**：0 成功 / 1 参数错误 / 2 文档未打开 / 3 patch 冲突 / 4 导出失败。

---

## 四、方式三：MCP Server

```
vellum-mcp   # stdio MCP，暴露以下 tools
```

| Tool | 说明 |
|---|---|
| `vellum_open_document(path)` | 打开/切换文档 |
| `vellum_get_outline(depth?, artboard?)` | 树形摘要（**轻量，默认给这个**） |
| `vellum_find(query)` | 按 name / tag / class / 文本 / 位置 查找 |
| `vellum_get_element(id, include?)` | 取元素详情（样式/文本/几何/HTML） |
| `vellum_get_html(scope?)` | 取 HTML（`--scope artboard:hero` / `subtree:<id>`） |
| `vellum_apply_patch(ops, base_rev?)` | 应用变更（事务） |
| `vellum_export(target, format, scale, engine, out)` | 导出 |
| `vellum_screenshot(artboard?, out)` | 截图（供多模态模型"看"页面）⭐ |
| `vellum_diff_since(rev)` | 自某版本以来的变更摘要 |
| `vellum_check(engine)` | 原生 vs 浏览器一致性校对 |

> `vellum_screenshot` 是杀手锏：**让 Agent 能"看见"它做的页面**，形成"生成→看图→修改"的闭环。

---

## 五、Patch 操作集

```jsonc
{
  "base_rev": 128,                 // 乐观锁；省略则不强校验
  "ops": [
    { "op": "insert",
      "parent": "a7f3c2", "index": 2,
      "node": { "tag": "h2", "name": "副标题",
                "text": "一杯好咖啡的诞生",
                "style": { "font-size": "32px", "color": "#fff" },
                "box": { "x": 120, "y": 300, "w": 600, "h": 48 } } },

    { "op": "set_text",   "id": "f5c4e6", "text": "香醇，从一颗豆开始" },
    { "op": "set_style",  "id": "a6d5f7",
      "css": { "background-color": "#00A870", "border-radius": "10px" },
      "state": "hover" },                       // 支持状态：default/hover/active/disabled
    { "op": "set_attr",   "id": "a6d5f7", "attrs": { "href": "/buy", "aria-label": "立即购买" } },
    { "op": "move",       "id": "f5c4e6", "parent": "d3a2c4", "index": 0 },
    { "op": "set_box",    "id": "f5c4e6", "box": { "x": 100, "y": 200, "w": 720, "h": 80 } },
    { "op": "rename",     "id": "f5c4e6", "name": "Hero 主标题" },
    { "op": "duplicate",  "id": "a6d5f7", "offset": { "x": 0, "y": 64 } },
    { "op": "delete",     "id": "e4b3d5" },
    { "op": "group",      "ids": ["f5c4e6","a6d5f7"], "name": "CTA 组" },
    { "op": "ungroup",    "id": "d3a2c4" },
    { "op": "align",      "ids": ["f5c4e6","a6d5f7"], "mode": "hcenter", "to": "key:artboard" },
    { "op": "order",      "id": "f5c4e6", "to": "front" },
    { "op": "set_token",  "name": "brand-1", "value": "#00A870" },
    { "op": "new_artboard","name": "Pricing", "w": 1440, "h": 900, "after": "b1e0aa" },
    { "op": "export",     "artboard": "b1e0aa", "format": "png", "scale": 2,
                          "engine": "native", "out": "out/hero@2x.png" },
    { "op": "run",        "command": "object.group" }   // 兜底：任何命令 ID 都可执行
  ]
}
```

**语义**：
- 事务：全部成功或全部回滚（失败返回已执行到第几条 + 原因）
- `run` 兜底保证命令表 100% 覆盖（新功能无需改协议）
- 每次成功 patch 返回 `{rev, created_ids[], changed_ids[], warnings[]}`
- 几何单位统一 px；坐标相对**所属画板**

---

## 六、寻址策略（Agent 如何找到元素）

优先级：

1. **`data-vb-id` 稳定短码**（首选）—— 全生命周期不变
2. **`data-vb-name`**（图层名）—— 人类/Agent 都可读
3. **CSS 选择器** —— 兜底；多重匹配时返回列表，要求 Agent 消歧
4. **语义查询** —— `find { text_contains: "立即购买" }` / `{ near: "artboard hero 左上角" }`

**Agent 友好约定**（软件侧保证）：
- 每个元素必带 `data-vb-id`（可关）
- 图层名同步写入 `data-vb-name`
- 画板起点写注释锚点 `<!-- vs:artboard hero -->`（可选输出）
- 输出 HTML 属性顺序固定 → patch 后的 diff 最小

---

## 七、上下文经济（重要）

直接把整页 HTML 丢给模型很贵。提供三档视图：

| 视图 | 内容 | 用途 |
|---|---|---|
| `outline` | 树形：`id / name / tag / 类型 / bbox`，不含样式 | **默认**，让 Agent 先建立结构认知 |
| `subtree(id)` | 某子树完整 HTML | 局部精改 |
| `diff_since(rev)` | 自某版本变更摘要 | 多轮迭代时只传增量 |
| `full` | 完整 HTML | 仅在必要时 |

推荐 Agent 流程：
```
outline → 定位 id → get_element(id) → patch → screenshot → （看图）→ 再 patch
```

---

## 八、典型工作流（产品灵魂场景）

```text
① 用户：帮我做一个咖啡品牌落地页
   Agent：写 index.html + styles/main.css（或直接用 Vellum Bench 新建文档后 patch）

② Agent：vellum_open_document("./Landing")
         vellum_screenshot(artboard=hero)      ← 自己看一眼
         发现标题压在图片上 → 调整 y

③ 用户：打开 Vellum Bench，像在 AI 里一样：
         把标题往左挪 20px、改渐变、给按钮加圆角
         Mod+S 保存

④ Agent：vellum_diff_since(rev=128)           ← 只拿增量
         vellum_apply_patch([把 CTA 文案改为「限时 8 折」])
         vellum_export(artboard=hero, scale=2, engine=browser)

⑤ 用户：继续在画布上微调…… 循环
```

**价值**：Agent 不用"猜"用户改了什么，用户不用"求"Agent 改细节。

---

## 九、并发、权限与安全

| 项 | 规则 |
|---|---|
| 乐观锁 | patch 带 `base_rev`，不匹配返回 `409 conflict` + 当前 rev（Agent 可重新 outline 后重试） |
| 写入范围 | 仅允许写：当前文档目录内 + 显式导出目录；**拒绝绝对路径越界**（如 `C:\Windows`） |
| 资源 | 置入图片必须是文档目录内的相对引用；外部 URL 需用户确认（或设置白名单域名） |
| 危险操作 | `delete` / `export`（覆盖）/ 关闭文档 → 默认需 `--force` 或 dry-run 确认 |
| dry-run | `patch --dry-run` 返回将要发生的变更，不改文档 |
| 速率 | CLI/MCP 默认串行；批量操作建议合并为一个 patch |
| 日志 | `~/.vellum/agent.log` 记录每次调用（可关），便于回溯 Agent 干了什么 |

---

## 十、模块划分

```
crates/vb_agent/
├── watcher/    文件监听 + 去抖 + 三向合并触发
├── cli/        clap 命令实现（与 commands.yaml 同源）
├── mcp/        rmcp（Rust MCP SDK）server，stdio 传输
├── patch/      ops 解析、校验、事务、冲突
└── views/      outline / subtree / diff 视图生成
```

所有入口最终调用 `Document::apply(Command)` —— **与 UI 完全同一条路径**，天然保证：可撤销、可重放、一致性。

---

## 十一、验收点

- [ ] 文件同步：外部改 HTML → 3s 内画布更新，撤销栈不丢
- [ ] CLI 完成「打开 → find → patch → export」全流程 < 3s
- [ ] patch 事务：中途失败完整回滚，`rev` 不变
- [ ] 乐观锁：`base_rev` 过期返回 409 且文档未被部分修改
- [ ] MCP server 可被 WorkBuddy / Claude Desktop 直接加载（stdio stdio 协议合规）
- [ ] `screenshot` 输出与画布视觉一致（像素比对通过）
- [ ] 越界写入被拒绝（写 `C:\Windows\evil.html` 失败且有日志）
- [ ] 100% 命令 ID 可通过 `run` 兜底执行（脚本比对 `commands.yaml`）
