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
- `pdf.rs`:自研 PDF 1.7 写入器 —— 真实文本(WinAnsi Tj)+ OCG 图层 +
  页面操作符子集;Ai 格式 = PDF 兼容流 + Illustrator 头(ADR-0008 路线)
- `postscript.rs`:EPS 3.0(BoundingBox/HiResBoundingBox + Level 2)
- `ooxml.rs`:PPTX 最小 OOXML + 自研 stored zip(手写 CRC32,零依赖)

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
