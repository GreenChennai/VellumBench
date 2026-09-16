# Kiln 九格式支持矩阵

| 格式 | 引擎 | 真实文本 | 图层 | 透明 | 动画 | 可再编辑 | 依赖 |
|---|---|---|---|---|---|---|---|
| PNG  | tiny-skia 真字形光栅 | —(像素) | — | ✅ | — | ❌ | 无 |
| JPG  | tiny-skia + image JPEG | —(像素) | — | ⚠️ 垫白底+告警 | — | ❌ | 无 |
| GIF  | 帧序列+感知量化 LZW | —(像素) | — | ✅ | ✅ fps/时长/循环 | ❌ | 无 |
| MP4  | ffmpeg 桥 libx264 yuv420p | —(像素) | — | ❌ | ✅ fps/时长/码率 | ❌ | ffmpeg(可选,缺则降级 GIF 流) |
| SVG  | DrawList→SVG | ✅ `<text>` | ✅ 分组 | ✅ | — | ✅ 编辑器可改字 | 无 |
| PDF  | 自研 PDF 1.7 写入器 | ✅ WinAnsi Tj | ✅ OCG | ✅ | — | ✅ 阅读器可改字 | 无 |
| EPS  | PostScript Level 2 | ✅ show | ⚠️ 扁平 | ⚠️ 白底 | — | ✅ 印刷软件 | 无 |
| Ai   | PDF 兼容流+AI 头 | ✅ | ✅ OCG(Acrobat/浏览器可见) | ✅ | — | ✅ Illustrator 打开 | 无 |
| PPTX | 自研 OOXML+stored zip | ✅ `<a:t>` | ✅ shape 树 | ⚠️ 白底 | — | ✅ PowerPoint | 无 |

注:
- ✅ 支持 / ⚠️ 部分支持(带告警) / ❌ 不适用
- Ai 格式为 PDF 兼容方案(ADR-0008):Illustrator 与任意 PDF 阅读器双开;
  不伪造 PGF 私有数据流(仅 Illustrator 自身能写)
- 画布守门:单边 ≤32768px,面积 ≤2.68 亿像素(约 1GB RGBA),超限报错不崩溃
- 异常兜底:字体缺失→占位+告警;位图缺失→占位框+告警;MP4 无 ffmpeg→GIF 流降级
