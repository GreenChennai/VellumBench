# Kiln — VellumBench 下一代导出核心

Kiln(窑):Vellum(羔皮纸)入窑,烧出九种成品。原生 Rust 导出核心,
**全面取代 WPI 浏览器导出路径**(WPI 保留为回退,`VB_EXPORT_ENGINE=wpi` 启用)。

## 九格式

| 媒体格式 | 可编辑格式 |
|---|---|
| PNG · JPG · GIF · MP4 | SVG · PDF · EPS · Ai · PPTX |

## 快速开始

```bash
# CLI 导出(扩展名决定格式)
kiln export --source <项目目录|index.html> --output out.pdf --scale 2

# 九格式全量基准
kiln bench --source <项目目录> --out-dir bench-out --scale 2
```

```rust
use vb_kiln::{export_artboard, ExportRequest, Format};

let req = ExportRequest {
    format: Format::Pdf,
    scale: 2,
    ..Default::default()
};
let (bytes, report) = export_artboard(&doc, artboard_id, &req, Some(project_dir))?;
// report.summary() -> "3 KB / 202ms / 2条警告"
```

## 架构

```
Document(vb_doc) ─encode─▶ DrawList(vb_render,引擎中立)
                               │
                    ExportContext(守门+光栅帧)
                               │
              FormatWriter(九个实现,trait 统一)
        ┌──────────┬──────────┬──────────┐
     光栅层     矢量容器层    动画层
   PNG/JPG    PDF/EPS/Ai/PPTX  GIF/MP4
              (自研写入器)    (量化/ffmpeg 桥)
```

- `context.rs`:ExportContext —— 画布守门(32768px/边,2.68 亿像素上限)、
  scale 钳制、动画参数校验、透明度语义归一
- `raster.rs`:DrawList → RGBA/PNG(复用 vb_render::cpu 真字形引擎)
- `frames.rs`:GIF(image 感知量化+LZW)/MP4(ffmpeg libx264 yuv420p,
  无 ffmpeg 自动降级 GIF 流并告警)
- `pdf.rs`:自研 PDF 1.7 写入器 —— CID 中文真文本(Type0/Identity-H 子集
  嵌入 + ToUnicode,阅读器可选中、AI 可改字)+ OCG 图层 + clip-path +
  渐变栅格化位图(pdfium 不渲染 PDF shading,按 CPU 同款数学降采样嵌入)
  + SMask 半透明;Ai 格式 = PDF 兼容流 + Illustrator 头(ADR-0008 路线)
- `postscript.rs`:EPS 3.0(BoundingBox/HiResBoundingBox + Level 2)
- `ooxml.rs`:PPTX 最小 OOXML + 自研 stored zip(手写 CRC32,零依赖)

## 导入与位图工具箱

```bash
kiln-cli import --source designer.pdf --output restored/   # PDF/AI:pdfium.dll 动态绑定
kiln-cli import --source icon.svg     --output restored/   # SVG:usvg 纯 Rust,零依赖
kiln-cli img crop --input a.png --box 10,20,300,200 --output crop.png
kiln-cli img stitch --input a.png --input b.png --vertical --gap 8 --bg "#fff" --output merged.png
kiln-cli img blur --input a.png --radius 4 --region 0,0,100,100 --output blur.png
kiln-cli img pad  --input a.png --all 12 --color "#ffffff" --output pad.png
kiln-cli img info --input a.png
```

- 导入产出规范化 HTML 项目(index.html + styles/main.css + assets/),
  文本提取真实字符串(不转曲),可直接重导出
- PDF 导入 v1 边界:统一近似色(取色 API 未暴露)、路径以盒近似;
  SVG 导入 v1 边界:自由曲线包围盒近似 + 警告、渐变取中点色

## 保真度验收

```bash
python bench/pdf_fidelity.py --kiln target/release/kiln-cli.exe     --cases <案例目录> --out bench/pdf-fidelity
```

口径:可编辑 PDF → PDFium 栅格化 PNG,与 HTML → Kiln 原生渲染 PNG 逐像素
MAD 对比 + 10×10 网格热区 + ToUnicode 文本一致率。当前 22 案例(artboard
全范例语料)平均 **98.63/100**,文本一致率 **100%**,0 案例 <90。
关键实现事实:pdfium(pypdfium2 5.13 实测)不渲染 PDF shading(`sh` 与
PatternType 2 均白屏),因此渐变一律按 CPU 栅格同款数学降采样为位图嵌入。

## 与 WPI 的关系

- WPI(浏览器截图)自 v0.4 起不再是默认路径,仅作回退:
  `set VB_EXPORT_ENGINE=wpi`(对 PDF/GIF/MP4 生效)
- 基准对比见 `docs/BENCHMARK.md`:速度 1.9×–48.4×,PNG/MP4/PDF 体积
  −33% ~ −92%,还原度 95.4/100(与 WPI 自洽分 96.15 同档)

## 测试

```bash
cargo test -p vb_kiln --release
```

13 项:九格式 magic 冒烟、PPTX zip 结构、PDF OCG+文本、SVG 真文本、
EPS 头、Ai 头、超大画布守门、scale 钳制、动画参数守门、JPG 透明告警、
静态画布动画、命名模板回归、回滚开关语义。
