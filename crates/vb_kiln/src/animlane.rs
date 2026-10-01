//! 车道 B 动画逐帧(MP4 流水线版;WPI 理论 + 确定性渲染方法论)。
//!
//! ## 两条采样路线
//!
//! - **确定性寻址(默认)**:加载后 `document.getAnimations()` 全部
//!   `pause()`,逐帧设 `currentTime = t*1000` —— 任意时刻画面是时间的
//!   纯函数(SEEK(t)),帧与帧零状态依赖。收益:① 输出帧严格落在
//!   1/fps 网格上,无墙钟重采样的跳帧/复制帧;② 时间段可独立渲染,
//!   分段并行成为可能。仅覆盖 CSS/WAAPI 动画(本产品动画模型即
//!   CSS @keyframes,ADR-0043);JS rAF 驱动的动画不适用,可用
//!   `--wall` 回退。
//! - **墙钟实时采样(`--wall`)**:旧行为,浏览器实时播放、按节奏截屏、
//!   墙钟重采样。截屏慢于帧间隔时输出时间与真实播放一致,但帧序不
//!   落网格,且单帧耗时直接拖慢全程。
//!
//! ## MP4 流水线(内存 O(1))
//!
//! 旧实现把全部帧解成 RGBA 驻留内存(1080p 一帧 8.3MB,3 分钟 60fps
//! ≈ 90GB,靠页面交换硬扛)再整体落盘 PNG。现改为:CDP 截图字节
//! (PNG 无损 / JPEG 直出)**不经解码**直接写 ffmpeg stdin
//! (`-f image2pipe`),ffmpeg 边收边编,每段产出独立 mp4,最后 concat
//! 无缝拼接(`-c copy`,零重编码)。
//!
//! ## 分段并行
//!
//! 确定性寻址下任意时间段独立渲染 → 总帧数均分 W 段,W 个 headless
//! 实例各自渲染编码自己的段。W 默认自动(CPU/2,上限 4;每实例
//! 常驻 1–3GB,内存是第一瓶颈),`--workers` 显式覆盖。
//!
//! ## GPU 光栅化
//!
//! `--gpu` / `VB_GPU=1` 时浏览器走 ANGLE→D3D11(NVIDIA/AMD/Intel 通用),
//! 光栅与合成落在显卡;默认软件光栅保持跨机逐像素可复现(ADR-0022),
//! GPU 路径与软件路径存在固定 AA 微差(MAD ≈ 1/255,肉眼无别,同路径
//! 自身可复现)。

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::context::{ExportContext, Frame};
use crate::writer::Format;
use vb_render::encode::DrawList;

pub struct AnimLaneResult {
    pub bytes: Vec<u8>,
    pub frames: usize,
    pub browser: String,
    /// 实际用于段编码的 H.264 编码器(nvenc/amf/qsv/libx264)。
    pub encoder_used: Option<String>,
    pub warnings: Vec<String>,
    /// 动画覆盖矩阵(VB-3):浏览器车道全量播放,`animated` = 源中声明的
    /// 全部关键帧属性;无动画声明时为 None。
    pub anim_coverage: Option<crate::anim::AnimCoverage>,
}

/// 动画导出选项(MP4 流水线;GIF 走内存路径忽略其中大部分)。
#[derive(Debug, Clone)]
pub struct AnimPipeOpts {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_s: f32,
    pub scale: u32,
    pub bitrate_kbps: u32,
    /// 并行分段数;0 = 自动(CPU/2,上限 4)。1 = 串行。
    pub workers: u32,
    /// GPU 光栅化(`--gpu` / `VB_GPU=1`)。
    pub gpu: bool,
    /// 中间帧格式:None = PNG(CDP 直出,无损管道);
    /// Some(q) = JPEG(q 为质量 1-100,预览提速档,4:2:0 色度损失)。
    pub jpeg_quality: Option<u8>,
    /// H.264 编码器选择。
    pub encoder: EncChoice,
    /// true = 墙钟实时采样(旧行为);false = 确定性寻址(默认)。
    pub wall_clock: bool,
    /// JS 驱动函数名(如 "SEEK"):页面自带确定性时间轴时的逐帧入口
    /// (`SEEK(t)` 把整片画成一帧,是确定性渲染工作流的约定接口)。
    /// None = 自动探测 window.SEEK / window.seek;显式指定优先。
    pub seek_fn: Option<String>,
}

impl Default for AnimPipeOpts {
    fn default() -> Self {
        AnimPipeOpts {
            width: 0,
            height: 0,
            fps: 25,
            duration_s: 2.0,
            scale: 1,
            bitrate_kbps: 8000,
            workers: 0,
            jpeg_quality: None,
            encoder: EncChoice::Auto,
            // 动画车道默认 GPU 光栅:逐帧导出没有跨机逐像素复现的诉求,
            // 而"CPU 99% / GPU 7%"的实测(下游 9070 GRE)说明软件光栅把
            // 渲染器全压在 CPU 上。AA 微差 MAD ≈ 1/255(与 Playwright
            // 实测一致);要旧口径用 --no-gpu。
            gpu: true,
            wall_clock: false,
            seek_fn: None,
        }
    }
}

/// 每帧画面驱动方式(加载后探测一次,决定帧循环怎么走)。
#[derive(Debug, Clone, PartialEq)]
enum Driver {
    /// 页面自带 JS 确定性时间轴:逐帧调 `fn(t)`(秒)。
    JsSeek(String),
    /// CSS/WAAPI 动画:pause 后逐帧 currentTime 定位(计数仅供参考)。
    CssAnims(u32),
    /// 页面无任何可寻址内容:回退墙钟节奏(输出可能为静止画面)。
    WallFallback,
}

/// 探测每帧驱动方式:显式 seek_fn > window.SEEK/seek > CSS 动画 > 墙钟。
fn detect_driver(page: &mut vb_browser::page::PageSession, seek_fn: &Option<String>) -> Driver {
    const QUOTE: char = '\'';
    // ① 显式指定 / ② 约定名探测:typeof 判定,名字命中即用
    let probe_js = format!(
        "(() => {{ const names = [{}];          for (const n of names) {{ if (typeof window[n] === 'function') return 'JS:' + n; }}          return String(document.getAnimations({{subtree:true}}).length); }})()",
        match seek_fn {
            Some(f) => format!("'{}'", f.replace(QUOTE, "")),
            None => "'SEEK','__SEEK__','seek','VB_SEEK'".to_string(),
        }
    );
    match page.evaluate(&probe_js, false) {
        Ok(v) => match v.as_str() {
            Some(s) if s.starts_with("JS:") => Driver::JsSeek(s[3..].to_string()),
            Some(n) => match n.parse::<u32>() {
                Ok(0) => Driver::WallFallback,
                Ok(count) => Driver::CssAnims(count),
                Err(_) => Driver::WallFallback,
            },
            _ => Driver::WallFallback,
        },
        Err(_) => Driver::WallFallback,
    }
}

/// 逐帧驱动:JS SEEK 同步绘制;CSS 动画 currentTime 定位;墙钟不动作。
/// seek 后等一次 repaint(双 rAF,250ms 兜底)再返回 —— SEEK 只改状态
/// 而绘制发生在下一渲染帧的页面(异步字体/位图/rAF 绘制)也能截到
/// 正确画面;同步绘制的页面只多花一次 rAF 往返。
fn drive_frame(page: &mut vb_browser::page::PageSession, driver: &Driver, frame: usize, fps: u32) {
    let timing = std::env::var("VB_ANIM_TIMING").is_ok();
    let t0 = std::time::Instant::now();
    let t_ms = (frame as f64 * 1_000.0 / fps as f64).round() as u64;
    match driver {
        Driver::JsSeek(name) => {
            // 与 Playwright 侧同一约定:`t => window.SEEK(t)`(SEEK 是纯函数)
            let js = format!("({})({})", name, t_ms as f64 / 1000.0);
            let _ = page.evaluate(&js, false);
        }
        Driver::CssAnims(_) => {
            let js = format!(
                "(() => {{ const T = {t_ms}; const anims = window.__vbAnims || [];                  for (const an of anims) {{ try {{ an.currentTime = T; }} catch (e) {{}} }}                  return anims.length; }})()"
            );
            let _ = page.evaluate(&js, false);
        }
        Driver::WallFallback => return,
    }
    // 等 repaint(仅 captureScreenshot 路径需要:确保状态已提交到一帧;
    // beginFrame 路径由 CDP 显式产帧,本身就是"等一次 repaint",
    // 再跑 rAF 是纯开销)。页面无 rAF 流时 setTimeout 兜底不挂死。
    if std::env::var("VB_NO_BEGINFRAME").is_ok() {
        let wait_js = "() => new Promise(res => { let n = 0;                     const tick = () => { if (++n >= 2) return res(true); requestAnimationFrame(tick); };                     requestAnimationFrame(tick);                     setTimeout(() => res(false), 250); })";
        let _ = page.evaluate(wait_js, true);
    }
    if timing {
        eprintln!("[timing] frame {frame}: drive+repaint {:?}", t0.elapsed());
    }
}

/// CSS 动画路线的一次性准备:全部 pause + 缓存清单;无动画返回 0。
fn pause_all_animations(page: &mut vb_browser::page::PageSession) -> i64 {
    let pause_js = "(() => { const anims = document.getAnimations({subtree:true});         anims.forEach(a => { try { a.pause(); } catch (e) {} });         window.__vbAnims = anims; return anims.length; })()";
    page.evaluate(pause_js, false)
        .ok()
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

/// H.264 编码器:Auto 依次探测 nvenc(N 卡)/ amf(A 卡)/ qsv(Intel),
/// 都缺则 libx264(CPU)。硬件编码把 x264 的编码耗时一并卸到显卡。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncChoice {
    Auto,
    X264,
    Nvenc,
    Amf,
    Qsv,
}

impl EncChoice {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "auto" => EncChoice::Auto,
            "x264" | "cpu" | "libx264" => EncChoice::X264,
            "nvenc" | "nvidia" => EncChoice::Nvenc,
            "amf" | "amd" => EncChoice::Amf,
            "qsv" | "intel" => EncChoice::Qsv,
            _ => return None,
        })
    }

    pub fn codec_name(self) -> &'static str {
        match self {
            EncChoice::X264 => "libx264",
            EncChoice::Nvenc => "h264_nvenc",
            EncChoice::Amf => "h264_amf",
            EncChoice::Qsv => "h264_qsv",
            EncChoice::Auto => "libx264",
        }
    }

    /// Auto → 运行时探针出**全部可用**编码器,按 nvenc→amf→qsv→x264 排成
    /// 候选表(编译进来 ≠ 有硬件;探针 = 编 2 帧测试片)。渲染段首选失败
    /// 时沿表回退 —— 消费级 N 卡有并发编码会话数上限(老卡 1–3 个),
    /// 多分段并行时硬件编码器可能个别段开不出会话,须能落到 x264。
    /// 显式选择也探针,不可用即 Err —— 宁可失败得清楚,不许半路炸掉
    /// 已跑了几分钟的渲染。
    fn resolve(self) -> Result<Vec<Self>, String> {
        let caps = ffmpeg_caps();
        let usable = |c: EncChoice| -> bool {
            let compiled = match c {
                EncChoice::Nvenc => caps.h264_nvenc,
                EncChoice::Amf => caps.h264_amf,
                EncChoice::Qsv => caps.h264_qsv,
                EncChoice::X264 => caps.libx264,
                EncChoice::Auto => false,
            };
            compiled && probe_encoder(c.codec_name())
        };
        match self {
            EncChoice::Auto => {
                let all = [
                    EncChoice::Nvenc,
                    EncChoice::Amf,
                    EncChoice::Qsv,
                    EncChoice::X264,
                ];
                let cands: Vec<EncChoice> = all.into_iter().filter(|c| usable(*c)).collect();
                if cands.is_empty() {
                    return Err("无可用的 H.264 编码器(ffmpeg 缺 libx264 且硬件编码器均不可用);请安装完整版 ffmpeg 或用 --encoder 显式指定".into());
                }
                Ok(cands)
            }
            c => {
                if usable(c) {
                    Ok(vec![c])
                } else {
                    Err(format!(
                        "编码器 {} 在本机不可用(ffmpeg 未编译或硬件缺失);--encoder auto 可自动回退",
                        c.codec_name()
                    ))
                }
            }
        }
    }
}

/// 编码器**运行时可用**探针:编 2 帧测试片实测(lavfi 黑帧 → null muxer)。
fn probe_encoder(codec: &str) -> bool {
    Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=64x64:r=25:d=0.08",
            "-frames:v",
            "2",
            "-c:v",
            codec,
            "-f",
            "null",
            "-",
        ])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 可用编码器候选链(编译探测 + 运行时探针;Auto 语义)。供 frames.rs 的
/// native MP4 路径复用 —— 此前该路径硬编码 libx264,裁剪版 ffmpeg(剪映等
/// 常见发行)上 MP4 直接报废。
pub fn usable_encoder_chain() -> Vec<EncChoice> {
    EncChoice::Auto.resolve().unwrap_or_default()
}

/// 输出色彩口径:NLE/拼接安全(下游实测反馈 #3)。
/// 源是 RGB(全程 0-255),swscale 显式压到 limited(MPEG)色程并打全
/// bt709 标记 —— 此前输出 yuvj420p + color_range=pc + transfer/primaries
/// unknown,与电视范围素材(典型 YMIN/YMAX≈16/235)拼接或进 NLE 会
/// 电平不匹配。
pub(crate) fn push_color_args(cmd: &mut Command) {
    cmd.arg("-vf")
        .arg("scale=in_range=full:out_range=mpeg:out_color_matrix=bt709")
        .arg("-color_range")
        .arg("tv")
        .arg("-colorspace")
        .arg("bt709")
        .arg("-color_primaries")
        .arg("bt709")
        .arg("-color_trc")
        .arg("bt709");
    // VUI 兜底:硬件编码器(nvenc/amf/qsv)不一定回填 transfer/primaries
    // (下游实测 color_space=bt709 但 transfer/primaries=unknown)。bsf 直接
    // 改写 H.264 SPS 的 VUI,与编码器无关。1 = BT.709(数值见 ITU-T H.264 表 E-1)
    cmd.arg("-bsf:v")
        .arg("h264_metadata=colour_primaries=1:transfer_characteristics=1:matrix_coefficients=1");
}

/// 给 ffmpeg 命令挂编码器参数(与 render_segment 同一份口径)。
pub(crate) fn apply_encoder_args(cmd: &mut Command, enc: EncChoice, workers: usize) {
    match enc {
        EncChoice::X264 => {
            let cores = std::thread::available_parallelism()
                .map(|c| c.get())
                .unwrap_or(4);
            cmd.arg("-preset")
                .arg("medium")
                .arg("-threads")
                .arg((cores / workers.max(1)).clamp(1, 16).to_string());
        }
        EncChoice::Nvenc => {
            cmd.arg("-preset").arg("p4");
        }
        EncChoice::Amf => {
            cmd.arg("-quality").arg("balanced");
        }
        EncChoice::Qsv => {
            cmd.arg("-preset").arg("medium");
        }
        EncChoice::Auto => {}
    }
}

/// ffmpeg 能力探测(编码器清单;一次探测,全程复用)。
#[derive(Debug, Default, Clone, Copy)]
pub struct FfmpegCaps {
    pub h264_nvenc: bool,
    pub h264_amf: bool,
    pub h264_qsv: bool,
    pub libx264: bool,
}

pub fn ffmpeg_caps() -> FfmpegCaps {
    let list = Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    FfmpegCaps {
        h264_nvenc: list.contains("h264_nvenc"),
        h264_amf: list.contains("h264_amf"),
        h264_qsv: list.contains("h264_qsv"),
        libx264: list.contains("libx264"),
    }
}

/// 从 HTML 源提取 `<style>` 块内容(anim_coverage 判定用;不入 Document,
/// 只读文本,与浏览器实际播放的声明同源——内联 style 块)。
pub(crate) fn style_blocks_of_html(html: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string(html) else {
        return out;
    };
    let mut rest = text.as_str();
    while let Some(start) = rest.find("<style") {
        let Some(open_rel) = rest[start..].find('>') else {
            break;
        };
        let after = start + open_rel + 1;
        let Some(close) = rest[after..].find("</style>") else {
            break;
        };
        out.push(rest[after..after + close].to_string());
        rest = &rest[after + close + "</style>".len()..];
    }
    out
}

/// 动画导出入口:GIF 走内存路径(调色板需全帧统计),MP4 走流式并行管道。
#[allow(clippy::too_many_arguments)]
pub fn export_anim(
    source: &Path,
    format: Format,
    width: u32,
    height: u32,
    fps: u32,
    duration_s: f32,
    scale: u32,
    bitrate_kbps: u32,
    gif_loops: u16,
) -> Result<AnimLaneResult, String> {
    let opts = AnimPipeOpts {
        width,
        height,
        fps,
        duration_s,
        scale,
        bitrate_kbps,
        ..AnimPipeOpts::default()
    };
    match format {
        Format::Mp4 => export_anim_pipe(source, &opts),
        _ => export_anim_inmemory(source, format, gif_loops, &opts),
    }
}

/// 完整选项版入口(kiln-cli `--workers/--gpu/--img/--encoder/--wall`)。
pub fn export_anim_pipe(source: &Path, opts: &AnimPipeOpts) -> Result<AnimLaneResult, String> {
    let t_start = std::time::Instant::now();
    let (mount_dir, html_path) = crate::domexport::resolve_source(source)?;
    let srv = vb_browser::staticsrv::StaticServer::start(&mount_dir)?;
    let url = if source.is_dir() {
        srv.url_for_dir()?
    } else {
        let name = html_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("源文件名非法")?;
        format!(
            "http://127.0.0.1:{}/{}",
            srv.port(),
            crate::domexport::url_encode(name)
        )
    };
    let exe = vb_browser::discover_browser(None)
        .ok_or("未发现系统浏览器(Edge/Chrome);动画浏览器路线不可用")?;

    let fps = opts.fps.clamp(1, 60);
    let n = ((fps as f32 * opts.duration_s.max(0.1)).ceil().max(1.0)) as usize;
    let vw = if opts.width > 0 { opts.width } else { 1080 };
    let vh = if opts.height > 0 { opts.height } else { vw };
    let dsf = opts.scale.clamp(1, 8);

    // 编码器解析(一次探测);ffmpeg 缺失时整体退 GIF 流(与旧行为一致)
    if !crate::frames::ffmpeg_available() {
        let mut r = export_anim_inmemory(source, Format::Gif, 0, opts)?;
        r.warnings.insert(0, "ffmpeg 缺失,MP4 降级 GIF 流".into());
        return Ok(r);
    }
    let enc_cands = opts.encoder.resolve()?;
    let mut warnings: Vec<String> = Vec::new();
    let chain: Vec<String> = enc_cands
        .iter()
        .map(|c| {
            let tag = if *c == EncChoice::X264 {
                "CPU"
            } else {
                "硬件"
            };
            format!("{}({})", c.codec_name(), tag)
        })
        .collect();
    warnings.push(format!("编码器候选:{}", chain.join(" → ")));
    if let Some(q) = opts.jpeg_quality {
        warnings.push(format!(
            "中间帧 JPEG(q={q},4:2:0 色度损失;交付建议 --img png)"
        ));
    }

    // 分段:总帧数均分;workers 上限 = 帧数(每段至少 1 帧)。
    // **默认 1**:两轮下游实测(9070 GRE)多实例都是负收益——轻量页面
    // 也随段数变慢(w1 10.6 → w2 5.9 → w3 1.8 帧/s),且 ≥4 段 HTTP 读
    // 超时;现象是跨实例串行点而非 CPU 不足,在定位清楚前不再按 CPU/2
    // 猜。并发是显式 opt-in:`--workers 2..16`(超限时段级候选回退兜底)。
    let cores = std::thread::available_parallelism()
        .map(|c| c.get())
        .unwrap_or(4);
    let _ = cores;
    let w_auto: u32 = 1;
    let workers = if opts.workers == 0 {
        w_auto
    } else {
        opts.workers.clamp(1, 16)
    }
    .min(n as u32) as usize;
    warnings.push(format!(
        "分段并行:{workers} 实例 × {} 帧(确定性={}, GPU={};默认 1,并发经 --workers 显式开启)",
        n.div_ceil(workers),
        if opts.wall_clock { "关" } else { "开" },
        if opts.gpu { "开" } else { "关" },
    ));

    // 临时工作区:段 mp4 + concat 清单
    let tmp = std::env::temp_dir().join(format!(
        "kiln-anim-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("创建动画工作目录失败:{e}"))?;

    // 共享状态进 scope 线程:同一静态服务,W 个独立浏览器实例。
    // **首帧门控派生**:worker i+1 等 worker i 抓到首帧(页面就绪)才拉起
    // —— 冷启动彻底串行化,渲染在各自就绪后并行。盲等 600ms 挡不住
    // 弱机上的并发冷启动雪崩(下游 w4 Page.navigate 超时)。
    let seg_results: Vec<Result<(EncChoice, Vec<String>), String>> = std::thread::scope(|scope| {
        let per = n.div_ceil(workers);
        let mut handles = Vec::with_capacity(workers);
        // 全部 tx 克隆共用一条就绪通道;主循环顺序收 N-1 个信号
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        for wi in 0..workers {
            // 门控:i>0 等前一个实例就绪(30s 兜底,防前实例挂死拖垮全队)
            if wi > 0 {
                let _ = ready_rx.recv_timeout(Duration::from_secs(30));
            }
            let a = wi * per;
            let b = (a + per).min(n);
            let url = url.clone();
            let exe = exe.clone();
            let enc_cands = enc_cands.clone();
            let seg_path = tmp.join(format!("seg_{wi:04}.mp4"));
            let ready_tx = if wi + 1 < workers {
                Some(ready_tx.clone())
            } else {
                None
            };
            let srv_ref = &srv;
            let is_edge = exe
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_ascii_lowercase().contains("msedge"))
                .unwrap_or(false);
            handles.push(scope.spawn(move || {
                render_segment(
                    &exe, srv_ref, &url, &seg_path, a, b, fps, vw, vh, dsf, opts, &enc_cands, wi,
                    ready_tx, is_edge,
                )
            }));
        }
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_else(|_| Err("渲染线程崩溃".into())))
            .collect()
    });

    let mut failed = None;
    let mut encoder_used: Option<String> = None;
    for (wi, r) in seg_results.iter().enumerate() {
        match r {
            Ok((enc_used, ws)) => {
                if encoder_used.is_none() {
                    encoder_used = Some(enc_used.codec_name().to_string());
                }
                warnings.extend(ws.iter().cloned());
            }
            Err(e) => {
                failed = Some(format!("分段 {wi} 渲染失败:{e}"));
                break;
            }
        }
    }
    if let Some(e) = failed {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }

    // concat 无缝拼接(同编码参数 CFR,-c copy 零重编码)
    let out_path = tmp.join("out.mp4");
    let concat_result = (|| -> Result<(), String> {
        if workers == 1 {
            std::fs::rename(tmp.join("seg_0000.mp4"), &out_path)
                .map_err(|e| format!("段文件改名失败:{e}"))?;
            return Ok(());
        }
        let list = tmp.join("list.txt");
        let mut text = String::new();
        for wi in 0..workers {
            text.push_str(&format!("file 'seg_{wi:04}.mp4'\n"));
        }
        std::fs::write(&list, text).map_err(|e| format!("写 concat 清单失败:{e}"))?;
        let output = Command::new("ffmpeg")
            .current_dir(&tmp)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "concat",
                "-safe",
                "0",
                "-i",
                "list.txt",
                "-c",
                "copy",
                "-movflags",
                "+faststart",
                "out.mp4",
            ])
            .output()
            .map_err(|e| format!("ffmpeg concat 启动失败:{e}"))?;
        if !output.status.success() {
            return Err(format!(
                "ffmpeg concat 失败: {}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(300)
                    .collect::<String>()
            ));
        }
        Ok(())
    })();
    if let Err(e) = concat_result {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let bytes = std::fs::read(&out_path).map_err(|e| format!("读取成片失败:{e}"))?;
    let _ = std::fs::remove_dir_all(&tmp);

    warnings.push(format!(
        "动画流水线:{n} 帧 @ {fps}fps,总耗时 {:.1}s({:.1} 帧/s)",
        t_start.elapsed().as_secs_f32(),
        n as f32 / t_start.elapsed().as_secs_f32().max(1e-3),
    ));

    // VB-1:静态资源 404 不许静默;VB-3:覆盖矩阵随结果输出
    warnings.extend(
        srv.take_not_found()
            .iter()
            .map(|s| vb_browser::staticsrv::asset_not_found_message(s)),
    );
    let browser = vb_browser::browser::browser_version(&exe);
    let anim_coverage = crate::anim::AnimCoverage::from_keyframes(
        &crate::anim::parse_keyframes(&style_blocks_of_html(&html_path)),
        "browser",
    );
    Ok(AnimLaneResult {
        bytes,
        frames: n,
        browser,
        encoder_used,
        warnings,
        anim_coverage,
    })
}

/// 渲染一个帧区间 [a, b) 到独立 mp4:独立浏览器实例 + image2pipe 直通编码。
/// 编码失败(如 N 卡并发会话超限)沿候选表回退整段重试;确定性寻址下
/// 重试就是重新 seek,结果与首跑一致。返回(实际用上的编码器,告警)。
#[allow(clippy::too_many_arguments)]
fn render_segment(
    exe: &Path,
    srv: &vb_browser::staticsrv::StaticServer,
    url: &str,
    seg_path: &Path,
    a: usize,
    b: usize,
    fps: u32,
    vw: u32,
    vh: u32,
    dsf: u32,
    opts: &AnimPipeOpts,
    enc_cands: &[EncChoice],
    wi: usize,
    ready_tx: Option<std::sync::mpsc::Sender<()>>,
    is_edge: bool,
) -> Result<(EncChoice, Vec<String>), String> {
    let _ = srv; // 静态服务由调用方持有保活;连接经 URL,无需逐段操作
    let mut warnings = Vec::new();
    // Edge 没有 HeadlessExperimental 域(0.12.3 起 beginFrame 首败缓存;
    // 这里直接按浏览器识别跳过,连第一次尝试都不必浪费)
    if is_edge && wi == 0 {
        warnings.push(
            "Edge 不支持 beginFrame,逐帧走 captureScreenshot(需要该通道可用 Chrome/chrome-headless-shell)"
                .into(),
        );
    }
    warnings.push(format!(
        "实例 {wi}:区间 {a}..{b},驱动 {}",
        if opts.wall_clock {
            "墙钟"
        } else {
            "确定性"
        }
    ));
    let proc = vb_browser::browser::BrowserProcess::launch_with(
        exe,
        vb_browser::browser::LaunchOptions { gpu: opts.gpu },
    )?;
    let mut page = vb_browser::page::PageSession::attach(&proc)?;
    page.set_device_metrics(vw, vh, dsf)?;
    page.navigate(url)?;
    page.wait_network_idle(Duration::from_secs(3));
    vb_browser::capture::wait_assets(&mut page);
    page.sleep(250); // 首帧稳定(不冻结动画、不仿真 reduced-motion)
                     // 首帧门控:本实例页面就绪,放行下一个实例拉起(彻底串行化冷启动)
    if let Some(tx) = &ready_tx {
        let _ = tx.send(());
    }

    // 帧驱动探测:JS 确定性时间轴(SEEK 约定)> CSS 动画寻址 > 墙钟回退
    let driver = if opts.wall_clock {
        Driver::WallFallback
    } else {
        let d = detect_driver(&mut page, &opts.seek_fn);
        match &d {
            Driver::JsSeek(name) => {
                warnings.push(format!(
                    "帧驱动:window.{name}(JS 确定性时间轴,分段并行安全)"
                ));
            }
            Driver::CssAnims(count) => {
                let paused = pause_all_animations(&mut page);
                warnings.push(format!(
                    "帧驱动:CSS/WAAPI 动画寻址({count} 条,已暂停 {paused})"
                ));
            }
            Driver::WallFallback => {
                warnings.push(
                    "页面无 window.SEEK 且无 CSS 动画:退回墙钟节奏(页面无自驱动时输出为静止画面)"
                        .into(),
                );
            }
        }
        d
    };

    let mut last_err = String::new();
    for (ci, &enc) in enc_cands.iter().enumerate() {
        let bf_broken = std::cell::Cell::new(is_edge);
        match capture_and_encode(
            &mut page, seg_path, a, b, fps, vw, vh, dsf, opts, enc, &driver, &bf_broken, wi,
        ) {
            Ok(ws) => {
                warnings.extend(ws);
                if ci > 0 {
                    warnings.push(format!(
                        "段编码回退:{} 不可用,已用 {} 完成",
                        enc_cands[0].codec_name(),
                        enc.codec_name()
                    ));
                }
                page.close();
                drop(proc);
                return Ok((enc, warnings));
            }
            Err(e) => {
                last_err = e;
                if ci + 1 < enc_cands.len() {
                    warnings.push(format!(
                        "编码器 {} 失败,尝试 {}",
                        enc.codec_name(),
                        enc_cands[ci + 1].codec_name()
                    ));
                }
            }
        }
    }
    Err(last_err)
}

/// 单次"采集 [a,b) + 直通编码"尝试:截图字节 → ffmpeg stdin → mp4。
#[allow(clippy::too_many_arguments)]
fn capture_and_encode(
    page: &mut vb_browser::page::PageSession,
    seg_path: &Path,
    a: usize,
    b: usize,
    fps: u32,
    vw: u32,
    vh: u32,
    dsf: u32,
    opts: &AnimPipeOpts,
    enc: EncChoice,
    driver: &Driver,
    bf_broken: &std::cell::Cell<bool>,
    wi: usize,
) -> Result<Vec<String>, String> {
    // 每实例帧间隔统计(串行点自证:多实例下单实例节奏显著劣化会直接
    // 反映在这行数据里)
    let stats_t0 = std::time::Instant::now();
    let mut stats_frames: usize = 0;
    let _ = dsf; // 设备像素比已在页面建立时设定
    let mut warnings = Vec::new();
    // ffmpeg 直通:截图字节 → stdin → 编码(零解码零重编码)
    let (shot_format, shot_quality, in_codec) = match opts.jpeg_quality {
        Some(q) => ("jpeg", Some(q), "mjpeg"),
        None => ("png", None, "png"),
    };
    let interval_us = (1_000_000.0f64 / fps as f64).round() as u64;
    // 墙钟车道按实际耗时记账(下游实测反馈 #2):旧实现采样只有
    // ~1 帧/s 仍按 1/fps 打时间戳 → 成片快放且速度不均。两段式:
    // 先采 PROBE 帧实测节奏(median),ffmpeg 以实测节奏 CFR 编码
    // (内容零丢失、时长≈真实墙钟;速度为均值口径,抖动不还原)。
    let mut probe_buf: Vec<Vec<u8>> = Vec::new();
    let wall_fps = if *driver == Driver::WallFallback {
        let clip_probe = (0.0, 0.0, vw as f64, vh as f64);
        let mut stamps: Vec<f64> = Vec::new();
        let t0 = std::time::Instant::now();
        for k in 0..8 {
            if k > 0 {
                std::thread::sleep(Duration::from_micros(interval_us));
            }
            let png = match page.begin_frame_screenshot(shot_format, shot_quality) {
                Ok(b) => b,
                Err(_) => page
                    .screenshot(shot_format, shot_quality, Some(clip_probe), false, false)
                    .map_err(|e| format!("逐帧截屏失败(探针帧 {k}):{e}"))?,
            };
            stamps.push(t0.elapsed().as_secs_f64());
            probe_buf.push(png);
        }
        let mut gaps: Vec<f64> = stamps.windows(2).map(|w| (w[1] - w[0]) * 1000.0).collect();
        gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let pace_ms = gaps[gaps.len() / 2].max(1.0);
        let measured = 1000.0 / pace_ms;
        warnings.push(format!(
            "墙钟车道:实测采样节奏 {pace_ms:.0}ms/帧(≈{measured:.1} 帧/s,请求 {fps});成片按实测节奏记账,时长 ≈ {:.1}s",
            (b - a) as f64 * pace_ms / 1000.0
        ));
        Some((1000.0 / pace_ms).round().clamp(1.0, f64::from(fps)))
    } else {
        None
    };
    let mut cmd = Command::new("ffmpeg");
    cmd.args([
        "-y",
        "-loglevel",
        "error",
        "-f",
        "image2pipe",
        "-c:v",
        in_codec,
        "-framerate",
        &wall_fps
            .map(|f| f.to_string())
            .unwrap_or_else(|| fps.to_string()),
        "-i",
        "-",
        "-c:v",
        enc.codec_name(),
        "-pix_fmt",
        "yuv420p",
        "-b:v",
        &format!("{}k", opts.bitrate_kbps),
    ]);
    push_color_args(&mut cmd);
    match enc {
        EncChoice::X264 => {
            // 分段并行时按份额限制 x264 线程,避免 W 段互相超订 CPU
            let cores = std::thread::available_parallelism()
                .map(|c| c.get())
                .unwrap_or(4);
            let workers = opts.workers.max(1) as usize;
            cmd.arg("-preset")
                .arg("medium")
                .arg("-threads")
                .arg((cores / workers).clamp(1, 16).to_string());
        }
        EncChoice::Nvenc => {
            cmd.arg("-preset").arg("p4");
        }
        EncChoice::Amf => {
            cmd.arg("-quality").arg("balanced");
        }
        EncChoice::Qsv => {
            cmd.arg("-preset").arg("medium");
        }
        EncChoice::Auto => {}
    }
    cmd.arg("-movflags")
        .arg("+faststart")
        .arg(seg_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("ffmpeg 编码进程启动失败:{e}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法获取 ffmpeg stdin".to_string())?;

    let _ = (vw, vh); // 视口截图无 clip;vw/vh 仅用于 establish metrics
    for (k, png) in probe_buf.drain(..).enumerate() {
        stdin
            .write_all(&png)
            .map_err(|e| format!("写入 ffmpeg 管道失败(探针帧 {k}):{e}"))?;
    }
    for i in a..b {
        stats_frames += 1;
        if *driver != Driver::WallFallback {
            drive_frame(page, driver, i, fps);
        }
        let ts = std::time::Instant::now();
        // 逐帧截屏走 beginFrame(CDP 显式产帧,空闲页面不再等 ~530ms
        // 合成器调度);beginFrame 不支持 clip —— 视口即画布(dsf 生效),
        // 静态车道的 settle/capture_png 协议不受影响
        // 不带 clip:clip 会强制 Chromium 重光栅化裁剪区(实测每帧 +150ms
        // 以上);视口即画布(dsf 经 set_device_metrics 生效),无需 clip。
        // VB_NO_BEGINFRAME=1 强制走 captureScreenshot(路径对照/兼容开关)。
        // beginFrame 失败(Edge 无 HeadlessExperimental 域,下游实测)后
        // **缓存结论**:此前每帧都重试一次必然失败的调用再退回,等于每帧
        // 多付一次死往返。
        let no_bf = std::env::var("VB_NO_BEGINFRAME").is_ok();
        let png = if no_bf || bf_broken.get() {
            // PNG 快档:optimizeForSpeed(Chrome 125+)轻压缩换快编码,
            // 无 JPEG 的 4:2:0 色度损失 —— 文字密集页交付的正解
            if shot_format == "png" {
                page.screenshot_fast_png().or_else(|_| {
                    page.screenshot(shot_format, shot_quality, None, false, false)
                        .map_err(|e| format!("逐帧截屏失败(帧 {i}):{e}"))
                })?
            } else {
                page.screenshot(shot_format, shot_quality, None, false, false)
                    .map_err(|e| format!("逐帧截屏失败(帧 {i}):{e}"))?
            }
        } else {
            match page.begin_frame_screenshot(shot_format, shot_quality) {
                Ok(b) => b,
                Err(e) => {
                    bf_broken.set(true);
                    if i == a {
                        warnings.push(format!("beginFrame 不可用,本段退回 captureScreenshot: {e}"));
                    }
                    if shot_format == "png" {
                        page.screenshot_fast_png().or_else(|_| {
                            page.screenshot(shot_format, shot_quality, None, false, false)
                                .map_err(|e| format!("逐帧截屏失败(帧 {i}):{e}"))
                        })?
                    } else {
                        page.screenshot(shot_format, shot_quality, None, false, false)
                            .map_err(|e| format!("逐帧截屏失败(帧 {i}):{e}"))?
                    }
                }
            }
        };
        if std::env::var("VB_ANIM_TIMING").is_ok() {
            eprintln!(
                "[timing] frame {i}: screenshot {:?} ({} KB)",
                ts.elapsed(),
                png.len() / 1024
            );
        }
        if let Err(e) = stdin.write_all(&png) {
            // 管道断裂 = ffmpeg 已死(典型:硬件编码会话超限被驱动拒绝)。
            // 收尸带出 stderr,别让真死因埋在 os error 109 里
            let stderr = child
                .wait_with_output()
                .ok()
                .map(|o| {
                    let t = String::from_utf8_lossy(&o.stderr).to_string();
                    t.chars()
                        .skip(t.chars().count().saturating_sub(300))
                        .collect::<String>()
                })
                .unwrap_or_default();
            return Err(format!(
                "写入 ffmpeg 管道失败(帧 {i}):{e};ffmpeg 已退出,stderr 尾部: {stderr}"
            ));
        }
    }
    drop(stdin); // EOF → ffmpeg 收尾
    let elapsed = stats_t0.elapsed().as_secs_f64();
    warnings.push(format!(
        "实例 {wi} 统计:{stats_frames} 帧 / {elapsed:.1}s = {:.1} 帧/s(平均 {:.0}ms/帧)",
        stats_frames as f64 / elapsed.max(1e-3),
        elapsed * 1000.0 / stats_frames.max(1) as f64,
    ));
    let output = child
        .wait_with_output()
        .map_err(|e| format!("等待 ffmpeg 失败:{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ffmpeg 编码失败: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(300)
                .collect::<String>()
        ));
    }
    Ok(warnings)
}

/// GIF / 兼容路径:内存帧序列(调色板需全帧统计;长片内存大,MP4 勿走此路)。
#[allow(clippy::too_many_arguments)]
fn export_anim_inmemory(
    source: &Path,
    format: Format,
    gif_loops: u16,
    opts: &AnimPipeOpts,
) -> Result<AnimLaneResult, String> {
    let (mount_dir, html_path) = crate::domexport::resolve_source(source)?;
    let srv = vb_browser::staticsrv::StaticServer::start(&mount_dir)?;
    let url = if source.is_dir() {
        srv.url_for_dir()?
    } else {
        let name = html_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("源文件名非法")?;
        format!(
            "http://127.0.0.1:{}/{}",
            srv.port(),
            crate::domexport::url_encode(name)
        )
    };
    let exe = vb_browser::discover_browser(None)
        .ok_or("未发现系统浏览器(Edge/Chrome);动画浏览器路线不可用")?;
    let proc = vb_browser::browser::BrowserProcess::launch_with(
        &exe,
        vb_browser::browser::LaunchOptions { gpu: opts.gpu },
    )?;
    let browser = proc.version();
    let mut page = vb_browser::page::PageSession::attach(&proc)?;
    let vw = if opts.width > 0 { opts.width } else { 1080 };
    let vh = if opts.height > 0 { opts.height } else { vw };
    let dsf = opts.scale.clamp(1, 8);
    page.set_device_metrics(vw, vh, dsf)?;
    page.navigate(&url)?;
    page.wait_network_idle(Duration::from_secs(3));
    vb_browser::capture::wait_assets(&mut page);
    page.sleep(250);

    // 实时采样:首帧立即,其后按 1/fps 节奏;截图慢于间隔则不等待(自适应)。
    // GIF 的调色板需要全帧统计,保持内存路径;确定性驱动同样适用
    // (JS SEEK / CSS 动画寻址,采样节奏不再影响画面内容)。
    let fps = opts.fps.clamp(1, 60);
    let n = ((fps as f32 * opts.duration_s.max(0.1)).ceil().max(1.0)) as usize;
    let interval = 1.0f64 / fps as f64;
    let driver = if opts.wall_clock {
        Driver::WallFallback
    } else {
        let d = detect_driver(&mut page, &opts.seek_fn);
        if let Driver::CssAnims(_) = d {
            pause_all_animations(&mut page);
        }
        d
    };
    let mut captures: Vec<Frame> = Vec::with_capacity(n);
    let wall_t0 = std::time::Instant::now();
    for i in 0..n {
        if driver == Driver::WallFallback {
            if i > 0 {
                std::thread::sleep(Duration::from_secs_f64(interval));
            }
        } else {
            drive_frame(&mut page, &driver, i, fps);
        }
        let png = page
            .screenshot(
                "png",
                None,
                Some((0.0, 0.0, vw as f64, vh as f64)),
                false,
                false,
            )
            .map_err(|e| format!("逐帧截屏失败(帧 {i}):{e}"))?;
        let mut img = image::load_from_memory(&png)
            .map_err(|e| format!("帧解码失败(帧 {i}):{e}"))?
            .to_rgba8();
        // 白底展平(与 capture_png 非透明口径一致)
        for px in img.pixels_mut() {
            if px[3] < 255 {
                let a = px[3] as f32 / 255.0;
                for c in 0..3 {
                    px[c] = (px[c] as f32 * a + 255.0 * (1.0 - a)).round() as u8;
                }
                px[3] = 255;
            }
        }
        let (fw_i, fh_i) = (img.width(), img.height());
        captures.push(Frame {
            rgba: img.into_raw(),
            width: fw_i,
            height: fh_i,
            delay_ms: (1000.0 / fps as f64).round() as u32,
        });
    }
    page.close();
    drop(proc);
    if captures.is_empty() {
        return Err("采样得到 0 帧".into());
    }
    // 墙钟记账:帧延迟按实测总耗时均值(速度正确;此前固定 1/fps,
    // 采样慢于网格时成片快放)
    if driver == Driver::WallFallback {
        let real_delay = (wall_t0.elapsed().as_secs_f64() * 1000.0 / n as f64).round() as u32;
        for f in captures.iter_mut() {
            f.delay_ms = real_delay.max(20);
        }
    }
    // 尺寸一致性守卫(ffmpeg 要求恒定帧尺寸;异常帧裁到首帧尺寸)
    let (fw, fh) = (captures[0].width, captures[0].height);
    let mut warnings: Vec<String> = Vec::new();
    for f in captures.iter_mut() {
        if f.width != fw || f.height != fh {
            warnings.push(format!(
                "帧尺寸漂移 {}x{}→{fw}x{fh},已裁齐",
                f.width, f.height
            ));
            if let Some(img) =
                image::RgbaImage::from_raw(f.width, f.height, std::mem::take(&mut f.rgba))
            {
                let cw = fw.min(f.width);
                let ch = fh.min(f.height);
                let cropped = image::imageops::crop_imm(&img, 0, 0, cw, ch).to_image();
                f.width = cropped.width();
                f.height = cropped.height();
                f.rgba = cropped.into_raw();
            }
        }
    }

    let ctx = ExportContext {
        artboard_name: String::new(),
        doc_title: String::new(),
        logical_w: vw as f64,
        logical_h: vh as f64,
        out_w: fw,
        out_h: fh,
        scale: dsf,
        transparent: false,
        requested_transparent: false,
        list: DrawList {
            w: vw as f64,
            h: vh as f64,
            background: [1.0, 1.0, 1.0, 1.0],
            items: Vec::new(),
        },
        frames: captures,
        fps,
        duration_s: opts.duration_s,
        gif_loops,
        mp4_bitrate_kbps: opts.bitrate_kbps,
        jpeg_quality: 92,
        build_warnings: Vec::new(),
        anim_coverage: None,
        project_dir: None,
    };
    let bytes = match format {
        Format::Mp4 => crate::frames::encode_mp4(&ctx).map_err(|e| e.to_string())?,
        _ => crate::frames::encode_gif(&ctx).map_err(|e| e.to_string())?,
    };
    warnings.extend(
        srv.take_not_found()
            .iter()
            .map(|s| vb_browser::staticsrv::asset_not_found_message(s)),
    );
    let anim_coverage = crate::anim::AnimCoverage::from_keyframes(
        &crate::anim::parse_keyframes(&style_blocks_of_html(&html_path)),
        "browser",
    );
    Ok(AnimLaneResult {
        bytes,
        frames: n,
        browser,
        encoder_used: None,
        warnings,
        anim_coverage,
    })
}
