//! 回归测试:2026-09 审查修复的 patch/协议缺陷。

use vb_agent::{apply_patch, PatchOp, PatchRequest};
use vb_doc::model::{Document, NodeKind};
use vb_doc::UndoStack;

fn req(ops: Vec<PatchOp>) -> PatchRequest {
    PatchRequest {
        base_rev: None,
        ops,
    }
}

fn ab0_sid(doc: &Document) -> String {
    doc.nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string()
}

/// duplicate 容器必须深拷贝全部子孙(此前 children: vec![] 只复制空壳)。
#[test]
fn duplicate_copies_subtree() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    // insert 一个带子级的组
    let ab_sid = ab0_sid(&doc);
    let out = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Insert {
            parent: ab_sid,
            index: None,
            node: vb_agent::InsertNodeSpec {
                tag: "div".into(),
                name: Some("组".into()),
                text: None,
                style: None,
                attrs: None,
                r#box: Some(vb_agent::BoxSpec {
                    x: 0.0,
                    y: 0.0,
                    w: 200.0,
                    h: 100.0,
                }),
            },
        }]),
    )
    .expect("insert 组失败");
    let group_sid = out.created_ids[0].clone();
    let group_id = doc.find_by_sid(&group_sid).unwrap();
    // 给组塞一个文本子级
    let out2 = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Insert {
            parent: group_sid.clone(),
            index: None,
            node: vb_agent::InsertNodeSpec {
                tag: "p".into(),
                name: None,
                text: Some("子文本".into()),
                style: None,
                attrs: None,
                r#box: None,
            },
        }]),
    )
    .expect("insert 子级失败");
    let child_sid = out2.created_ids[0].clone();

    let out3 = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Duplicate {
            id: group_sid.clone(),
            offset: None,
        }]),
    )
    .expect("duplicate 失败");
    let dup_sid = out3.created_ids[0].clone();
    assert_ne!(dup_sid, group_sid);
    let dup_id = doc.find_by_sid(&dup_sid).unwrap();
    assert_eq!(
        doc.nodes.get(dup_id).unwrap().children.len(),
        1,
        "副本必须带子级"
    );
    let dup_child = doc.nodes.get(dup_id).unwrap().children[0];
    assert!(matches!(
        doc.nodes.get(dup_child).unwrap().kind,
        NodeKind::Text { .. }
    ));
    // 副本子级的 sid 必须是新分配的
    assert_ne!(
        doc.nodes.get(dup_child).unwrap().sid.as_str(),
        child_sid,
        "副本子级 sid 必须重分配(身份全新)"
    );
    // 原组未受影响
    assert_eq!(doc.nodes.get(group_id).unwrap().children.len(), 1);
    // 撤销后副本整棵消失
    undo.undo(&mut doc).expect("撤销失败");
    assert!(doc.find_by_sid(&dup_sid).is_none());
    assert!(doc.find_by_sid(&child_sid).is_some(), "原组子级不受影响");
}

/// insert/duplicate/new_artboard 的新 sid 必须出现在 created_ids(此前永远为空)。
#[test]
fn created_ids_populated() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab_sid = ab0_sid(&doc);
    let out = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![
            PatchOp::NewArtboard {
                name: "画板 B".into(),
                w: 800.0,
                h: 600.0,
                after: None,
            },
            PatchOp::Insert {
                parent: ab_sid,
                index: None,
                node: vb_agent::InsertNodeSpec {
                    tag: "div".into(),
                    name: None,
                    text: None,
                    style: None,
                    attrs: None,
                    r#box: None,
                },
            },
        ]),
    )
    .expect("patch 失败");
    assert_eq!(
        out.created_ids.len(),
        2,
        "new_artboard + insert 都应上报 created_ids"
    );
    assert_eq!(
        doc.artboards.len(),
        2,
        "new_artboard 经 Insert 命令也要注册画板"
    );
}

/// 非法 CSS 声明(set_style)必须在编译期被拒绝,不得原样落盘。
#[test]
fn set_style_rejects_invalid_decl() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab_sid = ab0_sid(&doc);
    let out = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Insert {
            parent: ab_sid,
            index: None,
            node: vb_agent::InsertNodeSpec {
                tag: "div".into(),
                name: None,
                text: None,
                style: None,
                attrs: None,
                r#box: None,
            },
        }]),
    )
    .expect("insert 失败");
    let sid = out.created_ids[0].clone();
    let mut css = std::collections::BTreeMap::new();
    css.insert("background color".to_string(), "red".to_string());
    let err = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::SetStyle { id: sid, css }]),
    )
    .expect_err("非法属性名应被拒绝");
    assert!(err.to_string().contains("非法 CSS 声明"));
}

/// set_attr 写保留属性(class/style/data-vb-id/data-vb-name)必须被拒绝,
/// 否则导出产生重复 HTML 属性。
#[test]
fn set_attr_rejects_reserved_keys() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab_sid = ab0_sid(&doc);
    let out = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Insert {
            parent: ab_sid,
            index: None,
            node: vb_agent::InsertNodeSpec {
                tag: "div".into(),
                name: None,
                text: None,
                style: None,
                attrs: None,
                r#box: None,
            },
        }]),
    )
    .expect("insert 失败");
    let sid = out.created_ids[0].clone();
    let mut attrs = std::collections::BTreeMap::new();
    attrs.insert("data-vb-id".to_string(), "fake".to_string());
    attrs.insert("href".to_string(), "https://example.com".to_string());
    let err = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::SetAttr { id: sid, attrs }]),
    )
    .expect_err("保留属性应被拒绝");
    assert!(err.to_string().contains("保留属性"));
}

/// 跨画板 align 按画板分组各自对齐,不得把两块画板的本地坐标混算;
/// 落单成员跳过并给 warning。
#[test]
fn align_groups_by_artboard() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab_sid = ab0_sid(&doc);
    let out = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![
            PatchOp::NewArtboard {
                name: "画板 B".into(),
                w: 800.0,
                h: 600.0,
                after: None,
            },
            PatchOp::Insert {
                parent: ab_sid.clone(),
                index: None,
                node: vb_agent::InsertNodeSpec {
                    tag: "div".into(),
                    name: Some("A1".into()),
                    text: None,
                    style: None,
                    attrs: None,
                    r#box: Some(vb_agent::BoxSpec {
                        x: 100.0,
                        y: 0.0,
                        w: 50.0,
                        h: 50.0,
                    }),
                },
            },
            PatchOp::Insert {
                parent: ab_sid.clone(),
                index: None,
                node: vb_agent::InsertNodeSpec {
                    tag: "div".into(),
                    name: Some("A2".into()),
                    text: None,
                    style: None,
                    attrs: None,
                    r#box: Some(vb_agent::BoxSpec {
                        x: 300.0,
                        y: 40.0,
                        w: 50.0,
                        h: 50.0,
                    }),
                },
            },
            PatchOp::Insert {
                parent: "B".into(),
                index: None,
                node: vb_agent::InsertNodeSpec {
                    tag: "div".into(),
                    name: Some("B1".into()),
                    text: None,
                    style: None,
                    attrs: None,
                    r#box: Some(vb_agent::BoxSpec {
                        x: 10.0,
                        y: 10.0,
                        w: 50.0,
                        h: 50.0,
                    }),
                },
            },
        ]),
    );
    // 上一步可能因为 parent "B" 尚不存在而失败 —— new_artboard 与 insert 在同一事务里,
    // 编译期 find_by_sid("B") 找不到。分两次:先建画板,再插对象。
    let (a1, a2, b1) = match out {
        Ok(_) => panic!("跨依赖事务应失败"),
        Err(_) => {
            let out_ab = apply_patch(
                &mut doc,
                &mut undo,
                &req(vec![PatchOp::NewArtboard {
                    name: "画板 B".into(),
                    w: 800.0,
                    h: 600.0,
                    after: None,
                }]),
            )
            .expect("建画板失败");
            let ab1_sid = out_ab.created_ids[0].clone();
            let out_objs = apply_patch(
                &mut doc,
                &mut undo,
                &req(vec![
                    PatchOp::Insert {
                        parent: ab_sid.clone(),
                        index: None,
                        node: vb_agent::InsertNodeSpec {
                            tag: "div".into(),
                            name: Some("A1".into()),
                            text: None,
                            style: None,
                            attrs: None,
                            r#box: Some(vb_agent::BoxSpec {
                                x: 100.0,
                                y: 0.0,
                                w: 50.0,
                                h: 50.0,
                            }),
                        },
                    },
                    PatchOp::Insert {
                        parent: ab_sid.clone(),
                        index: None,
                        node: vb_agent::InsertNodeSpec {
                            tag: "div".into(),
                            name: Some("A2".into()),
                            text: None,
                            style: None,
                            attrs: None,
                            r#box: Some(vb_agent::BoxSpec {
                                x: 300.0,
                                y: 40.0,
                                w: 50.0,
                                h: 50.0,
                            }),
                        },
                    },
                    PatchOp::Insert {
                        parent: ab1_sid.clone(),
                        index: None,
                        node: vb_agent::InsertNodeSpec {
                            tag: "div".into(),
                            name: Some("B1".into()),
                            text: None,
                            style: None,
                            attrs: None,
                            r#box: Some(vb_agent::BoxSpec {
                                x: 10.0,
                                y: 10.0,
                                w: 50.0,
                                h: 50.0,
                            }),
                        },
                    },
                ]),
            )
            .expect("插对象失败");
            let sids = out_objs.changed_ids.clone();
            (sids[0].clone(), sids[1].clone(), sids[2].clone())
        }
    };
    let out_align = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Align {
            ids: vec![a1.clone(), a2.clone(), b1.clone()],
            mode: "left".into(),
            to: None,
        }]),
    )
    .expect("align 失败");
    // A1/A2 同画板对齐;B1 落单被跳过
    assert!(
        out_align.warnings.iter().any(|w| w.contains("B1")),
        "落单成员应产生 warning:{:?}",
        out_align.warnings
    );
    let g1 = doc.nodes.get(doc.find_by_sid(&a1).unwrap()).unwrap().geom;
    let g2 = doc.nodes.get(doc.find_by_sid(&a2).unwrap()).unwrap().geom;
    assert_eq!(g1.x, g2.x, "同画板成员对齐后左缘一致");
    assert_eq!(g1.x, 100.0, "对齐到集合最小 x=100");
    let g3 = doc.nodes.get(doc.find_by_sid(&b1).unwrap()).unwrap().geom;
    assert_eq!(g3.x, 10.0, "跨画板成员不得被拖进另一画板的坐标");
}

// ---------------------------------------------------------------------------
// 15 号计划 A1:root sid 守卫(root 曾击穿 MCP 主循环)+ 画板下限
// ---------------------------------------------------------------------------

fn root_sid(doc: &Document) -> String {
    doc.nodes.get(doc.root).unwrap().sid.as_str().to_string()
}

/// P0-1:order 作用于 root sid 必须返回结构化错误,而不是 panic。
#[test]
fn order_on_root_sid_is_rejected() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let rs = root_sid(&doc);
    let err = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Order {
            id: rs,
            to: "front".into(),
        }]),
    )
    .expect_err("order root 应被拒绝");
    assert!(err.to_string().contains("根节点"), "错误信息:{err}");
}

/// P0-1 家族:move / delete / ungroup 作用于 root 同样拒绝。
#[test]
fn structural_ops_on_root_are_rejected() {
    let mut doc = Document::new_default();
    let rs = root_sid(&doc);
    for (label, ops) in [
        (
            "move",
            vec![PatchOp::Move {
                id: rs.clone(),
                parent: ab0_sid(&doc),
                index: 0,
            }],
        ),
        ("delete", vec![PatchOp::Delete { id: rs.clone() }]),
        ("ungroup", vec![PatchOp::Ungroup { id: rs.clone() }]),
    ] {
        let mut undo = UndoStack::new();
        let err = apply_patch(&mut doc, &mut undo, &req(ops))
            .expect_err(&format!("{label} root 应被拒绝"));
        assert!(err.to_string().contains("根节点"), "{label} 错误信息:{err}");
    }
}

/// group 成员含 root 时拒绝;align 含 root 时跳过并 warning(不动 root geom)。
#[test]
fn group_align_root_member_handled() {
    let mut doc = Document::new_default();
    let rs = root_sid(&doc);
    let ab = ab0_sid(&doc);
    let mut undo = UndoStack::new();
    apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Insert {
            parent: ab.clone(),
            index: None,
            node: vb_agent::InsertNodeSpec {
                tag: "div".into(),
                name: None,
                text: None,
                style: None,
                attrs: None,
                r#box: Some(vb_agent::BoxSpec {
                    x: 10.0,
                    y: 10.0,
                    w: 50.0,
                    h: 50.0,
                }),
            },
        }]),
    )
    .expect("插对象失败");
    let obj = doc
        .nodes
        .get(doc.find_by_sid(&ab).unwrap())
        .unwrap()
        .children
        .iter()
        .find_map(|&c| {
            let n = doc.nodes.get(c).unwrap();
            (!matches!(n.kind, NodeKind::Artboard)).then(|| n.sid.as_str().to_string())
        })
        .expect("应有刚插入的对象");
    let root_geom = doc.nodes.get(doc.root).unwrap().geom;

    let err = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Group {
            ids: vec![obj.clone(), rs.clone()],
            name: None,
        }]),
    )
    .expect_err("group 含 root 应被拒绝");
    assert!(err.to_string().contains("根节点"), "错误信息:{err}");

    let out = apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Align {
            ids: vec![obj, rs.clone()],
            mode: "left".into(),
            to: None,
        }]),
    )
    .expect("align 含 root 应跳过而非报错");
    assert!(
        out.warnings.iter().any(|w| w.contains("根节点")),
        "应产生 root 跳过 warning:{:?}",
        out.warnings
    );
    assert_eq!(
        doc.nodes.get(doc.root).unwrap().geom,
        root_geom,
        "root geom 不得被 align 改动"
    );
}

/// P0-2(agent 侧):delete 最后一块画板被命令层守卫拒绝。
#[test]
fn delete_last_artboard_rejected() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab = ab0_sid(&doc);
    let err = apply_patch(&mut doc, &mut undo, &req(vec![PatchOp::Delete { id: ab }]))
        .expect_err("删除最后一块画板应被拒绝");
    assert!(err.to_string().contains("画板"), "错误信息:{err}");
    assert_eq!(doc.artboards.len(), 1, "画板数量不变");
}

/// G1(agent 侧):相邻两次 patch 目标集合相同也不得合并成一条 undo
/// (08 篇:一次 patch = 一条 undo)。
#[test]
fn consecutive_patches_stay_separate_undo_entries() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    let ab = ab0_sid(&doc);
    apply_patch(
        &mut doc,
        &mut undo,
        &req(vec![PatchOp::Insert {
            parent: ab,
            index: None,
            node: vb_agent::InsertNodeSpec {
                tag: "div".into(),
                name: None,
                text: None,
                style: None,
                attrs: None,
                r#box: Some(vb_agent::BoxSpec {
                    x: 0.0,
                    y: 0.0,
                    w: 50.0,
                    h: 50.0,
                }),
            },
        }]),
    )
    .expect("插入失败");
    let sid = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .children
        .iter()
        .find_map(|&c| {
            let n = doc.nodes.get(c).unwrap();
            (!matches!(n.kind, NodeKind::Artboard)).then(|| n.sid.as_str().to_string())
        })
        .expect("应有对象");

    // 两次相邻 patch 改同一对象(同目标集合 → 若不禁合并会被吞成一条)
    for color in ["#ff0000", "#0000ff"] {
        apply_patch(
            &mut doc,
            &mut undo,
            &req(vec![PatchOp::SetStyle {
                id: sid.clone(),
                css: vec![("background-color".into(), color.to_string())]
                    .into_iter()
                    .collect(),
            }]),
        )
        .expect("patch 失败");
    }
    // 第一次 undo 只回退第二次 patch(蓝→红);若合并则直接回到无色
    undo.undo(&mut doc).expect("撤销失败");
    let n = doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap();
    let has_red = n
        .style
        .iter()
        .any(|d| d.prop == "background-color" && d.value == "#f00");
    assert!(has_red, "第一次 undo 后应回到第一次 patch 的红色");
}
