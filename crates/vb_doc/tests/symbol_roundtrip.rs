//! 05-8 门禁测试(副文档 05 §4.8,台账 09-H,ADR-VB-L10)。
//!
//! 门禁清单:
//! ① 改主件 → 3 个实例同步一致(内容断言);
//! ② 覆盖字段在同步时保留;
//! ③ 往返 L0 / L1 幂等(另有语料 `corpus/23-symbol-defs.html`);
//! ④ 导出不因组件失败、不引入 JS(HTML 断言;kiln 链路复用同一渲染);
//! ⑤ Detach 后无残留标记。
//!
//! 附:实机冒烟产物落 `%TEMP%\vb-iter\phase5d\`(同步前后 HTML 对比、
//! detach 前后片段、导出 HTML),供人工核验「无 JS、可 diff」。

mod common;

use common::{check_l0_l1, css, find};
use vb_doc::commands::Command;
use vb_doc::export::render_project;
use vb_doc::import::{import_html, import_project};
use vb_doc::model::{Document, NodeKind};
use vb_doc::symbol::{
    symbol_create_commands, symbol_detach_commands, symbol_reset_overrides_commands,
    ATTR_OVERRIDES, ATTR_SYMBOL, ATTR_SYMBOL_REF, SYMBOL_DEF_CLASS,
};
use vb_doc::undo::UndoStack;

const CARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <title>符号组件</title>
  <style>
    .page { position: relative; width: 800px; height: 600px; }
    .sym-card { position: absolute; width: 220px; height: 90px; background-color: #ffffff; border: 1px solid #222222; border-radius: 8px; }
    .sym-title { position: absolute; left: 16px; top: 12px; width: 180px; height: 28px; font-size: 18px; font-weight: 700; color: #222222; }
  </style>
</head>
<body>
  <section class="vb-artboard page" data-vb-id="p0a11" data-vb-name="页面">
    <div class="sym-card" style="left: 40px; top: 40px" data-vb-id="i1b22" data-vb-name="卡片 1" data-vb-symbol="卡片" data-vb-symbol-ref="sy100">
      <p class="sym-title" data-vb-id="i1c33" data-vb-name="标题">标准标题</p>
    </div>
    <div class="sym-card" style="left: 300px; top: 40px" data-vb-id="i2b22" data-vb-name="卡片 2" data-vb-symbol="卡片" data-vb-symbol-ref="sy100">
      <p class="sym-title" data-vb-id="i2c33" data-vb-name="标题">标准标题</p>
    </div>
    <div class="sym-card" style="left: 560px; top: 40px" data-vb-id="i3b22" data-vb-name="卡片 3" data-vb-symbol="卡片" data-vb-symbol-ref="sy100" data-vb-symbol-overrides='["0:text"]'>
      <p class="sym-title" data-vb-id="i3c33" data-vb-name="标题">我的特例</p>
    </div>
  </section>
  <div class="vb-symbol-defs" hidden data-vb-id="sy100" data-vb-name="卡片">
    <div class="sym-card" style="left: 0px; top: 0px" data-vb-id="sy110" data-vb-name="卡片">
      <p class="sym-title" data-vb-id="sy120" data-vb-name="标题">标准标题</p>
    </div>
  </div>
</body>
</html>
"#;

/// 导入门控夹具:返回 (文档, 实例根 sid 表, 主件标题 sid)。
fn import_fixture() -> (Document, Vec<String>, String) {
    let r = import_html(CARD_HTML, std::path::Path::new(".")).unwrap();
    let doc = r.doc;
    let defs = doc.nodes.get(doc.defs_root).unwrap().children.clone();
    assert_eq!(defs.len(), 1, "定义区应识别出 1 个主件容器");
    let container = defs[0];
    let assert_n = doc.nodes.get(container).unwrap();
    assert_eq!(assert_n.name, "卡片");
    assert_eq!(assert_n.attrs.get("hidden").map(String::as_str), Some(""));
    let proto = doc.nodes.get(container).unwrap().children[0];
    let title = doc.nodes.get(proto).unwrap().children[0];
    let title_sid = doc.nodes.get(title).unwrap().sid.as_str().to_string();
    let insts: Vec<String> = ["i1b22", "i2b22", "i3b22"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    for s in &insts {
        let id = doc.find_by_sid(s).unwrap();
        let n = doc.nodes.get(id).unwrap();
        assert_eq!(
            n.attrs.get(ATTR_SYMBOL_REF).map(String::as_str),
            Some("sy100"),
            "实例 {s} 引用标记"
        );
        assert!(!vb_doc::symbol::is_in_defs(&doc, id), "实例必须在画板内");
    }
    (doc, insts, title_sid)
}

/// 实例根第 0 个子节点的文本。
fn child_text(doc: &Document, sid: &str) -> String {
    let id = doc.find_by_sid(sid).unwrap();
    let tid = doc.nodes.get(id).unwrap().children[0];
    match &doc.nodes.get(tid).unwrap().kind {
        NodeKind::Text { text, .. } => text.clone(),
        k => panic!("应为文本节点,实际 {k:?}"),
    }
}

/// 门禁①+②:改主件 → 3 实例同步一致;覆盖字段保留。
#[test]
fn gate_main_edit_syncs_three_instances_and_keeps_override() {
    let (mut doc, insts, title_sid) = import_fixture();
    let mut undo = UndoStack::new();

    // 主件标题编辑(经 push 收口 → 自动 SymbolSync)
    undo.push(
        &mut doc,
        Command::SetText {
            sid: title_sid.clone(),
            new: "新标题".into(),
            old: None,
        },
    )
    .unwrap();
    assert_eq!(child_text(&doc, &insts[0]), "新标题");
    assert_eq!(child_text(&doc, &insts[1]), "新标题");
    assert_eq!(child_text(&doc, &insts[2]), "我的特例", "覆盖字段必须保留");

    // 覆盖键登记核验(实例 3 保留原 JSON,不被同步重写丢掉)
    let i3 = doc.find_by_sid(&insts[2]).unwrap();
    let ov = doc
        .nodes
        .get(i3)
        .unwrap()
        .attrs
        .get(ATTR_OVERRIDES)
        .cloned()
        .unwrap_or_default();
    assert!(ov.contains("0:text"), "覆盖列表丢失:{ov}");

    // 再编辑:实例 2(无覆盖)跟随,实例 3 仍保留
    undo.push(
        &mut doc,
        Command::SetText {
            sid: title_sid,
            new: "第三版".into(),
            old: None,
        },
    )
    .unwrap();
    assert_eq!(child_text(&doc, &insts[0]), "第三版");
    assert_eq!(child_text(&doc, &insts[2]), "我的特例");

    // 几何重定基:子树整体替换后,实例 1(画板内 left:40px)的标题
    // 必须仍在 40+16=56 —— 同步不是"贴回主件坐标"而是整体平移
    {
        let iid = doc.find_by_sid(&insts[0]).unwrap();
        let tid = doc.nodes.get(iid).unwrap().children[0];
        let gx = doc.nodes.get(tid).unwrap().geom.x;
        assert!((gx - 56.0).abs() < 0.5, "实例子节点几何未重定基:x={gx}");
    }

    // 撤销(编辑+同步整条)→ 重做:结果 sid 稳定、内容一致
    undo.undo(&mut doc).unwrap();
    assert_eq!(child_text(&doc, &insts[0]), "新标题");
    undo.redo(&mut doc).unwrap();
    assert_eq!(child_text(&doc, &insts[0]), "第三版");
    assert_eq!(child_text(&doc, &insts[2]), "我的特例");
}

/// 门禁③:符号文档往返 L0 / L1 幂等。
#[test]
fn gate_roundtrip_l0_l1() {
    check_l0_l1(
        "符号组件",
        CARD_HTML,
        &[
            "vb-symbol-defs".to_string(),
            r#"data-vb-symbol-ref="sy100""#.to_string(),
            "我的特例".to_string(),
        ],
    )
    .unwrap();
}

/// 门禁④:导出不引入 JS、定义区以 hidden 呈现、CSS 规则覆盖原型节点。
#[test]
fn gate_export_no_js_and_hidden_defs() {
    let (doc, _, _) = import_fixture();
    let out = render_project(&doc);
    let html = find(&out.files, "index.html");
    assert!(!html.contains("<script"), "导出不得引入 JS");
    assert!(html.contains(SYMBOL_DEF_CLASS), "定义区标记类缺失");
    assert!(html.contains("hidden="), "定义区必须带原生 hidden 属性");
    assert!(html.contains(ATTR_SYMBOL), "实例标记缺失");
    let css_out = css(&out.files);
    assert!(
        css_out.contains("sym-title") || css_out.contains("vb-el-"),
        "原型/实例的样式规则必须照常输出"
    );
}

/// 门禁⑤:Detach 后无残留标记;undo 恢复;重置覆盖还原主件内容。
#[test]
fn gate_detach_and_reset() {
    let (mut doc, insts, title_sid) = import_fixture();
    let mut undo = UndoStack::new();

    // 分离实例 2:去标记,内容原样
    let cmd = symbol_detach_commands(&doc, &insts[1]).unwrap();
    undo.push(&mut doc, cmd).unwrap();
    {
        let id = doc.find_by_sid(&insts[1]).unwrap();
        let n = doc.nodes.get(id).unwrap();
        assert!(!n.attrs.contains_key(ATTR_SYMBOL), "残留主件名标记");
        assert!(!n.attrs.contains_key(ATTR_SYMBOL_REF), "残留引用标记");
        assert!(!n.attrs.contains_key(ATTR_OVERRIDES));
    }
    assert_eq!(child_text(&doc, &insts[1]), "标准标题", "分离不改变内容");
    undo.undo(&mut doc).unwrap();
    {
        let id = doc.find_by_sid(&insts[1]).unwrap();
        assert!(doc.nodes.get(id).unwrap().attrs.contains_key(ATTR_SYMBOL));
    }
    undo.redo(&mut doc).unwrap();

    // 分离后的实例不再参与同步;未分离的仍同步
    undo.push(
        &mut doc,
        Command::SetText {
            sid: title_sid,
            new: "第四版".into(),
            old: None,
        },
    )
    .unwrap();
    assert_eq!(child_text(&doc, &insts[0]), "第四版", "未分离实例仍同步");
    assert_eq!(
        child_text(&doc, &insts[1]),
        "标准标题",
        "已分离实例不再同步"
    );
    assert_eq!(child_text(&doc, &insts[2]), "我的特例", "覆盖保留");

    // 重置覆盖:实例 3 回主件内容、覆盖列表清空
    let cmd = symbol_reset_overrides_commands(&mut doc, &insts[2]).unwrap();
    undo.push(&mut doc, cmd).unwrap();
    assert_eq!(child_text(&doc, &insts[2]), "第四版");
    let i3 = doc.find_by_sid(&insts[2]).unwrap();
    assert!(
        !doc.nodes
            .get(i3)
            .unwrap()
            .attrs
            .contains_key(ATTR_OVERRIDES),
        "重置后覆盖列表应清空"
    );
}

/// symbol_create:选中元素 → 主件 + 首实例;编辑主件 → 新实例同步。
#[test]
fn create_then_edit_syncs() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    // 造一个文本元素
    let sid = doc.alloc_sid();
    let mut n = vb_doc::model::Node::new(
        NodeKind::Text {
            text: "卡片标题".into(),
            mode: vb_doc::model::TextMode::Point,
            segments: Vec::new(),
        },
        "卡片",
        sid.clone(),
    );
    n.geom = vb_doc::model::Geom {
        x: 10.0,
        y: 10.0,
        w: 200.0,
        h: 40.0,
    };
    let ab = doc.artboards[0];
    let pid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
    let tree = vb_doc::model::NodeTree {
        node: n,
        children: vec![],
    };
    undo.push(
        &mut doc,
        Command::Insert {
            parent_sid: pid,
            index: usize::MAX,
            tree,
        },
    )
    .unwrap();
    let elem_sid = sid.as_str().to_string();

    let (cmd, inst_sid) = symbol_create_commands(&mut doc, &elem_sid, "卡片").unwrap();
    undo.push(&mut doc, cmd).unwrap();
    // 原元素进定义区,实例留在画板
    assert!(vb_doc::symbol::is_in_defs(
        &doc,
        doc.find_by_sid(&elem_sid).unwrap()
    ));
    assert!(!vb_doc::symbol::is_in_defs(
        &doc,
        doc.find_by_sid(&inst_sid).unwrap()
    ));
    let iid = doc.find_by_sid(&inst_sid).unwrap();
    assert_eq!(
        doc.nodes
            .get(iid)
            .unwrap()
            .attrs
            .get(ATTR_SYMBOL)
            .map(String::as_str),
        Some("卡片")
    );
    // 实例内容 = 原内容(元素是叶子文本:实例根自身就是文本)
    let root_text = |doc: &Document, sid: &str| {
        let id = doc.find_by_sid(sid).unwrap();
        match &doc.nodes.get(id).unwrap().kind {
            NodeKind::Text { text, .. } => text.clone(),
            k => panic!("应为文本节点,实际 {k:?}"),
        }
    };
    assert_eq!(root_text(&doc, &inst_sid), "卡片标题");

    // 编辑主件(原元素)文本 → 实例同步
    undo.push(
        &mut doc,
        Command::SetText {
            sid: elem_sid,
            new: "新文案".into(),
            old: None,
        },
    )
    .unwrap();
    assert_eq!(root_text(&doc, &inst_sid), "新文案");
    undo.undo(&mut doc).unwrap();
    assert_eq!(root_text(&doc, &inst_sid), "卡片标题");
    undo.redo(&mut doc).unwrap();
    assert_eq!(root_text(&doc, &inst_sid), "新文案");
}

/// 回归(05-8 修 finalize 空类缺陷):实例子树在同步后携带主件的生成类
/// (vb-el-<sid>),再次导出时主件同类冲突 —— 旧实现会把主件清成空类、
/// 丢掉整条 CSS 规则。要求:sync → 导出 → 再导入 → 导出,主件规则仍在。
#[test]
fn regression_main_class_survives_sync_roundtrip() {
    let mut doc = Document::new_default();
    let mut undo = UndoStack::new();
    // 嵌套卡片:Box(class=card)→ 文本(class=title,颜色声明);经 Insert 入档
    let card_sid = doc.alloc_sid();
    let title_sid = doc.alloc_sid();
    let mut title = vb_doc::model::Node::new(
        NodeKind::Text {
            text: "标题".into(),
            mode: vb_doc::model::TextMode::Point,
            segments: Vec::new(),
        },
        "标题",
        title_sid.clone(),
    );
    title.classes.push("title".into());
    title.style.push(vb_css::Decl {
        prop: "color".into(),
        value: "#112233".into(), // vb-token-ok: 文档内容色
        important: false,
    });
    let mut card = vb_doc::model::Node::new(NodeKind::Box, "卡片", card_sid.clone());
    card.classes.push("card".into());
    let ab = doc.artboards[0];
    let ab_sid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
    let card_tree = vb_doc::model::NodeTree {
        node: card,
        children: vec![vb_doc::model::NodeTree {
            node: title,
            children: vec![],
        }],
    };
    undo.push(
        &mut doc,
        Command::Insert {
            parent_sid: ab_sid,
            index: usize::MAX,
            tree: card_tree,
        },
    )
    .unwrap();

    // 创建组件 + 改主件标题(触发同步)→ 导出 1
    let (cmd, _inst) = symbol_create_commands(&mut doc, card_sid.as_str(), "卡片").unwrap();
    undo.push(&mut doc, cmd).unwrap();
    undo.push(
        &mut doc,
        Command::SetText {
            sid: title_sid.as_str().to_string(),
            new: "新标题".into(),
            old: None,
        },
    )
    .unwrap();
    let out1 = render_project(&doc);
    let css1 = css(&out1.files);
    assert!(
        css1.contains("color: #112233"),
        "导出 1 应含主件样式:\n{css1}"
    );

    // 导出 1 → 再导入 → 再改主件(同步)→ 导出 2:主件子节点的规则必须仍在
    let dir = std::env::temp_dir().join(format!("vb-sym-reg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), &out1.files[0].1).unwrap();
    std::fs::create_dir_all(dir.join("styles")).unwrap();
    std::fs::write(dir.join("styles/main.css"), &css1).unwrap();
    let r2 = import_project(&dir).unwrap();
    let mut doc2 = r2.doc;
    let mut undo2 = UndoStack::new();
    let container = doc2.nodes.get(doc2.defs_root).unwrap().children[0];
    let proto = doc2.nodes.get(container).unwrap().children[0];
    let proto_title = doc2.nodes.get(proto).unwrap().children[0];
    let proto_title_sid = doc2
        .nodes
        .get(proto_title)
        .unwrap()
        .sid
        .as_str()
        .to_string();
    undo2
        .push(
            &mut doc2,
            Command::SetText {
                sid: proto_title_sid,
                new: "再改".into(),
                old: None,
            },
        )
        .unwrap();
    let out2 = render_project(&doc2);
    let css2 = css(&out2.files);
    // 颜色值经导入规范化为短 hex(#112233 → #123);关键断言:主件子节点的
    // 规则选择器与颜色声明都在(不被 finalize 清成空类)
    let stripped = css2.replace(' ', "");
    assert!(
        stripped.contains("color:#123"),
        "同步后的再次导出不得丢主件样式(finalize 空类回归):\n{css2}"
    );
    assert_eq!(
        stripped.matches("color:#123").count(),
        2,
        "实例与主件都应各有一条含颜色的规则:\n{css2}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 实机冒烟产物:同步前后 / detach 前后 / 导出 HTML → `%TEMP%\vb-iter\phase5d\`。
/// 断言极简(目录可写),产物供人工核验「无 JS、可 diff」。
#[test]
fn smoke_artifacts() {
    let dir = std::env::temp_dir().join("vb-iter").join("phase5d");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let (mut doc, insts, title_sid) = import_fixture();
    let mut undo = UndoStack::new();

    // 同步前
    let before = render_project(&doc);
    std::fs::write(dir.join("before-main-edit.html"), &before.files[0].1).unwrap();

    // 改主件标题 → 三实例同步
    undo.push(
        &mut doc,
        Command::SetText {
            sid: title_sid,
            new: "限时优惠".into(),
            old: None,
        },
    )
    .unwrap();
    let after = render_project(&doc);
    std::fs::write(dir.join("after-main-edit.html"), &after.files[0].1).unwrap();
    assert_eq!(child_text(&doc, &insts[0]), "限时优惠");
    assert_eq!(child_text(&doc, &insts[2]), "我的特例");

    // detach 前后片段
    let pick = |files: &[(String, String)]| find(files, "index.html");
    let seg = |html: &str, sid: &str| {
        let key = format!(r#"data-vb-id="{sid}""#);
        let start = html.find(&key).unwrap_or(0);
        let mut end = (start + 400).min(html.len());
        while end < html.len() && !html.is_char_boundary(end) {
            end += 1;
        }
        html[start..end].to_string()
    };
    let html_after = pick(&after.files);
    std::fs::write(
        dir.join("instance-before-detach.txt"),
        seg(&html_after, &insts[1]),
    )
    .unwrap();
    let cmd = symbol_detach_commands(&doc, &insts[1]).unwrap();
    undo.push(&mut doc, cmd).unwrap();
    let detached = render_project(&doc);
    let html_detached = pick(&detached.files);
    std::fs::write(
        dir.join("instance-after-detach.txt"),
        seg(&html_detached, &insts[1]),
    )
    .unwrap();
    assert!(
        !html_detached.split("\n").any(
            |l| l.contains(&format!(r#"data-vb-id="{0}""#, "i2b22")) && l.contains(ATTR_SYMBOL)
        ),
        "detach 后该元素不得再带实例标记"
    );

    // 导出 HTML(含组件)整份留档:证明无 JS、可 diff
    std::fs::write(dir.join("export-full.html"), &html_detached).unwrap();
    std::fs::write(dir.join("export-main.css"), css(&detached.files)).unwrap();
    let _ = std::fs::write(dir.join("README.txt"), "05-8 冒烟产物:同步前后/detach 前后/导出 HTML。核验点:无 <script>、实例标记、定义区 hidden。");
}
