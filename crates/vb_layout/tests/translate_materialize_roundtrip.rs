//! 诊断:`left:50% + transform:translate()` 的节点被**显式几何编辑**后,
//! 导出/重导入是否稳定(2026-09-21 总验收,判据 C)。
//! 用 `cargo test -p vb_doc --test translate_materialize_roundtrip -- --nocapture` 看数值。

use std::path::PathBuf;

use vb_doc::commands::Command;
use vb_doc::export::render_project;
use vb_doc::import::import_project;
use vb_doc::model::Geom;

fn fixture() -> String {
    r##"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>锚点平移</title></head>
<body>
<div class="poster" data-vb-id="ab0001" data-vb-name="画板 1" style="position: relative; width: 1920px; height: 1080px">
  <span class="planet p1" data-vb-id="aa0001" data-vb-name="行星"
        style="position: absolute; left: 50%; top: 46%; transform: translate(-410px, -40px); width: 12px; height: 12px; border-radius: 50%"></span>
</div>
</body>
</html>
"##
    .to_string()
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("vb-tm-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("styles")).unwrap();
    std::fs::write(d.join("index.html"), fixture()).unwrap();
    d
}

fn dump(files: &[(String, String)]) {
    for (p, c) in files {
        if p.ends_with(".css") {
            println!("--- {p} ---\n{c}");
        }
    }
}

#[test]
fn probe_materialize_of_anchored_translate_node() {
    let dir = tmp("probe");
    let mut doc = import_project(&dir).expect("导入").doc;
    let _ = vb_layout::apply_import_layout(&mut doc, Some(&dir), false);
    let id = doc.find_by_sid("aa0001").unwrap();
    let g0 = doc.nodes.get(id).unwrap().geom;
    println!("① 导入 geom = {g0:?}  (期望 x=550, y=456.8)");
    let declared0 = doc.nodes.get(id).unwrap().geom_declared;
    println!("   geom_declared = {declared0}");

    // 显式几何编辑(与"移动/对齐"等价):把 x 设为 0(对齐画板左边)
    Command::SetGeom {
        sid: "aa0001".into(),
        new: Geom {
            x: 0.0,
            y: g0.y,
            w: g0.w,
            h: g0.h,
        },
        old: None,
        old_declared: None,
    }
    .apply(&mut doc)
    .expect("SetGeom");
    let id = doc.find_by_sid("aa0001").unwrap();
    let g1 = doc.nodes.get(id).unwrap().geom;
    println!("② 对齐到画板左(x=0)后 geom = {g1:?}");
    println!(
        "   geom_declared = {}",
        doc.nodes.get(id).unwrap().geom_declared
    );

    let f1 = render_project(&doc).files;
    dump(&f1);

    // 写盘 → 重导入 → 再导出
    for (rel, c) in &f1 {
        let p = dir.join(rel);
        if let Some(par) = p.parent() {
            std::fs::create_dir_all(par).unwrap();
        }
        std::fs::write(&p, c).unwrap();
    }
    let mut doc2 = import_project(&dir).expect("二次导入").doc;
    let _ = vb_layout::apply_import_layout(&mut doc2, Some(&dir), false);
    let id2 = doc2.find_by_sid("aa0001").unwrap();
    let g2 = doc2.nodes.get(id2).unwrap().geom;
    println!("③ 重导入 geom = {g2:?}   (应等于 ②)");
    let f2 = render_project(&doc2).files;
    let same = f1 == f2;
    println!("④ 二次导出与首轮逐字节相同 = {same}");
    for (a, b) in f1.iter().zip(f2.iter()) {
        if a != b {
            println!("   差异文件 {}", a.0);
            for (la, lb) in a.1.lines().zip(b.1.lines()) {
                if la != lb {
                    println!("     左: {la}\n     右: {lb}");
                }
            }
        }
    }
    assert!(same, "锚点+translate 节点在几何编辑后必须 L1 幂等");
    let _ = std::fs::remove_dir_all(&dir);
}
