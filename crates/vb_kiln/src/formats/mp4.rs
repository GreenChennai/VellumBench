//! MP4 写入器:ffmpeg 桥(libx264/yuv420p);无 ffmpeg 降级 GIF 流。

use std::time::Instant;

use crate::context::ExportContext;
use crate::error::{KilnResult, KilnWarning};
use crate::frames::encode_mp4;
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
        let bytes = encode_mp4(ctx)?;
        let degraded = !crate::frames::ffmpeg_available();
        if degraded {
            warnings.push(KilnWarning::Mp4DowngradedToGif);
        }
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = ctx.frames.len();
        r.degraded = degraded;
        Ok(r)
    }
}
