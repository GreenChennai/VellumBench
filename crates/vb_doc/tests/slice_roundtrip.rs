//! 切片(data-vb-slice)建模与导出往返(05-2 / 09-C):
//! 编辑器建立的 `NodeKind::Slice` 节点导出为带 `data-vb-slice` 属性的
//! `<div>`;重导入必须**识别回 Slice kind**(编辑器画切片框、
//! `vellum-cli export --slice` 按名出图的共同前提)。

mod common;

use vb_doc::export::render_project;
use vb_doc::import::import_project;
use vb_doc::model::NodeKind;

#[test]
fn slice_kind_roundtrips_through_html() {
    let html = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>切片</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="aa0001" data-vb-name="画板">
    <div class="box" data-vb-id="aa0002" data-vb-name="盒子" style="left: 10px; top: 20px; width: 120px; height: 80px;"></div>
    <div data-vb-id="aa0003" data-vb-name="切片 Hero" data-vb-slice="切片 Hero" style="left: 10px; top: 20px; width: 120px; height: 80px;"></div>
  </section>
</body>
</html>
"#;
    let doc = common::import_files(&[("index.html", html)]);

    // 导入:data-vb-slice 属性 → Slice kind;属性经 attrs 通道保留
    let sid = doc.find_by_sid("aa0003").expect("切片节点导入");
    let n = doc.nodes.get(sid).unwrap();
    assert!(
        matches!(n.kind, NodeKind::Slice),
        "data-vb-slice 属性必须识别为切片节点,实际 {}",
        n.kind.kind_name()
    );
    assert_eq!(
        n.attrs.get("data-vb-slice").map(String::as_str),
        Some("切片 Hero")
    );
    // 普通盒子不受影响
    let box_n = doc.nodes.get(doc.find_by_sid("aa0002").unwrap()).unwrap();
    assert!(matches!(box_n.kind, NodeKind::Box));

    // 导出:切片节点原样写回属性(L1 幂等的结构前提)
    let files = render_project(&doc);
    let html_out = files
        .files
        .iter()
        .find(|(p, _)| p == "index.html")
        .map(|(_, c)| c.clone())
        .unwrap();
    assert!(
        html_out.contains(r#"data-vb-slice="切片 Hero""#),
        "切片属性必须导出到 HTML"
    );

    // 再导入 → 再导出字节一致(与既有 L1 门同口径)
    let tmp = std::env::temp_dir().join(format!("vb-slice-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    vb_doc::export::write_project(&doc, &tmp).unwrap();
    let r2 = import_project(&tmp).unwrap();
    let first: std::collections::BTreeMap<String, String> = files
        .files
        .iter()
        .map(|(p, c)| (p.clone(), c.clone()))
        .collect();
    let second_files = render_project(&r2.doc);
    let second: std::collections::BTreeMap<String, String> = second_files
        .files
        .iter()
        .map(|(p, c)| (p.clone(), c.clone()))
        .collect();
    assert_eq!(first, second, "切片往返 L1 幂等");
    let _ = std::fs::remove_dir_all(&tmp);
}
