# Vellum Bench 设计指导文档集

> **一句话定位**：用 Adobe Illustrator 的操作心智，编辑 100% 标准 HTML/CSS 文档；Vulkan 原生渲染；Agent 可直接读写；可导出为图片 / PDF / GIF / MP4。

| 项 | 内容 |
|---|---|
| 项目代号 | **Vellum Bench**（中文名：绘台） |
| 建议仓库 | `E:\平日资料\GitHub\VellumBench` |
| 目标平台 | Windows 优先（AMD RX 9070 GRE / Vulkan 1.3），后续 macOS(Metal) / Linux(Vulkan) |
| 主语言 | Rust（渲染 / 文档 / 工具），Python 仅作导出旁路（复用 WPI） |
| 渲染栈 | wgpu → **Vulkan**（主）+ Vello（GPU 2D 路径）+ Parley/Swash（文本）+ Taffy（布局） |
| UI 栈 | egui（与画布共享 wgpu device，单窗口单 GPU 上下文） |
| 导出旁路 | WPI（Playwright + Chromium + FFmpeg），进程/常驻服务集成 |
| 文档性质 | 设计指导（Design Guide），非 API 手册；用于指导 0→1 与 1→100 的实现与验收 |

---

## 文档地图与阅读顺序

| # | 文档 | 回答什么问题 | 必读 |
|---|---|---|---|
| 00 | [需求拷问与产品定义](00-需求拷问与产品定义.md) | 这东西到底为什么存在？做什么、不做什么？ | ⭐ 先读 |
| 01 | [核心概念与 AI 术语对照](01-核心概念与AI术语对照.md) | Illustrator 的每个概念在 HTML 里对应什么？ | ⭐ 灵魂 |
| 02 | [交互与快捷键规范](02-交互与快捷键规范.md) | 键盘鼠标按下去该发生什么？ | ⭐ 灵魂 |
| 03 | [界面布局与面板规范](03-界面布局与面板规范.md) | 界面长什么样、每个面板有哪些字段？ | ⭐ |
| 04 | [文档模型与 HTML 序列化](04-文档模型与HTML序列化.md) | 内存里怎么存、落盘成什么 HTML？ | ⭐ 灵魂 |
| 05 | [渲染架构与 Vulkan 管线](05-渲染架构与Vulkan管线.md) | 怎么画、怎么快、怎么不卡？ | ⭐ |
| 06 | [工具集与编辑能力](06-工具集与编辑能力.md) | 每个工具的行为规格是什么？ | |
| 07 | [导出管线与 WPI 集成](07-导出管线与WPI集成.md) | 图片/PDF/GIF/MP4 怎么出？WPI 怎么合？ | ⭐ |
| 08 | [Agent 协作协议](08-Agent协作协议.md) | Agent 怎么读写文档？ | ⭐ 差异化 |
| 09 | [工程架构与目录结构](09-工程架构与目录结构.md) | crate 怎么切、状态怎么管、怎么存？ | |
| 10 | [性能预算与质量门禁](10-性能预算与质量门禁.md) | 多快算达标？什么情况不许合入？ | |
| 11 | [路线图 0→1](11-路线图-0到1.md) | 从空白到 v1.0 每一步做什么、怎么验收 | ⭐ 开工用 |
| 12 | [路线图 1→100](12-路线图-1到100.md) | v1 之后往哪走 | |
| 13 | [ADR 与风险登记](13-ADR与风险登记.md) | 为什么这么选？最大的坑在哪？ | ⭐ 决策依据 |
| 14 | [下一阶段迭代方案 — UI 重制与 AI 心智对齐](14-下一阶段迭代方案-UI重制与AI心智对齐.md) | P1–P5 批次怎么排？UI 走什么战略？ | ⭐ 排期 |
| 15 | [下一批迭代计划 — 清算与承诺收口](15-下一批迭代计划-清算与承诺收口.md) | 2026-09-13 全库审查的 Bug 清单与批次 A→C 方案 | ⭐ 当前执行 |

**三种读法**：
- 你要开工 → 00 → 04 → 05 → 11 → 09
- 你要做交互/界面 → 01 → 02 → 03 → 06
- 你要做 Agent/导出 → 04 → 08 → 07

---

## 三条不可妥协的原则

1. **HTML 是文档格式，不是编译产物。** 输出必须干净、可读、可 diff — 任何人和任何 Agent 都能继续改。绝不生成 `<div style="position:absolute;left:137px;top:502px">` 这种垃圾堆（除非用户显式要求"绝对定位导出模式"）。
2. **Illustrator 心智不可妥协。** 术语、快捷键、鼠标手势必须让 AI 老用户零学习成本迁移。这是本产品对抗 Figma / Webflow / 在线编辑器的唯一护城河。
3. **Agent 是一等公民。** 任何用户能做的操作，Agent 都能通过协议做到；任何元素都必须稳定可寻址。

---

## 核心架构一图流

```
          ┌─────────────── 编辑态（Vulkan 主渲染，60/120fps）───────────────┐
  输入 ──▶ │ Command ──▶ Document(场景图 Arena) ──▶ Layout(Taffy) ──▶ Vello Scene │
 (AI 式)   │                    │                                        │        │
           │                    └── HTML/CSS 序列化 ──▶ index.html        ▼        │
           │                                                    wgpu RenderPass     │
           │                                                          +             │
           │                                                    egui UI Overlay     │
          └────────────────────────────────────────────────────────────────────────┘
                       │ 保存/导出                        │ 浏览器真值校对
                       ▼                                  ▼
        index.html + styles/ + assets/        WPI(Playwright+Chromium) ──▶ PNG/PDF/GIF/MP4
```

**双引擎是刻意设计，不是妥协**：Vulkan 自绘负责"编辑时的手速"，Chromium 旁路负责"导出时的像素真值"。二者靠同一份 HTML 对齐，差异可被"校对视图"显式暴露，而不是偷偷不一致。

---

## 现状盘点（已确认可用）

| 资产 | 路径 / 版本 | 在本项目中的角色 |
|---|---|---|
| Rust 工具链 | 1.97.1 `x86_64-pc-windows-msvc`（可链接） | 主开发语言 |
| Vulkan Loader | `C:\Windows\System32\vulkan-1.dll` 存在 | wgpu Vulkan 后端可用 |
| WPI | `E:\平日资料\GitHub\WPI`（PySide6 + Playwright + FFmpeg，已到 v2.6） | 导出旁路（PNG/GIF/MP4/PDF） |
| FFmpeg | `E:\平日资料\GitHub\MomentShift\tools\ffmpeg_bin\` | 原生导出管线的 GIF/MP4 编码 |
| GPU | AMD RX 9070 GRE（RDNA4） | Vulkan 目标卡；注意 tile/子组相关驱动坑 |

> WPI CLI 契约（已核实）：
> `WPI-noGUI-cli.exe --source <dir|html|url> --output <file> --format PNG|GIF|MP4|PDF --width N --scale 1|2|4|8 --height 0 --fps N --loop N --max-wait S --transparent --no-full-page`
