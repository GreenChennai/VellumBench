//! Agent `align` op 的**参照系与 L1 幂等**回归(2026-09-21 总验收发现并修复)。
//!
//! 缺陷原貌:`align_cmds` 把节点的 `geom`(父相对坐标)当成"画板本地坐标"
//! 直接算 `min_x / center / max_r`,于是
//! 1. 不同父级的成员被拿**错参照系**的数字比较 → 对齐结果错(浏览器里也跳);
//! 2. 写回的几何落盘后与"重导入值"不等 → **每存一次漂移一次**,判据 C(L1)打穿
//!    (实测:save1 `left:140px` → save2 `-270px` → save3 `-680px` …)。
//!
//! 现在几何内核在 `vb_tools::align`(与画布/对齐面板同源):
//! 目标盒与成员盒都取**绝对盒**,位移用绝对系差值平移自身 `geom`。
//! 本文件把它钉死:**对齐必须精确到绝对系 + 二次导出必须逐字节相同**。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use vb_agent::patch::{apply_patch, PatchOp, PatchRequest};
use vb_doc::export::render_project;
use vb_doc::import::import_project;
use vb_doc::model::Document;
use vb_tools::align::AbsBox;

static SEQ: AtomicU32 = AtomicU32::new(0);

fn unique_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("vb-align-l1-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 两个成员**父级不同、父偏移不同**——这正是原实现算错的场景。
fn fixture_html() -> &'static str {
    r##"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>对齐参照系</title><link rel="stylesheet" href="styles/main.css"></head>
<body>
<section class="poster" data-vb-id="ab0001" data-vb-name="画板 1" style="position: relative; width: 800px; height: 600px">
  <div class="wrap" data-vb-id="wr0001" data-vb-name="容器甲" style="position: absolute; left: 100px; top: 50px; width: 400px; height: 300px">
    <div class="a" data-vb-id="aa0001" data-vb-name="甲" style="position: absolute; left: 10px; top: 10px; width: 20px; height: 20px; background-color: #f00"></div>
  </div>
  <div class="wrap2" data-vb-id="wr0002" data-vb-name="容器乙" style="position: absolute; left: 500px; top: 200px; width: 200px; height: 200px">
    <div class="b" data-vb-id="bb0001" data-vb-name="乙" style="position: absolute; left: 300px; top: 5px; width: 40px; height: 10px; background-color: #00f"></div>
  </div>
</section>
</body>
</html>
"##
}

fn write_fixture(dir: &Path) {
    std::fs::create_dir_all(dir.join("styles")).unwrap();
    std::fs::write(dir.join("index.html"), fixture_html()).unwrap();
    // 甲绝对盒 = (110,60)-(130,80);乙绝对盒 = (800,205)-(840,215)
    let css = ".poster{position:relative;width:800px;height:600px}\
.wrap{position:absolute;left:100px;top:50px;width:400px;height:300px}\
.wrap2{position:absolute;left:500px;top:200px;width:200px;height:200px}\
.a{position:absolute;left:10px;top:10px;width:20px;height:20px;background-color:#f00}\
.b{position:absolute;left:300px;top:5px;width:40px;height:10px;background-color:#00f}\n";
    std::fs::write(dir.join("styles/main.css"), css).unwrap();
}

fn boxes(doc: &Document, sids: &[&str]) -> Vec<AbsBox> {
    sids.iter()
        .map(|s| {
            let id = doc.find_by_sid(s).unwrap_or_else(|| panic!("{s} 不存在"));
            AbsBox::of(doc, id).unwrap_or_else(|| panic!("{s} 无绝对盒"))
        })
        .collect()
}

fn run_align(doc: &mut Document, op: PatchOp) -> Vec<String> {
    let mut undo = vb_doc::UndoStack::default();
    let req = PatchRequest {
        base_rev: None,
        ops: vec![op],
    };
    let out = apply_patch(doc, &mut undo, &req).expect("patch 应成功");
    out.warnings
}

fn align_left_to_selection(doc: &mut Document) {
    let _ = run_align(
        doc,
        PatchOp::Align {
            ids: vec!["aa0001".into(), "bb0001".into()],
            mode: "left".into(),
            to: Some("selection".into()),
        },
    );
}

/// 判据:对齐后两个成员的**绝对左边界相等**(且等于并集左边 = 110)。
#[test]
fn align_left_is_exact_in_absolute_frame() {
    let dir = unique_dir("abs");
    write_fixture(&dir);
    let mut doc = import_project(&dir).expect("导入应成功").doc;

    let before = boxes(&doc, &["aa0001", "bb0001"]);
    assert_eq!(before[0].x0, 110.0, "夹具前置:甲绝对左 = 100+10");
    assert_eq!(before[1].x0, 800.0, "夹具前置:乙绝对左 = 500+300");

    align_left_to_selection(&mut doc);

    let after = boxes(&doc, &["aa0001", "bb0001"]);
    assert_eq!(after[0].x0, 110.0, "甲本来就在最左,不该动");
    assert_eq!(
        after[1].x0, 110.0,
        "乙必须真的贴到 110(旧实现会落到 500+110=610 之类的错位置)"
    );
    // 尺寸不变(对齐只平移)
    assert_eq!(after[1].w(), 40.0);
    // 乙的**自身** geom 仍是"父相对":110 - 500 = -390
    let b_id = doc.find_by_sid("bb0001").unwrap();
    assert_eq!(doc.nodes.get(b_id).unwrap().geom.x, -390.0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 判据:`to: artboard` 也是绝对系(对齐到画板左边 x=0)。
#[test]
fn align_left_to_artboard_is_absolute() {
    let dir = unique_dir("ab");
    write_fixture(&dir);
    let mut doc = import_project(&dir).expect("导入应成功").doc;
    let _ = run_align(
        &mut doc,
        PatchOp::Align {
            ids: vec!["aa0001".into(), "bb0001".into()],
            mode: "left".into(),
            to: Some("artboard".into()),
        },
    );
    for b in boxes(&doc, &["aa0001", "bb0001"]) {
        assert_eq!(b.x0, 0.0, "对齐到画板左边应精确到 0");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **判据 C(L1 幂等)**:对齐 → 导出 → 重导入 → 再导出,必须逐字节相同。
///
/// 这正是原缺陷被打穿的地方:每轮漂移一次(140 → -270 → -680 …)。
#[test]
fn align_then_roundtrip_is_byte_idempotent() {
    let dir = unique_dir("l1");
    write_fixture(&dir);

    let mut doc = import_project(&dir).expect("导入应成功").doc;
    align_left_to_selection(&mut doc);

    let write = |doc: &Document| {
        let out = render_project(doc);
        for (rel, content) in &out.files {
            let p = dir.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&p, content).unwrap();
        }
        out.files
    };

    let first = write(&doc);
    // 重导入 → 再导出(不做任何编辑)
    let doc2 = import_project(&dir).expect("二次导入应成功").doc;
    let second = write(&doc2);

    assert_eq!(
        first, second,
        "对齐后二次导出必须逐字节相同(旧实现每轮漂移一次)"
    );

    // 再跑一轮,确保不是"两轮才收敛"
    let doc3 = import_project(&dir).expect("三次导入应成功").doc;
    let third = write(&doc3);
    assert_eq!(second, third, "第三次必须继续稳定");

    // 绝对几何稳定:两成员左边界相等
    let b = boxes(&doc3, &["aa0001", "bb0001"]);
    assert_eq!(b[0].x0, b[1].x0, "重导入后绝对左边界仍相等");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 跨画板成员不得混算(各画板各自对齐),落单成员给 warning。
#[test]
fn align_keeps_artboards_separate() {
    let dir = unique_dir("multi");
    write_fixture(&dir);
    let mut doc = import_project(&dir).expect("导入应成功").doc;
    // 只选**同一个父级**下的成员:落单 → warning,且几何不变
    let before = boxes(&doc, &["aa0001"]);
    let warns = run_align(
        &mut doc,
        PatchOp::Align {
            ids: vec!["aa0001".into()],
            mode: "left".into(),
            to: Some("selection".into()),
        },
    );
    assert!(
        warns.iter().any(|w| w.contains("落单")),
        "单成员对齐应给出落单 warning,实得 {warns:?}"
    );
    let after = boxes(&doc, &["aa0001"]);
    assert_eq!(before, after, "被跳过的成员不得被移动");
    let _ = std::fs::remove_dir_all(&dir);
}
