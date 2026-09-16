//! PNG 写入器:vb_render CPU 引擎直出(真字形/渐变/圆角/位图/透明)。

use std::time::Instant;

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::raster::rasterize_png_bytes;
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};

pub struct PngWriter;

impl FormatWriter for PngWriter {
    fn format(&self) -> Format {
        Format::Png
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let warnings = common_warnings(ctx, Format::Png);
        let png = rasterize_png_bytes(&ctx.list, ctx.scale as f32, ctx.transparent)?;
        out.extend_from_slice(&png);
        let mut r = crate::writer::report_with(warnings, t, png.len());
        r.frame_count = 1;
        Ok(r)
    }
}
