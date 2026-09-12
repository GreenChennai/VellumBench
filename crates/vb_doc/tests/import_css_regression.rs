//! 回归测试:2026-09 审查修复的导入/CSS/序列化缺陷。

use vb_doc::export::render_project;
use vb_doc::import::import_html;
use vb_doc::model::NodeKind;

fn import_str(html: &str) -> vb_doc::import::ImportResult {
    import_html(html, std::path::Path::new(".")).expect("导入失败")
}

/// `<pre><code>` 的元素子节点不得被 rawtext 序列化吞掉(此前整块内容丢失)。
#[test]
fn pre_element_children_survive() {
    let html = r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8">
    <title>t</title>
  </head>
  <body>
    <section class="vb-artboard" data-vb-id="ab00000001" data-vb-name="画板">
      <div class="vb-el-pre1" data-vb-id="pre00000001" data-vb-name="代码块" style="left: 10px; top: 10px; width: 400px; height: 200px;">
        <pre><code>x = 1;</code></pre>
      </div>
    </section>
  </body>
</html>
"#;
    let r = import_str(html);
    let out = render_project(&r.doc).files;
    let html_out = out
        .iter()
        .find(|(p, _)| p.ends_with("index.html"))
        .map(|(_, c)| c.as_str())
        .unwrap_or("");
    assert!(
        html_out.contains("<code") && html_out.contains("x = 1;"),
        "pre 内的 code 元素必须保留:\n{html_out}"
    );
}

/// 手编 HTML 的重复 data-vb-id 必须重分配(此前双节点共享 sid,patch 寻址错乱)。
#[test]
fn duplicate_vb_ids_get_fresh_sids() {
    let html = r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8">
    <title>t</title>
  </head>
  <body>
    <section class="vb-artboard" data-vb-id="dup00000001" data-vb-name="画板">
      <div class="vb-el-a1" data-vb-id="same0000001" data-vb-name="A" style="left: 0px; top: 0px; width: 10px; height: 10px;"></div>
      <div class="vb-el-b1" data-vb-id="same0000001" data-vb-name="B" style="left: 20px; top: 20px; width: 10px; height: 10px;"></div>
    </section>
  </body>
</html>
"#;
    let r = import_str(html);
    assert!(r.doc.find_by_sid("same0000001").is_some());
    let mut count = 0;
    for (_, n) in r.doc.nodes.iter() {
        if n.sid.as_str() == "same0000001" {
            count += 1;
        }
    }
    assert_eq!(count, 1, "sid 必须全文档唯一");
    let ab = r.doc.artboards[0];
    assert_eq!(
        r.doc.nodes.get(ab).unwrap().children.len(),
        2,
        "两个节点都必须在"
    );
}

/// CSS 扫描必须感知字符串:`content: "}"` 不得截断 at-rule 块。
#[test]
fn css_string_brace_does_not_cut_rule() {
    let html = r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8">
    <title>t</title>
    <style>
      @media print {
        .vb-el-q1x0000001::before { content: "}" }
      }
      .vb-el-q1x0000001 { color: red; left: 10px; top: 10px; width: 10px; height: 10px; }
    </style>
  </head>
  <body>
    <section class="vb-artboard" data-vb-id="css00000001" data-vb-name="画板">
      <div class="vb-el-q1x0000001" data-vb-id="q1x00000001" data-vb-name="Q"></div>
    </section>
  </body>
</html>
"#;
    let r = import_str(html);
    let out = render_project(&r.doc).files;
    let css = out
        .iter()
        .find(|(p, _)| p.ends_with("main.css"))
        .map(|(_, c)| c.as_str())
        .unwrap_or("");
    assert!(
        css.contains("@media print"),
        "@media 块必须完整保留:\n{css}"
    );
    assert!(
        css.contains("color: red"),
        "后续规则不得被截断的 at-rule 吞掉:\n{css}"
    );
}

/// 自定义属性区分大小写:--brandColor 不得被小写成 --brandcolor。
#[test]
fn custom_property_case_preserved() {
    let html = r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8">
    <title>t</title>
    <style>
      :root { --brandColor: #ff0000; }
    </style>
  </head>
  <body>
    <section class="vb-artboard" data-vb-id="var00000001" data-vb-name="画板"></section>
  </body>
</html>
"#;
    let r = import_str(html);
    assert!(
        r.doc.tokens.iter().any(|(n, _)| n == "brandColor"),
        "令牌名必须保留大小写:{:?}",
        r.doc.tokens
    );
}

/// url(#fragment) 里的 #id 不是颜色,不得被缩短为 3 位 hex。
#[test]
fn url_fragment_hex_not_shortened() {
    let d = vb_css::Decl::parse("clip-path: url(#aabbcc)").expect("解析失败");
    assert_eq!(d.value, "url(#aabbcc)", "SVG 引用不得按颜色缩短");
}

/// 同特异度类规则按样式表顺序消解,与 class 属性顺序无关。
#[test]
fn class_cascade_follows_stylesheet_order() {
    let html = r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8">
    <title>t</title>
    <style>
      .vb-el-w1 { color: red; }
      .vb-el-w2 { color: blue; }
    </style>
  </head>
  <body>
    <section class="vb-artboard" data-vb-id="casc0000001" data-vb-name="画板">
      <div class="vb-el-w2 vb-el-w1" data-vb-id="casx0000001" data-vb-name="W" style="left: 0px; top: 0px; width: 10px; height: 10px;"></div>
    </section>
  </body>
</html>
"#;
    let r = import_str(html);
    let ab = r.doc.artboards[0];
    let w = r
        .doc
        .nodes
        .get(ab)
        .unwrap()
        .children
        .iter()
        .map(|&c| r.doc.nodes.get(c).unwrap())
        .find(|n| matches!(n.kind, NodeKind::Box))
        .expect("应有盒节点");
    let color = w.style_get("color").expect("应有 color");
    assert_eq!(
        color, "blue",
        "样式表里 .vb-el-w2 在后,blue 必须胜出(与 class 属性顺序无关)"
    );
}

/// body 里的 <style>/<link> 不得把 HTML 标签字面写进 main.css。
#[test]
fn body_style_link_not_dumped_into_css() {
    let html = r#"<!DOCTYPE html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8">
    <title>t</title>
  </head>
  <body>
    <section class="vb-artboard" data-vb-id="bst00000001" data-vb-name="画板"></section>
    <style>.x { color: red; }</style>
  </body>
</html>
"#;
    let r = import_str(html);
    let out = render_project(&r.doc).files;
    let css = out
        .iter()
        .find(|(p, _)| p.ends_with("main.css"))
        .map(|(_, c)| c.as_str())
        .unwrap_or("");
    assert!(
        !css.contains("<style>"),
        "main.css 里不得出现 HTML 标签:\n{css}"
    );
    let html_out = out
        .iter()
        .find(|(p, _)| p.ends_with("index.html"))
        .map(|(_, c)| c.as_str())
        .unwrap_or("");
    assert!(
        html_out.contains(".x { color: red; }"),
        "body 样式内容必须完整保留(转投 head):{html_out}"
    );
}
