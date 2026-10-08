//! GIF 写入器:帧序列 + 感知量化 + 循环控制。

use std::time::Instant;

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::frames::encode_gif_reporting;
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};

pub struct GifWriter;

impl FormatWriter for GifWriter {
    fn format(&self) -> Format {
        Format::Gif
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        // UP-4:用 reporting 版拿「实际走了哪条调色板路径」,声明进本份 report。
        // 此前 encode_gif 把 ffmpeg 桥失败 `let _ = e;` 吞掉、报告里也不声明,
        // 下游分不清「没装 ffmpeg」与「桥失败已回退」—— 两者色阶/体积都劣于两 pass。
        let outcome = encode_gif_reporting(ctx)?;
        let mut warnings = common_warnings(ctx, Format::Gif);
        warnings.push(outcome.warning());
        let bytes = outcome.bytes;
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = ctx.frames.len();
        Ok(r)
    }
}
