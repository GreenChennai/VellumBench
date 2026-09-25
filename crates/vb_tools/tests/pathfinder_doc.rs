//! 路径查找器多结果运算 × 文档事务测试(05-3 / X-1)。
//!
//! 覆盖:统一入口 `pathfinder_multi_cmds` 产出的 `Command::MultiResult`
//! ① 经 UndoStack 的 undo/redo 往返;② 结果几何与填充继承;
//! ③ 运算产物「序列化 → 导入 → 序列化」幂等(canonical 往返不破)。

use vb_common::geom::{BezPath, Point};
use vb_doc::export::render_project;
use vb_doc::model::{Document, Geom, Node, NodeKind};
use vb_doc::undo::UndoStack;
use vb_tools::pathfinder::{pathfinder_multi_cmds, MultiOp};

/// 本地系矩形路径(闭合)。
fn rect_path(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(Point::new(x, y));
    p.line_to(Point::new(x + w, y));
    p.line_to(Point::new(x + w, y + h));
    p.line_to(Point::new(x, y + h));
    p.close_path();
    p
}

/// 在画板下放一个矢量节点(geom 决定节点原点,path 为节点本地系)。
fn put_vector(doc: &mut Document, name: &str, geom: Geom, path: BezPath, fill: &str) -> String {
    let sid = doc.alloc_sid();
    let mut n = Node::new(NodeKind::Vector { path }, name, sid.clone());
    n.geom = geom;
    n.style = vec![
        vb_css::Decl {
            prop: "fill".into(),
            value: fill.into(),
            important: false,
        },
        // 渲染器可见通道(CPU 导出读 background-color;冒烟 PNG 用)
        vb_css::Decl {
            prop: "background-color".into(),
            value: fill.into(),
            important: false,
        },
    ];
    let ab = doc.artboards[0];
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    doc.nodes.get_mut(ab).unwrap().children.push(id);
    sid.as_str().to_string()
}

/// 夹具:A(红,0,0,200×200)+ B(蓝,100,50,200×100)。
fn fixture() -> (Document, Vec<String>) {
    let mut doc = Document::new_default();
    let a = put_vector(
        &mut doc,
        "A",
        Geom {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 200.0,
        },
        rect_path(0.0, 0.0, 200.0, 200.0),
        "#ff0000",
    );
    let b = put_vector(
        &mut doc,
        "B",
        Geom {
            x: 100.0,
            y: 50.0,
            w: 200.0,
            h: 100.0,
        },
        rect_path(0.0, 0.0, 200.0, 100.0),
        "#0000ff",
    );
    (doc, vec![a, b])
}

/// 分割:一条事务产出 3 个结果节点;undo 精确逆回;redo 后 sid 不变。
#[test]
fn divide_transaction_roundtrip() {
    let (mut doc, sids) = fixture();
    let before = render_project(&doc).files;

    let plan = pathfinder_multi_cmds(&mut doc, MultiOp::Divide, &sids).expect("分割成功");
    let result_sids = plan.result_sids.clone();
    assert_eq!(result_sids.len(), 3, "两交叉矩形分割应产出 3 件");

    let mut undo = UndoStack::new();
    undo.push(&mut doc, plan.command).expect("apply 成功");
    for s in &sids {
        assert!(doc.find_by_sid(s).is_none(), "源 {s} 应已删除");
    }
    // 结果几何:三块包围盒(画板系)
    let mut boxes: Vec<[f64; 4]> = result_sids
        .iter()
        .map(|r| {
            let n = doc.nodes.get(doc.find_by_sid(r).unwrap()).unwrap();
            [n.geom.x, n.geom.y, n.geom.w, n.geom.h]
        })
        .collect();
    boxes.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    let near = |g: &[f64; 4], w: [f64; 4], what: &str| {
        for (got, want) in g.iter().zip(w) {
            assert!((got - want).abs() < 1e-6, "{what}: {g:?} 应为 {w:?}");
        }
    };
    near(&boxes[0], [0.0, 0.0, 200.0, 200.0], "A−B 块");
    near(&boxes[1], [100.0, 50.0, 100.0, 100.0], "交叠块");
    near(&boxes[2], [200.0, 50.0, 100.0, 100.0], "B−A 块");

    // 填充继承:交叠块(含 x∈[100,200] y∈[50,150] 的)取最上层 B 的蓝
    let overlap = result_sids
        .iter()
        .map(|r| doc.nodes.get(doc.find_by_sid(r).unwrap()).unwrap())
        .find(|n| (n.geom.x - 100.0).abs() < 1.0 && (n.geom.y - 50.0).abs() < 1.0)
        .expect("交叠块");
    let fill = overlap.style.iter().find(|d| d.prop == "fill").unwrap();
    assert_eq!(fill.value, "#0000ff", "交叠块填充继承最上层源 B");

    // undo 精确逆回 → redo 等效且 sid 稳定
    undo.undo(&mut doc).expect("undo");
    assert_eq!(render_project(&doc).files, before, "撤销后逐字节还原");
    undo.redo(&mut doc).expect("redo");
    for r in &result_sids {
        assert!(doc.find_by_sid(r).is_some(), "结果 {r} 的 sid 稳定");
    }
}

/// 修边与轮廓:条目数与样式语义(fill:none / 描边继承)。
#[test]
fn trim_and_outline_styles() {
    // 修边:下层可见 + 上层完整,描边被去掉(源带 stroke)
    let (mut doc, sids) = fixture();
    {
        let b_id = doc.find_by_sid(&sids[1]).unwrap();
        doc.nodes.get_mut(b_id).unwrap().style.push(vb_css::Decl {
            prop: "stroke".into(),
            value: "#00ff00".into(),
            important: false,
        });
    }
    let plan = pathfinder_multi_cmds(&mut doc, MultiOp::Trim, &sids).expect("修边成功");
    assert_eq!(plan.result_sids.len(), 2, "异色修边产出 2 件");
    let mut undo = UndoStack::new();
    undo.push(&mut doc, plan.command).expect("apply");
    for n in doc.nodes.values() {
        if matches!(n.kind, NodeKind::Vector { .. }) {
            assert!(
                !n.style.iter().any(|d| d.prop == "stroke"),
                "修边产物不得带描边"
            );
        }
    }
    // 上层蓝块的可见部分就是 B 自身(其上没有别的对象),填充保持蓝
    let top = plan
        .result_sids
        .iter()
        .map(|r| doc.nodes.get(doc.find_by_sid(r).unwrap()).unwrap())
        .find(|n| (n.geom.x - 100.0).abs() < 1.0 && (n.geom.y - 50.0).abs() < 1.0)
        .expect("上层完整块");
    assert_eq!(
        top.style.iter().find(|d| d.prop == "fill").unwrap().value,
        "#0000ff"
    );

    // 轮廓:fill:none + 无描边源补 1px 黑
    let (mut doc, sids) = fixture();
    let plan = pathfinder_multi_cmds(&mut doc, MultiOp::Outline, &sids).expect("轮廓成功");
    assert_eq!(plan.result_sids.len(), 12, "两交叉矩形轮廓切 12 段");
    let mut undo = UndoStack::new();
    undo.push(&mut doc, plan.command).expect("apply");
    for r in &plan.result_sids {
        let n = doc.nodes.get(doc.find_by_sid(r).unwrap()).unwrap();
        let fill = n
            .style
            .iter()
            .find(|d| d.prop == "fill")
            .expect("轮廓件必有 fill 声明");
        assert_eq!(fill.value, "none");
        let stroke = n
            .style
            .iter()
            .find(|d| d.prop == "stroke")
            .expect("无描边源补黑");
        assert_eq!(stroke.value, "#000000");
        assert_eq!(
            n.style
                .iter()
                .find(|d| d.prop == "stroke-width")
                .unwrap()
                .value,
            "1"
        );
    }
}

/// 校验与提示:非矢量 / 跨画板 / 数量不足,给中文错误且不改文档。
#[test]
fn unified_entry_validation() {
    let (mut doc, sids) = fixture();
    let before = render_project(&doc).files;
    // 数量不足
    assert!(pathfinder_multi_cmds(&mut doc, MultiOp::Divide, &sids[..1]).is_err());
    // 非矢量(把 B 换成盒子)
    let b_id = doc.find_by_sid(&sids[1]).unwrap();
    doc.nodes.get_mut(b_id).unwrap().kind = NodeKind::Box;
    assert!(pathfinder_multi_cmds(&mut doc, MultiOp::Divide, &sids).is_err());
    assert_eq!(render_project(&doc).files, before, "校验失败不得改动文档");
}

/// 运算产物经「序列化 → 导入 → 序列化」幂等(canonical 往返不破)。
#[test]
fn products_survive_canonical_roundtrip() {
    use std::sync::atomic::{AtomicU32, Ordering};
    let (mut doc, sids) = fixture();
    let plan = pathfinder_multi_cmds(&mut doc, MultiOp::Divide, &sids).expect("分割成功");
    let mut undo = UndoStack::new();
    undo.push(&mut doc, plan.command).expect("apply");

    let out1 = render_project(&doc);
    // 落盘 → 导入 → 导出
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "vb-pf-rt-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (rel, content) in &out1.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let re = vb_doc::import::import_project(&dir).unwrap();
    let out2 = render_project(&re.doc);

    // L1 幂等:再导入 → 再导出必须逐字节一致
    let dir2 = dir.join("round2");
    std::fs::create_dir_all(&dir2).unwrap();
    for (rel, content) in &out2.files {
        let p = dir2.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let re2 = vb_doc::import::import_project(&dir2).unwrap();
    let out3 = render_project(&re2.doc);
    let f2 = out2.files.iter().find(|(r, _)| r == "index.html").unwrap();
    let f3 = out3.files.iter().find(|(r, _)| r == "index.html").unwrap();
    assert_eq!(f2.1, f3.1, "index.html 必须幂等(重导入 → 导出两次一致)");

    // L0 内容保留:3 件结果的 data-vb-id / 名称在重导出后不丢
    let f1 = out1.files.iter().find(|(r, _)| r == "index.html").unwrap();
    for needle in ["B 分割", "A 分割"] {
        assert!(
            f1.1.contains(needle) && f2.1.contains(needle),
            "{needle} 保留"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// 实机冒烟夹具生成器(不设 VB_SMOKE_DIR 时跳过,与 commands_yaml::gen
/// 同款门控):为 分割 / 修边 / 轮廓 各生成「操作前 / 操作后」两份项目,
/// 供 vellum-cli shot 出 PNG 对照。
#[test]
fn smoke_export() {
    let Some(dir) = std::env::var_os("VB_SMOKE_DIR").map(std::path::PathBuf::from) else {
        return;
    };
    for (op, zh) in [
        (MultiOp::Divide, "divide"),
        (MultiOp::Trim, "trim"),
        (MultiOp::Outline, "outline"),
    ] {
        let (mut doc, sids) = fixture();
        let before_dir = dir.join(zh).join("before");
        write_project(&render_project(&doc).files, &before_dir);

        let plan = pathfinder_multi_cmds(&mut doc, op, &sids).expect("运算成功");
        let mut undo = UndoStack::new();
        undo.push(&mut doc, plan.command).expect("apply");
        let after_dir = dir.join(zh).join("after");
        write_project(&render_project(&doc).files, &after_dir);
    }
}

/// 把导出文件表落盘(项目目录形态,供 vellum-cli 打开)。
fn write_project(files: &[(String, String)], dir: &std::path::Path) {
    for (rel, content) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
}
