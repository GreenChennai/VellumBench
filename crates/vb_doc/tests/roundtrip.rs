//! 往返保真语料测试(设计文档 04 篇 §六,10 篇门禁 3)。
//!
//! - **L0 不损坏**:导入 → 不编辑 → 导出,关键内容全部保留。
//! - **L1 幂等**:导入 → 导出 → 再导入 → 再导出,两次输出字节相同。

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

use vb_doc::export::render_project;
use vb_doc::import::import_project;
use vb_doc::Document;

/// 导入 → 导出(落盘)→ 再导入 → 再导出;返回两次导出的文件表。
/// 走真实目录,保证外链 CSS 在第二次导入时可见(与 Agent/浏览器看到的一致)。
fn roundtrip(html: &str) -> (Vec<(String, String)>, Vec<(String, String)>) {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "vb-rt-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();

    let r1 = import_project(&dir).unwrap();
    let out1 = render_project(&r1.doc);
    for (rel, content) in &out1.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }

    let r2 = import_project(&dir).unwrap();
    let out2 = render_project(&r2.doc);

    let result = (out1.files, out2.files);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn find(files: &[(String, String)], path: &str) -> String {
    files
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, c)| c.clone())
        .unwrap_or_else(|| panic!("缺少 {path}"))
}

fn css(files: &[(String, String)]) -> String {
    find(files, "styles/main.css")
}

// ---------- 语料 1:基础落地页(外链 CSS 形态的导入源) ----------

const SIMPLE_CSS: &str = r#"
:root { --vb-brand-1: #ff5a1f; }
.vb-artboard { position: relative; overflow: hidden; }
.hero { width: 1440px; height: 600px; }
.hero-bg {
  position: absolute;
  left: 0;
  top: 0;
  width: 1440px;
  height: 600px;
  background-image: linear-gradient(180deg, #2b1a12 0, #6b3f24 100%);
}
.hero-title {
  position: absolute;
  left: 120px;
  top: 220px;
  width: 720px;
  height: 80px;
  font-family: Inter, "PingFang SC", sans-serif;
  font-size: 64px;
  font-weight: 700;
  line-height: 1.2;
  color: #fff;
}
"#;

fn simple_page(extra_head: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <title>简单页面</title>
{extra_head}</head>
<body>
  <section class="vb-artboard hero" data-vb-id="b1e0aa" data-vb-name="Hero">
    <div class="hero-bg" data-vb-id="e4b3d5" data-vb-name="Hero 背景"></div>
    <h1 class="hero-title" data-vb-id="f5c4e6" data-vb-name="主标题">香醇,从一颗豆开始</h1>
  </section>
</body>
</html>
"#
    )
}

#[test]
fn l1_idempotent_simple() {
    let src = simple_page("<style>\n{SIMPLE_CSS}\n</style>\n");
    let src = src.replace("{SIMPLE_CSS}", SIMPLE_CSS);
    let (f1, f2) = roundtrip(&src);
    assert_eq!(find(&f1, "index.html"), find(&f2, "index.html"), "HTML 必须幂等");
    assert_eq!(css(&f1), css(&f2), "CSS 必须幂等");
}

#[test]
fn l0_preserves_ids_names_text_and_geometry() {
    let src = simple_page(&format!("<style>\n{SIMPLE_CSS}\n</style>\n"));
    let (f1, _) = roundtrip(&src);
    let html = find(&f1, "index.html");
    let css_out = css(&f1);
    for needle in [
        "data-vb-id=\"b1e0aa\"",
        "data-vb-name=\"Hero\"",
        "data-vb-id=\"f5c4e6\"",
        "data-vb-name=\"主标题\"",
        "香醇,从一颗豆开始",
        "<!DOCTYPE html>",
        "class=\"vb-artboard hero\"",
    ] {
        assert!(html.contains(needle), "L0 丢失:{needle}");
    }
    for needle in [
        "left: 120px;",
        "top: 220px;",
        "width: 720px;",
        "font-size: 64px;",
        "--vb-brand-1: #ff5a1f;",
        "linear-gradient(180deg, #2b1a12 0, #6b3f24 100%)",
    ] {
        assert!(css_out.contains(needle), "CSS L0 丢失:{needle}");
    }
}

// ---------- 语料 2:script / SVG 冻结块 / unknown CSS / @media / 伪类 ----------

const FROZEN_CSS: &str = r#"
.page { width: 800px; height: 600px; }
.card { background-color: #fff; border-radius: 8px; box-shadow: 0 2px 8px #00000026; }
.backdrop-panel { backdrop-filter: blur(8px); }
@media (max-width: 768px) {
  .card { width: 100%; }
}
.card:hover { background-color: #f5f5f5; }
"#;

const FROZEN_PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>Script 页面</title>
  <style>
.page { width: 800px; height: 600px; }
.card { background-color: #fff; border-radius: 8px; box-shadow: 0 2px 8px #00000026; }
.backdrop-panel { backdrop-filter: blur(8px); }
@media (max-width: 768px) {
  .card { width: 100%; }
}
.card:hover { background-color: #f5f5f5; }
  </style>
</head>
<body>
  <section class="vb-artboard page" data-vb-id="aaaaaa" data-vb-name="Page">
    <div class="card" data-vb-id="bbbbbb" data-vb-name="卡片"></div>
    <svg class="logo" data-vb-id="cccccc" viewBox="0 0 24 24"><path d="M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z"></path></svg>
    <script>console.log("保留我", 1 < 2);</script>
  </section>
</body>
</html>
"#;

#[test]
fn script_svg_media_hover_preserved() {
    let (f1, f2) = roundtrip(FROZEN_PAGE);
    assert_eq!(find(&f1, "index.html"), find(&f2, "index.html"));
    let html = find(&f1, "index.html");
    let css_out = css(&f1);
    assert!(html.contains("console.log(\"保留我\", 1 < 2);"), "脚本逐字保留");
    assert!(html.contains("M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z"), "SVG 冻结块保留");
    assert!(html.contains("data-vb-id=\"bbbbbb\""));
    let _ = FROZEN_CSS;
    assert!(css_out.contains("@media (max-width: 768px)"), "@media 原样保留");
    assert!(css_out.contains(".card:hover"), "伪类选择器原样保留");
    assert!(css_out.contains("backdrop-filter: blur(8px)"), "unknown 属性保留");
    assert!(css_out.contains("box-shadow: 0 2px 8px #00000026;"));
}

// ---------- 语料 3:文本混排 + HTML 属性 ----------

const MIXED_PAGE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <title>文本混排</title>
  <style>
.main { width: 1000px; height: 500px; }
.lead { position: absolute; left: 50px; top: 30px; width: 400px; height: 60px; font-size: 24px; color: #333; }
.cta { position: absolute; left: 50px; top: 120px; width: 120px; height: 48px; padding: 12px 24px; border-radius: 8px; background-color: #ff5a1f; }
  </style>
</head>
<body>
  <!-- 页面说明注释 -->
  <section class="vb-artboard main" data-vb-id="dd0001" data-vb-name="Main">
    <p class="lead" data-vb-id="dd0002" data-vb-name="引导语">Hello <b>world</b>!</p>
    <a class="cta" data-vb-id="dd0003" data-vb-name="CTA" href="/buy" aria-label="立即购买">立即购买</a>
  </section>
</body>
</html>
"#;

#[test]
fn text_mixed_and_attrs_preserved() {
    let (f1, f2) = roundtrip(MIXED_PAGE);
    assert_eq!(find(&f1, "index.html"), find(&f2, "index.html"));
    let html = find(&f1, "index.html");
    assert!(html.contains("Hello <b"), "行内起点");
    assert!(html.contains("world</b>!"), "行内元素与后续文本间的无空白边界保留");
    assert!(html.contains("href=\"/buy\""));
    assert!(html.contains("aria-label=\"立即购买\""));
    assert!(html.contains("<!-- 页面说明注释 -->"));
    assert!(html.contains("立即购买"));
}

// ---------- 语料 4:内联 style 直写(矢量模式 Agent 产物) ----------

const INLINE_PAGE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="UTF-8"><title>内联</title></head>
<body>
  <section class="vb-artboard ab" data-vb-id="ab0001" data-vb-name="AB">
    <div class="box" data-vb-id="bx0001" data-vb-name="盒子" style="left: 10px; top: 20px; width: 300px; height: 150px; background-color: #3a86ff; border-radius: 12px; opacity: 0.8;"></div>
  </section>
</body>
</html>
"#;

#[test]
fn inline_styles_roundtrip() {
    let (f1, f2) = roundtrip(INLINE_PAGE);
    assert_eq!(find(&f1, "index.html"), find(&f2, "index.html"));
    let css_out = css(&f1);
    assert!(css_out.contains("background-color: #3a86ff;"));
    assert!(css_out.contains("border-radius: 12px;"));
    assert!(css_out.contains("opacity: 0.8;"));
    assert!(css_out.contains("left: 10px;"));
    assert!(css_out.contains("top: 20px;"));
}

// ---------- 默认文档 ----------

#[test]
fn new_document_has_default_artboard() {
    let doc = Document::new_default();
    assert_eq!(doc.artboards.len(), 1);
    let ab = doc.nodes.get(doc.artboards[0]).unwrap();
    assert_eq!((ab.geom.w, ab.geom.h), (1440.0, 900.0));
    assert_eq!(ab.name, "画板 1");
    let out = render_project(&doc);
    let html = find(&out.files, "index.html");
    assert!(html.contains("vb-artboard"));
    assert!(find(&out.files, "styles/main.css").contains("width: 1440px;"));
    assert!(html.contains("data-vb-id="));
}
