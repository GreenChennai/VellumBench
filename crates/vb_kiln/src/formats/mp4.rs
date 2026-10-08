//! MP4 写入器:ffmpeg 桥(libx264/yuv420p);无 ffmpeg 降级 GIF 流。

use std::time::Instant;

use crate::context::ExportContext;
use crate::error::{KilnResult, KilnWarning};
use crate::frames::encode_mp4_reporting;
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};

pub struct Mp4Writer;

impl FormatWriter for Mp4Writer {
    fn format(&self) -> Format {
        Format::Mp4
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let mut warnings = common_warnings(ctx, Format::Mp4);
        // UP-4:reporting 版在**降级成 GIF 流**时回传调色板路径声明 ——
        // 产物是 GIF 写进 .mp4,调色板质量同源,同样不能默默用单 pass。
        let (bytes, gif_palette) = encode_mp4_reporting(ctx)?;
        let degraded = !crate::frames::ffmpeg_available();
        if degraded {
            warnings.push(KilnWarning::Mp4DowngradedToGif);
        }
        warnings.extend(gif_palette);
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = ctx.frames.len();
        // report_with 已按 VB-2 丢弃告警置 degraded;此处只叠加 ffmpeg 缺失
        r.degraded |= degraded;
        Ok(r)
    }
}
