//! 帧序列与 ffmpeg 进程桥(GIF/MP4)。
//!
//! GIF:Kiln 自研编码(image crate 感知量化 + LZW),零外部依赖。
//! MP4:探测 ffmpeg → libx264/yuv420p;无 ffmpeg 降级 GIF 流并告警。
//!
//! ## 协作式取消(硬骨头 #6,native 车道)
//!
//! `ctx.cancel = Some` 时,本模块在**分段边界**检查取消:每帧 PNG 落盘
//! 前、GIF 逐帧编码前、每次 ffmpeg 子进程运行期间(100ms 轮询,命中即
//! kill + wait 收尸)。命中返回 `KilnError::Cancelled`;临时帧目录由既
//! 有失败清扫路径(`remove_dir_all`)照常清理,产物不落半截。

use std::process::Command;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Frame as ImgFrame, RgbaImage};

use crate::cancel::guard_kiln;
use crate::context::ExportContext;
use crate::error::{KilnError, KilnResult, KilnWarning};

/// ffmpeg 是否在 PATH。
pub fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// 可取消的 ffmpeg 等待:100ms 轮询子进程;取消令牌命中即 kill + wait
/// 收尸(不泄漏进程),返回 `Cancelled`。
///
/// stderr 死锁边界:调用方均用 `-loglevel error`,子进程输出远小于管道
/// 缓冲(64KB),轮询期间不读管道也不会阻塞子进程;退出后统一
/// `wait_with_output` 排干收割。
fn wait_ffmpeg_cancellable(
    mut child: std::process::Child,
    ctx: &ExportContext,
    what: &str,
) -> KilnResult<std::process::Output> {
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            return child
                .wait_with_output()
                .map_err(|e| KilnError::FfmpegFailed {
                    code: None,
                    stderr: e.to_string(),
                });
        }
        if ctx.cancel.as_ref().is_some_and(|t| t.is_cancelled()) {
            let _ = child.kill();
            let _ = child.wait(); // 收尸:防 ffmpeg 僵尸/半截文件句柄
            return Err(KilnError::Cancelled);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = what; // 段标注留给人读的日志/断言扩展
    }
}

/// GIF 调色板实际走的编码路径(UP-4:让「走了哪条」可观测,而非隐式 `ffmpeg_available()`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GifPalettePath {
    /// ffmpeg `palettegen=stats_mode=diff` + `paletteuse=dither=sierra2_4a` 两 pass。
    FfmpegTwoPass,
    /// 纯 Rust 感知量化 + LZW 单 pass(未装 ffmpeg,或 ffmpeg 桥失败回退)。
    RustSinglePass,
}

impl GifPalettePath {
    pub fn as_str(self) -> &'static str {
        match self {
            GifPalettePath::FfmpegTwoPass => "ffmpeg-palettegen+paletteuse(两 pass)",
            GifPalettePath::RustSinglePass => "image-感知量化+LZW(单 pass)",
        }
    }
}

/// GIF 编码结果 + 实际路径声明(UP-4)。
///
/// `ExportContext` 在写出器签名里是 `&`(不可变),所以**不能**在 encode_* 内部
/// 往 `ctx.build_warnings` 塞告警(要为此把整个字段改成 RefCell,不值当)。
/// 改为把声明**回传给调用方**,由它们 push 进自己那份 report ——
/// `FormatWriter::write(&self, ctx: &ExportContext, out)` 本来就只能新增
/// 本地告警,这条路不 invasive。
pub struct GifEncodeOutcome {
    pub bytes: Vec<u8>,
    pub path: GifPalettePath,
    /// ffmpeg 两 pass 失败而回退时的原始原因(未装 ffmpeg 时为 None)。
    pub ffmpeg_error: Option<String>,
}

impl GifEncodeOutcome {
    /// 该次编码的声明告警。**非降级**:两条路径都产出合法 GIF89a、内容一致,
    /// 只是色阶与体积的档位不同 —— 所以不进 `is_degrading()`。
    pub fn warning(&self) -> KilnWarning {
        KilnWarning::GifPalettePath {
            via_ffmpeg: self.path == GifPalettePath::FfmpegTwoPass,
            ffmpeg_error: self.ffmpeg_error.clone(),
        }
    }
}

/// RGBA 帧序列 → GIF 字节(不取路径声明;要声明用 `encode_gif_reporting`)。
pub fn encode_gif(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    encode_gif_reporting(ctx).map(|o| o.bytes)
}

/// 同 `encode_gif`,额外回传**实际调色板路径**。
///
/// UP-4:此前 ffmpeg 桥失败被 `let _ = e;` **静默吞掉**,报告里也不声明走了
/// 哪条路 —— 下游无法区分「没装 ffmpeg」「桥失败已回退」,而两者的色阶与体积
/// 都劣于两 pass。现在失败原因与路径一并回传。
pub fn encode_gif_reporting(ctx: &ExportContext) -> KilnResult<GifEncodeOutcome> {
    guard_kiln(&ctx.cancel, "encode_gif 入口")?;
    if ffmpeg_available() {
        match encode_gif_ffmpeg(ctx) {
            Ok(bytes) => {
                return Ok(GifEncodeOutcome {
                    bytes,
                    path: GifPalettePath::FfmpegTwoPass,
                    ffmpeg_error: None,
                })
            }
            Err(KilnError::Cancelled) => return Err(KilnError::Cancelled),
            Err(e) => {
                // 桥失败(参数/编码异常)降级纯 Rust 路径,不中断导出 —— 但**必须留痕**,
                // 否则「本机 ffmpeg 坏了」会被当成「没装 ffmpeg」,排错方向完全不同。
                let why = e.to_string();
                let bytes = encode_gif_image(ctx)?;
                return Ok(GifEncodeOutcome {
                    bytes,
                    path: GifPalettePath::RustSinglePass,
                    ffmpeg_error: Some(why),
                });
            }
        }
    }
    Ok(GifEncodeOutcome {
        bytes: encode_gif_image(ctx)?,
        path: GifPalettePath::RustSinglePass,
        ffmpeg_error: None,
    })
}

/// ffmpeg 调色板双通道 GIF(参照 WPI gif_exporter 实测方案)。
fn encode_gif_ffmpeg(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    // EXP-10:落盘点统一经 limits::temp_name(命名约定即注册,pid 可解析回收)
    let tmp = std::env::temp_dir().join(vb_browser::limits::temp_name("gif", ""));
    std::fs::create_dir_all(&tmp).map_err(KilnError::Io)?;

    let result = (|| -> KilnResult<Vec<u8>> {
        for (i, f) in ctx.frames.iter().enumerate() {
            guard_kiln(&ctx.cancel, &format!("GIF 帧落盘分段边界 {i}"))?;
            let img = image::RgbaImage::from_raw(f.width, f.height, f.rgba.clone())
                .ok_or_else(|| KilnError::BadAnimation("帧尺寸不一致".into()))?;
            img.save_with_format(tmp.join(format!("f{i:05}.png")), image::ImageFormat::Png)
                .map_err(|e| KilnError::Encode(format!("帧落盘失败:{e}")))?;
        }
        let out_path = tmp.join("out.gif");
        // UP-4b:`stats_mode` 必须是 `full`(默认),**不能用 `diff`**。
        //
        // `diff` 只把「相邻帧之间变化过的像素」纳入调色板统计。而 Kiln 自己的
        // 动效指导推荐**硬切**(`steps()` 卡点、`from`→`to` 整片瞬变),这类动画
        // 恰恰是「大片纯色整体切换」:从不变化的那部分对调色板零贡献,于是某些
        // 颜色根本进不了调色板,被静默映射到最接近的已有色 —— 颜色直接错掉。
        //
        // 实测(本机 ffmpeg,40 帧纯色 200x400,0-19 红 / 20-39 蓝):
        //   stats_mode=diff  → 40 帧全解码为红(蓝色整段丢失),2269B  ← 错
        //   stats_mode=full  → 第 20 帧起正确为蓝,2737B            ← 对
        // 与 dither 无关(dither=bayer / sierra2_4a 结果一致)。
        // 代价:调色板略大(2269B → 2737B)。`diff` 省的那点体积要用「颜色错掉」
        // 去换,不划算 —— 交付错色是静默失败,用户往往到成品才发现。
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-framerate",
                &ctx.fps.to_string(),
                "-i",
                tmp.join("f%05d.png").to_str().unwrap_or("f%05d.png"),
                "-filter_complex",
                "[0:v]split[x][y];[x]palettegen=stats_mode=full[p];[y][p]paletteuse=dither=sierra2_4a",
                "-loop",
                &ctx.gif_loops.to_string(),
                out_path.to_str().unwrap_or("out.gif"),
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| KilnError::FfmpegFailed {
                code: None,
                stderr: e.to_string(),
            })?;
        let output = wait_ffmpeg_cancellable(output, ctx, "GIF palettegen 编码")?;
        if !output.status.success() {
            return Err(KilnError::FfmpegFailed {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(300)
                    .collect(),
            });
        }
        std::fs::read(&out_path).map_err(KilnError::Io)
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    result
}

/// 纯 Rust 回退:image 感知量化 + LZW(循环控制;帧延迟最小 20ms)。
fn encode_gif_image(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    let first = ctx
        .frames
        .first()
        .ok_or_else(|| KilnError::BadAnimation("空帧序列".into()))?;
    let mut out =
        Vec::with_capacity(512 * 1024.max(first.width as usize * first.height as usize / 2));
    let mut encoder = GifEncoder::new(&mut out);
    encoder
        .set_repeat(if ctx.gif_loops == 0 {
            Repeat::Infinite
        } else {
            Repeat::Finite(ctx.gif_loops)
        })
        .map_err(|e| KilnError::BadAnimation(format!("GIF 编码器初始化失败:{e}")))?;
    for f in &ctx.frames {
        guard_kiln(&ctx.cancel, "GIF 逐帧编码边界(纯 Rust 路径)")?;
        let img = RgbaImage::from_raw(f.width, f.height, f.rgba.clone())
            .ok_or_else(|| KilnError::BadAnimation("帧尺寸不一致".into()))?;
        let delay = image::Delay::from_numer_denom_ms(f.delay_ms.max(20), 1);
        let frame = ImgFrame::from_parts(img, 0, 0, delay);
        encoder
            .encode_frame(frame)
            .map_err(|e| KilnError::BadAnimation(format!("GIF 帧编码失败:{e}")))?;
    }
    drop(encoder);
    Ok(out)
}

/// RGBA 帧序列 → MP4(编码器候选链 yuv420p;帧 PNG 落临时目录 → ffmpeg 编码)。
///
/// 编码器不再硬编码 libx264:走 [`crate::animlane::usable_encoder_chain`]
/// (nvenc→amf→qsv→x264,含运行时探针)逐个尝试;ffmpeg 缺失或候选全败
/// 时降级 GIF 流(与既有语义一致,告警由调用方补)。
pub fn encode_mp4(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    encode_mp4_reporting(ctx).map(|(b, _)| b)
}

/// 同 `encode_mp4`,额外回传**降级成 GIF 流时的调色板路径声明**(UP-4)。
///
/// 降级产物是 GIF 流写进 `.mp4`,调色板质量与真 GIF 完全同源 —— 所以
/// 「走了哪条」在这条路径上同样需要声明,否则一个"MP4"文件悄悄用了单 pass
/// 量化而报告里只说 `mp4_downgraded_to_gif`,排错时会去找错方向。
/// 真出 MP4 时第二项为 `None`(调色板概念不存在)。
pub fn encode_mp4_reporting(ctx: &ExportContext) -> KilnResult<(Vec<u8>, Option<KilnWarning>)> {
    guard_kiln(&ctx.cancel, "encode_mp4 入口")?;
    if !ffmpeg_available() {
        // 降级:GIF 流(播放器大多兼容);告警由 Mp4Writer 补
        let o = encode_gif_reporting(ctx)?;
        let w = o.warning();
        return Ok((o.bytes, Some(w)));
    }
    // EXP-10:落盘点统一经 limits::temp_name
    let tmp = std::env::temp_dir().join(vb_browser::limits::temp_name("mp4", ""));
    std::fs::create_dir_all(&tmp).map_err(KilnError::Io)?;

    let result = (|| -> KilnResult<(Vec<u8>, Option<KilnWarning>)> {
        for (i, f) in ctx.frames.iter().enumerate() {
            guard_kiln(&ctx.cancel, &format!("MP4 帧落盘分段边界 {i}"))?;
            let img = RgbaImage::from_raw(f.width, f.height, f.rgba.clone())
                .ok_or_else(|| KilnError::BadAnimation("帧尺寸不一致".into()))?;
            img.save_with_format(tmp.join(format!("f{i:05}.png")), image::ImageFormat::Png)
                .map_err(|e| KilnError::Encode(format!("帧落盘失败:{e}")))?;
        }
        let mp4_path = tmp.join("out.mp4");
        let mut last_err: Option<KilnError> = None;
        for enc in crate::animlane::usable_encoder_chain() {
            guard_kiln(
                &ctx.cancel,
                &format!("MP4 编码分段边界({})", enc.codec_name()),
            )?;
            let mut cmd = Command::new("ffmpeg");
            cmd.args([
                "-y",
                "-loglevel",
                "error",
                "-framerate",
                &ctx.fps.to_string(),
                "-i",
                tmp.join("f%05d.png").to_str().unwrap_or("f%05d.png"),
                "-c:v",
                enc.codec_name(),
                "-b:v",
                &format!("{}k", ctx.mp4_bitrate_kbps),
                "-movflags",
                "+faststart",
            ]);
            // GOP/B 帧/像素格式(UP-4b):native 车道与浏览器车道**同一份参数**,
            // 默认 None = 落编码器默认,逐位保持旧行为。
            crate::animlane::push_gop_args(
                &mut cmd,
                ctx.mp4_gop,
                ctx.mp4_b_frames,
                Some(ctx.mp4_pix_fmt.as_deref().unwrap_or("yuv420p")),
                Some(enc.codec_name()),
            );
            crate::animlane::push_color_args_with(&mut cmd, ctx.mp4_color_tags);
            crate::animlane::apply_encoder_args(&mut cmd, enc, 1);
            cmd.arg(mp4_path.to_str().unwrap_or("out.mp4"));
            let child = cmd
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| KilnError::FfmpegFailed {
                    code: None,
                    stderr: e.to_string(),
                })?;
            let output = wait_ffmpeg_cancellable(child, ctx, "MP4 编码")?;
            if !output.status.success() {
                // 候选链语义:单个编码器失败不终局,记下原因沿表回退下一个
                last_err = Some(KilnError::FfmpegFailed {
                    code: output.status.code(),
                    stderr: String::from_utf8_lossy(&output.stderr)
                        .chars()
                        .take(400)
                        .collect(),
                });
                continue;
            }
            return Ok((
                std::fs::read(&mp4_path).map_err(KilnError::Io)?,
                None,
            ));
        }
        // 候选全败(含"无可用编码器"):降级 GIF 流,语义与"无 ffmpeg"一致
        if let Some(e) = last_err {
            eprintln!("kiln: MP4 编码失败({e}),降级 GIF 流");
        }
        // 降级产物是 GIF 流 → 调色板路径声明照样要带(UP-4)
        let o = encode_gif_reporting(ctx)?;
        let w = o.warning();
        Ok((o.bytes, Some(w)))
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    result
}
