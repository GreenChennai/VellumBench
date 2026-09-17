# 18 · PDF 可编辑导出全量修复计划书

> 目标:可编辑 PDF 经 PDFium 渲染为 PNG 后,与原生 HTML 经 Kiln 渲染的 PNG
> 逐像素对比,全部案例(22 个)平均还原度 ≥97%。
> 方法:像素对比 + 文本提取比对 + 网格分区定位偏移。
> 日期:2026-09-18 · 状态:🔴 待实施

---

## 一、问题定位(为什么 PDF 渲染 PNG ≠ HTML 渲染 PNG)

PDF 写入器(`pdf.rs`)将 DrawItem 转为 PDF 操作符。当前以下渲染特性
在 PDF 路径中**降级或缺失**,导致 PDFium 渲染 PNG 与 CPU 栅格 PNG 不一致:

| # | 缺失/降级项 | CPU 栅格(PNG)表现 | PDF 表现 | 影响 |
|---|---|---|---|---|
| F1 | **线性/径向渐变** | 逐像素插值,精确渐变 | 取中值色画纯色矩形 | 大面积渐变背景差异极大 |
| F2 | **图像嵌入** | 解码位图逐像素绘制 | `re B` 画灰色占位矩形 | 所有图片区域无内容 |
| F3 | **透明度** | 每项 alpha 混合 | 忽略 opacity(总是不透明) | 半透明卡片/遮罩变实色 |
| F4 | **filter(L3)** | blur/brightness/saturate | 不支持,按原图绘制 | 毛玻璃效果丢失 |
| F5 | **clip-path** | 区域裁剪 | 不支持,按完整矩形绘制 | 溢出内容可见 |
| F6 | **文本定位微偏** | 沿祖先链累加+旋转 Tm | 同路径但 Tm 计算/字体度量差异 | 文字偏移 1-3px |
| F7 | **边框圆角** | tiny-skia 精确圆角 | path_ops 近似圆角(6 坐标) | 大圆角处细微差异 |
| F8 | **混合模式** | 忽略(v0.1 不支持) | 同样忽略 | 一致(非差异项) |

**影响度排序**:F2(图像)> F1(渐变)> F3(透明度)> F6(文本微偏)> F5(裁剪)> F4(滤镜)> F7

## 二、修复计划(按影响度排序,逐项落地)

### F1 · PDF 渐变着色(Shading Pattern)

PDF 原生支持渐变: axial(`ShadingType 2`)与 radial(`ShadingType 3`)。

实现:
- 页面 Resources 增加 `/Shading << /Sh1 ... >>` 字典
- 线性渐变: `/ShadingType 2 /Coords [x0 y0 x1 y1] /Function << type 2 >>(指数插值)` 或 type 3(采样)
- 径向渐变: `/ShadingType 3 /Coords [cx cy r0 cx cy r1]`
- 多 stop 渐变: 使用 `/Function /Type 3`(Stitching)串联多个 Type 2
- 内容流: `q /Sh1 sh Q` 替代当前的 `rg + re f`

坐标:渐变 Coords 在 PDF 用户空间(Y 向上),需从 artboard 坐标系转换。

**验收**: c4-gradient 的 PDF 渲染 PNG 与 HTML 渲染 PNG 差异 MAD < 10。

### F2 · PDF 图像嵌入(XObject)

PDF 图像通过 Image XObject 嵌入:
- RGB → `/Filter /DCTDecode`(JPEG 压缩)或 `/FlateDecode`
- 内容流: `q {w} 0 0 {h} {x} {py} cm /Im1 Do Q`

实现:
- 从 DrawItem 的 `image: Option<BitmapData>` 提取 RGBA
- RGBA → RGB(白底合成)→ PNG 编码 → FlateDecode 流
- 每个 Image XObject 分配 `/Im{n}`,页面 Resources 增加 `/XObject << /Im{n} ... >>`
- 内容流: `q {w} 0 0 {h} {x} {py} cm /Im{n} Do Q`

坐标: PDF 图像 Do 操作符以 1×1 单位绘制,需 cm 缩放到目标尺寸。

**验收**: show2-xhs 的 .hero .photo 区域不再是灰色占位,显示正确图片。

### F3 · PDF 透明度(ExtGState)

PDF 透明度通过 ExtGState 的 `/ca`(fill alpha)与 `/CA`(stroke alpha):
- 每个唯一 opacity 值创建一个 ExtGState 对象
- 内容流: `/GS{a} gs` 设置透明度
- 页面 Resources: `/ExtGState << /GS1 << /ca 0.5 >> >>`

**验收**: 半透明卡片(如 opacity:0.8)在 PDF 中正确显示半透明效果。

### F4 · clip-path(可选,v1 跳过)

PDF 支持 `W n` 裁剪,但与 `q/Q` 配合复杂。v1 跳过,记录为已知边界。

### F5 · filter(L3 滤镜,可选,v1 跳过)

PDF 无 blur/brightness/saturate 原生操作。可选方案:
- 预渲染:在 CPU 栅格时对有 filter 的项单独渲染为位图,嵌入 PDF
- v1 跳过,记录为已知边界

### F6 · 文本定位校准

当前 Tm 使用 Tm=(cs,sn,-sn,cs,px,py) 旋转 + 平移。基线计算:
`baseline_art = y + run.ascent + vi * line_h`

校准:确保 ascent 取自实际整形结果的 metrics(而非近似值),且行高
与 CPU 渲染一致(1.32×字号,或 CSS line-height)。

**验收**: c2-headline 大字标题文本位置偏差 ≤ 2px。

## 三、对比管线(bench/pdf_fidelity.py)

### 流程(每案例)

```
HTML ──→ Kiln CPU 栅格 PNG(基准)
  │
  └──→ Kiln PDF ──→ PDFium 渲染 PNG(候选)
                          │
                    像素对比(MAD→score)
                          │
                    网格分区(10×10)定位差异区域
                          │
                    文本提取比对(ToUnicode vs HTML 文本)
```

### 评分口径

与 bench/fidelity.py 一致:score = 100 × (1 − MAD/255)
基线 = CPU 栅格 PNG(HTML → Kiln 直接渲染,即"原生 HTML 导出的 PNG")

### 网格分区

将画布分为 10×10 网格,每格独立计算 MAD。MAD > 30 的格子标记为差异区域,
输出坐标供人工排查。

### 文本比对

从 PDF 提取的文本(ToUnicode)与 HTML 源文件的可见文本做编辑距离比对。
差异率 > 5% 标记为文本异常。

## 四、验收标准

| 指标 | 要求 |
|---|---|
| 全部案例(22 个)平均还原度 | ≥ 97% |
| 单案例最低还原度 | ≥ 90% |
| 文本提取一致率 | ≥ 95% |
| 行首禁则字数 | 0 |
| 图像区域非灰色占位 | 有图像的案例全部通过 |
| 渐变区域 MAD | < 15 |

## 五、实施清单

- [ ] F1 渐变着色(axial + radial + 多 stop stitching)
- [ ] F2 图像嵌入(XObject + Do)
- [ ] F3 透明度(ExtGState)
- [ ] F6 文本定位校准(ascent 精确化)
- [ ] bench/pdf_fidelity.py 对比脚本(PDF→PNG vs HTML→PNG)
- [ ] 全案例跑分 ≥ 97%
- [ ] BENCHMARK.md 更新 §十

## 六、不在本轮做

- F4 clip-path(复杂度高,v1 跳过)
- F5 filter/L3 滤镜(需预渲染方案)
- SVG 导入(延续 v0.5 延后)
