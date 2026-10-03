# ADR-0047: 画布上屏通道 —— 裁定 GPUI 原生翻译层为主线,vello_cpu 读回为兜底车道

- 状态:**建议稿**(22 篇 §4 R0 交付 3 的 spike 产出;主 Agent 验收后定案)
- 编号说明:22 篇原文写「ADR-0046 画布上屏通道」,但 0046 已被
  「降级必须可观测」占用,本文顺延 **0047**(22 篇 §8 台账相应顺延,
  原 0047 Kiln 取消语义改 0048,余类推)。

- 背景:换宿主后画布怎么上屏是 R0 最后一道技术未知数(22 篇 §5 硬骨头
  #7 的 4096 纹理上限也在其中)。三条候选路:
  - **(a) 离屏纹理**:现有 vb_render wgpu 管线(vello 0.10)离屏渲染 →
    每帧拷出 → GPUI `img()` 贴图;
  - **(b) sable-paint 通道**:DrawList → sable-paint `PaintSink` 原语 →
    sable 的 GPUI 呈现元素上屏;
  - **(c) GPUI 原生翻译层**:DrawList 直接翻成 gpui paint 调用
    (quad / path / text),不经 vello。
  R0 以可运行 spike 实测三路,判据:1080p 下 1 万节点 60fps、缩放无糊、
  内存峰值 sane。

- 关键事实(gpui 0.2.2 源码核实,registry 实证):
  1. gpui 0.2.2 Windows 平台是 **DirectX 渲染器**(`platform/windows/
     directx_renderer.rs` + HLSL),文本走 DirectWrite;
  2. `Window::paint_image` 只接受 CPU 内存 `RenderImage`(`assets.rs`),
     **无外部纹理导入 API**(没有 NT handle / shared texture 通道,
     `paint_surface` 仅 macOS CVPixelBuffer)—— 22 篇 §6.4 设想的
     「零拷贝纹理上屏」在本版本**不存在**,(a)/(b) 的现实形态只能是
     「GPU 渲染 → `copy_texture_to_buffer` 读回 → 每帧上传」;
  3. `Background` 只有 **2 停靠点线性渐变**(`color.rs` BackgroundTag:
     Solid/LinearGradient/PatternSlash,无 radial、无多停靠点);
  4. `PathBuilder::build()` 在主线程同步做 lyon tessellation,且 gpui
     Path 是 immediate-mode 图元,**无跨帧缓存**。

## 实测(spike:`crates/vb_shell/src/bin/canvas_spike/`,可复现)

- 环境:i5-12400F / **NVIDIA GT 710 2GB(下限硬件,Vulkan)** / 16GB;
  release build;窗口 1280×800(设备像素 1264×752);10,000 节点确定性
  场景(5,500 实心矩形 + 1,000 四停靠点线性渐变 + 1,000 圆角描边 +
  1,000 椭圆 + 1,200 贝塞尔(描边/填充)+ 400 条 CJK 文本,~1/8 带
  ±30° 旋转,不透明度 0.25–1.0);每配置 200–300 帧取稳态。
- 复现:`cargo run -p vb_shell --bin canvas_spike --release -- \
  --route a|b|b2|c --nodes 10000 --zoom 1 --frames 300 --csv out.csv`
  (a=离屏读回 / b=sable VelloSink GPU+读回 / b2=sable CpuRenderer 现状 /
  c=gpui 原生;`--zoom 4` 看缩放,`--dump x.png` 出通道 parity 图)。

### 帧率与分解(release,缩放 1x)

| 节点 | (a) vello GPU+读回 | (b) sable VelloSink+读回 | (b2) sable vello_cpu | (c) gpui 原生 |
|---|---|---|---|---|
| 1,000 | 53.0 fps(p50 18.5ms:译 0.4 + 读回 16.9) | — | **61.7 fps**(16.0ms) | 40.8 fps(24.5ms:译 3.5 + paint ~21) |
| 3,000 | 23.0 fps(43.2ms:译 1.2 + 读回 41.0) | — | 33.5 fps(29.3ms) | 15.0 fps(66.6ms:译 9.8 + paint ~57) |
| 10,000 | 7.6 fps(p50 130ms:译 4.1 + 读回 130.5) | 7.7 fps(127.8ms:译 9.0 + 读回 119.7) | 10.6 fps(88.3ms) | 4.6 fps(217.2ms:译 31.4 + paint ~186) |

- 缩放 4x(10k):a 9.4 fps / b2 **19.3 fps** / c 7.1 fps —— 三路在
  settled 缩放下全部**矢量重渲,边缘与文字无糊**(截图目检 + 同场景
  parity dump 互证);(b) 与 (a) 画面逐像素同源(vello 同一 GPU 路径)。
- 内存(10k,40s 长跑):
  - **(c) 平稳**:Private 515MB 不随时间增长;
  - **(a)/(b)/(b2) 泄漏**:Private 随时间线性增长(b2 实测
    1527→2481MB/16s ≈ **+60MB/s**),增速 = 每帧一张全窗口
    `RenderImage` 的纹理字节 —— gpui 0.2.2 Windows atlas **不回收被
    替换的图像纹理**(每帧 new RenderImage → 新 ImageId → 新纹理,
    旧的不释放)。这不是 spike 代码的引用错误:(c) 不走 paint_image,
    同窗口同帧率完全平稳。
- warmup(首帧一次性):(a)/(b) ~170–270ms(vello shader 编译)+
  字体发现;(c) ~43ms(字形塑形)。
- 保真差((c) 现档位):多停靠点线性渐变取首尾 2 停靠点近似
  (gpui 0.2.2 无多停靠点);径向渐变以实心中灰降级;旋转矩形走
  path(quad 无旋转);Path 内渐变取单色。其余(实心/圆角/描边/
  椭圆/贝塞尔/CJK 文本)全保真。(b)/(a) 对 DrawList 渐变全保真,
  但文本要么缺位((a):vb_render gpu.rs 对 DrawKind::Text 直接跳过,
  沿袭「egui 覆盖层近似」旧档),要么走 skrifa 轮廓((b),与 (c) 的
  DirectWrite 文本等清晰)。

## 判据兑现(诚实结论)

1. **1080p 下 1 万节点 60fps:三路在本机全部未达**(最好 10.6 fps)。
   本机是下限硬件(GT 710 的读回 stall + 无纹理回收 + vello compute
   弱),但把「判据在本机达成」说成本轮结论是不诚实的 —— 如实记录,
   并给出各路瓶颈与优化路径:
   - (a)/(b):瓶颈在读回(`render_to_texture`→`map_async` 同步 stall,
     GT 710 上 ~120ms/帧)+ **每帧纹理泄漏**(上游缺陷,见上);
     异步 PBO 式读回 + 上游修 atlas 回收也只到「能看」,离 60fps 远;
   - (b2):瓶颈纯 CPU 光栅化(~9µs/节点),随节点数线性,无优化杠杆;
   - **(c):瓶颈是每帧 lyon tessellation(无缓存)+ 10k 图元逐帧
     提交,全部是 CPU 侧、本仓可控** —— 静态场景加「路径 tessellation
     缓存(key = sid + zoom 桶 + 几何哈希)」与脏区后,稳态帧只需
     提交缓存产物;1k 节点实测已 40.8 fps,证明基线健康。
2. **缩放无糊:三路达成**(settled 缩放全部按目标分辨率矢量重渲;
   (c) 文本按 font_size×zoom 走 DirectWrite 塑形缓存,天然清晰)。
3. **内存 sane:仅 (c) 达成**;读回族通道按现上游必然线性增长。

## 裁定(建议稿)

1. **主线 = (c) GPUI 原生翻译层**。理由:
   - 唯一内存平稳的通道;
   - **4096 上限(硬骨头 #7)结构性消解**:不存在任何整幅纹理
     (旧宿主 `vb_app/src/app/canvas.rs` L129 `min(4096)` 的约束对象
     在 (c) 中不存在),8K 屏/深缩放天然无糊,无需平铺;
   - 与宿主零胶水:无跨进程/跨 API 纹理同步,直接复用 gpui 的
     quad 快路径与 DirectWrite 文本(中文塑形实测正确);
   - 瓶颈全部在本仓可控范围(缓存策略),而 (a)/(b) 的两大硬伤
     (同步读回、atlas 不回收)都要改上游 gpui。
2. **兜底车道 = (b2) vello_cpu 读回**(sable `PaintSink` 抽象,与
   GPU 无关的 parity/导出预览通道),但**只做损伤驱动渲染**(有脏区
   才重渲)——每帧纹理是泄漏源;sable-canvas 现状 `gpui_element` 的
   每帧整幅重渲在真实画布上同病,接入时必须带脏区。
3. **(a)/(b) 离屏纹理路线否决(gpui 0.2.2 下)**:零拷贝 API 不存在
   (上文事实 2),读回 stall + 泄漏都命中。若未来 gpui 提供外部纹理
   导入/更新 API,可重启评估(sable-paint 的 `VelloSink`/`GpuGuard`
   资产保留,翻译层代码正是本 spike 的 `route_sable.rs`)。
4. (c) 的 R1 必做清单(从 spike 已知缺口收口):
   - 路径 tessellation 缓存 + 脏区(判据 60fps 的主杠杆);
   - 多停靠点渐变:向 gpui 上游提 multi-stop linear/radial
     (现 BackgroundTag 结构不支持);过渡期 >2 停靠点取首尾近似并在
     渲染审计中记「渐变简化」;
   - 径向/锥形渐变与旋转 quad 的 path 化收口;
   - 在目标开发硬件(独显)复测 10k@60fps 并回填本 ADR 数据表。

## 落地

- `crates/vb_shell/src/bin/canvas_spike/`(main/scene/gpu_frame/
  route_sable/route_gpui,~1,300 行):四通道可切换、`--frames/--csv/
  --dump/--zoom/--route` 自动化、overlay 实时统计;
- `crates/vb_shell/Cargo.toml`:spike 例外依赖已注明
  (sable+`backend-auto`、vb_render、vello、pollster、image),
  `src/main.rs` 零改动;vb_kit / vb_session 公开 API 零改动;
- 门禁:`cargo clippy -p vb_shell --all-targets -- -D warnings` 零警告。

## 取舍与风险

- 判据 1 本机未达就把主线押给 (c) 是**有条件的押注**:条件 = 「缓存 +
  脏区 + 目标硬件」兑现 60fps。回退链:若 R1 在目标硬件复测仍不达标,
  退 (b) sable-paint 通道 + 视口尺寸纹理平铺(22 篇 §4 R1 预案),
  并向上游提交:gpui 外部纹理导入、atlas 纹理回收、多停靠点渐变
  —— 三项均在 spike 中留下了可复现的失败证据与复现命令。
- (c) 的渐变保真差是**记账的降级**(渲染审计可见),不是静默丢失;
  与 ADR-0046「降级必须可观测」同纪律。

## 证据索引

- 帧率/内存原始输出与 CSV:`D:\Temp\rel_{a,b,b2,c}.log`、
  `rel_{route}_n{nodes}.log`、`rel_{route}.csv`(会话工件,数据已抄录
  本 ADR 表格;复现命令见上);
- 4x 缩放截图(三路互证清晰):`D:\Temp\rel_{b2,c,a}_zoom4.png`;
- 通道 parity dump((a) 与 (b2) 同场景):
  `D:\Temp\dump_a.png`、`D:\Temp\dump_b2.png`;
- 泄漏证据:`mem_a_long.log`(1377→2536MB/40s)、
  `mem_{b2,c}_long.log`(b2 增长 / c 平稳)。
