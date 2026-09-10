//! 节点标志位(hidden / locked)与 HTML 的双向语义。
//!
//! - `hidden` 有 HTML 通道:`export` 写 `display: none`, `import` 读回为 hidden。
//!   这条通道必须**对称**,否则「隐藏 → 保存 → 重新打开」会出现
//!   "图层面板眼睛是睁着的,但对象在画布上不可见" —— 界面说谎。
//! - `locked` 是编辑器私有状态(ADR-010),**不进 HTML**;只在会话内有效。

mod common;

use vb_doc::commands::Command;
use vb_doc::export::render_project;
use vb_doc::model::Document;
use vb_doc::undo::UndoStack;

fn doc_with_inline(style: &str) -> Document {
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>旗标</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="aa0001" data-vb-name="画板">
    <div class="box" data-vb-id="aa0002" data-vb-name="盒子" style="{style}"></div>
  </section>
</body>
</html>
"#
    );
    common::import_files(&[("index.html", html.as_str())])
}

fn find_css(doc: &Document) -> String {
    render_project(doc)
        .files
        .into_iter()
        .find(|(p, _)| p == "styles/main.css")
        .map(|(_, c)| c)
        .unwrap_or_default()
}

const BASE: &str = "left: 10px; top: 20px; width: 30px; height: 40px;";

#[test]
fn hidden_serializes_to_display_none() {
    let mut doc = doc_with_inline(BASE);
    assert!(
        !doc.nodes
            .get(doc.find_by_sid("aa0002").unwrap())
            .unwrap()
            .hidden
    );

    let mut stack = UndoStack::new();
    stack
        .push(
            &mut doc,
            Command::SetFlags {
                sid: "aa0002".into(),
                hidden: Some(true),
                locked: None,
                old: None,
            },
        )
        .unwrap();
    let css = find_css(&doc);
    assert!(
        css.contains("display: none;"),
        "hidden 应落成 display: none:\n{css}"
    );
}

#[test]
fn hidden_reads_back_from_display_none() {
    let doc = doc_with_inline(&format!("display: none; {BASE}"));
    let nid = doc.find_by_sid("aa0002").unwrap();
    let n = doc.nodes.get(nid).unwrap();
    assert!(n.hidden, "display: none 应回读为 hidden = true");
    // 回读后 `display` 必须从 style 里摘掉,否则导出会与 hidden 双写
    assert!(
        !n.style.iter().any(|d| d.prop == "display"),
        "回读后 style 里不应残留 display:"
    );
}

#[test]
fn display_flex_is_not_hidden() {
    let doc = doc_with_inline(&format!("display: flex; {BASE}"));
    let nid = doc.find_by_sid("aa0002").unwrap();
    let n = doc.nodes.get(nid).unwrap();
    assert!(!n.hidden, "display: flex 不是隐藏");
    assert!(
        n.style
            .iter()
            .any(|d| d.prop == "display" && d.value == "flex"),
        "display: flex 必须原样保留"
    );
}

#[test]
fn locked_is_editor_private_not_in_html() {
    let mut doc = doc_with_inline(BASE);
    let mut stack = UndoStack::new();
    stack
        .push(
            &mut doc,
            Command::SetFlags {
                sid: "aa0002".into(),
                hidden: None,
                locked: Some(true),
                old: None,
            },
        )
        .unwrap();

    let files = render_project(&doc).files;
    let all: String = files.iter().map(|(_, c)| c.as_str()).collect();
    assert!(
        !all.contains("locked"),
        "locked 是编辑器私有状态(ADR-010),不得泄漏进 HTML/CSS"
    );
    assert!(
        all.contains("data-vb-id=\"aa0002\""),
        "锁定不应影响正常的 sid 输出"
    );
}

/// 与上面成对:隐藏必须真的在 HTML 里可还原,且 undo 后回到可见。
#[test]
fn hidden_full_cycle_reversible() {
    let mut doc = doc_with_inline(BASE);
    let mut stack = UndoStack::new();

    let before = find_css(&doc);
    stack
        .push(
            &mut doc,
            Command::SetFlags {
                sid: "aa0002".into(),
                hidden: Some(true),
                locked: None,
                old: None,
            },
        )
        .unwrap();
    let hidden_css = find_css(&doc);
    assert_ne!(before, hidden_css);

    stack.undo(&mut doc).unwrap();
    assert_eq!(find_css(&doc), before, "undo 后 CSS 应精确逆回");
}
