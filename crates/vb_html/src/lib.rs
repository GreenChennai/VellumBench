//! `vb_html` — HTML 的忠实解析与 canonical 序列化。
//!
//! 目标(设计文档 04 篇 §六):
//! - **L0 不损坏**:导入 → 不编辑 → 保存,输出与输入语义等价。
//!   注释、未知属性、`<script>`、非 ASCII 文本、属性顺序(规范化后固定)全部保留。
//! - **L1 幂等**:导入 → 保存 → 再导入 → 再保存,两次输出字节相同。
//!   canonical 序列化是纯函数:`serialize ∘ parse` 幂等。
//!
//! 序列化规则(04 篇 §5.2):2 空格缩进;属性顺序 class → id → data-vb-id →
//! data-vb-name → 语义属性(源顺序);块级元素分行;行内内容保持在同一行
//! (源文本的空白折叠为单空格,避免引入渲染差异);`pre/script/style/textarea`
//! 内容逐字节保留;LF 结尾。

use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData as Rd, RcDom};

pub const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// 内容逐字节保留的元素(不能重排内部空白)。
pub const RAWTEXT_TAGS: &[&str] = &["script", "style", "pre", "textarea"];

/// 块级元素:序列化时独立成行。
pub const BLOCK_TAGS: &[&str] = &[
    "html",
    "head",
    "body",
    "div",
    "section",
    "article",
    "aside",
    "header",
    "footer",
    "nav",
    "main",
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "td",
    "th",
    "form",
    "fieldset",
    "blockquote",
    "figure",
    "figcaption",
    "hr",
    "address",
    "details",
    "summary",
    "dialog",
    "template",
    "script",
    "style",
    "link",
    "meta",
    "title",
    "pre",
    "textarea",
    "button",
];

/// 属性输出顺序档位:class → id → data-vb-id → data-vb-name → 其余(源顺序,稳定)。
fn attr_rank(name: &str) -> usize {
    match name {
        "class" => 0,
        "id" => 1,
        "data-vb-id" => 2,
        "data-vb-name" => 3,
        _ => 4,
    }
}

#[derive(Debug, Clone)]
pub struct Element {
    /// 标签(小写)。
    pub name: String,
    /// 属性(名称小写,值保持;顺序保留)。
    pub attrs: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub enum NodeData {
    Doctype(String),
    Comment(String),
    Text(String),
    Element(Element),
    /// 原样透传的 HTML 片段(冻结块在 DOM 层的形态)。
    Raw(String),
}

#[derive(Debug, Clone)]
pub struct HtmlNode {
    pub data: NodeData,
    pub children: Vec<HtmlNode>,
}

#[derive(Debug, Clone)]
pub struct HtmlDom {
    pub doctype: Option<String>,
    /// 文档级注释(`<!DOCTYPE html>` 与 `<html>` 之间的注释)。
    pub leading_comments: Vec<String>,
    pub root: HtmlNode, // <html>
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set_attr(&mut self, name: &str, value: &str) {
        for (k, v) in &mut self.attrs {
            if k == name {
                *v = value.to_string();
                return;
            }
        }
        self.attrs.push((name.to_string(), value.to_string()));
    }

    pub fn class_list(&self) -> Vec<&str> {
        self.attr("class")
            .map(|c| c.split_whitespace().collect())
            .unwrap_or_default()
    }

    pub fn is_void(&self) -> bool {
        VOID_TAGS.contains(&self.name.as_str())
    }

    pub fn is_rawtext(&self) -> bool {
        RAWTEXT_TAGS.contains(&self.name.as_str())
    }

    pub fn is_block(&self) -> bool {
        BLOCK_TAGS.contains(&self.name.as_str())
    }
}

impl HtmlNode {
    pub fn element(name: &str, attrs: Vec<(String, String)>) -> HtmlNode {
        HtmlNode {
            data: NodeData::Element(Element {
                name: name.to_string(),
                attrs,
            }),
            children: Vec::new(),
        }
    }

    pub fn text(t: impl Into<String>) -> HtmlNode {
        HtmlNode {
            data: NodeData::Text(t.into()),
            children: Vec::new(),
        }
    }

    pub fn raw(html: impl Into<String>) -> HtmlNode {
        HtmlNode {
            data: NodeData::Raw(html.into()),
            children: Vec::new(),
        }
    }

    pub fn as_element(&self) -> Option<&Element> {
        match &self.data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    /// 深度优先遍历(含自身)。
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a HtmlNode)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }
}

impl HtmlDom {
    /// 解析 HTML(html5ever,忠实模式)。
    pub fn parse(html: &str) -> HtmlDom {
        let dom: RcDom = parse_document(RcDom::default(), Default::default()).one(html);
        let mut doctype = None;
        let mut leading_comments = Vec::new();
        let mut root_node: Option<HtmlNode> = None;

        for child in dom.document.children.borrow().iter() {
            match &child.data {
                Rd::Doctype { name, .. } => doctype = Some(name.to_string()),
                Rd::Comment { contents } => leading_comments.push(contents.to_string()),
                Rd::Element { .. } => {
                    // 规范上只有一个 <html>;多个时保留第一个
                    if root_node.is_none() {
                        root_node = Some(convert(child));
                    }
                }
                Rd::Text { .. } | Rd::ProcessingInstruction { .. } => {}
                Rd::Document => {}
            }
        }
        HtmlDom {
            doctype: doctype.or(Some("html".to_string())),
            leading_comments,
            root: root_node.unwrap_or_else(|| HtmlNode::element("html", vec![])),
        }
    }

    pub fn body(&self) -> Option<&HtmlNode> {
        self.root
            .children
            .iter()
            .find(|n| matches!(&n.data, NodeData::Element(e) if e.name == "body"))
    }

    pub fn head(&self) -> Option<&HtmlNode> {
        self.root
            .children
            .iter()
            .find(|n| matches!(&n.data, NodeData::Element(e) if e.name == "head"))
    }

    /// canonical 序列化(纯函数,L1 幂等的来源)。
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        if let Some(d) = &self.doctype {
            out.push_str(&format!("<!DOCTYPE {}>\n", d.to_ascii_lowercase()));
        }
        for c in &self.leading_comments {
            out.push_str(&format!("<!--{}-->\n", c));
        }
        write_node(&self.root, 0, &mut out);
        out
    }
}

/// 解析 + canonical 序列化(测试与比对用)。
pub fn canonicalize(html: &str) -> String {
    HtmlDom::parse(html).serialize()
}

fn convert(h: &Handle) -> HtmlNode {
    match &h.data {
        Rd::Document => HtmlNode::element("#document", vec![]),
        Rd::Doctype { name, .. } => HtmlNode {
            data: NodeData::Doctype(name.to_string()),
            children: vec![],
        },
        Rd::Comment { contents } => HtmlNode {
            data: NodeData::Comment(contents.to_string()),
            children: vec![],
        },
        Rd::Text { contents } => HtmlNode {
            data: NodeData::Text(contents.borrow().to_string()),
            children: vec![],
        },
        Rd::ProcessingInstruction { .. } => HtmlNode::text(""),
        Rd::Element { name, attrs, .. } => {
            let attr_list: Vec<(String, String)> = attrs
                .borrow()
                .iter()
                .map(|a| (a.name.local.to_string(), a.value.to_string()))
                .collect();
            let mut node = HtmlNode::element(name.local.as_ref(), attr_list);
            for c in h.children.borrow().iter() {
                node.children.push(convert(c));
            }
            node
        }
    }
}

// ---------- 序列化 ----------

fn escape_text(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            c => o.push(c),
        }
    }
    o
}

fn escape_attr(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            '\n' | '\t' | '\r' => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// 折叠连续空白为单空格(行内上下文)。
fn collapse_ws(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            ws = true;
        } else {
            if ws && !o.is_empty() {
                o.push(' ');
            }
            ws = false;
            o.push(c);
        }
    }
    o
}

fn canonical_attrs(el: &Element) -> Vec<(String, String)> {
    let mut idx: Vec<usize> = (0..el.attrs.len()).collect();
    idx.sort_by_key(|&i| (attr_rank(&el.attrs[i].0), i));
    idx.into_iter().map(|i| el.attrs[i].clone()).collect()
}

fn write_attrs(el: &Element, out: &mut String) {
    for (k, v) in canonical_attrs(el) {
        out.push_str(&format!(" {}=\"{}\"", k, escape_attr(&v)));
    }
}

/// 判断子树是否可整体行内化:无块级后代、无 Raw。
fn can_inline(node: &HtmlNode) -> bool {
    match &node.data {
        NodeData::Raw(_) => false,
        NodeData::Element(e) => {
            if e.is_rawtext() {
                return e.name != "button";
            }
            node.children.iter().all(|c| match &c.data {
                NodeData::Text(_) => true,
                NodeData::Comment(_) => true,
                NodeData::Raw(_) => false,
                NodeData::Doctype(_) => false,
                NodeData::Element(ce) => {
                    !ce.is_block()
                        && can_inline(c)
                        && ce.name != "script"
                        && ce.name != "style"
                        && ce.name != "pre"
                        && ce.name != "textarea"
                }
            })
        }
        _ => true,
    }
}

/// 行内序列化:空白折叠,兄弟节点间由源空白决定是否补单空格(保真渲染语义)。
fn write_inline(node: &HtmlNode, out: &mut String) {
    match &node.data {
        NodeData::Text(t) => out.push_str(&collapse_ws(t)),
        NodeData::Comment(c) => out.push_str(&format!("<!--{c}-->")),
        NodeData::Raw(r) => out.push_str(r),
        NodeData::Doctype(_) => {}
        NodeData::Element(e) => {
            out.push('<');
            out.push_str(&e.name);
            write_attrs(e, out);
            if e.is_void() {
                out.push('>');
                return;
            }
            out.push('>');
            write_inline_content(node, out);
            out.push_str(&format!("</{}>", e.name));
        }
    }
}

/// 元素子内容的行内序列化:
/// - 文本折叠;仅空白的文本节点贡献"待补空格"标记
/// - 相邻兄弟间源空白 → 输出一个空格;源无空白(如 `</b>!`)则不加
fn write_inline_content(node: &HtmlNode, out: &mut String) {
    let mut pending_ws = false;
    for c in &node.children {
        match &c.data {
            NodeData::Text(t) => {
                let leading = t.starts_with(|ch: char| ch.is_whitespace());
                let trailing = t.ends_with(|ch: char| ch.is_whitespace());
                let content = collapse_ws(t);
                if content.is_empty() {
                    pending_ws = pending_ws || !t.is_empty();
                    continue;
                }
                if (pending_ws || leading) && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(&escape_text(&content));
                pending_ws = trailing;
            }
            NodeData::Comment(_) | NodeData::Element(_) => {
                if pending_ws && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                write_inline(c, out);
                pending_ws = false;
            }
            _ => {}
        }
    }
}

fn write_node(node: &HtmlNode, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match &node.data {
        NodeData::Doctype(_) => {}
        NodeData::Raw(r) => {
            for line in r.lines() {
                out.push_str(&pad);
                out.push_str(line.trim_end());
                out.push('\n');
            }
            if r.is_empty() {
                out.push('\n');
            }
        }
        NodeData::Comment(c) => {
            out.push_str(&format!("{pad}<!--{c}-->\n"));
        }
        NodeData::Text(t) => {
            let t = collapse_ws(t);
            if !t.is_empty() {
                out.push_str(&format!("{pad}{}\n", escape_text(&t)));
            }
        }
        NodeData::Element(e) => {
            if e.is_void() {
                out.push_str(&pad);
                out.push('<');
                out.push_str(&e.name);
                write_attrs(e, out);
                out.push_str(">\n");
                return;
            }
            if e.is_rawtext() {
                out.push_str(&pad);
                out.push('<');
                out.push_str(&e.name);
                write_attrs(e, out);
                out.push('>');
                // 内容逐字节保留(verbatim):任何装饰性换行/缩进都会在下次
                // 解析时进入内容,破坏 L1 幂等 —— 因此闭合标签紧跟内容。
                let inner: String = node
                    .children
                    .iter()
                    .filter_map(|c| match &c.data {
                        NodeData::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect();
                out.push_str(&inner);
                out.push_str(&format!("</{}>\n", e.name));
                return;
            }
            out.push_str(&pad);
            out.push('<');
            out.push_str(&e.name);
            write_attrs(e, out);
            // 空元素:双标签同行
            if node.children.is_empty() {
                out.push_str("></");
                out.push_str(&e.name);
                out.push_str(">\n");
                return;
            }
            // 行内化:子内容无块级/原始片段 → 单行
            if can_inline(node)
                && !node
                    .children
                    .iter()
                    .any(|c| matches!(&c.data, NodeData::Comment(c) if c.contains('\n')))
            {
                out.push('>');
                write_inline_content(node, out);
                out.push_str(&format!("</{}>\n", e.name));
                return;
            }
            out.push_str(">\n");
            for c in &node.children {
                write_node(c, indent + 1, out);
            }
            out.push_str(&pad);
            out.push_str(&format!("</{}>\n", e.name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn l1_idempotent_basic() {
        let src = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <title>测试</title>
</head>
<body>
  <!-- 顶部注释 -->
  <section class="vb-artboard hero" data-vb-id="b1e0aa" data-vb-name="Hero" style="width: 1440px;">
    <h1 class="title">香醇,从一颗豆开始</p>
  </section>
</body>
</html>"#;
        let once = canonicalize(src);
        let twice = canonicalize(&once);
        assert_eq!(once, twice, "canonical 序列化必须幂等");
        assert!(once.contains("<!-- 顶部注释 -->"));
        assert!(once.contains("香醇,从一颗豆开始"));
        assert!(once.contains("data-vb-id=\"b1e0aa\""));
    }

    #[test]
    fn attr_order_canonical() {
        let src = r##"<html><body><div data-vb-name="X" data-vb-id="abc123" class="a b" id="q" href="#">t</div></body></html>"##;
        let out = canonicalize(src);
        let cpos = out.find("class=\"a b\"").unwrap();
        let ipos = out.find("id=\"q\"").unwrap();
        let d1 = out.find("data-vb-id=\"abc123\"").unwrap();
        let d2 = out.find("data-vb-name=\"X\"").unwrap();
        assert!(cpos < ipos && ipos < d1 && d1 < d2);
    }

    #[test]
    fn script_and_style_preserved_verbatim() {
        let src = "<html><head><style>\n  .a > b { color: red; }\n</style></head><body><script>if (a<b && c>d) { f(); }</script></body></html>";
        let out = canonicalize(src);
        assert!(out.contains("if (a<b && c>d) { f(); }"));
        assert!(out.contains(".a > b { color: red; }"));
        let twice = canonicalize(&out);
        assert_eq!(out, twice);
    }

    #[test]
    fn text_escaping_roundtrip() {
        let src = r#"<html><body><p>A &amp; B &lt;tag&gt; "q"</p></body></html>"#;
        let out = canonicalize(src);
        assert!(out.contains(r#"A &amp; B &lt;tag&gt; "q""#));
        let twice = canonicalize(&out);
        assert_eq!(out, twice);
    }

    #[test]
    fn inline_whitespace_semantics() {
        // 行内内容必须在行内序列化,词间空白保留单空格
        let src = r#"<html><body><p>Hello <b>world</b>!</p></body></html>"#;
        let out = canonicalize(src);
        assert!(
            out.contains("<p>Hello <b>world</b> !</p>")
                || out.contains("<p>Hello <b>world</b>!</p>")
        );
        let twice = canonicalize(&out);
        assert_eq!(out, twice);
    }

    #[test]
    fn entities_and_cjk_kept() {
        let src = "<html><body><p>中文「引号」· emoji 🎨 · &nbsp;</p></body></html>";
        let out = canonicalize(src);
        assert!(out.contains('中'));
        assert!(out.contains("🎨"));
    }
}
