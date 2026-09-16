# Kiln 设计定稿(架构/模块/接口/数据流)

## 命名

**Kiln(窑)** —— Vellum(上等羔皮纸)入窑,烧出九种成品。crate 名 `vb_kiln`,
CLI 入口 `kiln`;与 WPI(Web Page Imager)无命名冲突,代码与文档统一使用 Kiln。

## 架构

```
Document(vb_doc)
   │ encode_artboard_opts
   ▼
DrawList(vb_render,引擎中立绘制指令,CPU/GPU 单一来源)
   │ ExportContext::build(守门/钳制/光栅帧)
   ▼
FormatWriter trait ── 九个实现
   ├── 光栅:png / jpg(raster.rs)
   ├── 动画:gif / mp4(frames.rs;量化+ffmpeg 桥)
   └── 矢量:svg(复用 vb_export) / pdf / eps / ai(pdf.rs+postscript.rs) / pptx(ooxml.rs)
   ▼
(字节流, KilnReport{warnings, encode_ms, bytes, frames, degraded})
```

## 关键接口

```rust
pub trait FormatWriter: Send + Sync {
    fn format(&self) -> Format;
    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport>;
}
pub fn export_artboard(doc, artboard, req: &ExportRequest, project_dir) -> KilnResult<(Vec<u8>, KilnReport)>
```

`ExportRequest`:format/scale(1..=8 钳制)/transparent/jpeg_quality/fps/duration_s/gif_loops/mp4_bitrate_kbps。

## 守门与降级链

| 异常输入 | 行为 |
|---|---|
| 画布单边 >32768px 或面积 >2.68 亿像素 | `KilnError::CanvasTooLarge/AreaTooLarge`(不 OOM) |
| fps/duration 非法 | `KilnError::BadAnimation` |
| scale >8 | 钳制为 8 + `ScaleClamped` 告警 |
| JPG 透明请求 | 垫白底 + `JpgOpaqueForced` 告警 |
| 位图资产缺失 | 占位框 + `ImageMissing` 告警 |
| MP4 无 ffmpeg | 降级 GIF 流 + `Mp4DowngradedToGif` 告警 |
| PDF/EPS CJK 文本 | 兼容字形降级 + `TextTransliterated` 告警 |

## 算法资产(继承自 artboard 管线实测经验)

- PDF 枢纽思想:矢量语义以 PDF 为中心表达(真文本 Tj + OCG 图层)
- OCG 合成结构:`/OC BDC…EMC` + OCProperties(artboard ADR-0009 实测形态)
- Ai = PDF 兼容流 + Illustrator 头(ADR-0008;不伪造 PGF 私有流)
- 双轨文字策略:print 版保真文本,衍生格式转曲/降级明确告警
- WPI 的 ffmpeg palettegen 双通道 GIF 调色板经验 → Kiln 预留 ffmpeg 桥接口

## WPI 替换与回退

- 默认路径:vb_app 导出对话框九格式全走 Kiln
- 回退:`VB_EXPORT_ENGINE=wpi` 时 PDF/GIF/MP4 走 vb_export::wpi(旧桥未动)
- 回滚手册:`docs/ROLLBACK.md`;语义回归测试锁定开关行为
