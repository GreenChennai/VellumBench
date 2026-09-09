# 07 · 导出管线与 WPI 集成

> 两条腿走路：**原生 Vulkan 导出**（快、离线、矢量）+ **浏览器真值导出**（准，复用 WPI）。
> 二者共享同一份 HTML，差异由「校对视图」显式暴露，而不是偷偷不一致。

---

## 一、导出能力矩阵

| 格式 | 原生 Vulkan | 浏览器(WPI) | 默认引擎 | 备注 |
|---|---|---|---|---|
| PNG / JPG / WebP | ✅ | ✅ | **原生** | 原生矢量重光栅，@2x/@3x 无损 |
| SVG | ✅ | — | 原生 | 矢量导出（非位图） |
| PDF（矢量，文本可选） | ✅ | — | 原生 | `printpdf` / `pdf-writer` |
| PDF（浏览器打印） | — | ✅ | 可选 | 超长页自动分页（WPI 已实现） |
| GIF / MP4（CSS/JS 动画） | ⚠️ 有限 | ✅ | **浏览器** | 原生不支持 JS 动画 |
| GIF / MP4（时间轴动效 v2） | ✅ | ✅ | 原生 | 自研时间轴逐帧 |
| HTML 打包 ZIP | ✅ | — | 原生 | |
| 单文件 HTML | ✅ | — | 原生 | 内联 CSS/JS/图片 |
| JSON（文档结构） | ✅ | — | 原生 | 给 Agent 用 |

**自动建议规则**（导出对话框显示徽章）：文档含 `冻结块` / `<script>` / L2+ CSS / 自定义字体 → 建议「浏览器引擎」。否则「原生引擎」。

---

## 二、原生导出管线

复用 05 篇的离屏渲染：

```
ExportTask { artboard | slice | selection | fullpage, format, scale, transparent, out }
  → 计算目标像素尺寸 (w*scale, h*scale)
  → 若超过 max_texture_dimension_2d (通常 16384) → 分块渲染（tile 1024，逐块 render+读回+拼接）
  → Vello render_to_texture（base_color = 透明/白/画板背景）
  → copy_texture_to_buffer（⚠️ bytes_per_row 必须 256 字节对齐）
  → image::RgbaImage → PNG/JPEG/WebP 编码
  → GIF/MP4：逐帧 → 送 FFmpeg 管道
```

| 项 | 规格 |
|---|---|
| @1x/@2x/@3x/@4x | **矢量重新光栅化**，不是位图放大 |
| 透明背景 | `base_color = TRANSPARENT`，PNG/WebP 支持 |
| 元数据 | 写入 `data-vb-id` 到 PNG tEXt 块（可选，便于溯源） |
| 并发 | 画板级并行（rayon），**GPU 串行**（单 device 顺序渲染，避免显存峰值） |
| 超大图 | 分块 + 磁盘拼接，进度可报 |
| 取消 | 任务级取消令牌（不只是 UI 关闭） |

**GIF/MP4（原生）**：逐帧渲染 → 直写 raw RGBA 到 FFmpeg stdin：
```
ffmpeg -f rawvideo -pix_fmt rgba -s WxH -r 30 -i - -c:v libx264 -pix_fmt yuv420p out.mp4
ffmpeg -f rawvideo -pix_fmt rgba -s WxH -r 15 -i - -filter_complex "split[a][b];[a]palettegen[p];[b][p]paletteuse" out.gif
```
FFmpeg 来源：随包（复用 `E:\平日资料\GitHub\MomentShift\tools\ffmpeg_bin\`），缺失时提示并禁用 GIF/MP4。

---

## 三、WPI 现状盘点（已核实）

```
E:\平日资料\GitHub\WPI
  Python 3.11+ / PySide6 / Playwright(系统 Edge|Chrome) / Pillow / 可选 FFmpeg
  src/core/   controller, browser_host, capture_engine, static_server, color_profiler
  src/export/ png_exporter, gif_exporter, mp4_exporter, pdf_exporter
  src/cli.py  → WPI-noGUI-cli.exe（无 Qt，~40MB）
```

**已解决的难问题（不要重写）**：本地静态服务、Playwright 启动与页面准备、**整页测量**、**reveal-on-scroll 动画展开**、**有限动画冻结与等待策略**、超长页 PDF 分页、GIF 调色板、MP4 编码、资源加载等待与失败提醒、重名文件 `_1/_2` 处理。

**已确认的 CLI 契约**：
```
WPI-noGUI-cli.exe --source <dir|html|url> --output <file> --format PNG|GIF|MP4|PDF
                  --width N --scale 1|2|4|8 --height 0 --fps N --loop N
                  --max-wait S --transparent --no-full-page --no-ffmpeg --selfcheck
```

---

## 四、WPI 集成三阶段

### 阶段 A：进程调用（v0.5，零改动 WPI）

```rust
Command::new(wpi_cli)
  .args(["--source", &tmp_dir, "--output", &out, "--format", "PNG",
         "--width", "1920", "--transparent"])
  .stdout(Stdio::piped()).spawn()
```
- 优点：零改动、隔离（WPI 崩了不影响主程序）
- 缺点：**每次调用冷启动浏览器 3–5s**；无法做画板区域裁剪（缺 `--clip`）
- 适用：低频导出、批量队列（可接受总时长）

### 阶段 B：常驻 sidecar（v0.6，推荐）

```
Vellum Bench ──stdio JSON-RPC──▶ wpi_sidecar.py（常驻，持有一个 Playwright browser）
         ◀──────────────────
```

请求：
```json
{"id":7,"cmd":"export","params":{
  "source_dir":"C:\\...\\.vb-export-tmp\\hero",
  "out":"C:\\...\\hero@2x.png",
  "format":"PNG","width":1920,"scale":2,
  "selector":"[data-vb-id=\"b1e0aa\"]",
  "wait_for":"fonts+networkidle+200ms",
  "transparent":true,"timeout_ms":60000}}
```
响应：
```json
{"id":7,"ok":true,"width":3840,"height":1200,"elapsed_ms":812,
 "warnings":["外部字体加载超时，已回退"]}
```

- 复用 browser 实例 → 后续导出 **200–800ms**
- 空闲 5 分钟自动退出；崩溃由 Rust 侧心跳检测并重启
- 并发：单实例串行；批量时可选启动 N 个 sidecar（N = min(CPU/2, 4)）

**需要给 WPI 增加的能力**（改造清单，优先级从高到低）：

| # | 新增 | 用途 | 必要性 |
|---|---|---|---|
| 1 | `--selector <css>` | 按 `data-vb-id` 定位元素，以其 bounding box 为导出区 | **必须**（画板/切片导出） |
| 2 | `--clip <x,y,w,h>` | 显式区域裁剪 | **必须**（切片/选区导出） |
| 3 | `--json` | 结构化输出（尺寸/耗时/警告）供 Rust 解析 | **必须**（错误处理与进度） |
| 4 | `--scale 3` | AI 用户习惯 @3x | 建议（当前只有 1/2/4/8） |
| 5 | `--out-dir` + `--template` | 批量导出到目录 | 建议 |
| 6 | `--wait-for fonts,networkidle,anim` | 等待策略细化 | 建议 |
| 7 | 剥离 `src/core`+`src/export` 为 `wpi_core` 包 | 供 sidecar 直接 import（不经 CLI 解析） | 可选（阶段 B+） |

> 实现方式：在 WPI 仓库开分支 `feat/vellum-bridge`，保持 CLI 向后兼容（新参数全部可选），不破坏 WPI 独立使用。

### 阶段 C：内嵌预览面板（v1.0+，可选）

在右侧面板坞提供「浏览器」面板：常驻 Playwright 页面 + 实时重载当前 HTML。
- 需要窗口嵌入（Windows：拿 Playwright 页面 HWND 并 SetParent）—— 复杂且脆弱
- **更稳的替代**：不做嵌入，提供「在系统浏览器打开校对」（`F5` 刷新）+ 分屏比对图
- 结论：**不做内嵌**，只做外置浏览器 + 校对视图

---

## 五、浏览器真值校对（Consistency Check）

```
命令：视图 → 浏览器校对（F5 打开外置浏览器 / Mod+Shift+B 生成对比图）
流程：
1. 导出当前视图区域：原生 PNG（tmp_a.png）+ 浏览器 PNG（tmp_b.png）
2. 逐像素 diff（容差 2/255），生成差异热区图
3. 报告：差异像素占比、最大偏差位置、Top5 差异区块（含元素 data-vb-id）
4. 差异 > 0.5% → 提示「该效果可能存在浏览器差异，建议用浏览器引擎导出」
```

这个功能是**信任基石**：它把"自绘引擎与浏览器不可能 100% 一致"这件事从隐患变成可测量的指标。

---

## 六、导出队列与 UI

导出面板（见 03 篇 5.11）：
- 列表：画板 / 切片 / 选区，多选批量
- 每行列：名称、格式、倍率、引擎徽章、状态
- 底部：输出目录、命名模板、**导出 N 项** 按钮 + 进度条 + 取消
- 完成：打开文件夹 / 通知

**命名模板**默认 `{artboard}@{scale}x`，可用变量：
`{doc}` `{artboard}` `{slice}` `{scale}` `{ext}` `{date}` `{index}` `{width}` `{height}`

示例：`{doc}_{index:02}_{artboard}@{scale}x.{ext}` → `Landing_01_Hero@2x.png`

---

## 七、依赖与降级

| 依赖 | 缺失时 |
|---|---|
| 系统 Chrome/Edge | 浏览器引擎不可用 → 提示安装链接 → **自动降级原生导出**（功能子集可用） |
| FFmpeg | GIF/MP4 禁用（按钮置灰 + 说明），PNG/PDF/HTML 不受影响 |
| WPI sidecar | 同「无浏览器」；设置页显示集成状态与手动路径配置 |
| Vulkan 驱动 | 主程序降级到 GL 后端（仍能跑）；原生导出仍可用（同一管线） |

**打包**：随包 `WPI-noGUI-cli.exe`（或 sidecar 版 `wpi_sidecar.exe`）+ `ffmpeg.exe`；不随附浏览器（体积与许可考虑，与 WPI 现状一致）。

---

## 八、临时目录与清理

- 导出前把当前文档渲染为临时 HTML 目录：`{temp}/vsm-export-{pid}-{seq}/`
- 内含 `index.html` + `styles/` + `assets/`（软链或复制，小文件复制更稳）
- 导出完成/取消后清理（与既有偏好一致：**任务结束先清理残留文件再开下一个**）
- 异常退出：下次启动扫描并清理 24 小时前的临时目录

---

## 九、验收点

- [ ] 画板 @1x/@2x/@3x PNG 导出，尺寸与倍率精确（像素级断言）
- [ ] 10000×12000 大图导出不失败（分块路径生效）
- [ ] 透明背景 PNG 正确（alpha 通道非预乘错误）
- [ ] 矢量 PDF 文本可选中；超长页走浏览器路径可分页
- [ ] 批量导出 50 个画板：原生路径总耗时 < 60s；浏览器路径（sidecar 复用）< 90s
- [ ] 无浏览器环境：浏览器引擎按钮置灰，原生导出全功能可用
- [ ] 无 FFmpeg：GIF/MP4 禁用且有说明
- [ ] 校对视图：人为造一个差异（如 `backdrop-filter`），能正确报出差异率与元素 id
- [ ] 导出中断（取消/崩溃）无临时目录残留
- [ ] WPI 的 3 个必须新增参数（selector/clip/json）在 `feat/vellum-bridge` 分支实现且 CLI 向后兼容
