# Changelog

## 0.12.3(2026-10-01)

下游第二轮实测四连报修复。

### Fixed(并发负收益与 w4 必败,#1)

- **默认 workers 改 1**:两轮下游实测多实例都是负收益——轻量最小页面
  也随段数变慢(w1 10.6 → w2 5.9 → w3 1.8 帧/s),现象是跨实例串行点
  而非 CPU 不足,在定位清楚前不再按 CPU/2 猜。并发是显式 opt-in
  (`--workers 2..16`,超限时段级候选回退兜底);相关自动档探针
  (冷启动测速/内存预算/会话探针)随默认 1 退役。
- **DevTools HTTP 超时放宽带重试**:`/json/new`(开标签页)5s→30s、
  `/json/version` 5s→15s+一次重试——多实例并发冷启动时浏览器自带
  DevTools HTTP 服务显著变慢,对应下游"w4 分段渲染失败:HTTP 读取
  超时"。

### Fixed(beginFrame 在 Edge 上不存在,#2)

- Edge 没有 `HeadlessExperimental` 域,此前每帧都重试一次必然失败的
  beginFrame 再退回 captureScreenshot——每帧多付一次死往返。现**首帧
  失败即缓存结论**,本段直接走 captureScreenshot(告警只出一条)。
  Edge 实测:告警恰 1 条、导出成功。

### Fixed(色彩 VUI 补全,#3)

- 硬件编码器(nvenc/amf/qsv)不一定回填 transfer/primaries(下游实测
  color_space=bt709 但 transfer/primaries=unknown)。MP4 输出统一追加
  `h264_metadata` bsf 直接改写 SPS VUI。实测(trace_headers):
  `colour_primaries=1 / transfer_characteristics=1 / matrix_coefficients=1`
  (全 BT.709),`video_full_range_flag=0`(tv)。

### Fixed(%TEMP%\kiln-* 残留,#4)

- 浏览器首次启动时清扫 %TEMP% 下陈旧的 `kiln-browser-*`/`kiln-anim-*`/
  `kiln-mp4-*`/`kiln-gif-*` 工作目录(进程崩溃/强杀时 Drop 不执行,
  下游机器上按几十个计);只动 mtime 超 6 小时的目录,并发运行中的
  新目录绝不误删,best-effort 静默。

## 0.12.2(2026-10-01)

### Changed(动画车道 GPU 光栅默认开)

下游 9070 GRE 实测"GPU 占用 7% / CPU 99%":根因是流水线形态——SEEK 的
JS 绘制、截图 JPEG 编码默认还在 CPU,浏览器光栅默认软件(--disable-gpu)。
本轮把能上 GPU 的都默认上:

- **动画车道(MP4/GIF)GPU 光栅默认开**(ANGLE→D3D11,渲染与合成落
  显卡;CanvasOopRasterization 让 canvas 位图光栅化也进 GPU 进程)。
  `--no-gpu` / `VB_GPU=0` 显式回到软件光栅(跨机逐像素可复现口径);
  静态 PNG/PDF 车道维持软件光栅不变。AA 微差 MAD ≈ 1/255(与 Playwright
  GPU 改造实测一致,同路径自身可复现)。
- **编码器透明化**:结果 JSON 新增 `encoder`(实际使用的 h264_nvenc/
  h264_amf/h264_qsv/libx264)与 `gpu`(光栅是否走 GPU)字段——"GPU 有没有
  用上"从此看 JSON 即可,不用读 stderr、不用猜。`selfcheck` 新增
  `encoder_lane` 字段报告可用硬件编码器(如 `h264_nvenc(硬件编码可用)`),
  ffmpeg 缺硬编时给出明确提示。

### 使用者怎么验证 GPU 真的用上了

1. `selfcheck` → `encoder_lane` 是否为硬件编码器(9070 GRE 应为
   `h264_amf`;若报 none 或 x264,说明 ffmpeg 缺 AMF 运行时,换完整版
   ffmpeg 构建即可,视频编码那一份 CPU 就省下来了);
2. 导出结果 JSON → `"gpu":true` + `"encoder":"h264_amf"`;
3. 任务管理器 GPU 页签:**光栅/合成走"3D"图表,AMF 走"Video Encode"
   图表** —— 只看总 GPU% 会低估(VCN 编码块不占 3D)。

## 0.12.1(2026-10-01)

下游实测三连报修复(浏览器选错静默降级 / 并发负收益 / 色彩口径)。

### Fixed(浏览器探测与降级语义,#1,最要紧)

- **探测顺序改回系统浏览器优先**:0.11.1 起自动优先 Playwright 缓存的
  chrome-headless-shell,下游环境里来路不明的旧版 shell
  (HeadlessChrome/153.0.8010.12)captureScreenshot 帧级挂死(单帧 180s
  超时)→ 整条浏览器车道静默降级,产物变 1 帧、尺寸瞎猜。现顺序:
  `VB_BROWSER_PATH` → 系统 Edge → Chrome(含 %LOCALAPPDATA%/%PROGRAMFILES%
  派生)→ shell 仅在 `VB_PREFER_SHELL=1` 或系统无浏览器时参与。
  shell 的 ~2× 截屏速度仍在,确认自己缓存的 shell 可用后设环境变量取回。
- **动画车道不再静默降级出废片**:自研逐帧(Lane K)只覆盖 4 类动画轨道,
  浏览器车道失败时降级产物是"1 帧 + 尺寸瞎猜"。现 auto 模式下动画车道
  失败即显式失败(stderr JSON 带原因 + 修复指引),确要降级产物显式
  `--engine native`。静态 PNG 车道的降级仍有意义,保留,且结果 JSON 已带
  `engine_fallback`/`degraded` 标记。

### Fixed(并发负收益,#2)

- **冷启动测速封顶**:自动 workers 在派发前先单实例采 3 帧测节奏——
  ≥400ms/帧锁 1 实例、≥200ms 限 2(下游重页面单实例 435ms,2/3 实例
  并发是超线性负收益:2.3 → 0.9 → 0.26 帧/s);阈值口径宁慢勿废。
  显式 `--workers` 不受影响。本机验证:1080p 不触发(auto 3),2K 触发
  (201ms → 限 2)。
- **错峰启动**:多实例浏览器逐个间隔 600ms 拉起,不再同瞬间冷启动互踩。
- **attach 超时放宽**:ws 连接 10s→30s,Page/Network/Runtime.enable
  5s→30s 且带一次自动重试(下游 4 实例并发时 5s 超时直接 attach 失败)。

### Fixed(色彩口径,#3)

- MP4 输出固定 **yuv420p + tv 色程 + bt709 全标记**
  (`scale=out_range=mpeg:out_color_matrix=bt709` + `-color_range tv
  -colorspace/-color_primaries/-color_trc bt709`),动画流水线与 native
  帧序列两条路径同改。此前输出 yuvj420p + color_range=pc + transfer/
  primaries unknown(Y 全程 0-255),与电视范围素材(NLE/拼接,典型
  16-235)电平不匹配。实测:ffprobe 报 `yuv420p(tv, bt709)`,
  YMIN/YMAX=9/211(内容为暗色调 MV,未顶满 235 属内容决定)。

## 0.12.0(2026-10-01)

生产化迭代批次(`docs/production-iteration-2026-10-01.md`):主 Agent 拆包
派发、三个子 Agent 于独立 worktree 并行实现、主 Agent 验收合并;全量门禁
729 测试零失败。

### PA · 断电安全写盘(vb_doc)

- `write_project` 全部落盘改原子写:同目录临时文件(`.tmp-<名>-<pid>`)→
  `fs::rename` 原子覆盖(Windows rename 带 REPLACE_EXISTING);写失败/
  rename 失败清理临时文件,错误信息带路径。此前直接 `fs::write`,保存中途
  断电/崩溃会留下截断的 `index.html`(用户文档永久损坏,v0.2 延期项就此
  落地)。
- 多文件项目 index.html **最后落盘**:存在 index.html 即完整文档,断电窗口
  内"可打开概率"最大化。
- 新增 3 个单测:失败注入(css 临时路径预占为目录)下旧 index.html 完整
  保持、无临时残留;rename 目标被占用(Windows 独占句柄)错误带路径且
  原文件未动;完整性与残留检查。

### PB · 静态 PNG 车道 settle 收敛(vb_browser/vb_kiln)

- 根因(下游实测单帧 27.5s):settle 协议**结构性固定成本 + 无总预算**——
  判稳固定 4 连拍(每拍在慢机 ~0.5-1s)+ 400ms 头 sleep + 3×200ms 间隔 +
  无限动画页 2×3s 宽限 + 滚动 reveal ≤40×130ms,叠加即 20s+。
- 修复:
  - 页内静止探针(无 running 动画/无待加载图/字体就绪)→ 确定性页面
    2 拍即收敛(原 4 拍);非静止页维持原口径;
  - **settle 总预算硬上限 5s**(`VB_SETTLE_BUDGET_MS` 可覆盖),assets/
    滚动/定格/稳定全部计入,到顶即用当前帧导出并追加 warning(降级可
    观测,不许静默吃预算);
  - 单屏页跳过滚动 reveal;所有 >130ms 的 sleep 逐一注释理由。
- 实测:MV 工程单帧 PNG 27.5s(下游慢机)/ 5.2-8.7s(本机)→ **2.4s**
  (本机主 Agent 复核);静态示例输出与修复前**逐字节一致**(含 Edge
  headless=new 路径);无限动画夹具按预算截断且告警可见。

### PD · native MP4 编码器候选链(vb_kiln)

- `frames::encode_mp4` 不再硬编码 libx264:复用 animlane 的候选链
  (nvenc→amf→qsv→x264,运行时探针)逐个尝试;候选全败降级 GIF 流
  (与"无 ffmpeg"语义一致)。此前在裁剪版 ffmpeg(剪映等常见发行,无
  libx264)上 MP4 必废且无降级,`selfcheck` ok:false —— 现九格式自检
  全绿。

### PC · CLI 错误一致性(kiln / vellum-mcp)

- `kiln` 入口消灭全部 `expect/panic` 错误出口(此前 exit 101 + backtrace
  噪音):错误改 stderr 单行 JSON `{"ok":false,"error":"…"}`(内置 jesc
  转义),退出码对齐 0/2/3/4(成功/用法/输入/IO),`--help`/`--version`
  退 0 —— 与 kiln-cli 完全同口径,上游脚本可稳定解析。
- `vellum-mcp` 写输出文件前自动建父目录,失败信息带完整路径(此前客户端
  给不存在的目录即失败且无路径上下文);冒烟验证深层目录自动创建。

## 0.11.2(2026-10-01)

### Kiln 提速迭代(单实例 +20%;并发安全自调优)

- **beginFrame `noDisplayUpdates`**:逐帧导出没有"观看者",不再把每帧
  提交到显示表面,只产出截图 —— 单实例 5.1 → 6.1 帧/s(+20%,本机
  GT 710 实测);`VB_BF_DISPLAY=1` 可回退。抽帧对比内容一致。
- **`--disable-lcd-text`**:与 Playwright 参数面对齐,文本光栅省一遍
  次像素 AA 滤波。
- **自动 workers 三重自调优**:`min(CPU/2, 内存预算, 硬件编码器并发
  会话实测)`。内存预算按输出分辨率折算(≤1080p 每实例 1.2GB,2K+
  2.5GB,空闲物理内存经 kernel32 GlobalMemoryStatusEx 查询,零依赖
  手写 FFI);硬件编码器会话数开局并发探针实测(消费级 N 卡有驱动级
  上限,GT 710 实测 3/4 后自动降为 3 实例 —— 此前 4 实例必整段暴毙;
  9070 GRE 的 AMF 无上限,32GB 内存预算下可自动开到 8 实例)。
  显式 `--workers` 仍完全尊重用户,靠候选链回退兜底。
- 本机终态:1080p30 60s = 1800 帧,148s(12.1 帧/s,3 实例,与
  0.11.1 持平 —— 本机出口瓶颈在初代 NVENC 芯片,单实例 +20% 被芯片
  吃掉;9070 GRE 无此瓶颈,上述优化全额生效)。

## 0.11.1(2026-10-01)

下游(9070 GRE 实机)实测反馈的三项落地;全部在本机用实际 MV 工程
(`world.execute(me);`)复现 → 插桩定位 → 修复 → 回归。

### 逐帧截屏的固定开销(建议 #3)——根因是三层,各修一层

下游测到 ~0.9s/帧固定开销且 `--gpu` 无改善;插桩拆解(`VB_ANIM_TIMING=1`
输出 seek/repaint/screenshot 分段耗时)后,实际是三笔独立的账:

- **截图 API 等合成器调度(~530ms,headless=new)**:空闲页面(SEEK 驱动、
  无 CSS 动画/无 rAF)上 `Page.captureScreenshot` 要等 viz 表面产出下一帧。
  现逐帧截屏改走 `HeadlessExperimental.beginFrame`(CDP 显式产帧,产帧 +
  截图一步完成,启动参数 `--enable-begin-frame-control`);不可用时自动
  回退 captureScreenshot(`VB_NO_BEGINFRAME=1` 强制回退)。语义上
  beginFrame 本身就是"seek → 等一次 repaint → 截屏"里的那次 repaint。
- **PNG 编码(~380ms/帧,占大头)**:浏览器内 zlib 压 8MB 原始像素。
  中间帧默认改 **JPEG(q95)**:实测截图 465ms → **80ms**(5.8×);
  MP4 本身就是有损 yuv420p,二次编码后损失通常不可见(实测逐帧对比
  无可见差异)。画质敏感交付用 `--img png`。
- **headless=new 的 viz 表面中转(约 +2×)**:支持 **chrome-headless-shell**
  (Playwright 同款老式 headless 实现):自动探测 Playwright 缓存的最新版
  (`%LOCALAPPDATA%\ms-playwright\chromium_headless_shell-*`),发现即优先
  使用,`VB_NO_SHELL=1` 可关;也可 `VB_BROWSER_PATH` 显式指定。JPEG 下
  shell ~80ms/帧 vs headless=new ~155ms/帧。
- 本机(GT 710 最弱配置)终态:实际 MV 工程 1080p30 60s = 1800 帧,
  **3 实例并行 2 分 27 秒(12.3 帧/s)**,成片精确 1800 帧/60.00s;单实例
  吞吐已超同机 Playwright 参照管线(4.1 vs 3.3 帧/s,后者编码串行)。
  9070 GRE 上按 A 卡 AMF 无并发会话限制 + 更多 worker 估算,1080p60
  全片可进 3 分钟内。

### seek-hook 车道(建议 #1)

- 自动探测序列加入 `window.__SEEK__`(现为 SEEK → `__SEEK__` → seek →
  VB_SEEK);新增 `--seek-hook` 作为 `--seek-fn` 的别名,接受
  `window.SEEK` / `SEEK` 两种写法(自动剥 `window.` 前缀)。
- 逐帧语义:seek → 等一次 repaint → 截屏。beginFrame 路径的 repaint 即
  beginFrame 本身;captureScreenshot 回退路径显式跑双 rAF(250ms 兜底,
  页面无 rAF 流不挂死)。

### 墙钟车道按实际耗时记账(建议 #2)

- **MP4**:两段式——先采 8 帧探针实测采样节奏(median),ffmpeg 以实测
  节奏 CFR 编码。修复旧实现"采样 ~1 帧/s 却按 1/fps 打时间戳 → 成片
  快放且速度不均"。告警输出实测节奏与预估时长(速度为均值口径)。
- **GIF**:帧延迟按实测总耗时均值(下限 20ms)。
- 管道断裂(ffmpeg 中途死亡,典型如 N 卡并发编码会话超限)时带出
  ffmpeg stderr 尾部,真死因不再埋在 `os error 109` 里。

## 0.11.0(2026-09-30)

### Kiln 动画导出重写:流式并行流水线(性能 ~6×)

针对 3 分钟 1080p60 MV 导出 30 分钟 / 2K60 约 1 小时的性能问题,重写
`animlane`(车道 B 动画逐帧)。旧实现的三个结构性瓶颈:墙钟实时采样
(本质上不可并行)、全部帧解成 RGBA 驻留内存(1080p 60fps×3 分钟 ≈ 90GB
靠页面交换硬扛)、`--disable-gpu` 软件光栅 + 帧数据在 Rust/ffmpeg 两侧
反复编解码。新流水线:

- **确定性寻址(SEEK(t) 方法论)**:页面全部 WAAPI 动画 `pause()` 后逐帧
  `currentTime = t` 定位,输出帧严格落在 1/fps 网格(不再有墙钟重采样的
  跳帧/复制帧);时间段零依赖 —— 分段并行成为可能。
- **JS 帧驱动协议**:页面自带确定性时间轴时(确定性渲染工作流的约定
  接口 `window.SEEK(t)` / `window.seek`,逐帧把整片画成一帧),自动探测
  并逐帧调用;`--seek-fn` 可显式指定函数名。**修复实际 MV 工程
  (纯 JS 驱动、零 @keyframes)导出无画面的问题** —— 此前这类页面
  没有任何可寻址动画,帧帧都在截初始状态。
- **自动降级链**:JS SEEK → CSS/WAAPI 动画寻址 → 墙钟回退(无自驱动的
  静止页面给出诚实警告,不再产出黑屏)。
- **流式管道(内存 O(1))**:CDP 截图字节(PNG 无损 / JPEG 直出)不经
  解码直接写 ffmpeg stdin(`-f image2pipe`),ffmpeg 边收边编;每段独立
  mp4,`-f concat -c copy` 零重编码拼接。
- **分段多实例并行**:总帧数均分 W 段,W 个 headless 实例各自渲染编码。
  `--workers 0`(默认)自动 = CPU/2 上限 4;确定性保证并行结果与串行
  一致。帧序号精确:10800 帧进 → 10800 帧出。
- **GPU 光栅化**:`--gpu` / `VB_GPU=1` 走 ANGLE→D3D11(NVIDIA/AMD/Intel
  通用),光栅与合成落在显卡;默认仍为软件光栅保持跨机可复现(ADR-0022),
  GPU 路径 AA 微差 MAD ≈ 1/255(与 Playwright 实测一致)。
- **H.264 硬件编码**:自动探测 nvenc(N 卡)→ amf(A 卡)→ qsv(Intel)
  → libx264,且做**运行时探针**(编 2 帧测试片)—— 编译进来 ≠ 本机
  能跑(amf 缺 DLL / 无 MFX 会话时不再误选)。渲染段首选编码失败沿候选
  表回退(消费级 N 卡并发编码会话数有限,多段并行时个别段自动落
  x264)。
- **静态服务绑定 8860–8960 端口段**:避开 Chrome 的 ERR_UNSAFE_PORT
  黑名单(bind(0) 撞上时页面静默打不开)。
- 实测(GT 710 + 剪映裁剪版 ffmpeg,该机最弱配置):1080p60 采样片
  300 帧串行 14.4 帧/s → 3 实例并行 25.5 帧/s;1200 帧长片稳态
  **33.8 帧/s**(同口径旧实现实测 6 帧/s)。用户 9070 GRE 机器上
  GPU 光栅 + AMF 硬编 + 更多 worker 预计再有量级提升。
- kiln-cli 新旗标:`--workers` `--gpu` `--img png|jpeg` `--jpeg-quality`
  `--encoder auto|x264|nvenc|amf|qsv` `--wall` `--seek-fn`;导出结果带
  逐条 `{"domwarn":…}`(编码器候选链 / 帧驱动方式 / 并行拓扑 / 流水线
  帧率),GIF 保持原路径(调色板需全帧统计)。

## 0.10.1(2026-09-30)

本轮为「一条龙体检」批次:三路并行深度审查(核心文档层 / 导出与 Agent 层 /
UI 与应用层)后逐项核实修复,重点打掉 Windows 发布阻断项与 L0/L1 往返承诺
的破口。

### Fixed(核心 / 往返幂等)

- **多行 Raw 片段每存一轮吸收一层缩进(打穿 L1 字节幂等)**:frozen 块 /
  `trailing_raw` / `head_extra` 的内部行在 canonical 序列化时逐行垫缩进,
  再导入时缩进进入 verbatim 内容,保存-重开循环里无限增长。现只给首行垫
  (首行 pad 落在开标签之前,不进入下次捕获),内部行逐字节原样写出。
- **`<pre>`/`<textarea>` 实体回写不转义(L0/L1 双破)**:`&lt;` 解码成 `<`
  后原样写回,二次解析变成真元素。现对该两类元素重新转义
  (`&amp;`→`&lt;`→`&gt;`),对已转义源字节稳定,对真 `<`/`&` 修复破坏。
- **NBSP 邻接处多插可见空格(L0 漂移)**:行内序列化的首尾空白探测与折叠
  口径不一致(Unicode vs ASCII),`&nbsp;` 相邻处误判"源有空白"。现统一
  `is_foldable_ws` 口径。
- **富文本段 lead 修剪错位**:`finalize_group` 的 text 用 ASCII 修剪而
  lead 用 Unicode 修剪,NBSP 起始的行内组段区间整体错位。
- **段注记区间缺 UTF-8 字符边界校验(导出 panic)**:Agent `set_segs` 对
  中文文本给非边界字节偏移,导出侧 `text[pos..s]` 字节切片直接 panic。
  现 `validate_segs` 在 apply/redo 双侧拒绝非边界区间。
- **画板可被移入非 root 容器后静默消失**:Move/Insert 只拦「非画板挂
  root」,不拦「画板挂非 root」,后者被 `sync_artboards` 除名后整棵子树
  从导出中消失且无告警。现补对称不变量(画板只能挂 root)。
- **带 BOM 的 CSS 首条规则静默丢失**:Windows 记事本出品的 `styles/main.css`
  带 UTF-8 BOM,`:root` 令牌与首个类选择器匹配失败整表降级。导入现剥
  BOM(html5ever 的 HTML 侧本就安全)。
- 命令面板「没有匹配的命令」用 `Color32::GRAY` 硬编码,归 `text_3` 令牌。

### Fixed(Kiln / 导出 / 浏览器车道)

- **`kiln-cli img blur --box` 区域语义反转**:box 外本应保持原图,实际
  whole 图全被模糊(combined 从模糊图克隆再写回模糊像素)。现从原图出发,
  仅 box 内写回模糊像素。
- **`kiln-cli` 诊断行 JSON 不转义**:`{"domwarn":"…"}` / `{"warn":"…"}` /
  `{"ok":false,"error":"…"}` 由 `format!` 手拼,warning 含 `"` 或 `\`
  (Windows 资产路径必然带 `\`)时产出非法 JSON,下游逐行 `json.loads`
  必炸。全部过 `jesc()`(主结果此前已修,告警/错误行漏网)。
- **静态服务 Drop 后端口泄漏**:accept 线程阻塞在 `incoming()`,Drop 只置
  `alive=0` 不够;批量导出时端口持续累积。现 Drop 发哑连接唤醒 accept,
  循环头见到停止位即退出。
- **浏览器启动失败泄漏临时目录**:`BrowserProcess` 未构造时 Drop 不执行,
  user-data-dir 遗留。现失败路径手动清理。
- **目录无 index.html 时两车道选不同入口**:domexport 取字母序最后一个、
  staticsrv 取第一个 → PNG 与 PDF/AI 可能渲染不同 HTML。统一为「index.html
  → index.htm → 字母序第一个」。
- **CDP 调试截断按字节切中文 panic**(`KILN_CDP_DEBUG` 门控):回退到字符
  边界再截断。
- **WebSocket connect 无超时 + 帧长无上限**:SYN 重试可挂数秒;对端声明
  64 位巨帧会无上限 `reserve`。connect 带与握手同源 deadline,帧长设
  256 MiB 上限。
- **SVG/PPTX 导出丢字体**:`<text>` 无 `font-family`,PPTX run 无
  `latin/ea/cs typeface`,设计字体全丢成查看器默认(度量按原字体整形,
  换默认字体行宽错位)。现按 run 落盘字体族。
- **PDF 导入 20 页截断静默**(ADR-0046 纪律):超出页数现在 warnings 里
  留痕。
- **SVG 导入退化几何 NaN 排序 panic**:`partial_cmp().unwrap()` 改
  `total_cmp`,含 NaN 直接放弃近似。
- **浏览器车道告警被吞**:kiln-cli 车道 B 分支只报 `warnings` 计数,
  告警内容不落 stderr,违反「降级必须可观测」(ADR-0046)。现逐条
  `{"domwarn":…}` 留痕。
- **favicon.ico 恒定 404 噪声**:favicon 是浏览器自发请求而非文档引用
  的资源,不计入 AssetNotFound 告警(文档引用的资源照旧留痕)。

### Fixed(UI / 应用层)

- **随包字体编译期绝对路径(Windows 发布阻断)**:`CARGO_MANIFEST_DIR`
  在用户机器上不存在,Inter/JetBrains Mono/MiSans 全体静默失效。现运行期
  多候选探测:`VB_FONTS_DIR` → `<exe>/assets/fonts` → `<exe>/../assets/fonts`
  → 开发布局;并新增 `%WINDIR%` 解析的系统 CJK 回退(此前硬编码
  `C:\Windows\Fonts`,装非 C 盘的机器中文变占位块),候选补等线/黑体。
- **画布纹理按逻辑点渲染(高分屏发糊)**:150%/200% 缩放下纹理只有物理
  像素的 1/1.5~1/2。现纹理按物理像素创建,vello 场景变换外乘
  `pixels_per_point`,画布内容与 UI 同等锐度。
- **外部改动热重载无去抖**:注释声称 200ms 窗口,实际事件到达即整页重载,
  编辑器连写多文件触发双次。现 200ms 静默窗收敛后统一处理。
- **自动保存快照落盘无路径消毒**:恢复中转对快照 `files` 表的相对路径
  直接 join,`..`/绝对路径可写出项目目录之外。现拒绝越界路径。
- **LayerRow 双击眼睛/锁误触重命名**:双击开关同时命中行级
  `double_clicked` 语义。现一并排除。
- **NumField 键盘步进覆盖输入草稿**:聚焦输入中按 ↑/↓,已键入未回车的
  表达式被格式化回显直接覆盖。现步进前先吸收草稿(步进基于草稿求值)。
- **最近项目全部固定时 LRU 删固定项**:`unwrap_or(len-1)` 兜底违反
  「固定项不淘汰」承诺。现全固定时容忍暂时超限。
- **启动主页缩略图缓存永不失效**:缓存键加 mtime,外部替换 `thumb.png`
  即时刷新。
- **导出对话框「透明背景」静默隐藏**:改置灰 + 悬停说明(仅 PNG 支持)。
- **画布底色硬编码 hex 与主题令牌漂移**:归 `bg_canvas` 唯一令牌源,
  与隔离遮罩同源。
- **插件面板状态/日志色硬编码 RGB**:归 `danger/success/warn/text_3` 令牌,
  深浅主题各自正确。
- **新建项目名未处理 Windows 保留设备名**:CON/PRN/AUX/NUL/COM1-9/LPT1-9
  与结尾 `.`/空格会让 `create_dir_all` 以难懂的 OS 错误失败。清洗层现
  加 `_` 前缀兜住(带单测)。

### Fixed(CLI / Agent)

- **`vellum-cli --help` 退出码 1**:求助与出错无法用 `$?` 区分。`--help`/
  `--version` 现退出 0,参数错误仍按退出码表。
- **WPI 桥裸跑 `python` 命中微软商店 stub**:优先 `py -3`(以即退的
  `--version` 探测,绝不裸跑 `py -3` 防 REPL 挂死),失败回退 `python`。
- **WPI 临时目录毫秒级碰撞**:`subsec_millis` 每秒重复,同进程并发导出可
  互删临时目录。改 `subsec_nanos`。
- **批量 CSV 编码错误信息不可读**:GBK/ANSI(中文 Excel 默认)报裸 UTF-8
  错误。现附「另存为 UTF-8」提示。

### Changed

- `kiln-cli --max-wait` help 文本改为如实声明「保留参数,当前不生效」
  (此前声称浏览器车道 settle 预算,误导)。

### Added

- `dist/package.ps1`:Windows 便携发行包脚本(GUI + 双 CLI + 示例 +
  调用封装 + 随包字体说明目录),与 `vb_ui::fonts` 的运行期查找布局对应。
- 浏览器发现补 `%LOCALAPPDATA%` 的 per-user Chrome 与
  `%PROGRAMFILES%`/`%ProgramFiles(x86)%` 环境变量派生路径。

## 0.10.0(2026-09-26)

### Fixed

- **VB-1(P0)· 静态服务越界判定与目录联接不兼容**(ADR-0045):本地静态
  服务的 404 判定原用 `canonicalize()`,会把项目内声明的目录联接
  (`src/fonts` → 技能字体库等减重影子)解析到 root 之外,合法字体/脚本
  全部 404 且浏览器静默回退系统字体。现改为词法规范化主判定(折叠 `./..`,
  拒绝越出 root)+ 对词法越出的请求再查 canonicalize 是否落在允许根集合;
  `../` 逃逸、绝对路径与反斜杠注入仍被拒绝(安全回归锁定)。联接下
  woff2/js 恢复 200。
- **VB-2(P0)· native/矢量道静默丢弃不支持原语**(ADR-0046):自研引擎与
  矢量写出器对 3D/透视 transform、mask、box-shadow、mix-blend-mode、不可
  解析的 clip-path 形状、内联 SVG 子树此前丢弃/栅格化后零告警。现于
  构建期扫描、采集计数与写出器检出三处收集,同属性同车道聚合计数,命中置
  `degraded:true`;SVG/EPS/Ai/PPTX 写出器不表达 clip-path 也在交付前检出。

### Added

- 新告警变体:`AssetNotFound{src}`(静态资源 404 必然留痕,不再静默)、
  `UnsupportedPropertyDropped{prop,count,lane}`、
  `InlineSvgRasterized{count,format}`、`ClipShapeApproximated{shape,count}`、
  `ImportObjectSkipped{kind,count}`(均含稳定 `kind()` 键与
  `is_degrading()` 语义)。
- `KilnReport.warnings_by_kind` / `count_of(kind)`:按稳定小写蛇形键聚合
  告警计数,导出/导入结果 JSON 同步输出 `warnings_by_kind`,下游可直接写
  门禁(如 `unsupported_dropped > 0` 拒绝交付矢量稿);kiln-cli 的 dom /
  anim / native / import 四路结果 JSON 均补齐 `degraded` 判定。
- `anim_coverage` 动画覆盖矩阵(VB-3):`anim.rs` 新增 `AnimCoverage`,
  按车道(browser 全量 / native 四类轨道)把本源关键帧属性分为
  animated / static_fallback / unsupported,随 GIF/MP4 导出结果输出。
- SVG 导入跳过清单强类型化(VB-4):`import_svg` 的跳过/近似类别映射为
  `ImportObjectSkipped` 进 KilnReport,倾斜变换计入清单;kiln-cli import
  结果与 export 同构(`warnings[]` + `degraded` + `warnings_by_kind`)。
- ADR-0045(静态服务越界判定与联接语义)、ADR-0046(降级可观测性契约);
  `crates/vb_kiln/docs/DESIGN.md` 增「降级必须可观测」一节。
