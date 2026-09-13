//! 三端一致性门禁(15 号计划 B1,设计文档 10 篇门禁 9):
//! 同一份文档 → DrawList → CPU 光栅 PNG 与 SVG(resvg 参考栅格化)做
//! 像素级容差比对。「画布所见 = 导出所得」从口号变成红绿灯。
//!
//! 容差依据:两条光栅管线的抗锯齿与色彩管理不同,边缘像素必然有差;
//! 门禁约束的是**结构级偏差**(渐变错位/缺失填充/双重缩放),故:
//! 逐像素最大通道差 ≤ 12/255 视为一致,≥ 97% 像素一致 + 平均差 ≤ 4
//! 即通过。任何 W 系 Bug 级别的偏差(整块纯色/错位/缺失)都会击穿阈值。

use vb_doc::import::import_project;

/// 练习全部视觉特性的样例文档:纯色/线性/径向渐变、圆角、半透明叠加、
/// 椭圆、描边。不含文本(ADR-0017 文本占位三端本就不同,B4/B3 批次处理)。
const SAMPLE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab1234" data-vb-name="AB" style="background-color:#202028">
    <div data-vb-id="s00001" style="position:absolute;left:20px;top:20px;width:120px;height:80px;background-color:#e2543e"></div>
    <div data-vb-id="s00002" style="position:absolute;left:170px;top:20px;width:120px;height:80px;background-image:linear-gradient(135deg,#2244cc,#22cc88)"></div>
    <div data-vb-id="s00003" style="position:absolute;left:320px;top:20px;width:120px;height:80px;background-image:radial-gradient(circle at 40% 40%,#f0a030,#7a3010)"></div>
    <div data-vb-id="s00004" style="position:absolute;left:20px;top:130px;width:120px;height:80px;background-color:#3e7ae2;border-radius:18px;border:4px solid #101010"></div>
    <div data-vb-id="s00005" style="position:absolute;left:170px;top:130px;width:120px;height:80px;background-color:#ffffff;opacity:0.5"></div>
    <div data-vb-id="s00006" style="position:absolute;left:320px;top:130px;width:120px;height:80px;background-color:#8e44ad;border-radius:40px 12px 40px 12px"></div>
    <div data-vb-id="s00007" style="position:absolute;left:60px;top:250px;width:80px;height:80px;background-color:#1abc9c;border-radius:50%"></div>
  </section>
</body>
</html>
"#;

fn cpu_rgba() -> (Vec<u8>, u32, u32) {
    let dir = std::env::temp_dir().join(format!("vb-consistency-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), SAMPLE).unwrap();
    let r = import_project(&dir).expect("导入");
    let ab = r.doc.artboards[0];
    let (png, warnings) =
        vb_export::export_artboard_png(&r.doc, ab, 1.0, false, Some(&dir)).expect("CPU 渲染");
    assert!(warnings.is_empty(), "样例不应产生渲染警告:{warnings:?}");
    let img = image::load_from_memory(&png).expect("PNG 解码");
    let (w, h) = (img.width(), img.height());
    let _ = std::fs::remove_dir_all(&dir);
    (img.to_rgba8().into_raw(), w, h)
}

fn svg_rgba() -> (Vec<u8>, u32, u32) {
    let dir = std::env::temp_dir().join(format!("vb-consistency-svg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), SAMPLE).unwrap();
    let r = import_project(&dir).expect("导入");
    let ab = r.doc.artboards[0];
    let svg = vb_export::export_artboard_svg(&r.doc, ab, 1, false, Some(&dir)).expect("SVG 导出");
    let _ = std::fs::remove_dir_all(&dir);

    let tree =
        resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).expect("SVG 解析");
    let size = tree.size();
    let (w, h) = (size.width() as u32, size.height() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).expect("SVG 栅格化画布");
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    (pixmap.data().to_vec(), w, h)
}

#[test]
fn cpu_and_svg_render_the_same_scene() {
    let (cpu, cw, ch) = cpu_rgba();
    let (svg, sw, sh) = svg_rgba();
    assert_eq!((cw, ch), (sw, sh), "三端画布尺寸必须一致");

    assert_eq!(cpu.len(), svg.len());
    let n = cpu.len() / 4;
    let mut worst = 0u32;
    let mut total = 0u64;
    let mut mismatched = 0usize;
    for i in 0..n {
        let a = &cpu[i * 4..i * 4 + 4];
        let b = &svg[i * 4..i * 4 + 4];
        let mut d = 0u32;
        for k in 0..4 {
            d = d.max((a[k] as i32 - b[k] as i32).unsigned_abs());
        }
        worst = worst.max(d);
        total += (a[0].abs_diff(b[0]) + a[1].abs_diff(b[1]) + a[2].abs_diff(b[2])) as u64;
        if d > 12 {
            mismatched += 1;
        }
    }
    let mean = total as f64 / (n as f64 * 3.0);
    let ok_ratio = 1.0 - mismatched as f64 / n as f64;
    assert!(
        ok_ratio >= 0.97,
        "一致像素率 {ok_ratio:.4} < 0.97(最差通道差 {worst},平均差 {mean:.2})"
    );
    assert!(
        mean <= 4.0,
        "平均通道差 {mean:.2} > 4(最差 {worst},一致率 {ok_ratio:.4})"
    );
}
