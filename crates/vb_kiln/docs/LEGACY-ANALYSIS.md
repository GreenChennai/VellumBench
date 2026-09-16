# 导出链路资产盘点与瓶颈分析(Kiln 立项依据)

> 盘点对象:VellumBench v0.3 导出模块、WPI(Python 网页截图工具)、
> artboard 技能的 WebHtml2VectorEdit / VectorEdit2WebHtml 转换核心。
> 分析日期:2026-09-16。

## 一、VellumBench v0.3(现状)

- `vb_export`:PNG(tiny-skia 真字形)+ SVG(真实 `<text>`)+ WPI 进程桥
  (PDF/GIF/MP4,`python src/cli.py` 子进程,依赖本机 WPI 仓库+Python+Playwright)
- `vb_render`:DrawList 引擎中立编码(ADR-0016)+ CPU 光栅 + fontique/swash
  真文本管线(C4)
- 缺口:JPG/EPS/Ai/PPTX 无;PDF/GIF/MP4 依赖外部浏览器进程;无图层语义

## 二、WPI(浏览器截图方案)算法与瓶颈

链路:本地 static server → Playwright 驱动系统 Edge/Chrome → 截图/帧采样
→ Pillow/ffmpeg 编码。

| 瓶颈 | 证据 | Kiln 对策 |
|---|---|---|
| 启动成本 1.5–4s(浏览器+Python) | 实测 PNG 4467ms/GIF 3529ms/MP4 2739ms/PDF 4789ms | 原生进程内直出(193/1848/422/99ms) |
| rAF 密集动画会卡死无头渲染 | capture_engine.py 注入 rAF 节流脚本兜底 | 无浏览器主线程问题 |
| PDF 是打印流(46KB 起步,含浏览器资源) | pdf_exporter.py Page.printToPDF | 自研写入器 3.8KB(−92%) |
| MP4 采样走 CDP JPEG(q95)有损 | mp4_exporter.py 注释 | PNG 帧序列直喂 libx264 |
| 格式仅 4 种;矢量输出为零 | cli.py choices=[PNG,GIF,MP4,PDF] | 九格式,五种可编辑 |
| 异常路径=进程错误码 | cli.py 退出码 | KilnError 分类+告警矩阵 |
| 值得保留的经验 | ffmpeg palettegen 双通道 GIF 调色板;动画结束双判据(像素不变+getAnimations) | GIF 桥接口预留;动画判据入后续时间轴迭代 |

## 三、artboard 技能(WebHtml2VectorEdit / VectorEdit2WebHtml)

- 正向:浏览器打印精确尺寸 PDF 枢纽 → DOM 分层手术(visibility 多遍打印)
  → pikepdf OCG 合成 → pdftocairo/gs 衍生 SVG/EPS/outline → SSIM 自检
- 逆向:pdftohtml -xml 提文字 + pdftocairo -svg 底景 → 透明文字层 HTML
- 关键实测结论:**AI 不认手写 OCG 为图层**(真图层只能 Illustrator 自写
  PGF);text_run_merger 状态机把逐字 Td/Tj 合并整句 TJ 是"AI 文字可编辑"
  的命门;转曲走 cairo 系引擎(gs pdfwrite 有褪色坑)
- 瓶颈:五类外部进程链(playwright/pikepdf/poppler/gs/COM)110MB+ 部署;
  无结构化中间层;SSIM 串行
- Kiln 吸收:OCG 结构形态、Ai PDF 兼容路线、双轨文字策略、
  印刷行业"转曲交付"哲学;并引入 DrawList 作为结构化中间层
  (单测可覆盖,这是原管线最大短板)

## 四、结论

WPI 的像素路径慢在进程链、矢量能力为零;artboard 管线矢量能力强但部署
重、不可嵌入。Kiln 以 VellumBench 自有 DrawList 为源,原生实现九格式,
速度/体积/能力三面超越;浏览器路径(WPI)保留为回退开关。
基准数据见 `docs/BENCHMARK.md`。
