# 20 - 迭代计划:AI 可编辑性二轮——浏览器布局 × Kiln 矢量写入(零断字/双图层/多画板)

| 项 | 内容 |
|---|---|
| 文档版本 | v1.0(2026-09-19,全权授权自查模式) |
| 状态 | 已裁决,随本笔记直接开工(N1–N6) |
| 上游 | 用户二轮七点诉求(§1.1);一轮成果 design/19 + ADR-0020(v0.7.0,门禁 26/26) |
| 新增决策 | **ADR-0021:AI/PDF 矢量写入改为「浏览器布局 → Kiln 写入器」(DOM 快照路线)** |
| 验收源 | 同一轮:`E:\平日资料\Kiln验收参考项目`(不新增) |

---

## 1. 执行摘要

一轮门禁证明了**像素保真**(G1 99.93 / G2·G3 99.37),但用户在 Illustrator 里实测 .ai 暴露了**结构保真**的全面溃败。今日取证(解剖 `Kiln-AI/a4-front.ai` 内容流):

- 首个文本块:**一个字形一个 `<EF> Tj`**,且每 1–2 个字形切换字体资源(/F18→F19→…),全文件 **17 个 Type3 字体、190 个显示算子 / 24 个 BT 块**;
- **86 个 clip(`W n`)、122 对 q/Q**——逐元素蒙版的直接来源;
- **21 个 SMask + Type3 组合**——Illustrator "未知的阴影类型/未知的图像结构" 报错的来源。

结论:Chrome printToPDF 是"给人看/给打印用"的 PDF,不是"给 Illustrator 编辑用"的 PDF。**修补它(后处理合并 Tj、剥离 clip)治标且救不回被栅格化的文字**。本轮裁决:**矢量写入换轨到 ADR-0021 DOM 快照路线**——浏览器只负责布局与测量(它最强的部分),Kiln 的 PDF 写入器负责结构与矢量(车道 K 一直最强的部分,一轮里它只是输在布局)。文字从根上变成整行连续 CID 文本,蒙版/Type3/SMask 报错源从根上不再产生。

## 1.1 七点诉求 → 验收翻译

| # | 原文要点 | 验收翻译(机器断言/人工验收) |
|---|---|---|
| 1 | AI 打开报"未知的阴影类型/未知的图像结构" | A1:内容流 0 个 Type3、0 个原生 Shading;SMask 仅图像 alpha;Illustrator 人工打开零报错(验收包) |
| 2 | 编组变剪切蒙版,逐元素自带蒙版 | A3:clip(`W n`)计数 ≤ 圆角图片数 + 显式 overflow 数(DOM 侧统计同值对拍) |
| 3 | 文字逐字断开("G","E","O"…),最严重 | A2:文本显示算子数 ≤ DOM 文本行数 × 1.2;人工:Illustrator 双击可整串改字 |
| 4 | 部分文字转曲(中秋最严重) | A4:pdfium 抽取 vs DOM 文本(一轮 G4 口径),**festival-zhongqiu delta ≤ 3%**(blend 文字救活) |
| 5 | GitHub/本地仓库垃圾(测试图、提取文本等) | A8:`git ls-files` 无产物类垃圾;~72MB 入库产物删除并 push |
| 6 | AI 拆两个图层:背景 + 内容 | A5:OCG 计数 = 2,命名"背景/内容" |
| 7 | 多画板:A4 正反面 → 单文件双画板 | A6:该案例 PDF 页数 = 2;人工:Illustrator 显示两画板 |

## 2. 根因分析(逐点,今日取证数据)

| # | 现象 | 根因(Chrome printToPDF 行为) |
|---|---|---|
| 1 | 打开报错 | Chrome 用 **SMask(/Luminosity)+ Type3 字形 CharProcs + 原生 Shading** 表达半透明/描边大字/渐变;Illustrator 的 PDF 兼容层不认这些组合 |
| 2 | 蒙版泛滥 | 每个元素(圆角/overflow/blend)都发射 `q … W n`;86 clip/122 q-Q 全部被 Illustrator 解释为剪切蒙版 |
| 3 | 逐字断开 | letter-spacing≠normal 时 Chrome **按字形逐个 `<glyph> Tj` + `Td` 位移**;描边/合成大字再拆进 **Type3(每 1–2 字形一个字体资源)** → 每字一个对象 |
| 4 | 文字转曲 | `mix-blend-mode` 层文字打印时**整层栅格化**(一轮已知 caveat,当时以 G2 像素门背书放行;本轮按用户新优先级救活) |
| 5 | 仓库垃圾 | 历史批次目录(bench/pdf-fidelity×2、artboard-baseline/m1)+ 一轮入库的 26 张基线 PNG 与多份报告,共 ~72MB |
| 6 | 无图层 | Chrome PDF 无 OCG |
| 7 | 单画板 | printToPDF 单页;CLI 也无多源入口 |

## 3. 方案裁决:ADR-0021 DOM 快照路线

### 3.1 两条路线对比

| | A:Chrome PDF 后处理 | **B:DOM 快照 → Kiln 写入(裁决)** |
|---|---|---|
| 原理 | pikepdf/自写解析器改 Chrome 产物 | 浏览器给布局,Kiln 现有 pdf.rs 写矢量 |
| 断字(3) | TJ 数组合并(可行但 Type3 字体仍碎) | **根除**:整行 `<…> Tj` + `Tc` 表达 letter-spacing |
| 蒙版(2) | clip 剥离/归并(启发式,易碎) | **根除**:仅显式裁剪才发 clip |
| 报错(1) | SMask/Type3 结构替换(高危) | **根除**:从不产生这些结构 |
| 转曲(4) | **救不回**(位图里没有文字) | **可救**:文字永远文本;blend 视觉近似 |
| 图层(6)/画板(7) | 可后处理注入 | **原生**:OCG/多页本来就是 vb_kiln 能力 |
| 代价 | 中(自写 PDF 解析 ~1 周) | 中大(DOM 采集 + 映射 ~1 周,复用面大) |

裁决:**B 为主**。`--engine browser` 的 PDF 快速通道(printToPDF)保留为 `--vector chrome`(打印/阅读用途);**AI 一律走 DOM 快照**(`--vector dom`,默认)。

### 3.2 架构

```
kiln-cli export --format AI [--source a.html --source b.html]   ← 多源=多画板
  └─ vb_browser:加载页面(十要素 settle)→ 注入 domsnap.js
       采集 PaintList(JSON):
        · 元素序:z 排序(含 zIndex 稳定序)、overflow/圆角/transform 矩阵
        · 盒:背景色/渐变(computed 已规范化)/边框/圆角/box-shadow/opacity
        · 图:img.src + 显示矩形(SVG 内联元素 → outerHTML 随行带回)
        · 文本:每个文本节点按 Range.getClientRects 分行 →
          run{字符串, family, size, weight, color, 行矩形, baseline, letter-spacing}
        · 图层分类:面积 ≥60% 画板或 z 最底的封面元素 → 背景;其余 → 内容
        · blend/filter/backdrop 元素 → 标记(整页截图按 rect 裁剪作位图降级;
          **文字例外**:注入 CSS 去 blend,填充色从截图采样)
  └─ vb_kiln 新模块 dompaint:PaintList → DrawList
       · 文本:浏览器行矩形为锚,整行 CID `<…> Tj`;Tc = (行宽 − Σadvance)/字符数
         (反推字距,行末与浏览器对齐)
       · 渐变:linear/radial → 现有栅格化位图路线(pdfium/AI 兼容)
       · 圆角/边框/图片:现有 path/图像能力;clip 仅显式裁剪
       · OCG×2(背景/内容);多源 → 多页(Catalog Pages kids)
  └─ 现有 write_pdf + AI9 头 → .ai
```

### 3.3 关键工程决定(自查轮)

| 轮 | 问题 | 裁决 |
|---|---|---|
| Q1 | 后处理还是重写写入? | **重写(方案 B)**:修补 Chrome PDF 治标且救不回转曲文字 |
| Q2 | 断字根除的技术保证? | 整行 run + CID Tj;letter-spacing 映射 PDF 原生 `Tc`;Type3 从不使用 |
| Q3 | 我们的字形 advance ≠ Blink,文字会漂吗? | 行锚点用浏览器矩形,`Tc` 反推行宽对齐;残余差异由 A7 视觉门(≥97)捕捉迭代 |
| Q4 | blend 文字救活的视觉代价? | 填充色从整页截图采样(混合结果色),差异记 warning;AI 场景编辑性优先(用户本轮实际调整了一轮的优先级,仅对 AI 生效;PNG/PDF 像素路线不变) |
| Q5 | 内联 SVG(A4 图标)? | v1 元素矩形位图降级 + warning;v1.5 用已有 usvg 解析 outerHTML 转真矢量(Stretch) |
| Q6 | box-shadow/text-shadow? | box-shadow:多层圆角矩形 radial 衰减位图(复用渐变栅格);text-shadow v1 近似描边或降级 warning,A7 盯分 |
| Q7 | 历史里的 72MB blob 要不要 filter-repo? | **不重写历史**(main 已公开、release 引用链);本轮普通删除 + gitignore;彻底瘦身列为 carry-forward 可选项 |
| Q8 | 多源 CLI 形态? | `--source` 允许重复(保持单源兼容);artboard 侧导出.py 后续跟进入口 |
| Q9 | 车道 K 定位变化? | 升格:其 PDF 写入器成为两条车道共用的矢量后端(布局来源=浏览器);"自研布局"仍服务编辑器与无浏览器兜底 |

## 4. 门禁(机器断言,新增 `bench/ai_struct.py`)

| 门 | 断言(pikepdf 解析内容流/对象表) |
|---|---|
| A1 | Type3 计数 = 0;原生 Shading = 0;SMask 仅挂图像 XObject |
| A2 | 文本显示算子数 ≤ DOM 采集行数 × 1.2(dompaint 附带行数元数据对拍) |
| A3 | clip 计数 ≤ DOM 侧显式裁剪数(圆角图片/overflow 元素计数,采集时随行) |
| A4 | pdfium 抽取 vs DOM 文本,全案例 delta ≤ 3%(中秋必须直过,无 caveat) |
| A5 | OCG = 2 且命名 背景/内容 |
| A6 | A4 正反面单文件页数 = 2 |
| A7 | pdfium 渲染 vs 车道 B PNG ≥ 97(承一轮 G2 口径) |
| A8 | `git ls-files` 无 png/pdf/jpg 产物类垃圾(白名单:docs 示意图除外) |

人工验收包(交付用户):`_验收导出2\`(AI × 若干 + 打开要点清单:零报错/双击整串改字/图层面板两层的可见性切换/双画板切换/中秋标题可编辑)。

## 5. 里程碑

- **N1 清库(先行,~0.5h)**:git rm bench/pdf-fidelity×2、artboard-baseline/m1、acceptance/baseline 26 PNG、旧报告(留最新 1 份);gitignore 收紧(bench 产物/基线/报告仅脚本);push。
- **N2 DOM 采集(~1.5d)**:domsnap.js + Rust 解析 → PaintList JSON;单测:行分组、渐变规范化串、图层分类、裁剪计数。
- **N3 Kiln 写入(~2d)**:dompaint( PaintList→DrawList )+ vb_kiln 扩展(多页 Catalog、OCG×2、Tc);`ai_struct.py` 门禁;A1–A5、A7 过;26 案例 AI 重导。
- **N4 多画板(~0.5d)**:`--source` 多值;A4 正反 → 单文件双页;A6。
- **N5 效果降级与救活(~1d)**:blend 文字采样色;box-shadow 位图;SVG 位图降级;中秋 A4 直过;Illustrator 人工验收包产出。
- **N6 收口(~0.5d)**:design/20 实施记录、README/CHANGELOG、版本 v0.8.0、Release、artboard 同步(setup_kiln v0.8.0)、记忆更新、push。

## 6. 风险登记

| # | 风险 | 缓解 |
|---|---|---|
| R1 | 字形 advance 与 Blink 漂移(文字基线/行末偏) | Tc 反推行宽;A7 视觉门迭代收紧 |
| R2 | Illustrator 对自研 PDF 兼容性未知 | 一轮车道 K PDF 结构同源(Type0/OCG/渐变位图);人工验收包第一时间回收反馈;保留 `--vector chrome` 退路 |
| R3 | DOM 采集覆盖缺口(伪元素/复杂堆叠上下文) | 采集覆盖率报告(未识别样式计数);渐近补 |
| R4 | 采集 JS 在超长页(19418px)性能 | 分块滚动窗口内采集(复用分块协议)或一次性(文本量小,实测定) |
| R5 | 多画板在 Illustrator 呈现不符预期 | PDF 多页=多画板是标准行为;人工验收确认 |
| R6 | 图层启发误分类 | 双图层先求有;`vb-layer` 注释锚点可显式指定(carry-forward) |

## 7. Carry-forward(本轮不做,入池)

- 内联 SVG 真矢量化(usvg 复用);`--vector chrome` 同样做 Tj 合并后处理(打印场景);
- text-shadow 矢量化;图层显式标记;filter 滤镜矢量化;
- 仓库历史彻底瘦身(filter-repo,可选);GIF/MP4 车道 B 化(一轮遗留);
- DOM 快照路线反哺车道 K 布局测试(对拍两布局引擎几何)。

---

*取证数据:2026-09-19 解剖 `Kiln-AI/a4-front.ai`(Chrome/Edg 153 产物):240KB 内容流、24 BT、190 显示算子、86 clip、122 q/Q、17 Type3、21 SMask。*
