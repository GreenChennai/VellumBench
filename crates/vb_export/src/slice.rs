//! 切片导出(05-2 / 09-C,design/06 §3.14):按 `data-vb-slice` 切片节点
//! 单独出图 —— 原生 PNG 引擎渲染整画板后,按切片几何(画板本地)裁剪。
//!
//! 切片建模:`NodeKind::Slice` + `attrs["data-vb-slice"] = 切片名`;
//! vellum-cli `export --slice <名>`(单个)/ `--slices`(全部)经此模块出图。

use std::path::Path;

use vb_doc::model::{Document, NodeId, NodeKind};

/// 收集画板下的全部切片:(节点 id, 切片名, [x, y, w, h] 画板本地)。
/// 切片名以 `data-vb-slice` 属性为准(缺省回退节点名)。
pub fn slices_of(doc: &Document, artboard: NodeId) -> Vec<(NodeId, String, [f64; 4])> {
    let mut out = Vec::new();
    let Some(ab) = doc.nodes.get(artboard) else {
        return out;
    };
    for &c in &ab.children {
        let Some(n) = doc.nodes.get(c) else {
            continue;
        };
        if !matches!(n.kind, NodeKind::Slice) {
            continue;
        }
        let name = n
            .attrs
            .get("data-vb-slice")
            .cloned()
            .unwrap_or_else(|| n.name.clone());
        out.push((c, name, [n.geom.x, n.geom.y, n.geom.w, n.geom.h]));
    }
    out
}

/// 按名解析切片(大小写不敏感;同时匹配 `data-vb-slice` 属性与节点名 ——
/// 图层面板改名只动节点名,属性值保持创建时的名字,两者都应可命中)。
pub fn resolve_slice(
    doc: &Document,
    artboard: NodeId,
    key: &str,
) -> Option<(NodeId, String, [f64; 4])> {
    let all = slices_of(doc, artboard);
    all.into_iter().find(|(id, attr_name, _)| {
        attr_name.eq_ignore_ascii_case(key)
            || doc
                .nodes
                .get(*id)
                .map(|n| n.name.eq_ignore_ascii_case(key))
                .unwrap_or(false)
    })
}

/// 渲染单个切片为 PNG:整画板原生渲染(与 `export_artboard_png` 同链路)
/// 后按切片几何裁剪。切片区域出图与整板出图**同一编码路径**,保证像素一致。
pub fn export_slice_png(
    doc: &Document,
    artboard: NodeId,
    slice_geom: [f64; 4],
    scale: f32,
    transparent: bool,
    project_dir: Option<&Path>,
) -> Result<(Vec<u8>, Vec<String>), String> {
    let (full, warnings) =
        crate::export_artboard_png(doc, artboard, scale, transparent, project_dir)?;
    let img = image::load_from_memory(&full).map_err(|e| format!("解码画板 PNG 失败:{e}"))?;
    let s = scale as f64;
    // 画板本地几何 → 像素裁剪矩形(四舍五入并夹回画板内)
    let (iw, ih) = (img.width() as f64, img.height() as f64);
    let x0 = (slice_geom[0] * s).round().clamp(0.0, iw) as u32;
    let y0 = (slice_geom[1] * s).round().clamp(0.0, ih) as u32;
    let x1 = ((slice_geom[0] + slice_geom[2]) * s).round().clamp(0.0, iw) as u32;
    let y1 = ((slice_geom[1] + slice_geom[3]) * s).round().clamp(0.0, ih) as u32;
    if x1 <= x0 || y1 <= y0 {
        return Err(format!(
            "切片区域为空({x0},{y0})-({x1},{y1});画板 {iw}×{ih}px"
        ));
    }
    let cropped = img.crop_imm(x0, y0, x1 - x0, y1 - y0);
    let mut png = Vec::new();
    cropped
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| format!("PNG 编码失败:{e}"))?;
    Ok((png, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    use vb_doc::model::{Geom, Node};

    fn slice_node(doc: &mut Document, parent: NodeId, name: &str, g: Geom) -> NodeId {
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Slice, name, sid.clone());
        n.geom = g;
        n.attrs.insert("data-vb-slice".into(), name.to_string());
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        id
    }

    fn box_node(doc: &mut Document, parent: NodeId, g: Geom, color: &str) -> NodeId {
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, "盒", sid.clone());
        n.geom = g;
        n.style.push(vb_css::Decl {
            prop: "background-color".into(),
            value: color.into(),
            important: false,
        });
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        id
    }

    /// 切片收集与按名解析(属性优先、大小写不敏感)。
    #[test]
    fn slices_are_collected_and_resolved() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        slice_node(
            &mut doc,
            ab,
            "Hero 切片",
            Geom {
                x: 10.0,
                y: 20.0,
                w: 300.0,
                h: 200.0,
            },
        );
        assert_eq!(slices_of(&doc, ab).len(), 1);
        let (_, name, g) = resolve_slice(&doc, ab, "hero 切片").expect("按名可解析");
        assert_eq!(name, "Hero 切片");
        assert_eq!(g, [10.0, 20.0, 300.0, 200.0]);
        assert!(resolve_slice(&doc, ab, "不存在").is_none());
    }

    /// 切片出图 = 整板渲染后裁剪:产物尺寸 = 切片几何 × scale,
    /// 且切片区域的像素与整板同区域逐像素一致(同一编码路径的裁剪)。
    #[test]
    fn slice_png_crops_artboard_render() {
        let mut doc = Document::new("t", "zh-CN");
        let ab = doc.artboards[0];
        // 红块 (0,0,400,300) + 蓝块 (400,300,400,300);切片盖住蓝块
        box_node(
            &mut doc,
            ab,
            Geom {
                x: 0.0,
                y: 0.0,
                w: 400.0,
                h: 300.0,
            },
            "#ff0000",
        );
        box_node(
            &mut doc,
            ab,
            Geom {
                x: 400.0,
                y: 300.0,
                w: 400.0,
                h: 300.0,
            },
            "#0000ff",
        );
        slice_node(
            &mut doc,
            ab,
            "蓝块切片",
            Geom {
                x: 400.0,
                y: 300.0,
                w: 400.0,
                h: 300.0,
            },
        );
        let (_, _name, geom) = resolve_slice(&doc, ab, "蓝块切片").expect("切片可解析");
        let (png, _) = export_slice_png(&doc, ab, geom, 1.0, false, None).expect("切片出图成功");
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!(
            (img.width(), img.height()),
            (400, 300),
            "产物 = 切片几何 × scale"
        );
        // 裁剪区域像素应为蓝色(蓝块左上角)
        let px = img.get_pixel(10, 10);
        assert!(px[2] > 200 && px[0] < 50, "切片区域应为蓝块像素:{px:?}");
        // 全板出图对照:同区域像素一致
        let (full, _) = crate::export_artboard_png(&doc, ab, 1.0, false, None).expect("全板出图");
        let full_img = image::load_from_memory(&full).unwrap();
        assert_eq!(
            img.get_pixel(10, 10),
            full_img.get_pixel(410, 310),
            "切片像素 = 全板同区域像素"
        );
    }
}
