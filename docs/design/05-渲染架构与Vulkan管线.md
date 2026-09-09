# 05 · 渲染架构与 Vulkan 管线

> 目标：**拖动 1 个对象时 ≤8ms 重绘，平移/缩放 5 万对象稳定 60fps，冷启动 ≤1.5s。**
> 选型：Rust + **wgpu**（Vulkan 优先）+ **Vello**（GPU 2D 路径渲染）+ **Parley/Swash**（文本）+ **Taffy**（布局）。

---

## 一、为什么是这套（详细论证见 13 篇 ADR-002）

| 层 | 选型 | 理由 |
|---|---|---|
| GPU 抽象 | **wgpu** | Vulkan / Metal / DX12 / GL 后端；安全 API；跨版本稳定；被 Firefox、Bevy、Blitz 使用 |
| 2D 绘制 | **Vello**（linebender） | Compute-shader 路径渲染，GPU 全量光栅化；支持海量路径、图层混合、裁剪；**encoding 可并行**；与 wgpu 无缝 |
| 文本 | **Parley**（布局/整形）+ **Swash**（光栅）+ 自建图集 | Rust 原生；支持中文、双向、换行、字距；与 Vello 官方示例同源 |
| 布局 | **Taffy** | 成熟 flexbox + grid；我们的定位以 absolute 为主，flex 为辅，负载很小 |
| 路径 | **kurbo** | Vello 原生使用的几何库 |
| SVG | **usvg**(解析) + Vello(绘制) | 复用 SVG 导入导出 |
| UI | **egui** + `egui-wgpu` | 与画布共享同一个 `wgpu::Device`，单窗口单 GPU 上下文，无跨进程拷贝 |

**为什么不用裸 Vulkan**：要多写 5000+ 行样板、跨平台后退化为零、驱动兼容坑自负。wgpu 保留对 `VkInstance`/`VkDevice` 的逃生舱（`instance.as_hal::<vulkan::Api>()`），需要手写特定 pass 时仍可下探。

---

## 二、渲染分层

```
┌─ Layer 4  UI        egui（面板/对话框/工具提示）                   每帧
├─ Layer 3  Overlay   选区框、手柄、智能参考线、网格、画板名、提示     每帧（极轻量）
├─ Layer 2  Active    正在交互的对象（拖动/变换/绘制中）              每帧（小）
├─ Layer 1  Static    稳定内容（画板背景 + 未参与交互的对象）         缓存纹理
└─ Layer 0  Backdrop  画布底色、棋盘透明格                            缓存纹理
```

**核心性能手段 = 静态层缓存**：
- Layer 0/1 渲染进一张与视口同尺寸的 `Rgba8Unorm` 纹理，只在「文档结构/样式变化」或「视图变换停止并稳定 N 帧后」重建
- 拖动/平移/缩放时：只重绘 Layer 2/3，Layer 1 用**纹理采样 + 视图变换矩阵**近似（缩放时先双线性拉伸、静止后重渲染到清晰）
- 这是 AI / Figma / 浏览器合成器同一个思路：**交互优先响应，精度随后补齐**

---

## 三、帧循环

```rust
fn frame(&mut self) {
    // 1. 输入 → 命令 → 文档变更（可能来自用户或 Agent）
    let changed = self.drain_commands();          // 命令模式，见 09 篇
    // 2. 脏标记传播
    if changed.structure { self.layout_dirty = true; self.static_dirty = true; }
    if changed.style_only { self.layout_dirty |= affects_layout; self.static_dirty = true; }
    if self.layout_dirty { self.relayout(); }     // Taffy，仅脏子树
    // 3. 视口裁剪（R-tree 查询可见画板 + 可见节点）
    let visible = self.spatial_index.query(self.viewport);
    // 4. 编码交互层：并行构建 Vello Scene（rayon）
    let mut scene = Scene::new();
    encode_visible(&mut scene, &visible, /*low_detail*/ self.view.moving);
    // 5. 静态层：若 dirty 则重建（离屏 pass），否则复用
    if self.static_dirty && !self.view.moving { self.rebuild_static_layer(); }
    // 6. 合成：Backdrop → Static(纹理) → Active → Overlay
    let mut pass = encoder.begin_render_pass(...);
    self.composite(&mut pass);
    // 7. UI
    self.egui.render(&mut pass, &self.document_selection);
    // 8. Present
}
```

**脏标记分级**（避免过度重算）：
`None → Transform（只重绘自身）→ Style（可能触发布局）→ Layout（重排子树）→ Structure（重建索引与静态层）`

---

## 四、Vello 编码策略

```rust
// 并行编码：把可见节点按 512 个一组切片，rayon 并行生成 SceneFragment，再合并
let fragments: Vec<SceneFragment> = visible
    .par_chunks(512)
    .map(|chunk| {
        let mut frag = SceneFragment::default();
        let mut enc = frag.encoder();
        for node in chunk {
            match node.kind {
                Box    => enc.fill(Fill::NonZero, mat, brush, None, &rect),
                Vector => enc.stroke(&stroke, mat, brush, None, &path),
                Text   => enc.draw_glyph_run(...),
                Image  => enc.draw_image(&image, mat),
                Frozen => enc.draw_image(&snapshot, mat),
                _ => {}
            }
        }
        frag
    }).collect();
// 按 z 序顺序 append 到主 Scene
```

要点：
1. **并行编码是 Vello 的原生能力**（`SceneFragment` 可脱离 GPU 构建），多核利用率是它相对 Skia 的优势之一。
2. **顺序敏感**：fill/stroke 顺序 = z 序，合并时必须保持顺序（并行切片但不并行合并）。
3. **裁剪用 clip layer**：蒙版 = `enc.push_clip_layer` / `pop_clip_layer`，闭合必须严格配对（RAII 封装防止漏配）。
4. **Brush 复用**：纯色/渐变 brush 做 interning，避免重复构造开销。

---

## 五、文本管线

```
字体加载(FontContext) → Parley 布局(RangedBuilder + 样式 span) → 
形状/位置(GlyphRun) → Swash 光栅化(alpha/SDF) → Glyph Atlas(纹理) → Vello draw_glyph_run
```

| 环节 | 策略 |
|---|---|
| 图集 | 单张 2048×2048 R8 图集（可增配第二张），LRU 淘汰，跨帧复用 |
| 光栅 | 常用字号/字重缓存 key = (font_id, glyph_id, ppem, subpixel) |
| 小字号 | < 8px 时用灰度矩形占位（LOD），滚动/缩放时极大提速 |
| 中文 | 字形数量多，图集淘汰策略要保守；首次打开大中文文档预热常用 500 字 |
| 缺字 | fallback 字体链（系统字体枚举 + 用户指定），缺字显示豆腐块并报警 |

**降级路径**：若 Parley/Swash 组合出现质量问题，回退方案 = 用 `skia-safe` 或 `fontdue` 在 CPU 光栅化字形进图集（Vello 只负责贴 quad）。接口先抽象成 `trait TextShaper`，避免后期大改。

---

## 六、图像与资源

| 项 | 策略 |
|---|---|
| 解码 | 后台线程（`image` crate），主线程只上传纹理；超大图（>4096²）分块/降采样 |
| 纹理 | `Rgba8Unorm`（sRGB 转换在采样时做），`mipmap` 按需 |
| 显存预算 | 默认 1GB，LRU 换出（保留 CPU 侧缩略图作占位） |
| SVG 导入 | usvg 解析 → 转为 kurbo 路径 + Vello 绘制（保持可编辑）；复杂 SVG（滤镜/图案）→ 冻结块 |
| 冻结块快照 | 首次渲染时生成一次位图（或请求 Chromium 截图），缓存到 `.vbdoc/cache`，缩放 >150% 提示精度有限 |

---

## 七、拾取（Hit Test）与空间索引

- `rstar` R-tree 存所有节点的**世界坐标 AABB**，结构变更时增量更新（删除+插入，不是全量重建）
- 拾取顺序：z 序从顶向下；`Mod+点击` 跳过当前命中项取下一个（穿透）
- 矢量路径命中：先 AABB 粗筛，再用 kurbo 的 `path.contains(point)` / 轮廓距离判定（描边命中用 2px 容差）
- 框选：R-tree 范围查询 + AI 语义（**相交即选中**，非包含）

---

## 八、视口与 LOD

| 视口状态 | 策略 |
|---|---|
| 平移/缩放中 | 静态层拉伸采样；文本与细节省略为灰条；只渲染 LOD0 |
| 静止 100ms 后 | 重建静态层（全精度），分帧执行（每帧最多 8ms 预算，避免长卡顿） |
| 缩放 < 20% | 跳过文本/阴影/渐变细节，只画色块 |
| 缩放 > 400% | 只渲染视口内对象，像素网格可见 |
| 对象数 > 50k | 自动启用"性能模式"：关闭阴影/模糊实时预览（显示为简化效果） |

---

## 九、离屏渲染与原生导出

同一套 Vello 管线复用于导出（保证编辑态与导出一致）：

```rust
let tex = device.create_texture(&TextureDescriptor {
    size: Extent3d { width: w * scale, height: h * scale, .. },
    format: TextureFormat::Rgba8Unorm,
    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
    ..
});
renderer.render_to_texture(&mut scene, &tex_view, RenderParams { width, height, base_color });
// 读回：注意 bytes_per_row 必须 256 字节对齐！
let padded = (width * 4 + 255) & !255;
encoder.copy_texture_to_buffer(...);
queue.submit(..); buffer.slice(..).map_async(..);
// → image::RgbaImage → PNG/JPEG/WebP 编码
```

要点：
- **@2x/@3x 天然无损**：矢量按目标分辨率重新光栅化，不是位图放大
- 超大画布（> `max_texture_dimension_2d`，通常 16384）→ **分块渲染**（tile 512/1024，逐块渲染后拼接）
- 透明背景：PNG 导出 `base_color = Color::TRANSPARENT`（对应 WPI 的 `--transparent`）
- GIF/MP4：逐帧渲染 → PNG 序列（或直写 raw RGBA 到管道）→ FFmpeg（复用 `E:\平日资料\GitHub\MomentShift\tools\ffmpeg_bin`）

---

## 十、Vulkan 特定注意事项

| 项 | 处理 |
|---|---|
| 后端选择 | `Backends::VULKAN` 优先，失败依次 `METAL` → `DX12` → `GL`；启动日志写明实际后端 |
| 实例扩展 | 按需：`VK_KHR_surface` / `VK_KHR_win32_surface`；调试层仅在 debug 构建开启 |
| 特性探测 | `Features::TIMESTAMP_QUERY`（性能面板，缺失则关闭）、`Features::PUSH_CONSTANTS`（Vello 需要） |
| 多采样 | MSAA 4x 默认（性能好、质量够）；设置可切 关闭/2x/4x/8x |
| 呈现模式 | `Fifo`（vsync，默认）；`Mailbox`（低延迟，设置项，可能导致撕裂） |
| 设备丢失 | 监听 `DeviceLost`，自动重建 device + 全量重上传资源 + 恢复文档（必须做，AMD 驱动休眠唤醒常见） |
| AMD RDNA4 (RX 9070 GRE) | wgpu/Vulkan 支持良好；已知需避开显式 tile 尺寸相关的非标准路径（与 ncnn 经验一致：**用默认/auto，不要手动调 tile**） |
| 显存 | 纹理预算 + LRU；显存不足告警而非崩溃 |
| 驱动黑名单 | `gpu_drivers.toml` 记录已知问题驱动；命中时提示"性能模式"或 GL 回退 |
| 多显示器 | 不同 DPI 显示器间移动窗口时，重建 swapchain 并刷新静态层 |

---

## 十一、UI 与画布合成（egui）

- 单一 `wgpu::Device` + `Queue`，egui 通过 `egui_wgpu::Renderer` 使用同一 device
- 画布内容先渲染进**离屏纹理**，再由 egui 以 `TextureId` 绘制为中央面板（保证 UI 可叠加半透明/圆角/阴影，且与画布同帧无撕裂）
- 备选：画布直接渲染到 surface，UI 在第二个 pass 叠加（省一次拷贝，但 UI 无法对画布做模糊/圆角）→ **v0.1 用此方案**（更快落地），v0.4 视需要切换

**输入路由**：egui 判断是否消费了事件（面板/输入框/对话框），未消费才传给画布。`Space`/`Alt`/`Shift` 等修饰键状态需同时暴露给画布（拖动中按 Alt 复制依赖它）。

---

## 十二、性能度量（内置）

| 指标 | 来源 |
|---|---|
| FPS / 帧时间 | CPU 计时 + GPU timestamp query |
| 编码耗时 / 光栅耗时 | Vello render 前后打点 |
| 布局耗时 | Taffy 计时 |
| 可见节点数 / 绘制指令数 | 场景编码统计 |
| 显存占用 | 纹理缓存统计 |

- `设置 → 性能` 面板实时显示；超预算自动建议（"对象过多，建议启用性能模式"）
- **性能回归门禁**：CI 中跑基准场景（见 10 篇），帧时间回归 >10% 直接失败

---

## 十三、模块划分

```
crates/vb_render/
├── gpu/         adapter/device/surface 选择、能力探测、device-lost 恢复
├── encoder/     Document → Vello Scene（并行分片）
├── text/        FontContext、Parley 封装、GlyphAtlas
├── image/       解码队列、纹理缓存、LRU
├── svg/         usvg → kurbo
├── overlay/     选区/手柄/智能参考线/网格/画板标签
├── cache/       静态层纹理、冻结块快照
├── offscreen/   render-to-texture、分块渲染、读回编码
└── stats/       GPU 计时、帧统计
```

---

## 十四、验收点

- [ ] 冷启动（空文档）≤1.5s；打开 5 万对象文档 ≤5s
- [ ] 5 万矩形文档：平移/缩放 ≥60fps（RX 9070 GRE @1440p）
- [ ] 拖动单对象（1 万对象文档中）重绘 ≤8ms
- [ ] 首帧 ≤100ms
- [ ] 设备丢失可自动恢复（人为触发：更新驱动/禁用显卡）
- [ ] 无 Vulkan 环境（强制 `--backend gl`）可正常启动，功能不降级到不可用
- [ ] 显存占用 5 万对象 + 200 张图 ≤1.5GB
- [ ] 导出 10000×10000 PNG 不失败（分块路径生效）
- [ ] 内置性能面板可显示上述所有指标
