# Vellum Bench · 绘台

> 用 Adobe Illustrator 的操作心智,编辑 100% 标准 HTML/CSS 文档;GPU(Vulkan) 原生渲染;Agent 可直接读写;内置导出核心 **Kiln** 原生直出 PNG / JPG / GIF / MP4 / SVG / PDF / EPS / Ai / PPTX 九格式(零浏览器依赖),并支持 HTML / PDF / AI / SVG **反向导入**。

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
| **`vb_layout` 文档流布局引擎**(taffy:flow/flex/absolute、inset/margin/padding、var()/calc()、authored flags 区分显式与推断几何) | ✅ |
| **文本引擎**:富文本段(字节区间样式)、`<br>` 换行、贪心断行 + 禁則(。、！？不入行首)、字重感知选字(fontique+swash)、@font-face 注册表 | ✅ |
| **CSS 动画时间轴**:@keyframes 导出期解析,animation 简写(var()/calc() 时序)+ cubic-bezier 求解;**GIF/MP4 主路 = animlane 车道 B 逐帧**(WPI 理论:真浏览器实时采样+墙钟重采样,人工评分 35→99.4);车道 K 静态求值为无浏览器兜底 | ✅ |
| **双车道导出(ADR-0020 + ADR-0022 路线分化)**:车道 B(浏览器,Rust 原生 CDP 驱动系统 Edge/Chrome,默认)——PNG=原生截屏、AI/SVG/EPS/PDF=DOM 矢量写入、GIF/MP4=animlane 逐帧;车道 K(自研,零依赖兜底)九格式 | ✅ |
| **AI 可编辑三轮(docs/design/21)**:G6 浏览器真值门禁(诚实尺:dom 89.5-96.7 / chrome 锚 98.6-99.8)、分段样式逐段整形(重点字号不丢)、矢量渐变 Pattern、圆角公式修复、svg 资产矢量导入、副产物出源目录 | ✅ |
| **Kiln 车道 K 原生九格式**:PNG(@1x-4x)· JPG · GIF · MP4 · SVG(真文本)· PDF(CID 中文真文本)· EPS · Ai · PPTX | ✅ |
| **PDF 可编辑质量**:Type0/CIDFontType2 子集嵌入 + ToUnicode(阅读器可选中复制,Illustrator 可改字)、OCG 图层、clip-path(Inset/Circle/Ellipse/Polygon)、渐变栅格化位图 + SMask 半透明、q/Q 仅旋转项(单层图形状态) | ✅ |
| **验收门禁(机器出分,禁止手写)**:[`bench/acceptance.py`](bench/acceptance.py) × 用户指定验收集(6 类 26 HTML):**G1** PNG vs 浏览器基线平均 **99.93**(最差 99.65,尺寸严格相等);**G2/G3** PDF/AI vs PNG 平均 **99.37**;**G4** 文本层(容差+栅格化 caveat);**G5** 字体 100% 嵌入 | ✅ |
| **外部格式导入(`kiln-cli import`)**:HTML 项目;PDF/AI(pdfium.dll 动态绑定,文本可编辑);SVG(usvg 纯 Rust:矩形/圆角/圆/椭圆/真实文本逐对象映射) | ✅ |
| **位图工具箱 `kiln-cli img`**:crop(box/trim-border)· stitch(vertical/horizontal+gap+bg)· blur(高斯)· pad · info | ✅ |
| `vellum-cli`:**10 个子命令** + `patch` 的 **16 种 op**(事务 + `base_rev` 乐观锁) | ✅ |
| **vellum-mcp**:MCP stdio Server(**10 工具**,JSON-RPC 2.0) | ✅ |
| GUI:Vulkan(wgpu 29)+ Vello 画布 | ✅ |
| 选择/矩形/椭圆/抓手、**Alt 复制、Shift 约束、框选(相交即选)** | ✅ |
| **8 手柄缩放(Shift 等比/Alt 中心)+ 角外圈旋转** | ✅ |
| **智能参考线**(边/中心对齐兄弟与画板,品红) | ✅ |
| 双击文本编辑、画板管理(新建/删除/改名)、层序(`Mod+[` / `Mod+]`) | ✅ |
| 设计令牌面板(CSS 变量改一处全站生效)、语义标签、链接/aria、flex 布局 | ✅ |
| **文件监听热重载**(Agent 改 HTML → 画布更新,基于 `notify`) | ✅ |
| **往返语料库 20 例**(L0 无损坏 / L1 字节幂等) | ✅ |
| **剪贴板 / 对齐六键 / 命令面板 Ctrl+K / 数值浮层 / Mod 禁用吸附**(注册表扩至 49 命令) | ✅ |
| **质量门禁**(`pwsh tools/ci.ps1` 一键:格式 / clippy `-D warnings` / 全量测试 / Agent 无头自检 / 硬编码颜色棘轮) | ✅ |
| **路径查找器**:四基本运算落地(flo_curves);扩展 6 运算留后续 | C1 完成 |
| **2026-09 全库审查修复**:命令层(artboards 派生同步/Move 环防护/Compound 原子回滚/Group 坐标重定基)、Agent 层(duplicate 深拷贝/对齐跨画板分组/MCP -32700)、导入层(CSS 字符串感知扫描/级联顺序/自定义属性大小写)、GUI 层(多画板世界坐标统一/Esc 取消还原/Alt 复制入 undo 栈),21 个回归测试锁定 | ✅ |

### 未落地 ⏳ —— 别按「已完成」读

| 缺口 | 现状(代码实测) | 计划 |
|---|---|---|
| **快捷键** | 注册表 75 命令 / 61 键位绑定(`commands.yaml` 同步,门禁锁定);02 篇目标 ~120 | P3 剩余批次 |
| **主题** | 深/浅双令牌 + 切换;vb_ui 已落 theme/fonts/icons/components/cursor | P2 收尾 |
| 画布真文本 | CPU 导出真字形已落地;画布仍 egui 近似(见 [ADR-0017](docs/adr/0017-canvas-text-approximation-v01.md));Parley 多行/双向留后续 | 复议中 |
| 路径查找器扩展运算 | 基础四运算已落,扩展 6 运算未开始 | C2 |
| 响应式断点 / 伪类编辑 | 未开始 | P3 后半 |
| SVG 导入边界 | 自由曲线路径以包围盒矩形近似 + 警告;渐变取中点色;filter/mask/clipPath 跳过 | 逐版补 |
| PDF 导入边界 | pdfium 对象取色 API 未暴露(统一近似色);路径以盒近似 | 随 pdfium |
| 门禁 5/10 | **5 性能基线** `tools/bench.ps1`(环形历史+回归告警);**10 三端一致性**(CPU vs SVG 像素容差)已接;6 术语扫描 / i18n ftt 内容未落 | P5 |
| 组件 / 时间轴 / CRDT | 未开始(1→100) | 不承诺档期 |

## Kiln 导出核心

```
Document(vb_doc) ─encode─▶ DrawList(vb_render,引擎中立)
                               │
                    ExportContext(守门+动画逐帧)
                               │
      ┌────────┬───────┬───────┼───────┬────────┐
    PNG/JPG   GIF     MP4     SVG     PDF/Ai   PPTX/EPS
   (CPU栅格) (逐帧)  (逐帧)  (真文本) (CID真文本) (真文本)
```

- **双车道(ADR-0020)**:`--engine auto`(默认)浏览器可用即走车道 B——Rust 原生 CDP(零新依赖,手写 WebSocket/HTTP)驱动系统 Edge/Chrome:PNG=整页截图(WPI 捕获协议十要素移植),PDF/AI=printToPDF(screen 媒体+精确纸张);浏览器缺席自动降级车道 K 并告警
- **保真度口径(诚实声明)**:历史 98.63 为「Kiln PDF→pdfium vs Kiln 自家 PNG」**自洽分**(闭环无浏览器真值);现行门禁以系统浏览器渲染为基线,分数全部由夹具机器生成([验收报告](bench/acceptance/)、[实施记录](docs/design/19-迭代计划-双车道保真导出重构.md))
- **AI 边界**:PGF 无公开规范,.ai=PDF 兼容流+AI9 头(Illustrator 可开、文字可改、矢量保留;无原生图层面板),详见 [可编辑性检查表](docs/ai-editability-checklist.md)
- **中文真文本**(车道 K):PDF 走 Type0/CIDFontType2 子集嵌入 + ToUnicode;SVG/PPTX 保留文字节点;不转曲

## 构建

```bash
cargo build --release
# GUI(Vulkan/wgpu,无独显自动回退)
target/release/vellumbench.exe
# Kiln 无头 CLI(可独立分发给 artboard 等下游)
target/release/kiln-cli.exe export --source <项目目录|index.html> --output out.pdf
# Agent CLI(无头,可进 CI)
target/release/vellum-cli.exe --doc examples/landing/index.html tree --json
```

## 快速体验(Agent 闭环)

```bash
vellum-cli --doc examples/landing/index.html tree --json
vellum-cli --doc examples/landing/index.html find --name "主标题" --json
vellum-cli --doc examples/landing/index.html patch ops.json   # set_text / set_style / move ...
vellum-cli --doc examples/landing/index.html export --artboard hero --format png --scale 2 --out hero@2x.png

kiln-cli export --source examples/poster --output poster.pdf --scale 2   # 九格式任选
kiln-cli import --source designer.ai --output restored/                  # PDF/AI(需 pdfium.dll)
kiln-cli import --source icon.svg --output restored/                     # SVG(纯 Rust)
kiln-cli img stitch --input a.png --input b.png --vertical --gap 8 --output merged.png
```

Agent 改 HTML → 用户在 GUI 里接着画;用户保存 → Agent 读增量。**双向通畅,永不锁定**。

## 设计文档(灵魂,先读)

| 文档 | 内容 |
|---|---|
| [docs/design/00](docs/design/00-需求拷问与产品定义.md) | 为什么存在、做什么、不做什么 |
| [docs/design/01](docs/design/01-核心概念与AI术语对照.md) | Illustrator 概念 ↔ HTML 对照(灵魂) |
| [docs/design/04](docs/design/04-文档模型与HTML序列化.md) | 场景图与序列化(灵魂) |
| [crates/vb_kiln/docs/README.md](crates/vb_kiln/docs/README.md) | Kiln 导出核心架构 |
| [CONTEXT.md](CONTEXT.md) | 术语表(单一真相) |
| [docs/adr/](docs/adr/) | 架构决策记录 |

## 三条不可妥协的原则

1. **HTML 是文档格式,不是编译产物** — 输出干净、可读、可 diff。
2. **Illustrator 心智不可妥协** — 快捷键、修饰键、术语零学习成本。
3. **Agent 是一等公民** — 用户能做的 Agent 都能做;元素稳定可寻址(`data-vb-id`)。

## License

MIT
