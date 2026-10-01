# 全 GPU 加速调研 · Kiln 动画导出(2026-10-01)

> 问题:当前动画导出仍是"截图 → 管道 → ffmpeg"架构,GPU 只吃到视频编码
> 一小块(9070 GRE 实测 GPU 7% / CPU 99%),单实例 ~6 帧/s。能不能全 GPU?
> 本文先调研(GitHub 轮子 + 网络方案)→ 再实测验证 → 最后下结论。
> **结论先行:可行,且已在本机实测出 11× 的单实例提速证据(6.1 → 68.5 帧/s)。**

---

## 一、现状:每帧的活都在哪(实测汇总)

当前流水线(0.12.x,1080p,GT 710 弱机 + Chrome 154):

| 环节 | 落点 | 实测耗时/帧 | 能否上 GPU |
|---|---|---|---|
| SEEK 绘制(JS canvas fillText) | CPU(JS) | 3-20ms(页面而异) | ✓ canvas2D GPU 加速 |
| 页面光栅 + 合成 | GPU(0.12.2 起默认)/CPU | 含在上项 | 已 GPU |
| **截图编码(PNG/JPEG)** | **CPU(Skia)** | 80ms(JPEG,shell)~155ms(new) | ✗ CDP 不暴露 GPU 编码 |
| base64 + 管道传输 | CPU | ~5ms | — |
| **视频编码** | GPU(AMF/NVENC)或 CPU(x264) | 已硬件化(候选链) | 已 GPU |

**卡脖子的就是截图编码这一环**:CDP 的 DOM→位图通路
(`Page.captureScreenshot` / `beginFrame`)全部由 Skia 在 CPU 上编码,
这是架构上限,flags 调不出来。

## 二、候选方案调研

### 方案 A(推荐):WebCodecs 页内硬编 —— canvas → VideoFrame → VideoEncoder → 封装

**原理**:页面内直接 `new VideoFrame(canvas)`(GPU 位图引用,零拷贝)→
`VideoEncoder.encode()`(Windows 上走 D3D11VideoEncoder,即 AMF/NVENC/
QSV 硬件编码)→ `EncodedVideoChunk` → JS 封装成 MP4。**全程不过 CPU
像素,没有截图、没有管道、没有 ffmpeg**。

- 成熟度:这是业界成熟模式。参考
  [Fast video rendering and encoding using web APIs](https://pietrasiak.com/fast-video-rendering-and-encoding-using-web-apis)、
  [Chrome WebCodecs 最佳实践](https://developer.chrome.com/docs/web-platform/best-practices/webcodecs)、
  [mp4box.js #243:浏览器内 WebCodecs+封装产 MP4](https://github.com/gpac/mp4box.js/issues/243)。
- 封装轮子:**[mediabunny](https://github.com/Vanilagy/mediabunny)**
  (MIT,零依赖 TypeScript,mp4-muxer/webm-muxer 的官方继任者,
  [mediabunny.dev](https://mediabunny.dev/))——`Output` +
  `BufferTarget` 可在内存里产完整 MP4;同类的 mp4box.js 亦可但 API 重。
- 硬编可用性:Windows 10/11 上 D3D11VideoEncoder 已随 Chromium 启用
  ([Chromium 40249559](https://issues.chromium.org/40249559));headless
  下需要 GPU flags([官方文档](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/gpu/using-gpu-hardware-in-headless-chrome.md))。
- 已知坑:大量帧编码在 Windows 有偶发报错案例
  ([w3c/webcodecs #748](https://github.com/w3c/webcodecs/issues/748));
  强制关键帧可能让 N 卡硬编回退软编——实现时 keyframe 间隔放宽 + 失败
  自动回退截图车道。

**本机实测(真实 MV 工程 world.execute(me);,1080p,Chrome 154 headless,
SEEK 驱动 60 帧,`crates/vb_browser/examples/webcodecs_poc.rs`)**:

| 配置 | 结果 |
|---|---|
| GPU 光栅 + WebCodecs(prefer-hardware) | **68.5 帧/s(14.6ms/帧)**;SEEK 仅 3.3ms,VideoFrame ≈0ms,flush 成功 60 chunks |
| GPU + 重页面(6000 fillText) | **39.7 帧/s(25.2ms/帧)**;SEEK 18.5ms |
| 软件光栅(--disable-gpu) | `isConfigSupported → unsupported`:硬编直接不可用 |

对照:同页面当前截图管线 **6.1 帧/s** —— **单实例 11×**,且这还没算
多实例并行(重页面单实例已 39.7 帧/s,超过 60fps 实时线,一个实例即可
实时渲染)。

**架构增量**(Kiln 侧):
1. `--render webcodecs` 车道:检测 `window.SEEK` + 全屏 `<canvas>` →
   注入编码脚本(mediabunny bundle 由 Kiln 静态服务附带);
2. 静态服务新增 `POST /__kiln-upload`:页内编码完成后经本地回环把 MP4
   字节流上传(避免 base64 evaluate 搬大文件);
3. 时间戳显式 `i/fps` 微秒——比截图管线**更确定**(SEEK 纯函数 + 无墙钟);
4. `isConfigSupported` 探针失败 / encode 中途报错 → 自动回退现有截图
   车道(保留全部既有能力)。

**限制(诚实口径)**:只覆盖「canvas + SEEK」页(MV 工程全部命中;
纯 DOM/CSS 动画页回退截图车道);输出 H.264 MP4(WebCodecs 能力域);
音频仍不在 Kiln 范围;强制关键帧场景需要失败回退兜底。

### 方案 B(否决):CDP `Page.startScreencast`

合成器逐帧推 JPEG——仍是 CPU 编码 + base64,且节奏受合成器调度
(非确定性)。无增益。

### 方案 C(否决):MediaRecorder + canvas.captureStream()

实时墙钟绑定,时间戳不可控(确定性破坏),VP8/9 编码质量/兼容弱。
与 SEEK 纯函数方法论冲突。

### 方案 D(参考,不解决):既有确定性渲染轮子

- [timesnap / timecut](https://github.com/tungs/timesnap)(虚拟时间 +
  逐帧截图)——确定性金标准,但就是我们现在这套截图架构,slow-by-design;
- [puppeteer-capture](https://github.com/alexey-pelykh/puppeteer-capture)
  (`HeadlessExperimental.beginFrame` 精确控帧)——同样截图制;
- [Remotion](https://github.com/remotion-dev/remotion)——服务端逐帧截图
  + Lambda 分布式,靠横向扩容不靠 GPU。
- 结论:**没有任何现成轮子做「SEEK 页面 + WebCodecs 硬编」**,需要自研
  (但注入脚本部分很小,核心是胶水)。

### 方案 E(已做):ffmpeg 全硬件链

AMF/NVENC 探针候选链已上线;WebCodecs 路径下 ffmpeg 整个消失,此项
自然退役(截图车道保留)。

## 三、预期收益(按实测推算)

| 场景 | 现状(0.12.x 截图车道) | WebCodecs 车道(推算) |
|---|---|---|
| 轻页面单实例 | 6.1 帧/s | 68.5 帧/s(**11×**) |
| 重页面(6000 文本)单实例 | ~2-4 帧/s | 39.7 帧/s |
| 3 分钟 1080p30 MV(5400 帧) | 3 实例 ~2.5 分钟 | 单实例 ~1.3 分钟;2 实例 <1 分钟 |
| 1080p60(10800 帧) | 3 实例 ~5 分钟 | 2 实例 ~1.5 分钟 |

CPU 侧:JS SEEK 与编码控制剩少量 CPU;**像素工作(绘制光栅、YUV 转换、
编码)全部在 GPU** —— 正面回应"GPU 7% / CPU 99%"。

## 四、结论与实施建议

1. **结论:全 GPU 加速可行,且已被实测证明**(本机弱卡 11× 提升;硬编
   不可用时自动回退现有车道,无能力退化风险)。
2. **实施分三阶段**:
   - 阶段 1:MVP——canvas+SEEK 检测、mediabunny 注入、上传端点、
     `--render webcodecs` 旗标(默认 auto:探针通过走 WebCodecs,否则
     截图车道);
   - 阶段 2:正确性自验(ffmpeg 解码回读抽帧 vs SEEK 截图基线逐帧比对)
     + 编码失败自动回退 + Windows 硬编 flaky 兜底(#748 类);
   - 阶段 3:多实例吞吐适配(单实例超实时后,并行用于 >60fps 或 2K+
     场景)、码率/profile 自动档位、(可选)HEVC/AV1 硬编档。
3. **风险登记**:Windows 硬编 flaky(#748)、强制关键帧回退软编、
   mediabunny bundle 体积(~100KB 级,可接受)、非 canvas 页面回退。

## 附:参考资料

- [MDN WebCodecs](https://developer.mozilla.org/en-US/docs/Web/API/WebCodecs_API) ·
  [webcodecsfundamentals.org](https://webcodecsfundamentals.org)
- [mediabunny(GitHub)](https://github.com/Vanilagy/mediabunny) ·
  [mediabunny.dev](https://mediabunny.dev/)
- [Chromium:headless 使用 GPU 硬件](https://chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/gpu/using-gpu-hardware-in-headless-chrome.md) ·
  [Chromium 40249559:硬编 VideoEncoder 启用](https://issues.chromium.org/40249559)
- [w3c/webcodecs #748](https://github.com/w3c/webcodecs/issues/748) ·
  [StaZhu/enable-chromium-hevc-hardware-decoding](https://github.com/StaZhu/enable-chromium-hevc-hardware-decoding)
- [timesnap](https://github.com/tungs/timesnap) ·
  [puppeteer-capture](https://github.com/alexey-pelykh/puppeteer-capture) ·
  [Remotion](https://github.com/remotion-dev/remotion)
- 本仓实测:`crates/vb_browser/examples/webcodecs_poc.rs`(可复现,
  `cargo run -p vb_browser --example webcodecs_poc <url|dir> [帧数]`,
  `VB_GPU=1` 开 GPU)
