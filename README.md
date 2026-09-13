# Vellum Bench · 绘台

> 用 Adobe Illustrator 的操作心智,编辑 100% 标准 HTML/CSS 文档;GPU(Vulkan) 原生渲染;Agent 可直接读写;可导出 PNG / PDF / GIF / MP4。

**HTML 是文档格式,不是编译产物。** 任何人和任何 Agent 都能继续改输出文件。Vellum Bench 把「Agent 出草稿 → 人类像画矢量图一样精修 → HTML 回到 Agent 继续迭代」变成一个无限循环。

## 状态(以代码为唯一真相)

> 版本里程碑归 [`docs/design/`](docs/design/) 管,不在 README 里声明。
> 本表**只写代码可验证的事实**;每个数值的核对命令见 [13 篇 §四「状态表真相」](docs/design/13-ADR与风险登记.md)。

### 已落地 ✅

| 模块 | 状态 |
|---|---|
| 工程骨架(12 crate workspace,依赖方向受控) | ✅ |
| `vb_doc` 场景图 + 命令模式 Undo/Redo(sid 寻址,14 个 Command 变体) | ✅ |
| `vb_css` L1 白名单属性表 + 值规范化 | ✅ |
| `vb_html` 忠实解析 + canonical 序列化(L0/L1 往返幂等) | ✅ |
| HTML 导入 → 场景图 → 导出(多画板纵向堆叠) | ✅ |
| **输入上下文栈 + 快捷键注册表**(单一真相:菜单键位文本 / 派发 / 冲突自检同源) | ✅ |
| `vellum-cli`:**10 个子命令** + `patch` 的 **16 种 op**(事务 + `base_rev` 乐观锁) | ✅ |
| **vellum-mcp**:MCP stdio Server(**10 工具**,JSON-RPC 2.0) | ✅ |
| 原生导出:**PNG @1x-4x + SVG 矢量(真实文本)**;WPI 桥:PDF/GIF/MP4(系统 Edge/Chrome) | ✅ |
| GUI:Vulkan(wgpu 29)+ Vello 画布 | ✅ |
| 选择/矩形/椭圆/抓手、**Alt 复制、Shift 约束、框选(相交即选)** | ✅ |
| **8 手柄缩放(Shift 等比/Alt 中心)+ 角外圈旋转** | ✅ |
| **智能参考线**(边/中心对齐兄弟与画板,品红) | ✅ |
| 双击文本编辑、画板管理(新建/删除/改名)、层序(`Mod+[` / `Mod+]`) | ✅ |
| 设计令牌面板(CSS 变量改一处全站生效)、语义标签、链接/aria、flex 布局 | ✅ |
| **文件监听热重载**(Agent 改 HTML → 画布更新,基于 `notify`) | ✅ |
| **往返语料库 20 例**(L0 无损坏 / L1 字节幂等) | ✅ |
| **剪贴板 / 对齐六键 / 命令面板 Ctrl+K / 数值浮层 / Mod 禁用吸附**(P3 批次,注册表扩至 49 命令) | ✅ |
| **质量门禁 5 项**(`pwsh tools/ci.ps1` 一键:格式 / clippy `-D warnings` / 全量测试 / Agent 无头自检 / 硬编码颜色棘轮) | ✅ |
| **2026-09 全库审查修复**:命令层(artboards 派生同步/Move 环防护/Compound 原子回滚/Group 坐标重定基)、Agent 层(duplicate 深拷贝/对齐跨画板分组/MCP -32700)、导入层(CSS 字符串感知扫描/级联顺序/自定义属性大小写)、GUI 层(多画板世界坐标统一/Esc 取消还原/Alt 复制入 undo 栈),21 个回归测试锁定 | ✅ |

### 未落地 ⏳ —— 别按「已完成」读

| 缺口 | 现状(代码实测) | 计划 |
|---|---|---|
| **工具集** | **8 / 13** —— Select/DirectSelect(锚点拖拽已接线,走 SetVector 入 undo)/Rect/Ellipse/Line/**Pen**/Zoom/Hand;剪刀、路径查找器在 P4 后半 | P4 后半 |
| **快捷键** | **注册表 61 命令 / 52 键位绑定**(`commands.yaml` 36 条已同步);02 篇目标 ~120 | P3 剩余批次 |
| **剪贴板** | ✅ 已实现 —— Ctrl+C/X/V + 就地粘贴 Ctrl+F(内部剪贴板,事务粘贴) | 完成 |
| **对齐** | ✅ 已实现 —— 6 快捷键 + 属性面板按钮组(单选对齐画板/多选对齐集合) | 完成 |
| **命令面板** | ✅ Ctrl+K,搜索 + 键位展示,直达派发 | 完成 |
| **吸附禁用** | ✅ 拖动中按 Mod 临时关闭智能参考线(02 篇 §4.1) | 完成 |
| **主题** | 深/浅双令牌 + 切换(`view.toggle_theme`); vb_ui 已落 theme/fonts/icons/components/cursor | P2 收尾 |
| `vb_layout` / `vb_platform` | **空壳 crate** | P4 / P5 |
| `i18n/` `assets/fonts/` | 仍为空(字体走系统 fallback 链) | P5 |
| Parley 中文文本管线 | 未接入依赖(画布文本为 egui 近似,见 [ADR-0017](docs/adr/0017-canvas-text-approximation-v01.md)) | 待定 |
| 路径布尔 / 剪刀 | 见 [ADR-0012](docs/adr/0012-path-boolean-pending-spike.md)(待 Spike 结论);钢笔直线段版已落地(平滑手柄待补) | P4 后半 |
| 响应式断点 / 伪类编辑 | 未开始 | P3 后半 |
| **门禁 4/5/6/7** | 渲染快照(`vello_cpu`)、性能基准(B1–B6)、i18n 双语扫描、输出校验(W3C + prettier) **均未落地**(14 篇 §7.1 共列 9 项,`ci.ps1` 已自动化的只有 5 项) | P5 |
| 组件 / 时间轴 / CRDT | 未开始(1→100) | 不承诺档期 |

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
vellum-cli --doc examples/landing/index.html tree --json
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
