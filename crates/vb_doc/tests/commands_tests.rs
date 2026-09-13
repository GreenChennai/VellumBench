//! 命令模式 Undo/Redo 测试(ADR-0008:apply/revert 精确可逆,sid 寻址跨结构变更稳定)。

use vb_common::units::fmt_num;
use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, Node, NodeKind};
use vb_doc::undo::UndoStack;

fn make_box(doc: &mut Document, parent: &str, x: f64, y: f64) -> String {
    let sid = doc.alloc_sid();
    let mut n = Node::new(NodeKind::Box, format!("盒 {}", sid.as_str()), sid.clone());
    n.geom = Geom {
        x,
        y,
        w: 100.0,
        h: 80.0,
    };
    let pid = doc.find_by_sid(parent).unwrap();
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(pid);
    doc.nodes.get_mut(pid).unwrap().children.push(id);
    sid.as_str().to_string()
}

fn geom_of(doc: &Document, sid: &str) -> Geom {
    doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().geom
}

fn name_of(doc: &Document, sid: &str) -> String {
    doc.nodes
        .get(doc.find_by_sid(sid).unwrap())
        .unwrap()
        .name
        .clone()
}

fn text_of(doc: &Document, sid: &str) -> String {
    doc.nodes
        .get(doc.find_by_sid(sid).unwrap())
        .unwrap()
        .text()
        .unwrap_or("<non-text>")
        .to_string()
}

fn set_geom(sid: &str, g: Geom) -> Command {
    Command::SetGeom {
        sid: sid.to_string(),
        new: g,
        old: None,
    }
}

#[test]
fn geom_undo_redo_idempotent() {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let a = make_box(&mut doc, &ab, 10.0, 10.0);
    let mut stack = UndoStack::new();

    let g0 = geom_of(&doc, &a);
    stack
        .push(
            &mut doc,
            set_geom(
                &a,
                Geom {
                    x: 200.0,
                    y: 300.0,
                    w: 50.0,
                    h: 60.0,
                },
            ),
        )
        .unwrap();
    assert_eq!(geom_of(&doc, &a).x, 200.0);

    stack.undo(&mut doc).unwrap();
    assert_eq!(geom_of(&doc, &a), g0);

    stack.redo(&mut doc).unwrap();
    assert_eq!(geom_of(&doc, &a).x, 200.0);

    // 多轮往返
    for _ in 0..5 {
        stack.undo(&mut doc).unwrap();
        stack.redo(&mut doc).unwrap();
    }
    assert_eq!(geom_of(&doc, &a).x, 200.0);
}

#[test]
fn delete_undo_restores_with_same_sid() {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let a = make_box(&mut doc, &ab, 10.0, 10.0);
    let mut stack = UndoStack::new();

    stack
        .push(
            &mut doc,
            Command::Delete {
                target_sid: a.clone(),
                captured: None,
            },
        )
        .unwrap();
    assert!(doc.find_by_sid(&a).is_none(), "删除后 sid 不应存在");

    stack.undo(&mut doc).unwrap();
    assert!(doc.find_by_sid(&a).is_some(), "撤销后同 sid 恢复");
    assert_eq!(geom_of(&doc, &a).x, 10.0);

    stack.redo(&mut doc).unwrap();
    assert!(doc.find_by_sid(&a).is_none());

    stack.undo(&mut doc).unwrap();
    assert!(doc.find_by_sid(&a).is_some());
}

#[test]
fn insert_undo_redo() {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let mut stack = UndoStack::new();

    let sid = doc.alloc_sid();
    let mut n = Node::new(NodeKind::Box, "新盒子", sid.clone());
    n.geom = Geom {
        x: 0.0,
        y: 0.0,
        w: 40.0,
        h: 40.0,
    };
    let tree = vb_doc::model::NodeTree {
        node: n,
        children: vec![],
    };
    stack
        .push(
            &mut doc,
            Command::Insert {
                parent_sid: ab.clone(),
                index: 0,
                tree,
            },
        )
        .unwrap();
    assert!(doc.find_by_sid(sid.as_str()).is_some());
    stack.undo(&mut doc).unwrap();
    assert!(doc.find_by_sid(sid.as_str()).is_none());
    stack.redo(&mut doc).unwrap();
    assert!(doc.find_by_sid(sid.as_str()).is_some());
}

#[test]
fn move_and_group_undo() {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let a = make_box(&mut doc, &ab, 10.0, 10.0);
    let b = make_box(&mut doc, &ab, 200.0, 10.0);
    let mut stack = UndoStack::new();

    // 编组
    let group_sid = doc.alloc_sid().as_str().to_string();
    stack
        .push(
            &mut doc,
            Command::Group {
                member_sids: vec![a.clone(), b.clone()],
                name: "CTA 组".into(),
                group_sid: group_sid.clone(),
                old_slots: None,
            },
        )
        .unwrap();
    let gid = doc.find_by_sid(&group_sid).unwrap();
    let gparent = doc.nodes.get(gid).unwrap().parent.unwrap();
    assert_eq!(doc.nodes.get(gparent).unwrap().sid.as_str(), ab);
    assert_eq!(doc.nodes.get(gid).unwrap().children.len(), 2);
    // 成员的父级是编组
    assert_eq!(
        doc.nodes
            .get(doc.find_by_sid(&a).unwrap())
            .unwrap()
            .parent
            .unwrap(),
        gid
    );

    // 撤销编组:成员回到画板原槽位
    stack.undo(&mut doc).unwrap();
    assert!(doc.find_by_sid(&group_sid).is_none());
    assert_eq!(
        doc.nodes
            .get(doc.find_by_sid(&a).unwrap())
            .unwrap()
            .parent
            .unwrap(),
        doc.find_by_sid(&ab).unwrap()
    );
    assert_eq!(geom_of(&doc, &a).x, 10.0);
    assert_eq!(geom_of(&doc, &b).x, 200.0);

    // 重做
    stack.redo(&mut doc).unwrap();
    assert!(doc.find_by_sid(&group_sid).is_some());
    assert_eq!(
        doc.nodes
            .get(doc.find_by_sid(&group_sid).unwrap())
            .unwrap()
            .children
            .len(),
        2
    );
}

#[test]
fn ungroup_undo_redo() {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let a = make_box(&mut doc, &ab, 10.0, 10.0);
    let b = make_box(&mut doc, &ab, 200.0, 10.0);
    let group_sid = doc.alloc_sid().as_str().to_string();
    let mut stack = UndoStack::new();
    stack
        .push(
            &mut doc,
            Command::Group {
                member_sids: vec![a.clone(), b.clone()],
                name: "组".into(),
                group_sid: group_sid.clone(),
                old_slots: None,
            },
        )
        .unwrap();

    stack
        .push(
            &mut doc,
            Command::Ungroup {
                group_sid: group_sid.clone(),
                captured: None,
            },
        )
        .unwrap();
    assert!(doc.find_by_sid(&group_sid).is_none());
    assert!(doc.find_by_sid(&a).is_some(), "解散后成员保留");
    assert_eq!(
        doc.nodes
            .get(doc.find_by_sid(&a).unwrap())
            .unwrap()
            .parent
            .unwrap(),
        doc.find_by_sid(&ab).unwrap()
    );

    stack.undo(&mut doc).unwrap();
    assert!(doc.find_by_sid(&group_sid).is_some(), "撤销解散:编组恢复");
    assert_eq!(
        doc.nodes
            .get(doc.find_by_sid(&group_sid).unwrap())
            .unwrap()
            .children
            .len(),
        2
    );

    stack.redo(&mut doc).unwrap();
    assert!(doc.find_by_sid(&group_sid).is_none());
    assert!(doc.find_by_sid(&a).is_some());
}

#[test]
fn merge_window_collapses_geom_sets() {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let a = make_box(&mut doc, &ab, 0.0, 0.0);
    let mut stack = UndoStack::new();

    let g0 = geom_of(&doc, &a);
    for i in 1..=10 {
        stack
            .push(
                &mut doc,
                set_geom(
                    &a,
                    Geom {
                        x: i as f64,
                        y: 0.0,
                        w: 100.0,
                        h: 80.0,
                    },
                ),
            )
            .unwrap();
    }
    // 10 次连续 SetGeom → 1 条 undo;撤销一次回到最初
    stack.undo(&mut doc).unwrap();
    assert_eq!(geom_of(&doc, &a), g0);
    assert!(!stack.can_undo());
}

#[test]
fn text_set_and_rename() {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let sid = doc.alloc_sid();
    let mut n = Node::new(
        NodeKind::Text {
            text: "旧文案".into(),
            mode: vb_doc::model::TextMode::Point,
        },
        "主标题",
        sid.clone(),
    );
    n.tag = "h1".into();
    n.geom = Geom {
        x: 10.0,
        y: 10.0,
        w: 300.0,
        h: 40.0,
    };
    let pid = doc.find_by_sid(&ab).unwrap();
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(pid);
    doc.nodes.get_mut(pid).unwrap().children.push(id);
    let mut stack = UndoStack::new();

    stack
        .push(
            &mut doc,
            Command::SetText {
                sid: sid.as_str().to_string(),
                new: "新文案".into(),
                old: None,
            },
        )
        .unwrap();
    assert_eq!(text_of(&doc, sid.as_str()), "新文案");
    stack
        .push(
            &mut doc,
            Command::Rename {
                sid: sid.as_str().to_string(),
                new: "Hero 主标题".into(),
                old: None,
            },
        )
        .unwrap();
    assert_eq!(name_of(&doc, sid.as_str()), "Hero 主标题");

    stack.undo(&mut doc).unwrap();
    assert_eq!(name_of(&doc, sid.as_str()), "主标题");
    stack.undo(&mut doc).unwrap();
    assert_eq!(text_of(&doc, sid.as_str()), "旧文案");
}

#[test]
fn compound_is_single_undo() {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let a = make_box(&mut doc, &ab, 0.0, 0.0);
    let mut stack = UndoStack::new();

    stack
        .push_compound(
            &mut doc,
            vec![
                set_geom(
                    &a,
                    Geom {
                        x: 50.0,
                        y: 50.0,
                        w: 100.0,
                        h: 80.0,
                    },
                ),
                Command::Rename {
                    sid: a.clone(),
                    new: "改名".into(),
                    old: None,
                },
            ],
        )
        .unwrap();
    assert_eq!(geom_of(&doc, &a).x, 50.0);
    assert_eq!(name_of(&doc, &a), "改名");

    stack.undo(&mut doc).unwrap();
    assert_eq!(geom_of(&doc, &a).x, 0.0);
    assert_eq!(name_of(&doc, &a), format!("盒 {a}")); // make_box 的命名规则
}

#[test]
fn fmt_num_smoke() {
    assert_eq!(fmt_num(3.0), "3");
}

#[allow(dead_code)]
fn unused_decl_helper() -> Decl {
    Decl {
        prop: "opacity".into(),
        value: "1".into(),
        important: false,
    }
}

// ---------------------------------------------------------------------------
// 15 号计划 A1 / P0-2:文档至少保留一块画板(Delete 命令层硬守卫)
// ---------------------------------------------------------------------------

#[test]
fn delete_last_artboard_blocked() {
    let mut doc = Document::new_default();
    let mut stack = UndoStack::new();
    let ab = doc.artboards[0];
    let sid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
    let err = stack
        .push(&mut doc, Command::Delete {
            target_sid: sid,
            captured: None,
        })
        .expect_err("删除最后一块画板应被拒绝");
    assert!(err.to_string().contains("画板"), "错误信息:{err}");
    assert_eq!(doc.artboards.len(), 1);
    assert!(doc.nodes.get(ab).is_some(), "画板节点仍在");
}

#[test]
fn delete_second_artboard_allowed() {
    let mut doc = Document::new_default();
    let mut stack = UndoStack::new();
    let second = doc.new_artboard("画板 2", 800.0, 600.0);
    let second_sid = doc.nodes.get(second).unwrap().sid.as_str().to_string();
    stack
        .push(
            &mut doc,
            Command::Delete {
                target_sid: second_sid,
                captured: None,
            },
        )
        .expect("有两块画板时删除其中一块应成功");
    assert_eq!(doc.artboards.len(), 1);
    // 撤销后画板回来
    stack.undo(&mut doc).unwrap();
    assert_eq!(doc.artboards.len(), 2);
}

// ---------------------------------------------------------------------------
// 15 号计划 A3 / R5+R6:编组 z 序补偿与成员校验
// ---------------------------------------------------------------------------

use vb_doc::VbError;

/// R5:[A,B,C] 框选 A、B 编组 → [G,C](此前 [C,G],组越过未选中的 C)。
#[test]
fn group_lands_at_top_member_z_order() {
    let mut doc = Document::new_default();
    let ab = doc.artboards[0];
    let ab_sid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
    let a = make_box(&mut doc, &ab_sid, 0.0, 0.0);
    let b = make_box(&mut doc, &ab_sid, 10.0, 10.0);
    let _c = make_box(&mut doc, &ab_sid, 20.0, 20.0);
    let mut stack = UndoStack::new();
    let gid = doc.alloc_sid().as_str().to_string();
    stack
        .push(
            &mut doc,
            Command::Group {
                member_sids: vec![a, b],
                name: "组".into(),
                group_sid: gid.clone(),
                old_slots: None,
            },
        )
        .expect("编组应成功");
    let children: Vec<String> = doc.nodes.get(ab).unwrap().children.iter()
        .map(|&c| doc.nodes.get(c).unwrap().sid.as_str().to_string())
        .collect();
    assert_eq!(children.len(), 2);
    assert_eq!(
        children[0], gid,
        "编组应落在最上层成员原位(在 C 之前),实际 {children:?}"
    );
}

/// R6:跨父级编组被拒绝(此前静默产出错乱几何,成员视觉瞬移)。
#[test]
fn group_rejects_cross_parent_members() {
    let mut doc = Document::new_default();
    let ab0 = doc.artboards[0];
    let ab0_sid = doc.nodes.get(ab0).unwrap().sid.as_str().to_string();
    let ab1 = doc.new_artboard("画板 2", 800.0, 600.0);
    let ab1_sid = doc.nodes.get(ab1).unwrap().sid.as_str().to_string();
    let a = make_box(&mut doc, &ab0_sid, 0.0, 0.0);
    let b = make_box(&mut doc, &ab1_sid, 0.0, 0.0);
    let mut stack = UndoStack::new();
    let gid = doc.alloc_sid().as_str().to_string();
    let err = stack
        .push(
            &mut doc,
            Command::Group {
                member_sids: vec![a, b],
                name: "组".into(),
                group_sid: gid,
                old_slots: None,
            },
        )
        .expect_err("跨父级编组应被拒绝");
    assert!(err.to_string().contains("父级"), "{err}");
}

/// R6:祖先+后代编组被拒绝(此前坐标重定基错乱)。
#[test]
fn group_rejects_ancestor_descendant() {
    let mut doc = Document::new_default();
    let ab = doc.artboards[0];
    let ab_sid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
    let parent = make_box(&mut doc, &ab_sid, 0.0, 0.0);
    let child = make_box(&mut doc, &parent, 10.0, 10.0);
    let mut stack = UndoStack::new();
    let gid = doc.alloc_sid().as_str().to_string();
    let err = stack
        .push(
            &mut doc,
            Command::Group {
                member_sids: vec![parent, child],
                name: "组".into(),
                group_sid: gid,
                old_slots: None,
            },
        )
        .expect_err("祖先+后代编组应被拒绝");
    assert!(err.to_string().contains("祖先"), "{err}");
}

/// R6:画板不能编入组(否则可绕过「至少一块画板」删除守卫)。
#[test]
fn group_rejects_artboard_member() {
    let mut doc = Document::new_default();
    let ab0 = doc.artboards[0];
    let ab0_sid = doc.nodes.get(ab0).unwrap().sid.as_str().to_string();
    let ab1 = doc.new_artboard("画板 2", 800.0, 600.0);
    let a = make_box(&mut doc, &ab0_sid, 0.0, 0.0);
    let b = doc.nodes.get(ab1).unwrap().sid.as_str().to_string();
    let mut stack = UndoStack::new();
    let gid = doc.alloc_sid().as_str().to_string();
    let err = stack
        .push(
            &mut doc,
            Command::Group {
                member_sids: vec![a, b],
                name: "组".into(),
                group_sid: gid,
                old_slots: None,
            },
        )
        .expect_err("画板入组应被拒绝");
    assert!(err.to_string().contains("画板"), "{err}");
}

#[allow(dead_code)]
fn ensure_vberror_import_used(_: Option<VbError>) {}
