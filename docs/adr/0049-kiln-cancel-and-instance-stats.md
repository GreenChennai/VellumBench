# ADR-0049: Kiln 取消语义三态 × 静态快照车道收口 × 跨实例串行点终判(硬骨头 #3/#6/#18)

- 状态:**已裁定并落地**(R0,2026-10-04;本机实测数据在案,复现命令逐条可跑)
- 落点:22 篇 §4 R0 交付 5/6;§5 #3/#6/#18
- 前序:ADR-0048(换宿主裁定/R0 口径);`vb_kiln::cancel`(R0 已落的动画三车道取消)

## 1. 取消语义三态(全车道统一,定案)

Kiln 导出的三态 = **成功(exit 0)/ 失败(非 0,结构化 `{"ok":false,...}`)/ 取消(exit 130,`{"ok":false,"cancelled":true,...}`)**。取消语义四条铁律:

1. **协作式**:取消不打断阻塞调用,只在预设**边界检查点**生效。检查点粒度 = 取消延迟上界;R0 收口后静态快照车道的粒度达到「≤100ms 泵片 + 当前 CDP 调用收尾」(<1s,满足硬骨头 #3 完成定义)。
2. **三态可判**:native 车道返回强类型 `KilnError::Cancelled`;浏览器系车道(String 错误通道)统一以 `vb_browser::cancel::WAIT_CANCELLED_PREFIX`(="导出已取消")前缀标记。该常量是**编译期单源**——`vb_kiln::cancel::LANE_CANCELLED_PREFIX` 就是它的别名,两条车道的取消错误由同一个 `is_lane_cancelled` 判定,字面量不可能漂移。
3. **不留半截产物**:最终产物只经 `write_atomic` 原子落盘;取消路径清扫临时/分段文件(anim 工作目录、静态车道根本不写输出)。
4. **进程不泄漏**:取消路径上 ffmpeg 显式 kill+wait;浏览器实例随 `BrowserProcess::Drop`(taskkill 整树 + user-data 清理)收割。

## 2. 静态快照车道收口(硬骨头 #3 的 Kiln 侧收口,R0)

R0 前:动画三车道(anim pipe / GIF 内存 / WebCodecs)已接 `CancelToken`,但**单页 PNG/JPG/PDF/AI 截图导出**的浏览器等待仍是 60s/180s/300s 一把堵死——对话框关了导出停不下来。

R0 收口(本 ADR 落地):

- `vb_browser::Cdp::call_cancellable`:60s(beginFrame)/180s(captureScreenshot)/300s(printToPDF)长等待拆成 **≤100ms 泵片**,片间查取消探针。取消瞬点后迟到的 CDP 响应留在 responses 表(新 call 用新 id 永不误配,随连接关闭释放)。
- `PageSession::set_cancel_probe`:navigate/load 等待、Runtime.evaluate、四类截图/打印调用全部走分片通道;**未设探针时行为与旧版逐字节一致**(动画车道性能零影响)。
- 入口:`vb_browser::export_source_cancellable` / `vb_kiln::domexport::export_dom_with_cancel` / `export_dom_pages_with_cancel`。**预取消在静态服务/浏览器拉起前收口**;写出段走 `export_artboard_with_cancel`。
- CLI 同通道:kiln-cli 的 stdin `'c'`/`'q'` 监听线程持有 child 令牌,静态车道探针共享同一旗标——`{ sleep 1.5; echo c; } | kiln-cli export --source … --output out.png` 实测:stderr 输出 `{"ok":false,"cancelled":true,"error":"导出已取消"}`,退出码 **130**,输出文件不存在,`tasklist` 无 chrome 残留(2026-10-04 本机实测,Chrome/154.0.8037.95)。
- 测试:`cancel_lanes.rs` 增 4 条确定性集成测试(单页/多页 DOM 入口、静态车道起跑边界、CLI child→probe 传播);真浏览器逐帧取消属手动/下游范畴(与既有口径一致)。

## 3. #18 跨实例串行点终判:本机实测

### 3.1 环境与负载

- 硬件:i5-12400F(6C/12T)/ 16GB RAM / GT 710 2GB(ffmpeg 9 无 Kepler NVENC → 编码器链 auto 实际落 **libx264 软编**);OS Windows 10 19045。
- 浏览器:Chrome/154.0.8037.95 headless;`HeadlessExperimental.beginFrame` 不可用(实测 `-32601 wasn't found`),自动退回 `Page.captureScreenshot` 逐帧。
- 负载:`bench/anim-fixture/index.html`(随本 ADR 入库)——1280×720 canvas,240 个确定性运动元素,**`window.SEEK(t)` 纯函数时间轴**(kiln 确定性寻址约定),帧内容只依赖 t,分段并行结果与串行逐帧一致。
- 参数:`--fps 30 --duration 20`(= 600 帧)`--width 1280 --height 720 --render screenshot`(强制截图车道:WebCodecs 页内硬编无跨实例语义)`--img jpeg`(默认 q95);编码器默认 auto(实测落 x264)。

### 3.2 命令(逐条原样,6 次运行)

```
kiln-cli export --source D:/Temp/kiln18/index.html --output D:/Temp/kiln18/runs/w{W}_r{R}.mp4 \
  --format mp4 --fps 30 --duration 20 --workers {W} --width 1280 --height 720 --render screenshot
# {W} ∈ {1,2,4},每档 2 轮({R} ∈ {1,2});stdin 无输入(不取消)
```

### 3.3 数据表(instances JSON 原文摘录,节选自各次运行 stdout)

| 档·轮 | 墙钟(含起跑) | 聚合渲染吞吐¹ | 逐实例 fps | 逐实例 p50 | 逐实例 p95 | p99 | max | 直方图主体² |
|---|---|---|---|---|---|---|---|---|
| w1·r1 | 29.5s | 22.1 fps | 22.13 | 46.7 | 49.3 | 53.5 | 90.1 | 34-66 **91.2%** |
| w1·r2 | 29.4s | 22.0 fps | 21.96 | 46.7 | 50.0 | 59.9 | 87.9 | 34-66 **92.2%** |
| w2·r1 | 21.3s | 34.6 fps | 17.47 / 17.13 | 61.9 / 61.7 | 65.4 / 66.1 | 107.9 / 101.2 | 144.4 | 34-66 **96.0%** |
| w2·r2 | 20.6s | 35.5 fps | 17.67 / 17.82 | 60.9 / 59.3 | 65.9 / 65.2 | 84.6 / 82.9 | 129.0 | 34-66 **96.2%** |
| w4·r1 | 28.1s | 31.2 fps | 8.78/7.63/7.26/7.54 | 104–127 | 235–266 | 299–389 | 922.8 | 67-133+>133 **85.7%** |
| w4·r2 | 30.1s | 28.5 fps | 7.96/6.76/6.85/6.95 | 117–129 | 264–313 | 380–439 | 620.0 | 67-133+>133 **85.5%** |

> ¹ 聚合渲染吞吐 = Σ 逐实例 fps(`instances[].fps` 之和;不含浏览器起跑与 concat 尾巴)。
> ² 占 600 间隔样本的百分比;桶界 = 60/30/15/7.5fps 标称帧间隔(见 `report.rs::IntervalHistogram`)。

原始 JSON 形样(w1·r1 单实例、w4·r1 四实例,键序 = report.rs 契约):

```json
{"index":0,"frames":600,"wall_seconds":27.11,"fps":22.13,
 "frame_interval_ms":{"p50":46.7,"p95":49.3,"p99":53.5,"max":90.1,
 "histogram":{"<=16":0,"17-33":51,"34-66":547,"67-133":1,">133":0}}}

{"index":0,"frames":150,"wall_seconds":17.07,"fps":8.78,
 "frame_interval_ms":{"p50":104.4,"p95":239.7,"p99":298.6,"max":379.3,
 "histogram":{"<=16":0,"17-33":1,"34-66":45,"67-133":56,">133":47}}}
```

### 3.4 终判:**CPU 真饱和,非 GPU 读回串行**

按 `report.rs::FrameIntervalStats` 的判读口径逐条对:

1. **"p95 集中在固定倍数间隔且不随实例数变化 → GPU 读回串行"——不成立**:若读回在驱动级串行,逐实例 p95 应钉在标称帧档(16.7/33.3/66.7ms)附近且随实例数不变。实测 p95 从 49.3(w1)→ 65.4–66.1(w2)→ 235–313ms(w4),单调大幅右移,峰值 max 到 922.8ms——与固定倍数假设完全不符。
2. **"p95 与直方图主体随实例数右移 → CPU 真饱和"——成立**:直方图主体从 `34-66`(91–92%)整体迁移到 `67-133 + >133`(85–86%),且四个实例的分布**同步**恶化(w4 四实例 p50 彼此只差 ~23ms,无"一个赢家多个饿死"的 GPU 争抢形态)——典型的 CPU 时间片被 4×(浏览器渲染 + JPEG 编码 + x264 编码)线程群摊薄的特征。
3. **旁证——吞吐塌缩**:聚合渲染吞吐 w1→w2 是 +57%(22.0→35.5),w2→w4 反而 −17%(35.5→28.5);w4 墙钟(28.1/30.1s)与 w1(29.5s)持平。6C/12T 上 4 个浏览器实例 + 4 路 x264 已把核占满,上下文切换与内存带宽把并行收益吃光——这就是 `--workers` 帮助里"两轮下游实测多实例都是负收益"的本机定量版。
4. **本机 GPU 语境**:GT 710(Kepler)不在 Chrome 154 的硬件光栅支持面内,实测帧率天花板(单实例 p50 46.7ms ≈ 21.4fps)由 CPU 侧软件光栅 + 截图回传 + JPEG 编码决定;beginFrame 通道在本机 Chrome 亦不可用。即便换强 GPU,**只要逐帧成本由 CPU 侧主导,w1 默认档依旧正确**。

### 3.5 workers 默认值:**维持 1 不变;w2 为显式 opt-in 的甜点档**

- **默认 1 维持**:w4 明确负收益(吞吐低于 w2、墙钟与 w1 持平、内存 4×、max 尖刺逼近 Page.navigate 超时红线);w2 虽有 +40% 墙钟收益,但负载画像(轻量 canvas 页)与真实海报页不同,auto 提档会在弱机上重演"w2 起跑雪崩"——保持确定性安全档,并发由用户显式 `--workers 2`。
- **对下游的诚实建议**:12 线程级 CPU + 轻量确定性页可试 `--workers 2`(本机两轮稳定 +40% 墙钟);≥4 一律不建议。
- 后续若把截图回传/编码下沉 GPU(ADR-0047 路线 (c) 的画布翻译层反向复用),本表应重测——判读方法(report.rs 三条)不变。

## 4. 后果

- 正面:硬骨头 #3 完成定义全满足(任意阶段取消 <1s 生效、产物不留半截、CLI 同通道);#18 有数据终判,`--workers` 默认值从"经验保守"升级为"实测裁决"。
- 代价:静态车道多一条探针分支(未设探针零开销);取消后单条迟到 CDP 响应滞留 responses 表(有界,随连接释放)。
- 未竟:Edge 的 beginFrame 缺失与 GT 710 不受 Chrome 光栅支持均为环境事实,如实记录不修;`kiln-cli selfcheck` 的编码器标签把 x264 标成"硬件编码可用"(文案瑕疵,不影响数据,遗留下一批文案门禁)。

## 5. 复现清单

1. `cargo build -p vb_kiln`(ffmpeg 不在 PATH 时把 `D:/MomentShift` 之类加进 PATH,或装 ffmpeg;缺失则 MP4 降级 GIF 流、无跨实例语义)。
2. 基准页:`bench/anim-fixture/index.html`(入库);拷到任意目录后用**绝对路径**作 `--source`(相对路径会触发"源文件缺少父目录"的浏览器车道降级)。
3. 逐条跑 §3.2 命令,`instances` 数组即判读数据;直方图判读口径见 `crates/vb_kiln/src/report.rs` 注释。
4. 静态车道取消复现:`{ sleep 1.5; echo c; sleep 2; } | kiln-cli export --source <绝对路径>/index.html --output out.png --width 1280 --height 720`,期望 exit 130 + `cancelled:true` + 无 out.png + 无 chrome 残留。
