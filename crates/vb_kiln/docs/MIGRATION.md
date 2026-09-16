# WPI → Kiln 迁移指南

## 谁受影响

VellumBench 的导出入口(GUI 导出对话框 / `vb_kiln::export_artboard` API)。
WPI 仓库与 `vb_export::wpi` 模块**保留但不再是默认**。

## 行为变化

| 项 | 迁移前(WPI 默认) | 迁移后(Kiln 默认) |
|---|---|---|
| PDF/GIF/MP4 | 浏览器进程截图(需 Python+Edge) | 原生直出(无外部依赖;MP4 需 ffmpeg,缺则降级) |
| PNG/SVG | 原生(旧路径) | 原生(Kiln 同一引擎,统一入口) |
| JPG/EPS/Ai/PPTX | 无 | 新增 |
| 导出对话框 | 5 个格式项 | 9 个格式项,全部标 (Kiln) |
| 返回信息 | 状态栏字节数 | KilnReport(字节/耗时/帧数/降级/告警) |

## API 迁移

```rust
// 旧(仍在,回退用):
vb_export::wpi::export_via_wpi(&doc, &dir, &req, &wpi_dir)?;

// 新(Kiln,推荐):
let req = vb_kiln::ExportRequest {
    format: vb_kiln::Format::Pdf,
    scale: 2,
    transparent: false,
    ..Default::default()
};
let (bytes, report) = vb_kiln::export_artboard(&doc, artboard, &req, Some(&dir))?;
```

## 用法示例(九格式)

```bash
kiln export --source project/ --output hero.png  --scale 2 --transparent
kiln export --source project/ --output hero.jpg  --scale 2            # 质量 92 默认
kiln export --source project/ --output hero.gif  --fps 30 --duration 2
kiln export --source project/ --output hero.mp4  --fps 30 --duration 2
kiln export --source project/ --output hero.svg  --scale 2
kiln export --source project/ --output hero.pdf  --scale 2
kiln export --source project/ --output hero.eps
kiln export --source project/ --output hero.ai
kiln export --source project/ --output hero.pptx
```

## 画板语义注意

- 画板尺寸取 `.vb-artboard <第二类名>` 的 width/height 规则
  (canonical 格式,见 `vb_doc/tests/corpus/04-multi-artboard.html`);
  marker 类(`vb-artboard`)本身不带样式
- 画板背景 `background: linear-gradient(...)` 简写已支持
  (v0.4 修复;简写等价 background-image)
- 文本节点按画板绝对定位排版(流式内层元素导入时已提示警告)

## 已知边界

- GIF 编码用 image 感知量化(单通道);与 WPI 的 ffmpeg palettegen 相比
  同帧体积略大,如需拉平可切 ffmpeg 桥(接口已留,见 frames.rs)
- PDF/EPS 的 CJK 文本当前以兼容字形降级(告警标注);SVG/PPTX 保持原文;
  CID 字体嵌入在文本管线迭代计划内
