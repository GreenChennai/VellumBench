//! JPG 写入器:光栅 → quality 可调 JPEG;透明强制垫白底。

use std::time::Instant;

use image::codecs::jpeg::JpegEncoder;

use crate::context::ExportContext;
use crate::error::KilnResult;
use crate::raster::rasterize_rgba;
use crate::report::KilnReport;
use crate::writer::{common_warnings, Format, FormatWriter};

pub struct JpgWriter;

impl FormatWriter for JpgWriter {
    fn format(&self) -> Format {
        Format::Jpg
    }

    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>) -> KilnResult<KilnReport> {
        let t = Instant::now();
        let warnings = common_warnings(ctx, Format::Jpg);
        let rgba = rasterize_rgba(&ctx.list, ctx.scale as f64)?;
        let img = if ctx.transparent { blend_white(&rgba) } else { rgba };
        let rgb: Vec<u8> = img.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        let mut jpg = Vec::with_capacity(rgb.len() / 2);
        let mut enc = JpegEncoder::new_with_quality(&mut jpg, ctx.jpeg_quality);
        enc.encode(&rgb, ctx.out_w, ctx.out_h, image::ExtendedColorType::Rgb8)
            .map_err(|e| crate::error::KilnError::Encode(format!("JPEG 编码失败:{e}")))?;
        out.extend_from_slice(&jpg);
        let mut r = crate::writer::report_with(warnings, t, jpg.len());
        r.frame_count = 1;
        Ok(r)
    }
}

/// RGBA → 白底混合。
fn blend_white(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as f32 / 255.0;
            [
                (p[0] as f32 * a + 255.0 * (1.0 - a)) as u8,
                (p[1] as f32 * a + 255.0 * (1.0 - a)) as u8,
                (p[2] as f32 * a + 255.0 * (1.0 - a)) as u8,
            ]
        })
        .collect()
}
