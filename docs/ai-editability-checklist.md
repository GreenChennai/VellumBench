# AI(Adobe Illustrator)导出可编辑性检查表

> 对应 design/19 §4.4 → **已被 ADR-0021 取代**:AI 一律走「浏览器布局 → Kiln 矢量写入」
> (DOM 快照路线),不是车道 B 的 Chrome printToPDF。
>
> **AI 头**:只写 `%%AI8_CreatorVersion` / `%%Creator` 两行注释,且必须在写入器
> 内部落笔(`pdf::write_pdf_head`)——事后往首行后插入会让全表 xref 偏移整体错位。
> **严禁伪造 `%AI9_PrivateDataBegin`**:Illustrator 会把后续 PDF 对象当成私有
> 阴影/图像结构,报「遇到未知的阴影类型/未知的图像结构」。

## 格式现实(边界声明)

- 现代 .ai = PDF 兼容流 + 私有 PGF 数据。**PGF 无公开规范,第三方无法生成**;
  开源界通行做法即"生成 PDF 改后缀 .ai"(Inkscape 官方同路线)。
- 因此 Kiln 的 .ai:**Illustrator 可正常打开、文字可选中改字、路径可选可改色、
  渐变为矢量 shading;但无 AI 原生图层面板语义**(除非启用 OCG 增强)。
- 这不是缺陷而是格式现实;需要原生图层的工作流请用 PDF + OCG(Stretch 项)。

## 检查表(交付前逐项核对)

| # | 检查项 | 方法 | 通过标准 |
|---|---|---|---|
| 1 | 文件可打开 | Illustrator / pdfium / Acrobat 打开 | 无修复弹窗,内容完整 |
| 2 | 真文本可选中 | 文字工具点击正文 | 能进入编辑,字符正确 |
| 3 | 字体全嵌入 | `bench/acceptance.py` G5 审计 | 全部 FontFile2 / Type3 内嵌 |
| 4 | 路径可编辑 | 直接选择工具拖锚点 | 形状随动 |
| 5 | 渐变渲染正确 | 放大 800% 观察 | **位图**(pdfium 不渲染 `sh`/`PatternType 2`,矢量渐变会变空白;见 `pdf.rs` 注释) |
| 6 | 栅格化区域占比 | acceptance 报告 `raster_ratio` | 参考值;>30% 时 WARNING |
| 7 | 文本层完整度 | acceptance 报告 G4(delta/caveat) | delta≤3% 直过;≤15% 且 G2≥97 带 caveat |
| 8 | 效果层文字说明 | 对照 G4 caveat | mix-blend/clip:text 层文字已栅格化(像素保真) |

## 已验证的引擎行为(Edge 153,2026-09-19)

- 文本:Type0 子集嵌入(FontFile2)+ ToUnicode;小字形/合成样式走 Type3(内嵌 CharProcs)。
- `background-clip:text`、描边字:打印时字形**双写**(G4 以连写折叠对齐)。
- `mix-blend-mode` 层内文字:打印时**整层栅格化**——文字不可编辑但像素保真
  (保真度 > 可编辑性的既定取舍)。
- 渐变、圆角、路径:保持矢量;feTurbulence/backdrop-filter 栅格化。
- 栅格化面积占比参考:a4-front 0.87%(几乎全矢量)。

## 二轮实测(2026-09-19,`bench/ai_struct.py`,6/6 通过)

| 用例 | Type3 | Shading | `W n` 剪切 | 显示算子/上限 | OCG | 文本 delta | 画板页数 |
|---|---:|---:|---:|---|---:|---:|---:|
| rollup-80x200 | 0 | 0 | **0** | 90/111 | 背景·内容 | 0 | 1 |
| a4-front | 0 | 0 | **0** | 22/29 | 背景·内容 | 0 | 1 |
| a4-back | 0 | 0 | **0** | 59/74 | 背景·内容 | 0 | 1 |
| festival-zhongqiu | 0 | 0 | **0** | 45/57 | 背景·内容 | 0 | 1 |
| festival-guoqing | 0 | 0 | **0** | 53/67 | 背景·内容 | 0 | 1 |
| qr-poster | 0 | 0 | **0** | 62/77 | 背景·内容 | 11 | 1 |

PNG ↔ AI 栅格化相似度(`.agents/skills/artboard/scripts/ai_fidelity.py`,门禁 97):
a4-front 98.83 / a4-back 98.16 / festival-zhongqiu 98.19 / festival-guoqing 99.03 /
qr-poster 98.26。多画板 `双面.ai`(A4 正反合一):pikepdf/pdfium 均 2 页、OCG 2 个,
逐页相似度 98.83 / 98.16。

## 人工验收(需 Illustrator 环境,一次性)

- [ ] 文件双击可开,画板尺寸与 HTML 一致
- [ ] 中文正文可改字并保存(整行一条字符串)
- [ ] 主视觉路径可改色
- [ ] 每个元素**不带**自带剪切蒙版(内容流 0 个 `W n`)
- [ ] 图层面板仅「背景」「内容」两层;双画板文件两个画板
