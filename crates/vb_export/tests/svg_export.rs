//! SVG 导出回归测试(15 号计划 A2:双重缩放 / 渐变 userSpaceOnUse / 透明导出)。

use std::sync::atomic::{AtomicU32, Ordering};

use vb_doc::import::import_project;

/// 并行测试的目录名去重(同二进制内 style.len() 相同的两个测试
/// 曾撞同一目录,全仓并行时偶发 flake)。
static SEQ: AtomicU32 = AtomicU32::new(0);

fn doc_with_node(style: &str) -> (vb_doc::Document, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "vb-svg-{}-{}-{}",
        style.len(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        format!(
            r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab1234" data-vb-name="AB" style="background-color:#ff0000">
    <div data-vb-id="n00001" style="position:absolute; left:100px; top:50px; width:200px; height:100px; {style}"></div>
  </section>
</body>
</html>
"#
        ),
    )
    .unwrap();
    (import_project(&dir).expect("导入").doc, dir)
}

/// W4:scale=2 不得产生 scale() transform(此前几何乘 s 又叠 scale(s),内容放大 s²)。
#[test]
fn svg_no_double_scale_at_2x() {
    let (doc, dir) = doc_with_node("background-color:#00ff00;");
    let ab = doc.artboards[0];
    let svg = vb_export::export_artboard_svg(&doc, ab, 2, false, None).expect("SVG 导出");
    assert!(svg.contains(r#"width="2880""#), "画板 1440@2x 应宽 2880");
    assert!(!svg.contains("scale("), "不得出现 scale() transform:{svg}");
    // 节点 x=100 → 缩放一次 = 200
    assert!(svg.contains(r#"x="200""#), "几何应只缩放一次:{svg}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// W5/W3:渐变必须 userSpaceOnUse + 节点偏移;渐变端要输出 fill-opacity。
#[test]
fn svg_gradient_userspace_and_opacity() {
    let (doc, dir) =
        doc_with_node("opacity: 0.5; background-image: linear-gradient(90deg, #ff0000, #0000ff);");
    let ab = doc.artboards[0];
    let svg = vb_export::export_artboard_svg(&doc, ab, 1, false, None).expect("SVG 导出");
    assert!(
        svg.contains(r#"gradientUnits="userSpaceOnUse""#),
        "缺 userSpaceOnUse(否则像素坐标被当比例读):{svg}"
    );
    // 节点在 x=100:渐变起点应带偏移,而不是从 0 开始
    assert!(svg.contains(r#"x1="100""#), "渐变起点缺节点偏移:{svg}");
    assert!(
        svg.contains("fill-opacity=\"0.5\""),
        "渐变端缺节点 opacity:{svg}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// W5:径向渐变半径用引擎公式 sqrt(w²+h²)/2,不再是硬编码 0.7。
#[test]
fn svg_radial_gradient_radius_matches_engine() {
    let (doc, dir) =
        doc_with_node("background-image: radial-gradient(circle at 50% 50%, #ff0000, #0000ff);");
    let ab = doc.artboards[0];
    let svg = vb_export::export_artboard_svg(&doc, ab, 1, false, None).expect("SVG 导出");
    // 200×100 节点:r = sqrt(200²+100²)/2 ≈ 111.8
    assert!(svg.contains("r=\"111.8"), "径向半径应≈111.8:{svg}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// W2:透明导出跳过画板底色矩形(画板带 #ff0000 背景)。
#[test]
fn svg_transparent_skips_background() {
    let (doc, dir) = doc_with_node("background-color:#00ff00;");
    let ab = doc.artboards[0];
    let opaque = vb_export::export_artboard_svg(&doc, ab, 1, false, None).expect("SVG 导出");
    assert!(
        opaque.contains(r#"fill="rgb(255,0,0)""#),
        "不透明导出应有底色矩形"
    );
    let clear = vb_export::export_artboard_svg(&doc, ab, 1, true, None).expect("SVG 导出");
    assert!(
        !clear.contains(r#"fill="rgb(255,0,0)""#),
        "透明导出不得铺底色:{clear}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// 15 号计划 B2:SVG 四角异径走 path;矢量描边色取 border
// ---------------------------------------------------------------------------

/// B2:四角异径圆角在 SVG 中发 <path>(rect 的 rx 单值表达不了)。
#[test]
fn svg_uneven_radii_emit_path() {
    let (doc, dir) = doc_with_node("background-color:#00ff00;border-radius:40px 12px 40px 12px;");
    let ab = doc.artboards[0];
    let svg = vb_export::export_artboard_svg(&doc, ab, 1, false, None).expect("SVG 导出");
    assert!(svg.contains("<path d=\"M"), "四角异径应发 path:{svg}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// B2:矢量路径描边取 border 色(此前硬编码 rgb(20,20,20))。
#[test]
fn svg_vector_stroke_uses_border_color() {
    use vb_common::geom::{BezPath, PathEl, Point};
    use vb_doc::model::{Geom, Node, NodeKind};
    use vb_doc::UndoStack;

    let mut doc = vb_doc::Document::new_default();
    let ab = doc.artboards[0];
    let ab_sid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
    let sid = doc.alloc_sid();
    let mut path = BezPath::new();
    path.push(PathEl::MoveTo(Point::new(0.0, 0.0)));
    path.push(PathEl::LineTo(Point::new(50.0, 50.0)));
    let mut n = Node::new(NodeKind::Vector { path }, "矢量", sid.clone());
    n.geom = Geom {
        x: 10.0,
        y: 10.0,
        w: 60.0,
        h: 60.0,
    };
    n.style.push(vb_css::Decl {
        prop: "border".into(),
        value: "3px solid #ff8800".into(),
        important: false,
    });
    let pid = doc.find_by_sid(&ab_sid).unwrap();
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(pid);
    doc.nodes.get_mut(pid).unwrap().children.push(id);
    let _ = UndoStack::new();

    let svg = vb_export::export_artboard_svg(&doc, ab, 1, false, None).expect("SVG 导出");
    assert!(
        svg.contains(r#"stroke="rgb(255,136,0)""#),
        "矢量描边应取 border 色:{svg}"
    );
    assert!(!svg.contains("rgb(20,20,20)"), "不得残留硬编码描边色");
}

/// B3:SVG 位图以 data URL 嵌入(导出的独立 SVG 不丢图)。
#[test]
fn svg_embeds_bitmap_as_data_url() {
    let dir = std::env::temp_dir().join(format!("vb-svg-bmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 0, 255, 255]));
    img.save(dir.join("assets").join("dot.png")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab1234" data-vb-name="AB">
    <img data-vb-id="n00001" src="assets/dot.png" style="position:absolute;left:20px;top:20px;width:40px;height:40px">
  </section>
</body>
</html>
"#,
    )
    .unwrap();
    let doc = import_project(&dir).expect("导入").doc;
    let ab = doc.artboards[0];
    let svg = vb_export::export_artboard_svg(&doc, ab, 1, false, Some(&dir)).expect("SVG 导出");
    assert!(
        svg.contains("data:image/png;base64,"),
        "位图应以 base64 data URL 嵌入"
    );
    assert!(svg.contains("<image "), "应有 <image> 元素:{svg}");
    let _ = std::fs::remove_dir_all(&dir);
}
