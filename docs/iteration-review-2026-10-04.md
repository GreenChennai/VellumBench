# VellumBench 迭代审查与 UI 美化方向

> 本文是 **2026-10-04 一轮全仓审查** 的单一真相：先记录已发现的问题（缺陷 / 优化 / 生产差距 / UI 观感），再给出 UI 美化的完整方向与落地规格。
> **纪律：本文不写"后续再议"。** 每条问题当场给编号、给证据、给修复方向、给验收口径；把"以后"消灭在文档里。
> **范围**：只写文档，本轮不改任何代码。

---

## 0 · 同步基线与方法

| 项 | 值 |
|---|---|
| 远端 | `https://github.com/GreenChennai/VellumBench.git` |
| 同步前本地 | `7830d5e`（落后 origin/main **61** 个提交） |
| 同步后本地 = origin/main | `612a405`（`fix(recent,layout): re-export 冒烟测试改键不变式…`） |
| 工作区状态 | 干净，`git pull --ff-only` 快进，**无冲突、无本地改动丢失** |
| workspace 版本 | `0.14.0`（`Cargo.toml:29`），Rust MSRV 1.85 |
| workspace 成员 | **19** 个 crate（`Cargo.toml:3-26`） |
| 审查方式 | 4 路并行静态审查（核心文档层 / 导出导入层 / UI 应用层 / Agent 插件平台层）+ 关键点人工复核 + UI 基线截图走查 |

**审查证据原则**：每条问题的行号均来自同步后的 `612a405` 工作区，可 `grep` 复现。

---

## 1 · 结论摘要（TL;DR）

1. **架构骨架是健康的**：19 crate 的依赖方向基本受控，`vb_doc` 命令层 + `undo` 是全仓唯一编辑入口，`vb_agent`/`vb_kiln` **没有**反向依赖 GUI（这是很多桌面工具做不到的），`.ai`/PDF/SVG 的 L0/L1 往返纪律有实测门禁。**不要推翻骨架**。
2. **真正危险的是"边缘鲁棒性"**：全局递归无深度上限、`expect/unwrap` 散布在 IO 与命令路径、插件进程**无沙箱**、MCP 会话可被一次 panic 永久毒死、PDF 字体子集失败后仍套用错误 remap 会**静默产出错字**。这些是"交付给第三方就翻车"的类别，必须在**当前**迭代关掉。
3. **性能主瓶颈集中在两处**：文本整形管线（每次取字重建字体集合 + 逐字形 O(n²)）与导出（整棵 `Document` clone + PDF 两遍渲染）。它们同时拖慢画布与导出。
4. **与生产级软件的最大差距不在功能，而在"工程表面"**：无可观测性（tracing/metrics）、无统一配置层、无安装/升级链路、可访问性几乎为零、i18n 只有 ~15 个 key、插件无沙箱与资源限制。**功能"能跑"，工程"不能交付"。**
5. **UI 是当前最短的板**：基于 `egui 0.35` 默认样式的深色界面——17 个颜色令牌、3 档文字色、无 elevation、深色无阴影、无空/加载/错误态、焦点环不存在、字号层级只有 6 档且标题仅 15px。**用户说的"太基础、基本上不能用"属实**，且**美化必须落在 egui 栈**（`vb_app`/`vb_ui`），因为 sable 新宿主只有 1 个面板（能力台账）迁完，其余 28 个面板仍在 egui。

### 1.1 优先级矩阵（本轮必须闭环）

| 级别 | 含义 | 数量 | 归属章节 |
|---|---|---|---|
| **P0** | 安全红线 / 数据损坏 / 崩溃 | 6 | §3.1、§3.4、§3.5 |
| **P1** | 正确性 / 鲁棒性 / 主性能瓶颈 | 18 | §3.*、§4、§5 |
| **P2** | 工程质量 / 观感 / 一致性 | 30+ | §3.*、§6、§8 |

---

## 2 · 架构与依赖现状

### 2.1 crate 地图与依赖方向

```
vb_common ─┐
vb_css ────┼─→ vb_html ─→ vb_doc ─┬─→ vb_layout ─→ vb_render
           │                       ├─→ vb_tools
           │                       ├─→ vb_export
           │                       ├─→ vb_kiln ─→ vb_browser
           │                       ├─→ vb_agent  (CLI / MCP / patch)
           │                       ├─→ vb_plugin (子进程 JSON-RPC)
           │                       ├─→ vb_platform (6 trait: clipboard/dialog/...)
           │                       ├─→ vb_session (宿主无关会话层)
           │                       └─→ vb_ui(egui 组件) ─→ vb_app(egui 宿主)
                                                          vb_kit(sable 面板) ─→ vb_shell(sable 宿主, GPUI)
                                                          vb_web(wasm32, 仅 K2)
```

**健康项（保持）**
- `vb_agent` 依赖面为 `vb_common/vb_html/vb_css/vb_doc/vb_layout/vb_render/vb_tools/vb_export + clap/anyhow/...`，**无 `vb_app`/`vb_ui`/`egui`/`gpui`**（`crates/vb_agent/Cargo.toml:20-34`）。
- `vb_kiln` **无代码级反向依赖 GUI**（仅 `docs`/`tests` 提及 `vb_app`）。
- `vb_ui` 不依赖 `vb_app`/`vb_doc`（`crates/vb_ui/src/lib.rs:17-18`），无循环。
- 命令层单入口：`crates/vb_app/src/app.rs:483 exec()` → `vb_doc::commands::Command` + `UndoStack`。

**待修的层次倒置（P1）**

| 编号 | 问题 | 证据 | 影响 |
|---|---|---|---|
| COUP-01 | `vb_layout` 反向依赖 `vb_render` | `crates/vb_layout/Cargo.toml:15`、`crates/vb_layout/src/lib.rs:357,370` 调 `vb_render::text::measure_text_weighted` | 为**一处**文本量测，把一个纯几何/布局 crate 拉进 `vello`/`wgpu`/`swash`/`fontique` 编译面；破坏"布局不依赖渲染"的分层。**提取 `vb_textmeasure`（或下沉到 `vb_common::text`）**。 |
| COUP-02 | `Document` 全 `pub` 字段越界直改 | `crates/vb_doc/src/model.rs:308-342`（`nodes`/`root`/`defs_root`/`artboards` 全公开） | 不变量靠约定：commands/import/export/symbol/collab 均直接改内部状态，任一漏改即破坏 L0/L1。**收窄为访问器 + `pub(crate)`，给跨 crate 的只读视图 trait**。 |
| COUP-03 | 块级标签清单两份硬编码 | `crates/vb_doc/src/import.rs:33-79` vs `crates/vb_html/src/lib.rs:27-78` `BLOCK_TAGS` | 两处手工对齐，必然漂移。**单一真相放到 `vb_html`，`vb_doc` 引用**。 |
| COUP-04 | `vb_agent` 内联 `vb_tools` 几何算法 | `crates/vb_agent/Cargo.toml:27`；`patch.rs:490-517` / `541-618` | patch 事务编排与几何实现同层耦合。**保持依赖但把"编译 op"与"几何"分层，几何只经 `vb_tools` 公共 API**。 |
| COUP-05 | 插件契约用字符串前缀匹配 | `crates/vb_browser/src/cancel.rs:34`、`crates/vb_kiln/src/cancel.rs:103` | 文案一改即静默失配。**升级为 `enum CancelState` 跨 crate 公共类型**。 |

### 2.2 全局可变状态（P1，污染源）

| 编号 | 问题 | 证据 | 影响 |
|---|---|---|---|
| COUP-06 | 字体注册表是进程级 `static Mutex<HashMap>` + 一把全局串行锁 | `crates/vb_render/src/text.rs:23-26,105-108` | 所有文本整形互相阻塞；跨项目不自动清（仅 `vb_kiln` 调 `clear_font_registry`）。**改为 `FontRegistry` 实例经参数/上下文传递**。 |
| COUP-07 | 布局图片尺寸 `thread_local` 缓存**从不清理** | `crates/vb_layout/src/lib.rs:406-413` | 同线程先后处理两个项目的同名图片会**命中错误尺寸**（跨项目污染）。**缓存键加入项目根（绝对路径）或提供 `clear_image_cache()`**。 |
| COUP-08 | i18n 语言态是进程级 `AtomicU8` | `crates/vb_common/src/i18n.rs:93` | 多窗口共享语言态，无实例隔离。**语言随 `vb_session` 实例走**。 |
| COUP-09 | UI 动效开关写进 `ctx.data` | `crates/vb_ui/src/theme.rs:346-368` | 与窗口生命周期绑定，跨窗口/热重载易丢。可接受，但需在 `workspace.json` 单一真相下持久化并加恢复测试。 |

---

## 3 · 缺陷清单

> 格式：`编号 · 严重度 · 位置 · 症状 · 修复方向 · 验收`。行号对应 `612a405`。

### 3.1 核心文档 / 格式层（`vb_common` `vb_css` `vb_html` `vb_doc` `vb_layout` `vb_render`）

#### P0

- **DOC-01 · P0 · 递归深度全链无上限**
  - 位置：`crates/vb_html/src/lib.rs:193-198,263-292,365-389,392-447,449-558`（`walk/convert/can_inline/write_inline/write_node` 全无界递归）；`crates/vb_doc/src/import.rs:1455-1527`（建树）；`crates/vb_doc/src/model.rs:504-511,585-601`（`subtree`/`from_document`）；`crates/vb_layout/src/calc.rs:121-199`（`calc((((…))))`）。
  - 症状：一份深嵌套 HTML 就能在**导入 / 保存 / Undo 捕获 / 布局求值**任一处栈溢出崩溃，直接击穿"忠实解析 + L0/L1 往返"三条不可妥协原则。
  - 修复方向：解析器与序列化器统一加 `MAX_DEPTH`（建议 512，超限报 `VbError::DepthExceeded` 并给用户可见清单，**不静默**）；`subtree`/`from_document` 改显式栈迭代；`calc` 括号深度计数。
  - 验收：新增单测：1024 层嵌套 `<div>` 导入 → 返回结构化错误而非崩溃；序列化同一文档不 panic；`vb_html`/`vb_doc`/`vb_layout` 三包测试全绿。

- **DOC-02 · P0 · `UndoStack::push` 可 panic（唯一编辑入口）**
  - 位置：`crates/vb_doc/src/undo.rs:63` `self.undo.last_mut().expect("merge 需要栈顶")`；触发链 `:69 enforce_memory_cap()` → `:236 drain(..drop_from)` 清空栈，但 `:70 last_merge` 仍置 `Some`。
  - 症状：单条命令体积 > `MAX_UNDO_BYTES` 时栈被清空，下一条**可合并**命令进入 merge 分支 → `expect` panic，**中断整个编辑会话**。
  - 修复方向：`enforce_memory_cap` 清栈后同步 `self.last_merge = None`；merge 分支改 `let Some(top) = self.undo.last_mut() else { 走非合并路径 }`。
  - 验收：注入"单条超大命令 + 后续可合并命令"单测，断言不 panic 且 undo 语义正确。

#### P1

- **DOC-03 · P1 · 导出按**字节**切文本，非字符边界即 panic** — `crates/vb_doc/src/export.rs:372-389` `&text[pos..s]`/`&text[s..e]`，s/e 只 clamp 到 `len()` 未对齐字符边界；`validate_segs` 只在命令 apply 侧把关（`commands.rs:594`），导出侧不复验。→ 导出前对 segments 做 **char-boundary 校验**，越界降级为整段纯文本并 `log::warn`。
- **DOC-04 · P1 · 主件符号定义构建失败被整条吞掉** — `crates/vb_doc/src/import.rs:289` `let _ = importer.build_symbol_def(child);` 违反"不静默"纪律。→ 收集为 `warnings` 并进入导入清单。
- **DOC-05 · P1 · HTML 读盘不剥 BOM（CSS 却剥了）** — `crates/vb_doc/src/import.rs:124` vs `:193`。→ 统一 `strip_bom` 工具函数，两处共用。
- **DOC-06 · P1 · `href` 路径无规范化/无 URL 解码** — `crates/vb_doc/src/import.rs:187` `project_dir.join(href.trim_start_matches("./"))`，`..`/绝对路径/反斜杠/`%20` 未处理 → 越界读与路径不匹配。→ `canonicalize` 后校验**必须在项目根内**，否则记 warning 跳过。
- **DOC-07 · P1 · 协同 writer id 直接进文件名** — `crates/vb_doc/src/collab.rs:345` `format!("ops-{}.json", writer.id)`，含 `/`、`..` 可写出目录外。→ `sanitize` + 大小写归一（Windows）。
- **DOC-08 · P1 · 临时文件名只用 pid，无线程号** — `crates/vb_doc/src/export.rs:155` `.tmp-{file_name}-{pid}`；同进程多线程并发导出同一文件互相覆盖，削弱原子性。→ 加 `thread::current().id()` 或原子计数器 + 随机后缀。
- **DOC-09 · P1 · 文本整形热路径每次重建字体集合 + 全局锁 + 多份字体拷贝 + O(n²) 逐字形索引**
  - 位置：`crates/vb_render/src/text.rs:117-197`（每次 `Collection::new`+`SourceCache::new`，遍历 15 候选族，`qf.blob.data().to_vec()` 整份拷贝）；`:248` `Arc::new(font_data.to_vec())` 再拷一份；`:329/504-509/625-626`、`crates/vb_render/src/cpu.rs:268` 的 `chars().nth(gi)` / `chars().take(..).sum()` → O(n²)。
  - 修复方向：`FontRegistry` 实例化 + 缓存 `Collection`/`SourceCache`/字形度量；`Chars` 迭代器改一次性 `Vec<usize>` 索引（或 `char_indices` 单趟）；去掉多余 `to_vec()`。
  - 验收：1000 字符文本整形耗时基准降 ≥50%，`vb_render` 测试全绿。

#### P2

- **DOC-10 · P2 · `@font-face` 读盘失败静默成 0 字节字体** — `crates/vb_render/src/text.rs:34` `unwrap_or_default()` → 改 `Result` + 缺失字体清单（对齐 03 篇"字体缺失"对话框）。
- **DOC-11 · P2 · 公共 API 依赖调用方保证非空** — `text.rs:552,479`（`line.last().expect("nonempty")`、`line[0]`）；`break_lines` `:355` 会 push 空 `cur`。→ API 内守卫 + 空行跳过。
- **DOC-12 · P2 · 裸 `unwrap` 于模型映射路径** — `crates/vb_doc/src/import.rs:453,477`；`crates/vb_doc/src/commands.rs:978,1010`（`slots[0]` 越界）；`crates/vb_doc/src/model.rs:499,516`（`expect("node")`）。→ 统一 `Option`/`Result` 出口。
- **DOC-13 · P2 · 命令路径出现 `unreachable!`** — `crates/vb_doc/src/collab.rs:288` → 未来枚举扩展即崩溃点，改穷举 + 返回错误。
- **DOC-14 · P2 · 数值输出可溢出为 `inf`** — `crates/vb_common/src/units.rs:26`（`is_finite` 只校验输入）→ 对结果 `is_finite` 复检，非法则返回原值。
- **DOC-15 · P2 · 错误类型用 `String` 糊弄** — `crates/vb_doc/src/collab.rs:340,354`（`Result<_, String>` + 读路径吞成空 `Vec`）→ 并入 `VbError`。
- **DOC-16 · P2 · 未知名属性排序用字节和散列（注释写"字母序"）** — `crates/vb_css/src/lib.rs:142-147`，散列可碰撞 → 与契约不符。→ 改为真字典序或改注释对齐实现。

### 3.2 导出 / 导入 / 浏览器车道（`vb_kiln` `vb_export` `vb_browser`）

#### P1

- **EXP-01 · P1 · PDF 字体子集化失败回退后仍套用子集 remap → 静默错字/豆腐块**
  - 位置：`crates/vb_kiln/src/pdf.rs:664-682`（`_ => { f.subset_len1 = data.len(); f.subset = (*data).clone(); }` 回退**全量**字体），但下方**无条件** `f.remap.insert(gid, new_cid)`，内容流按新 CID 发射（`:1085-1090`），`Identity` 映射下 CID≠原 GID。
  - 症状：静默产出**错字/豆腐块**的可编辑矢量稿——"输出与源不等价"，交付即翻车，且无告警。
  - 修复方向：子集失败时**跳过 remap**（走 Identity/GID 直发）或整体降级为轮廓化文本 + 告警；`unwrap_or(0)` 缺失字形必须可见告警。
  - 验收：构造"子集化失败"注入用例，断言 PDF 可提取文本与源逐字一致；`cargo test -p vb_kiln` 全绿。

- **EXP-02 · P1 · `--max-wait` 被静默忽略，长任务不可中断**
  - 位置：`crates/vb_kiln/src/bin/kiln-cli.rs:407` `let _ = max_wait;`（默认 15.0，`:82-83` 常驻参数面）；settle 5s / 截图 180s / printToPDF 300s 均不可被该参数收口。
  - 修复方向：把 `max_wait` 接到 settle/截图/printToPDF 的 deadline；导出任务句柄化 + 协作式取消（与 ADR-0049 一致）。

- **EXP-03 · P1 · 浏览器车道失败慢、无重连、无 connect 超时**
  - 位置：`crates/vb_browser/src/ws.rs:337-339`（对端关闭返回 `Ok(None)`），`cdp.rs:137-142`（当作"暂无进展"每 5ms 睡到 deadline）；`httpc.rs:16-17` 裸 `TcpStream::connect`（对比 `ws.rs:216` 用了 `connect_timeout`）；`page.rs:192-200` 仅对 enable 类调用重试一次，`screenshot`/`begin_frame_screenshot` 无重试。
  - 修复方向：WS 区分"Closed/Failed"与"NoMessage"立即失败；HTTP 加 connect/read 超时；关键截图路径加一次重试 + 明确错误。

- **EXP-04 · P1 · PDF 写入器两遍渲染，渐变/位图重复光栅化与复制**
  - 位置：`crates/vb_kiln/src/pdf.rs:63-72`（pass1 全量绘制后 `usage.images/opacities/patterns.clear()` 丢弃，pass2 重画）；`:1189 image_for(bmp.rgba.to_vec(), …)` 与 `:815-816 / :871-872`（渐变位图 + `bake_shape_mask`）**不判 `remap`**，两遍全跑。
  - 症状：每张位图 `to_vec()` 两次、每个渐变位图光栅化两次，大图内存峰值与耗时成倍。
  - 修复方向：pass1 只走**度量**路径（不产位图），或为资源建立"跨 pass 复用表"（`remap` 判据）。

#### P2

- **EXP-05 · P2 · CLI 入口残留 `expect/unwrap` → 退出码漂到 101** — `crates/vb_kiln/src/bin/kiln-cli.rs:750`（有前置白名单护住但脆弱）、`:952,1182,1191,1192`。契约要求 2/3/4。→ 全部改 `Result` 出口（`bin/kiln.rs` 已是干净榜样）。
- **EXP-06 · P2 · 文本轮廓对空行 panic** — `crates/vb_kiln/src/pdf.rs:1114`、`crates/vb_kiln/src/postscript.rs:233`（`line.last().expect("nonempty")`）。
- **EXP-07 · P2 · SVG path 数字精度无舍入** — `crates/vb_export/src/svg.rs:183-217`（`write!("M{} {}")` 对 f64 最短往返，产出 `123.45000000000002`）；对比 PDF 侧有 `fnum`（`pdf.rs:1768-1774`）3 位舍入。→ 两车道统一精度函数。
- **EXP-08 · P2 · SVG 属性转义不处理双引号** — `crates/vb_export/src/svg.rs:483-487`，而 `:274` `font-family="{}"` 用双引号包裹 → 含 `"` 的字体名破坏 XML。
- **EXP-09 · P2 · base64 解码对畸形尾块不报错** — `crates/vb_browser/src/b64.rs:44-55`（`chunk.len()==1` 仍 `push`）→ 截断截图静默产出垃圾字节。→ 严格模式返回 `Err`。
- **EXP-10 · P2 · 临时目录/进程残留窗口期长** — `crates/vb_browser/src/browser.rs:257-298`（仅回收 mtime > 6h 且前缀命中 `PREFIXES` 的目录；崩溃留下的是**新鲜**目录要等 6h）；前缀清单硬编码白名单，新增落盘点易漏。→ 启动时按"存活 pid 探测"回收 + 落盘点集中注册。
- **EXP-11 · P2 · settle 预算外挂：`wait_assets` 不受 deadline 约束** — `crates/vb_browser/src/capture.rs:57-60,204-207`（页内 3s/5s + 200ms 先于 deadline 检查跑完，慢机可吃穿预算）。
- **EXP-12 · P2 · 魔法数字散落三 crate** — 采集上限 `15_000`（`capture.rs:309-310`、`domexport.rs:115`）；视口兜底 `1080`（`lib.rs:74-77`、`animlane.rs:457-458`）；settle `5s`（`capture.rs:52`）；大量经验 sleep 120–4000ms。→ 集中到配置常量模块。
- **EXP-13 · P2 · `auto` 降级判定无单一权威点** — `kiln-cli.rs:448,487-488,558-566,575-585,616-668,743-748,800-809`，`lane_fallback_native` 在 `454/561/804/933/939` 多处读写。→ 抽 `resolve_lane()` 单函数返回结构体。

### 3.3 Agent / CLI / MCP（`vb_agent`）

- **AGT-01 · P1 · MCP 互斥锁中毒 = 一次工具 panic 永久废掉会话**
  - 位置：`crates/vb_agent/src/bin/vellum-mcp.rs:25` `static SESSION: Mutex<Option<Session>>`；`:27-33 with_session` **持锁执行闭包**。
  - 症状：任一工具 panic 时 `MutexGuard` 随 unwind 释放 → 锁被 **poison**；此后再 `lock().map_err(|_| "session poisoned")?`（`:28,72`）恒失败，**会话再也无法恢复**（无 clear/re-lock 路径），与注释 `:331-333`"任何工具 bug 不得击穿主循环"直接矛盾。
  - 修复方向：`lock().unwrap_or_else(|e| e.into_inner())`（panic 后恢复）或在锁内不执行用户闭包；工具 panic 后**重建会话**并返回结构化错误。
  - 验收：注入"工具 panic"用例，断言后续 `tools/call` 仍可成功。

- **AGT-02 · P1 · CLI 退出码契约被 panic 打破** — `crates/vb_agent/src/main.rs:209,234,237,238,377,448,692,871`；`patch.rs:231,261,294,407,437,470,482` 的 `doc.nodes.get(..).unwrap()`。id 源自导入文档容错路径，存在悬挂 id 即触发，退出码 101 落在 0–4 约定之外。→ 全改 `ok_or(CliError::…)`。
- **AGT-03 · P2 · `--json` 模式下错误不走结构化输出** — `main.rs:136-157`（一律 `eprintln!` 纯文本；`Other` 与 `Usage` 同码 1）。→ `--json` 时输出 `{"ok":false,"error":…}`。
- **AGT-04 · P2 · MCP 无版本协商 / `--doc` 导入失败被吞** — `vellum-mcp.rs:315-319`（硬编码 `2024-11-05`，不读客户端版本）；`:384-394` `let _ = tool_open(..)` 丢弃导入失败。→ 版本比对 + 启动即报错。
- **AGT-05 · P2 · MCP `tool_get_html` 的 `scope` 未校验** — `:279-298`（非法值静默当 html，schema 声明 enum）。`tool_export` 的 `scale` 无上界（`:189 as u32`）→ 巨图分配。
- **AGT-06 · P2 · patch `compile_op` 有校验期副作用** — `patch.rs:378,450,636,644` 在校验期就 `alloc_sid_for_dup()`，后续 op 编译失败时已消耗 sid 不归还 → 违反"全部成功或全部回滚"字面承诺（树/rev 未变，仅 sid 空间泄漏）。`Compound` 回滚吞错 `let _ = prev.revert(doc)`（`commands.rs:1125-1126`）。
- **AGT-07 · P2 · `patch --dry-run` 实为"真应用不落盘"** — `main.rs:499-503`（注释自陈），不预演保存步骤、不回报 `dry_run`。→ 在克隆 doc 上执行 + 回报标志。

### 3.4 插件 / 平台（`vb_plugin` `vb_platform`）

#### P0

- **PLG-01 · P0 · 插件进程无沙箱，"零权限/无直改通道"仅对协议成立**
  - 位置：`crates/vb_plugin/src/lib.rs:8-15`（三条安全红线 + `:11-12`"崩溃隔离"）vs `crates/vb_plugin/src/process.rs:69-88`（`spawn` 只设 stdio 三管道 +（Windows）`CREATE_NO_WINDOW`，**无 job object / 无 token 降权 / 无 namespace / 无系统调用过滤**）；白名单闸门只作用于 `runCommand` 这一 JSON-RPC 方法（`host.rs:755-798`）。
  - 症状：插件是**普通用户权限原生进程**，可直接读写 `index.html`、联网、执行任意命令，完全绕过"无直改通道"。白名单**只对诚实插件有约束力**（源于主动放弃 WASM，`lib.rs:5-6`）。
  - 修复方向（择一，需 ADR）：(a) Windows **Job Object**（`KILL_ON_JOB_CLOSE` + 句柄/内存/CPU 限额）+ 低完整性令牌；(b) 恢复 WASM/WASI 沙箱路线（代价与收益需重估）；(c) 至少在首次安装时**明确告知"插件为原生进程，等同你本人权限"**并强制显式勾选。
  - 验收：恶意示例插件无法写出插件目录之外、无法在宿主退出后残留；安装对话框有权限说明。

#### P1

- **PLG-02 · P1 · 授权只快照 `commands`，不绑定 `entry` → manifest 篡改防线不完整** — `crates/vb_plugin/src/auth.rs:147-152`（只比 `g.commands == commands`）；`entry` 每次从磁盘现读（`manifest.rs:243-257`←`host.rs:502`）。注释 `auth.rs:145-146` 声称防"先授权 A 再换 B"，但**不覆盖入口可执行**。→ 授权快照纳入 `entry` 路径 + 文件哈希。
- **PLG-03 · P1 · `entry` 路径穿越无包含性校验** — `manifest.rs:251-256`（绝对路径原样透传；相对路径 `manifest_dir.join(p)` 不归一、不校验落在插件目录内）。`"entry": "../../../../Windows/System32/xxx.exe"` 即可以用户权限启动任意可执行文件；`bin:` 落宿主同目录。→ `canonicalize` + **前缀包含校验**，`..` 直接拒绝。
- **PLG-04 · P1 · 插件 stdout 帧无长度上限、入站队列无界 → 可 OOM 宿主** — `process.rs:96-145`（`reader.lines()` 逐行 `push_back`，单行无上限，`queue`/`responses` 无容量）；对比 stderr 有 500 条上限（`:156`）。→ 单行长度上限（如 8MB）+ 队列容量上限 + 超限断连。
- **PLG-05 · P1 · `poll` 在 UI 线程同步执行宿主命令/投影** — `host.rs:662-692`（注释"每帧调用"），`M_RUN_COMMAND → services.run_command`（`:790`）、`M_DOC_PROJECTION → doc_projection`（`:713`）同步执行；单帧处理请求数无上限。→ 命令与投影走后台线程 + 结果回投；每帧处理数设上限。

#### P2

- **PLG-06 · P2 · 握手无版本协商/能力探测** — `host.rs:861-876`（只校验回包含 `name`，不比对 `PROTOCOL_VERSION`）。
- **PLG-07 · P2 · `call` 忙等轮询 5ms + `pending` 泄漏窗口** — `process.rs:226-255`（`thread::sleep(5ms)` 轮询；`:232` 残留 `expect`；`:197-199` 写入 `pending` 后若调用方不 `call` 永不清理）→ condvar/通道。
- **PLG-08 · P2 · 无资源限制与孙进程回收** — `process.rs:299-322` 仅 `kill()+wait()` 直接子进程；无 job object → 插件派生的孙进程可能残留孤儿。
- **PLG-09 · P2 · `vb_platform` 接线不完整（非空壳）** — 六 trait 与两后端齐备，但宿主混用：`launcher.rs:400-401` 走 `EguiClipboard`，`:444` 又直呼 `ui.ctx().copy_text()`；`:240-242` 直呼 `rfd::FileDialog`。仅 `Clipboard`/`DarkModeProbe` 被真正消费。→ 宿主侧 seam 收口，purity 门禁扩展到 `vb_app`。
- **PLG-10 · P2 · `is_semver` 过宽** — `manifest.rs:270-276`（每段 1–4 位数字，`"0100.0.0"` 通过）。

### 3.5 UI / 应用层（`vb_ui` `vb_app` `vb_kit` `vb_shell`）

- **UI-01 · P1 · i18n 覆盖 ≈ 0** — `crates/vb_app/src/i18n.rs:12-13` 自陈"仅本批新增文案走 `t()`，**既有界面文案仍为中文硬编码**"。实测 `vb_app` 中调用 `t()/t_args()` 的**只有 4 个文件 8 处**；中文硬编码密度：`app.rs` 272 行、`shell.rs` 260、`shortcuts/catalog.rs` 261、`timeline.rs` 210、`panel_dock.rs` 196……**门禁漏洞**：`crates/vb_kit/tests/ui_literals.rs:401-403` 只扫 `vb_kit`/`vb_shell`，**完全不覆盖 `vb_app`**。
  → 先补门禁（扫 `vb_app`）+ 白名单，再按文件批量抽取到 `i18n/{zh,en}.ftl`。
- **UI-02 · P1 · 每帧重跑整套样式注入** — `crates/vb_app/src/app/frame.rs:27` `theme::apply_ex(ui.ctx(), …)` 每帧改 100+ 字段（`theme.rs:399-532`），样式幂等可缓存。→ 仅在主题/缩放/动效开关变化时注入。
- **UI-03 · P1 · 自定义控件绕过 `WidgetInfo` → 无屏幕阅读器可达性** — `crates/vb_ui/src/components.rs:320-326` 等用 `ui.painter().text(...)` 直接画；`theme.rs:319-320 stroke::FOCUS` 只当 active 态描边用（`:469`），**不是键盘 focus ring**。→ 自绘控件补 `WidgetInfo`/`with_label`，焦点环真接线。
- **UI-04 · P1 · 深色主题完全没有阴影/层次** — `theme.rs:421-440`（`window_shadow = if dark { Shadow::NONE }`），只有浅色有阴影 → 深色面板全平。见 §8.3。
- **UI-05 · P2 · 面板布局硬编码、不支持自由拖拽停靠** — `crates/vb_app/src/app/dock_layout.rs:3-4`（自研 `enum DockSide`，不引 `egui_dock`）；`panel_dock.rs:49-61` 九个面板硬编成 4 个固定 Tab 组，只能右键左右移（`components.rs:1291-1300`），不能拖到任意位置；工具栏只支持 4 边吸附（`dock_layout.rs:63-70`）。
- **UI-06 · P2 · 组件未 token 化，魔法数字散落** — `components.rs:33 FIELD_LABEL_WIDTH=56.0`、`:1245 h=28.0`、`:1319 size=20.0`、`:266/305/313` 内联偏移；`capabilities_panel.rs:160/175/256` 的 3/2/34 非 4 基数。
- **UI-07 · P2 · 空/加载/错误态几乎缺失** — 唯一空态在 `capabilities_panel.rs:292-303`；egui 宿主无统一空态组件，体检只有一句绿字（`health.rs:627-630`），后台导出仅状态栏文本（`frame.rs:139-140`），无骨架屏/spinner。
- **UI-08 · P2 · `VellumApp` 单一巨型 struct（约 347 行字段）** — `crates/vb_app/src/app.rs:121-468`；面板开关布尔 16+ 个（`char_panel_open`…`plugins_mgr_open`）散在各处；`panel_dock.rs:24-26` 自陈"开关真值仍在 `VellumApp` 的 `*_open`"。
- **UI-09 · P2 · 事件处理 = ~1900 行字符串 match** — `dispatch.rs:153`（file/edit ~300 臂）、`dispatch_view.rs`（487）、`dispatch_canvas.rs`（291）、`menu_commands.rs`（469）+ `symbol_cmds.rs`（508）。
- **UI-10 · P2 · undo 会话开关是非局部状态** — `app.rs:521-536 num_commit`（把 NumField scrubby/失焦折算成 `UndoStack::begin_session/end_session`），兜底在 `dispatch.rs:103-106`；跨面板/拖拽/快捷键易遗漏 → undo 不一致隐患。
- **UI-11 · P2 · 新宿主窗口标题 `Box::leak`** — `crates/vb_shell/src/main.rs:52-58` 把取词结果 leak 成 `'static`（每开一窗泄漏一小段；热切语言不更新）。
- **UI-12 · P2 · 多窗口 `workspace.json` "最后写入胜"** — `crates/vb_app/src/shell.rs:16-18`（并写无合并）。
- **UI-13 · P2 · `cjk_tweak()` 是恒等变换（排版项未验收）** — `crates/vb_ui/src/fonts.rs:211-213`（14 篇 §3.3 验收项"中英混排不跳基线"仍是 TODO）。
- **UI-14 · P2 · 随包字体缺失，退系统字体** — `fonts.rs:104-143`（需外部 `assets/fonts/` 或 `VB_FONTS_DIR`；缺省 `msyh.ttc`）。
- **UI-15 · P2 · 调试残留** — `crates/vb_layout/src/lib.rs:298` `eprintln!("[rect] {n}")`（落 stderr，安全但应清理）。

---

## 4 · 性能优化点（按收益排序）

| 编号 | 收益 | 位置 | 动作 |
|---|---|---|---|
| PERF-01 | ★★★ | `crates/vb_render/src/text.rs:117-197,248,329` + `cpu.rs:268` | 字体集合/字形度量缓存 + 去 O(n²) 索引 + 去多余 `to_vec()`（见 DOC-09）。画布与导出同时受益。 |
| PERF-02 | ★★★ | `crates/vb_kiln/src/pdf.rs:63-72,1189,815-872` | PDF 两遍渲染 → 度量遍 + 资源复用（见 EXP-04）。 |
| PERF-03 | ★★ | `crates/vb_doc/src/export.rs:23,228,250,349,358` | 导出整棵 `Document` clone + 逐节点 clone → 借用/`Arc` 共享只读快照。 |
| PERF-04 | ★★ | `crates/vb_browser/src/capture.rs:163-182,380-427,460-488` | `wait_visual_stability` 每拍全页 JPEG 解码；分块拼接峰值 ≈ 2×整页 + 又一份 RGB 副本；`b64.rs:39-43` 双份缓冲 → 缩略探针 + 流式编码。 |
| PERF-05 | ★★ | `crates/vb_app/src/app/frame.rs:27` | 每帧样式注入 → 变更时注入（见 UI-02）。 |
| PERF-06 | ★ | `crates/vb_doc/src/model.rs:401-417,428-433` | `find_by_sid`/`sid_in_use` 线性扫描 + `alloc_sid` 循环 → O(n²)；建 `sid→NodeId` 索引。 |
| PERF-07 | ★ | `crates/vb_doc/src/import.rs:1400-1445` | `merged_class_decls` 每元素遍历全部规则（O(n·m)）→ 预索引 class → 规则。 |
| PERF-08 | ★ | `crates/vb_html/src/lib.rs:450,473,478,523,547`、`crates/vb_css/src/lib.rs:259,322,374-416` | 序列化/规范化热路径 `format!`/`repeat`/`collect::<Vec<char>>` → 复用缓冲、`write!` 到 `String`。 |
| PERF-09 | ★ | `crates/vb_ui/src/components.rs:254-265,357-364` | 每帧构造含 `&str` 拼接的 `Id`、`format_num` 产生 `String` → 缓存/`Cow`。 |
| PERF-10 | ★ | `crates/vb_kiln/src/animlane.rs:496-502`、`crates/vb_browser/src/cdp.rs:31-54` | 并行度默认 1 + CDP 全同步串行 → 评估受限并发/流式。 |

---

## 5 · 鲁棒性专项（横切全部改动）

> 以下为**硬约束**，任何新代码进入 `main` 前必须满足；不符合即门禁红。

| 编号 | 规则 | 现状反例 | 验收 |
|---|---|---|---|
| RB-01 | **无 `unwrap/expect/panic` 于可失败路径**（IO、解析、命令、CLI、插件）；GUI 内 panic 比缺功能严重 | 本文件 §3 全部 `unwrap/expect` 条目 | `grep` 扫描 + 定向单测注入 |
| RB-02 | **递归必须有深度上限**（解析/序列化/建树/子树立/`calc`） | DOC-01 | 深嵌套单测不崩溃 |
| RB-03 | **一切落盘走原子写**（临时文件 → `rename`，多文件 `index.html` 最后写） | `vb_doc::export::write_project`（PA 项） | 注入写失败后旧文件完整 |
| RB-04 | **长任务必须可取消**（导出/截图/settle/导入），取消延迟 < 1s | EXP-02、EXP-11 | 任意阶段取消测试 |
| RB-05 | **循环/队列必须有容量与时长上限**（插件入站、缩略探针、settle 预算） | PLG-04、EXP-11 | 超限断连/超时退出 |
| RB-06 | **降级链必须显式告警，不许静默**（车道降级、字体缺失、SVG/PDF 导入跳过项、子集化失败） | EXP-01、DOC-10、EXP-07 | 每个降级点有可见清单 + 测试 |
| RB-07 | **线程纪律**：文档读写/导出/自动保存/缩略图一律后台；UI 线程只 poll（节流 100ms）；panic 如实上报 | PLG-05 | UI 帧不被长任务阻塞 |
| RB-08 | **坏输入回退默认并告警**（workspace/keymap/tokens/manifest JSON 解析失败不许崩） | UI-12、PLG-02 | 坏文件单测 |
| RB-09 | **进程不得残留**（浏览器/插件/临时目录），启动时按存活探测回收 | EXP-10、PLG-08 | 崩溃后重启无残留 |
| RB-10 | **poison-safe 锁**：会话/共享态锁不得因一次 panic 永久不可用 | AGT-01 | panic 注入后仍可用 |
| RB-11 | **可观测性**：导出/导入/插件/CLI 关键路径有结构化日志与耗时统计（`tracing`） | 全仓无 tracing/metrics | 日志字段断言 |
| RB-12 | **配置单一入口**：超时/阈值/上限集中定义，环境变量与常量不散落 | EXP-12、PLG-10 | 配置对象单测 |

---

## 6 · 耦合性专项

| 编号 | 规则 | 违例 |
|---|---|---|
| COUP-R1 | 依赖方向 `vb_common ← vb_doc ← (vb_layout, vb_export, vb_kiln, vb_agent, vb_session) ← (vb_ui/vb_kit) ← (vb_app/vb_shell)`；**下层不得反向依赖上层，布局层不得依赖渲染层** | COUP-01 |
| COUP-R2 | 跨 crate 契约用**类型**而非字符串前缀 | COUP-05 |
| COUP-R3 | 跨 crate 只读访问走访问器/trait，**不暴露可变内部** | COUP-02 |
| COUP-R4 | 全局可变状态（字体注册表/图片缓存/语言态）必须**实例化或显式清理**，禁止跨项目污染 | COUP-06/07/08 |
| COUP-R5 | 标签/清单类常量**单一真相**，不得两份 | COUP-03 |
| COUP-R6 | 面板 = `fn(state) -> Content` 纯投影 + 事件上行；**禁止面板持有可变会话引用**（egui 侧先按此收敛，为 sable 迁移铺路） | UI-08、UI-09 |
| COUP-R7 | 命令单源：一切 UI 动作 → `commands.yaml` ID → `run_command` 单入口；新命令先登记 | 现状基本满足，保持 |
| COUP-R8 | 动画/求值单路：画布游标、预览、导出共享同一求值函数；shell 不自算 | 现状满足，保持 |
| COUP-R9 | 令牌命名空间隔离：`--vb-ui-*`（软件外观）与 `--vb-brand-*`（用户文档）禁止互引 | 现状满足，保持 |

---

## 7 · 与生产级软件的差距分析

> 对标：Figma / Penpot 2.x / Illustrator / tldraw / Remotion。按"能不能放心交付第三方"的维度打分（1=缺，5=生产级）。

| 维度 | 自评 | 差距事实（证据） | 必须补的动作 |
|---|---|---|---|
| **稳定性 / 崩溃面** | 2 | `unwrap/expect` 遍布命令与 IO 路径（§3）；递归无上限（DOC-01）；MCP 会话可被一次 panic 永久毒死（AGT-01） | RB-01/02/10；崩溃自动保存 + 恢复（已有 `.vb-autosave/`，需补崩溃上报） |
| **数据安全** | 3 | 原子写已在 autosave 落地，但 `write_project` 原子化仍是 PA 项；协同 writer id 进文件名（DOC-07） | RB-03；路径包含校验 |
| **可观测性** | 1 | 全仓**无** `tracing`/metrics；`vellum-mcp` 甚至未初始化 logger；插件仅内存日志环 200 条 | 引入 `tracing` + 结构化日志 + 关键路径耗时；日志级别与配置 |
| **安装 / 分发 / 升级** | 2 | 有 `dist/package.ps1` 与单文件 CLI，但**无安装器、无自动更新、无签名**；GUI 依赖 GPU（Vulkan/wgpu）缺硬降级说明 | 安装包 + 版本检查 + 崩溃报告 + GPU 能力探测与友好降级 |
| **性能** | 3 | 静态 PNG 车道曾 27.5s；文本管线与 PDF 两遍（§4）；无可视性能预算门禁 | PERF-01/02 + dev stats 实测入 CI（doc22 §7.2 预算表） |
| **可访问性** | 1 | 焦点环不存在（UI-03）；自绘控件无 `WidgetInfo`；只对浅色做了 WCAG AA；`cursor.rs` 自定义光标缺失 | RB + doc22 G-UI5 焦点走查；对比度双主题门禁；读屏标签 |
| **国际化** | 1 | `~15` key，`vb_app` ~99% 硬编码中文；门禁不覆盖 `vb_app`（UI-01） | Fluent 全量 + 门禁扩到 `vb_app` + `en.ftl` |
| **安全 / 插件隔离** | 2 | 插件原生进程无沙箱（PLG-01 P0）；entry 路径穿越（PLG-03）；授权不绑定二进制（PLG-02） | job object / WASM 重估 + 路径包含校验 + 授权哈希 |
| **协同** | 3 | CRDT 会话层全绿但 GUI 无入口（README 未落地项） | doc22 R6 接线 |
| **插件生态** | 2 | 有 manifest/权限/示例，但无沙箱、无版本协商、无资源限制 | PLG-02/04/06 + 插件市场/签名（可选） |
| **测试 / 门禁** | 4 | 726+ 测试、fmt/clippy/术语/L1 幂等/像素对拍门禁齐全——**这是最强项** | 保持；补 fuzz/属性测试（解析器）、panic 注入、坏输入 |
| **文档** | 4 | ADR 51 篇、design 25 篇、CONTEXT 术语表——**很强** | 保持；代码与文档同步门禁 |
| **工程化（配置/CLI 契约）** | 2 | 退出码契约不统一（AGT-02/03）；配置散落（EXP-12）；`dry-run` 语义不实（AGT-07） | 统一 `CliError` + 退出码表 + 配置对象 |

### 7.1 "生产级"最小充分集（本轮应达成的不可妥协项）

1. **崩溃面归零**：可失败路径零 panic（RB-01/02），MCP/插件锁 poison-safe（RB-10），深输入不栈溢出（DOC-01）。
2. **数据不丢**：全量原子写（RB-03）+ 崩溃恢复 + 三方差异（已有，需补原子化）。
3. **输出与源等价**：导出降级必须显式告警（RB-06），PDF 子集化失败不得错字（EXP-01）。
4. **可中断**：所有长任务可取消、有超时（RB-04/05）。
5. **可观测**：`tracing` + 结构化日志 + 关键耗时（RB-11）。
6. **可访问与可读**：焦点环 + 双主题对比度 + 标签完整（§8.10）。
7. **插件不可越界**：至少安装期显式告知 + 路径包含校验 + 资源上限（PLG-01/02/03/04）。

---

## 8 · UI 美化方向（完整方案）

> 本节是"用户说的 UI 太基础、不能用"的**完整应答**。目标不是"换个配色"，而是把界面从"egui 默认外观"升级为**有设计语言的现代专业工具**。

### 8.1 现状判定（先承认事实）

**技术栈真相**：`vb_app`/`vb_ui` 仍基于 **`egui 0.35` + `eframe 0.35` + `vello 0.10` + `wgpu 29`**（`crates/vb_app/Cargo.toml:35,39,40,41`；`vb_ui/Cargo.toml:10`）；`sable`(GPUI) 只在 `vb_kit`/`vb_shell`，**仅 1 个面板（能力台账）迁完**，旧宿主 29+ 面板全在 egui。**结论：本轮美化必须落在 egui 栈，且用"令牌先行"的方式做，让未来 sable 迁移直接复用数值。**

**实测短板**（截图 `tools/baselines/ui/100/first.png`、`panels7.png`、`home.png` + 代码）：

| 症状 | 证据 | 观感后果 |
|---|---|---|
| 颜色令牌只有 **17 个**，文字 3 档 | `vb_ui/src/theme.rs:127` | 层次扁平，所有东西"一个灰" |
| 深色**完全没有阴影** | `theme.rs:421-440`（dark→`Shadow::NONE`） | 面板/浮层/画布糊成一片，无深度 |
| 字号只有 6 档，`Heading` 仅 **15px** | `theme.rs:491-519` | 无标题层级，像"控制台"不像"工具" |
| 无 elevation/state-layer 体系 | 全 `theme.rs` | 悬停/按下/选中差异弱，交互无反馈 |
| 圆角虽已 6/8/12，但按钮全局去框 | `theme.rs:522` `button_frame=false` | 未走自定义组件的按钮**掉成裸按钮** |
| 图标已是 Lucide（亮点）但只有 Regular 一套、不可单独上色 | `vb_ui/src/icons.rs:348,380` | 激活态辨识弱 |
| 无空/加载/错误态、无骨架屏 | UI-07 | 首次使用"一片黑"，像坏了 |
| 焦点环不存在 | UI-03、`theme.rs:469` | 键盘不可用，无障碍为零 |
| 启动器是朴素列表 | `home.png` | 第一印象即"业余" |
| 工具栏挤满文字标签 | `first.png` 底部 | 拥挤、无呼吸感 |
| 中英混排基线未校准、字体不随包 | UI-13、UI-14 | 文字"发糊/跳基线" |

### 8.2 视觉基调（明确一个方向，不含糊）

**方向："精密仪器（Precision Instrument）"——石墨灰机身 + 单一高饱和功能色 + 发丝级几何 + 分层材质。**

- **画布是主角**：面板用低压石墨灰阶，画布外更暗；**任何抢画板戏的"好看颜色"都是错的**。
- **分层而非描边**：深色靠 **elevation 阶梯 + 顶部高光发丝线** 表达层级，而不是靠灰色描边堆叠（现状的病根）。
- **单一功能色**：`#0D99FF` 仅用于"选中/激活/焦点/可交互强调"；**不做装饰**。
- **克制动效**：只做"状态反馈"（hover/press/focus/展开），不做装饰性动画。
- **可读性优先**：标签给全、数字用等宽数位、密度可切换。

> 与项目 14/22 篇一致，但**在"材质分层/中性色阶/状态层/密度模式/可访问性"四点上前推**（这是现状最薄之处）。

### 8.3 设计令牌扩展（从 17 → 完整系统）

**原则**：`docs/design/assets/vb-ui-tokens.json` 仍是唯一真相；`theme.rs` 由 `tokens_sync` 门禁锁定；新令牌必须同步进 JSON（否则 CI 红）。

#### 8.3.1 中性色阶（新增，替代"只有 3 档文字色"）

深色（机身，从深到浅）：`N0 #0E0E10` → `N1 #161618`（画布外）→ `N2 #1C1C1E`（画布）→ `N3 #232326`（面板底）→ `N4 #2A2A2E`（面板）→ `N5 #313136`（凸起/输入）→ `N6 #3A3A40`（悬停）→ `N7 #45454C`（按下）→ `N8 #55555E`（分隔强）→ `N9 #6E6E78`（弱文字）→ `N10 #9A9AA4`（次文字）→ `N11 #C7C7CE` → `N12 #FFFFFF`（主文字）。

浅色：反向独立调校（`#F7F7F8 / #FFFFFF / #F1F1F3 / #E8E8EB / #DDDDE1 / #1B1B1F …`），**不是深色的反相**（沿用 `theme.rs:603 light_is_not_an_inversion` 口径）。

#### 8.3.2 材质 / Elevation 阶梯（新增）

| 层 | 深色底 | 顶部高光发丝 | 阴影 | 用途 |
|---|---|---|---|---|
| L0 画布 | `N1` | 无 | 无 | 画板外区域 |
| L1 面板 | `N3/N4` | `rgba(255,255,255,.05)` 1px | 无（靠层次） | 工具箱/坞/状态栏 |
| L2 凸起 | `N5` | 同上 | `0 1px 2px rgba(0,0,0,.35)` | 输入框/Tab 选中/下拉 |
| L3 浮层 | `N6 + 8% accent?不` | 同上 | `0 8px 24px rgba(0,0,0,.45)` | 对话框/菜单/命令面板 |
| L4 提示 | `N7`，表面 92% 不透明 | 同上 | `0 4px 16px rgba(0,0,0,.5)` | tooltip/浮动工具条/数值浮层 |

> **关键修复**：`theme.rs:421-440` 深色 `Shadow::NONE` 改为上表三档阴影（这是"扁平没层次"的直接原因）。

#### 8.3.3 状态层（State Layer，新增）

统一用 **alpha 叠加于基础色**，不再靠换灰阶常量：
- hover `rgba(255,255,255,.06)`（浅色 `rgba(0,0,0,.04)`）
- press `rgba(255,255,255,.10)`
- selected `accent @ 14%`（`#0D99FF24`）
- focus ring `accent @ 100%`，1.5px 外描边 + 内侧 `rgba(0,0,0,.4)` 隔离环
- disabled：前景 `N9`，容器不变（**不降饱和整体**）

#### 8.3.4 功能色扩展

- `accent` `#0D99FF`（保留，两套共用）
- `accent-hover` `#3AAEFF`、`accent-press` `#0B87E5`、`accent-subtle` `#0D99FF24`、`accent-border` `#0D99FF80`
- `danger/warn/success` 保留；新增 `info`（与 accent 区分，用于非破坏提示）= `#5E5CE6`（浅色 `#4B49CC`）
- 语义色（智能参考线品红等）**保持不可变**，`guides-smart #FF00FF` 不动。

#### 8.3.5 圆角 / 描边 / 间距（在既有基础上补齐）

- 圆角：`xs 3 / sm 4 / md 6 / lg 8 / xl 12 / pill 999`。
- 描边：`hairline 1`（分隔/控件边）、`strong 1`（面板边界）、`focus 1.5`。
- 间距：沿用 4 基数；**新增密度档** `compact`（行高 24）/ `comfortable`（行高 28），**默认 comfortable**，设置项持久化。

### 8.4 排版系统（修复"发糊/跳基线/无层级"）

| 令牌 | 字号/行高/字重 | 用途 |
|---|---|---|
| `display` | 24/32/600 | 启动器标题、空态主标题（**现状缺**） |
| `title` | 15/22/600 | 对话框标题、面板组标题 |
| `body-strong` | 13/20/600 | 分组头/选中项 |
| `body` | 13/20/400 | 默认正文 |
| `label` | 12/18/500 | 字段标签、图层行 |
| `caption` | 11/16/400 | 状态栏、tooltip 副行 |
| `mono` | 12/18/400 | 十六进制、代码，**启用 tabular-nums** |

**动作**：
1. **字体随包**：`assets/fonts/` 内置 `Inter Regular/Medium/SemiBold` + `MiSans Regular/Medium`（约 +2.5MB）；fallback `Inter → MiSans → 系统 CJK`（消 `UI-14`）。
2. **修 `cjk_tweak()`**：`fonts.rs:211-213` 恒等变换 → 按实测基线偏移校准，验收 `宽度 W 320 px` 同一基线无跳变（消 `UI-13`）。
3. **数字对齐**：数值框统一 tabular-nums，小数点对齐。
4. **字重实现**：为每个字重建独立 `FontFamily`（`vb-ui-regular/medium/semibold`），`FontId::new(size, Family)` 取字重（egui 的 `FontId` 无字重的既有绕法）。

### 8.5 图标系统（在 Lucide 基础上补齐）

- 三档尺寸：`14`（面板内）/`16`（工具条）/`20`（底部工具条/浮动条）；描边统一 `1.5`。
- **新增 Filled 变体**用于激活态（工具箱当前工具、图层类型），补 `icons.rs:348` 只有 Regular 的缺口。
- **支持单独着色**：激活 `accent`，危险 `danger`（改 `icons.rs:380` 的"不单独上色"约束为"仅激活/危险态上色"）。
- 补专业图标（Lucide 缺）：图层/编组/画板/冻结块/剪切蒙版/符号/切片——自绘 SVG 追加私有字体。
- tooltip 强制 `名称 (快捷键)`（已有门禁保持）。
- 验收：`all_icons_resolve` 扩展 + 新增 filled 解析断言。

### 8.6 组件规范（把 5 个扩到 20 个，逐个定死）

> 现状只抽了 `ToolButton / NumField / ColorField / SectionHeader / PanelTabs`（`components.rs:12-23`），其余裸 egui。本轮补齐：

| # | 组件 | 规格要点 |
|---|---|---|
| 1 | `ToolButton` | 24/32 两档；hover 80ms；press scale 0.94 弹簧回弹；激活 `accent-subtle` 底 + 图标 accent；长按 200ms 展开同组（小三角角标） |
| 2 | `IconButton` | 20/24；无文字；必须有 tooltip |
| 3 | `NumField` ⭐ | scrubby 拖标签改值（光标 col-resize）+ `↑/↓` 步进 1、`Shift` 步进 10 + 表达式（`320/2`）+ 混合态斜纹 + 合并一次 undo |
| 4 | `TextField` | 统一高度（派生制）、清晰占位色、聚焦环 |
| 5 | `Select`/`Combo` | 凸起材质 L2、下拉菜单 L3、键盘 ↑↓/Enter/Esc |
| 6 | `ColorField` | 色块 + hex 并排；点击紧凑取色器 / `Alt+点击` 完整；HEX/RGB/HSL/CSS 变量四态 |
| 7 | `Slider` | 圆点手柄（保留 `HandleShape::Circle`）+ 值标签 + 双击复位 |
| 8 | `Checkbox`/`Radio`/`Switch` | 自定义绘制（非裸 egui），选中 accent，120ms |
| 9 | `SectionHeader` | 可折叠，`caption/600`，整行点击，状态持久化 |
| 10 | `PanelTabs` | 选中 `accent-subtle` 底 + accent 下划线 2px **滑动 200ms** |
| 11 | `LayerRow` | 行高 24/28（密度）；图标 14 + 名称 12 + 右侧 👁🔒（hover 显形）；选中整行 `accent-subtle`；双击就地改名 |
| 12 | `Card`/`GroupBox` | `lg` 圆角 + L2 材质 + 内边距 `S4` |
| 13 | `Separator` | hairline，颜色 `N8`，缩进对齐内容 |
| 14 | `Badge`/`Chip` | pill 圆角，`caption` 字，用于状态栏/台账三态 |
| 15 | `Toast` | 右下，滑入 120ms，TTL 信息 2.5s/告警 5s/错误 10s，错误可选中复制 |
| 16 | `EmptyState` | 图标 + `display` 标题 + `body` 引导 + 主行动按钮（**新增，消 UI-07**） |
| 17 | `Spinner`/`Progress` | 不确定 pill 1.2s 循环；确定态线性 + 完成对勾 pop 120ms |
| 18 | `ValueOverlay` ⭐ | 画布浮动数值条：L4 材质 + 圆角 6 + `caption` 白字，显示 `X Y ΔX ΔY W H ∠` |
| 19 | `CommandPalette` ⭐ | `Mod+K`，模糊搜索（拼音首字母容错），下滑 120ms，背板 fade 80ms，Esc 100ms 退出 |
| 20 | `Dialog` | 标题 title/内容 body/底部按钮右对齐；出场 8px + fade 120ms；Esc=取消、Enter=确认 |

⭐ = 最高性价比四件：`NumField`（手感）、`ValueOverlay`（AI 心智）、`CommandPalette`（可发现性）、`ColorField`（改配色效率）。

### 8.7 布局重构（解决"信息架构混乱、太挤"）

```
┌──────────────────────────────────────────────────────────────────────────┐
│ 文件 编辑 对象 文字 选择 效果 视图 窗口 帮助                    ─ □ ×     │ 菜单 32
├──────────────────────────────────────────────────────────────────────────┤
│ [当前工具] ▸ X Y W H ∠  ⬚  ■填充 □描边  100%   ← 随工具/选区变化          │ 控制条 40
├────┬──────────────────────────────────────────────────┬──────────────────┤
│ 工 │  ▏标尺(上)                                        │ ┌属性┬图层┬令牌┐ │
│ 具 │──────────────────────────────────────────────────│ │              │ │
│ 箱 │        ┌───────────────────────────────┐          │ │  位置        │ │
│ 44 │        │  画板 Hero 1440×900           │          │ │   X 120 Y 80 │ │
│    │        │   ┌──────┐   ┌──────────┐     │          │ │   W 320 H180 │ │
│    │        └───────────────────────────────┘          │ │  外观 …      │ │
│    │                    画板外略暗                       │ │  ▸ 布局      │ │
│    │        ╭────────────────────────────────╮         │ │  ▸ 交互      │ │
│    │        │ ➤ ▭ ◯ T P ╲ ✋ Z  ⌖ 100% ▾      │         │ │  ▸ 导出      │ │
│    │        ╰────────────────────────────────╯         │ │              │ │
├────┴──────────────────────────────────────────────────┴──────────────────┤
│ ◀ 1/4 ▶ │ 1440×900 px │ 66% ▾ │ ⌖ 320,180 │ 提示文本…                     │ 状态 28
└──────────────────────────────────────────────────────────────────────────┘
```

| 区域 | 规格 | 相对现状的变化 |
|---|---|---|
| 菜单栏 | 32 | **移除工具按钮**（挪到画布底部浮动条） |
| 控制条 | 40 | **新增**（AI 招牌：随工具/选区变化，8 种工具态） |
| 工具箱 | 44（单列）/64（双列） | 图标 24，比现状紧凑；长按展开同组 |
| 右侧坞 | 280（240–420 可拖） | **改 Tab 分组**，不再纵向堆叠；`<1200px` 自动折叠为 40px 图标条 |
| 浮动工具条 | 底部居中，40，圆角 12，L4 材质 96% | **新增**（Figma UI3 标志设计） |
| 状态栏 | 28 | 保持；窄窗分级折叠（消"状态栏溢出"遗留） |
| 画布 | 自适应 | 画板外 `N1`；画板白底可设色；顶部标尺 |

> **停靠策略**：现状自研 `DockSide` 不支持自由拖拽（UI-05）。**先做"Tab 分组 + 可调宽 + 可折叠"**（满足 90% 需求，风险低）；**自由拖拽停靠作为独立项**，评估引入 `egui_dock` 0.20（MIT，支持 egui 0.35）——若引入，需走 ADR 并核对许可与 ACL-1.0 合规。

### 8.8 动效与手感（对齐 doc22 §6，落到 egui）

- **时长四档**：`INSTANT 0`（画布一切几何）/`HOVER 80ms`（悬停、参考线 fade、tooltip）/`STATE 120ms`（选中、浮层出场、Tab 内容交叉淡化）/`PANEL 200ms`（折叠、Tab 下划线、主题切换）。
- **两条弹簧**：`SPRING_SNAPPY`（按下回弹、手柄吸附）、`SPRING_SOFT`（面板拖拽跟手）。
- **reduced-motion / 动效总开关任一关 → 全部直通**（已有 `theme.rs:346-368`，扩展到新组件）。
- **光标体系**：现状仅 `Space→Grab`（`cursor.rs`），补默认/文本 I 型/抓取/各向缩放/旋转弯曲（egui 不支持图像光标，退化为对角箭头并**在测试中钉住妥协**，sable 侧再回归图像光标）。
- **命中区 ≥ 12px**（屏幕像素，DPI 感知）；拖动阈值 3px 启动。

### 8.9 状态设计（新增，最影响"能不能用"的观感）

| 状态 | 设计 |
|---|---|
| **空态** | 无选区：属性面板显示**画板属性** + 引导（"双击进入文字 / 按 V 选择"），**不许白板**；图层空：图标 + "导入 HTML 或拖入素材" + 主按钮 |
| **加载态** | 打开工程：骨架屏（面板轮廓 + shimmer 1.2s）；导出：状态栏进度 pill（不确定 1.2s 循环） |
| **错误态** | 内联错误条（`danger` 左描边 + 图标 + 详情可展开）；破坏性操作二次确认；错误 toast 可复制 |
| **首次使用** | 启动器空态显示"新建/打开/模板"三卡 + 最近项目网格（缩略图，非纯文本列表） |
| **禁用态** | 前景 `N9`，容器不变，tooltip 说明原因 |

### 8.10 无障碍（当前最大空白，必须补）

1. **焦点环真接线**：`stroke::FOCUS` 从"active 态描边"改为**键盘 focus ring**（`theme.rs:469`）；外描边 accent 1.5px + 内侧隔离环。
2. **Tab 序**：所有可交互控件入 Tab 序；`F6` 循环面板区、`Ctrl+F6` 反向；`Esc` 逐级退出（浮层→面板→画布）。
3. **自绘控件补 `WidgetInfo`/`with_label`**：`components.rs:320-326` 等为读屏预留语义名。
4. **对比度门禁扩到深色**：`theme.rs:804/826` 现只覆盖浅色 → 深色同样跑 WCAG AA；新增"焦点环/选中底/禁用文字"对比度断言。
5. **reduced-motion** 系统探测接入 `vb_platform`（现为显式开关，需补系统读取）。

### 8.11 双宿主与迁移策略（不返工）

**令牌先行**：所有新令牌与组件规格先落 `vb-ui-tokens.json` + `theme.rs`，**egui 侧立即生效**；sable 侧 `vb_kit` 从同一 JSON 注入（`doc22 G-UI1 tokens_sync2`），**数值零复制**。这样本轮美化**不会在换宿主时白做**。

- 本轮（egui）：扩令牌 + 20 组件 + 布局重构 + 状态设计 + 无障碍。
- 迁移期：`vb_session` 投影层同步长出新面板所需状态（COUP-R6），面板从 egui 逐个搬到 sable。
- 换默认：按 doc22 `G-SWITCH` 判据。

### 8.12 UI 门禁（新增，防回归）

| 编号 | 门禁 | 机制 |
|---|---|---|
| G-UI-A | 令牌同步 | `tokens_sync` 扩展到中性色阶/elevation/state-layer/排版四表，与 JSON 逐值比对 |
| G-UI-B | 硬编码颜色扫描 | 扩展到 `vb_kit`/`vb_shell`；白名单仅令牌定义文件 |
| G-UI-C | 文案走 `t()` | **扩到 `vb_app`**（消 UI-01 门禁漏洞），白名单仅日志/调试 |
| G-UI-D | 焦点走查 | 遍历面板断言 Tab 可达数 = 可交互控件数 |
| G-UI-E | 对比度（深+浅） | 双主题 WCAG AA：正文/次级/禁用/语义/焦点环 |
| G-UI-F | 组件尺寸 token 化 | 扫描 `components.rs` 无裸魔法尺寸（除白名单刻度） |
| G-UI-G | ui_shots 基线 | 每面板深浅双主题基线；本轮美化后**一次性重刷基线并人工走查** |

---

## 9 · 落地分期（每条都有归属，不留"以后"）

> 铁律：每轮结束**可演示、可回滚、门禁真实运行并引用退出码**。数值为粗估工作日。

### S0 · 安全与崩溃面（P0，最高优先，≈4–6 天）
- DOC-01 递归深度上限（全链）
- DOC-02 undo panic
- EXP-01 PDF 子集化错字
- PLG-01 插件沙箱（**先落"安装期显式告知 + Job Object 最小版"**）+ PLG-03 entry 路径包含校验
- AGT-01 MCP poison-safe
- 验收：上述五条各有单测/注入用例；`pwsh tools/ci.ps1 -Light` 全绿。

### S1 · 鲁棒性与可观测（P1，≈5–7 天）
- RB-01/02/03/04/05/06/08/09/10 逐条扫尾（§3 全部 `unwrap/expect` 与 IO 路径）
- EXP-02 `--max-wait` 接线 + 取消
- EXP-03 WS/HTTP 超时与重连
- RB-11 引入 `tracing` + 关键路径耗时
- 验收：`ci.ps1` 全量；新增坏输入/超时/取消测试。

### S2 · 性能（P1，≈4–6 天）
- PERF-01 文本管线（缓存 + 去 O(n²)）
- PERF-02 PDF 两遍渲染
- PERF-03 导出 clone
- PERF-05 每帧样式注入
- 验收：基准降幅入 dev stats；像素对拍门禁（`canvas_parity`）不回归。

### S3 · UI 地基（令牌 + 排版 + 材质，≈5–7 天）
- §8.3 令牌扩展（中性色阶/elevation/state-layer/功能色）+ JSON 同步 + 门禁
- §8.4 字体随包 + `cjk_tweak` 校准 + tabular-nums
- §8.2 深色阴影/材质分层
- 验收：G-UI-A/E；截图基线重刷。

### S4 · UI 组件与布局（≈8–12 天）
- §8.6 20 组件（先 ⭐ 四件）
- §8.7 布局重构（控制条/浮动工具条/Tab 分组坞/标尺预留）
- §8.9 状态设计（空/加载/错误/首启）
- 验收：G-UI-D/F/G；Win 1024×640 与 150% DPI 走查。

### S5 · 无障碍与国际化（≈5–7 天）
- §8.10 焦点环/Tab 序/`WidgetInfo`/深色对比度/reduced-motion 探测
- UI-01 i18n：先补门禁（扫 `vb_app`）→ 批量抽 `zh.ftl`/`en.ftl`
- 验收：G-UI-C/D/E；焦点走查表。

### S6 · 工程表面与差距收口（≈4–6 天）
- AGT-02/03 CLI 退出码与 `--json` 错误结构；AGT-06/07 patch 事务与 `dry-run`
- PLG-02/04/06 授权哈希/stdout 上限/版本协商；PLG-05 poll 后台化
- COUP-01..09 耦合收口（提取 `vb_textmeasure`、实例化字体注册表、图片缓存清理、契约类型化）
- EXP-07/08/09/10/12/13 导出与配置一致性
- 验收：§5/§6 规则全部有对应测试或扫描；CLI 契约测试；`ci.ps1` 全绿 + `fmt/clippy -D warnings`。

**全局完成定义**
- [ ] §3 全部 P0/P1 闭环，P2 有归属（本文件即台账）
- [ ] §5/§6 规则有可自动验证的门禁或测试
- [ ] §8 令牌/组件/布局/状态/无障碍落地并过 G-UI-*
- [ ] 全量 `fmt --check + clippy -D warnings + test + build --release` 全绿
- [ ] UI 基线重刷 + 深浅双主题人工走查表入 PR
- [ ] 磁盘：迭代收尾清理 `target/debug`，报告释放量（`target/release` 保留）

---

## 10 · 附录

### 10.1 `vb_app/src` 最大文件（重构优先级）

| # | 文件 | 行数 |
|---|---|---|
| 1 | `app/timeline.rs` | ~1652 |
| 2 | `app/commands.rs` | 1476 |
| 3 | `shell.rs` | 1295 |
| 4 | `app/canvas.rs` | 1130 |
| 5 | `app/health.rs` | 1114 |
| 6 | `app/dialogs.rs` | 983 |
| 7 | `app/toolbar.rs` | 924 |
| 8 | `app/panels/properties.rs` | 917 |
| 9 | `launcher.rs` | 897 |
| 10 | `app/panel_dock.rs` | 855 |

### 10.2 复现与验证命令

```bash
# 同步
cd 'e:/平日资料/GitHub/VellumBench'
git fetch --all --prune && git pull --ff-only

# 轻量门禁（fmt · clippy -D warnings · 全量测试 · 术语 · 输出校验 · 三端一致性）
pwsh tools/ci.ps1 -Light

# 全量门禁（含画布↔导出像素对拍；UI 截图基线报告模式）
pwsh tools/ci.ps1
pwsh tools/ci.ps1 -UiShots

# 定向
cargo test -p vb_doc -p vb_html -p vb_css -p vb_layout
cargo test -p vb_kiln -p vb_export -p vb_browser
cargo test -p vb_agent -p vb_plugin -p vb_platform
cargo test -p vb_ui -p vb_app -p vb_kit -p vb_shell
```

### 10.3 证据锚点索引（`612a405`）

| 主题 | 关键锚点 |
|---|---|
| UI 技术栈 | `crates/vb_app/Cargo.toml:35,39-41`；`crates/vb_ui/Cargo.toml:10`；`Cargo.toml:57-59`；`crates/vb_kit/src/capabilities_panel.rs:17-29`；`crates/vb_shell/src/main.rs:22-42` |
| 主题令牌 | `crates/vb_ui/src/theme.rs:57-148,262-334,399-532,421-440` |
| 字体 | `crates/vb_ui/src/fonts.rs:33-41,104-143,211-213,342-350` |
| 图标 | `crates/vb_ui/src/icons.rs:262-341,348,380,433` |
| 组件 | `crates/vb_ui/src/components.rs:12-23,33,254-265,320-326,1245,1319` |
| 面板/停靠 | `crates/vb_app/src/app/dock_layout.rs:3-4,63-70`；`panel_dock.rs:24-26,49-61` |
| 文本管线 | `crates/vb_render/src/text.rs:23-34,117-197,248,329,479,552` |
| 导出 | `crates/vb_kiln/src/pdf.rs:63-72,664-682,815-872,1085-1090,1114,1189`；`crates/vb_export/src/svg.rs:183-217,274,483-487` |
| 浏览器车道 | `crates/vb_browser/src/ws.rs:216,337-339`；`cdp.rs:137-142`；`httpc.rs:16-17`；`capture.rs:52,57-60,163-182,204-207,380-427,460-488` |
| 文档/undo | `crates/vb_doc/src/undo.rs:45-81,236`；`export.rs:23,155,372-389`；`import.rs:124,187,193,289,453,477`；`model.rs:308-342,401-433,499-516,504-511,585-601`；`collab.rs:288,340,345,354` |
| HTML 解析 | `crates/vb_html/src/lib.rs:27-78,193-198,263-292,365-389,392-447,449-558` |
| 布局 | `crates/vb_layout/Cargo.toml:15`；`src/lib.rs:298,357,370,406-413`；`src/calc.rs:121-199` |
| Agent/MCP | `crates/vb_agent/src/bin/vellum-mcp.rs:25-33,72,279-298,315-319,384-394`；`src/main.rs:136-157,209-238,377,448,692,871,499-503`；`src/patch.rs:1-4,154-161,231-482,490-517,541-618,644` |
| 插件 | `crates/vb_plugin/src/lib.rs:5-15,44-50`；`manifest.rs:243-257,270-276`；`auth.rs:145-152,165-190`；`process.rs:69-88,96-145,156,197-199,226-255,299-322`；`host.rs:196-203,493,502,580-583,662-692,713,755-798,861-876` |
| 平台 | `crates/vb_platform/src/traits.rs:14-67`；`crates/vb_app/src/launcher.rs:240-242,400-401,444` |

---

**一句话版本**：骨架别动，先把"会崩、会丢、会错字、会挂死、能越界"的五类问题在**当前这轮**关掉；再把 UI 从"egui 默认外观"升级为"精密仪器"——**令牌先行**、20 组件、Tab 分组布局、状态设计、无障碍，全部落进 `vb-ui-tokens.json`，保证换宿主不返工。
