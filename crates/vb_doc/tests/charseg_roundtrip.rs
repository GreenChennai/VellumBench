//! 04 阶段字符面板落盘:SegStyle「HTML → 模型 → HTML」往返幂等测试。
//!
//! 捕获与发射必须对称(import `inline_style_of` ⇔ export `seg_style_attr`):
//! 扩展字段(line_height / letter_spacing / baseline_shift / underline /
//! strikethrough)逐一验证「HTML 捕获为段样式」与「段样式发射回 HTML」
//! 双向不丢不改,且整段富文本经两轮导入导出字节幂等。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use vb_doc::export::render_project;
use vb_doc::import::import_project;
use vb_doc::model::{NodeKind, SegStyle};

fn unique_dir(tag: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("vb-charseg-{}-{tag}-{n}", std::process::id()))
}

/// 导入单文件 HTML,返回 (文档, 临时目录)。
fn load(html: &str) -> (vb_doc::Document, PathBuf) {
    let dir = unique_dir("load");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();
    let r = import_project(&dir).expect("导入失败");
    (r.doc, dir)
}

/// 找首个富文本节点。
fn rich_text_node(doc: &vb_doc::Document) -> (String, Vec<vb_doc::model::TextSeg>) {
    for (_, n) in doc.nodes.iter() {
        if let NodeKind::Text { text, segments, .. } = &n.kind {
            if !segments.is_empty() {
                return (text.clone(), segments.clone());
            }
        }
    }
    panic!("文档中无富文本段");
}

fn full_html(body_inner: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>字符段</title></head>
<body>
<section class="vb-artboard" data-vb-id="aa0000" data-vb-name="画板 1" style="position: relative; width: 600px; height: 400px">
  <p class="vb-el-aa0001" data-vb-id="aa0001" data-vb-name="标题" style="position: absolute; left: 40px; top: 40px; width: 400px; height: 60px">{body_inner}</p>
</section>
</body>
</html>
"#
    )
}

/// 逐字段:HTML 行内样式/语义标签 → SegStyle 捕获。
#[test]
fn charseg_captures_all_extended_fields() {
    let html = full_html(
        r#"价格 <span style="color: #cc0000; font-size: 28px; line-height: 1.5; letter-spacing: 2px; vertical-align: 3px; text-decoration: underline">¥100</span> 起"#,
    );
    let (doc, _dir) = load(&html);
    let (text, segs) = rich_text_node(&doc);
    assert_eq!(segs.len(), 1, "一个样式段:{segs:?}");
    let st: &SegStyle = &segs[0].style;
    assert_eq!(text[segs[0].start..segs[0].end].trim(), "¥100");
    assert_eq!(
        st.color.as_deref(),
        Some("#c00"),
        "字色捕获(canonical 缩写)"
    );
    assert_eq!(st.font_size, Some(28.0), "字号捕获");
    assert_eq!(st.line_height, Some(42.0), "无单位行距 × 本段字号 = 1.5×28");
    assert_eq!(st.letter_spacing, Some(2.0), "字距捕获");
    assert_eq!(st.baseline_shift, Some(3.0), "基线偏移(vertical-align)捕获");
    assert_eq!(st.underline, Some(true), "下划线捕获");
    assert_eq!(st.strikethrough, None, "未声明删除线 = None(继承)");
}

/// 语义标签:b/strong → 粗体,em/i → 斜体(UA 默认落为显式覆盖)。
#[test]
fn charseg_semantic_bold_italic_captured() {
    let html = full_html("加粗 <strong>重点</strong> 与 <em>强调</em>");
    let (doc, _dir) = load(&html);
    let (_, segs) = rich_text_node(&doc);
    let mut saw_bold = false;
    let mut saw_italic = false;
    for s in &segs {
        if s.style.bold == Some(true) {
            saw_bold = true;
        }
        if s.style.italic == Some(true) {
            saw_italic = true;
        }
    }
    assert!(saw_bold, "strong 应捕获为粗体段:{segs:?}");
    assert!(saw_italic, "em 应捕获为斜体段:{segs:?}");
}

/// 往返:导入 → 导出 → 再导入,段样式逐字段等值;两轮导出字节幂等。
#[test]
fn charseg_html_model_html_roundtrip_idempotent() {
    let html = full_html(
        r#"注:<span style="color: #10406b; font-size: 18px; line-height: 24px; letter-spacing: 1px; vertical-align: -2px; text-decoration: underline line-through; font-style: italic">见附则</span>(含删除线)"#,
    );
    let dir = unique_dir("rt");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();

    let r1 = import_project(&dir).unwrap();
    let (_, segs1) = rich_text_node(&r1.doc);
    let out1 = render_project(&r1.doc);
    for (rel, content) in &out1.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let r2 = import_project(&dir).unwrap();
    let (_, segs2) = rich_text_node(&r2.doc);
    let out2 = render_project(&r2.doc);

    assert_eq!(segs1, segs2, "段样式应经 HTML 往返无损");
    let seg = &segs2[0].style;
    assert_eq!(seg.line_height, Some(24.0), "显式 px 行距直接捕获");
    assert_eq!(seg.baseline_shift, Some(-2.0), "负基线偏移");
    assert_eq!(seg.underline, Some(true));
    assert_eq!(
        seg.strikethrough,
        Some(true),
        "underline line-through 同捕获"
    );
    assert_eq!(seg.italic, Some(true));
    for (a, b) in out1.files.iter().zip(out2.files.iter()) {
        assert_eq!(a.1, b.1, "{} 两轮导出应字节幂等", a.0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
