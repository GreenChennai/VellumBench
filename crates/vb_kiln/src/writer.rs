//! FormatWriter trait 与九格式分发。

use vb_render::encode::{DrawItem, DrawList, FillDef};

use crate::context::ExportContext;
use crate::error::KilnWarning;
use crate::report::KilnReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Png,
    Jpg,
    Gif,
    Mp4,
    Svg,
    Pdf,
    Eps,
    Ai,
    Pptx,
}

impl Format {
    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
            Format::Gif => "gif",
            Format::Mp4 => "mp4",
            Format::Svg => "svg",
            Format::Pdf => "pdf",
            Format::Eps => "eps",
            Format::Ai => "ai",
            Format::Pptx => "pptx",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG 位图",
            Format::Jpg => "JPG 位图",
            Format::Gif => "GIF 动图",
            Format::Mp4 => "MP4 视频",
            Format::Svg => "SVG 矢量",
            Format::Pdf => "PDF 文档",
            Format::Eps => "EPS 印刷",
            Format::Ai => "Illustrator 兼容",
            Format::Pptx => "PowerPoint 演示",
        }
    }

    pub fn all() -> [Format; 9] {
        [
            Format::Png,
            Format::Jpg,
            Format::Gif,
            Format::Mp4,
            Format::Svg,
            Format::Pdf,
            Format::Eps,
            Format::Ai,
            Format::Pptx,
        ]
    }

    pub fn from_ext(ext: &str) -> Option<Format> {
        let e = ext.trim_start_matches('.').to_ascii_lowercase();
        match e.as_str() {
            "png" => Some(Format::Png),
            "jpg" | "jpeg" => Some(Format::Jpg),
            "gif" => Some(Format::Gif),
            "mp4" => Some(Format::Mp4),
            "svg" => Some(Format::Svg),
            "pdf" => Some(Format::Pdf),
            "eps" => Some(Format::Eps),
            "ai" => Some(Format::Ai),
            "pptx" => Some(Format::Pptx),
            _ => None,
        }
    }
}

/// 九格式统一写入接口。
pub trait FormatWriter: Send + Sync {
    fn format(&self) -> Format;
    /// 写入字节流;返回带 warnings 与耗时的报告。
    fn write(&self, ctx: &ExportContext, out: &mut Vec<u8>)
        -> crate::error::KilnResult<KilnReport>;
}

/// 按格式取写入器(writer 均无状态单例)。
pub fn writer_for(fmt: Format) -> &'static dyn FormatWriter {
    use crate::formats::*;
    match fmt {
        Format::Png => &png::PngWriter,
        Format::Jpg => &jpg::JpgWriter,
        Format::Gif => &gif::GifWriter,
        Format::Mp4 => &mp4::Mp4Writer,
        Format::Svg => &svg::SvgWriter,
        Format::Pdf => &pdf::PdfWriter,
        Format::Eps => &eps::EpsWriter,
        Format::Ai => &ai::AiWriter,
        Format::Pptx => &pptx::PptxWriter,
    }
}

/// 报告构造辅助。
pub fn report_with(
    mut warnings: Vec<KilnWarning>,
    started: std::time::Instant,
    bytes_written: usize,
) -> KilnReport {
    let mut r = KilnReport::new();
    warnings.extend(std::mem::take(&mut r.warnings));
    r.warnings = warnings;
    r.encode_ms = started.elapsed().as_millis() as u64;
    r.bytes = bytes_written;
    r
}

/// 公共告警:静态画布动画、JPG 透明强制。
pub fn common_warnings(ctx: &ExportContext, fmt: Format) -> Vec<KilnWarning> {
    let mut w = ctx.build_warnings.clone();
    if matches!(fmt, Format::Gif | Format::Mp4) && ctx.is_static() {
        w.push(KilnWarning::StaticCanvasAnimation);
    }
    if matches!(fmt, Format::Jpg) && ctx.requested_transparent {
        w.push(KilnWarning::JpgOpaqueForced);
    }
    w
}

/// DrawList → 图层名清单(ADR-0021:双图层「背景/内容」;更多层按序扩展)。
pub fn collect_layers(list: &DrawList) -> Vec<String> {
    let _ = list;
    // AI 交付固定两层：背景承载画板/底图，内容承载所有可编辑元素。
    // 额外的内部 layer 标记仍可用于绘制顺序，但不泄漏为 Illustrator 图层。
    vec!["背景".to_string(), "内容".to_string()]
}

/// 填充首色 → 十六进制(PPTX 用)。
pub fn fill_hex(f: &FillDef) -> String {
    let c = match f {
        FillDef::Solid(c) => c,
        FillDef::LinearGradient { stops, .. } => {
            &stops
                .first()
                .unwrap_or(&vb_render::GradientStop {
                    pos: 0.0,
                    color: [0.0, 0.0, 0.0, 1.0],
                })
                .color
        }
        FillDef::RadialGradient { stops, .. } => {
            &stops
                .first()
                .unwrap_or(&vb_render::GradientStop {
                    pos: 0.0,
                    color: [0.0, 0.0, 0.0, 1.0],
                })
                .color
        }
    };
    format!(
        "#{:02X}{:02X}{:02X}",
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8
    )
}

/// item 文本色。
pub fn item_text_color(item: &DrawItem) -> [f32; 4] {
    item.label
        .as_ref()
        .map(|t| t.color)
        .unwrap_or([0.0, 0.0, 0.0, 1.0])
}
