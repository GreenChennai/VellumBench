//! CPU 渲染快照测试(设计文档 10 篇 §四门禁 4 的 v0.1 版):
//! 导入示例落地页 → DrawList → CPU 光栅 → PNG 结构断言。
//! 像素级基线比对随 vello_cpu 统一接入(ADR-0016)。

use std::path::{Path, PathBuf};

use image::GenericImageView;

use vb_doc::import::import_project;
use vb_render::cpu;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = crates/vb_render
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn cpu_render_landing_hero_png() {
    let dir = repo_root().join("examples").join("landing");
    let r = import_project(&dir).expect("示例项目应可导入");
    assert!(!r.doc.artboards.is_empty());

    let hero = r.doc.artboards[0];
    let list = vb_render::encode::encode_artboard(&r.doc, hero).expect("编码");
    assert!(list.w > 0.0 && list.h > 0.0);
    assert!(list.items.len() >= 5, "Hero 画板应至少 5 个绘制项");

    let out = cpu::render_png(&list, 1.0, false, Some(&dir)).expect("CPU 渲染");
    // PNG 头
    assert_eq!(&out.png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(out.png.len() > 10_000, "PNG 应有实质内容");

    // @2x:尺寸翻倍
    let out2 = cpu::render_png(&list, 2.0, false, Some(&dir)).expect("CPU 渲染 @2x");
    assert!(out2.png.len() > out.png.len() * 3 / 2, "@2x 内容应显著更多");

    // 解码验证尺寸
    let img = image::load_from_memory(&out2.png).expect("PNG 解码");
    assert_eq!(img.width(), (list.w * 2.0).round() as u32);
    assert_eq!(img.height(), (list.h * 2.0).round() as u32);
}

#[test]
fn cpu_render_text_and_frozen_warn() {
    // 文本 → 占位条 warning;脚本/冻结 → 占位 warning(ADR-0016/0017)
    let dir = std::env::temp_dir().join(format!("vb-render-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab1234" data-vb-name="AB">
    <h1 class="t" data-vb-id="t00001" data-vb-name="标题" style="left: 10px; top: 10px; width: 300px; height: 60px; color: #fff; font-size: 48px;">你好</h1>
    <svg data-vb-id="s00001" viewBox="0 0 1 1"><path d="M0 0"></path></svg>
  </section>
</body>
</html>
"#,
    )
    .unwrap();

    let r = import_project(&dir).unwrap();
    let ab = r.doc.artboards[0];
    let list = vb_render::encode::encode_artboard(&r.doc, ab).unwrap();
    let out = cpu::render_png(&list, 1.0, true, Some(&dir)).unwrap();
    assert!(out.warnings.iter().any(|w| w.contains("占位条")));
    assert!(out.warnings.iter().any(|w| w.contains("冻结块")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_project_writes_files() {
    let dir = std::env::temp_dir().join(format!("vb-export-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!DOCTYPE html>\n<html><body></body></html>",
    )
    .unwrap();

    let r = import_project(&dir).unwrap();
    let written = vb_doc::export::write_project(&r.doc, &dir).unwrap();
    assert!(written.iter().any(|p| p.ends_with("index.html")));
    assert!(written.iter().any(|p| p.ends_with("main.css")));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// 15 号计划 A2:所见即所得修复(渐变偏移/透明导出/方向关键字/节点透明度)
// ---------------------------------------------------------------------------

/// 带一个节点的最小文档构造器:left/top/width/height/extra_style。
fn one_node_doc(
    dir: &Path,
    left: f64,
    top: f64,
    w: f64,
    h: f64,
    extra_style: &str,
) -> vb_doc::Document {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        format!(
            r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab1234" data-vb-name="AB">
    <div data-vb-id="n00001" style="position:absolute; left:{left}px; top:{top}px; width:{w}px; height:{h}px; {extra_style}"></div>
  </section>
</body>
</html>
"#
        ),
    )
    .unwrap();
    import_project(dir).expect("导入").doc
}

fn sample(img: &image::DynamicImage, x: u32, y: u32) -> [u8; 4] {
    img.get_pixel(x, y).0
}

/// W1:非原点节点的线性渐变必须有方向(此前整块被 Pad 成末档色)。
#[test]
fn linear_gradient_reaches_non_origin_node() {
    let dir = std::env::temp_dir().join(format!("vb-grad-{}", std::process::id()));
    let doc = one_node_doc(
        &dir,
        200.0,
        100.0,
        100.0,
        100.0,
        "background-image: linear-gradient(180deg, #ff0000, #0000ff);",
    );
    let ab = doc.artboards[0];
    let list = vb_render::encode::encode_artboard_opts(&doc, ab, true).expect("编码");
    let out = cpu::render_png(&list, 1.0, true, Some(&dir)).expect("渲染");
    let img = image::load_from_memory(&out.png).unwrap();
    let top = sample(&img, 250, 115);
    let bottom = sample(&img, 250, 185);
    assert!(
        top[0] > 180 && top[2] < 120,
        "节点上缘应为红,实际 rgba={top:?}"
    );
    assert!(
        bottom[2] > 180 && bottom[0] < 120,
        "节点下缘应为蓝,实际 rgba={bottom:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// W3:节点 opacity 必须作用于渐变填充(CPU 端此前完全忽略)。
#[test]
fn node_opacity_applies_to_gradient() {
    let dir = std::env::temp_dir().join(format!("vb-grad-op-{}", std::process::id()));
    let doc = one_node_doc(
        &dir,
        0.0,
        0.0,
        100.0,
        100.0,
        "opacity: 0.5; background-image: linear-gradient(180deg, #ff0000, #ff0000);",
    );
    let ab = doc.artboards[0];
    let list = vb_render::encode::encode_artboard_opts(&doc, ab, true).expect("编码");
    let out = cpu::render_png(&list, 1.0, true, Some(&dir)).expect("渲染");
    let img = image::load_from_memory(&out.png).unwrap();
    let px = sample(&img, 50, 50);
    assert!(
        (px[3] as i32 - 127).abs() <= 6,
        "半透明节点渐变的 alpha 应≈127,实际 {px:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// W2:--transparent 导出必须真的透明(此前底色矩形无条件铺满)。
#[test]
fn transparent_export_is_actually_transparent() {
    let dir = std::env::temp_dir().join(format!("vb-tsp-{}", std::process::id()));
    let doc = one_node_doc(&dir, 10.0, 10.0, 50.0, 50.0, "background-color: #123456;");
    let ab = doc.artboards[0];
    let list = vb_render::encode::encode_artboard_opts(&doc, ab, true).expect("编码");
    let out = cpu::render_png(&list, 1.0, true, Some(&dir)).expect("渲染");
    let img = image::load_from_memory(&out.png).unwrap();
    let corner = sample(&img, 0, 0);
    assert_eq!(corner[3], 0, "透明导出四角 alpha 应为 0,实际 {corner:?}");

    let list2 = vb_render::encode::encode_artboard_opts(&doc, ab, false).expect("编码");
    let out2 = cpu::render_png(&list2, 1.0, false, Some(&dir)).expect("渲染");
    let img2 = image::load_from_memory(&out2.png).unwrap();
    assert_eq!(sample(&img2, 0, 0)[3], 255, "默认导出应有不透明底");
    let _ = std::fs::remove_dir_all(&dir);
}

/// W6:方向关键字与 turn 单位解析(此前 `to right` 被当色标吞掉)。
#[test]
fn gradient_direction_keywords_parse() {
    let doc = vb_doc::Document::new_default();
    let p = |v: &str| vb_render::encode::parse_linear_gradient(&doc, v, 100.0, 50.0);

    let (angle, stops) = p("linear-gradient(to right, #ff0000, #0000ff)").expect("to right");
    assert!(
        (angle - 90.0).abs() < 1e-9,
        "to right 应为 90°,实际 {angle}"
    );
    assert_eq!(stops.len(), 2);
    assert!((stops[0].pos - 0.0).abs() < 1e-6 && (stops[1].pos - 1.0).abs() < 1e-6);

    // to top right:α = atan(h/w) = atan(0.5) ≈ 26.565°
    let (angle, _) = p("linear-gradient(to top right, #ff0000, #0000ff)").expect("corner");
    assert!((angle - 26.565_051).abs() < 1e-3, "实际 {angle}");

    let (angle, _) = p("linear-gradient(0.5turn, #ff0000, #0000ff)").expect("turn");
    assert!((angle - 180.0).abs() < 1e-9);

    // 命名色开头不得被误判为方向
    let (angle, stops) = p("linear-gradient(red, blue)").expect("named colors");
    assert!((angle - 180.0).abs() < 1e-9);
    assert_eq!(stops.len(), 2);

    // 未知方向:整条无效
    assert!(p("linear-gradient(to somewhere, red, blue)").is_none());
}

// ---------------------------------------------------------------------------
// 15 号计划 B2:渲染 P2 清账(圆角多值/百分比、bg-color 兜底、位图)
// ---------------------------------------------------------------------------

fn one_node_doc_raw(html_body_inner: &str, dir: &Path) -> vb_doc::Document {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        format!(
            r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab1234" data-vb-name="AB">
{html_body_inner}  </section>
</body>
</html>
"#
        ),
    )
    .unwrap();
    import_project(dir).expect("导入").doc
}

/// B2:border-radius 四值简写展开为 [tl,tr,br,bl]。
#[test]
fn border_radius_four_values() {
    let dir = std::env::temp_dir().join(format!("vb-br4-{}", std::process::id()));
    let doc = one_node_doc_raw(
        r#"    <div data-vb-id="n00001" style="position:absolute;left:0;top:0;width:100px;height:80px;background-color:#123456;border-radius:20px 4px 30px 8px"></div>
"#,
        &dir,
    );
    let ab = doc.artboards[0];
    let list = vb_render::encode::encode_artboard(&doc, ab).expect("编码");
    let item = list
        .items
        .iter()
        .find(|i| i.kind == vb_render::encode::DrawKind::Box && i.rect[2] == 100.0)
        .expect("应有目标项");
    assert_eq!(item.radii, [20.0, 4.0, 30.0, 8.0], "{:?}", item.radii);
    let _ = std::fs::remove_dir_all(&dir);
}

/// B2:border-radius 百分比按短边近似。
#[test]
fn border_radius_percentage() {
    let dir = std::env::temp_dir().join(format!("vb-brp-{}", std::process::id()));
    let doc = one_node_doc_raw(
        r#"    <div data-vb-id="n00001" style="position:absolute;left:0;top:0;width:100px;height:80px;background-color:#123456;border-radius:25%"></div>
"#,
        &dir,
    );
    let ab = doc.artboards[0];
    let list = vb_render::encode::encode_artboard(&doc, ab).expect("编码");
    let item = list
        .items
        .iter()
        .find(|i| i.kind == vb_render::encode::DrawKind::Box && i.rect[2] == 100.0)
        .expect("应有目标项");
    // 25% × min(100,80) = 20
    assert_eq!(item.radii[0], 20.0, "{:?}", item.radii);
    let _ = std::fs::remove_dir_all(&dir);
}

/// B2:background-image 为 url 等不支持的形态时,回退 background-color。
#[test]
fn background_color_fallback_when_image_unsupported() {
    let dir = std::env::temp_dir().join(format!("vb-bgf-{}", std::process::id()));
    let doc = one_node_doc_raw(
        r#"    <div data-vb-id="n00001" style="position:absolute;left:0;top:0;width:100px;height:80px;background-image:url(assets/x.png);background-color:#123456"></div>
"#,
        &dir,
    );
    let ab = doc.artboards[0];
    let list = vb_render::encode::encode_artboard(&doc, ab).expect("编码");
    let item = list
        .items
        .iter()
        .find(|i| i.kind == vb_render::encode::DrawKind::Box && i.rect[2] == 100.0)
        .expect("应有目标项");
    match &item.fill {
        Some(vb_render::encode::FillDef::Solid(c)) => {
            assert!((c[0] - 0x12 as f32 / 255.0).abs() < 0.01, "应回退到背景色");
        }
        other => panic!("应有 Solid 填充兜底,实际 {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// B2:位图 @2x 落点必须乘 scale(此前位置减半)且不透明度生效。
#[test]
fn bitmap_scales_position_and_opacity() {
    let dir = std::env::temp_dir().join(format!("vb-bmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    let img = image::RgbaImage::from_pixel(10, 10, image::Rgba([255, 0, 0, 255]));
    img.save(dir.join("assets").join("dot.png")).unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab1234" data-vb-name="AB">
    <img data-vb-id="n00001" src="assets/dot.png" style="position:absolute;left:40px;top:40px;width:20px;height:20px;opacity:0.5">
  </section>
</body>
</html>
"#
        ,
    )
    .unwrap();
    let doc = import_project(&dir).expect("导入").doc;
    let ab = doc.artboards[0];
    let list = vb_render::encode::encode_artboard_opts(&doc, ab, true).expect("编码");
    let out = vb_render::cpu::render_png(&list, 2.0, true, Some(&dir)).expect("渲染 @2x");
    let img = image::load_from_memory(&out.png).unwrap();
    let px = img.get_pixel(2 * 40 + 10, 2 * 40 + 10).0;
    assert!(
        px[0] > 200 && (px[3] as i32 - 127).abs() <= 8,
        "@2x 位图应落在 (100,100) 且半透明,实际 {px:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
