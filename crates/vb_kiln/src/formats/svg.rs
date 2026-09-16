//! SVG 写入器:复用 vb_export::svg(真实 <text> 可编辑 + 渐变/圆角/旋转)。

use std::time::Instant;

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};

pub struct SvgWriter;

impl FormatWriter for SvgWriter {
    fn format(&self) -> Format {
        Format::Svg
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let warnings = common_warnings(ctx, Format::Svg);
        let svg = vb_export::svg::render_svg(&ctx.list, ctx.scale.max(1), ctx.transparent);
        let bytes = svg.into_bytes();
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = 1;
        Ok(r)
    }
}
