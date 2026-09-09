//! `vb_export` — 导出管线:命名模板、目标解析、原生/浏览器引擎调度。
//!
//! v0.1:原生 CPU 引擎(ADR-0016);WPI 浏览器引擎按 ADR-0005 排期 v0.5。

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

/// 命名模板展开(设计文档 07 篇 §六):`{doc} {artboard} {scale} {ext} {index} {width} {height}`。
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
