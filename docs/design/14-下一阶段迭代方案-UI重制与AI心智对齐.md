# 14 · 下一阶段迭代方案 — UI 重制 × AI 心智对齐

> 本篇是 `/grill-me` 的产物：先把问题往死里问一遍，再谈做什么。
> **前 8 条约束来自 00 篇，被本篇第 2 章修订 1 条、新增 1 条，其余不动。**
> 阅读顺序建议：先看 TL;DR，再看第 0 章（问答），需要动手时看第 3/4 章（规格），排期看第 6 章。

---

## TL;DR

| 判断 | 结论 |
|---|---|
| **真实进度** | 比 README 声称的低得多：GUI 只有 **4 个工具 / 24 个快捷键 / 1 套写死的深色主题**；`tests/`、`i18n/`、`assets/` 三个目录是空的；`vb_ui`、`vb_layout`、`vb_platform` 是空壳 crate |
| **真实资产** | 比想象的高：**命令层与 Agent 层是扎实的**（14 个 Command 变体、patch 已含 6 种对齐运算、CLI 8 个子命令、MCP stdio 可跑）。能力是「倒挂」的 |
| **本轮核心动作** | **不造新能力，只做「接线 + 重塑」** —— 把命令层已有的能力接进 GUI，把 GUI 的皮换掉 |
| **UI 战略** | **Figma 的皮，AI 的骨**。视觉语言（色彩/字体/间距/圆角/动效）学 Figma / 即时设计；交互语义（修饰键/术语/手柄/参考线）只认 Illustrator。两层之间划一条不可逾越的缝 |
| **框架裁定** | **留在 egui，不自研 UI 框架**。Graphite 在 2020 年自研 GUI 框架，结果是「halting progress on higher-priority features」——这是花钱买来的教训 |
| **UI 值不值得先做** | 值得，但**不是先做**。UI 重制与 AI 对齐并行推进，用**垂直切片**交付：每个阶段结束必须有一条「用户能完整跑通的任务流」 |
| **最优先修的 3 个 bug** | ① 文本编辑态下按 `V`/`Delete` 会切工具/删对象（02 篇明令禁止）② 视图菜单 4 个加速键是假的（显示但不生效）③ `Ctrl+Y` 绑成了重做，与 02 篇的「轮廓模式」冲突 |
| **总排期** | 5 个阶段，**P1–P5**。P1 只做「地基 + 视觉地基」，2 周内必须能看见变化 |

---

## 0 · 自我拷问（grill-me）：先回答 10 个硬问题

### Q1 · 软件现在真实到哪一步？

**答**：见第 1 章的代码级审计。一句话：**命令层像 v0.6，GUI 层像 v0.2。**

这不是"进度慢"，而是"两条腿不一样长"。Agent 已经能通过 CLI 做对齐、批量、导出，但用户在界面上连一个对齐按钮都找不到。这种状态下最忌讳的动作是**继续往命令层加东西**——加了也看不见。

### Q2 · 文档说"v0.7 网页能力 / v1.4 CSV 批量 / v1.0 打包已完成"，代码说不是，怎么办？

**答**：以代码为唯一真相，并且**立刻停止在 commit message 和 README 里写没落地的东西**。

现状证据：8 个 commit 全部发生在同一天（2026-09-10），最后一个写着 `feat(app,agent,tools): v0.7 web-capability panels + v1.4 CSV batch + v1.0 packaging`。实际检查：

- "v0.7 web-capability panels" → 断点 **0 处**、`@media` **0 处**、伪类 **0 处**、自动布局只有 1 个 `collapsing` 折叠块
- "v1.4 CSV batch" → GUI 无关（这是 CLI 的 `Batch` 子命令，确实存在，写在 `feat(app,...)` 里属于归属错误）
- "v1.0 packaging" → `tools/build.ps1` 存在且写得不差，但 10 篇要求的 **7 项门禁脚本一个都没有**，`tests/` 为空

**处置**：本篇第 7 章确立「提交信息纪律」。**虚标的版本号会污染后续所有判断**——当你说不清自己站在哪，就没法决定下一步。

### Q3 · "Figma 的皮 + AI 的骨"会不会精神分裂？

**答**：不会，**前提是先把三层切开、写清楚哪层归谁**。绝大多数"国产替代"失败的原因就是把三层搅在一起：学了 Figma 的颜色，顺手把 Figma 的框选语义也抄了，AI 用户一试就废。

第 2 章给出三层模型 + 12 条可拿 + 9 条禁拿 + 一张冲突裁决表。**只要裁决表是硬的，就不会分裂。**

### Q4 · 功能都没齐，现在做 UI 是不是本末倒置？

**答**：不是，因为**它俩根本不是两件事**。

"让 AI 用户一用就上手"里，**视觉可信度是入场券**：一个用 SimHei 黑体、📁 emoji 当图层图标、圆角为 0 的界面，AI 用户第一眼就判定"这是个小玩具"，然后不会给第二次机会去验证你的快捷键对不对。这是心理事实，不是审美偏好。

反过来，如果只做皮不补交互，用户会在"看起来专业"的界面里发现 `Ctrl+Shift+L` 没反应，信任崩得更彻底。

**所以：并行推进，但顺序有讲究——**
1. 先修 3 个"界面说谎"的 bug（菜单假加速键、快捷键串味、超范围提示缺失）
2. 再做视觉地基（令牌 + 字体 + 图标 + 组件层）
3. 再按命令表逐条把 AI 交互接上

### Q5 · egui 能不能做出 Figma 质感？要不要换框架？

**答**：**能做出"克制的现代深色工具 UI"，做不出 Figma 那种"有呼吸感"的精细。** 这是实话——egui 是 immediate-mode，没有真正的合成层，做不了多层阴影+背景模糊的玻璃质感。

**但不需要。** 目标不是"像 Figma 一样漂亮"，是"像一个被认真对待的专业工具"。参照坐标是 **Rerun viewer**（egui 生态里公认品质最好的产品级应用），它证明 egui 的上限足够高。

框架候选与裁定：

| 方案 | 成本 | 结论 |
|---|---|---|
| **A. 留在 egui + 自研令牌与组件层** | 低（1–2 周出效果） | ✅ **选它** |
| B. egui + 自研 widget 层，只保留 egui 的输入/布局 | 中 | 作为 A 的自然演进（先 A，遇到 egui 组件不够用再逐块替换） |
| C. 换 Xilem / Masonry（Rust 官方） | 高 | ❌ 不成熟，且会失去 egui 生态（egui_dock / iconflow 全部作废） |
| D. 换 GPUI（Zed） | 高 | ❌ 无稳定发布，生态小，文档少 |
| E. 自研 UI 框架 | 极高 | ❌ **Graphite 的教训**：自研 GUI 框架吃掉了整个 2020 年，拖垮了高优先级功能 |

### Q6 · "AI 用户一用就上手"怎么算达标？

**答**：**必须可测，否则就是口号。** 00 篇已经写了"AI 老用户不看文档完成 5 项基础任务成功率 ≥ 80%"，但没写怎么测。第 4.6 章给出可执行协议：

- 招募 **3 名**用 AI ≥ 2 年的人（社区/Discord 最容易找）
- 5 项任务，**不给任何文档**，屏幕录制 + 计时 + 记录卡壳点
- 通过线：**成功率 ≥ 80%（12/15）且无一项任务全员失败**
- 每项任务记录：首次点击位置、卡壳时长、放弃点 → 这些数据直接变成下一轮的 bug list

### Q7 · 一个人 + Agent，时间预算怎么排？

**答**：诚实版：**P1–P5 全走完约 4–6 个月（业余 10–15h/周）**。但这个数字不重要，重要的是**每个阶段都必须能演示**。

排期原则（沿用 11 篇铁律）：**每版结束都是一个能跑、能演示、能回滚的软件。** 本篇进一步收紧：**每版结束必须多出一条完整任务流**（不是多出若干个功能点）。

### Q8 · 开源项目怎么参考才不踩坑？

**答**：两条坑，都得防。

- **坑 1：许可污染。** 最想抄的东西恰好是 GPL 的。典型：**Inkscape 的 `adobe-illustrator-cs2.xml` / 新版 Illustrator CC 2024 键位文件是 GPL-2.0-or-later**，本项目是 MIT。**可以拿它当"对照答案"来核对我们的键位表，不可以把文件内容粘贴进仓库。** 事实（"AI 里 Ctrl+Shift+C 是水平居中对齐"）不受版权保护，抄事实没问题；抄文件有问题。详见第 5 章。
- **坑 2：抄歪。** 抄 Graphite 的节点图架构？本项目不需要。抄 Penpot 的协作？v3 才做。**只抄与本项目空位同构的那一层。**

### Q9 · 最大的三个风险？

**答**：

| # | 风险 | 触发信号 | 对策 |
|---|---|---|---|
| R1 | **视觉重制滑向"无限调色"** | 连续 3 天没提交功能，只在改颜色 | 令牌表先冻结（第 3.2 章），改令牌要走 ADR |
| R2 | **AI 对齐滑向"复刻 AI 全家桶"** | 开始讨论"实时上色怎么做" | 06 篇的"不做清单" + 本篇第 2.3 章，触发即引用 |
| R3 | **门禁缺失导致重构失控** | 改 `vb_doc` 时不敢动 | P1 第一件事：补 `tests/` 与门禁脚本（第 7 章） |

### Q10 · 这一轮的"不做"清单？

**答**：在 00 篇五节的基础上，本轮补充：

- ❌ 不做浮动面板（Figma 试过，回滚了 —— 见 3.1）
- ❌ 不做自研 UI 框架
- ❌ 不做暗色以外的第三套主题（只做深/浅两套 + 令牌化）
- ❌ 不接第三方大模型 API（沿用 00 篇）
- ❌ 不做路径布尔 / 钢笔（P4 才碰，本轮只预留键位）
- ❌ 不做插件 / 协作 / 动效时间轴

---

## 1 · 诚实基线：代码级审计

### 1.1 审计结论

**审计时间**：2026-09-10 · **HEAD**：`a29b4c0` · **规模**：12 crate / 34 个 `.rs` / 10,924 行

| 维度 | 文档声称 | 代码现实 | 差距 |
|---|---|---|---|
| 工具 | P0 需 13 个（06 篇 §二） | **4 个**：`Select` / `Rect` / `Ellipse` / `Hand` | 缺 9 个 |
| 命令表 | ~120 条（11 篇 v0.3） | **19 条**（`commands.yaml`） | 缺 ~100 条 |
| 已绑快捷键 | — | **24 个 key**（实测 `Key::*` 全量枚举） | — |
| 剪贴板 | `Mod+X/C/V` / 就地粘贴 | **0 处**（`clipboard`/`copy`/`paste` 全 0 命中） | **完全缺失** |
| 对齐 | 面板 + 6 快捷键 + 分布 | GUI **无**；但 `patch.rs` 已实现 `align`（6 模式） | **命令层领先 UI 层** |
| 隔离模式 | 双击进入 + 面包屑 | **0 处** | 完全缺失 |
| 轮廓模式 | `Mod+Y` | **0 处**；`Ctrl+Y` 被绑成重做（与 02 篇冲突） | 语义冲突 |
| 视图加速键 | 菜单显示 `Ctrl+0/1/+/-` | **4 个都未绑定**（菜单在说谎） | 真 bug |
| 输入上下文栈 | 02 篇 §一 明令（最高优先级） | **无**：文本编辑时按 `V` 切工具、按 `Delete` 删对象 | 真 bug |
| 智能参考线 | 五类（对齐/间距/角度/尺寸/画板中心） | 有骨架（4 处引用），未验证五类齐全 | 待核 |
| 主题 | 4 档明暗 + 令牌化，禁止字面量 | **只有深色**，`setup_dark_theme` 注释写明"本应用只用深色"；颜色**全是字面量**（`#2a2a2a` 等） | 完全未做 |
| 字体 | 与 AI 一致的字重层次 | 硬编码 `simhei.ttf`/`msyh.ttc`，无 Inter，无字重分层 | 完全未做 |
| 图标 | 统一视觉语言 | **emoji**（📁 ▢ 🖼 ✎ ❄ 🗑 ✋）+ ASCII（▣ ➤ ▭ ◯） | 完全未做 |
| 组件层 | `vb_ui` 独立面板库 | `vb_ui/src/lib.rs` = **4 行注释**；全部 UI 挤在 `vb_app/src/app.rs`（2694 行单文件） | crate 是空壳 |
| 布局 | `vb_layout` Taffy | **4 行注释** | crate 是空壳 |
| 平台层 | `vb_platform` | **1 行注释** | crate 是空壳 |
| 测试 | 100 个往返语料 + 渲染快照 + 基准 | **`tests/` 目录为空** | 完全缺失 |
| i18n | `zh-CN.ftl` / `en.ftl` | **`i18n/` 为空** | 完全缺失 |
| 资源 | `assets/` 放图标/字体/键位表 | **`assets/` 为空** | 完全缺失 |
| 门禁 | 7 项脚本 + 一键 `tools/ci.ps1` | 只有 `tools/build.ps1`（写得不差，含离线冒烟） | 缺 6 项 |
| CI | fmt + clippy + test + 语料 + 快照 + i18n + HTML 校验 | 只有 fmt + clippy + test + `selfcheck` | 缺 4 项 |

### 1.2 关键判断：能力倒挂

命令层与 Agent 层**明显比 UI 层成熟**：

| 层 | 成熟度 | 证据 |
|---|---|---|
| `vb_doc` / `vb_html` / `vb_css` | **较高** | 14 个 `Command` 变体、`Compound` 复合事务、`SetToken` 令牌命令、L0/L1 往返幂等 |
| `vb_agent`（CLI / patch / MCP） | **较高** | 8 个子命令（tree/find/get/patch/export/shot/batch/selfcheck）、patch 含 `insert/set_text/set_style/move/rename/set_tag/delete/group/ungroup/align`、事务 + 乐观锁 + dry-run |
| `vb_render` / `vb_export` | **中** | Vello 画布 + CPU 光栅 + PNG/SVG + WPI 桥 |
| **`vb_app`（GUI）** | **低** | 4 工具、24 快捷键、1 主题、emoji 图标、无组件层、2694 行单文件 |

**结论：这一轮不该写新命令，该把已有命令接出来。**
`SetToken` 命令已经存在，界面上的"设计令牌"面板也已经有雏形 —— 但令牌化 UI 主题（把 `#2a2a2a` 换成 `--vb-bg-panel`）还没做。**同一个能力，在文档层有，在 GUI 层没有。**

### 1.3 三个必须立刻修的真 bug

| # | bug | 证据 | 危害 |
|---|---|---|---|
| **B1** | 文本编辑态下按 `V` 切工具、按 `Delete` 删对象、按 `Backspace` 删对象 | `handle_shortcuts` 直接读 `ctx.input(|i| i.events.clone())`，无上下文守卫；02 篇 §一 要求上下文栈，§八 验收项明写"文本编辑态下按 `V` 不会切换工具" | 用户写文案时误删对象 —— 数据丢失级 |
| **B2** | 视图菜单 4 个加速键是假的（`Ctrl+0/1/+/-`） | 菜单 label 写了，`Key::Num0`/`Key::Num1`/`Key::Plus`/`Key::Minus` 全 0 命中 | 用户按了没反应 = 界面撒谎 |
| **B3** | `Ctrl+Y` 绑成重做，02 篇规定 `Mod+Y` = 轮廓模式 | `app.rs` 中 `(Key::Y, true) => undo.redo(...)` | AI 用户按 Ctrl+Y 期待线框，得到重做 |

**B1 是数据丢失级，必须在任何 UI 工作之前修掉。**

---

## 2 · 战略裁定：Figma 的皮，AI 的骨

### 2.1 三层模型

把产品切成三层，**每层只认一个老师**：

```
┌─────────────────────────────────────────────────────────────┐
│  第 1 层 · 视觉层（Visual）                                  │
│    色彩 / 字体 / 图标 / 圆角 / 阴影 / 间距 / 动效曲线          │
│    老师：Figma UI3 · 即时设计                                 │
│    判定标准：截图放一起，不像个"业余项目"                     │
├─────────────────────────────────────────────────────────────┤
│  第 2 层 · 交互层（Interaction）                             │
│    修饰键 / 手柄 / 吸附 / 参考线 / 光标 / 拖动浮层 / 快捷键    │
│    老师：Adobe Illustrator CC 2024                            │
│    判定标准：AI 用户盲测 ≥ 80%                                │
├─────────────────────────────────────────────────────────────┤
│  第 3 层 · 语义层（Semantics）                                │
│    术语 / 文档模型 / 什么是"画板"/"编组"/"图层"               │
│    老师：HTML/CSS 事实（CONTEXT.md 已定，不动）               │
│    判定标准：界面上说的词，落盘时能找到对应的 HTML            │
└─────────────────────────────────────────────────────────────┘
```

**关键纪律：老师不串门。**
- 第 1 层做视觉时，不许顺手改第 2 层的快捷键
- 第 2 层对 AI 时，不许顺手改成 Figma 的行为
- 第 3 层不动 —— `CONTEXT.md` 是单一真相，本轮不改术语表

### 2.2 可以从 Figma / 即时设计 拿的（12 条）

| # | 拿什么 | 为什么值得拿 | 落到哪 |
|---|---|---|---|
| 1 | **右侧单列面板坞 + 可折叠分组** | AI 的 12 个独立面板太碎，Figma 的"一列到底 + 折叠"信息密度更好 | 第 3.6 章布局 |
| 2 | **面板标签页（Tab）组合** | 「属性 / 图层」用 Tab 切，比左右并排省 280px 宽度 | `egui_dock` |
| 3 | **数值框 scrubby 拖动 + 表达式输入** | 03 篇已要求，Figma 的交互实现更顺（拖动即可见数值变化 + 光标变 col-resize） | 3.5 组件 |
| 4 | **颜色框：色块 + 十六进制并排，点击展开取色器** | 比 AI 的"填充色块弹窗"少一次点击 | 3.5 组件 |
| 5 | **悬停 120ms 背景渐变 + 文字微亮** | egui 目前 hover 只改描边，太生硬 | 3.7 手感 |
| 6 | **选中态用 8% 蓝色底 + 2px 圆角高亮** | 比 AI 的"整行反白"更轻，减少视觉噪音 | 3.2 令牌 |
| 7 | **底部浮动工具条**（Figma UI3 的标志性设计） | 用 `egui::Area` 悬浮在画布底部，把"工具选择"从菜单栏挪走，给顶部腾出菜单空间 | 3.6 布局 |
| 8 | **`Mod+K` 命令面板（Command Palette）** | 这是"最省力的可发现性"——AI 用户不记得快捷键时，打字就能找命令；直接复用 `commands.yaml` | 4.1 |
| 9 | **面板分组标题用 11px / 字重 600 / 二级文字色** | 建立信息层级，成本极低 | 3.2 令牌 |
| 10 | **空状态设计**（无选区时的属性面板） | 当前无选区时属性面板直接消失，用户以为坏了。Figma 会显示"画板属性"或提示 | 3.5 组件 |
| 11 | **图标按钮必须有 tooltip 且含快捷键** | Figma 的 tooltip 写 `选择工具 V` —— 这是"学习模式"的零成本实现 | 3.4 图标 |
| 12 | **浅色主题做真的，不是"深色反相"** | 即时设计的浅色主题是独立调过的（灰阶层次不同） | 3.9 主题 |

### 2.3 绝对不许从 Figma 拿的（9 条）

**这 9 条每一条都会让 AI 用户当场懵掉。列在这里是为了被诱惑时能引用。**

| # | Figma 的行为 | AI 的行为 | 采用 | 原因 |
|---|---|---|---|---|
| **F1** | 框选：**完全包含**才选中 | 框选：**相交即选中** | **AI** | 已实现且正确，别改。AI 用户框选"扫一下"的肌肉记忆依赖这个 |
| **F2** | `Ctrl+D` = Duplicate（原地复制） | `Ctrl+D` = **再次变换**（重复上次位移/旋转/缩放） | **AI** | 这是 AI 灵魂功能。代码已实现（`transform_again`），必须在 UI 上强化提示，避免用户以为是复制 |
| **F3** | Frame 语义：Frame 即画板即容器 | **画板**是导出单元；**编组**是容器；两回事 | **AI + CONTEXT.md** | 术语禁用表已禁"Frame" |
| **F4** | Auto Layout 优先（画框就自动布局） | 默认绝对定位，flex 是可选属性 | **AI** | 网页场景确实需要 flex，但**不能默认开启**，否则坐标语义崩坏 |
| **F5** | 拖角手柄 = 等比；`Shift` 取消等比 | 拖角手柄 = **单轴**；`Shift` 强制等比 | **AI** | 代码 `resize_geom(..., shift, alt)` 已是 AI 语义，保持 |
| **F6** | 单击进入编组内部（双击深入） | **双击**进隔离模式 | **AI** | 单击在 AI 里是"选中整个编组" |
| **F7** | `Shift+0` 缩放 100%，`Shift+1` 适应窗口 | `Mod+1` = 实际大小，`Mod+0` = 适合窗口 | **AI** | 02 篇已定 |
| **F8** | 面板可浮动、可拖到任意位置 | 面板**固定停靠**，只可调宽 + 折叠 | **AI（本轮）** | 见 3.1 —— Figma 自己把浮动面板回滚了 |
| **F9** | 无标尺（Figma 的标尺是后加的且弱） | **标尺 + 参考线**是核心 | **AI** | 对齐精度是矢量编辑的命 |

### 2.4 冲突裁决表（当三层打架时按此判）

| 冲突 | 判决 | 依据 |
|---|---|---|
| "更好看的交互" vs "和 AI 一致" | **和 AI 一致**，另提供开关 | 00 篇 §四-2（肌肉记忆优先于好看） |
| "Figma 的视觉" vs "AI 的布局" | **视觉从 Figma，布局从 AI** | 本章 2.1 |
| "减少点击" vs "术语一致" | **术语一致** | CONTEXT.md 单一真相 |
| "新功能的默认值" vs "AI 的默认值" | **AI 的默认值** | 用户迁移成本最小 |
| "Figma 的极简标签" vs "可读性" | **可读性** | Figma 自己承认：极简标签"hindered accessibility"，最终回滚成完整标签 |

> **对 00 篇的修订**：00 篇 §四-2「肌肉记忆优先于好看」被本篇**细化为"分层适用"**——第 2 层（交互）严守 AI，第 1 层（视觉）允许脱离 AI 去学 Figma。这不是推翻，是消歧。
> **对 03 篇的修订**：03 篇 §一 的布局图（工具箱左列 + 右侧 12 面板坞）改为第 3.6 章的新布局（工具箱左列保留，右侧改为**单列可折叠面板坞 + Tab**，底部增加浮动工具条）。原因：12 个常驻面板在 1440p 下会挤掉 40% 画布。

---

## 3 · UI 重制方案

### 3.1 视觉基调：三条原则 + 四条别人的血泪教训

**三条原则**

1. **克制的现代工具感**：目标是"专业"，不是"炫"。阴影只在浮层用，渐变只在品牌色用，装饰性动效一律不做。
2. **画布是主角，UI 是配角**：面板用低对比灰阶（`#2C2C2E`），画布外区域更暗（`#1C1C1E`），让眼睛自动聚焦画板。**任何"好看的 UI 颜色"如果抢了画板的戏，就是错的。**
3. **可读性 > 简洁**：宁可标签长一点，不要图标孤零零。工具类软件的用户一天看 8 小时，不是看 8 秒。

**四条从 Figma UI3 学到的教训（他们试错换来的，我们直接抄结论）**

| # | Figma 的教训 | 我们的决策 |
|---|---|---|
| L1 | **浮动面板被回滚**。UI3 押注浮动面板，beta 期间发现"cramped the canvas, especially on smaller screens"、"slowed people down"，正式版改回**固定但可调宽** | **面板固定停靠，可调宽，可折叠。不做浮动。** 唯一例外：底部工具条（Figma 保留的浮动设计） |
| L2 | **图标按钮要显示当前值**。混合模式改成纯图标按钮后，"slowed workflows by making users wait for a tooltip" | **任何"有状态"的控件必须显示当前值文本**，不能只给图标 |
| L3 | **极简标签被回滚**。"all labels provide full context… the initial minimalist approach hindered accessibility" | **标签给全**。不玩"X/Y/W/H"以外的缩写 |
| L4 | **布局控件合并，但 X/Y 仍在 W/H 之上**。"early testing showed this inversion disrupted muscle memory too much" | 属性面板：**位置在上，尺寸在下**，布局（flex）折叠在独立分组里 |

### 3.2 设计令牌（完整表）

**令牌命名规范**：`--vb-<域>-<角色>-<变体>`。UI 主题令牌与文档令牌（`--vb-brand-1`）**分两个命名空间，不许混用**。

#### 色彩 · 深色（默认）

| 令牌 | 值 | 用途 | egui 落点 |
|---|---|---|---|
| `--vb-bg-canvas` | `#1C1C1E` | 画板外画布底 | `visuals.extreme_bg_color` |
| `--vb-bg-panel` | `#2C2C2E` | 面板/工具条底 | `visuals.panel_fill` |
| `--vb-bg-raised` | `#3A3A3C` | 弹出层、下拉、Tab 选中 | `visuals.window_fill` |
| `--vb-bg-input` | `#38383A` | 输入框、数值框底 | `visuals.widgets.inactive.weak_bg_fill` |
| `--vb-bg-hover` | `#48484A` | 悬停底 | `visuals.widgets.hovered.weak_bg_fill` |
| `--vb-bg-active` | `#545457` | 按下底 | `visuals.widgets.active.weak_bg_fill` |
| `--vb-border` | `#3F3F42` | 分隔线、控件描边 | `visuals.widgets.inactive.bg_stroke` |
| `--vb-border-strong` | `#545457` | 焦点环、面板边界 | `visuals.widgets.hovered.bg_stroke` |
| `--vb-text` | `#FFFFFF` | 主文字 | `visuals.override_text_color` |
| `--vb-text-2` | `#B8B8BD` | 次文字、标签 | `visuals.weak_text_color` |
| `--vb-text-3` | `#7A7A80` | 占位、"无"、禁用 | `visuals.weak_text_alpha` 派生 |
| `--vb-accent` | `#0D99FF` | 选中、激活、焦点 | `visuals.selection.bg_fill` |
| `--vb-accent-hover` | `#3AAEFF` | accent 悬停 | — |
| `--vb-accent-dim` | `rgba(13,153,255,0.16)` | 选中行底、Tab 选中底 | — |
| `--vb-danger` | `#F24822` | 删除、溢出标记 | `visuals.error_fg_color` |
| `--vb-warn` | `#FFC700` | 冻结块警告、缺字体 | `visuals.warn_fg_color` |
| `--vb-success` | `#14AE5C` | 保存成功、导出完成 | — |
| **`--vb-guide-smart`** | **`#FF00FF`** | 智能参考线 | **不可改**（AI 品红，02/03 篇已定） |
| `--vb-select-box` | `#0D99FF` | 边界框、8 手柄 | — |
| `--vb-hover-box` | `rgba(13,153,255,0.6)` | 悬停对象轮廓 | — |

#### 色彩 · 浅色

| 令牌 | 值 | 说明 |
|---|---|---|
| `--vb-bg-canvas` | `#F5F5F5` | |
| `--vb-bg-panel` | `#FFFFFF` | |
| `--vb-bg-raised` | `#FFFFFF` | + 一层 `window_shadow` |
| `--vb-bg-input` | `#F0F0F0` | |
| `--vb-bg-hover` | `#E8E8E8` | |
| `--vb-bg-active` | `#DEDEDE` | |
| `--vb-border` | `#E6E6E6` | |
| `--vb-border-strong` | `#C9C9C9` | |
| `--vb-text` | `#1E1E1E` | |
| `--vb-text-2` | `#6B6B6B` | |
| `--vb-text-3` | `#9B9B9B` | |
| `--vb-accent` | `#0D99FF` | 两套主题共用品牌色 |
| `--vb-accent-dim` | `rgba(13,153,255,0.12)` | |
| `--vb-guide-smart` | `#E000E0` | 浅底上加深，保证对比度 |

#### 间距 · 圆角 · 描边

| 令牌 | 值 | 用途 |
|---|---|---|
| `--vb-space-1/2/3/4/5/6/8` | `2 / 4 / 6 / 8 / 12 / 16 / 24 / 32` px | **4 为基数**，禁止 5/7/10/15/20 这类值 |
| `--vb-radius-sm` | `4px` | 输入框、小按钮 |
| `--vb-radius-md` | `6px` | 按钮、Tab、下拉 |
| `--vb-radius-lg` | `8px` | 面板内的卡片、分组块 |
| `--vb-radius-xl` | `12px` | 底部工具条、浮层 |
| `--vb-stroke-hairline` | `1px` | 分隔线 |
| `--vb-stroke-focus` | `1.5px` | 焦点环 |

> **圆角纪律**：egui 默认圆角是 2px（近乎直角），这是"业余感"的三大来源之一。**全局提到 6px，浮层 12px。** 对应 `visuals.widgets.*.corner_radius`、`menu_corner_radius`、`window_corner_radius`。

#### 字号与字重

| 令牌 | 字号 / 行高 | 字重 | 用途 |
|---|---|---|---|
| `--vb-font-caption` | `11 / 16` | 400 | 状态栏、tooltip 副行 |
| `--vb-font-label` | `12 / 18` | 500 | 面板字段标签、图层行 |
| `--vb-font-body` | `13 / 20` | 400 | 默认正文、输入内容 |
| `--vb-font-body-strong` | `13 / 20` | 600 | 面板分组标题、选中项 |
| `--vb-font-title` | `15 / 22` | 600 | 对话框标题 |
| `--vb-font-mono` | `12 / 18` | 400 | 十六进制、代码视图 |

> **egui 限制**：`Style.text_styles` 是 `BTreeMap<TextStyle, FontId>`，**FontId 只有 family + size，没有字重**。要做字重需要**为每个字重注册独立的 FontFamily**（见 3.3）。这是本轮唯一需要"绕"的地方。

#### 动效

| 令牌 | 时长 | 用途 | egui API |
|---|---|---|---|
| `--vb-motion-instant` | `0ms` | 画布缩放/平移、拖动 | 直接赋值，不走动画 |
| `--vb-motion-hover` | `80ms` | 悬停变色 | `Style.animation_time = 0.08` |
| `--vb-motion-state` | `120ms` | 选中、开关、展开收起 | `ctx.animate_bool_with_time(id, v, 0.12)` |
| `--vb-motion-panel` | `200ms` | 面板折叠、Tab 切换 | `animate_value_with_time(id, v, 0.20)` |
| `--vb-motion-popup` | `150ms` | 下拉、tooltip 淡入 | egui 内建 |

> **绝不做的动效**：画布元素入场动画、按钮弹跳、粒子效果。工具软件里这些是减速带。

### 3.3 字体系统

**现状问题**：`setup_fonts` 硬编码 `C:\Windows\Fonts\simhei.ttf` → 中文用黑体（一款偏"印刷/公告"的老字体，在小字号下发糊），英文用 egui 默认字体，**中英混排时基线不齐**。这是"业余感"的三大来源之二。

**目标方案**

| 用途 | 字体 | 许可 | 备注 |
|---|---|---|---|
| 拉丁字母 / 数字 / 符号 | **Inter**（首选） | SIL OFL 1.1 | Figma 的 UI 字体就是 Inter。数字字形尤其重要（数值框里全是数字） |
| 中文（简体） | **MiSans**（首选） / **思源黑体 SC**（备选） | MiSans 免费商用；思源 OFL | MiSans 更贴合"现代工具"气质；思源覆盖更全 |
| 图标 | 见 3.4 | — | 单独 FontFamily |
| 等宽 | **JetBrains Mono** 或 Inter + `tnum` | OFL | 十六进制对齐 |

**字体策略**

1. **字体随包分发**，不再读系统字体路径。`assets/fonts/` 放 `Inter-Regular/Medium/SemiBold.ttf` + `MiSans-Regular/Medium.ttf`。体积约 +2.5MB（压缩后更小），相对 120MB 预算可忽略。
2. **建 3 个 FontFamily**：`vb-ui-regular` / `vb-ui-medium` / `vb-ui-semibold`。用 `ctx.set_fonts` 注册后，`FontId::new(13.0, FontFamily::Name("vb-ui-medium".into()))` 就能拿到字重。
3. **fallback 链**：`Inter → MiSans → 系统 CJK`，保证缺字时降级不出现豆腐块。
4. **进阶（P3 可选）**：用 `fontdb`/`fontique` 扫描系统已安装的 Inter/MiSans，命中则优先用系统的（省体积 + 用户可能装了更好的版本）。

**验收**：中英混排（如 `宽度 W 320 px`）在同一基线上，无高度跳变；`11px` 的中文可辨。

### 3.4 图标系统

**现状问题**：图标是 emoji（📁 ▢ 🖼 ✎ ❄ 🗑 ✋）与 ASCII（▣ ➤ ▭ ◯）。问题有三：① emoji 在不同 Windows 版本渲染不同（Segoe UI Emoji 更新会变样）② 彩色 emoji 与单色 UI 冲突 ③ 无法统一描边粗细。这是"业余感"的三大来源之三。

**方案**

| 项 | 选择 | 理由 |
|---|---|---|
| 图标集 | **Lucide**（首选，MIT，1500+ 图标，描边风格，与 Figma 的图标气质接近） | 开源、风格统一、有 SVG 源可直接转字体 |
| 集成库 | **`iconflow` 1.0**（内置 Bootstrap/Heroicons/Phosphor/Lucide/Tabler，type-safe，提供原始字体字节） | **不依赖具体 egui 版本**（只吃字体字节），比 `egui-phosphor`（跟随 egui 版本，当前可能滞后于 0.35）更稳 |
| 备选 | `egui-phosphor` 0.13（MIT/Apache，使用最简单） | 若不想引入 iconflow 的包管理，这是次优 |
| 缺失图标 | 自绘 SVG 转字体，追加到私有字体里 | Lucide 缺"图层/编组/画板/冻结块"这类专业图标 |

**图标规格**

| 项 | 规格 |
|---|---|
| 尺寸 | `14px`（面板内）/ `16px`（工具条）/ `20px`（底部工具条） |
| 描边 | 统一 `1.5px`（Lucide 默认） |
| 颜色 | 跟随文字色，不单独上色（激活态用 `--vb-accent`） |
| 与文字间距 | `6px` |
| tooltip | **必须写"名称 (快捷键)"**，例：`选择工具 (V)` —— 零成本学习模式（见 2.2 第 11 条） |

**必须替换的清单**（现状 → 目标）：图层树图标（`kind_icon`）、工具箱 4 个工具、面板展开箭头、图层面板按钮（⊕图层/⊕子层/⊕编组/🗑）、状态栏（画板导航 ◀▶、坐标 ⌖、缩放 ▼）。

### 3.5 组件规格

**建立 `vb_ui` 组件层**（把 `vb_ui` 从 4 行空壳变成真 crate）。11 个组件：

| # | 组件 | 规格要点 | 参考 |
|---|---|---|---|
| 1 | **ToolButton** | 24×24，圆角 6，悬停 `--vb-bg-hover`，激活 `--vb-accent-dim` + 图标变 accent；长按 200ms 展开同组（小三角角标） | AI 工具箱 |
| 2 | **NumField** ⭐ | `scrubby`：拖动标签横向改值，光标变 `col-resize`；`↑/↓` 步进 1，`Shift+↑/↓` 步进 10；支持表达式（`320/2`→160）；**连续输入合并为一次 undo** | 03 篇 §四 + Figma |
| 3 | **ColorField** | 色块 + hex 并排；点击展开取色器；`Alt+点击` 开完整选择器；支持 `HEX/RGB/HSL/CSS变量` 四态输入 | 03 篇 §四 |
| 4 | **SectionHeader** | 可折叠分组头：`11px/600` + 折叠箭头 `⌄`；点击整行切换；状态持久化 | Figma 面板 |
| 5 | **PanelTabs** | 顶部分组：`属性 / 图层 / 令牌`；Tab 选中用 `--vb-accent-dim` 底 + accent 下划线 2px | Figma 右侧 |
| 6 | **LayerRow** | 行高 24；图标 14 + 名称 12 + 右侧 👁🔒（悬停才显形）；选中整行 `--vb-accent-dim`；双击行进就地改名 | 03 篇 §5.1 |
| 7 | **ValueOverlay** ⭐ | 画布上的浮动数值条：半透明深底 + 圆角 6 + `11px` 白字；显示 `X Y ΔX ΔY W H ∠` | AI 招牌 |
| 8 | **StatusChip** | 状态栏的胶囊标签（画板 1/4、缩放下拉、坐标） | — |
| 9 | **Toast** | 右下角短暂提示（"已保存" / "导出 3 项完成"），2s 淡出 | — |
| 10 | **EmptyState** | 无选区时属性面板不消失，显示画板属性或"选中对象以编辑属性" | 2.2 第 10 条 |
| 11 | **CommandPalette** ⭐ | `Mod+K` 弹出，模糊搜索 `commands.yaml` 的 label，回车执行 | 2.2 第 8 条 |

> ⭐ 标记的 4 个是本轮**最高性价比**的组件：NumField 和 ValueOverlay 决定"手感"，CommandPalette 决定"可发现性"，ColorField 决定"改配色的效率"。

### 3.6 布局重构

**现状**：菜单栏（含工具按钮）→ 右侧单面板（画板 + 属性 + 网页 + 图层 + 令牌，**纵向堆在一个滚动条里**）→ 状态栏 → 画布。信息架构混乱：图层面板在属性面板下面，需要滚很久。

**目标布局**

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ 文件  编辑  对象  选择  效果  视图  窗口  帮助                      ─  □  ×  │ 菜单 32
├──────────────────────────────────────────────────────────────────────────────┤
│ 变换  X 120  Y 80   W 320  H 180   ∠0°  ⬚  填充 ■  描边 □  不透明度 100%    │ 控制面板 40
├────┬────────────────────────────────────────────────┬────────────────────────┤
│    │  ▏标尺                                         │ ┌ 属性 ┬ 图层 ┬ 令牌 ┐ │
│ 工 │────────────────────────────────────────────────│ │                    │ │
│ 具 │                                                │ │  位置              │ │
│ 箱 │        ┌───────────────────────────────┐       │ │   X 120   Y 80     │ │
│ 44 │        │  画板 Hero  1440×900          │       │ │   W 320   H 180    │ │
│    │        │   ┌──────┐    ┌──────────┐    │       │ │   ∠ 0°    ⬚       │ │
│ 单 │        │   │      │    │          │    │       │ │                    │ │
│ 列 │        │   └──────┘    └──────────┘    │       │ │  外观              │ │
│    │        │                               │       │ │   填充 ■ #FF5A1F   │ │
│    │        └───────────────────────────────┘       │ │   描边 □ 无  2px   │ │
│    │                                                │ │   效果 + 添加      │ │
│    │                                                │ │                    │ │
│    │              画板外区域略暗 8%                  │ │  ▸ 布局            │ │
│    │                                                │ │  ▸ 交互            │ │
│    │                                                │ │  ▸ 导出            │ │
│    │                                                  │ │                    │ │
│    │        ╭──────────────────────────────╮          │ │                    │ │
│    │        │ ➤ ▭ ◯ T P ╲ ✋ Z  ⬚ 100% ▾ │          │ │                    │ │
│    │        ╰──────────────────────────────╯          │ │                    │ │
├────┴────────────────────────────────────────────────┴────────────────────────┤
│ ◀ 1/4 ▶ │ 1440×900 px │ 66% ▾ │ ⌖ 320,180 │ 提示文本…                        │ 状态 28
└──────────────────────────────────────────────────────────────────────────────┘
```

| 区域 | 规格 | 变化 |
|---|---|---|
| 菜单栏 | 高 32 | **移除工具按钮**（挪到画布底部） |
| 控制面板 | 高 40 | **新增**（AI 招牌：随工具/选区变化） |
| 工具箱 | 宽 44（单列）/ 64（双列） | 图标 24px，比 03 篇的 60px 更紧凑（Figma 的工具栏也不宽） |
| 标尺 | 上 / 左，高 20 | 新增（P4 落地，本轮预留空间） |
| 浮动工具条 | 底部居中，高 40，圆角 12，半透明 96% + 阴影 | **新增**，Figma UI3 的标志性设计 |
| 右侧面板坞 | 宽 280（240–420 可拖） | **改为 Tab 分组**，不再纵向堆叠；`< 1200px` 自动折叠为 40px 图标条 |
| 状态栏 | 高 28 | 保持 |

**落地建议**：右侧面板坞用 **`egui_dock` 0.20**（MIT，支持 egui 0.35，`213k 下载/月`，成熟）而不是自研。`egui_tiles` 0.17（Rerun 赞助）更灵活但更早期，本项目不需要网格布局，选 `egui_dock`。

### 3.7 操作手感规格

**"手感"= 输入延迟 + 视觉反馈 + 光标语义 + 动效**。逐项定死：

| 项 | 规格 | 现状 |
|---|---|---|
| 按键 → 反馈 | ≤ 16ms（高刷屏 ≤ 8ms） | 未测 |
| 拖动对象重绘 | ≤ 8ms | 未测 |
| **光标体系** | 每种工具 + 每个区域一个光标（见下表） | **只有 Space→Grab** |
| **拖动数值浮层** | 移动/缩放/旋转时跟随光标，显示实时数值 | **缺失** |
| 悬停高亮 | 悬停对象显示 `--vb-hover-box` 轮廓 1px | 部分有 |
| 手柄命中半径 | 6px（屏幕空间） | ✅ 已有（`hit_handle` R=6.0） |
| 智能参考线吸附阈值 | 6px 屏幕空间（不随缩放变） | 待核 |
| **吸附临时禁用** | 拖动中按 `Mod` 禁用吸附（AI 行为） | 缺失 |
| 拖动中 `Esc` 取消 | 对象回原位 | ✅ 已有（`Drag::None`） |
| 数字输入合并 undo | 连续输入算一次 | 待核 |

**光标映射表**

| 场景 | 光标 | egui |
|---|---|---|
| 选择工具·空白 | `Default` | `CursorIcon::Default` |
| 悬停可选对象 | `PointingHand` | `CursorIcon::PointingHand` |
| 悬停手柄（角） | `ResizeNwSe` / `ResizeNeSw` | `CursorIcon::ResizeNwSe` |
| 悬停手柄（边） | `ResizeHorizontal` / `ResizeVertical` | 同名 |
| 手柄外圈（旋转） | 自定义旋转光标 | 用 `ResizeNwSe` 兜底或自绘 |
| Space / 抓手工具 | `Grab` → 拖动中 `Grabbing` | ✅ 部分有 |
| 缩放工具 | `ZoomIn` / `ZoomOut`（`Alt` 时） | 同名 |
| 钢笔 | `Crosshair` | 同名 |
| 文本工具 | `Text` | `CursorIcon::Text` |
| **禁用/锁定对象** | `NotAllowed` | `CursorIcon::NotAllowed` |

> **egui 限制**：不支持自定义光标图片（0.35 只支持系统内置 `CursorIcon`）。旋转光标只能"用最接近的内置光标 + 视觉提示（手柄外圈画弧线）"来兜。这是可接受的妥协。

### 3.8 egui 落地配方

**核心手法：把令牌写成一个 `vb_ui::theme` 模块，用 `all_styles_mut` 一次性注入。**

```rust
// crates/vb_ui/src/theme.rs （新文件）
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

pub struct Tokens { /* 从 vb-ui-tokens.json 生成或用 const 写死 */ }

pub fn apply(ctx: &egui::Context, dark: bool, scale: f32) {
    let t = Tokens::dark_or_light(dark);

    // 1) 注册字重 family（Inter + MiSans 按 3.3 节注册）
    //    ctx.set_fonts(...) 在 app 启动时做一次即可

    ctx.all_styles_mut(|s| {
        // ---- 色彩 ----
        s.visuals.dark_mode            = dark;
        s.visuals.panel_fill           = t.bg_panel;
        s.visuals.window_fill          = t.bg_raised;
        s.visuals.extreme_bg_color     = t.bg_canvas;
        s.visuals.faint_bg_color       = t.bg_input;
        s.visuals.weak_text_color      = Some(t.text_2);
        s.visuals.selection.bg_fill    = t.accent;
        s.visuals.selection.stroke     = Stroke::new(1.0, t.text);
        s.visuals.hyperlink_color      = t.accent;
        s.visuals.warn_fg_color        = t.warn;
        s.visuals.error_fg_color       = t.danger;

        // ---- 圆角（业余感三大来源之一，必须改） ----
        let r_sm = CornerRadius::same(4);
        let r_md = CornerRadius::same(6);
        let r_lg = CornerRadius::same(8);
        s.visuals.window_corner_radius = CornerRadius::same(12);
        s.visuals.menu_corner_radius   = r_lg;
        s.visuals.widgets.noninteractive.corner_radius = r_md;
        s.visuals.widgets.inactive.corner_radius       = r_md;
        s.visuals.widgets.hovered.corner_radius        = r_md;
        s.visuals.widgets.active.corner_radius         = r_md;
        s.visuals.widgets.open.corner_radius           = r_md;

        // ---- 去"凸起感"：expansion 是 egui 默认的立体效果来源 ----
        s.visuals.widgets.inactive.expansion = 0.0;
        s.visuals.widgets.hovered.expansion  = 0.0;
        s.visuals.widgets.active.expansion   = 0.0;

        // ---- 状态底/描边 ----
        s.visuals.widgets.inactive.weak_bg_fill = t.bg_input;
        s.visuals.widgets.hovered.weak_bg_fill  = t.bg_hover;
        s.visuals.widgets.active.weak_bg_fill   = t.bg_active;
        s.visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
        s.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
        s.visuals.widgets.hovered.bg_stroke  = Stroke::new(1.0, t.border_strong);
        s.visuals.widgets.active.bg_stroke   = Stroke::new(1.5, t.accent);

        // ---- 间距（4 基数） ----
        s.spacing.item_spacing     = egui::vec2(8.0, 6.0);
        s.spacing.button_padding   = egui::vec2(10.0, 5.0);
        s.spacing.menu_margin      = egui::Margin::same(6);
        s.spacing.window_margin    = egui::Margin::same(12);
        s.spacing.indent           = 16.0;
        s.spacing.interact_size    = egui::vec2(24.0, 24.0);
        s.spacing.slider_width     = 120.0;
        s.spacing.combo_width      = 100.0;
        s.spacing.text_edit_width  = 100.0;
        s.spacing.icon_width       = 14.0;
        s.spacing.icon_spacing     = 6.0;

        // ---- 动效（3.2 节表） ----
        s.animation_time = 0.12;

        // ---- 排版 ----
        s.text_styles.insert(TextStyle::Body,      FontId::new(13.0, family_medium()));
        s.text_styles.insert(TextStyle::Button,    FontId::new(13.0, family_medium()));
        s.text_styles.insert(TextStyle::Small,     FontId::new(11.0, family_regular()));
        s.text_styles.insert(TextStyle::Heading,   FontId::new(15.0, family_semibold()));
        s.text_styles.insert(TextStyle::Monospace, FontId::new(12.0, family_mono()));

        // ---- 去掉 egui 默认的"UI 感"开关 ----
        s.visuals.button_frame         = false; // 按钮默认无框，用自定义 ToolButton
        s.visuals.striped              = false; // 表格不斑马纹（很"Excel"，不专业）
        s.visuals.slider_trailing_fill = true;
        s.visuals.handle_shape         = egui::style::HandleShape::Circle;
    });
}
```

**四条关键 diff（相对现状）**

| 改动 | 现状 | 目标 | 效果 |
|---|---|---|---|
| `corner_radius` | 默认 2px | 6/8/12px | 立刻"现代化" |
| `expansion` | 默认有凸起 | `0.0` | 去掉 2005 年的立体感 |
| `animation_time` | 未设置（默认 0.083，但 hover 只改描边） | `0.12` + hover 改底 | 悬停有"呼吸" |
| `widgets.*.weak_bg_fill` | 只有描边变化 | 底色变化 | 悬停可感知 |

**中文输入注意**：egui 0.35 的 `ImeComposition.legacy_visuals` 在 Windows 上**默认是 `true`**（因为 winit 的韩文 IME 光标位置 bug），这会让中文输入法的候选/预编辑文本"看起来像选中文本"。对中文优先的软件建议**显式设为 `false`** 并实测，获得正确的预编辑高亮。这是一个容易漏掉的细节。

### 3.9 主题系统（消灭字面量）

**目标**：03 篇 §九 验收项「深色/浅色主题切换无残留硬编码颜色（代码扫描：禁止 `fill: #xxxxxx` 字面量）」。

**落地步骤**

1. 新建 `crates/vb_ui/src/theme.rs`，令牌以 `const` + `struct Tokens` 形式定义（不引入 JSON 运行时解析，编译期确定）
2. 同时产出 `docs/design/assets/vb-ui-tokens.json` 作为**设计侧单一真相**（给设计工具/未来主题编辑器用），并在 CI 里校验两者一致
3. **全局替换 `app.rs` 里的字面量**（当前至少 12 处：`0x2a,0x2a,0x2a` / `0x1e,0x1e,0x1e` / `0x2e,0x86,0xff` / `0x3a,0x3a,0x3a`（`draw_grid`）/ `0x3d,0x3d,0x3d` …）
4. **主题切换**：`设置 → 界面 → 深色/浅色/跟随系统`，切换后 `ctx.all_styles_mut` 重跑，无需重启
5. **新增 CI 脚本** `tools/check_no_hardcoded_color.ps1`：正则扫 `Color32::from_rgb\(0x` 与 `#` 后跟 6 位十六进制，**允许白名单**（智能参考线品红、测试代码）

> **注意**：智能参考线的 `#FF00FF` 与边界框的 `#0D99FF` 是**语义色不是主题色**，写进令牌但标注"跨主题不变/仅微调"。

---

## 4 · AI 心智对齐方案

### 4.1 命令注册表：19 → 120 条

**架构**：`commands.yaml` 是单一真相（02 篇 §七-2 已定）。**现状是 19 条，02 篇 §八 验收项要求"命令 ID 覆盖 100% 可见操作"。**

**扩展方案**

| 步骤 | 动作 | 产出 |
|---|---|---|
| 1 | 把 `commands.yaml` 扩展为 ~120 条，字段补齐 | `id / label / keys{win,mac} / menu / undoable / expose_to_agent / context` |
| 2 | 新增 `crates/vb_ui/src/commands.rs`：**编译期**用 `include_str!` + `serde_yaml` 解析（或 `build.rs` 生成为 Rust 常量） | 命令表进二进制 |
| 3 | 菜单 / 快捷键 / 面板按钮 / CLI / MCP **全部改为查表派发**，不再手写 `match` | 一处改动五处生效 |
| 4 | 启动时**冲突自检**：同一 context 内重复键位 → warning + 禁用后者 | 02 篇 §七-3 |
| 5 | 新增 `Mod+/` **键位速查表面板** + `Mod+K` **命令面板** | 02 篇 §七-4；2.2 第 8 条 |

**命令表分批**（按 02 篇的章节）：

| 批次 | 内容 | 条数 | 阶段 |
|---|---|---|---|
| 批 1 | 文件 12 + 编辑（含剪贴板）14 + 视图 14 + 状态 8 | ~48 | **P2** |
| 批 2 | 对象（编组/层序/锁定/隐藏/蒙版）18 + 对齐分布 12 + 选择 10 | ~40 | **P3** |
| 批 3 | 文字 12 + 画板切片 8 + 工具箱 24 | ~44 | **P4–P5** |

**命令上下文（context）字段**是本轮新增的关键设计——它同时是 B1 bug 的解法：

```yaml
- id: object.delete
  label: 删除
  keys: { win: "Delete", mac: "Backspace" }
  context: [global, canvas]        # ← 新增：不在 text.edit 上下文内生效
  undoable: true
  expose_to_agent: true
```

上下文栈（02 篇 §一）：
```
text.edit  >  panel.focus  >  tool.active  >  canvas  >  global
```
**只有栈顶上下文消费按键。** 这一条实现后，B1 自动消失。

### 4.2 工具箱：4 → 13 个 P0 工具

**工具接口**（06 篇 §一 已定 `trait Tool`，本轮落地）：

```rust
pub trait Tool {
    fn id(&self) -> ToolId;
    fn cursor(&self, ctx: &ToolCtx) -> CursorIcon;
    fn on_pointer_down(&mut self, ctx: &mut ToolCtx, e: PointerEvent) -> ToolSignal;
    fn on_pointer_move(&mut self, ctx: &mut ToolCtx, e: PointerEvent) -> ToolSignal;
    fn on_pointer_up(&mut self, ctx: &mut ToolCtx, e: PointerEvent) -> ToolSignal;
    fn on_key(&mut self, ctx: &mut ToolCtx, k: KeyEvent) -> ToolSignal;
    fn draw_overlay(&self, ctx: &ToolCtx, ov: &mut OverlayCanvas);
    fn control_panel(&self) -> ControlPanelSpec;   // 控制面板字段
}
// ToolSignal::{None, Commit(Command), Preview(Command), Cancel, SwitchTool(ToolId)}
```

**铁律**（06 篇 §一）：**工具只能产出 `Command`，不得直接改 `Document`。** 拖动中的 `Preview` 不入 undo 栈，`up` 时 `Commit` 一次。

**现状问题**：当前 `Tool` 是个裸 `enum`（4 个变体），所有交互逻辑写成 `VellumApp` 上的大 `match`（`Drag` enum + `app.rs` 里四处 `impl`）。**这是本轮最大的重构点**：把 4 个工具的散装逻辑抽成 trait 实现，才能往上加第 5–13 个而不失控。

**13 个 P0 工具分三批**

| 批 | 工具 | 键 | 关键行为（06 篇） | 阶段 |
|---|---|---|---|---|
| **批 1** | 选择 `V` | 已有 | 补：拖动数值浮层、`Mod` 禁用吸附、`Space` 拖中平移、`Esc` 取消 | P2 |
| | 直接选择 `A` | 新 | 锚点空心/实心、框选锚点、`Alt` 单侧手柄 | P4（依赖矢量） |
| | 矩形 `M` | 已有 | 补：拖动中 `↑↓` 调圆角、`Alt` 从中心、拖动中 `C`/`F`/`[`/`]` | P2 |
| | 椭圆 `L` | 已有 | 补：`Alt` 从中心、`Shift` 正圆 | P2 |
| | 直线 `\` | 新 | 拖动 + `Shift` 45° | P2 |
| | 抓手 `H` | 已有 | 补：双击适合窗口 | P2 |
| | 缩放 `Z` | 新 | `Z+点击` 放大、`Alt+点击` 缩小、`Z+拖框` 放大到区域 | P2 |
| **批 2** | 文字 `T` | 新 | 点文本 / 区域文本、双击进编辑、溢出红点、`Esc` 二次放弃 | P3 |
| | 画板 `Shift+O` | 新 | 拖框新建、双击改名、`⋮` 菜单（导出/复制/删除/重排） | P3 |
| | 吸管 `I` | 新 | 点击取色、`Alt` 取全部属性、`Shift` 只取色 | P3 |
| | 渐变 `G` | 新 | 画布内批注者、拖动改方向长度、双击色标改色 | P3 |
| | 编组选择 `Y` | 新 | 单击选中编组内对象 | P3 |
| **批 3** | 切片 `Shift+K` | 新 | 拖框建切片 + 导出预设 | P5 |
| | 钢笔 `P` | 保留键位 | **P4 才实现**，本轮：按 `P` 弹"计划于 v0.4 支持" | P4 |

> **不支持工具的处理**（06 篇 §二）：默认隐藏；`设置 → 工具 → 显示未支持工具` 开启后显示为**置灰图标**，点击弹"计划于 vX 支持"。**绝不能点了没反应。**

### 4.3 四大灵魂手势的精确语义

**这四个手势是 02 篇 §二 的"AI 招牌"，本轮必须逐位对齐并加自动化测试。**

| # | 手势 | 精确语义 | 现状 |
|---|---|---|---|
| **G1** | `Alt + 拖动对象` | 复制并拖动。**拖动前按下或拖动中按下都生效**（AI 支持中途按下） | 部分有（`Drag::MoveObj` 有 `alt` 首动复制）——需补"中途按下" |
| **G2** | `Shift + 拖动对象` | 约束方向：水平 / 垂直 / 45°。**不是"等比"**（等比是手柄行为） | ✅ 有（`constrain_axis`） |
| **G3** | `Alt + Shift + 拖动` | 约束 + 复制 | ✅ 组合已有 |
| **G4** | `Space + 拖动`（**拖动中按下**） | 边拖边平移画布 ⚠️ 02 篇标注"AI 独有，很多人不知道但一用就回不去" | **缺失**（`space_down` 只设光标） |

**手柄语义**（02 篇 §5.4）

| 操作 | 行为 | 现状 |
|---|---|---|
| 拖角手柄 | **单轴**（AI 语义） | ✅ `resize_geom(g0, handle, dx, dy, shift, alt)` |
| `Shift` + 拖角手柄 | 强制等比 | ✅ |
| `Alt` + 拖手柄 | 从中心缩放 | ✅ |
| 手柄外悬停 | 旋转光标 | 缺（见 3.7 egui 限制） |
| `Shift` + 旋转 | 约束 15° | ✅ 有"15° 吸附" |
| **变换时实时数值浮层** | `W/H/ΔX/ΔY/角度` | **缺失** ⭐ |

### 4.4 数值浮层与实时信息

**这是"AI 手感"的最后一公里，成本低、感知强。** 规格：

- **位置**：跟随光标，`+16px` 偏移，靠近画布边缘时自动翻转到另一侧
- **样式**：`rgba(28,28,30,0.92)` 底 + 圆角 6 + `11px` 白字 + 1px 边框
- **内容**（按操作类型）

| 操作 | 显示 |
|---|---|
| 移动对象 | `X 120  Y 80   ΔX +12  ΔY -4` |
| 缩放对象 | `W 320  H 180   ΔW +20  ΔH +0` |
| 旋转对象 | `∠ 15.0°` |
| 绘制矩形 | `W 320  H 180`（+ 圆角 `R 8` 有值时显示） |
| 画板工具 | `W 1440  H 900` |
| 智能参考线触发 | 参考线旁 `11px` 品红字标注间距数值 |

### 4.5 智能参考线五类（待核清单）

06 篇 §4.1 要求五类。**现状有骨架但未验证齐全，本轮逐类核对：**

| # | 类型 | 触发条件 | 现状 |
|---|---|---|---|
| 1 | **对齐** | 对象边/中心/锚点 与 其他对象或画板 的 边/中心 对齐 | 待核 |
| 2 | **间距** | 与其他对象形成等距时，显示间距数值 + 双向箭头 | 待核（很可能缺） |
| 3 | **角度** | 旋转接近 0/45/90/180/270 或与其他路径平行/垂直 | 待核 |
| 4 | **尺寸相等** | 拖动尺寸与其他对象宽/高一致 | 待核（很可能缺） |
| 5 | **画板中心** | 与画板水平/垂直中心对齐 | 待核 |

**共同规格**：颜色品红 `#FF00FF`，1px 虚线；标签 `11px` 深底品红字；**吸附阈值 = 屏幕空间 6px**（不随缩放变化）；优先级 **画板 > 最近对象 > 网格**；拖动中按 `Mod` **临时禁用**（缺）。

### 4.6 AI 用户盲测协议（可执行）

**这是本节唯一能证明"上手了"的东西。**

**准备**
- 招募 3 名 **Adobe Illustrator 使用 ≥ 2 年**的人（社区/Discord/朋友）
- 环境：干净 Windows 机器 + 本软件 + `examples/landing` 工程
- **不给任何文档、不给提示**。只给一句引导："请打开这个工程，完成这 5 件事。"
- 屏幕录制 + 计时 + 记录卡壳

**5 项任务**（对应 00 篇 §七 MVP 链路）

| # | 任务 | 通过标准 |
|---|---|---|
| T1 | 把「主标题」向左移动 20px，并把它的字号改大 | 用拖动或 `←` 或属性面板，**不打开代码视图** |
| T2 | 复制这个按钮，放到原按钮右边 | 用 `Alt+拖动` 或 `Ctrl+D`（**变换沿用也认**） |
| T3 | 把「标题 + 副标题」编组，再取消编组 | `Ctrl+G` / `Ctrl+Shift+G`，或菜单 |
| T4 | 让「卡片」在画板上水平居中 | `Ctrl+Shift+C` 或对齐面板 |
| T5 | 导出 Hero 画板 @2x PNG | `Ctrl+E` 或菜单，**文件真的生成** |

**通过线**
- **成功率 ≥ 80%（≥ 12/15）**
- **无任何一项任务全员失败**（否则判定该环节设计失败，不是用户问题）
- **T1 平均耗时 ≤ 30s**（这是最基础的操作，慢了说明手感有问题）

**记录表**（每次盲测产出一份，进 `docs/usability/`）

| 被试 | 任务 | 成功 | 耗时 | 首次点击位置 | 卡壳点 | 是否看了代码视图 |
|---|---|---|---|---|---|---|
| P1 | T1 | ✅ | 22s | 属性面板 X 输入框 | 找不到字号在哪 | 否 |

**数据去向**：卡壳点 → 下一轮 bug list；"找不到 X" → 面板信息架构调整；"看了代码视图" → 该能力在 GUI 缺失。

---

## 5 · 开源参照物与许可边界

### 5.1 参考清单

| 项目 | 许可 | 拿什么 | **不拿什么** | 优先级 |
|---|---|---|---|---|
| **Graphite**（Rust + wgpu 2D 编辑器，27k★） | Apache-2.0 / GPL-3.0（需确认具体 crate） | ① **反例**：自研 GUI 框架的代价（2020 年整年"halting progress"）② 工具/消息总线架构思路 ③ 面板与画布同进程的 wgpu 组织方式 | 节点图架构（本项目是文档树不是节点图）、程序化编辑模型 | ⭐⭐⭐ |
| **Penpot**（Clojure，开源 Figma） | MPL-2.0 | ① 面板信息架构（属性分组的顺序）② 设计令牌的 UI 表达 ③ 深色主题的灰阶层次 | 代码（语言不同）、协作/后端部分 | ⭐⭐⭐ |
| **Inkscape** | **GPL-2.0-or-later** ⚠️ | **只作对照答案**：`share/keys/` 下的 `adobe-illustrator-cs2.xml` 与社区贡献的 **Illustrator CC 2024 兼容键位文件**，用来逐条核对我们的键位表是否与 AI 一致 | **不粘贴文件内容进仓库**（GPL 污染 MIT 项目）。只抄"事实"（某命令对应某键），并在文档里注明"键位事实经 Inkscape 键位文件交叉核对" | ⭐⭐⭐ |
| **Blender** | GPL | **思路**：`Industry Compatible` 键位方案——让 Maya 用户能直接用 Blender。这正是本项目对 AI 用户要做的事，可参考其"键位方案切换器"的产品设计 | 代码 | ⭐⭐ |
| **Krita** | GPL | **思路**：内置多套键位方案（`krita` / `photoshop` / `illustrator`）供切换。呼应 02 篇 §一 的 `AI 默认 / AI 兼容 / 自定义` | 代码 | ⭐⭐ |
| **egui_dock** 0.20 | MIT | 右侧面板坞 + Tab + 拖拽停靠。支持 egui 0.35，成熟（213k 下载/月） | — | ⭐⭐⭐ |
| **egui_tiles** 0.17（Rerun 赞助） | MIT/Apache-2.0 | 若未来需要网格布局再评估；本轮不需要 | — | ⭐ |
| **iconflow** 1.0 | MIT | 图标字体集成（Lucide / Phosphor / Tabler / Heroicons / Bootstrap），**不绑定 egui 版本** | — | ⭐⭐⭐ |
| **egui-phosphor** 0.13 | MIT/Apache-2.0 | iconflow 的替代（更简单，但跟随 egui 版本，当前可能滞后） | — | ⭐⭐ |
| **Lucide** | ISC（近似 MIT） | 图标集本身（1500+，描边风格，气质接近 Figma） | — | ⭐⭐⭐ |
| **Inter** | SIL OFL 1.1 | UI 字体（Figma 同款） | — | ⭐⭐⭐ |
| **MiSans / 思源黑体 SC** | MiSans 免费商用 / OFL | 中文 UI 字体 | — | ⭐⭐⭐ |
| **Rerun viewer** | MIT/Apache-2.0 | **质量标杆**：egui 生态里产品级 UI 的参照坐标。看它的间距/层次/克制程度 | — | ⭐⭐ |
| **Figma UI3 设计记录**（博客，非开源） | — | 读它的**设计决策与回滚原因**（浮动面板、极简标签、图标按钮），避免重走 | 视觉直接复刻（侵权风险） | ⭐⭐⭐ |
| **WKWebView / Servo** | — | 不参考。不引入浏览器内核（ADR-0001） | — | — |

### 5.2 许可红线（写进 `docs/deps.md`）

1. **MIT 项目不得包含 GPL 代码或数据文件**。Inkscape 的键位文件、Blender/Krita 的任何代码，**只能作为"阅读材料"**，不得进入仓库。
2. **抄事实不抄表达**：命令与键位的对应关系是事实；键位文件的 XML 结构、注释、组织方式是表达。
3. **字体嵌入要核对许可**：Inter（OFL）与 MiSans 可随包分发；**不要**嵌入 Windows 系统字体（SimHei/MSYH 的再分发有限制）——现状硬编码读系统路径反而"安全"，但换自备字体时必须确认许可。
4. **图标集许可独立于集成库**：`egui-phosphor` 是 MIT，但它捆绑的 Phosphor Icons 也是 MIT；Lucide 是 ISC。都要在 `docs/deps.md` 记一行（09 篇 §二"依赖纪律"）。

---

## 6 · 迭代路线（P1–P5 垂直切片）

**核心纪律**：每个阶段结束，**必须多出一条"用户能完整跑通的任务流"**，而不是"多了 N 个功能点"。

### 阶段总览

| 阶段 | 主题 | 关键交付（任务流） | 预估 |
|---|---|---|---|
| **P1** | **地基** | "界面不再说谎" —— 假加速键修好、快捷键不串味、门禁能跑 | 1.5 周 |
| **P2** | **视觉重制** | "一眼像个正经工具" —— 新主题 + 字体 + 图标 + 组件层 + 新布局 | 3 周 |
| **P3** | **AI 交互对齐（核心）** | "AI 用户能上手" —— 命令表 + 8 个工具 + 灵魂手势 + 数值浮层 | 4 周 |
| **P4** | **矢量与精度** | "能画能用" —— 钢笔 + 直接选择 + 参考线五类 + 标尺 + 隔离 | 4 周 |
| **P5** | **收口** | "能交付" —— 门禁全过 + 打包 + 盲测达标 + 示例工程 | 2 周 |

**P1–P5 全走完 ≈ 14.5 周（全职）；业余 10–15h/周 → 4–6 个月。**

---

### P1 · 地基（1.5 周）

> **这一阶段不做任何视觉改动。** 目的是让后续所有改动有安全网。

| # | 任务 | 验收 |
|---|---|---|
| 1.1 | **修 B1**：引入输入上下文栈（`text.edit > panel.focus > tool.active > canvas > global`），只有栈顶消费按键 | 文本编辑态下按 `V`/`M`/`Delete`/`Backspace` 不切工具、不删对象 |
| 1.2 | **修 B2**：把视图菜单 4 个加速键真正绑上（`Ctrl+0/1/+/-`） | 菜单显示的每个加速键都生效；写测试断言"菜单 label 里的键位 ⊆ 已绑定键位集合" |
| 1.3 | **修 B3**：`Ctrl+Y` 改为轮廓模式（或先解绑，`Ctrl+Shift+Z` 保留重做） | 与 02 篇一致 |
| 1.4 | 补 `tests/`：往返语料 20 个 + `vb_doc` 命令单元测试（`Command` 的 apply/undo 往返） | `cargo test --workspace` 全绿，`vb_doc` 覆盖率 ≥ 70% |
| 1.5 | 补 `tools/ci.ps1`：`fmt + clippy + test + selfcheck` 一键 | 本地一键跑通；接进 `.github/workflows/ci.yml` |
| 1.6 | 补 `tools/check_no_hardcoded_color.ps1`（先建立基线，允许现有违规，只报数） | 输出违规清单（约 12 处），作为 P2 的输入 |
| 1.7 | **提交信息纪律**：更新 `CONTRIBUTING` 或 `docs/design/13` 加一节，禁止 overclaim；同时**修正 README 的状态表**，把未落地项标回 ⏳ | README 状态表与代码一致 |

**P1 结束可演示**：在文本框里输入 "vmdelete" 不会切工具也不会删对象；按 `Ctrl+0` 画布适应窗口；`tools/ci.ps1` 一键绿。

---

### P2 · 视觉重制（3 周）

| # | 任务 | 验收 |
|---|---|---|
| 2.1 | **令牌落地**：`crates/vb_ui/src/theme.rs`（第 3.8 节配方）+ `docs/design/assets/vb-ui-tokens.json` | 界面视觉立刻变化；CI 校验两处一致 |
| 2.2 | **消灭字面量**：替换 `app.rs` 全部硬编码颜色（P1.6 清单） | `check_no_hardcoded_color` 零违规（白名单除外） |
| 2.3 | **字体系统**：`assets/fonts/`（Inter + MiSans）+ 3 个 FontFamily + fallback 链 | 中英混排不跳基线；`11px` 中文可辨 |
| 2.4 | **图标系统**：`iconflow`(Lucide) 集成，替换全部 emoji/ASCII | 全界面无 emoji；tooltip 含快捷键 |
| 2.5 | **组件层**：`vb_ui` 建立并抽出 11 个组件中的前 6 个（ToolButton / NumField / ColorField / SectionHeader / PanelTabs / LayerRow） | `vb_app` 不再直接调 `egui::DragValue` 裸控件 |
| 2.6 | **布局重构**：底部浮动工具条 + 右侧面板坞改 Tab（`egui_dock`）+ 控制面板骨架 | 1440p 下画布可见面积 ≥ 65%；`< 1200px` 自动折叠无横向滚动条 |
| 2.7 | **浅色主题** | 深/浅切换无残留色；浅色主题不是反相（独立调过灰阶） |
| 2.8 | **手感基础**：`animation_time = 0.12` + hover 改底色 + 光标体系（3.7 表） | 悬停 80ms 内可见；每种工具/区域光标正确 |

**P2 结束可演示**：一段 20 秒的录屏，展示新界面 + 深浅切换 + 悬停动效 + 图层 Tab。**这条录屏是"看起来像正经工具了"的证据。**

---

### P3 · AI 交互对齐（4 周）★ 最高价值

| # | 任务 | 验收 |
|---|---|---|
| 3.1 | **命令注册表**：`commands.yaml` 扩到 ~120 条（批 1+2），`vb_ui/src/commands.rs` 编译期加载，全部派发改查表 | 菜单/快捷键/面板/CLI 同源；启动无冲突 warning |
| 3.2 | **`Mod+K` 命令面板 + `Mod+/` 键位速查表** | 打字能搜到命令并执行 |
| 3.3 | **剪贴板**：`Mod+X/C/V` + 就地粘贴 `Mod+Shift+V`（当前**完全缺失**） | 跨文档粘贴可用 |
| 3.4 | **工具 trait 重构**：把 `VellumApp` 的散装 `Drag` 逻辑抽成 `trait Tool` | `app.rs` 从 2694 行降到 ≤ 1800 行；工具逻辑独立文件 |
| 3.5 | **工具箱补到 7 个**（批 1: 选择/直接选择/矩形/椭圆/直线/抓手/缩放） | 每个工具有 `cursor()` + `control_panel()` |
| 3.6 | **四大灵魂手势**（第 4.3 节，尤其 **G4 拖动中按 Space**） | 四个手势逐条手工验证 + 自动化测试 |
| 3.7 | **数值浮层**（第 4.4 节） | 移动/缩放/旋转/绘制时全部显示实时数值 |
| 3.8 | **对齐面板 + 6 快捷键 + 分布** | 复用 `patch.rs` 已有的 `align` 运算（**命令层已实现，只差接线**） |
| 3.9 | **层序/锁定/隐藏快捷键**（`Mod+[ ]` 已有，补 `Mod+2/3/Alt+2/Alt+3`）+ 图层面板拖拽重排 | 图层拖拽跨编组可用 |
| 3.10 | **禁用吸附**：拖动中 `Mod` 临时关吸附 | 02 篇 §4.1 |
| 3.11 | **盲测第 1 轮**（第 4.6 节协议） | 成功率 ≥ 80%，无任务全员失败 |

**P3 结束可演示**：**盲测录屏**。3 名 AI 用户不看文档完成 5 项任务。这是本轮最重要的交付物。

---

### P4 · 矢量与精度（4 周）

| # | 任务 | 验收 |
|---|---|---|
| 4.1 | **智能参考线五类**补齐（第 4.5 节，重点补"间距"与"尺寸相等"） | 五类逐条触发正确；阈值屏幕空间 6px |
| 4.2 | **标尺 + 参考线 + 网格**（`Mod+R` / `Mod+'` / `Mod+5` / `Mod+Alt+;`） | 从标尺拖出参考线；锁定生效 |
| 4.3 | **隔离模式**（双击进入 + 面包屑 + `Esc` 退出 + 淡化 25%） | 06 篇 §4.3 全部行为 |
| 4.4 | **轮廓模式** `Mod+Y`、隐藏边缘 `Mod+Shift+H`、像素预览 | 三项开关正确 |
| 4.5 | **钢笔工具**（完整贝塞尔：落点/平滑点/`Alt` 单侧/转换点/闭合/增删锚点） | 06 篇 §5.3 全部行为 |
| 4.6 | **直接选择工具**（锚点与手柄编辑、框选、删除重连） | 描摹一个 logo，锚点行为与 AI 无差异 |
| 4.7 | **文字工具**（点文本 / 区域文本 / 双击进编辑 / 溢出红点） | `T` 单击 vs 拖动区分；`Esc` 二次放弃 |
| 4.8 | **画板工具** `Shift+O` + 吸管 `I` + 渐变 `G` | 画板拖框新建、改名、`⋮` 菜单 |
| 4.9 | **隐藏边缘 / 空状态 / Toast**（P2 未完成的组件） | — |

**P4 结束可演示**：用钢笔描一个 logo，加参考线对齐，导出 PNG + SVG，浏览器打开视觉一致。

---

### P5 · 收口（2 周）

| # | 任务 | 验收 |
|---|---|---|
| 5.1 | **门禁补齐至 7 项**（10 篇）：渲染快照（`vello_cpu` 无 GPU）+ 性能基准 B1–B6 + 术语扫描 + i18n 覆盖率 + W3C/prettier 校验 | `tools/ci.ps1` 一键 7 项全过 |
| 5.2 | **i18n**：`i18n/zh-CN.ftl` + `en.ftl`，术语禁用表扫描接进 CI | 双语 100% 覆盖；无禁用词 |
| 5.3 | **基准首基线**：B1–B6 跑一遍，`bench-history.json` 落第一版数据 | 有基线可比 |
| 5.4 | **性能达标**：冷启动 ≤ 1.5s、拖动重绘 ≤ 8ms、平移缩放 ≥ 60fps（5 万对象） | 性能面板数字可信 |
| 5.5 | **打包 + 离线冒烟**（`tools/build.ps1` 已有，补 `--diag` 环境报告） | 归档 `E:\平日资料\构建\VellumBench\{semver}-{short_hash}\` |
| 5.6 | **示例工程 3 个**（落地页 / 简历单页 / 活动海报） | 每个都能完整走一遍任务流 |
| 5.7 | **盲测第 2 轮** | 成功率 ≥ 80% 且 3 项任务耗时比第 1 轮下降 ≥ 30% |
| 5.8 | **README 与手册**：状态表与代码一致；键位速查表；用户手册 | — |

---

### 与既有路线图的关系

| 既有（11 篇） | 本轮（P1–P5） | 说明 |
|---|---|---|
| v0.1 骨架 / v0.2 文档模型 | 已完成（且超出） | 文档模型与往返比 11 篇要求更扎实 |
| v0.3 AI 心智 | **→ P3** | 推迟了，但补齐了前置地基 |
| v0.4 矢量 | **→ P4** | 保持 |
| v0.5 画板导出 | 部分完成（画板有，切片无） | 切片挪到 P5 |
| v0.6 Agent 协议 | **已完成** ✅ | CLI/MCP/patch/监听 都在 |
| v0.7 网页能力 | **→ P3 子项** | 断点/伪类/@media 全部未做，降级为 P3 的一部分 |
| v0.8 导入兼容 | 未做 | 挪到 P5 之后 |
| v0.9 打磨 | **→ P2 + P5** | 拆分：视觉打磨进 P2，工程打磨进 P5 |
| v1.0 发布 | **→ P5** | — |
| **新增** | **P1 地基 · P2 视觉重制** | 11 篇完全没有"UI 重制"这一项，是本轮补上的缺口 |

---

## 7 · 质量门禁与执行纪律

### 7.1 门禁补齐路线

| 门禁 | 现状 | 补齐阶段 |
|---|---|---|
| 1 格式与 Lint（`fmt` + `clippy -D warnings`） | ✅ 有（CI 已跑） | — |
| 2 测试（覆盖率 ≥ 70%，核心 crate ≥ 85%） | ⚠️ CI 有 `cargo test` 但 `tests/` 为空 | **P1** |
| 3 往返语料（100 个 HTML：L0 100% / L1 100%） | ❌ 无 | P1 建 20 个 → P5 到 100 |
| 4 渲染快照（`vello_cpu`，容差 2/255） | ❌ 无 | P5 |
| 5 性能基准（B1–B6，回归 > 10% 阻断） | ❌ 无 | P5 |
| 6 术语与 i18n（禁用表扫描 + 双语 100%） | ❌ `i18n/` 为空 | P5 |
| 7 输出校验（W3C Nu Validator + `prettier --check`） | ❌ 无 | P5 |
| **+8 无硬编码颜色扫描**（本轮新增） | ❌ 无 | **P1 建基线 → P2 清零** |
| **+9 菜单加速键一致性**（本轮新增） | ❌ 无 | **P1**（B2 的回归防护） |

> 门禁 8 和 9 是本轮新增的：**它们是"界面不说谎"的自动化保障**。门禁 9 的做法很直接——解析 `commands.yaml` 的键位集合，断言它是"菜单 label 中出现的键位文本"的超集。

### 7.2 提交信息纪律

**问题**：8 个 commit 全在同一天，最后一条声称了 `v0.7 + v1.4 + v1.0` 三类内容，实际范围远小于此。**虚标版本会污染所有后续判断**——你无法决定"下一步做什么"，如果你不知道"现在到哪了"。

**规则**

1. **commit message 只描述本次改动本身**，不写版本里程碑（里程碑写在 `docs/design/` 里）
2. **禁止在一个 commit 里混多层**：`feat(app,agent,tools): v0.7 + v1.4 + v1.0` 拆成 3 条，各自可回滚
3. **README 状态表与代码同步**：每次改动若影响状态表，同 commit 内更新
4. **提交前自检一句话**："如果有人 checkout 这个 commit 并按 README 操作，会得到什么？"——如果答不出来，说明 message 写虚了

### 7.3 文档与代码同步规则

| 触发 | 动作 |
|---|---|
| 新增命令 / 工具 | 同 commit 更新 `commands.yaml` + 本文件第 4 章 |
| 新增主题令牌 | 同 commit 更新 `theme.rs` + `vb-ui-tokens.json` + 第 3.2 章 |
| 与 AI 行为有意不一致 | 同 commit 写进 02 篇 §六 差异表，**必须写原因** |
| 新增依赖 | 同 commit 在 `docs/deps.md` 记一行（用途/体积/许可/替代） |
| 文档里的 ⚠️待实测 | 有条件时真机核对 AI 并更新（02 篇 §八 首项） |

---

## 8 · 风险登记

| # | 风险 | 概率 | 影响 | 触发信号 | 对策 |
|---|---|---|---|---|---|
| R1 | 视觉重制滑向"无限调色" | 高 | 中 | 连续 3 天无功能提交，只改颜色 | 令牌表冻结（3.2）；改令牌走 ADR；P2 硬性 3 周 |
| R2 | AI 对齐滑向"复刻 AI 全家桶" | 高 | 高 | 开始讨论实时上色/网格渐变/3D | 00 篇"不做清单" + 02 篇 §七 差异表 + 本篇 2.3；触发即引用 |
| R3 | `trait Tool` 重构时间超预期 | 中 | 高 | P3 第 2 周仍无法跑通"选择工具" | 允许折中：先抽 trait 骨架 + 只把"选择/矩形"两个工具迁进去，其余用旧路径过渡 |
| R4 | 字体/图标体积推高安装包 | 低 | 低 | 包体 > 120MB | Inter 只留 3 个字重；MiSans 用子集化（`pyftsubset`）；预算 +2.5MB |
| R5 | `egui` 升级破坏主题 | 中 | 中 | 升 0.36 时视觉回归 | 令牌与主题集中在一个模块（3.8）；升级前跑渲染快照门禁 |
| R6 | 盲测失败（成功率 < 80%） | 中 | 高 | P3 第 1 轮 < 80% | 盲测失败是**最有价值的信息**，不是失败。把卡壳点变成 P4 的任务，做第 2 轮 |
| R7 | 一个人维护 12 个 crate 的认知负担 | 中 | 中 | 改一处要动 5 个 crate | P2 已把 `vb_ui` 建起来；`vb_platform` 若 P5 仍空壳则**合并回 `vb_app`**（不做僵尸 crate） |
| R8 | 中文 IME 输入体验问题 | 中 | 中 | 输入法预编辑文本显示异常 | 3.8 节末尾：显式设 `ImeComposition.legacy_visuals = false` 并实测 |

---

## 附 · 本轮的"一句话版本"

> **把界面做对，把手感做实，把话说真。**
> 能力已经有了大半，就差接出来；皮要换，但骨不许动。
> 每个阶段结束，都要有一条"用户能完整跑通的任务流"和一段能给别人看的录屏。

---

**相关文档**
- [00 需求拷问与产品定义](00-需求拷问与产品定义.md) — 本篇修订其 §四-2（细化为分层适用）
- [02 交互与快捷键规范](02-交互与快捷键规范.md) — 本篇第 4 章是其落地排期
- [03 界面布局与面板规范](03-界面布局与面板规范.md) — 本篇 §3.6 修订其 §一布局图
- [06 工具集与编辑能力](06-工具集与编辑能力.md) — 本篇 §4.2 是其分批落地
- [10 性能预算与质量门禁](10-性能预算与质量门禁.md) — 本篇第 7 章新增门禁 8/9
- [assets/vb-ui-tokens.json](assets/vb-ui-tokens.json) — 设计令牌的可机读版本
