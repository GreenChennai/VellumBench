//! `transform` 平移折算的门禁测试(阶段 2 / 副文档 03-3;判据 A)。
//!
//! `translate()` 折进画布几何是"画布与浏览器一致"的前提(s2-kv 的轨道环、
//! show2-card 的四角都是靠 `translate` 定位的)。这里把**折算口径**钉死:
//! 支持 px 与 `%`(按自身尺寸)、`translate/translateX/translateY`;
//! 解析不出的单位与 rotate/scale/skew **不折算**(不猜)—— 后者原样留在 CSS。

use vb_layout::parse_translate;

#[test]
fn translate_px_and_percent_are_folded() {
    assert_eq!(
        parse_translate("translate(12px, -8px)", 100.0, 50.0),
        (12.0, -8.0)
    );
    // 百分比按**自身尺寸**折算(s2-kv 的居中形态)
    assert_eq!(
        parse_translate("translate(-50%, -50%)", 560.0, 400.0),
        (-280.0, -200.0)
    );
    // 单参 = 只有 x(CSS 语义)
    assert_eq!(parse_translate("translate(10px)", 100.0, 50.0), (10.0, 0.0));
    // 混合
    assert_eq!(
        parse_translate("translate(10px, 50%)", 100.0, 40.0),
        (10.0, 20.0)
    );
}

#[test]
fn translate_axis_helpers_are_folded_and_accumulate() {
    assert_eq!(parse_translate("translateX(25%)", 200.0, 0.0), (50.0, 0.0));
    assert_eq!(parse_translate("translateY(-4px)", 0.0, 0.0), (0.0, -4.0));
    // 组合写法叠加
    assert_eq!(
        parse_translate("translateX(10px) translateY(20px)", 0.0, 0.0),
        (10.0, 20.0)
    );
}

#[test]
fn unknown_units_and_rotate_do_not_move_geometry() {
    // em/rem 等解析不出 → 不折算(不猜)
    assert_eq!(
        parse_translate("translate(2em, 1rem)", 100.0, 100.0),
        (0.0, 0.0)
    );
    // 旋转/缩放不属于平移折算范围(如实记录在 `parse_translate` 文档里)
    assert_eq!(
        parse_translate("rotate(30deg) scale(2)", 100.0, 100.0),
        (0.0, 0.0)
    );
    assert_eq!(parse_translate("none", 100.0, 100.0), (0.0, 0.0));
}

#[test]
fn nested_translate_survives_import_layout() {
    // 端到端:含 translate(-50%,-50%) 的稿件经导入 + 布局求值后,
    // 画布几何 = left/top 求值 + 自身平移(祖先平移经父相对坐标自然继承)。
    let html = r#"<!DOCTYPE html>
<html lang="zh-CN"><head><meta charset="utf-8"><title>折算</title>
<style>
  .poster { position: relative; width: 400px; height: 300px }
  .dot { position: absolute; left: 50%; top: 50%; width: 100px; height: 60px;
         transform: translate(-50%, -50%) }
</style></head>
<body><div class="poster"><div class="dot"></div></div></body></html>"#;
    let dir = std::env::temp_dir().join(format!("vb-tf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();

    let r = vb_doc::import::import_project(&dir).unwrap();
    let mut doc = r.doc;
    let synthetic = r.synthetic_artboard;
    vb_layout::apply_import_layout(&mut doc, Some(&dir), synthetic);

    let dot = doc
        .nodes
        .iter()
        .find(|(_, n)| n.classes.iter().any(|c| c == "dot"))
        .map(|(id, _)| id)
        .expect("有 .dot 节点");
    let g = doc.node(dot).unwrap().geom;
    // left:50% → 200;top:50% → 150;自身 100×60 → translate(-50,-30)
    assert!(
        (g.x - (200.0 - 50.0)).abs() < 0.6 && (g.y - (150.0 - 30.0)).abs() < 0.6,
        "translate 未折算: {g:?}"
    );
    assert!((g.w - 100.0).abs() < 0.01 && (g.h - 60.0).abs() < 0.01);

    let _ = std::fs::remove_dir_all(&dir);
}
