//! `vb_kiln` — Kiln 窑:VellumBench 下一代导出核心。
//!
//! 命名:Vellum(羔皮纸)入窑,烧出九种成品 —— PNG/JPG/GIF/MP4(媒体)
//! 与 SVG/PDF/EPS/Ai/PPTX(可编辑矢量)。全面取代 WPI 浏览器导出路径;
//! WPI 保留为回退(vb_export::wpi),默认不启用。
//!
//! 架构(.cluster 设计文档):
//! - `context`:ExportContext = DrawList + 光栅帧 + 动画参数 + 画板元数据
//! - `FormatWriter` trait:九个格式写入器统一接口(`writer::writer_for`)
//! - `raster`/`frames`:光栅与动画层(PNG/JPG/GIF/MP4 用)
//! - `pdf`/`postscript`/`ooxml`:自研矢量容器(PDF/EPS/Ai 与 PPTX)
//! - `error`/`report`:全分类错误 + 可观测报告,异常输入不 panic

pub mod abprobe;
pub mod anim;
pub mod animlane;
pub mod context;
pub mod domexport;
pub mod dompaint;
pub mod error;
pub mod formats;
pub mod frames;
pub mod img;
pub mod import_pdf;
pub mod import_svg;
pub mod ooxml;
pub mod pdf;
pub mod postscript;
pub mod raster;
pub mod report;
pub mod writer;

pub use context::ExportContext;
pub use error::{KilnError, KilnResult, KilnWarning};
pub use report::KilnReport;
pub use writer::{Format, FormatWriter};

use std::path::Path;

use vb_doc::model::{Document, NodeId};

/// 画布单边上限(像素)。超过报 CanvasTooLarge,防 OOM。
pub const MAX_CANVAS_EDGE: u32 = 32_768;
/// 画布面积上限(约 1GB RGBA)。
pub const MAX_CANVAS_PIXELS: u64 = 268_435_456;

/// Kiln 统一导出入口:按格式分发写入器,返回(字节, 报告)。
pub fn export_artboard(
    doc: &Document,
    artboard: NodeId,
    req: &ExportRequest,
    project_dir: Option<&Path>,
) -> KilnResult<(Vec<u8>, KilnReport)> {
    let ctx = ExportContext::build(doc, artboard, req, project_dir)?;
    let writer = writer::writer_for(req.format);
    let mut out = Vec::with_capacity(256 * 1024);
    let report = writer.write(&ctx, &mut out)?;
    Ok((out, report))
}

/// 便捷:导出并直接落盘。
pub fn export_artboard_to_file(
    doc: &Document,
    artboard: NodeId,
    req: &ExportRequest,
    project_dir: Option<&Path>,
    out_path: &Path,
) -> KilnResult<KilnReport> {
    let (bytes, report) = export_artboard(doc, artboard, req, project_dir)?;
    std::fs::write(out_path, &bytes).map_err(KilnError::Io)?;
    Ok(report)
}

/// 导出请求:格式 + 渲染参数(默认:PNG @2x 不透明)。
#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub format: Format,
    /// 倍率 1..=8(超出钳制 + 告警)。
    pub scale: u32,
    /// 透明背景(PNG/GIF/SVG/PDF 支持;JPG 垫白底并告警)。
    pub transparent: bool,
    /// JPG 质量 1..=100(默认 92)。
    pub jpeg_quality: u8,
    /// GIF/MP4 帧率 1..=60(默认 30)。
    pub fps: u32,
    /// GIF/MP4 时长秒 0.04..=3600(默认 2.0)。
    pub duration_s: f32,
    /// GIF 循环次数(0 = 无限,默认)。
    pub gif_loops: u16,
    /// MP4 码率 kbps(默认 8000;ffmpeg 桥)。
    pub mp4_bitrate_kbps: u32,
}

impl Default for ExportRequest {
    fn default() -> Self {
        ExportRequest {
            format: Format::Png,
            scale: 2,
            transparent: false,
            jpeg_quality: 92,
            fps: 30,
            duration_s: 2.0,
            gif_loops: 0,
            mp4_bitrate_kbps: 8000,
        }
    }
}
