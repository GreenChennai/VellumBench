//! 字符/段落面板门禁测试(字段真相表 / 对齐九式 / run 管线可逆)。
//! 06-1 自 `panels/charpara.rs` 拆出(纯搬移,零行为变化)。

use crate::app::control_panel::style_prop_cmds;
use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::export::render_project;
use vb_doc::model::Document;
use vb_doc::model::{Geom, Node, NodeKind, SegStyle, TextMode, TextSeg};
use vb_doc::undo::UndoStack;

use super::fields::field_tables;
use super::*;

/// 夹具:画板 + 文本节点(内容与几何自定)。
fn text_doc(text: &str, mode: TextMode, w: f64, h: f64) -> (Document, String) {
    let mut doc = Document::new_default();
    let ab = doc.artboards.first().copied().unwrap();
    let sid = doc.alloc_sid();
    let mut n = Node::new(
        NodeKind::Text {
            text: text.into(),
            mode,
            segments: Vec::new(),
        },
        "文本",
        sid.clone(),
    );
    n.geom = Geom {
        x: 0.0,
        y: 0.0,
        w,
        h,
    };
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    doc.nodes.get_mut(ab).unwrap().children.push(id);
    (doc, sid.as_str().to_string())
}

fn style_of<'a>(doc: &'a Document, sid: &str) -> &'a Vec<Decl> {
    &doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().style
}

fn segs_of<'a>(doc: &'a Document, sid: &str) -> &'a [TextSeg] {
    match &doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().kind {
        NodeKind::Text { segments, .. } => segments,
        _ => panic!("不是文本"),
    }
}

// ───────────── 门 1:字段清单与 design/03 §5.10 一致 ─────────────

/// 字符面板字段清单 = design/03 §5.10 字段表(顺序一致;冻结登记项
/// 显式在列 —— 是「登记」而非「丢失」)。
#[test]
fn char_fields_match_design_5_10() {
    let ids: Vec<&str> = field_tables::CHAR_FIELDS
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(
        ids,
        vec![
            "char.family",          // 字体
            "char.bold",            // 样式·粗体
            "char.italic",          // 样式·斜体
            "char.size",            // 大小
            "char.line_height",     // 行距
            "char.tracking",        // 字距
            "char.baseline",        // 基线偏移
            "char.underline",       // 下划线
            "char.strike",          // 删除线
            "char.lang",            // 语言
            "char.smoothing",       // 抗锯齿
            "char.frozen.kerning",  // 字偶距(冻结)
            "char.frozen.vscale",   // 垂直缩放(冻结)
            "char.frozen.hscale",   // 水平缩放(冻结)
            "char.frozen.rotation", // 字符旋转(冻结)
        ],
        "字符面板字段与 design/03 §5.10 漂移"
    );
}

/// 段落面板字段清单 = design/03 §5.10 / 副文档 04-2。
#[test]
fn para_fields_match_design() {
    let ids: Vec<&str> = field_tables::PARA_FIELDS
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(
        ids,
        vec![
            "para.align",
            "para.indent_l",
            "para.indent_r",
            "para.indent_first",
            "para.space_before",
            "para.space_after",
            "para.kinsoku",
            "para.hyphens",
            "para.punct_squeeze",
            "para.area_fit",
        ],
        "段落面板字段与 design/03 §5.10 漂移"
    );
}

// ───────────── 门 2:对齐九式投影/写回对称 + 命令可逆 ─────────────

/// 九式声明组互不相同;from_style(decls) 恒等投影(往返幂等的根基)。
#[test]
fn align9_decls_are_distinct_and_project_identity() {
    for a in Align9::ALL {
        let decls: Vec<Decl> = a
            .decls()
            .into_iter()
            .map(|(p, v)| Decl {
                prop: p.into(),
                value: v.into(),
                important: false,
            })
            .collect();
        assert_eq!(
            Align9::from_style(&decls),
            Some(a),
            "{} 的声明组投影不是恒等",
            a.label()
        );
    }
    let distinct: std::collections::HashSet<Vec<String>> = Align9::ALL
        .iter()
        .map(|a| a.decls().iter().map(|(p, v)| format!("{p}:{v}")).collect())
        .collect();
    assert_eq!(distinct.len(), 9, "九式声明组必须互不相同");
}

/// 段落对齐写回 → undo 精确逆回 → redo 等效(导出快照级)。
#[test]
fn para_align_cmds_undo_exact() {
    let (mut doc, sid) = text_doc("段落对齐测试", TextMode::Area, 200.0, 60.0);
    let before = render_project(&doc).files;
    let mut stack = UndoStack::new();
    // 本测试验证「逐命令」精确逆回;合并行为由 vb_doc charseg/undo 测试覆盖
    stack.merging_enabled = false;
    let cmd = para_align_cmds(&doc, std::slice::from_ref(&sid), Align9::JustifyLastCenter)
        .into_iter()
        .next()
        .unwrap();
    stack.push(&mut doc, cmd).expect("apply");
    let styled = render_project(&doc).files;
    assert_ne!(styled, before);
    let css = styled.iter().find(|(p, _)| p == "styles/main.css").unwrap();
    assert!(
        css.1.contains("text-align-last: center"),
        "导出应带末行居中"
    );
    stack.undo(&mut doc).expect("undo");
    assert_eq!(render_project(&doc).files, before, "undo 精确逆回");
    stack.redo(&mut doc).expect("redo");
    assert_eq!(render_project(&doc).files, styled, "redo 等效");

    // 切回左对齐:text-align-last 整条移除(不留残留声明)
    let cmd = para_align_cmds(&doc, std::slice::from_ref(&sid), Align9::Left)
        .into_iter()
        .next()
        .unwrap();
    stack.push(&mut doc, cmd).expect("apply");
    assert!(
        !style_of(&doc, &sid)
            .iter()
            .any(|d| d.prop == "text-align-last"),
        "切回左对齐不得残留 text-align-last"
    );
    assert_eq!(Align9::from_style(style_of(&doc, &sid)), Some(Align9::Left));
}

// ───────────── 门 2:字符 run / 整段双路写回 ─────────────

/// 整段转 run → run 字段覆盖 → 移除 run;全程 undo 精确逆回。
#[test]
fn char_run_pipeline_undo_exact() {
    let (mut doc, sid) = text_doc("富文本样式", TextMode::Point, 200.0, 40.0);
    let before = render_project(&doc).files;
    let mut stack = UndoStack::new();
    stack.merging_enabled = false; // 同上:逐命令逆回语义
    let push = |stack: &mut UndoStack, doc: &mut Document, cmd: Option<Command>| {
        stack.push(doc, cmd.expect("命令应存在")).expect("apply");
    };
    // 注意:构建器(&doc)与 push(&mut doc) 参数先行物化,借用分离

    let c = make_full_run_cmd(&doc, &sid);
    push(&mut stack, &mut doc, c);
    assert_eq!(segs_of(&doc, &sid).len(), 1);

    let c = seg_field_cmd(&doc, &sid, |s| {
        s.bold = Some(true);
        s.letter_spacing = Some(2.0);
        s.baseline_shift = Some(4.0);
        s.underline = Some(true);
    });
    push(&mut stack, &mut doc, c);
    let segs = segs_of(&doc, &sid);
    assert_eq!(segs[0].style.bold, Some(true));
    assert_eq!(segs[0].style.letter_spacing, Some(2.0));
    let styled = render_project(&doc).files;

    let c = clear_runs_cmd(&doc, &sid);
    push(&mut stack, &mut doc, c);
    assert!(segs_of(&doc, &sid).is_empty());

    // 连续三次 undo:移除 run → 改字段 → 建 run,逐步精确逆回
    stack.undo(&mut doc).expect("undo");
    assert_eq!(segs_of(&doc, &sid).len(), 1);
    assert_eq!(render_project(&doc).files, styled);
    stack.undo(&mut doc).expect("undo");
    assert_eq!(
        segs_of(&doc, &sid)[0].style,
        SegStyle::default(),
        "字段覆盖应被精确逆回"
    );
    stack.undo(&mut doc).expect("undo");
    assert!(segs_of(&doc, &sid).is_empty());
    assert_eq!(render_project(&doc).files, before);
}

/// 整段作用域:粗体 / 斜体 / 下划线 / 字距写 Node.style,undo 精确逆回。
#[test]
fn char_node_scope_writes_and_reverts() {
    let (mut doc, sid) = text_doc("整段样式", TextMode::Point, 200.0, 40.0);
    let before = render_project(&doc).files;
    let mut stack = UndoStack::new();
    stack.merging_enabled = false;
    let sids = vec![sid.clone()];
    // SetStyle 是整表替换:命令必须按**当前文档状态**顺序构建
    // (与面板交互时序一致;预建数组会用旧状态互相覆盖)
    let cmd = char_bold_cmds(&doc, &sids, true).remove(0);
    stack.push(&mut doc, cmd).expect("apply");
    let cmd = char_italic_cmds(&doc, &sids, true).remove(0);
    stack.push(&mut doc, cmd).expect("apply");
    let cmd = char_deco_cmds(&doc, &sids, true, true).remove(0);
    stack.push(&mut doc, cmd).expect("apply");
    let cmd = style_prop_cmds(&doc, &sids, "letter-spacing", "1.5px").remove(0);
    stack.push(&mut doc, cmd).expect("apply");
    let styled = render_project(&doc).files;
    let css = styled.iter().find(|(p, _)| p == "styles/main.css").unwrap();
    for expect in [
        "font-weight: 700",
        "font-style: italic",
        "text-decoration: underline line-through",
        "letter-spacing: 1.5px",
    ] {
        assert!(css.1.contains(expect), "导出应含 {expect}");
    }
    for _ in 0..4 {
        stack.undo(&mut doc).expect("undo");
    }
    assert_eq!(render_project(&doc).files, before, "逐条 undo 精确逆回");
}

// ───────────── 门 2:区域文本溢出估算 + 自动扩高 ─────────────

/// 溢出估算与自动扩高:窄框溢出 → 扩高到内容需要高度 → 再估不溢出;
/// SetGeom undo 精确逆回。量测与导出同引擎(真字形),不依赖 egui。
#[test]
fn area_fit_height_expands_to_content_and_reverts() {
    let long_text = "这是一段用于溢出测试的中文文案,反复重复以触发换行。".repeat(6);
    let (mut doc, sid) = text_doc(&long_text, TextMode::Area, 120.0, 30.0);
    let nid = doc.find_by_sid(&sid).unwrap();
    assert!(area_overflow_px(&doc, nid) > 0.0, "窄框长文必须判溢出");
    let before = render_project(&doc).files;
    let h0 = doc.nodes.get(nid).unwrap().geom.h;
    let mut stack = UndoStack::new();
    let fit = area_fit_height_cmd(&doc, &sid).unwrap();
    stack.push(&mut doc, fit).expect("apply");
    let h1 = doc
        .nodes
        .get(doc.find_by_sid(&sid).unwrap())
        .unwrap()
        .geom
        .h;
    assert!(h1 > h0, "扩高后必须更高:{h0} → {h1}");
    let nid = doc.find_by_sid(&sid).unwrap();
    assert!(area_overflow_px(&doc, nid) <= 0.0, "扩高后不应再溢出");
    let after = render_project(&doc).files;
    stack.undo(&mut doc).expect("undo");
    assert_eq!(render_project(&doc).files, before, "undo 精确逆回");
    stack.redo(&mut doc).expect("redo");
    assert_eq!(render_project(&doc).files, after, "redo 等效");

    // 不溢出时自动扩高 = 无命令(按钮置灰的依据)
    let (doc2, sid2) = text_doc("短", TextMode::Area, 400.0, 200.0);
    assert!(area_fit_height_cmd(&doc2, &sid2).is_none());
    // 点文本不参与
    let (doc3, sid3) = text_doc("点文本", TextMode::Point, 40.0, 8.0);
    let nid3 = doc3.find_by_sid(&sid3).unwrap();
    assert_eq!(area_overflow_px(&doc3, nid3), 0.0);
    let _ = doc2;
}
