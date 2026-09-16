//! EPS 格式包装(postscript::write_eps 的 FormatWriter 接口)。

use std::time::Instant;

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};

pub struct EpsWriter;

impl FormatWriter for EpsWriter {
    fn format(&self) -> Format {
        Format::Eps
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let warnings = common_warnings(ctx, Format::Eps);
        let bytes = crate::postscript::write_eps(ctx)?;
        out.extend_from_slice(&bytes);
        let mut r = crate::writer::report_with(warnings, t, bytes.len());
        r.frame_count = 1;
        Ok(r)
    }
}
