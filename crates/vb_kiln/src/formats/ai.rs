//! Ai 格式包装:PDF 兼容流 + Illustrator 头(ADR-0008 路线)。

use std::time::Instant;

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};

pub struct AiWriter;

impl FormatWriter for AiWriter {
    fn format(&self) -> Format {
        Format::Ai
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let warnings = common_warnings(ctx, Format::Ai);
        let bytes = crate::postscript::write_ai(ctx)?;
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = 1;
        Ok(r)
    }
}
