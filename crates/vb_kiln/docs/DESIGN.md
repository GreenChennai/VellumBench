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
| 位图资产 404(浏览器道) | 浏览器静默回退 + `AssetNotFound` 告警(VB-1) |
| 3D/mask/box-shadow/blend 丢弃、内联 SVG 栅格化 | 聚合计数 + `UnsupportedPropertyDropped` / `InlineSvgRasterized` 告警 + degraded(VB-2) |
| 静态资源 404(静态服务) | 请求线程收集,导出结束并入报告(VB-1,ADR-0045) |

## 降级必须可观测(ADR-0046 契约)

「输出与源不等价」的**每一条**分支都必须往 `KilnReport.warnings` 写强类型
告警并置 `degraded = true`,不许只打日志或静默——这是把 `import_svg` 的
既有实践(跳过清单,「内容已导入,效果不带」)上升为**全导出路径的契约**:

- **强类型**:`KilnWarning` 变体携带结构化字段(prop/lane/format/count),
  每个变体有稳定 `kind()` 键(小写蛇形)与 `is_degrading()` 语义;
- **聚合**:同属性同车道合并为一条 + `count`(10 个同属性元素 → 1 条),
  防告警刷屏;404 收集上限 64 条;
- **lane 三值**:browser / native / vector,下游按道分流;
- **可编程判定**:结果 JSON 同步输出 `warnings_by_kind`(键 = `kind()`),
  export 与 import 同构(均含 `warnings[]` + `degraded`);
  动画能力由 `anim_coverage`(animated / static_fallback / unsupported)
  如实呈现,下游门禁示例:`unsupported_dropped > 0` 拒绝交付矢量稿。

判定来源分三层,同一契约:`ExportContext::build` 的画板子树属性扫描
(构建期)、`dompaint`/`domsnap` 的采集期计数、`writer::common_warnings`
的写出器丢弃检出;导入侧为 `import_svg` 的跳过清单 → `ImportObjectSkipped`。

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
