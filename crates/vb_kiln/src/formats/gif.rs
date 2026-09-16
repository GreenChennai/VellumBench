//! GIF 写入器:帧序列 + 感知量化 + 循环控制。

use std::time::Instant;

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::frames::encode_gif;
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};

pub struct GifWriter;

impl FormatWriter for GifWriter {
    fn format(&self) -> Format {
        Format::Gif
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let warnings = common_warnings(ctx, Format::Gif);
        let bytes = encode_gif(ctx)?;
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = ctx.frames.len();
        Ok(r)
    }
}
