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
use std::fmt::Write as _;

/// 递归深度上限(单一真相 [`vb_common::MAX_TREE_DEPTH`];RB-02 / DOC-01)。
/// 解析建树、序列化(块级行内两路)一律以此为界:解析超限记入
/// [`HtmlDom::depth_exceeded`](导入层必须显式报错),写出超限插可见
/// 注释标记 —— 均不静默截断。
pub const MAX_DEPTH: usize = vb_common::MAX_TREE_DEPTH;

pub const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// 内容逐字节保留的元素(不能重排内部空白)。
pub const RAWTEXT_TAGS: &[&str] = &["script", "style", "pre", "textarea"];

/// 块级元素:序列化时独立成行。
///
/// **单一真相**(COUP-R5 / 审查 COUP-03):本表是块级标签的权威清单,
/// `vb_doc` 导入器的建树边界判定**派生自本表**(加上极少量导入特有
/// 条目,见其 `IMPORT_EXTRA_BLOCK_TAGS`);禁止在任何 crate 里再抄一份
/// 全量清单手工对齐。
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
    /// 解析时命中 [`MAX_DEPTH`] 的**显式标志**(DOC-01):导入层看到
    /// true 必须报结构化错误(不静默吃掉被截断的子树);canonical 序列化
    /// 对应位置会写入可见注释标记。
    pub depth_exceeded: bool,
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
    ///
    /// 显式栈迭代(前序 DFS,访问序与旧递归版一致):遍历入口对任意
    /// 来源的树开放,递归版在深树上会栈溢出(RB-02)。
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a HtmlNode)) {
        let mut stack = vec![self];
        while let Some(n) = stack.pop() {
            f(n);
            for c in n.children.iter().rev() {
                stack.push(c);
            }
        }
    }
}

impl HtmlDom {
    /// 解析 HTML(html5ever,忠实模式)。
    ///
    /// 建树(`convert`)带 [`MAX_DEPTH`] 深度上限:超限**不丢弃标志**,
    /// 记入 [`HtmlDom::depth_exceeded`](同时超限子树被截断)—— 导入层
    /// 必须检查本标志并显式报错(DOC-01:不静默)。
    pub fn parse(html: &str) -> HtmlDom {
        let dom: RcDom = parse_document(RcDom::default(), Default::default()).one(html);
        let mut doctype = None;
        let mut leading_comments = Vec::new();
        let mut root_node: Option<HtmlNode> = None;
        let mut depth_exceeded = false;

        for child in dom.document.children.borrow().iter() {
            match &child.data {
                Rd::Doctype { name, .. } => doctype = Some(name.to_string()),
                Rd::Comment { contents } => leading_comments.push(contents.to_string()),
                Rd::Element { .. } => {
                    // 规范上只有一个 <html>;多个时保留第一个
                    if root_node.is_none() {
                        let mut depth_hit = false;
                        root_node = Some(convert(child, 0, &mut depth_hit));
                        depth_exceeded |= depth_hit;
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
            depth_exceeded,
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
    ///
    /// 写出带 [`MAX_DEPTH`] 上限(RB-02):手写构造的异常深树超限时在
    /// 对应位置写入**可见注释标记**,不静默截断;经 [`HtmlDom::parse`]
    /// 构造的树深 ≤ MAX_DEPTH(超限已在解析侧标记),正常路径字节不变。
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        if let Some(d) = &self.doctype {
            let _ = writeln!(out, "<!DOCTYPE {}>", d.to_ascii_lowercase());
        }
        for c in &self.leading_comments {
            let _ = writeln!(out, "<!--{c}-->");
        }
        if self.depth_exceeded {
            out.push_str("<!--vb:depth-limit-exceeded-->\n");
        }
        let mut pad = String::new();
        write_node(&self.root, 0, &mut pad, &mut out);
        out
    }
}

/// 解析 + canonical 序列化(测试与比对用)。
pub fn canonicalize(html: &str) -> String {
    HtmlDom::parse(html).serialize()
}

/// rcdom 树 → HtmlNode(带深度上限;超限截断并置 `*depth_hit`,导入层
/// 必须显式报错 —— DOC-01/RB-02,不许静默)。
fn convert(h: &Handle, depth: usize, depth_hit: &mut bool) -> HtmlNode {
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
            if depth >= MAX_DEPTH {
                *depth_hit = true;
                return node;
            }
            for c in h.children.borrow().iter() {
                node.children.push(convert(c, depth + 1, depth_hit));
            }
            node
        }
    }
}

// ---------- 序列化 ----------

/// 文本转义,直接写入输出缓冲(PERF-08:热路径不产中间 String)。
fn escape_text_into(s: &str, o: &mut String) {
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            c => o.push(c),
        }
    }
}

/// 属性值转义,直接写入输出缓冲。
///
/// 属性值内的换行/制表是数据(alt/title/data-*),原样保留 ——
/// 此前改写为空格造成字节级 L0 漂移。
fn escape_attr_into(s: &str, o: &mut String) {
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '"' => o.push_str("&quot;"),
            c => o.push(c),
        }
    }
}

/// HTML 可折叠空白 = ASCII 空白(U+00A0 不换行空格是内容,折叠或剥除
/// 都会丢「禁止换行」语义 —— 浏览器只折叠 ASCII 空白)。
pub fn is_foldable_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0C}')
}

/// 按 HTML 空白定义剥除两端空白(区别于 str::trim:后者含 U+00A0)。
pub fn trim_html_ws(s: &str) -> &str {
    s.trim_matches(is_foldable_ws)
}

/// 折叠连续空白为单空格(行内上下文)。
fn collapse_ws(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut ws = false;
    for c in s.chars() {
        if is_foldable_ws(c) {
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
        let _ = write!(out, " {}=\"", k);
        escape_attr_into(&v, out);
        out.push('"');
    }
}

/// 判断子树是否可整体行内化:无块级后代、无 Raw。
///
/// 带 [`MAX_DEPTH`] 上限(RB-02):异常深树不再内联(退回块级路径,
/// 由 `write_node` 的深度上限兜底),防 `can_inline` 递归栈溢出。
fn can_inline(node: &HtmlNode, depth: usize) -> bool {
    if depth >= MAX_DEPTH {
        return false;
    }
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
                        && can_inline(c, depth + 1)
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
/// 深度上限同 `can_inline`(RB-02)。
fn write_inline(node: &HtmlNode, depth: usize, out: &mut String) {
    match &node.data {
        NodeData::Text(t) => out.push_str(&collapse_ws(t)),
        NodeData::Comment(c) => {
            let _ = write!(out, "<!--{c}-->");
        }
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
            write_inline_content(node, depth, out);
            let _ = write!(out, "</{}>", e.name);
        }
    }
}

/// 元素子内容的行内序列化:
/// - 文本折叠;仅空白的文本节点贡献"待补空格"标记
/// - 相邻兄弟间源空白 → 输出一个空格;源无空白(如 `</b>!`)则不加
fn write_inline_content(node: &HtmlNode, depth: usize, out: &mut String) {
    let mut pending_ws = false;
    for c in &node.children {
        match &c.data {
            NodeData::Text(t) => {
                // 首尾空白探测与折叠口径一致(is_foldable_ws,不含 NBSP):
                // NBSP 是可见内容而非可折叠空白 —— 用 Unicode 口径会把
                // `&nbsp;` 邻接处误判成"源有空白",多插一个可见空格(L0 漂移)。
                let leading = t.starts_with(is_foldable_ws);
                let trailing = t.ends_with(is_foldable_ws);
                let content = collapse_ws(t);
                if content.is_empty() {
                    pending_ws = pending_ws || !t.is_empty();
                    continue;
                }
                if (pending_ws || leading) && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                escape_text_into(&content, out);
                pending_ws = trailing;
            }
            NodeData::Comment(_) | NodeData::Element(_) => {
                if pending_ws && !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                if depth < MAX_DEPTH {
                    write_inline(c, depth + 1, out);
                }
                pending_ws = false;
            }
            _ => {}
        }
    }
}

/// 块级序列化(缩进版)。`pad` 是**复用缓冲**(PERF-08:每层节点重填,
/// 不再 `"  ".repeat(indent)` 逐节点分配);深度超 [`MAX_DEPTH`] 写可见
/// 注释标记后截断(RB-02,不静默)。
fn write_node(node: &HtmlNode, indent: usize, pad: &mut String, out: &mut String) {
    pad.clear();
    for _ in 0..indent {
        pad.push_str("  ");
    }
    match &node.data {
        NodeData::Doctype(_) => {}
        NodeData::Raw(r) => {
            // 只给首行加缩进:Raw(frozen/script/head_extra)的内部是逐字节
            // 保留的原文,再导入时内部行不会被剥缩进 —— 若每行都垫 pad,
            // 每轮保存都会多吸收一层缩进(打穿 L1 字节幂等)。首行的 pad
            // 落在开标签之前,不进入下次捕获,是安全的。
            let mut lines = r.lines();
            if let Some(first) = lines.next() {
                out.push_str(pad);
                out.push_str(first.trim_end());
                out.push('\n');
            }
            for line in lines {
                out.push_str(line.trim_end());
                out.push('\n');
            }
            if r.is_empty() {
                out.push('\n');
            }
        }
        NodeData::Comment(c) => {
            let _ = writeln!(out, "{pad}<!--{c}-->");
        }
        NodeData::Text(t) => {
            let t = collapse_ws(t);
            if !t.is_empty() {
                out.push_str(pad);
                escape_text_into(&t, out);
                out.push('\n');
            }
        }
        NodeData::Element(e) => {
            if indent >= MAX_DEPTH {
                let _ = writeln!(out, "{pad}<!--vb:depth-limit-exceeded <{}>-->", e.name);
                return;
            }
            if e.is_void() {
                out.push_str(pad);
                out.push('<');
                out.push_str(&e.name);
                write_attrs(e, out);
                out.push_str(">\n");
                return;
            }
            if e.is_rawtext() {
                // script/style 是真 rawtext:内容不经实体解码,逐字节保留
                // (verbatim):任何装饰性换行/缩进都会在下次解析时进入内容,
                // 破坏 L1 幂等 —— 因此闭合标签紧跟内容。
                // pre/textarea 是 escapable rawtext:实体会被解码,写出时
                // 必须重新转义 —— 否则源里的 `&lt;` 解码成 `<` 后原样写回,
                // 下次解析变成真元素(首存与次存字节不同,L0/L1 双破)。
                // pre 是普通元素可含子元素(如 <pre><code>):有元素子节点时
                // 退回普通序列化(空白折叠一次后幂等),否则整个元素被吞掉。
                let has_element_child = node
                    .children
                    .iter()
                    .any(|c| !matches!(&c.data, NodeData::Text(_)));
                if !has_element_child {
                    let entity_decoded = matches!(e.name.as_str(), "pre" | "textarea");
                    out.push_str(pad);
                    out.push('<');
                    out.push_str(&e.name);
                    write_attrs(e, out);
                    out.push('>');
                    let mut inner = String::new();
                    for c in &node.children {
                        if let NodeData::Text(t) = &c.data {
                            inner.push_str(t);
                        }
                    }
                    if entity_decoded {
                        escape_text_into(&inner, out);
                    } else {
                        out.push_str(&inner);
                    }
                    let _ = writeln!(out, "</{}>", e.name);
                    return;
                }
            }
            out.push_str(pad);
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
            if can_inline(node, indent)
                && !node
                    .children
                    .iter()
                    .any(|c| matches!(&c.data, NodeData::Comment(c) if c.contains('\n')))
            {
                out.push('>');
                write_inline_content(node, indent, out);
                let _ = writeln!(out, "</{}>", e.name);
                return;
            }
            out.push_str(">\n");
            for c in &node.children {
                write_node(c, indent + 1, pad, out);
            }
            // 子节点递归复用了同一 pad 缓冲(PERF-08),此处必须按本层
            // 缩进**重填**再用 —— 否则闭合标签会带上最后一个子节点的
            // 缩进(L1 字节幂等被打破,此即 l1_idempotent_basic 曾红的原因)。
            pad.clear();
            for _ in 0..indent {
                pad.push_str("  ");
            }
            out.push_str(pad);
            let _ = writeln!(out, "</{}>", e.name);
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

    /// 多行 rawtext 不得在保存循环里吸收缩进(L1):内部行逐字节原样,
    /// 只有首行垫缩进(落在开标签之前,不进入下次捕获)。
    /// 此前每行垫 pad,script 体每存一轮多一层缩进,无限增长。
    #[test]
    fn multiline_rawtext_no_indent_growth() {
        let src = "<html><body><script>\nif (a) {\n    b();\n}\n</script></body></html>";
        let once = canonicalize(src);
        let twice = canonicalize(&once);
        let thrice = canonicalize(&twice);
        assert_eq!(once, twice, "首轮序列化后必须字节幂等");
        assert_eq!(twice, thrice, "循环保存不得继续变化");
        // 内容本体逐字节保留(未被垫上缩进)
        assert!(once.contains("\nif (a) {\n    b();\n}\n"));
    }

    /// pre/textarea 是 escapable rawtext:实体会被解析器解码,写出必须
    /// 重新转义 —— 否则 `&lt;code&gt;` 首存变真 `<code>` 元素(L0/L1 双破)。
    /// 对已转义源必须字节稳定;对解码后含 `<` 的文本必须回写转义。
    #[test]
    fn pre_entity_reescaping() {
        let src = "<html><body><pre>&lt;code&gt; &amp; text</pre></body></html>";
        let once = canonicalize(src);
        assert!(
            once.contains("&lt;code&gt;"),
            "已转义实体必须原样保留: {once}"
        );
        let twice = canonicalize(&once);
        assert_eq!(once, twice, "pre 实体必须字节幂等: {once}");
        // 真标记字符进入文本(解析为 text 而非元素)时也要转义回写
        let tricky = "<html><body><pre>a &lt;b</pre></body></html>";
        let out = canonicalize(tricky);
        assert!(
            out.contains("a &lt;b") && !out.contains("<pre>a <b"),
            "解码出的 < 必须转义回写: {out}"
        );
        assert_eq!(out, canonicalize(&out));
    }

    /// NBSP 是可见内容:行内首尾空白探测不得把它当可折叠空白,
    /// 否则 `&nbsp;` 邻接处多插一个可见空格(L0 漂移)。
    #[test]
    fn nbsp_not_foldable_ws() {
        let src = "<html><body><p>Hello&nbsp;world</p></body></html>";
        let once = canonicalize(src);
        assert!(
            once.contains("Hello&nbsp;world") || once.contains("Hello\u{a0}world"),
            "NBSP 必须保留: {once}"
        );
        assert!(
            !once.contains("Hello &nbsp;"),
            "不得在 NBSP 前多插空格: {once}"
        );
        assert_eq!(once, canonicalize(&once));
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

    /// DOC-01/RB-02:超过 MAX_DEPTH 的嵌套,解析必须置显式标志
    /// (导入层据此报结构化错误),序列化不得 panic 且有可见标记。
    #[test]
    fn depth_over_limit_is_flagged_and_serialize_never_panics() {
        let n = MAX_DEPTH * 3; // 1536 层,深超上限
        let mut src = String::from("<html><body>");
        for _ in 0..n {
            src.push_str("<div class=\"d\">");
        }
        src.push_str("deep");
        for _ in 0..n {
            src.push_str("</div>");
        }
        src.push_str("</body></html>");
        let dom = HtmlDom::parse(&src);
        assert!(dom.depth_exceeded, "超限必须有显式标志(不静默)");
        // 序列化:深度截断但有可见标记;不 panic
        let out = dom.serialize();
        assert!(out.contains("vb:depth-limit-exceeded"));
        // 深度恰好在上限内的文档:标志不置位,内容完整(L0 不受影响)
        let ok_n = 64;
        let mut ok = String::from("<html><body>");
        for _ in 0..ok_n {
            ok.push_str("<div>");
        }
        ok.push_str("fine");
        for _ in 0..ok_n {
            ok.push_str("</div>");
        }
        ok.push_str("</body></html>");
        let ok_dom = HtmlDom::parse(&ok);
        assert!(!ok_dom.depth_exceeded);
        assert!(ok_dom.serialize().contains("fine"));
    }

    /// walk 迭代版与旧递归版同序(前序 DFS):访问序是 collect_text 等
    /// 保真路径的行为契约。
    #[test]
    fn walk_visits_preorder() {
        let dom = HtmlDom::parse("<html><body><div>a<span>b</span>c<i>d</i></div></body></html>");
        let mut seen = Vec::new();
        dom.body().unwrap().walk(&mut |n| match &n.data {
            NodeData::Text(t) => seen.push(t.clone()),
            NodeData::Element(e) => seen.push(format!("<{}>", e.name)),
            _ => {}
        });
        assert_eq!(
            seen,
            vec!["<body>", "<div>", "a", "<span>", "b", "c", "<i>", "d"],
            "walk 必须保持前序 DFS 访问序"
        );
    }
}
