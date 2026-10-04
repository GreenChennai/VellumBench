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
use crate::error::{KilnError, KilnResult};

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

/// RGBA 帧序列 → GIF 字节。
///
/// 双通道:优先 ffmpeg palettegen/paletteuse(WPI 同款,体积更优、
/// 动态调色板);无 ffmpeg 时回退 image 感知量化 LZW(纯 Rust 零依赖)。
pub fn encode_gif(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    guard_kiln(&ctx.cancel, "encode_gif 入口")?;
    if ffmpeg_available() {
        match encode_gif_ffmpeg(ctx) {
            Ok(bytes) => return Ok(bytes),
            Err(KilnError::Cancelled) => return Err(KilnError::Cancelled),
            Err(e) => {
                // 桥失败(参数/编码异常)降级纯 Rust 路径,不中断导出
                let _ = e;
            }
        }
    }
    encode_gif_image(ctx)
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
                "[0:v]split[x][y];[x]palettegen=stats_mode=diff[p];[y][p]paletteuse=dither=sierra2_4a",
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
    guard_kiln(&ctx.cancel, "encode_mp4 入口")?;
    if !ffmpeg_available() {
        // 降级:GIF 流(播放器大多兼容);告警由 Mp4Writer 补
        return encode_gif(ctx);
    }
    // EXP-10:落盘点统一经 limits::temp_name
    let tmp = std::env::temp_dir().join(vb_browser::limits::temp_name("mp4", ""));
    std::fs::create_dir_all(&tmp).map_err(KilnError::Io)?;

    let result = (|| -> KilnResult<Vec<u8>> {
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
                "-pix_fmt",
                "yuv420p",
                "-b:v",
                &format!("{}k", ctx.mp4_bitrate_kbps),
                "-movflags",
                "+faststart",
            ]);
            crate::animlane::push_color_args(&mut cmd);
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
            return std::fs::read(&mp4_path).map_err(KilnError::Io);
        }
        // 候选全败(含"无可用编码器"):降级 GIF 流,语义与"无 ffmpeg"一致
        if let Some(e) = last_err {
            eprintln!("kiln: MP4 编码失败({e}),降级 GIF 流");
        }
        encode_gif(ctx)
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    result
}
