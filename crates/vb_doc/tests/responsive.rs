//! 05-5 断点与伪类(台账 09-F)门禁:
//! ① @media 序列化往返幂等(含编辑产物);② 断点内属性写入/读回/撤销;
//! ③ 伪类(:hover)规则生成;④ 复杂媒体查询 / 歧义选择器回冻结不静默丢。

mod common;

use common::{css, find};
use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::export::render_project;
use vb_doc::import::import_html;
use vb_doc::undo::UndoStack;

fn page(extra_css: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>断点</title>
<style>
.ab {{ width: 1200px; height: 600px; }}
.card {{ position: absolute; left: 40px; top: 40px; width: 400px; height: 240px; font-size: 24px; }}
{extra_css}</style></head>
<body>
  <section class="vb-artboard ab" data-vb-id="aa0001" data-vb-name="画板">
    <div class="card" data-vb-id="cc0001" data-vb-name="卡片"></div>
  </section>
</body>
</html>
"#
    )
}

/// ① 导入既有 @media → 结构化 → canonical 导出 → 再导入 → 再导出,字节相等。
#[test]
fn media_roundtrip_idempotent() {
    let src = page("@media (max-width: 768px) {\n  .card { width: 100%; }\n}\n");
    let dir = std::env::temp_dir().join(format!("vb-resp-1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), &src).unwrap();
    let r1 = vb_doc::import::import_project(&dir).unwrap();
    assert_eq!(r1.doc.media_rules.len(), 1, "媒体块应结构化");
    let f1 = render_project(&r1.doc);
    let css1 = css(&f1.files);
    assert!(css1.contains("@media (max-width: 768px)"));
    assert!(css1.contains("width: 100%;"));

    // 二次往返(canonical 导出落盘后重导入)
    std::fs::write(dir.join("index.html"), find(&f1.files, "index.html")).unwrap();
    std::fs::create_dir_all(dir.join("styles")).unwrap();
    std::fs::write(dir.join("styles/main.css"), css(&f1.files)).unwrap();
    let r2 = vb_doc::import::import_project(&dir).unwrap();
    assert_eq!(r2.doc.media_rules.len(), 1, "canonical 块应再结构化");
    let f2 = render_project(&r2.doc);
    assert_eq!(css(&f1.files), css(&f2.files), "L1 必须幂等");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// ② 断点内属性写入(SetMediaStyle)→ media_rules 读回;apply→undo→redo
/// 以 render_project 快照为判据(ADR-0008);空声明 = 删除覆盖。
#[test]
fn set_media_style_write_undo_redo() {
    let src = page("");
    let r = import_html(&src, std::path::Path::new(".")).unwrap();
    let mut doc = r.doc;
    let mut undo = UndoStack::default();
    let sid = "cc0001";

    let push = |doc: &mut vb_doc::Document, undo: &mut UndoStack, decls: Vec<Decl>| {
        undo.push(
            doc,
            Command::SetMediaStyle {
                sid: sid.into(),
                max_width: 375,
                new: decls,
                old: None,
            },
        )
        .unwrap();
    };
    push(
        &mut doc,
        &mut undo,
        vec![Decl {
            prop: "width".into(),
            value: "80%".into(),
            important: false,
        }],
    );
    assert_eq!(doc.media_rules.len(), 1);
    assert_eq!(doc.media_rules[0].max_width, 375);
    assert_eq!(doc.media_rules[0].sid, sid);
    assert_eq!(doc.media_rules[0].decls[0].value, "80%");

    // 导出应含 canonical 断点块,且位于节点规则之后(级联覆盖语义)
    let css_out = css(&render_project(&doc).files);
    let media_pos = css_out
        .find("@media (max-width: 375px)")
        .expect("断点块缺失");
    let card_pos = css_out.find(".card {").expect("节点规则缺失");
    assert!(media_pos > card_pos, "断点块必须后于节点规则(覆盖生效)");
    assert!(css_out.contains("width: 80%;"));

    // 撤销 → 覆盖删除;重做 → 等效恢复
    undo.undo(&mut doc).unwrap();
    assert!(doc.media_rules.is_empty(), "撤销应删除覆盖条目");
    undo.redo(&mut doc).unwrap();
    assert_eq!(doc.media_rules.len(), 1);

    // 空声明 = 删除(可撤销)
    undo.push(
        &mut doc,
        Command::SetMediaStyle {
            sid: sid.into(),
            max_width: 375,
            new: vec![],
            old: None,
        },
    )
    .unwrap();
    assert!(doc.media_rules.is_empty(), "空声明应删除条目");
    undo.undo(&mut doc).unwrap();
    assert_eq!(doc.media_rules.len(), 1, "撤销删除应恢复条目");
}

/// ③ 伪类(:hover)规则生成:顶层 `.cls:hover` 导入结构化,导出 canonical。
#[test]
fn hover_rule_generation() {
    let src = page(".card:hover { background-color: #f5f5f5; }\n");
    let r = import_html(&src, std::path::Path::new(".")).unwrap();
    assert_eq!(r.doc.pseudo_rules.len(), 1, ":hover 应结构化");
    assert_eq!(r.doc.pseudo_rules[0].pseudo, "hover");
    let css_out = css(&render_project(&r.doc).files);
    assert!(css_out.contains(".card:hover {"), "canonical 伪类规则缺失");
    assert!(css_out.contains("background-color: #f5f5f5;"));
    // SetPseudoStyle 写入/删除
    let mut doc = r.doc;
    doc.nodes.get(doc.find_by_sid("cc0001").unwrap()).unwrap();
    vb_doc::commands::Command::SetPseudoStyle {
        sid: "cc0001".into(),
        pseudo: "hover".into(),
        new: vec![Decl {
            prop: "opacity".into(),
            value: "0.8".into(),
            important: false,
        }],
        old: None,
    }
    .apply(&mut doc)
    .unwrap();
    assert_eq!(doc.pseudo_rules[0].decls[0].prop, "opacity");
}

/// ④ 不支持项不静默:screen and 组合 / :focus-visible / 歧义首类 → 原文冻结。
#[test]
fn unsupported_media_and_pseudo_stay_frozen() {
    let src = page(
        "@media screen and (max-width: 768px) {\n  .card { width: 90%; }\n}\n\
         .card:focus-visible { outline: 2px; }\n@supports (display: grid) {\n  .card { display: grid; }\n}\n",
    );
    let r = import_html(&src, std::path::Path::new(".")).unwrap();
    assert!(
        r.doc.media_rules.is_empty(),
        "screen and 组合不应结构化:{:?}",
        r.doc.media_rules
    );
    assert!(r.doc.pseudo_rules.is_empty(), ":focus-visible 不在本轮闭环");
    let joined = r.doc.raw_css.join("\n");
    assert!(
        joined.contains("screen and (max-width: 768px)"),
        "组合查询冻结保留"
    );
    assert!(joined.contains(".card:focus-visible"), "其余伪类冻结保留");
    assert!(joined.contains("@supports"), "其他 at-rule 冻结保留");
    let f = render_project(&r.doc);
    let css_out = css(&f.files);
    assert!(css_out.contains("screen and (max-width: 768px)"));
    assert!(css_out.contains(".card:focus-visible"));
}
