//! 命令**全变体**可逆性测试(ADR-0008 / 09 篇 §四)。
//!
//! 铁律:`apply` 之后 `undo` 必须**精确逆回**,`redo` 必须与首次 `apply` 等效。
//!
//! 这里的"等效"以 `render_project` 的产物做快照 —— 它覆盖结构、样式、几何、
//! 文本、属性、令牌的**全部用户可见结果**,比逐字段断言更强:任何"撤销后字段
//! 对了但落盘结果不对"的隐性遗漏都会被抓住。
//!
//! 覆盖面:`Command` 的 14 个变体逐一过一遍(10 篇门禁 2)。

use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::export::render_project;
use vb_doc::model::{Document, Geom, Node, NodeKind, NodeTree};
use vb_doc::undo::UndoStack;

fn snap(doc: &Document) -> Vec<(String, String)> {
    render_project(doc).files
}

/// 测试夹具:一个画板 + 一个文本节点 + 两个盒子。
struct Fx {
    doc: Document,
    ab: String,
    t: String,
    a: String,
    b: String,
}

fn put(
    doc: &mut Document,
    parent: &str,
    kind: NodeKind,
    name: &str,
    geom: Geom,
    tag: &str,
) -> String {
    let sid = doc.alloc_sid();
    let mut n = Node::new(kind, name, sid.clone());
    n.geom = geom;
    n.tag = tag.to_string();
    let pid = doc.find_by_sid(parent).unwrap();
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(pid);
    doc.nodes.get_mut(pid).unwrap().children.push(id);
    sid.as_str().to_string()
}

fn box_geom(x: f64, y: f64, w: f64, h: f64) -> Geom {
    Geom { x, y, w, h }
}

fn fixture() -> Fx {
    let mut doc = Document::new_default();
    let ab = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let t = put(
        &mut doc,
        &ab,
        NodeKind::Text {
            text: "原始文本".into(),
            mode: vb_doc::model::TextMode::Point,
            segments: Vec::new(),
        },
        "文本",
        box_geom(10.0, 10.0, 200.0, 40.0),
        "p",
    );
    let a = put(
        &mut doc,
        &ab,
        NodeKind::Box,
        "盒 A",
        box_geom(20.0, 80.0, 120.0, 60.0),
        "div",
    );
    let b = put(
        &mut doc,
        &ab,
        NodeKind::Box,
        "盒 B",
        box_geom(160.0, 80.0, 120.0, 60.0),
        "div",
    );
    Fx { doc, ab, t, a, b }
}

/// 应用 `pre` 后快照 → `cmd` → undo → redo 三段断言。
fn check_reversible(label: &str, mut doc: Document, pre: Vec<Command>, cmd: Command) {
    let mut stack = UndoStack::new();
    for p in pre {
        stack
            .push(&mut doc, p)
            .unwrap_or_else(|e| panic!("[{label}] 预处理命令失败:{e}"));
    }
    let s0 = snap(&doc);
    stack
        .push(&mut doc, cmd)
        .unwrap_or_else(|e| panic!("[{label}] apply 失败:{e}"));
    let s1 = snap(&doc);
    assert_ne!(s0, s1, "[{label}] apply 后落盘结果没变化,命令形同虚设");

    stack
        .undo(&mut doc)
        .unwrap_or_else(|e| panic!("[{label}] undo 失败:{e}"));
    let s2 = snap(&doc);
    assert_eq!(s2, s0, "[{label}] undo 未精确逆回");

    stack
        .redo(&mut doc)
        .unwrap_or_else(|e| panic!("[{label}] redo 失败:{e}"));
    let s3 = snap(&doc);
    assert_eq!(s3, s1, "[{label}] redo 与首次 apply 不等效");
}

// ---------- 14 个变体 ----------

#[test]
fn cmd_insert() {
    let mut fx = fixture();
    let sid = fx.doc.alloc_sid();
    let mut n = Node::new(NodeKind::Box, "新盒子", sid);
    n.geom = box_geom(300.0, 10.0, 60.0, 40.0);
    check_reversible(
        "Insert",
        fx.doc,
        vec![],
        Command::Insert {
            parent_sid: fx.ab.clone(),
            index: usize::MAX,
            tree: NodeTree {
                node: n,
                children: vec![],
            },
        },
    );
}

#[test]
fn cmd_delete() {
    let fx = fixture();
    check_reversible(
        "Delete",
        fx.doc,
        vec![],
        Command::Delete {
            target_sid: fx.b.clone(),
            captured: None,
        },
    );
}

#[test]
fn cmd_move() {
    let mut fx = fixture();
    // 先编组 A+B,再把 A 移回画板(跨父移动)
    let gsid = fx.doc.alloc_sid().as_str().to_string();
    let group = Command::Group {
        member_sids: vec![fx.a.clone(), fx.b.clone()],
        name: "编组".into(),
        group_sid: gsid.clone(),
        old_slots: None,
    };
    check_reversible(
        "Move",
        fx.doc,
        vec![group],
        Command::Move {
            sid: fx.a.clone(),
            new_parent_sid: fx.ab.clone(),
            new_index: usize::MAX,
            old: None,
        },
    );
}

#[test]
fn cmd_set_geom() {
    let fx = fixture();
    check_reversible(
        "SetGeom",
        fx.doc,
        vec![],
        Command::SetGeom {
            sid: fx.a.clone(),
            new: box_geom(20.0, 80.0, 260.0, 140.0),
            old: None,
            old_declared: None,
        },
    );
}

#[test]
fn cmd_set_style() {
    let fx = fixture();
    check_reversible(
        "SetStyle",
        fx.doc,
        vec![],
        Command::SetStyle {
            sid: fx.a.clone(),
            new: vec![
                Decl {
                    prop: "background-color".into(),
                    value: "#ff5a1f".into(),
                    important: false,
                },
                Decl {
                    prop: "border-radius".into(),
                    value: "8px".into(),
                    important: false,
                },
            ],
            old: None,
        },
    );
}

#[test]
fn cmd_set_text() {
    let fx = fixture();
    check_reversible(
        "SetText",
        fx.doc,
        vec![],
        Command::SetText {
            sid: fx.t.clone(),
            new: "改过的文本".into(),
            old: None,
        },
    );
}

#[test]
fn cmd_set_attrs() {
    let fx = fixture();
    check_reversible(
        "SetAttrs",
        fx.doc,
        vec![],
        Command::SetAttrs {
            sid: fx.a.clone(),
            new: vec![
                ("href".into(), "/buy".into()),
                ("aria-label".into(), "立即购买".into()),
            ],
            old: None,
        },
    );
}

#[test]
fn cmd_rename() {
    let fx = fixture();
    check_reversible(
        "Rename",
        fx.doc,
        vec![],
        Command::Rename {
            sid: fx.b.clone(),
            new: "改名后的盒子".into(),
            old: None,
        },
    );
}

#[test]
fn cmd_set_tag() {
    let fx = fixture();
    check_reversible(
        "SetTag",
        fx.doc,
        vec![],
        Command::SetTag {
            sid: fx.a.clone(),
            new: "section".into(),
            old: None,
        },
    );
}

#[test]
fn cmd_set_flags_hidden() {
    let fx = fixture();
    check_reversible(
        "SetFlags(hidden)",
        fx.doc,
        vec![],
        Command::SetFlags {
            sid: fx.a.clone(),
            hidden: Some(true),
            locked: None,
            old: None,
        },
    );
}

/// `locked` 是**编辑器私有状态**——ADR-010 明确「网格/参考线/历史」这类信息
/// 不进 HTML(只有 `data-vb-id` / `data-vb-name` / `data-vb-slice` 这类需要
/// 精确还原的数据才用 `data-vb-*` 承载)。所以它的可逆性在**字段层**验证;
/// `hidden` 才有 HTML 通道(`display: none`),由 `flags_roundtrip.rs` 覆盖。
#[test]
fn cmd_set_flags_locked_field_roundtrip() {
    let fx = fixture();
    let mut doc = fx.doc;
    let mut stack = UndoStack::new();
    let sid = fx.a.clone();
    let locked = |d: &Document| d.nodes.get(d.find_by_sid(&sid).unwrap()).unwrap().locked;
    assert!(!locked(&doc), "初始应为未锁定");

    stack
        .push(
            &mut doc,
            Command::SetFlags {
                sid: sid.clone(),
                hidden: None,
                locked: Some(true),
                old: None,
            },
        )
        .unwrap();
    assert!(locked(&doc), "apply 后应为已锁定");

    stack.undo(&mut doc).unwrap();
    assert!(!locked(&doc), "undo 后应回到未锁定");

    stack.redo(&mut doc).unwrap();
    assert!(locked(&doc), "redo 后应回到已锁定");
}

#[test]
fn cmd_group() {
    let mut fx = fixture();
    let gsid = fx.doc.alloc_sid().as_str().to_string();
    check_reversible(
        "Group",
        fx.doc,
        vec![],
        Command::Group {
            member_sids: vec![fx.a.clone(), fx.b.clone()],
            name: "新编组".into(),
            group_sid: gsid,
            old_slots: None,
        },
    );
}

#[test]
fn cmd_ungroup() {
    let mut fx = fixture();
    let gsid = fx.doc.alloc_sid().as_str().to_string();
    let pre = vec![Command::Group {
        member_sids: vec![fx.a.clone(), fx.b.clone()],
        name: "待解组".into(),
        group_sid: gsid.clone(),
        old_slots: None,
    }];
    check_reversible(
        "Ungroup",
        fx.doc,
        pre,
        Command::Ungroup {
            group_sid: gsid,
            captured: None,
        },
    );
}

#[test]
fn cmd_compound() {
    let fx = fixture();
    check_reversible(
        "Compound",
        fx.doc,
        vec![],
        Command::Compound {
            cmds: vec![
                Command::SetGeom {
                    sid: fx.a.clone(),
                    new: box_geom(0.0, 0.0, 50.0, 50.0),
                    old: None,
                    old_declared: None,
                },
                Command::Rename {
                    sid: fx.a.clone(),
                    new: "复合改名".into(),
                    old: None,
                },
            ],
        },
    );
}

#[test]
fn cmd_set_token() {
    let fx = fixture();
    check_reversible(
        "SetToken",
        fx.doc,
        vec![],
        Command::SetToken {
            name: "brand".into(),
            new: "#ff5a1f".into(),
            old: None,
        },
    );
}

/// 变体清单必须与 `CmdKind` 一一对应:漏一个就说明本文件该更新。
#[test]
fn all_command_variants_covered() {
    let mut fx = fixture();
    let gsid = fx.doc.alloc_sid().as_str().to_string();
    let mut n = Node::new(NodeKind::Box, "x", fx.doc.alloc_sid());
    n.geom = box_geom(0.0, 0.0, 1.0, 1.0);
    let samples = vec![
        Command::Insert {
            parent_sid: fx.ab.clone(),
            index: 0,
            tree: NodeTree {
                node: n,
                children: vec![],
            },
        },
        Command::Delete {
            target_sid: fx.b.clone(),
            captured: None,
        },
        Command::Move {
            sid: fx.a.clone(),
            new_parent_sid: fx.ab.clone(),
            new_index: 0,
            old: None,
        },
        Command::SetGeom {
            sid: fx.a.clone(),
            new: box_geom(0.0, 0.0, 1.0, 1.0),
            old: None,
            old_declared: None,
        },
        Command::SetStyle {
            sid: fx.a.clone(),
            new: vec![],
            old: None,
        },
        Command::SetText {
            sid: fx.t.clone(),
            new: "x".into(),
            old: None,
        },
        Command::SetAttrs {
            sid: fx.a.clone(),
            new: vec![],
            old: None,
        },
        Command::Rename {
            sid: fx.a.clone(),
            new: "x".into(),
            old: None,
        },
        Command::SetTag {
            sid: fx.a.clone(),
            new: "div".into(),
            old: None,
        },
        Command::SetFlags {
            sid: fx.a.clone(),
            hidden: Some(false),
            locked: None,
            old: None,
        },
        Command::Group {
            member_sids: vec![fx.a.clone()],
            name: "g".into(),
            group_sid: gsid,
            old_slots: None,
        },
        Command::Ungroup {
            group_sid: "000000".into(),
            captured: None,
        },
        Command::Compound { cmds: vec![] },
        Command::SetToken {
            name: "t".into(),
            new: "v".into(),
            old: None,
        },
    ];
    assert_eq!(
        samples.len(),
        14,
        "Command 变体数应为 14;若增删变体请同步本文件"
    );
    let mut kinds: Vec<String> = samples.iter().map(|c| format!("{:?}", c.kind())).collect();
    kinds.sort();
    kinds.dedup();
    assert_eq!(kinds.len(), 14, "存在未覆盖的变体:{kinds:?}");
}
