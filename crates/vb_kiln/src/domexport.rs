//! DOM 快照导出编排(ADR-0021):浏览器布局 → Kiln 矢量写入。
//!
//! 流程:静态服务挂载 → 浏览器 settle → domsnap(PaintList)+ 整页截图 →
//! @font-face 注册 → paintlist_to_document → export_artboard(AI/PDF)。
//! 文本按行建节点:写入器一行一个 CID Tj(零逐字断字)。

use std::path::{Path, PathBuf};

use crate::abprobe::viewport_plan;
use crate::dompaint::{paintlist_to_document, DomPaintMeta};
use crate::{ExportRequest, Format};

pub struct DomExportResult {
    pub bytes: Vec<u8>,
    pub width: f64,
    pub height: f64,
    pub meta: DomPaintMeta,
    pub warnings: Vec<String>,
    pub browser: String,
}

/// 单源导出(AI 默认走此路线;PDF 可选)。
pub fn export_dom(
    source: &Path,
    format: Format,
    transparent: bool,
    width: u32,
    scale: u32,
    height: u32,
) -> Result<DomExportResult, String> {
    let (mount_dir, html_path) = resolve_source(source)?;
    let srv = vb_browser::staticsrv::StaticServer::start(&mount_dir)?;
    let url = if source.is_dir() {
        srv.url_for_dir()?
    } else {
        let name = html_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("源文件名非法")?;
        format!("http://127.0.0.1:{}/{}", srv.port(), url_encode(name))
    };
    let prefix = format!("http://127.0.0.1:{}/", srv.port());

    let exe = vb_browser::discover_browser(None)
        .ok_or("未发现系统浏览器(Edge/Chrome);DOM 快照路线不可用")?;
    let proc = vb_browser::browser::BrowserProcess::launch(&exe)?;
    let browser_ver = proc.version();
    let mut page = vb_browser::page::PageSession::attach(&proc)?;
    // 采集视口(P0-3 尺寸门):显式 --width/--height 优先,否则按画板声明
    // 尺寸取景(abprobe,与导入器同一识别口径),再否则历史兜底 1080。
    // 此前视口宽固定 1080:1920 宽内容被裁掉 44%(PDF 页只剩 1080×1080)。
    let (vw, vh, artboard) = viewport_plan(source, width, height);
    // 视口高 = --height 锁定值(动画卡 100vh 型必须锁,否则 100vh 撑成视口宽)。
    // 采集 DSF:位图降级裁剪的分辨率上限;超长页(易拉宝 11812px)按 1 兜底,
    // 否则整页截图会超过浏览器单帧上限。矢量与图片项不受影响(后者原生分辨率)。
    let mut dsf = scale.clamp(1, 8);
    if (vh as u64 * dsf as u64) > 15_000 || (vw as u64 * dsf as u64) > 15_000 {
        dsf = 1;
    }
    page.set_device_metrics(vw, vh, dsf)?;
    page.navigate(&url)?;
    if artboard {
        // 画板即画布(P0-3):页边距属页面 chrome,重置后与画板声明几何对齐
        // (与 native 车道同语义;缺显式标记仍以 degraded_artboard 诚实标注)
        let _ = page.evaluate(vb_browser::page::BODY_MARGIN_RESET_JS, false);
        page.sleep(120);
    }
    page.wait_network_idle(std::time::Duration::from_secs(3));
    page.sleep(200);
    let cap = vb_browser::capture::capture_dom(&mut page, &url)?;
    page.close();
    drop(proc);

    // 视口宽对齐内容:重设一次视口为内容宽,让 fixed 元素矩形与内容同帧
    let mut warnings = cap.warnings.clone();

    // @font-face 注册(family 名与采集一致,写入器才能 CID 嵌入)
    vb_render::text::clear_font_registry();
    for (family, weight, path) in collect_font_faces(&html_path, &mount_dir) {
        vb_render::text::register_font_file(&family, weight, path);
    }

    if std::env::var("KILN_DUMP_PAINTLIST").is_ok() {
        // 调试转储落系统临时目录(K4:不写源工程目录)
        let dump = std::env::temp_dir().join(format!(
            "kiln-paintlist-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|t| t.as_nanos())
                .unwrap_or(0)
        ));
        eprintln!("{{\"domwarn\":\"paintlist dump: {}\"}}", dump.display());
        let _ = std::fs::write(
            dump,
            serde_json::to_string_pretty(&cap.list).unwrap_or_default(),
        );
    }
    let dom = paintlist_to_document(
        &cap.list,
        &mount_dir,
        Some(&cap.page_png),
        &prefix,
        dsf as f64,
        vh as f64,
        // 画板取景时声明宽是唯一真相(透明容器不产生绘制项,启发式会低估)
        if artboard { vw as f64 } else { 0.0 },
    )?;
    if std::env::var("KILN_DUMP_DRAWLIST").is_ok() {
        if let Ok(list) = vb_render::encode::encode_artboard_opts(&dom.doc, dom.artboard, false) {
            for (i, it) in list.items.iter().enumerate() {
                let fill = match &it.fill {
                    Some(vb_render::encode::FillDef::Solid(c)) => {
                        format!("solid({:.2},{:.2},{:.2},{:.2})", c[0], c[1], c[2], c[3])
                    }
                    Some(vb_render::encode::FillDef::LinearGradient { angle_css, stops }) => {
                        format!("linear({angle_css}) stops={}", stops.len())
                    }
                    Some(vb_render::encode::FillDef::RadialGradient { .. }) => "radial".into(),
                    None => "none".into(),
                };
                eprintln!("[draw{i}] {:?} {:?} {}", it.kind, it.rect, fill);
            }
        }
    }
    let (w, h) = (
        dom.doc
            .nodes
            .get(dom.artboard)
            .map(|n| n.geom.w)
            .unwrap_or(0.0),
        dom.doc
            .nodes
            .get(dom.artboard)
            .map(|n| n.geom.h)
            .unwrap_or(0.0),
    );
    let req = ExportRequest {
        format,
        // 几何是 CSS px:光栅按 --scale 放大(与车道 B 的像素口径一致),
        // 矢量写入器只用 logical_w/h,不受影响
        scale: scale.clamp(1, 8),
        transparent,
        ..Default::default()
    };
    let export_res = crate::export_artboard(&dom.doc, dom.artboard, &req, Some(&mount_dir))
        .map_err(|e| format!("DOM 快照导出失败: {e}"));
    cleanup_intermediate(&dom.raster_dir);
    let (bytes, report) = export_res?;
    warnings.extend(report.warnings.iter().map(|w| w.message()));
    warnings.extend(dom.meta.warnings.iter().cloned());
    Ok(DomExportResult {
        bytes,
        width: w,
        height: h,
        meta: dom.meta,
        warnings,
        browser: browser_ver,
    })
}

/// 副产物清理(K4):导出结束即删临时目录;`KILN_KEEP_INTERMEDIATE` 显式保留。
fn cleanup_intermediate(dir: &Option<std::path::PathBuf>) {
    if std::env::var("KILN_KEEP_INTERMEDIATE").is_ok() {
        return;
    }
    if let Some(d) = dir {
        let _ = std::fs::remove_dir_all(d);
    }
}

pub fn resolve_source(source: &Path) -> Result<(PathBuf, PathBuf), String> {
    if source.is_dir() {
        for idx in ["index.html", "index.htm"] {
            let p = source.join(idx);
            if p.is_file() {
                return Ok((source.to_path_buf(), p));
            }
        }
        let mut htmls: Vec<PathBuf> = std::fs::read_dir(source)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
                    .unwrap_or(false)
            })
            .collect();
        htmls.sort();
        htmls
            .pop()
            .map(|p| (source.to_path_buf(), p))
            .ok_or_else(|| format!("目录中无 HTML: {}", source.display()))
    } else {
        let parent = source
            .parent()
            .filter(|p| p.is_dir())
            .ok_or("源文件无父目录")?
            .to_path_buf();
        Ok((parent, source.to_path_buf()))
    }
}

/// 字体回退搜索目录:环境变量 VB_FONT_FALLBACK_DIRS(分号分隔)+ 内置常见位。
/// 场景:@font-face 只带 woff2 而 Rust 解析器只认 ttf/otf 时,按
/// 「同名 ttf → 同 stem ttf → weight 后缀名」在回退目录里找实体。
fn font_fallback_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var("VB_FONT_FALLBACK_DIRS")
        .unwrap_or_default()
        .split(';')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if let Ok(home) = std::env::var("USERPROFILE") {
        dirs.push(PathBuf::from(home).join(".agents/skills/artboard/fonts"));
    }
    dirs
}

fn weight_suffix(weight: u16) -> &'static str {
    match weight {
        100..=350 => "Light",
        351..=450 => "Regular",
        451..=550 => "Medium",
        551..=650 => "Semibold",
        651..=750 => "Bold",
        751..=850 => "Heavy",
        _ => "Black",
    }
}

/// 从 HTML(含内联 style)解析 @font-face:family/weight/src。
/// woff2/woff 回退链:同目录同名 ttf/otf → VB_FONT_FALLBACK_DIRS 全局搜
/// (同 stem → family 名 × weight 后缀,如 MiSans-Bold.ttf)。
fn collect_font_faces(html: &Path, base: &Path) -> Vec<(String, u16, PathBuf)> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string(html) else {
        return out;
    };
    let mut i = 0usize;
    while let Some(pos) = text[i..].find("@font-face") {
        let start = i + pos;
        let end = text[start..]
            .find('}')
            .map(|e| start + e)
            .unwrap_or(text.len());
        let block = &text[start..end.min(text.len())];
        let family = extract_quoted_after(block, "font-family")
            .or_else(|| extract_unquoted_after(block, "font-family"));
        let weight = extract_unquoted_after(block, "font-weight")
            .and_then(|w| w.trim().parse::<u16>().ok())
            .unwrap_or(400);
        let src =
            extract_quoted_after(block, "src").or_else(|| extract_unquoted_after(block, "src"));
        if let (Some(family), Some(src)) = (family, src) {
            let rel = src.split('?').next().unwrap_or(&src);
            let rel = rel.trim_start_matches("./");
            let path = base.join(rel);
            let resolved = if path.is_file() {
                Some(path)
            } else if matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("woff2") | Some("woff")
            ) {
                ["ttf", "otf"]
                    .iter()
                    .map(|e| path.with_extension(e))
                    .find(|p| p.is_file())
                    .or_else(|| global_font_lookup(&family, weight))
            } else {
                None
            };
            if let Some(p) = resolved {
                out.push((family, weight, p));
            }
        }
        i = end;
    }
    out
}

/// 回退目录里按 family×weight 找实体字体(文件名模式 Family-Suffix.ttf)。
fn global_font_lookup(family: &str, weight: u16) -> Option<PathBuf> {
    let fam = family.trim().trim_matches('"').trim_matches('\'');
    let suffixes = [weight_suffix(weight), "Regular", "Bold"];
    for dir in font_fallback_dirs() {
        for sub in [
            "",
            "misans",
            "source-han-sans",
            "source-han-serif",
            "alibaba-puhuiti",
        ] {
            let root = if sub.is_empty() {
                dir.clone()
            } else {
                dir.join(sub)
            };
            if !root.is_dir() {
                continue;
            }
            for suffix in suffixes {
                let cand = root.join(format!("{fam}-{suffix}.ttf"));
                if cand.is_file() {
                    return Some(cand);
                }
                // 大小写不敏感兜底
                if let Ok(entries) = std::fs::read_dir(&root) {
                    for e in entries.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.eq_ignore_ascii_case(&format!("{fam}-{suffix}.ttf")) {
                            return Some(e.path());
                        }
                    }
                }
            }
        }
    }
    None
}

fn extract_quoted_after(block: &str, prop: &str) -> Option<String> {
    let idx = block.find(prop)?;
    let rest = &block[idx + prop.len()..];
    let rest = rest.trim_start_matches(|c: char| c == ':' || c.is_whitespace());
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let inner = &rest[1..];
    let end = inner.find(quote)?;
    Some(inner[..end].to_string())
}

fn extract_unquoted_after(block: &str, prop: &str) -> Option<String> {
    let idx = block.find(prop)?;
    let rest =
        block[idx + prop.len()..].trim_start_matches(|c: char| c == ':' || c.is_whitespace());
    let end = rest
        .find(|c: char| c == ';' || c == '}' || c.is_whitespace())
        .unwrap_or(rest.len());
    let v = rest[..end].trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 多源导出(N4 多画板):每源一页(浏览器采集 → 单页 PDF)→ 合并 →
/// AI 头在头部之内落笔。Illustrator 中 PDF 页 = 画板。
pub fn export_dom_pages(
    sources: &[PathBuf],
    transparent: bool,
    width: u32,
    scale: u32,
    height: u32,
) -> Result<DomExportResult, String> {
    if sources.is_empty() {
        return Err("多源导出至少需要一个 --source".into());
    }
    if sources.len() == 1 {
        return export_dom(&sources[0], Format::Ai, transparent, width, scale, height);
    }
    let mut page_pdfs: Vec<Vec<u8>> = Vec::new();
    let mut meta = DomPaintMeta::default();
    let mut warnings = Vec::new();
    let mut browser = String::new();
    let mut w0 = 0f64;
    let mut h0 = 0f64;
    for src in sources {
        // 逐源走单页导出(浏览器会话各自起落;效率列 carry-forward)
        let r = export_dom_pdf_bytes(src, transparent, width, height)?;
        meta.line_count += r.0;
        meta.clip_demand += r.1;
        meta.raster_count += r.2;
        warnings.extend(r.3);
        browser = r.4;
        w0 = w0.max(r.5);
        h0 = h0.max(r.6);
        page_pdfs.push(r.7);
    }
    let bytes = crate::pdf::merge_pdf_pages_head(&page_pdfs, crate::pdf::AI_HEAD)
        .map_err(|e| format!("多页合并失败: {e}"))?;
    Ok(DomExportResult {
        bytes,
        width: w0,
        height: h0,
        meta,
        warnings,
        browser,
    })
}

/// 单源 → (行数, clip, raster, 警告, 浏览器, w, h, PDF 字节)。
/// 单源 → (行数, clip, raster, 警告, 浏览器, w, h, PDF 字节)。
#[allow(clippy::type_complexity)] // 8 元组为内部管道中间态,出口处即拆解
fn export_dom_pdf_bytes(
    src: &Path,
    transparent: bool,
    width: u32,
    height: u32,
) -> Result<(u32, u32, u32, Vec<String>, String, f64, f64, Vec<u8>), String> {
    let (mount_dir, html_path) = resolve_source(src)?;
    let srv = vb_browser::staticsrv::StaticServer::start(&mount_dir)?;
    let url = if src.is_dir() {
        srv.url_for_dir()?
    } else {
        let name = html_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("源文件名非法")?;
        format!("http://127.0.0.1:{}/{}", srv.port(), url_encode(name))
    };
    let prefix = format!("http://127.0.0.1:{}/", srv.port());
    let exe = vb_browser::discover_browser(None)
        .ok_or("未发现系统浏览器(Edge/Chrome);DOM 快照路线不可用")?;
    let proc = vb_browser::browser::BrowserProcess::launch(&exe)?;
    let browser_ver = proc.version();
    let mut page = vb_browser::page::PageSession::attach(&proc)?;
    // P0-3:逐源取画板声明尺寸(多画板按各自尺寸),显式 --width/--height 优先
    let (vw, vh, artboard) = viewport_plan(src, width, height);
    page.set_device_metrics(vw, vh, 1)?;
    page.navigate(&url)?;
    if artboard {
        let _ = page.evaluate(vb_browser::page::BODY_MARGIN_RESET_JS, false);
        page.sleep(120);
    }
    page.wait_network_idle(std::time::Duration::from_secs(3));
    page.sleep(200);
    let cap = vb_browser::capture::capture_dom(&mut page, &url)?;
    page.close();
    drop(proc);
    let mut warnings = cap.warnings.clone();
    vb_render::text::clear_font_registry();
    for (family, weight, path) in collect_font_faces(&html_path, &mount_dir) {
        vb_render::text::register_font_file(&family, weight, path);
    }
    let dom = paintlist_to_document(
        &cap.list,
        &mount_dir,
        Some(&cap.page_png),
        &prefix,
        1.0,
        vh as f64,
        if artboard { vw as f64 } else { 0.0 },
    )?;
    let (w, h) = (
        dom.doc
            .nodes
            .get(dom.artboard)
            .map(|n| n.geom.w)
            .unwrap_or(0.0),
        dom.doc
            .nodes
            .get(dom.artboard)
            .map(|n| n.geom.h)
            .unwrap_or(0.0),
    );
    let req = ExportRequest {
        format: Format::Pdf,
        scale: 1,
        transparent,
        ..Default::default()
    };
    let export_res = crate::export_artboard(&dom.doc, dom.artboard, &req, Some(&mount_dir))
        .map_err(|e| format!("DOM 快照导出失败: {e}"));
    cleanup_intermediate(&dom.raster_dir);
    let (bytes, report) = export_res?;
    warnings.extend(report.warnings.iter().map(|w| w.message()));
    warnings.extend(dom.meta.warnings.iter().cloned());
    Ok((
        dom.meta.line_count,
        dom.meta.clip_demand,
        dom.meta.raster_count,
        warnings,
        browser_ver,
        w,
        h,
        bytes,
    ))
}

// AI 识别注释现由写入器在头部之内落笔(`pdf::AI_HEAD`):事后插入会让
// 全表 xref 偏移整体错位(见 `pdf::write_pdf_head` 注释)。
