# -*- coding: utf-8 -*-
# B3 收尾:SVG base64 嵌入 + GUI 缓存接线
SVG = r'crates\vb_export\src\svg.rs'
APP = r'crates\vb_app\src\app.rs'
MAIN = r'crates\vb_agent\src\main.rs'
MCP = r'crates\vb_agent\src\bin\vellum-mcp.rs'
EXP = r'crates\vb_export\src\lib.rs'
DEPS = r'docs\deps.md'

def rep(path, old, new, tag, count=1):
    s = open(path, encoding='utf-8').read()
    assert old in s, "anchor missing: " + tag
    open(path, 'w', encoding='utf-8', newline='').write(s.replace(old, new, count))

# ── svg.rs:位图 base64 嵌入;未挂载回退红框 ──
rep(SVG, """        DrawKind::Image => {
            let _ = write!(
                out,
                r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none" stroke="rgb(230,77,77)" stroke-width="2"{tf}/><!-- image: {:?} -->"#,
                item.src
            );
            out.push('\\n');
        }""",
"""        DrawKind::Image => {
            if let Some(bmp) = &item.image {
                // 位图以 data URL 嵌入(B3):导出的独立 SVG 不再丢图;
                // PNG 载荷由原始 RGBA 现场编码(导出一次,不逐帧)
                if let Some(href) = bitmap_data_url(bmp) {
                    let _ = write!(
                        out,
                        r#"<image x="{x}" y="{y}" width="{w}" height="{h}" opacity="{}" preserveAspectRatio="none" href="{href}"{tf}/><!-- image: {:?} -->"#,
                        item.opacity,
                        item.src
                    );
                    out.push('\\n');
                    return;
                }
            }
            let _ = write!(
                out,
                r#"<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none" stroke="rgb(230,77,77)" stroke-width="2"{tf}/><!-- image missing: {:?} -->"#,
                item.src
            );
            out.push('\\n');
        }""", "svg-image")

# bitmap_data_url 助手
rep(SVG, """/// 四角异径圆角矩形 → SVG path d 串""",
"""/// RGBA 位图 → `data:image/png;base64,…`(导出一次性成本)。
fn bitmap_data_url(bmp: &vb_render::encode::BitmapData) -> Option<String> {
    use std::io::Write as _;
    let img = image::RgbaImage::from_raw(bmp.width, bmp.height, (*bmp.rgba).clone())?;
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png.get_ref());
    Some(format!("data:image/png;base64,{b64}"))
}

/// 四角异径圆角矩形 → SVG path d 串""", "data-url")

# ── lib.rs:导出函数接 loader(透传给 attach) ──
rep(EXP, """/// 渲染单个画板为 PNG(原生 CPU 引擎)。
pub fn export_artboard_png(
    doc: &Document,
    artboard: vb_doc::model::NodeId,
    scale: f32,
    transparent: bool,
    project_dir: Option<&Path>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let list =
        vb_render::encode::encode_artboard_opts(doc, artboard, transparent).map_err(|e| e.to_string())?;
    let res = vb_render::cpu::render_png(&list, scale, transparent, project_dir)?;
    Ok((res.png, res.warnings))
}

/// 渲染单个画板为 SVG(原生矢量;文本为真实 `<text>`)。
pub fn export_artboard_svg(
    doc: &Document,
    artboard: vb_doc::model::NodeId,
    scale: u32,
    transparent: bool,
) -> Result<String, String> {
    let list =
        vb_render::encode::encode_artboard_opts(doc, artboard, transparent).map_err(|e| e.to_string())?;
    Ok(svg::render_svg(&list, scale, transparent))
}""",
"""/// 渲染单个画板为 PNG(原生 CPU 引擎)。
/// `image_loader` 缺省时按 project_dir 现场解码。
pub fn export_artboard_png(
    doc: &Document,
    artboard: vb_doc::model::NodeId,
    scale: f32,
    transparent: bool,
    project_dir: Option<&Path>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut list =
        vb_render::encode::encode_artboard_opts(doc, artboard, transparent).map_err(|e| e.to_string())?;
    if let Some(dir) = project_dir {
        vb_render::encode::attach_images(&mut list, &|src| {
            load_bitmap_from(dir, src)
        });
    }
    let res = vb_render::cpu::render_png(&list, scale, transparent, project_dir)?;
    Ok((res.png, res.warnings))
}

/// 渲染单个画板为 SVG(原生矢量;文本为真实 `<text>`;位图 base64 嵌入)。
pub fn export_artboard_svg(
    doc: &Document,
    artboard: vb_doc::model::NodeId,
    scale: u32,
    transparent: bool,
    project_dir: Option<&Path>,
) -> Result<String, String> {
    let mut list =
        vb_render::encode::encode_artboard_opts(doc, artboard, transparent).map_err(|e| e.to_string())?;
    if let Some(dir) = project_dir {
        vb_render::encode::attach_images(&mut list, &|src| {
            load_bitmap_from(dir, src)
        });
    }
    Ok(svg::render_svg(&list, scale, transparent))
}

/// 从项目目录解码一张位图(无缓存;批量导出由调用方持有缓存更优)。
fn load_bitmap_from(
    dir: &Path,
    src: &str,
) -> Option<vb_render::encode::BitmapData> {
    let p = dir.join(src);
    let img = image::open(&p).ok()?;
    let rgba = img.to_rgba8();
    Some(vb_render::encode::BitmapData {
        width: rgba.width(),
        height: rgba.height(),
        rgba: std::sync::Arc::new(rgba.into_raw()),
    })
}""", "export-loaders")

print("B3 SVG/EXPORT PATCHED")
