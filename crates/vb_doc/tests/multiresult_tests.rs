//! 多结果事务底座测试(05-3:一次操作 → 多节点)。
//!
//! 铁律同 `command_coverage.rs`:apply 后 undo 必须**精确逆回**(以
//! `render_project` 全产物快照断言),redo 必须与首次 apply 等效,结果
//! `data-vb-id`(sid)在 undo/redo 往返中保持不变。

use vb_doc::commands::{Command, MultiResultSlot};
use vb_doc::export::render_project;
use vb_doc::model::{Document, Geom, Node, NodeKind, NodeTree};
use vb_doc::undo::UndoStack;

fn snap(doc: &Document) -> Vec<(String, String)> {
    render_project(doc).files
}

/// 放一个盒子到指定父级末尾,返回 sid。
fn put(doc: &mut Document, parent: &str, name: &str, geom: Geom) -> String {
    let sid = doc.alloc_sid();
    let mut n = Node::new(NodeKind::Box, name, sid.clone());
    n.geom = geom;
    let pid = doc.find_by_sid(parent).unwrap();
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(pid);
    doc.nodes.get_mut(pid).unwrap().children.push(id);
    sid.as_str().to_string()
}

fn box_tree(doc: &mut Document, name: &str, geom: Geom) -> (String, NodeTree) {
    let sid = doc.alloc_sid();
    let mut n = Node::new(NodeKind::Box, name, sid.clone());
    n.geom = geom;
    (
        sid.as_str().to_string(),
        NodeTree {
            node: n,
            children: vec![],
        },
    )
}

/// 夹具:画板 + [A, B, C] 三个盒子(C 在最上)。返回 (doc, sid 列表)。
fn fixture() -> (Document, Vec<String>) {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let sids = vec![
        put(
            &mut doc,
            &ab,
            "A",
            Geom {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
        ),
        put(
            &mut doc,
            &ab,
            "B",
            Geom {
                x: 200.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
        ),
        put(
            &mut doc,
            &ab,
            "C",
            Geom {
                x: 400.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
        ),
    ];
    (doc, sids)
}

/// 2 进 3 出事务:undo 精确逆回,redo 与首次 apply 等效,sid 稳定。
#[test]
fn multi_result_undo_redo_roundtrip() {
    let (mut doc, sids) = fixture();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let before = snap(&doc);

    let (r1, t1) = box_tree(
        &mut doc,
        "R1",
        Geom {
            x: 0.0,
            y: 200.0,
            w: 50.0,
            h: 50.0,
        },
    );
    let (r2, t2) = box_tree(
        &mut doc,
        "R2",
        Geom {
            x: 100.0,
            y: 200.0,
            w: 50.0,
            h: 50.0,
        },
    );
    let (r3, t3) = box_tree(
        &mut doc,
        "R3",
        Geom {
            x: 200.0,
            y: 200.0,
            w: 50.0,
            h: 50.0,
        },
    );
    let result_sids = vec![r1.clone(), r2.clone(), r3.clone()];

    let cmd = Command::MultiResult {
        op: "路径查找器:分割".into(),
        src_sids: vec![sids[0].clone(), sids[1].clone()],
        results: vec![t1, t2, t3],
        slot: MultiResultSlot::ReplaceAnchor,
        captured: None,
    };
    let mut undo = UndoStack::new();
    undo.push(&mut doc, cmd.clone()).expect("apply 成功");

    // ① 结构:2 源已删,3 结果在锚点(A 的原槽位,即画板 children 首位)
    for s in &sids[..2] {
        assert!(doc.find_by_sid(s).is_none(), "源 {s} 应已删除");
    }
    for r in &result_sids {
        assert!(doc.find_by_sid(r).is_some(), "结果 {r} 应存在");
    }
    let kids: Vec<String> = doc
        .nodes
        .get(doc.find_by_sid(&ab).unwrap())
        .unwrap()
        .children
        .iter()
        .map(|&c| doc.nodes.get(c).unwrap().sid.as_str().to_string())
        .collect();
    assert_eq!(&kids[..3], &result_sids[..], "结果按序占据锚点原槽位");
    assert_eq!(kids[3], sids[2], "C 顺移");

    // ② undo:精确逆回(全产物快照相等)
    undo.undo(&mut doc).expect("undo 成功");
    assert_eq!(snap(&doc), before, "撤销后必须与操作前逐字节一致");

    // ③ redo:与首次 apply 等效(快照相等)
    undo.redo(&mut doc).expect("redo 成功");
    let after = snap(&doc);
    undo.undo(&mut doc).expect("undo2 成功");
    undo.redo(&mut doc).expect("redo2 成功");
    assert_eq!(snap(&doc), after, "重复 undo/redo 后仍一致");

    // ④ sid 稳定:redo 后结果 sid 与首次 apply 相同(undo 已把结果提出、
    // redo 复用命令里预分配的同一批 sid)
    for r in &result_sids {
        assert!(doc.find_by_sid(r).is_some(), "结果 {r} 的 data-vb-id 稳定");
    }
    for s in &sids[..2] {
        assert!(doc.find_by_sid(s).is_none(), "源 {s} 在 redo 后仍不存在");
    }
}

/// OnTop 策略:结果插到锚点父级末尾(z 序最上)。
#[test]
fn multi_result_ontop_policy() {
    let (mut doc, sids) = fixture();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let (r1, t1) = box_tree(&mut doc, "R1", Geom::default());
    let cmd = Command::MultiResult {
        op: "路径查找器:修边".into(),
        src_sids: vec![sids[0].clone(), sids[1].clone()],
        results: vec![t1],
        slot: MultiResultSlot::OnTop,
        captured: None,
    };
    let mut undo = UndoStack::new();
    undo.push(&mut doc, cmd).expect("apply 成功");
    let kids: Vec<String> = doc
        .nodes
        .get(doc.find_by_sid(&ab).unwrap())
        .unwrap()
        .children
        .iter()
        .map(|&c| doc.nodes.get(c).unwrap().sid.as_str().to_string())
        .collect();
    assert_eq!(kids.last().unwrap(), &r1, "OnTop:结果在末尾");
    assert_eq!(&kids[0], &sids[2], "其余兄弟保持原相对顺序");

    // undo 后源归位(原索引),结果消失
    undo.undo(&mut doc).expect("undo 成功");
    assert!(doc.find_by_sid(&r1).is_none());
    assert!(doc.find_by_sid(&sids[0]).is_some());
}

/// 跨父级源:结果全部落到锚点(首位源)的父级;undo 精确逆回。
#[test]
fn multi_result_cross_parent_sources() {
    let (mut doc, sids) = fixture();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    // 组挂在画板末尾,把 C 移进组(源 = A[画板] + C[组] 跨父级)
    let (g, gt) = {
        let sid = doc.alloc_sid();
        let n = Node::new(NodeKind::Group, "G", sid.clone());
        (
            sid.as_str().to_string(),
            NodeTree {
                node: n,
                children: vec![],
            },
        )
    };
    let ins = Command::Insert {
        parent_sid: ab.clone(),
        index: 1,
        tree: gt,
    };
    let mut undo = UndoStack::new();
    undo.push(&mut doc, ins).expect("建组");
    undo.push(
        &mut doc,
        Command::Move {
            sid: sids[2].clone(),
            new_parent_sid: g.clone(),
            new_index: 0,
            old: None,
        },
    )
    .expect("C 入组");
    let before = snap(&doc);

    let (r1, t1) = box_tree(
        &mut doc,
        "R1",
        Geom {
            x: 10.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
        },
    );
    let cmd = Command::MultiResult {
        op: "路径查找器:轮廓".into(),
        src_sids: vec![sids[0].clone(), sids[2].clone()],
        results: vec![t1],
        slot: MultiResultSlot::ReplaceAnchor,
        captured: None,
    };
    undo.push(&mut doc, cmd).expect("apply 成功");
    assert!(doc.find_by_sid(&r1).is_some());
    assert!(doc.find_by_sid(&sids[0]).is_none());
    assert!(doc.find_by_sid(&sids[2]).is_none());

    undo.undo(&mut doc).expect("undo 成功");
    assert_eq!(snap(&doc), before, "跨父级源必须精确逆回(C 回组内)");
    assert!(doc.find_by_sid(&r1).is_none());
}

/// 前置校验:结果 sid 已占用 → 整条事务报错,文档不动。
#[test]
fn multi_result_rejects_occupied_result_sid() {
    let (mut doc, sids) = fixture();
    let before = snap(&doc);
    let (r1, t1) = box_tree(&mut doc, "R1", Geom::default());
    // 结果 sid 故意与现有源冲突
    let mut t_clash = t1;
    {
        let mut clash = t_clash.node.clone();
        clash.sid = vb_common::StableId::parse(&sids[2]).unwrap();
        t_clash.node = clash;
    }
    let _ = r1;
    let cmd = Command::MultiResult {
        op: "路径查找器:分割".into(),
        src_sids: vec![sids[0].clone()],
        results: vec![t_clash],
        slot: MultiResultSlot::ReplaceAnchor,
        captured: None,
    };
    let mut undo = UndoStack::new();
    assert!(undo.push(&mut doc, cmd).is_err(), "占用 sid 必须报错");
    assert_eq!(snap(&doc), before, "失败事务不得改动文档");
}

/// 前置校验:源互为祖先/后代 → 报错(提取顺序无法原子化)。
#[test]
fn multi_result_rejects_ancestor_descendant_sources() {
    let (mut doc, sids) = fixture();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let (g, gt) = {
        let sid = doc.alloc_sid();
        let n = Node::new(NodeKind::Group, "G", sid.clone());
        (
            sid.as_str().to_string(),
            NodeTree {
                node: n,
                children: vec![],
            },
        )
    };
    let mut undo = UndoStack::new();
    undo.push(
        &mut doc,
        Command::Insert {
            parent_sid: ab.clone(),
            index: 1,
            tree: gt,
        },
    )
    .expect("建组");
    undo.push(
        &mut doc,
        Command::Move {
            sid: sids[0].clone(),
            new_parent_sid: g.clone(),
            new_index: 0,
            old: None,
        },
    )
    .expect("A 入组");
    let before = snap(&doc);

    let (_r1, t1) = box_tree(&mut doc, "R1", Geom::default());
    let cmd = Command::MultiResult {
        op: "路径查找器:分割".into(),
        // 组 与 组内成员 A 互为祖先/后代
        src_sids: vec![g.clone(), sids[0].clone()],
        results: vec![t1],
        slot: MultiResultSlot::ReplaceAnchor,
        captured: None,
    };
    assert!(undo.push(&mut doc, cmd).is_err(), "祖先/后代源必须报错");
    assert_eq!(snap(&doc), before, "失败事务不得改动文档");
}
