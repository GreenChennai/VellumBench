//! 回归测试:2026-09 审查修复的结构/撤销缺陷。
//! 每个用例对应一个已修复的真实 bug,防止回潮。

use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, Node, NodeKind, NodeTree};
use vb_doc::UndoStack;

fn add_box(doc: &mut Document, parent_id: vb_doc::model::NodeId, x: f64, y: f64) -> String {
    let sid = doc.alloc_sid();
    let sid_str = sid.as_str().to_string();
    let mut n = Node::new(NodeKind::Box, "盒", sid);
    n.geom = Geom {
        x,
        y,
        w: 100.0,
        h: 50.0,
    };
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(parent_id);
    doc.nodes.get_mut(parent_id).unwrap().children.push(id);
    sid_str
}

fn exec(doc: &mut Document, undo: &mut UndoStack, cmd: Command) {
    undo.push(doc, cmd).expect("命令应用失败");
}

/// Insert 命令创建的画板必须进 doc.artboards(此前 patch/GUI 新建画板不可见不导出)。
#[test]
fn insert_artboard_registers_in_artboards() {
    let mut doc = Document::new_empty("t", "zh-CN");
    let mut undo = UndoStack::new();
    let sid = doc.alloc_sid();
    let mut n = Node::new(NodeKind::Artboard, "画板 B", sid);
    n.geom = Geom {
        x: 0.0,
        y: 980.0,
        w: 800.0,
        h: 600.0,
    };
    let root_sid = doc.nodes.get(doc.root).unwrap().sid.as_str().to_string();
    exec(
        &mut doc,
        &mut undo,
        Command::Insert {
            parent_sid: root_sid,
            index: usize::MAX,
            tree: NodeTree {
                node: n,
                children: vec![],
            },
        },
    );
    assert_eq!(doc.artboards.len(), 1);
    assert!(
        doc.nodes.get(doc.artboards[0]).is_some(),
        "artboards[0] 应为存活节点"
    );

    undo.undo(&mut doc).expect("撤销失败");
    assert!(doc.artboards.is_empty(), "撤销插入后画板应从注册表移除");
    undo.redo(&mut doc).expect("重做失败");
    assert_eq!(doc.artboards.len(), 1);
}

/// 删除画板后 doc.artboards 不得残留悬挂 NodeId;撤销后画板回到导出列表。
#[test]
fn delete_artboard_no_dangling_id() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    // 文档至少保留一块画板(15 号计划 A1 不变量),先补第二块再删第一块
    doc.new_artboard("画板 2", 800.0, 600.0);
    let ab_sid = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    exec(
        &mut doc,
        &mut undo,
        Command::Delete {
            target_sid: ab_sid,
            captured: None,
        },
    );
    assert_eq!(doc.artboards.len(), 1, "删一块后仍剩一块");
    undo.undo(&mut doc).expect("撤销删除失败");
    assert_eq!(doc.artboards.len(), 2, "撤销后画板应恢复注册");
    let ab = doc.artboards[0];
    assert!(doc.nodes.get(ab).is_some());
    assert_eq!(doc.nodes.get(ab).unwrap().geom.w, 1440.0);
}

/// move 把节点移入自身/后代必须被拒绝(否则场景图成环,遍历栈溢出)。
#[test]
fn move_into_own_subtree_rejected() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab = doc.artboards[0];
    let parent_sid = add_box(&mut doc, ab, 0.0, 0.0);
    let parent_id = doc.find_by_sid(&parent_sid).unwrap();
    let child_sid = add_box(&mut doc, parent_id, 10.0, 10.0);

    let err = undo
        .push(
            &mut doc,
            Command::Move {
                sid: parent_sid.clone(),
                new_parent_sid: child_sid,
                new_index: 0,
                old: None,
            },
        )
        .expect_err("移入后代应被拒绝");
    // 文档未被破坏:父子关系保持
    assert_eq!(
        doc.nodes.get(parent_id).unwrap().parent,
        Some(doc.artboards[0])
    );
    assert!(matches!(
        err,
        vb_doc::VbError::Conflict(_) | vb_doc::VbError::NoSuchNode(_)
    ));
}

/// Compound 中途失败必须整体回滚,文档不留半条事务。
#[test]
fn compound_failure_rolls_back() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab = doc.artboards[0];
    let a_sid = add_box(&mut doc, ab, 0.0, 0.0);
    let b_sid = add_box(&mut doc, ab, 200.0, 0.0);

    // 先删 A,再改 A 的几何(第二条必失败)→ 整条事务应回滚
    let err = undo
        .push(
            &mut doc,
            Command::Compound {
                cmds: vec![
                    Command::Delete {
                        target_sid: a_sid.clone(),
                        captured: None,
                    },
                    Command::SetGeom {
                        sid: a_sid.clone(),
                        new: Geom {
                            x: 1.0,
                            y: 1.0,
                            w: 1.0,
                            h: 1.0,
                        },
                        old: None,
                    },
                ],
            },
        )
        .expect_err("复合事务应失败");
    assert!(!err.to_string().is_empty());
    assert!(
        doc.find_by_sid(&a_sid).is_some(),
        "事务失败后 A 必须还在(半条事务被回滚)"
    );
    assert!(doc.find_by_sid(&b_sid).is_some());
}

/// 编组:成员坐标重定基到组系,渲染位置不变;撤销后回到原坐标与原层序。
#[test]
fn group_rebases_members_and_undo_restores() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab = doc.artboards[0];
    let a = add_box(&mut doc, ab, 100.0, 80.0);
    let b = add_box(&mut doc, ab, 300.0, 200.0);
    let a_id = doc.find_by_sid(&a).unwrap();
    let b_id = doc.find_by_sid(&b).unwrap();

    // 乱序传入(降序 index),撤销后仍须恢复原 z 序 [a, b]
    let group_sid = doc.alloc_sid().as_str().to_string();
    exec(
        &mut doc,
        &mut undo,
        Command::Group {
            member_sids: vec![b.clone(), a.clone()],
            name: "编组".into(),
            group_sid: group_sid.clone(),
            old_slots: None,
        },
    );
    let gid = doc.find_by_sid(&group_sid).unwrap();
    let g = doc.nodes.get(gid).unwrap();
    assert_eq!((g.geom.x, g.geom.y), (100.0, 80.0), "组原点=成员最小 x/y");
    // 成员渲染位置 = 组原点 + 组系坐标 → 必须等于原坐标
    let ma = doc.nodes.get(a_id).unwrap();
    let mb = doc.nodes.get(b_id).unwrap();
    assert_eq!(
        (g.geom.x + ma.geom.x, g.geom.y + ma.geom.y),
        (100.0, 80.0),
        "成员 A 世界位置不得因编组漂移"
    );
    assert_eq!(
        (g.geom.x + mb.geom.x, g.geom.y + mb.geom.y),
        (300.0, 200.0),
        "成员 B 世界位置不得因编组漂移"
    );

    undo.undo(&mut doc).expect("撤销编组失败");
    let ab_children: Vec<String> = doc.nodes.get(doc.artboards[0]).unwrap().children[0..2]
        .iter()
        .map(|&c| doc.nodes.get(c).unwrap().sid.as_str().to_string())
        .collect();
    assert_eq!(
        ab_children,
        vec![a.clone(), b.clone()],
        "撤销后 z 序必须恢复为插入时的 [a, b]"
    );
    let ga = doc.nodes.get(doc.find_by_sid(&a).unwrap()).unwrap().geom;
    assert_eq!((ga.x, ga.y), (100.0, 80.0), "撤销后成员坐标应回到原父级系");
    undo.redo(&mut doc).expect("重做编组失败");
    let g2 = doc.nodes.get(doc.find_by_sid(&group_sid).unwrap()).unwrap();
    let ma2 = doc.nodes.get(doc.find_by_sid(&a).unwrap()).unwrap().geom;
    assert_eq!((g2.geom.x + ma2.x, g2.geom.y + ma2.y), (100.0, 80.0));
}

/// 解组:成员坐标加回组原点,世界位置不变;撤销恢复整组。
#[test]
fn ungroup_rebases_members_back() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab = doc.artboards[0];
    let a = add_box(&mut doc, ab, 100.0, 80.0);
    let group_sid = doc.alloc_sid().as_str().to_string();
    exec(
        &mut doc,
        &mut undo,
        Command::Group {
            member_sids: vec![a.clone()],
            name: "编组".into(),
            group_sid: group_sid.clone(),
            old_slots: None,
        },
    );
    exec(
        &mut doc,
        &mut undo,
        Command::Ungroup {
            group_sid: group_sid.clone(),
            captured: None,
        },
    );
    let a_id = doc.find_by_sid(&a).unwrap();
    let g = doc.nodes.get(a_id).unwrap().geom;
    assert_eq!(
        (g.x, g.y),
        (100.0, 80.0),
        "解组后成员坐标应回到原父级系(世界位置不变)"
    );
    undo.undo(&mut doc).expect("撤销解组失败");
    assert!(
        doc.find_by_sid(&group_sid).is_some(),
        "解组撤销后编组应恢复"
    );
    undo.redo(&mut doc).expect("重做解组失败");
    let g2 = doc.nodes.get(doc.find_by_sid(&a).unwrap()).unwrap().geom;
    assert_eq!((g2.x, g2.y), (100.0, 80.0));
}

/// 同 sid 连续两次 SetVector 在合并窗口内:undo 回到最初,redo 必须到最新值
/// (此前栈顶 new 未被替换,redo 恢复中间值吞掉第二次编辑)。
#[test]
fn merged_setvector_redo_reaches_latest() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab = doc.artboards[0];
    let sid = doc.alloc_sid();
    let sid_str = sid.as_str().to_string();
    let mut n = Node::new(
        NodeKind::Vector {
            path: kurbo::BezPath::new(),
        },
        "路径",
        sid,
    );
    n.geom = Geom {
        x: 0.0,
        y: 0.0,
        w: 10.0,
        h: 10.0,
    };
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    doc.nodes.get_mut(ab).unwrap().children.push(id);

    let p1 = {
        let mut p = kurbo::BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 0.0));
        p
    };
    let p2 = {
        let mut p = kurbo::BezPath::new();
        p.move_to((0.0, 0.0));
        p.line_to((10.0, 10.0));
        p
    };
    undo.merging_enabled = true;
    exec(
        &mut doc,
        &mut undo,
        Command::SetVector {
            sid: sid_str.clone(),
            new: p1.clone(),
            old: None,
        },
    );
    exec(
        &mut doc,
        &mut undo,
        Command::SetVector {
            sid: sid_str.clone(),
            new: p2.clone(),
            old: None,
        },
    );
    undo.undo(&mut doc).expect("撤销失败");
    undo.redo(&mut doc).expect("重做失败");
    let nid = doc.find_by_sid(&sid_str).unwrap();
    let cur = match &doc.nodes.get(nid).unwrap().kind {
        NodeKind::Vector { path } => path.clone(),
        _ => panic!("应为矢量节点"),
    };
    assert_eq!(cur.to_svg(), p2.to_svg(), "redo 必须恢复最后一次编辑的路径");
}

/// SetToken 空值 = 删除;撤销按原索引恢复,令牌顺序不漂移。
#[test]
fn settoken_removes_and_restores_in_place() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    doc.tokens = vec![
        ("brand-1".into(), "#ff0000".into()),
        ("brand-2".into(), "#00ff00".into()),
    ];
    exec(
        &mut doc,
        &mut undo,
        Command::SetToken {
            name: "brand-1".into(),
            new: String::new(),
            old: None,
        },
    );
    assert_eq!(
        doc.tokens,
        vec![("brand-2".to_string(), "#00ff00".to_string())]
    );
    undo.undo(&mut doc).expect("撤销失败");
    assert_eq!(
        doc.tokens,
        vec![
            ("brand-1".to_string(), "#ff0000".to_string()),
            ("brand-2".to_string(), "#00ff00".to_string())
        ],
        "撤销后令牌应按原索引恢复"
    );
}

/// 空成员编组被拒绝(此前会产生无父孤儿节点,find_by_sid 可见但渲染不可见)。
#[test]
fn group_with_no_members_rejected() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let group_sid = doc.alloc_sid().as_str().to_string();
    let err = undo.push(
        &mut doc,
        Command::Group {
            member_sids: vec![],
            name: "编组".into(),
            group_sid,
            old_slots: None,
        },
    );
    assert!(err.is_err());
}
