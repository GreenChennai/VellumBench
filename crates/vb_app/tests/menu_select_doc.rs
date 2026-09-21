//! 阶段 5「选择」菜单的**文档状态级**验收门(副文档 06 §06-1)。
//!
//! 选择类命令的判据全部是接收 `&Document` 的纯函数(见
//! `vb_app::app::menu_commands`),因此可以脱离 GUI 逐条断言:
//! 反向 / 下一个对象 / 相同填充色 / 对象 → 文本·锁定·隐藏。

use vb_app::app::menu_commands::{
    all_of_targets, artboard_of, inverse_targets, same_targets, step_target, subtree, AllOf,
    SameKey,
};
use vb_css::Decl;
use vb_doc::model::{Document, Geom, Node, NodeKind, TextMode};

fn add(doc: &mut Document, kind: NodeKind, name: &str) -> String {
    let ab = doc.artboards.first().copied().unwrap();
    let sid = doc.alloc_sid();
    let mut n = Node::new(kind, name, sid.clone());
    n.geom = Geom {
        x: 0.0,
        y: 0.0,
        w: 50.0,
        h: 50.0,
    };
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    doc.nodes.get_mut(ab).unwrap().children.push(id);
    sid.as_str().to_string()
}

fn text_node(doc: &mut Document, name: &str, body: &str) -> String {
    add(
        doc,
        NodeKind::Text {
            text: body.into(),
            segments: Vec::new(),
            mode: TextMode::Point,
        },
        name,
    )
}

fn set_style(doc: &mut Document, sid: &str, css: &str) {
    let id = doc.find_by_sid(sid).unwrap();
    doc.nodes.get_mut(id).unwrap().style = vec![Decl::parse(css).unwrap()];
}

// ────────────────── 反向 / 步进 ──────────────────

#[test]
fn inverse_selects_unselected_objects_in_artboard() {
    let mut doc = Document::new_default();
    let a = add(&mut doc, NodeKind::Box, "甲");
    let b = add(&mut doc, NodeKind::Box, "乙");
    let _c = add(&mut doc, NodeKind::Box, "丙");

    let hits = inverse_targets(&doc, std::slice::from_ref(&a));
    assert_eq!(hits.len(), 2, "反向应得到另外两个对象:{hits:?}");
    assert!(!hits.contains(&a));
    assert!(hits.contains(&b));

    // 全部选中 → 反向为空
    let all: Vec<String> = vec![a.clone(), b.clone(), hits[1].clone()];
    assert!(inverse_targets(&doc, &all).is_empty());
}

#[test]
fn step_target_cycles_dfs_order() {
    let mut doc = Document::new_default();
    let a = add(&mut doc, NodeKind::Box, "甲");
    let b = add(&mut doc, NodeKind::Box, "乙");
    let c = add(&mut doc, NodeKind::Box, "丙");

    let (first, i0, n) = step_target(&doc, std::slice::from_ref(&a), true).unwrap();
    assert_eq!((first.as_str(), i0, n), (b.as_str(), 1, 3));
    let (second, i1, _) = step_target(&doc, std::slice::from_ref(&b), true).unwrap();
    assert_eq!((second.as_str(), i1), (c.as_str(), 2));
    // 末尾回绕到首个
    let (wrap, i2, _) = step_target(&doc, std::slice::from_ref(&c), true).unwrap();
    assert_eq!((wrap.as_str(), i2), (a.as_str(), 0));
    // 反向:首个 → 末尾
    let (back, i3, _) = step_target(&doc, std::slice::from_ref(&a), false).unwrap();
    assert_eq!((back.as_str(), i3), (c.as_str(), 2));
}

// ────────────────── 相同 → ──────────────────

#[test]
fn same_fill_matches_only_equal_fill() {
    let mut doc = Document::new_default();
    let a = add(&mut doc, NodeKind::Box, "甲");
    let b = add(&mut doc, NodeKind::Box, "乙");
    let c = add(&mut doc, NodeKind::Box, "丙");
    set_style(&mut doc, &a, "background-color: #ff0000");
    set_style(&mut doc, &b, "background-color: #f00"); // 最短 hex 规范化后等价
    set_style(&mut doc, &c, "background-color: #00ff00");

    let hits = same_targets(&doc, &a, SameKey::Fill);
    assert_eq!(hits.len(), 2, "同色应命中甲乙:{hits:?}");
    assert!(hits.contains(&a) && hits.contains(&b) && !hits.contains(&c));
}

#[test]
fn same_stroke_width_matches_only_equal_width() {
    let mut doc = Document::new_default();
    let a = add(&mut doc, NodeKind::Box, "甲");
    let b = add(&mut doc, NodeKind::Box, "乙");
    let c = add(&mut doc, NodeKind::Box, "丙");
    set_style(&mut doc, &a, "border-width: 2px");
    set_style(&mut doc, &b, "border-width: 2px");
    set_style(&mut doc, &c, "border-width: 8px");

    let hits = same_targets(&doc, &a, SameKey::StrokeWidth);
    assert_eq!(hits, vec![a, b]);
}

#[test]
fn same_reports_empty_when_reference_lacks_property() {
    let mut doc = Document::new_default();
    let a = add(&mut doc, NodeKind::Box, "甲");
    assert!(same_targets(&doc, &a, SameKey::Fill).is_empty());
}

// ────────────────── 对象 → ──────────────────

#[test]
fn all_of_picks_text_locked_hidden() {
    let mut doc = Document::new_default();
    let t1 = text_node(&mut doc, "标题", "甲");
    let t2 = text_node(&mut doc, "正文", "乙");
    let bx = add(&mut doc, NodeKind::Box, "盒");
    doc.nodes
        .get_mut(doc.find_by_sid(&bx).unwrap())
        .unwrap()
        .locked = true;
    let hid = add(&mut doc, NodeKind::Box, "隐");
    doc.nodes
        .get_mut(doc.find_by_sid(&hid).unwrap())
        .unwrap()
        .hidden = true;

    let texts = all_of_targets(&doc, AllOf::Text);
    assert_eq!(texts.len(), 2);
    assert!(texts.contains(&t1) && texts.contains(&t2));

    assert_eq!(all_of_targets(&doc, AllOf::Locked), vec![bx]);
    assert_eq!(all_of_targets(&doc, AllOf::Hidden), vec![hid]);
}

// ────────────────── 基础遍历 ──────────────────

#[test]
fn subtree_and_artboard_lookup() {
    let mut doc = Document::new_default();
    let a = add(&mut doc, NodeKind::Box, "甲");
    let nid = doc.find_by_sid(&a).unwrap();
    let ab = artboard_of(&doc, nid).unwrap();
    assert!(matches!(
        doc.nodes.get(ab).unwrap().kind,
        NodeKind::Artboard
    ));
    let all = subtree(&doc, ab);
    assert!(all.contains(&nid), "子树应含自身后代");
    assert!(all.len() >= 2, "画板 + 至少一个子节点");
}
