//! 帧序列与 ffmpeg 进程桥(GIF/MP4)。
//!
//! GIF:Kiln 自研编码(image crate 感知量化 + LZW),零外部依赖。
//! MP4:探测 ffmpeg → libx264/yuv420p;无 ffmpeg 降级 GIF 流并告警。

use std::process::Command;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Frame as ImgFrame, RgbaImage};

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

/// RGBA 帧序列 → GIF 字节(循环控制;帧延迟单位 10ms,最小 20ms)。
pub fn encode_gif(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    let first = ctx
        .frames
        .first()
        .ok_or_else(|| KilnError::BadAnimation("空帧序列".into()))?;
    let mut out = Vec::with_capacity(512 * 1024.max(first.width as usize * first.height as usize / 2));
    let mut encoder = GifEncoder::new(&mut out);
    encoder
        .set_repeat(if ctx.gif_loops == 0 {
            Repeat::Infinite
        } else {
            Repeat::Finite(ctx.gif_loops)
        })
        .map_err(|e| KilnError::BadAnimation(format!("GIF 编码器初始化失败:{e}")))?;
    for f in &ctx.frames {
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

/// RGBA 帧序列 → MP4(libx264 yuv420p;帧 PNG 落临时目录 → ffmpeg 编码)。
pub fn encode_mp4(ctx: &ExportContext) -> KilnResult<Vec<u8>> {
    if !ffmpeg_available() {
        // 降级:GIF 流(播放器大多兼容);告警由 Mp4Writer 补
        return encode_gif(ctx);
    }
    let seq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("kiln-mp4-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(KilnError::Io)?;

    let result = (|| -> KilnResult<Vec<u8>> {
        for (i, f) in ctx.frames.iter().enumerate() {
            let img = RgbaImage::from_raw(f.width, f.height, f.rgba.clone())
                .ok_or_else(|| KilnError::BadAnimation("帧尺寸不一致".into()))?;
            img.save_with_format(tmp.join(format!("f{i:05}.png")), image::ImageFormat::Png)
                .map_err(|e| KilnError::Encode(format!("帧落盘失败:{e}")))?;
        }
        let mp4_path = tmp.join("out.mp4");
        let output = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-framerate",
                &ctx.fps.to_string(),
                "-i",
                tmp.join("f%05d.png").to_str().unwrap_or("f%05d.png"),
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-b:v",
                &format!("{}k", ctx.mp4_bitrate_kbps),
                "-movflags",
                "+faststart",
                mp4_path.to_str().unwrap_or("out.mp4"),
            ])
            .output()
            .map_err(|e| KilnError::FfmpegFailed {
                code: None,
                stderr: e.to_string(),
            })?;
        if !output.status.success() {
            return Err(KilnError::FfmpegFailed {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(400)
                    .collect(),
            });
        }
        std::fs::read(&mp4_path).map_err(KilnError::Io)
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    result
}
