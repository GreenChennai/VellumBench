//! `vb_export` — 导出管线:命名模板、目标解析、原生/浏览器引擎调度(设计文档 07 篇)。
//!
//! 双引擎(ADR-0005/0016):
//! - 原生:CPU 光栅 PNG + 矢量 SVG(快、离线、无依赖)
//! - 浏览器(WPI 进程桥):PNG/GIF/MP4/PDF(像素真值;GIF/MP4 仅浏览器能捕获动画)

pub mod svg;
pub mod wpi;

use std::path::Path;

use vb_doc::model::Document;

/// 渲染单个画板为 PNG(原生 CPU 引擎)。
pub fn export_artboard_png(
    doc: &Document,
    artboard: vb_doc::model::NodeId,
    scale: f32,
    transparent: bool,
    project_dir: Option<&Path>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let list = vb_render::encode::encode_artboard(doc, artboard).map_err(|e| e.to_string())?;
    let res = vb_render::cpu::render_png(&list, scale, transparent, project_dir)?;
    Ok((res.png, res.warnings))
}

/// 渲染单个画板为 SVG(原生矢量;文本为真实 `<text>`)。
pub fn export_artboard_svg(
    doc: &Document,
    artboard: vb_doc::model::NodeId,
    scale: u32,
) -> Result<String, String> {
    let list = vb_render::encode::encode_artboard(doc, artboard).map_err(|e| e.to_string())?;
    Ok(svg::render_svg(&list, scale))
}

/// 命名模板展开(设计文档 07 篇 §六):`{doc} {artboard} {scale} {ext} {index} {width} {height}`。
#[allow(clippy::too_many_arguments)]
pub fn expand_name_template(
    template: &str,
    doc_name: &str,
    artboard_name: &str,
    scale: u32,
    ext: &str,
    index: usize,
    width: u32,
    height: u32,
) -> String {
    let safe = |s: &str| s.replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|', ' '], "-");
    template
        .replace("{doc}", &safe(doc_name))
        .replace("{artboard}", &safe(artboard_name))
        .replace("{scale}", &scale.to_string())
        .replace("{ext}", ext)
        .replace("{index}", &format!("{index:02}"))
        .replace("{width}", &width.to_string())
        .replace("{height}", &height.to_string())
}

/// 默认命名模板。
pub const DEFAULT_TEMPLATE: &str = "{artboard}@{scale}x.{ext}";
