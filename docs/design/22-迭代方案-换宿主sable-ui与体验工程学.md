# 22 · 迭代方案 — 换宿主 sable-ui × 操作工程学 × 功能迭代

> 撰写日:2026-10-04。基线:**v0.14.0**(2026-10-02,733 测试全绿/clippy 零警告)。
> 前序:本篇**部分推翻** [14 篇](14-下一阶段迭代方案-UI重制与AI心智对齐.md) Q5 的"留在 egui"裁定(推翻理由与时点见 §0 Q2);与 sable-ui 仓库 `docs/upstream/00-总结与迁移路线.md`、`02-vellumbench-ui-analysis.md` 互为衔接。15/16/19/20/21 篇(Kiln 与 AI 可编辑性工作线)不在本篇范围,继续独立推进。
> 配套设计令牌真相源不变:`docs/design/assets/vb-ui-tokens.json`。

---

## TL;DR

1. **换宿主**:UI 组件库整体切换到自有项目 **sable-ui v4.0.0**(GPUI 0.2.2 + Vello 0.10 + gpui-component 0.5.1,均 crates.io 锁定)。sable 已被 cutforge 桌面壳生产接入验证(`git rev 72d83c6`),"GPUI 生态不成熟"这个 14 篇 Q5 的否决前提已消失。
2. **迁移方式是"双宿主并行",不是渐进嵌面板**:GPUI 自持 winit 事件循环,与 eframe 不能同进程共存——这是硬结论(§3.2)。新宿主 `vb_shell`+`vb_kit` 与旧宿主 `vb_app`(冻结)并行跑完整 CI,门禁全绿才切默认,R7 删旧宿主。
3. **ACL-1.0 红线**:vb_kit 一律**按规格重写**(规格 = sable `docs/upstream/02` + `vb-ui-tokens.json` + 本篇),实现者不对照 vb_ui `.rs` 写码(§3.5)。
4. **八轮迭代 R0–R7**,每轮可演示、可回滚、有门禁;总预估 4–6 个月(业余 10–15h/周),与 14 篇 Q7 的诚实口径一致。
5. **硬骨头全部立刻排入轮次,零"留后续"**:导出真取消、i18n 全量、键盘焦点体系、贝塞尔拖柄编辑器、4096 纹理上限、画布真文本、vb_platform 实体化、undo 溢出写盘、CRDT GUI 入口、跨实例串行点终判……全部见 §5 硬骨头总表,每条有归属轮次与完成定义。
6. **操作工程学**:动效四档(INSTANT/HOVER 80ms/STATE 120ms/PANEL 200ms)+ 弹簧档、命令面板 `Mod+K`、上下文浮动工具条、吸附间距气泡、属性字段原位 token 绑定、混合选中态——全部落到控件级规格(§4/§6)。

---

## 0 · 自我拷问(grill-me):先回答 9 个硬问题

### Q1 · 现在真实站在哪一步?

v0.14.0:733 测试全绿;双车道导出(Kiln)九格式;204 条命令单源;深浅双主题 + WCAG AA 门禁;自动保存/崩溃恢复/undo 200MB 上限/多窗口/工作区布局 v2 均已落地(见 §1.2 已还清单)。UI 层欠的债集中在:**框架天花板**(egui 无自定义光标图、无合成层、4096 截图纹理、画布文本近似)、**交互完备度**(命令面板未做、上下文工具条未做、贝塞尔只能预览)、**i18n**(ftl 仅 ~15 key,全仓中文硬编码)、**焦点体系**(仅 active 描边,Tab 遍历不完整)。

### Q2 · 为什么现在推翻 14 篇 Q5"留在 egui"的裁定?

14 篇 Q5 否决 GPUI 的三个前提——**无稳定发布、生态小、文档少**——在 2026-10 已全部变化:

| 14 篇时点(2026-09 中) | 现在(2026-10-04) |
|---|---|
| GPUI 无稳定发布 | gpui 0.2.2 **crates.io 正式版**,gpui-component 0.5.1 为最后绑定 gpui ^0.2.2 的稳定线 |
| 生态小、无生产案例 | **cutforge 桌面壳已用 sable 全套(dock/widgets/video/theme)生产接入并多轮迭代**(rev 72d83c6 起);sable-ui 自身 v4.0.0,11 crate 全有测试 |
| egui 上限够用(Rerun 证明) | egui 天花板实际咬人:自定义光标图不支持(旋转光标退化)、无合成层(玻璃/阴影/真动画)、画布文本只能近似(X-2)、截图纹理 4096 上限高分屏发糊、焦点体系要全手搓 |

同时换宿主的**红利清单**是实打实的:sable-canvas 的 parley+skrifa **真文本管线**直接偿还 X-2;GPUI/winit 支持自定义光标图(补回 AI 弯曲双箭头);sable 动画引擎三层(Tween/Spring/关键帧)让"有动画"成为默认而不是逐个手搓;窗口效果 Mica/Acrylic 白送。

**代价也说清楚**:vb_app 63 个文件用 egui、29+ 面板是主工程量(sable 02 分册 §5 估算),这是一次以月计的迁移,不是一周的换库。所以必须有 §3.2 的双宿主策略兜底。

### Q3 · GPUI 成熟度风险怎么兜底?

三层兜底:①**双宿主并行**——旧宿主冻结在 zh、只修 P0,任何一轮不绿都随时 `--host=egui` 回退;②**版本锁定**——gpui-component 锁 0.5.1(0.6+ 迁 gpui-pre 0.3,类型世界不兼容),升级走 sable TD-01 的"升级窗口",不在本工程内追新;③**R0 画布上屏 spike 前置**——vello Scene→GPUI 窗口的上屏通道是最大技术未知数,R0 就做三条路线对比 spike 并落 ADR-0046,不允许带着未知数进 R1。

### Q4 · ACL-1.0 红线怎么守?

见 §3.5 操作规程。一句话:**迁移方向是"VellumBench 按 sable 规格重写面板层",不是"把 vb_ui 搬进 sable"**; sable 保持 MIT/Apache 双许可纯净,VB 专属控件全部落在本仓 `vb_kit`。

### Q5 · 过渡期功能开发怎么办?

旧宿主进入**冻结模式**:只修崩溃/P0 正确性,不接新功能、不做新面板。所有新功能(命令面板、渲染队列、Agent 幽灵 diff……)一律落新宿主。两条纪律:新命令照旧先登记 `commands.yaml`(新旧宿主共用命令单源,可逆性门禁不放假);新面板只出现在 vb_kit。

### Q6 · 为什么不能"一个面板一个面板"渐进换?

GPUI 与 eframe 各自拥有 winit 事件循环与窗口,**不能同进程共存**(sable 02 分册 §6 结论)。唯一渐进是"进程级":新宿主独立二进制,共享全部非 UI crate(vb_doc/vb_kiln/vb_render/vb_session)。所以 §3.1 才要先提取 `vb_session`——那是渐进的唯一载体。

### Q7 · 工期诚实版?

R0–R7 全走完约 **4–6 个月**(业余 10–15h/周;多子代理 worktree 并行可压缩,dev-loop 流水线已多轮验证)。铁律不变:**每轮结束都是一个能跑、能演示、能回滚的软件**。

### Q8 · "不把问题留到明天"怎么体现?

§5 硬骨头总表:**24 项,每项都有归属轮次、完成定义、门禁**;本篇正文不出现"留后续""以后再做"。若某项在执行中确需变更归属,必须改本篇并说明,不许口头漂移。

### Q9 · 本轮的"不做"清单(新增 + 沿用)?

沿用 14 篇 9 条禁拿与 DROP-1/2;新增见 §2.4。特别新增:节点图文档模型(破坏 CSS 单一真相)、Rive 式状态机(无标准 CSS 落点)、内置生成式 AI 与云服务(AI 能力 = 本机 Agent 走 CLI/MCP)、多人实时协作光标(协同 = CRDT 合并会话,无云)。另有一项**翻案**:14 篇 F8"面板禁止浮动"是 egui 自研 dock 时代的防御性裁定;sable-dock 原生浮动 + 布局序列化,本篇改为**"默认停靠、允许浮动、窄窗强制折叠规则保留"**(§3.4 裁决表)。

---

## 1 · 现状体检(v0.14.0 代码级)

### 1.1 UI 架构现状

| 层 | 现状 | 规模 |
|---|---|---|
| 宿主 | eframe/egui 0.35(wgpu feature)+ vello 0.10 + wgpu 29 | — |
| `vb_ui` | 纯无状态组件库:theme/components/gradient/icons/fonts/expr/toast/motion/cursor/dock,不依赖 vb_app/vb_doc | 12 文件 5,378 行 |
| `vb_app` | 全部会话态 + 29+ 面板 + 命令分发 + 多窗口(viewport)+ 启动器 | ≈31,000 行(根 7,010 + app/ 21,955 + shortcuts/ 2,080) |
| 画布 | vb_render DrawList → vello::Scene(wgpu);文本/冻结块 egui 覆盖层近似(X-2) | 2,731 行 |
| 命令 | `commands.yaml`(204 条单源)↔ shortcuts/catalog+binds+menus;run_command 单入口 | — |
| 门禁 | fmt→clippy→test→selfcheck→术语→L1 幂等→硬编码色扫描→示例体检→canvas parity(GPU)→许可 | tools/ci.ps1 |

### 1.2 已还清的债(不要重复立项)

自动保存(autosave.rs 580,.vb-autosave 滚动 3 份)+ 崩溃恢复(recover.rs 434,差异视图+恢复对话框);undo 200MB 上限(vb_doc/undo.rs,command_bytes 估算+栈底丢弃,有测试);焦点描边 `theme::stroke::FOCUS` 已用于 active 控件;输入上下文栈 `InputContext`;`Mod+D` 再次变换、8 手柄+角外圈旋转、智能参考线、对齐关键对象——四项 Illustrator 心智功能已落地;工作区布局 workspace.json v2;启动器(缩略图缓存+搜索+键盘导航)。

### 1.3 未还的债(索引,详表见 §5)

i18n 全量(ftl 仅 ~15 key)、键盘焦点 Tab 遍历不完整(ToolButton/PanelTabs/icon_button/ColorField 等不可达)、导出不可真取消(Kiln 无中途取消接口)、时间轴贝塞尔只有预览无拖柄、画布截图纹理 min(4096) 高分屏发糊、画布文本近似(X-2 Partial)、状态栏窄窗溢出、vb_platform 仅 1 行注释、undo 溢出不写盘、CRDT 会话层全绿但 GUI 无入口、07-L 图层两条菜单项缺命令支撑、X-5 曲率/铅笔/形状生成器未做、WebCodecs 阶段 2 正确性自验、音频导出、GIF 流式化、跨实例串行点终判(等下游贴 warnings"实例 N 统计"数据)。

### 1.4 能力台账现状

Partial(6):09-H 符号/组件、09-K CRDT/协同/Web、07-L 图层富交互、X-2 画布真文本、X-3 SVG/PDF 导入边界、X-5 曲率/铅笔/形状生成器。Dropped(2):DROP-1(实时上色/网格/描摹/3D/透视)、DROP-2(云端工程/账号/市场)。门禁:Dropped≤2、Planned=0。**本篇所有新能力条目入账规则见 §10。**

---

## 2 · 竞品调研结论(2026-10)

> 调研对象:Figma(含 Motion/Variables/MCP)、Penpot 2.15–2.17、tldraw/excalidraw、Graphite、Rive/Jitter/LottieFiles、Remotion/Motion Canvas、Illustrator 2024–2026、Affinity、Cursor/v0/Bolt 的 Agent 工作流。来源清单见 §11。

### 2.1 Top 12 借鉴清单(按适配度排序)

| # | 借鉴点 | 来源 | 落点(本篇) |
|---|---|---|---|
| 1 | **属性字段原位 token/变量绑定**(字段右侧"变量点"→弹绑定列表,而非跳去面板) | Figma Variables / Penpot 2.16 | R2 · CSS 自定义属性天然映射 |
| 2 | **数字输入三件套**:标签拖改值、算式输入、混合选中态横线 | Figma / Penpot 2.16 | R2(scrubby+expr 已有,补混合态与标签拖改) |
| 3 | **Agent 幽灵 diff + 逐对象接受/拒绝** | tldraw agent kit / Cursor / v0 | R6 · HTML diff 现成 |
| 4 | **MCP 读写双向 + 连接状态 UI** | Figma MCP / Penpot MCP | R6 |
| 5 | **时间轴可停靠面板 + 画布联动**,不做成独立模态 | Figma Motion | R4 |
| 6 | **图层↔结构双视图同一文档** | Graphite | 不做(§2.4);精神上由"外观多条目=声明列表"承载 |
| 7 | **选中上下文浮动工具条/任务条**(就地预测下一步) | Illustrator CTB / tldraw | R3 |
| 8 | **吸附对齐线 + 间距测量气泡 + Alt 旁路** | tldraw | R3(snap.rs 升级) |
| 9 | **渲染引擎可切换、预览=导出一致** | Penpot 2.17 / Graphite | R5 · 双车道引擎标签入 UI |
| 10 | **缓动曲线小窗 + 自定义缓动库;命名动画复用** | Rive / Jitter | R4 · 落点 = CSS easing + 共享 @keyframes |
| 11 | **渲染队列:分片并行、进度/重试/失败原因/取消** | Remotion Studio | R5 |
| 12 | **历史快照只读预览 + 时间戳** | Penpot 2.17 | R2 · autosave 差异视图升级 |

### 2.2 可以拿的(在 14 篇 12 条之上新增 9 条)

13. **属性字段变量绑定点**(上表 1);14. **混合选中态**(多选不同值显示"混合"横线,编辑即批量应用);15. **间距气泡 + Alt 临时禁吸**(上表 8);16. **空状态设计**(无选区时属性面板显示画板属性与引导,不许白板);17. **渲染队列 UI**(上表 11);18. **自定义缓动存进文档**(R4,落点 `:root` 内 `--vb-ease-*` 注释块或内联 `cubic-bezier`);19. **Agent 活动指示**(状态栏徽章:"Agent 正在编辑…",rev 冲突时锁写);20. **tooltip 一律「名称 (快捷键)」**(规格已有 key_badge_text,新宿主全量接线);21. **窗口材质**(Mica/Acrylic,Win11 22621+,失败静默降级——sable window_effects 白送)。

### 2.3 绝对不许拿的(在 14 篇 9 条之上新增)

| # | 行为 | 裁决 | 原因 |
|---|---|---|---|
| F10 | Figma AI 生成式功能内建(Text to Vector/Generative Recolor 云端) | ❌ | DROP-2 沿用;同类能力由本机 Agent 经 CLI/MCP 完成,软件不内置云 |
| F11 | Rive 状态机(driver+state+transition) | ❌ | 无法落为标准 `@keyframes`,违反"HTML 落点"第一原则;动效交互性交给孩子页面脚本/冻结块 |
| F12 | Graphite 节点式文档模型 | ❌ | 文档唯一真相是 CSS;节点图会制造第二真相 |
| F13 | tldraw "完全包含才选中" | ❌ | F1 已裁:相交即选中(AI 语义) |
| F14 | Penpot/Figma 多人实时协作光标 | ❌ | 协同 = CRDT 合并会话(ADR-0031),不做云服务与实时通道 |
| F15 | Figma Motion "动画定义不落 CSS" 的私有层 | ❌ | 关键帧真相必须是文档里的 `@keyframes vb-anim-<sid>`(ADR-0043 现状正确) |

### 2.4 三层模型仍有效(老师不串门)

14 篇 §2.1 的三层(视觉学 Figma / 交互学 Illustrator / 语义守 HTML/CSS)与 §2.4 冲突裁决表**原样沿用**,本篇所有迭代不得越层。换宿主只换第 1 层的实现技术与第 2 层的可达性(焦点/命令面板),第 2 层语义与第 3 层一字不改。

---

## 3 · 换宿主总案

### 3.1 目标架构与 crate 切分

```
┌────────────────────────────────────────────────────────────────┐
│ vb_shell(新,GPUI 宿主)                                        │
│   Application::new().run / open_window;启动器窗口;项目多窗口;   │
│   工作台组装(sable-dock WorkspacePresets);窗口材质;全局快捷键   │
├────────────────────────────────────────────────────────────────┤
│ vb_kit(新,sable 面板与控件)                                    │
│   29+ 面板(按 sable 规格重写);VB 专属控件(取色器双档/渐变条/    │
│   外观条目/时间轴扩展/能力台账);t() 文案;动画接线                │
│   依赖:sable(widgets/dock/foundation)+ vb_session + vb_common   │
├────────────────────────────────────────────────────────────────┤
│ vb_session(从 vb_app 提取,宿主无关)                            │
│   选中态/工具状态机/修饰键/吸附引擎/覆盖层(参考线/徽章)/          │
│   命令分发(run_command 适配)/导出任务服务/自动保存调度/           │
│   撤销栈门面/工作区布局状态 —— 全部纯数据 + 纯函数,零 UI 依赖     │
├────────────────────────────────────────────────────────────────┤
│ vb_doc / vb_render / vb_kiln / vb_export / vb_browser /         │
│ vb_agent / vb_plugin / vb_platform(本篇实体化)  —— 不动或后置     │
├────────────────────────────────────────────────────────────────┤
│ vb_app(冻结,egui 旧宿主)+ vb_ui(冻结)                          │
│   只修 P0 崩溃;R7 删除                                          │
└────────────────────────────────────────────────────────────────┘
```

**依赖方向(硬规则,进 CI)**:`vb_doc ← vb_session ← vb_kit ← vb_shell`;vb_kit/vb_shell 禁止 import vb_doc 内部类型(一切经 vb_session 投影)——这是对现状"面板摸 VellumApp 私有字段"耦合模式的总清算,也是本工程最重要的解耦交付。

### 3.2 双宿主并行策略与回滚

1. 新宿主独立二进制 `vellum-sable.exe`(旧 `vellum.exe` 不动),`--host=sable|egui` 仅是开发期便利参数,发布包 R7 前含两者、R7 后只含 sable。
2. CI 门禁双跑:既有全套(fmt/clippy/test/L1/parity/术语/色扫描)对两个宿主各跑一遍;ui_shots 双基线。
3. **切换判据(G-SWITCH)**:新宿主在 §4 各轮验收门全绿 + canvas parity 不低于旧宿主分数 + §8 性能预算达标,连续 3 天无 P0,才允许 `vb_app` 默认化;R7 删除前旧宿主保留在 git 历史即可回退。
4. 冻结纪律:vb_app/vb_ui 冻结期改动必须在本篇 §5 表中登记为 P0 修复,否则不许合入。

### 3.3 壳不持真相(对齐 sable 00 分册 §4.3 的 check-shell-purity 纪律)

- 命令单源:`commands.yaml` 是唯一命令表;vb_shell/vb_kit 里出现的每个菜单项/按钮/手势都必须解析到命令 ID(G-UI6 门禁);CLI/MCP/GUI 三端同源不变。
- 吸附/几何/动画求值不允许在 vb_kit 里手写第二份——吸附引擎在 vb_session(从 snap.rs 提取的纯函数),动画求值与导出同一路径(vb_kiln 现状保持)。
- 用户内容与软件外观隔离:`--vb-ui-*` 与 `--vb-brand-*` 命名空间禁止互引(theme.rs 既有纪律平移到 token 消费端)。

### 3.4 令牌与两项裁决

**映射表**:vb-ui-tokens.json ↔ sable `ColorTokens/SpacingTokens/RadiusTokens` 的逐项对照已在 sable `docs/upstream/02 §3` 完成(bg-canvas→surface_0 等),本工程不另造表;落地方式 = `SableTheme::inject(vb 调色板)`,JSON 仍是唯一真相。新增门禁 **G-UI1 tokens_sync2**:inject 后的语义色与 JSON 逐值比对进 CI(机制照搬 theme.rs tokens_sync)。

**裁决一 · 控件高度**(sable 02 分册 §3.4 悬案,本篇拍板):**名义档 22/26/32 保留;一切承载文字的控件高度走派生函数 = 当前字号实测行高 + 8,下限 24**——两上游经验合并,CJK 不压叠,派生公式进 G-UI8 门禁测试。

**裁决二 · 浮动面板**(翻 14 篇 F8):默认全部停靠;允许拖出为浮动窗(DockArea 原生);**窄窗 <1200px 强制折叠为 40px 图标条**的规则(dock.rs 纯函数语义)在新宿主保留并复测;布局序列化走 DockAreaState + workspace.json **v3**(带 v2 迁移函数与坏文件回退默认)。

### 3.5 ACL-1.0 合规操作规程(每次 PR 自查)

1. vb_kit/vb_shell 实现者只读三份输入:sable `docs/upstream/02`(语义转述)、`docs/design/assets/vb-ui-tokens.json`(客观数值)、本篇;**不打开 vb_ui `.rs` 对照写码,不做逐行 API 改名翻译**。
2. 行为规格(scrubby 公式、Alt 优先于 Shift、toast TTL 2.5/5/10s、错误可复制、dock 折叠表)是功能性事实,允许对齐;代码组织不允许复制。
3. PR 模板加勾选项"本变更未对照 VellumBench/Sable 之外的受限源码书写";sable 侧 NOTICE.md 条目保持更新。
4. 图标:GPUI 侧用 Lucide **SVG** 直渲(替代 iconflow 字体),保留"语义名枚举 + all_icons_resolve 式可解析门禁"纪律;Inter/MiSans/JetBrains Mono 许可登记平移进本仓 docs/deps.md。

### 3.6 版本锁定(升级窗口纪律,TD-01)

| 依赖 | 锁定 | 说明 |
|---|---|---|
| gpui | 0.2.2(crates.io) | 0.3-pre 类型世界不兼容,不追 |
| gpui-component | **0.5.1** | 最后绑 gpui ^0.2.2 的版本;升级须等 sable 统一走升级窗口 |
| vello / wgpu | 0.10 / 29 | 与 vb_render 现状一致,画布通道 spike 的前提 |
| sable | git rev 锁定(同 cutforge 模式) | 本仓升级 sable = 显式提交,带 diff 审查 |

### 3.7 vb_platform 实体化(硬骨头 #13,落 R0)

现状仅 1 行注释。R0 落地为最小实体:窗口句柄/剪贴板/文件对话框/系统光标/每显示器 DPI/深色模式探测 六个 trait,vb_app(egui)与 vb_shell(GPUI)各给一个实现。目的:让 vb_session 与 vb_kit 里不再出现任何 `std::path` 之外的 OS 直呼,Windows 特有逻辑(深色标题栏、Mica、感知 DPI)集中一处。完成定义:vb_kit 全仓 grep 无 `windows::`/`winit::` 直引。

---

## 4 · 迭代路线(R0–R7)

> 每轮固定结构:目标 / 前置 / 交付(UI 规格 · 交互规格 · 动画规格)/ 硬骨头 / 鲁棒性与耦合 / 门禁 / 回滚 / 预估。
> 铁律:每轮结束可演示、可回滚;门禁必须真实运行并引用退出码。
> 并行建议:标注 ⟳ 的交付可拆独立 worktree 子代理(受 §3.1 依赖方向约束)。

### R0 · 双宿主脚手架 + 四项硬地基(预估 1.5–2 周)

**目标**:新宿主能开窗、能打开示例工程、画布出图、命令可执行;四项与 UI 无关的硬骨头同时落地。

**交付**
1. `vb_shell`:GPUI Application 入口、单项目窗口、标题/图标/深色标题栏;启动器窗口(先只读 MRU 列表,复用启动器纯逻辑)。
2. `vb_kit` 骨架:SableTheme 注入(vb 调色板)+ tokens_sync2 门禁;h_flex/v_flex 布局件;一个示范面板(能力台账,79 行逻辑最薄)走通"面板=纯投影"模式。
3. **⟳ 画布上屏通道 spike(ADR-0046)**:三条路实测——(a) 现有 vb_render wgpu 管线离屏渲染→纹理上传 GPUI;(b) vb_render DrawList→sable-paint PaintSink→sable gpui_element 直绘;(c) GPUI 原生 paint path 翻译层。判据:1080p 下 1 万节点 60fps、缩放无糊、内存峰值。**R0 结束必须裁定,不许带着未知数进 R1。**
4. `vb_session` 提取第一批:选中态、工具状态机、命令分发适配(run_command 原样搬家)。
5. **硬骨头 #6 导出真取消(Kiln 侧)**:导出任务句柄化 + 协作式取消(分段边界检查 `cancel_flag`,浏览器车道每帧后检查,native 车道每分段后检查;`kiln-cli` 同步获得 `--cancel-token` 语义=stdin 'c' 或信号)。**UI 无关,先落核心。**
6. **硬骨头 #18 跨实例串行点终判**:kiln 报告新增逐实例帧间隔直方图(不改 warnings 兼容字段),用数据终判 GPU 读回串行 vs CPU 饱和;结论写进 ADR-0047 备忘。
7. i18n 地基:Fluent 目录结构定稿(`i18n/zh.ftl`/`en.ftl` + `vb_session::i18n::t()`),命令标签 204 条全部入 catalog;新宿主从第一行代码起禁裸文案(G-UI3)。

**门禁**:既有全套对 vb_shell/vb_kit 生效;G-UI1/3/7 首次运行;spike 结论入 ADR。
**回滚**:删除两个新 crate 即回到 0.14.0,vb_session 提取以 re-export 保持 vb_app 编译。

### R1 · 画布换血(预估 2–3 周)

**目标**:画布交互达到并超过旧宿主,偿还两大 Partial(X-2 真文本、4096 上限)。

**交付**
1. 按 R0 裁定的通道渲染 vb_render DrawList;缩放/平移/框选/手柄/旋转全部走 vb_session 状态机(GPUI 事件→状态机→命令)。
2. **画布真文本(X-2 收口)**:文本与冻结块经 sable-canvas 文本管线(parley 布局 + skrifa 字形轮廓)绘制,删除 egui 近似覆盖层与常驻"近似渲染"提示;与 CPU 导出的 parity 门禁加文本用例。⟳
3. **4096 上限破除(硬骨头 #7)**:GPU 矢量路径下交互缩放无纹理上限;若 R0 裁定含离屏纹理,则实现平铺渲染(tiling)替代整幅截图纹理。
4. **自定义光标回归**:工具/手柄→光标映射表升级为图像光标(AI 弯曲双箭头旋转光标、缩放双向箭头、抓取手型),GPUI/winit 原生支持,`cursor.rs` 映射表语义平移进 vb_session。
5. 选中/悬停/框选/智能参考线呈现规格:选中框 #0D99FF 2px + 8 手柄 8×8 白底蓝边;悬停轮廓 accent 60%;品红参考线 fade-in 80ms;框选填充 alpha 24。
6. 铅笔/钢笔路径编辑强化(锚点+手柄直接拖拽,为 X-5 铺路;X-5 全量在 R6)。

**动画规格**:画布一切几何变化 INSTANT(零动画,14 篇铁律);参考线/徽章是仅有的画布动画(80ms fade)。
**鲁棒性**:GPU 设备丢失接入 sable-paint GpuGuard(generation 失效全部缓存,`render_with_recovery` 重试一次,二次失败致命弹窗+自动保存兜底)——硬骨头 #11。
**门禁**:canvas parity 文本用例;光标映射表测试;GpuGuard 注入测试(模拟 device lost)。

### R2 · 面板全家福(预估 3–4 周,最大轮)

**目标**:29+ 面板按规格重写完成,交互质量全面超过旧宿主。

**迁移顺序表**(每批合入即过 G-UI6/7 门禁):

| 批 | 面板 | 备注 |
|---|---|---|
| 1 | 图层(1,735 行:拖拽重排/Alt 复制/右键菜单/F2 改名/Esc 取消)+ 属性(916) | sable LayerPanel(FLIP)+ InspectorPanel(RowSpec 声明式)为底 |
| 2 | 外观(2,017:多条目/排序/禁用)+ 渐变(474)+ 颜色(718)+ 透明度(266) | 渐变条按 gradient.rs 纯函数规格重写(几何/命中全纯函数);ColorField 双档取色器(点击紧凑/Alt+点击完整) |
| 3 | 画板(782)/令牌(71→升级)/资产(574)/历史(416) | 令牌面板升级为 token 管理中枢(配合 #1 绑定点) |
| 4 | 字符段落(charpara)/变换(744)/对齐(323)/控制面板(1,999,spec 驱动) | 控制面板是"上下文条",规格见 R3 |
| 5 | 时间轴(1,652,只迁移壳与轨道 UI,编辑器强化在 R4)/插件(639)/健康检查(1,114)/校对(535)/能力台账(升级)/开发者统计 | — |
| 6 | 启动器全功能(892:缩略图/搜索/↑↓ 导航/恢复会话)+ 新建项目(676)+ 工作区对话框(298)+ 多窗口(一项目一窗,pending_focus 语义平移)+ 状态栏(重排 + 窄窗溢出修复,硬骨头 #14) | — |

**新交互(本轮核心增量)**
1. **属性字段原位 token 绑定**:数值/颜色字段右侧"变量点"图标,点击弹文档 CSS 变量绑定列表(`--vb-brand-*` 等,含搜索与"新建变量");已绑定字段显示变量名而非裸值。落点 = 字段样式写入 `var(--…)`,纯 CSS,零私货。⟳
2. **混合选中态**:多选异值字段显示"混合"斜纹占位;点击清空→输入即批量应用;Esc 恢复显示原混合值。
3. **空状态设计**:无选区时属性面板显示画板属性 + 下一步引导(双击进入文字/按 V 选择),不许白板。
4. **历史快照只读预览**:恢复对话框在差异视图上加渲染缩略图(autosave 快照离屏小图)。
5. **数字输入三件套补全**:scrubby(已有纯函数规格)+ 表达式(expr.rs 纯函数直接复用,零改)+ **标签拖改值**(label 左右拖 = scrubby 同路径,光标 col-resize)+ 混合态 + undo 会话信号(scrub_started/ended/focus_lost 合并一次撤销,已有)。

**动画规格**:面板折叠高度 tween 200ms(EASE_STANDARD);Tab 切换下划线滑动 200ms;图层重排 FLIP 200ms(FlipTracker);字段 hover 底色 80ms;面板入场位移 120ms(一次性,motion 语义)。
**门禁**:G-UI3(t() 全覆盖)、G-UI6(每条目解析命令 ID)、G-UI7(壳纯度)、ui_shots 每面板基线、既有 numfield_commit/workspace_persist 等测试平移。

### R3 · 操作工程学冲刺(预估 2–3 周)

**目标**:把"美观、动画、交互"从组件级提升到全局级;焦点体系全量收口。

**交付**
1. **命令面板 `Mod+K`**(14 篇 #8 兑现):全 204 条命令模糊搜索(拼音首字母容错),`key_badge_text` 统一「名称 (键位)」;最近使用置顶;执行后原地保留;从 `commands.yaml` 生成,零第二真相。下滑 120ms + 背板 80ms 淡入,Esc 100ms 退出。⟳
2. **上下文浮动工具条**:选区上方 40px 高浮动条(工具箱同源图标),内容随工具/选区变化:选择工具→填充/描边/不透明度/对齐/编组;文字→字号/字重/对齐。按 `.` 收起。与右键菜单内容一致(双入口,tldraw 范式)。
3. **吸附升级**:品红对齐线(已有)+ **间距测量气泡**(拖动中显示与相邻对象的等距间距,px)+ **Alt 临时禁吸**(释放恢复)+ 等尺寸/等距参考线(snap.rs 已有前两类,补气泡与 Alt)。气泡为实时数值无动画(出现 80ms fade)。
4. **智能尺寸/位置徽章**:拖动/缩放时手柄旁实时 w×h 徽章(Penpot 旋转徽章范式:徽章随对象旋转)。
5. **键盘焦点体系全量(硬骨头 #4)**:全部可交互控件入 Tab 序(GPUI focus handle 全接线);焦点环 `border-strong` 1.5px 全局可见;`F6` 循环面板区、`Ctrl+F6` 反向;`Esc` 逐级退出(浮层→面板→画布);无障碍标签(TD-04:每个控件 with_label 语义名,为读屏预留)。**焦点走查测试(G-UI5):遍历所有面板断言 Tab 可达数 = 可交互控件数。**⟳
6. **快捷键对话框升级**:冲突检测(已有)+ 按命令/按键双视图 + 录制式改键 + 与 keymap.json 用户覆盖层持久化(语义不变)。
7. **动效总开关跨宿主迁移**:view.toggle_motion 设置值平移,reduced-motion 系统设置探测接入(vb_platform),关闭后一切动画直通(G-UI10)。

**门禁**:G-UI5 焦点走查;G-UI10 动效归零;命令面板首帧 <50ms 实测入 CI(dev stats);术语扫描扩展到浮动条/气泡文案。

### R4 · 动画工作台(预估 2–3 周)

**目标**:时间轴从"能看"到"专业",偿还最大的单点硬骨头(贝塞尔拖柄)。

**交付**
1. **贝塞尔缓动拖柄编辑器(硬骨头 #5)**:双击关键帧弹曲线小窗(300×300,网格 + 预置曲线列表);两拖柄直接拖拽,实时回显 `cubic-bezier(x1,y1,x2,y2)` 数值;y 轴钳制 [0,1](CSS 合法域),x 轴钳制 [0,1];拖柄命中区 ≥12px(操作工程学下限)。落点:每关键帧 easing 已存 `@keyframes` 内,零格式变更。⟳
2. **缓动预设库**:linear/ease/ease-in-out/标准四档 + **自定义缓动保存进文档**(`:root` 内 `--vb-ease-<name>` 注释锚定块,`var()` 引用,仍是标准 CSS);预设列表支持应用/另存/删除。
3. **关键帧多选与批量操作**:Shift/Ctrl 点选、框选、拖动整组(吸附到整帧)、Alt+拖复制、右键菜单(删除/缓动统一/时间平移);多选拖动时显示总位移提示。
4. **命名动画组件(Jitter 范式,落标准 CSS)**:把一组关键帧定义为命名动画(存共享 `@keyframes vb-anim-<name>` + 工具类),对象"应用动画"= 挂类;改主件全局更新。能力台账入账 09-H 扩展条目。
5. **播放预览增强**:循环区间(I/O 点)、逐帧步进(`,`/`.`)、播放头 scrub 即时求值(与导出同路径已有)、播放时隐藏手柄与参考线(自动,停止恢复)。
6. **画布联动**(Figma Motion 范式):拖动画布对象时,若该属性有关键帧且播放头在某关键帧上,自动更新该关键帧值(而非插入新帧——AI 语义,弹一次提示可记住选择)。
7. 时间轴视图换 sable TimelineView 底座:时间码/刻度/缩放(捏合 + `Ctrl+滚轮`)、轨道行高 24 派生制、关键帧菱形 8px 命中 12px。

**鲁棒性**:关键帧时间值全部走 f64 毫秒,序列化回归测试(字节幂等沿用 L1 纪律);播放求值与导出求值共享单一函数(壳不持真相)。
**门禁**:贝塞尔编辑器纯函数(求值/反求/钳制)单测 ≥20;缓动库序列化幂等;`@keyframes` 落盘与浏览器车道 parity 抽帧比对。

### R5 · 导出与渲染队列(预估 2 周)

**目标**:导出从"一次性对话框"升级为"队列工作台",并兑现双车道阶段目标。

**交付**
1. **渲染队列 UI**(Remotion 范式):队列面板(次级坞)列出任务:目标格式/画板/进度(帧级)/耗时/ETA;**取消按钮接 R0 的 Kiln 取消句柄(硬骨头 #3 收口:UI 侧)**;失败显示原因与"重试";完成项可"在文件夹显示"。进度 pill 不确定态 1.2s 循环,确定态平滑推进(禁跳变)。⟳
2. **批量导出**:多画板 × 多格式矩阵勾选,一次入队;导出预设(命名保存:尺寸/倍率/格式/背景)。
3. **引擎标签**:导出对话框与队列显示实际使用的车道(浏览器 B / 自研 K / WebCodecs)与降级警告(K 车道必带,现状语义)。
4. **WebCodecs 阶段 2 正确性自验(硬骨头 #16)**:解码回读 vs 截图基线逐帧比对工具(阈值入 selfcheck);Windows 硬编 flaky 兜底(#748:编码器创建失败自动降截图车道并告警,不静默)。
5. **音频导出**(硬骨头 #17a):时间轴挂音频(Assume 现文档模型无音频轨——新增 `vb-anim-audio` 声明区,MP4 mux AAC;GIF 天然无声诚实提示)。
6. **GIF 流式化**(硬骨头 #17b):内存车道全帧调色板改为流式帧写入,长动画内存上限从 O(帧数) 降为 O(窗口)。
7. 导出后台线程纪律保持(ExportJob/poll 语义平移到队列模型,UI 线程零阻塞,poll 节流 100ms)。

**门禁**:取消延迟 <1s 实测(任意车道任意阶段);队列崩溃恢复(进程重启后队列状态从 .vb-autosave/ 邻接目录重建);音频导出 selfcheck 用例。

### R6 · Agent 协同表面 + 工具完备性(预估 2–3 周)

**目标**:"Agent 是一等公民"从协议层浮出到 UI 层;X-5 与 07-L 收口。

**交付**
1. **Agent 幽灵 diff(Cursor/v0 范式)**:Agent patch(MCP/CLI)落库前,变更对象以幽灵样式(accent 40% 描边 + 斜纹)叠加画布,弹出"变更 N 处"审查条:逐对象接受/拒绝/全部接受;拒绝 = patch 事务回滚(rev 乐观锁已有,零新协议)。diff 源 = vb_doc patch 前后场景 diff(HTML diff 兜底)。⟳
2. **Agent 活动指示**:状态栏徽章"Agent 正在编辑…"(最近 3s 有写操作时显示);冲突时 UI 侧输入锁写并提示(写冲突由 rev 拒绝,UI 诚实呈现)。
3. **MCP 连接状态 UI**:设置页显示 MCP 服务状态/端点/最近调用列表(本地隐私,不外发);一键复制连接串。
4. **CRDT 协同 GUI 入口(硬骨头 #15)**:文件 → 协同会话 → 启动/加入(本机多开 + 局域网文件共享模式,无云);协同面板显示参与端列表与冲突解决队列(LWW 已有);落盘仍是 canonical HTML(纪律不变)。入口最小可用即可,协议层已全绿。
5. **能力台账 UI 升级**:三态徽章(Done/Partial+去向/Dropped)+ "Agent 可复现"过滤列 + 从台账直达对应命令。
6. **工具完备性(X-5 + 07-L,硬骨头 #21/22)**:曲率工具、铅笔平滑(落 R1 铺好的路径编辑)、形状生成器(路径求并/差/交,落点 = CSS clip-path 或冻结块,诚实标注可编辑边界);补齐图层右键"单节点导出/剪切蒙版"两条命令(commands.yaml 登记 + 倍率存储位定义)。
7. **符号跨文档复用(09-H 收口,硬骨头 #20)**:主件库面板支持导出/导入 `.vb-symbol`(JSON 片段,`vb-symbol-defs` 命名空间),粘贴时实例化并可选"重链主件"。

**门禁**:幽灵 diff 的接受/拒绝可逆性测试(拒绝后 rev 与字节双还原);CRDT 双端合并回归(已有测试扩 UI 入口冒烟);新命令全部过可逆性门禁。

### R7 · 清账、i18n 全量与切默认(预估 2 周)

**目标**:删旧宿主,新宿主转正;最后一颗硬骨头(i18n)收口。

**交付**
1. **i18n 全量(硬骨头 #2)**:vb_kit/vb_shell 全部界面文案入 Fluent catalog(命令 204 + 菜单/面板/对话框/气泡,预估 ~600 key);en.ftl 全量翻译(术语表 CONTEXT.md + 禁用表为唯一词汇真相,`check_terminology.py` 扩展双语扫描);`view.language` 设置即改即生效(Fluent 运行时切换,无需重启)。⟳
2. **G-SWITCH 执行**:§3.2 判据复核 → sable 宿主设为默认 → **删除 vb_app/vb_ui/vb_shell 的 egui 分支与 `--host` 参数**(0.14.0 删 LayerRow 同款"删死代码"纪律);dist 打包单宿主。
3. **undo 溢出写盘(硬骨头 #12)**:超 200MB 的栈底命令序列化到 `.vbdoc/history/`(滑动窗口 50 步),历史面板可跳转已溢出条目(只读重放)。
4. **无障碍与观感终审**:TD-04 清单过一遍;ui_shots 全面板深浅双主题人工走查(走查表入 PR);WCAG AA 门禁复测。
5. **性能预算复测**(§8 表全项)与 docs 同步:README/CONTEXT/能力台账/ADR 索引更新。
6. 版本 **0.15.0** 发布,RELEASE.md 冒烟走 ship 门。

**门禁**:删除后全仓 `grep egui` 仅剩 CHANGELOG/文档;CI 全绿;G-UI3 双语零裸文案。

---

## 5 · 硬骨头总表(24 项,全部有归属,零"留后续")

| # | 硬骨头 | 现状证据 | 归属 | 完成定义(可验收) |
|---|---|---|---|---|
| 1 | GPUI 换宿主本体 | vb_app 63 文件用 egui | R0–R7 | G-SWITCH 判据全绿,旧宿主删除 |
| 2 | i18n 全量 | ftl ~15 key,全仓中文硬编码 | R0(地基)+R7(收口) | 双语 600 key,CI 零裸文案,即切即生效 |
| 3 | 导出真取消 | 对话框关=后台继续 | R0(核心)+R5(UI) | 任意阶段取消 <1s 生效,产物不留半截文件 |
| 4 | 键盘焦点体系 | FOCUS 仅 active 描边,多控件不可 Tab | R3 | G-UI5 焦点走查 100% 可达 |
| 5 | 贝塞尔缓动拖柄 | timeline.rs L1325 仅预览,"拖柄留后续" | R4 | 拖柄编辑+钳制+落盘幂等 |
| 6 | Kiln 取消接口 | 无中途取消语义 | R0 | 任务句柄 + 分段边界检查,CLI 同步支持 |
| 7 | 画布 4096 纹理上限 | canvas.rs L129 min(4096) | R1 | 高分屏缩放无糊,交互路径无整幅上限 |
| 8 | 画布真文本 X-2 | egui 近似覆盖层+常驻提示 | R1 | parley+skrifa 真文本,parity 文本用例过,提示删除 |
| 9 | 多窗口迁移 | egui viewport(show_viewport_immediate) | R2 批 6 | GPUI 一项目一窗 + 启动器窗 + focus 语义平移 |
| 10 | 布局持久化 v3 | workspace.json v2 | R2 批 6 | v3 迁移函数 + 坏文件回退默认 |
| 11 | GPU 设备丢失恢复 | egui 侧无体系 | R1 | GpuGuard generation 失效 + 重试一次 + 自动保存兜底 |
| 12 | undo 溢出写盘 | 200MB 后栈底丢弃 | R7 | .vbdoc/history/ 滑动窗口,历史面板可跳转只读重放 |
| 13 | vb_platform 实体化 | 仅 1 行注释 | R0 | 六 trait 双实现,vb_kit 零 OS 直引 |
| 14 | 状态栏窄窗溢出 | 0.14.0 遗留 | R2 批 6 | <1200px 分级折叠,零遮挡 |
| 15 | CRDT GUI 入口 | 会话层全绿,无入口 | R6 | 本机/局域网会话可建可加,冲突队列可视 |
| 16 | WebCodecs 阶段 2 自验 | 解码回读比对未做 | R5 | selfcheck 阈值门 + flaky 自动降道告警 |
| 17 | 音频导出 + GIF 流式化 | 未做 | R5 | MP4 带音轨;GIF 流式 O(窗口) 内存 |
| 18 | 跨实例串行点终判 | 等 warnings 实例统计回传 | R0 | 直方图自证数据,结论落 ADR 备忘 |
| 19 | egui 观感修补不可平移 | theme.rs 451-531 是 egui 特供 | R0–R2 | GPUI 侧同等走查清单(文本度量/圆角抗锯齿)过查 |
| 20 | 符号跨文档复用 | 09-H Partial | R6 | .vb-symbol 导入导出 + 重链 |
| 21 | 07-L 两条缺命令菜单项 | 台账 Partial | R6 | 命令登记 + 可逆性门禁过 |
| 22 | X-5 曲率/铅笔/形状生成器 | 台账 Partial(生成器未做) | R1(铺路)+R6 | 三工具可用,可编辑边界诚实标注 |
| 23 | 控件高度体系裁决 | 两上游冲突悬案 | R0 | ADR-0048 落案(§3.4 裁决一),门禁测试 |
| 24 | 图标系统迁移 | iconflow 字体(egui 生态) | R2 | Lucide SVG + all_icons_resolve 式门禁 |

---

## 6 · 动效与手感规范(全局)

### 6.1 动效令牌(时长四档 + 弹簧两档 + 曲线三支)

| 令牌 | 值 | 用途 |
|---|---|---|
| INSTANT | 0ms | 画布缩放/平移/拖动、播放头、数值实时回显——**绝不动画** |
| HOVER | 80ms | 悬停底色 tint、参考线/气泡 fade、tooltip 出现 |
| STATE | 120ms | 选中态切换、浮层/对话框出场、关键帧 pop、命令面板下滑 |
| PANEL | 200ms | 面板折叠/展开、Tab 下划线、FLIP 重排、主题切换 |
| SPRING_SNAPPY | stiffness 300 / damping 22 | 按下回弹(图标缩 92%→1)、手柄吸附到位 |
| SPRING_SOFT | stiffness 200 / damping 28 | 面板拖拽跟手、浮动条惯性 |
| EASE_STANDARD | cubic-bezier(0.2, 0, 0, 1) | 默认曲线(进出场共用) |
| EASE_ENTER / EASE_EXIT | (0,0,0.2,1) / (0.4,0,1,1) | 有方向浮层专用 |

实现底座 = sable 动画引擎三层(L1 Tween `Animated<T>`/L2 Spring/L3 关键帧),`AnimScheduler` 脏区合并;**reduced-motion 系统设置与动效总开关任一关闭 → 全部直通**(G-UI10)。明确不做(14 篇沿用):画布元素入场动画、按钮弹跳、装饰粒子。

### 6.2 微交互清单(逐处落到控件)

| 场景 | 规格 |
|---|---|
| 按钮/工具按钮 | hover 底色 80ms;按下图标 scale 0.92(spring 回);激活态 accent_dim 底 + accent 描边 |
| ToolButton tooltip | 强制「名称 (快捷键)」,延迟 400ms,HOVER 淡入 |
| 面板折叠 | 高度 tween 200ms;箭头旋转 120ms;状态进 workspace v3 |
| Tab 切换 | 下划线滑动 200ms;内容交叉淡化 80ms(不做滑动翻页) |
| 图层重排 | FLIP 200ms;被拖行 60% 不透明度 + 阴影提升一级 |
| Toast | 右下滑入 120ms;TTL 信息/成功 2.5s、告警 5s、错误 10s;错误可选中+复制;退场 100ms |
| 对话框 | 出场位移 8px + fade 120ms;退场 100ms;Esc/Enter 接线固定(dialog_footer 规格) |
| 命令面板 | 下滑 120ms + 背板 80ms;结果高亮跟随键盘即时无动画 |
| 关键帧操作 | 插入 pop(scale 0.85→1,120ms);删除直接消失(不做死亡动画) |
| 播放 | 播放头/时间码 INSTANT;循环点跳变无动画 |
| 进度 | 不确定态 pill 1.2s 循环;确定态线性推进,完成态对勾 120ms pop |
| 主题切换 | 全 token lerp 200ms(set_mode_animated);用户内容色(文档)不受影响 |
| 窗口材质 | Mica/Acrylic 静默启用,失败降级纯色(不告警不重试) |

### 6.3 光标与手感

自定义图像光标全量回归(R1):默认箭头/文字 I 型/抓取手/缩放各向箭头/旋转弯曲双箭头(AI 同款);手柄命中区 ≥12px(屏幕像素,DPI 感知);拖动阈值 3px 启动(防误触);双击间隔用系统值;滚轮画布=纵向平移、`Alt+滚轮`=缩放、`Ctrl+滚轮`=时间轴缩放(与 AI/Figma 双兼容,02 篇口径不变)。

---

## 7 · 鲁棒性与耦合专项(横切所有轮次)

### 7.1 耦合规则(违者门禁红)

1. **依赖方向**:`vb_doc ← vb_session ← vb_kit ← vb_shell`,vb_kit/vb_shell 禁 import vb_doc 内部类型(G-UI7,依赖方向测试)。
2. **命令单源**:一切 UI 动作 → commands.yaml ID → run_command 单入口;新命令先登记后接线(可逆性门禁不豁免)。
3. **视图投影**:面板 = `fn(state) -> Content` 纯函数 + 事件上行;禁止面板持有会话可变引用(对现状"子模块摸会话态"的总清算)。
4. **令牌单点 + 命名空间**:vb-ui-tokens.json 唯一真相;`--vb-ui-*` / `--vb-brand-*` 禁止互引;UI 令牌永不写进用户文档,文档令牌永不进主题。
5. **键位单源**:命令 ID 单源,keymap.json 用户覆盖层语义不变;sable-dock keymap 只做绑定层(00 分册 §4.7 建议)。
6. **动画求值单路**:播放预览、画布时间游标、导出求值共享同一函数(现 vb_kiln 路径),壳不自算。

### 7.2 鲁棒性规则

1. **线程纪律**:文档读写/导出/自动保存/缩略图生成一律后台线程;UI 线程只做 poll(节流 100ms);panic 断连必须如实上报(0.14.0 语义平移)。
2. **原子写**:一切落盘走 `write_atomic`(vb_kiln 语义全仓推广,含 workspace v3/keymap/队列状态)。
3. **GPU 韧性**:GpuGuard 接入(R1);设备丢失 → 缓存全失效 → 重建重试一次 → 失败致命弹窗 + 自动保存兜底提示。
4. **坏输入韧性**:布局/键位/令牌 JSON 解析失败一律回退默认并告警 toast,不许崩;ftl 缺 key 回退 zh + debug 断言。
5. **性能预算**(CI 内 dev stats 实测):面板打开 <100ms;命令面板首帧 <50ms;画布平移/缩放 60fps@1 万节点;拖动输入延迟 <33ms;启动到启动器 <2s;项目窗开 <1.5s;UI 常驻内存 <350MB(不含 undo 上限)。
6. **降级链**:浏览器缺席→K 车道(已有);Mica 失败→纯色;硬编失败→截图车道(R5);自定义光标失败→系统光标;字体缺→逐级降级(已有)。

---

## 8 · 门禁清单(新增 G-UI1–G-UI10,既有门禁全部保留)

| 编号 | 门禁 | 机制 |
|---|---|---|
| G-UI1 | tokens_sync2 | inject 后语义色/间距/圆角/动效四表与 vb-ui-tokens.json 逐值比对(机制照搬 theme.rs) |
| G-UI2 | 硬编码颜色扫描 | 扩展到 vb_kit/vb_shell,唯一白名单 = 令牌定义文件 |
| G-UI3 | 界面文案走 t() | vb_kit/vb_shell 渲染路径禁裸文案字面量(白名单:日志/调试);R7 后加双语 key 完整性断言 |
| G-UI4 | 术语扫描 | check_terminology.py 扩展新 crate + en.ftl(禁 Frame/Component 等) |
| G-UI5 | 焦点走查 | 遍历面板断言 Tab 可达数 = 可交互控件数;焦点环可见走查表人工复核 |
| G-UI6 | 命令覆盖 | 菜单/工具条/浮动条/命令面板每个条目解析到 commands.yaml ID(既有测试扩展) |
| G-UI7 | 壳纯度 | 依赖方向测试:vb_kit/vb_shell 无 vb_doc 内部类型;vb_kit 无 OS 直引 |
| G-UI8 | WCAG AA 对比度 + 高度派生公式 | token 层测试移植 + §3.4 裁决一公式断言 |
| G-UI9 | ui_shots 双宿主基线 | 过渡期双跑;R7 后单基线;含深浅双主题 |
| G-UI10 | 动效归零 | 总开关/reduced-motion 关闭后一切 Animated 直通 |
| 既有 | fmt/clippy/test(733+)/selfcheck/术语/L1 幂等/示例体检/canvas parity/Dropped≤2/Planned=0/许可 | 不变,对新 crate 生效 |

---

## 9 · 风险登记(前五)

| 风险 | 级别 | 对策 |
|---|---|---|
| GPUI/gpui-component 上游断档(0.6+ 不兼容) | 高 | 版本锁 0.5.1 + rev 锁 sable;升级窗口单独排期,不带 feature 升级 |
| 画布上屏通道性能不达标 | 高 | R0 spike 三路对比前置;最坏回退 (a) 纹理路线 + 平铺,仍破 4096 |
| 迁移期双维护拖慢功能迭代 | 中 | 旧宿主冻结纪律(§3.2.4);功能只落新宿主 |
| ACL-1.0 无意污染 sable | 高 | §3.5 规程 + PR 勾选 + NOTICE 登记;sable 侧 PR 一律审 |
| 29+ 面板重写工期失控 | 中 | R2 分 6 批,每批独立合入可演示;批内 ⟳ 子代理并行 |

---

## 10 · 与能力台账的对账

本篇触发的新台账条目(实现时落 `crates/vb_app/src/capabilities.rs` 或其继任者,遵守 Done/Partial(+去向)/Dropped 三态、Planned=0 门禁):

- **SAB-1 GPUI 宿主**(Partial → R7 Done):判据 = G-SWITCH。
- **SAB-2 命令面板**(Partial → R3 Done);**SAB-3 上下文工具条**(R3);**SAB-4 token 原位绑定**(R2);**SAB-5 混合选中态**(R2);**SAB-6 吸附气泡+Alt 旁路**(R3);**SAB-7 渲染队列+真取消**(R5);**SAB-8 缓动库+命名动画组件**(R4);**SAB-9 Agent 幽灵 diff**(R6);**SAB-10 MCP 状态 UI**(R6);**SAB-11 协同入口**(R6,09-K 收口);**SAB-12 音频导出/GIF 流式**(R5)。
- 收口既有 Partial:X-2(R1)、07-L(R6)、X-5(R6)、09-H(R6)。
- 新增 Dropped:**DROP-3 节点图文档模型 / 状态机动效 / 内置生成式 AI / 实时协作光标**(理由见 §2.3,Dropped≤2 门禁需同步修订为按类别计数或并入 DROP-1/2 语义——实现时裁决,裁决记录进 ADR-0045)。

## 11 · 本篇产出的 ADR 清单(实现时落 docs/adr/)

ADR-0045 换宿主裁定(推翻 14 篇 Q5 的前提变化 + 双宿主策略 + DROP 修订);ADR-0046 画布上屏通道(R0 spike 结论);ADR-0047 Kiln 取消语义与逐实例直方图;ADR-0048 控件高度与浮动面板两项裁决;ADR-0049 i18n 策略(Fluent、回退链、门禁)。

## 12 · 调研来源

Figma Blog / Figma MCP docs(figma.com, developers.figma.com)· Penpot Releases 2.15–2.17(github.com/penpot/penpot/releases)· tldraw docs 与 agent starter kit(tldraw.dev, github.com/tldraw/tldraw)· Graphite Blog(graphite.art/blog)· Rive Blog(rive.app/blog)· Jitter(jitter.video)· Remotion Blog(remotion.dev/blog)· Motion Canvas(motioncanvas.io)· Illustrator What's New(helpx.adobe.com)· Cursor/v0/Bolt 公开工作流演示。事实层仅采信官方 release notes 与官方文档。

---

*本篇为规划文档;执行遵循仓库既定纪律:门禁真实运行、能力台账如实入账、每轮可演示可回滚。任何"留后续"字样出现在实现 PR 中即为违规。*
