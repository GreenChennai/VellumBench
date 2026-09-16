//! 外部 PDF/AI 导入(M4,Q11=A):pdfium 提取 → 场景图 → 规范化 HTML。
//!
//! 设计:pdfium.dll 动态绑定(缺席时显式报错,Kiln 其余功能不受影响);
//! 页 → 画板(pt→px = ×4/3,96dpi);文本对象 → 文本节点(按对象边界
//! 定位,颜色/字号取自对象);路径对象 → 盒节点(填充色);图像对象
//! → PNG 落盘 + 图像节点。.ai 按 artboard ADR-0008(PDF 兼容流)同路。
//!
//! 已知边界(v1):文本按对象粒度(不重建行内混排);路径以盒近似;
//! Form/XObject 不递归。

use std::path::{Path, PathBuf};

use vb_doc::model::NodeId;
use vb_doc::model::{Document, Geom, Node, NodeKind, TextMode};

use pdfium_render::prelude::*;

/// 定位 pdfium.dll:环境变量 PDFIUM_DLL → exe 目录 → 常见工具目录。
fn find_pdfium_dll() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PDFIUM_DLL") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("pdfium.dll");
            if p.exists() {
                return Some(p);
            }
        }
    }
    for p in [
        "E:\\Tools\\pdfium\\pdfium.dll",
        "C:\\Tools\\pdfium\\pdfium.dll",
    ] {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn bind() -> Result<Pdfium, String> {
    let dll = find_pdfium_dll().ok_or_else(|| {
        "PDF 导入需要 pdfium.dll(未找到):设置 PDFIUM_DLL 环境变量,或将 pdfium.dll 放到 exe 同目录 / E:\\Tools\\pdfium\\".to_string()
    })?;
    let bindings = Pdfium::bind_to_library(dll)
        .or_else(|_| Pdfium::bind_to_system_library())
        .map_err(|e| format!("pdfium.dll 绑定失败:{e}"))?;
    Ok(Pdfium::new(bindings))
}

const PT_TO_PX: f64 = 4.0 / 3.0; // 72dpi → 96dpi

fn attach(doc: &mut Document, ab: NodeId, mut n: Node) -> NodeId {
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(ab).unwrap().children.push(id);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    id
}

/// 导入 PDF/AI → Document(每页一个画板,页序即画板序)。
/// `assets_dir`:图像落盘目录(None = 不落盘,图像跳过)。
pub fn import_pdf_to_doc(
    pdf_path: &Path,
    assets_dir: Option<&Path>,
) -> Result<(Document, Vec<String>), String> {
    let pdfium = bind()?;
    let doc = pdfium
        .load_pdf_from_file(pdf_path.to_str().ok_or("路径非 UTF-8")?, None)
        .map_err(|e| format!("PDF 打开失败:{e}"))?;
    let mut warnings = Vec::new();
    let mut document = Document::new_empty("", "zh-CN");

    let page_count = doc.pages().len().min(20);
    for page_idx in 0..page_count {
        let page = doc
            .pages()
            .get(page_idx)
            .map_err(|e| format!("页 {page_idx} 读取失败:{e}"))?;
        let pw = page.width().value as f64 * PT_TO_PX;
        let ph = page.height().value as f64 * PT_TO_PX;
        let ab = document.new_artboard(&format!("第 {} 页", page_idx + 1), pw, ph);

        for obj in page.objects().iter() {
            let Ok(bounds) = obj.bounds() else { continue };
            // PDF 坐标:左下原点 → 画布坐标(左上原点)
            let x = bounds.left().value as f64 * PT_TO_PX;
            let w = (bounds.right().value - bounds.left().value) as f64 * PT_TO_PX;
            let y = ph - bounds.top().value as f64 * PT_TO_PX;
            let h = (bounds.top().value - bounds.bottom().value) as f64 * PT_TO_PX;
            // v1:pdfium-render 0.9 未暴露对象取色 API,文本统一深墨、
            // 路径统一浅灰(记录于流程表 M4 已知边界)
            let hex = match obj.object_type() {
                PdfPageObjectType::Path => "#e8e8e4".to_string(),
                _ => "#1a1a1a".to_string(),
            };

            match obj.object_type() {
                PdfPageObjectType::Text => {
                    let Some(text_obj) = obj.as_text_object() else {
                        continue;
                    };
                    let text = text_obj.text();
                    if text.trim().is_empty() || w < 1.0 {
                        continue;
                    }
                    let fs = (h * 0.78).max(6.0);
                    let sid = document.alloc_sid();
                    let mut n = Node::new(
                        NodeKind::Text {
                            text,
                            mode: TextMode::Point,
                            segments: Vec::new(),
                        },
                        "文本",
                        sid,
                    );
                    n.tag = "p".to_string();
                    n.geom = Geom {
                        x,
                        y,
                        w: w.max(10.0),
                        h: h.max(fs),
                    };
                    n.authored = [true, true, true, true];
                    n.style_set("font-size", &format!("{}px", fs.round()));
                    n.style_set("color", &hex);
                    n.style_set("white-space", "nowrap");
                    attach(&mut document, ab, n);
                }
                PdfPageObjectType::Image => {
                    let Some(dir) = assets_dir else { continue };
                    let Some(img_obj) = obj.as_image_object() else {
                        continue;
                    };
                    let _ = std::fs::create_dir_all(dir);
                    let file_name = format!("img-p{}-{:.0}-{:.0}.png", page_idx + 1, x, y);
                    let out_path = dir.join(&file_name);
                    let Ok(img) = img_obj.get_raw_image() else {
                        warnings.push("图像对象提取失败(已跳过)".into());
                        continue;
                    };
                    if img.save(&out_path).is_err() {
                        warnings.push("图像落盘失败(已跳过)".into());
                        continue;
                    }
                    let sid = document.alloc_sid();
                    let mut n = Node::new(NodeKind::Image { src: file_name }, "图像", sid);
                    n.geom = Geom {
                        x,
                        y,
                        w: w.max(4.0),
                        h: h.max(4.0),
                    };
                    n.authored = [true, true, true, true];
                    attach(&mut document, ab, n);
                }
                PdfPageObjectType::Path => {
                    let Some(path_obj) = obj.as_path_object() else {
                        continue;
                    };
                    let filled =
                        matches!(path_obj.fill_mode(), Ok(f) if f != PdfPathFillMode::None);
                    if !filled || w < 1.0 || h < 1.0 {
                        continue;
                    }
                    let sid = document.alloc_sid();
                    let mut n = Node::new(NodeKind::Box, "矩形", sid);
                    n.geom = Geom {
                        x,
                        y,
                        w: w.max(1.0),
                        h: h.max(1.0),
                    };
                    n.authored = [true, true, true, true];
                    n.style_set("background-color", &hex);
                    attach(&mut document, ab, n);
                }
                _ => {}
            }
        }
    }
    if document.artboards.is_empty() {
        return Err("PDF 无页面".into());
    }
    Ok((document, warnings))
}
