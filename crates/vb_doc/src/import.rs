//! 项目目录导入:`index.html` + `styles/*.css` → `Document`。
//!
//! 保真纪律(ADR-0004 + 设计文档 04 篇 §六):
//! - 白名单外声明 → 保留在 `Node::style`(unknown 不丢弃,原样写回)
//! - 复杂选择器 / at-rules → `Document::raw_css`(verbatim)
//! - script/svg/iframe 等不可建模元素 → 冻结块 / `trailing_raw`
//! - head 中除 charset/viewport/title/link 样式表之外的元素 → `head_extra`(verbatim)
//!
//! v0.1 限制(记录于 README):**矢量模式页面**(元素用 position/left/top 定位)。
//! 流式页面的自动布局换算是 v0.8「导入兼容」的范围;当前会把无 left/top 的元素
//! 摆到画布原点并给出警告,不静默失败。

use std::path::{Path, PathBuf};

use vb_css::{parse_decls, Decl};
use vb_html::{trim_html_ws, Element, HtmlDom, HtmlNode, NodeData};

use crate::model::{Document, Geom, Node, NodeKind, SegStyle, TextMode, TextSeg};
use crate::Result;
use crate::VbError;

/// 画板标记 class(ADR-0014:vb- 统一,兼容旧前缀)。
const ARTBOARD_CLASSES: &[&str] = &["vb-artboard", "vs-artboard", "vsm-artboard"];
const LAYER_CLASSES: &[&str] = &["vb-layer", "vs-layer", "vsm-layer"];
const GROUP_CLASSES: &[&str] = &["vb-group", "vs-group", "vsm-group"];
/// 无法建模为可编辑对象的标签 → 冻结块 / 透传。
/// pre 空白敏感(折叠会毁排版),整体冻结保真。
const FROZEN_TAGS: &[&str] = &[
    "svg", "iframe", "video", "audio", "canvas", "object", "embed", "template", "map", "math",
    "pre",
];
/// 块级容器标签:打断行内分组,子内容递归建树(浏览器默认 display:block 语义)。
const BLOCK_TAGS: &[&str] = &[
    "div",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "aside",
    "nav",
    "ul",
    "ol",
    "li",
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
    "pre",
    "figure",
    "figcaption",
    "details",
    "summary",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "p",
    "address",
    "dl",
    "dt",
    "dd",
    "figure",
    "figcaption",
    "center",
];
/// 即便只含行内内容也保持为容器 Box 的标签(承载自身视觉/层级语义)。
const CONTAINER_BOX_TAGS: &[&str] = &[
    "div", "section", "article", "header", "footer", "main", "aside", "nav", "ul", "ol", "form",
    "fieldset", "table", "thead", "tbody", "tfoot", "tr", "figure", "details",
];

pub struct ImportResult {
    pub doc: Document,
    pub warnings: Vec<String>,
    /// 文档根目录(index.html 所在目录)。
    pub project_dir: PathBuf,
    /// 无画板标记时合成了单一画板(画布尺寸应由内容包围盒回填)。
    pub synthetic_artboard: bool,
    /// @font-face 声明(项目内 webfont;渲染期按 家庭+字重 选字)。
    pub font_faces: Vec<FontFace>,
}

/// 一条 @font-face(仅保留项目内渲染所需的最小集)。
#[derive(Debug, Clone)]
pub struct FontFace {
    pub family: String,
    pub weight: u16,
    /// src url(相对项目根;http 外链忽略)。
    pub src: String,
}

/// 导入项目目录或 index.html 文件。
pub fn import_project(path: &Path) -> Result<ImportResult> {
    let (project_dir, html_path) = if path.is_dir() {
        let p = path.join("index.html");
        if !p.exists() {
            return Err(VbError::Parse(format!(
                "目录中无 index.html: {}",
                path.display()
            )));
        }
        (path.to_path_buf(), p)
    } else {
        (
            path.parent().unwrap_or(Path::new(".")).to_path_buf(),
            path.to_path_buf(),
        )
    };
    let html = std::fs::read_to_string(&html_path)?;
    import_html(&html, &project_dir)
}

pub fn import_html(html: &str, project_dir: &Path) -> Result<ImportResult> {
    let dom = HtmlDom::parse(html);
    let mut warnings = Vec::new();

    // ---- head:标题 / 语言 / 样式表 / head_extra ----
    let mut title = String::new();
    let mut lang = "zh-CN".to_string();
    let mut css_texts: Vec<String> = Vec::new();
    let mut head_extra: Vec<String> = Vec::new();
    let mut extra_html_attrs: Vec<(String, String)> = Vec::new();
    let mut extra_body_attrs: Vec<(String, String)> = Vec::new();

    if let Some(html_el) = dom.root.as_element() {
        if let Some(l) = html_el.attr("lang") {
            lang = l.to_string();
        }
        // html/body 其余属性保真(此前 lang 之外全部丢失)
        for (k, v) in html_el.attrs.iter() {
            if k != "lang" && k != "data-vb-output" {
                extra_html_attrs.push((k.clone(), v.clone()));
            }
        }
        if let Some(body_el) = dom.body().and_then(|b| b.as_element()) {
            for (k, v) in body_el.attrs.iter() {
                if k != "data-vb-output" {
                    extra_body_attrs.push((k.clone(), v.clone()));
                }
            }
        }
    }
    if let Some(head) = dom.head() {
        for child in &head.children {
            // head 内注释保真(此前静默丢弃)
            if let NodeData::Comment(c) = &child.data {
                head_extra.push(format!("<!--{c}-->"));
                continue;
            }
            let Some(el) = child.as_element() else {
                continue;
            };
            match el.name.as_str() {
                "title" => {
                    title = collect_text(child);
                }
                "meta" => {
                    let is_charset = el.attr("charset").is_some();
                    let is_viewport = el.attr("name").map(|n| n == "viewport").unwrap_or(false);
                    if !is_charset && !is_viewport {
                        head_extra.push(serialize_node(child));
                    }
                }
                "link" => {
                    let rel = el.attr("rel").unwrap_or("");
                    let href = el.attr("href").unwrap_or("");
                    if rel.eq_ignore_ascii_case("stylesheet")
                        && !href.is_empty()
                        && !href.starts_with("http")
                        && !href.starts_with("//")
                    {
                        let css_path = project_dir.join(href.trim_start_matches("./"));
                        match std::fs::read_to_string(&css_path) {
                            Ok(t) => css_texts.push(t),
                            Err(_) => warnings.push(format!("样式表缺失:{href}(按无该表导入)")),
                        }
                    } else {
                        head_extra.push(serialize_node(child));
                    }
                }
                "style" => {
                    // 带非 all media 属性的样式表是条件样式,并入全局会改默认画布
                    // 表现;整块 verbatim 转投 head 透传(media 语义由浏览器保留)
                    if el
                        .attr("media")
                        .map(|m| !m.is_empty() && !m.eq_ignore_ascii_case("all"))
                        .unwrap_or(false)
                    {
                        head_extra.push(serialize_node(child));
                    } else {
                        css_texts.push(collect_text(child));
                    }
                }
                "script" => {
                    head_extra.push(serialize_node(child));
                }
                _ => {
                    head_extra.push(serialize_node(child));
                }
            }
        }
    }

    // body 子树内嵌的 <style>(存量项目常见:分段样式散在 body 各处)
    // 与 head 样式同权生效;不收集则数万字符 CSS 静默丢失(部署 GEO 实测)
    fn collect_body_styles(node: &HtmlNode, out: &mut Vec<String>) {
        if let NodeData::Element(el) = &node.data {
            if el.name.eq_ignore_ascii_case("style") {
                out.push(collect_text(node));
                return;
            }
        }
        for c in &node.children {
            collect_body_styles(c, out);
        }
    }
    if let Some(body_el) = dom.body() {
        for child in &body_el.children {
            collect_body_styles(child, &mut css_texts);
        }
    }

    // ---- 样式表 → 类规则 ----
    let mut sheet = Stylesheet::default();
    let mut font_faces: Vec<FontFace> = Vec::new();
    for t in &css_texts {
        font_faces.extend(parse_font_faces(t, &mut sheet));
    }

    // ---- body → 画板/节点 ----
    let mut doc = Document::new_empty(title.trim(), &lang);
    doc.head_extra = head_extra;
    doc.extra_html_attrs = extra_html_attrs;
    doc.extra_body_attrs = extra_body_attrs;
    doc.raw_css = sheet.raw_blocks.clone();
    // :root 变量 → 设计令牌
    for (var, value) in &sheet.root_vars {
        doc.tokens.push((var.clone(), value.clone()));
    }

    let doc_title = doc.meta.title.clone();

    let body = dom
        .body()
        .ok_or_else(|| VbError::Parse("无 <body>".into()))?;
    let mut importer = NodeImporter {
        doc: &mut doc,
        sheet: &sheet,
        warnings: &mut warnings,
        tag_counter: Default::default(),
        pending_comments: Vec::new(),
        matched_classes: Default::default(),
        ancestors: vec![("body".to_string(), Vec::new())],
        matched_rules: Default::default(),
    };

    let mut artboard_nodes: Vec<NodeIdT> = Vec::new();
    let mut loose: Vec<&HtmlNode> = Vec::new();

    for child in &body.children {
        match &child.data {
            NodeData::Comment(c) => importer.pending_comments.push(c.clone()),
            NodeData::Element(el) => {
                if el.name.eq_ignore_ascii_case("style") {
                    continue; // 已并入样式表,不建节点(UA 默认 display:none)
                }
                if is_artboard(el) {
                    let id = importer.build_artboard(child);
                    artboard_nodes.push(id);
                } else {
                    loose.push(child);
                }
            }
            NodeData::Text(t) if !trim_html_ws(t).is_empty() => loose.push(child),
            _ => {}
        }
    }

    let mut synthetic_flag = false;
    if artboard_nodes.is_empty() {
        // 无画板标记:整个 body 内容收进一个合成画板
        synthetic_flag = true;
        // body 显式约束(html,body{width/max-width/height}px)→ 画板初始几何。
        // 浏览器语义:画布宽 = body 宽(max-width 生效),横向溢出被
        // overflow-x:hidden 裁掉;body 不成为节点,约束必须摘到画板上,
        // 否则回填把 1080 宽的版面撑成 1440(部署报告 Issue 2)
        let (bw, bh) = body_explicit_size(&css_texts);
        let name = if doc_title.is_empty() {
            "画板 1".to_string()
        } else {
            doc_title.clone()
        };
        let ab = importer.doc.doc_new_artboard(&name);
        let loose_refs: Vec<&HtmlNode> = loose.to_vec();
        importer.build_children(ab, &loose_refs);
        // 估算画板高度:内容最大 y+h(下限 900)
        let mut maxb = 900.0f64;
        if let Some(ab_node) = importer.doc.node_mut(ab) {
            let kids = ab_node.children.clone();
            for k in kids {
                if let Some(n) = importer.doc.node(k) {
                    maxb = maxb.max(n.geom.y + n.geom.h + 40.0);
                }
            }
        }
        importer.doc.node_mut(ab).unwrap().geom.h = maxb;
        if bw.is_some() || bh.is_some() {
            let n = importer.doc.node_mut(ab).unwrap();
            if let Some(w) = bw {
                n.geom.w = w;
                n.authored[2] = true;
            }
            if let Some(h) = bh {
                n.geom.h = n.geom.h.max(h);
                n.authored[3] = true;
            }
        }
        importer
            .warnings
            .push("未找到 vb-artboard 画板标记:已合成单一画板".to_string());
    } else if !loose.is_empty() {
        let first = artboard_nodes[0];
        let loose_refs: Vec<&HtmlNode> = loose.to_vec();
        importer.build_children(first, &loose_refs);
        importer
            .warnings
            .push("画板外存在游离内容:已并入第一个画板".to_string());
    }

    // body 尾注释保真(此前 pending 队列在循环结束后被静默丢弃);
    // 先取出内容,待 importer 借用结束后再落盘
    let trailing_comments = if importer.pending_comments.is_empty() {
        None
    } else {
        Some(importer.pending_comments.join("\n"))
    };
    importer.pending_comments.clear();

    // 结束 importer 对 doc 的可变借用
    let matched = std::mem::take(&mut importer.matched_classes);
    let importer_matched = std::mem::take(&mut importer.matched_rules);
    drop(importer);
    if let Some(joined) = trailing_comments {
        doc.trailing_raw.push(format!("<!--{joined}-->"));
    }

    // 孤儿类规则(没有任何元素使用)也必须保留,否则丢失(unknown 保底语义)
    for (cls, _, decls) in &sheet.class_rules {
        if !matched.contains(cls) {
            let body = decls
                .iter()
                .map(|d| d.to_css())
                .collect::<Vec<_>>()
                .join("; ");
            doc.raw_css.push(format!(".{cls} {{{body}}}"));
        }
    }
    // 未命中的链规则同样回写 raw(选择器原文保真)
    for (ri, rule) in sheet.rules.iter().enumerate() {
        if !importer_matched.contains(&ri) {
            let body = rule
                .decls
                .iter()
                .map(|d| d.to_css())
                .collect::<Vec<_>>()
                .join("; ");
            doc.raw_css.push(format!("{} {{{}}}", rule.selector, body));
        }
    }

    // 画板纵向堆叠(HTML 中画板按文档流排列;编辑器画布需要显式且互不重叠的位置)
    {
        let mut y = 0.0f64;
        for &ab in &doc.artboards {
            let h = doc.nodes.get(ab).map(|n| n.geom.h).unwrap_or(600.0);
            if let Some(n) = doc.nodes.get_mut(ab) {
                n.geom.y = y;
            }
            y += h + 80.0;
        }
    }
    // body 内的 script 等透传已在 build 时进入 trailing_raw
    Ok(ImportResult {
        doc,
        warnings,
        project_dir: project_dir.to_path_buf(),
        synthetic_artboard: synthetic_flag,
        font_faces,
    })
}

/// 从样式表抽取 @font-face(家庭/字重/src);块仍按原样进 raw 保真。
fn parse_font_faces(text: &str, sheet: &mut Stylesheet) -> Vec<FontFace> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("@font-face") {
        let after = &rest[pos..];
        let Some(brace) = after.find('{') else { break };
        let Some(end_rel) = after[brace..].find('}') else {
            break;
        };
        let body = &after[brace + 1..brace + end_rel];
        let decls = parse_decls(body);
        let get = |p: &str| decls.iter().find(|d| d.prop == p).map(|d| d.value.clone());
        let family = get("font-family").map(|v| {
            let q = '\u{0027}';
            v.trim_matches('"').trim_matches(q).trim().to_string()
        });
        let src = get("src").and_then(|v| {
            let inner = v.strip_prefix("url(").and_then(|s| s.strip_suffix(')'))?;
            let q = '\u{0027}';
            let u = inner
                .split(',')
                .next()?
                .trim()
                .trim_matches('"')
                .trim_matches(q);
            if u.starts_with("http") || u.starts_with("//") {
                None
            } else {
                Some(u.to_string())
            }
        });
        if let (Some(family), Some(src)) = (family, src) {
            let weight = get("font-weight")
                .and_then(|w| w.trim().parse::<u16>().ok())
                .unwrap_or(400);
            out.push(FontFace {
                family,
                weight,
                src,
            });
        }
        rest = &rest[pos + brace + end_rel + 1..];
    }
    // 原 stylesheet 解析照常(font-face 块在 at-rule 分支进 raw)
    let parsed = parse_stylesheet(text);
    sheet.extend(parsed);
    out
}

/// 扫描样式文本,取 body(含 `html, body` 复合选择器)的显式
/// width/max-width/height(px)。取最后一次声明(CSS 级联)。
fn body_explicit_size(css_texts: &[String]) -> (Option<f64>, Option<f64>) {
    let mut w = None;
    let mut h = None;
    for css in css_texts {
        let mut rest = css.as_str();
        while let Some(brace) = rest.find('{') {
            let selector = rest[..brace].to_lowercase();
            let Some(close) = rest[brace..].find('}') else { break };
            let decls = &rest[brace + 1..brace + close];
            let applies = selector
                .split(',')
                .map(str::trim)
                .any(|sel| sel == "body" || sel == "html");
            if applies {
                for d in decls.split(';') {
                    let d = d.trim();
                    let pick = |prop: &str| -> Option<f64> {
                        d.strip_prefix(prop)
                            .and_then(|v| v.trim().strip_prefix(':'))
                            .and_then(|v| v.trim().strip_suffix("px"))
                            .and_then(|v| v.trim().parse::<f64>().ok())
                    };
                    if let Some(v) = pick("max-width").or_else(|| pick("width")) {
                        w = Some(v);
                    }
                    if let Some(v) = pick("height") {
                        h = Some(v);
                    }
                }
            }
            rest = &rest[brace + close + 1..];
        }
    }
    (w, h)
}

type NodeIdT = crate::model::NodeId;

fn is_artboard(el: &Element) -> bool {
    el.class_list().iter().any(|c| ARTBOARD_CLASSES.contains(c))
}

fn collect_text(node: &HtmlNode) -> String {
    let mut s = String::new();
    node.walk(&mut |n| {
        if let NodeData::Text(t) = &n.data {
            s.push_str(t);
        }
    });
    s
}

/// 把单个节点序列化为片段字符串(rcdom 树 → 临时 HtmlDom)。
fn serialize_node(node: &HtmlNode) -> String {
    let mut out = String::new();
    write_fragment(node, &mut out);
    out
}

fn write_fragment(node: &HtmlNode, out: &mut String) {
    // 复用 vb_html 的 canonical 写出:通过公开 canonicalize 不适用于片段,
    // 这里用简易包装:构造单节点 dom 序列化。
    let dom = HtmlDom {
        doctype: None,
        leading_comments: vec![],
        root: node.clone(),
    };
    let s = dom.serialize();
    out.push_str(s.trim_end());
}

// ---------- 样式表解析(顶层规则级) ----------

#[derive(Default)]
pub struct Stylesheet {
    /// 简单类选择器 → 声明(`.foo` / `tag.foo`)。
    pub class_rules: Vec<(String, bool, Vec<Decl>)>,
    /// 后代链选择器(`.hero .tt` / `h3` / `*` 等;含原文供孤儿回写)。
    pub rules: Vec<SelectorRule>,
    /// at-rules / 复杂选择器(verbatim)。
    pub raw_blocks: Vec<String>,
    /// `:root` 中的 CSS 变量。
    pub root_vars: Vec<(String, String)>,
}

/// 一条后代链规则;chain 从祖先到自身,匹配从右往左贪心。
#[derive(Debug, Clone)]
pub struct SelectorRule {
    pub selector: String,
    pub chain: Vec<Compound>,
    pub decls: Vec<Decl>,
}

/// 复合选择器单元:可选 tag + 类集(或 `*`)。
#[derive(Debug, Clone)]
pub struct Compound {
    pub tag: Option<String>,
    pub classes: Vec<String>,
    pub universal: bool,
}

impl Compound {
    /// 特异度 (类数, tag 数)。
    fn spec(&self) -> (usize, usize) {
        (
            self.classes.len(),
            usize::from(self.tag.is_some() || self.universal),
        )
    }
}

impl SelectorRule {
    /// 特异度 (类总数, tag 总数)。
    fn spec(&self) -> (usize, usize) {
        self.chain
            .iter()
            .fold((0, 0), |(b, c), cp| (b + cp.spec().0, c + cp.spec().1))
    }
}

impl Stylesheet {
    fn extend(&mut self, other: Stylesheet) {
        self.class_rules.extend(other.class_rules);
        self.rules.extend(other.rules);
        self.raw_blocks.extend(other.raw_blocks);
        self.root_vars.extend(other.root_vars);
    }
}

/// 字符串/转义/注释感知的扫描状态:`content: "}"`、`url(a;b)`、属性选择器里
/// 的引号、`/* } */` 注释内的花括号,都不能当成块/规则边界(CSS 规范:
/// 字符串内无特殊字符;注释只在 `*/` 结束)。
#[derive(Default)]
struct CssScan {
    in_str: Option<char>,
    escape: bool,
    in_comment: bool,
    comment_close: bool,
    pending_slash: bool,
}

impl CssScan {
    /// 推进一个字符;返回 true 表示该字符位于字符串/转义/注释内,不做边界判断。
    fn step(&mut self, c: char) -> bool {
        if self.pending_slash {
            self.pending_slash = false;
            if c == '*' {
                self.in_comment = true;
                return true;
            }
            // '/' 是字面量(calc(1/2) 等),当前字符继续正常判断
        }
        if self.in_comment {
            if self.comment_close && c == '/' {
                self.in_comment = false;
                self.comment_close = false;
            } else {
                self.comment_close = c == '*';
            }
            return true;
        }
        if let Some(q) = self.in_str {
            if self.escape {
                self.escape = false;
            } else if c == '\\' {
                self.escape = true;
            } else if c == q {
                self.in_str = None;
            }
            return true;
        }
        match c {
            '"' | '\'' => {
                self.in_str = Some(c);
                true
            }
            '/' => {
                self.pending_slash = true;
                true
            }
            _ => false,
        }
    }
}

/// 解析顶层规则(括号感知,注释剔除);子选择器/at-rule 原样保留。
pub fn parse_stylesheet(text: &str) -> Stylesheet {
    let mut sheet = Stylesheet::default();
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut i = 0usize;

    let skip_ws_and_comments = |i: &mut usize| loop {
        while *i < n && chars[*i].is_whitespace() {
            *i += 1;
        }
        if *i + 1 < n && chars[*i] == '/' && chars[*i + 1] == '*' {
            *i += 2;
            while *i + 1 < n && !(chars[*i] == '*' && chars[*i + 1] == '/') {
                *i += 1;
            }
            *i = (*i + 2).min(n);
        } else {
            break;
        }
    };

    while i < n {
        skip_ws_and_comments(&mut i);
        if i >= n {
            break;
        }
        if chars[i] == '@' {
            // at-rule:读到 ';' 或配平的 '}'
            let start = i;
            let mut depth = 0usize;
            let mut paren = 0usize;
            let mut scan = CssScan::default();
            while i < n {
                let c = chars[i];
                if scan.step(c) {
                    i += 1;
                    continue;
                }
                match c {
                    '(' => paren += 1,
                    ')' => paren = paren.saturating_sub(1),
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    // @import url(a;b) 的分号在括号内,不是规则边界
                    ';' if depth == 0 && paren == 0 => {
                        i += 1;
                        break;
                    }
                    _ => {}
                }
                i += 1;
            }
            let block: String = chars[start..i.min(n)].iter().collect();
            // at-rule(@media/@supports/@keyframes…)一律作为**冻结块**逐字保留。
            //
            // 早期实现把 @media 内部的简单类规则"平铺"到节点样式上,这有两个问题:
            // ① 语义错误:媒体查询只在特定断点生效,平铺会让默认画布显示出断点样式;
            // ② 破坏 L1 幂等:raw 块在导出时排在节点规则之前,二次导入后
            //    `merged_class_decls` 的"首个规则生效"顺序被改写,导出结果随之漂移。
            // 详见 04 篇 §四(冻结块)与 15 篇 P1 执行记录。
            sheet.raw_blocks.push(block.trim().to_string());
            continue;
        }
        // 普通规则:selector { body }
        let sel_start = i;
        let mut sel_scan = CssScan::default();
        while i < n && (sel_scan.step(chars[i]) || chars[i] != '{') {
            i += 1;
        }
        if i >= n {
            break;
        }
        let selector: String = chars[sel_start..i].iter().collect();
        let selector = selector.trim();
        i += 1; // '{'
        let body_start = i;
        let mut depth = 1usize;
        let mut body_scan = CssScan::default();
        while i < n {
            let c = chars[i];
            if body_scan.step(c) {
                i += 1;
                continue;
            }
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let body: String = chars[body_start..i.min(n)].iter().collect();
        i += 1; // '}'

        if selector == ":root" {
            for d in parse_decls(&body) {
                if let Some(var) = d.prop.strip_prefix("--") {
                    sheet.root_vars.push((var.to_string(), d.value));
                }
            }
            continue;
        }
        if let Some((class, tag_qualified)) = simple_class_selector(selector) {
            sheet
                .class_rules
                .push((class, tag_qualified, parse_decls(&body)));
        } else if let Some(selectors) = parse_selector_list(selector) {
            // 多选择器逐条解析为链规则(全部失败才回 raw)
            for chain in selectors {
                sheet.rules.push(SelectorRule {
                    selector: chain.0,
                    chain: chain.1,
                    decls: parse_decls(&body),
                });
            }
        } else if !selector.is_empty() {
            sheet
                .raw_blocks
                .push(format!("{selector} {{{}}}", body.trim()));
        }
    }
    sheet
}

/// 选择器列表(逗号分隔)→ 每条的复合链;全部解析失败返回 None。
#[allow(clippy::type_complexity)]
fn parse_selector_list(sel: &str) -> Option<Vec<(String, Vec<Compound>)>> {
    let mut out = Vec::new();
    for part in sel.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.contains('>')
            || part.contains('+')
            || part.contains('~')
            || part.contains('[')
            || part.contains(':')
            || part.contains('(')
        {
            return None;
        }
        let mut chain = Vec::new();
        for tok in part.split_whitespace() {
            let (tag, classes, universal) = parse_compound(tok)?;
            chain.push(Compound {
                tag,
                classes,
                universal,
            });
        }
        if chain.is_empty() {
            return None;
        }
        out.push((part.to_string(), chain));
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 复合单元:`div.foo.bar` / `.foo` / `h3` / `*`。
fn parse_compound(tok: &str) -> Option<(Option<String>, Vec<String>, bool)> {
    if tok == "*" {
        return Some((None, Vec::new(), true));
    }
    let mut parts = tok.split('.');
    let first = parts.next()?;
    let tag = if first.is_empty() {
        None
    } else if first.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(first.to_ascii_lowercase())
    } else {
        return None;
    };
    let mut classes = Vec::new();
    for c in parts {
        if !valid_class(c) {
            return None;
        }
        classes.push(c.to_string());
    }
    if tag.is_none() && classes.is_empty() {
        return None;
    }
    Some((tag, classes, false))
}

/// 复合单元匹配元素(tag 一致 + 类全含;universal 恒真)。
fn compound_matches_el(c: &Compound, el: &Element) -> bool {
    if c.universal {
        return true;
    }
    if let Some(t) = &c.tag {
        if el.name.to_ascii_lowercase() != *t {
            return false;
        }
    }
    let classes = el.class_list();
    c.classes.iter().all(|k| classes.iter().any(|x| x == k))
}

/// 复合单元匹配祖先上下文。
fn compound_matches_ctx(c: &Compound, tag: &str, classes: &[String]) -> bool {
    if c.universal {
        return true;
    }
    if let Some(t) = &c.tag {
        if tag.to_ascii_lowercase() != *t {
            return false;
        }
    }
    c.classes.iter().all(|k| classes.iter().any(|x| x == k))
}

/// `.foo` / `tag.foo` → Some("foo");其余 None(逗号/组合器/伪类都不算)。
fn simple_class_selector(sel: &str) -> Option<(String, bool)> {
    let sel = sel.trim();
    if sel.is_empty()
        || sel.contains(',')
        || sel.contains(' ')
        || sel.contains('>')
        || sel.contains('+')
        || sel.contains('~')
        || sel.contains('[')
        || sel.contains(':')
        || sel.contains('*')
    {
        return None;
    }
    if let Some(cls) = sel.strip_prefix('.') {
        // 单类名 `.cls`(不得再有第二个点)
        if valid_class(cls) && !cls.contains('.') {
            return Some((cls.to_string(), false));
        }
        return None;
    }
    // `tag.cls`
    if let Some((tag, cls)) = sel.split_once('.') {
        if valid_class(cls)
            && !cls.contains('.')
            && !tag.is_empty()
            && tag.chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Some((cls.to_string(), true));
        }
    }
    None
}

fn valid_class(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ---------- 行内内容分组 ----------

/// 行内内容累积器:同一行内格式化上下文的文本/`<br>`/行内元素收进一个
/// 富文本 Text 节点;`\n` 是结构性换行(来自 `<br>`),普通空白折叠为空格。
/// 空白状态跨片段连续(浏览器语义):「Hello 」+`<b>`world`</b>` 的词间空格
/// 落在后段行内,不因元素边界丢失;组首/行首空白吸收。
#[derive(Default)]
struct InlineGroup {
    text: String,
    segs: Vec<(usize, usize, SegStyle)>,
    styles: Vec<SegStyle>,
    /// 上一片段以空白结尾(下一个非空白字符前要补一个空格)。
    ends_with_ws: bool,
    /// 位于组首或 `\n` 之后(行首空白吸收)。
    line_start: bool,
}

impl InlineGroup {
    fn cur(&self) -> SegStyle {
        self.styles.last().cloned().unwrap_or_default()
    }

    fn push_text(&mut self, raw: &str) {
        let st = self.cur();
        let styled = st != SegStyle::default();
        let mut out = String::new();
        let mut pending_ws = self.ends_with_ws;
        // 只折叠 ASCII 空白(NBSP 等不空白折叠,CSS/HTML 规范语义)
        for c in raw.chars() {
            if c.is_ascii_whitespace() {
                pending_ws = true;
                continue;
            }
            if pending_ws && !(self.text.is_empty() && out.is_empty()) && !self.line_start {
                out.push(' ');
            }
            pending_ws = false;
            self.line_start = false;
            out.push(c);
        }
        if out.is_empty() {
            self.ends_with_ws = self.ends_with_ws || pending_ws;
            return;
        }
        self.ends_with_ws = pending_ws;
        if styled {
            let start = self.text.len();
            self.text.push_str(&out);
            self.segs.push((start, self.text.len(), st));
        } else {
            self.text.push_str(&out);
        }
    }

    fn push_break(&mut self) {
        let st = self.cur();
        let start = self.text.len();
        self.text.push('\n');
        if st != SegStyle::default() {
            self.segs.push((start, start + 1, st));
        }
        self.ends_with_ws = false;
        self.line_start = true;
    }

    fn push_style(&mut self, s: SegStyle) {
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        self.styles.pop();
    }
}

struct NodeImporter<'a> {
    doc: &'a mut Document,
    sheet: &'a Stylesheet,
    warnings: &'a mut Vec<String>,
    tag_counter: std::collections::HashMap<String, u32>,
    pending_comments: Vec<String>,

    ///79c16709:51fa73b08fc77684 class(5b64513f89c4521956de586b7528)
    matched_classes: std::collections::BTreeSet<String>,
    /// 祖先上下文(元素构建栈;body 起)
    ancestors: Vec<(String, Vec<String>)>,
    /// 已匹配的链规则索引(孤儿判定)
    matched_rules: std::collections::BTreeSet<usize>,
}

impl<'a> NodeImporter<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new_artboard_named(
        &mut self,
        name: &str,
        geom: Geom,
        classes: Vec<String>,
        attrs: std::collections::BTreeMap<String, String>,
        style: Vec<Decl>,
        comment: Vec<String>,
        sid: vb_common::StableId,
    ) -> NodeIdT {
        let mut n = Node::new(NodeKind::Artboard, name, sid);
        n.geom = geom;
        n.classes = classes;
        n.attrs = attrs;
        n.style = style;
        n.comment_before = comment;
        let id = self.doc.nodes.insert(n);
        let root = self.doc.root;
        self.doc.nodes.get_mut(root).unwrap().children.push(id);
        self.doc.nodes.get_mut(id).unwrap().parent = Some(root);
        self.doc.artboards.push(id);
        id
    }

    fn build_artboard(&mut self, el_node: &HtmlNode) -> NodeIdT {
        let el = el_node.as_element().unwrap();
        for c in el.class_list() {
            self.matched_classes.insert(c.to_string());
        }
        self.ancestors.push((
            el.name.clone(),
            el.class_list().iter().map(|s| s.to_string()).collect(),
        ));
        let class_rule_decls = self.merged_class_decls(el);
        let inline = el.attr("style").map(parse_decls).unwrap_or_default();
        let mut style = merge_decls(class_rule_decls, inline);
        expand_font_shorthand(&mut style);
        let get = |p: &str| style.iter().find(|d| d.prop == p).map(|d| d.value.clone());
        let w = vb_common::units::parse_px(get("width").as_deref().unwrap_or("")).unwrap_or(1440.0);
        let h = vb_common::units::parse_px(get("height").as_deref().unwrap_or("")).unwrap_or(900.0);
        let authored_wh = [
            false,
            false,
            get("width")
                .and_then(|v| vb_common::units::parse_px(&v))
                .is_some(),
            get("height")
                .and_then(|v| vb_common::units::parse_px(&v))
                .is_some(),
        ];
        // 几何属性从 style 移除(导出时由 geom 重建,避免双写)
        for p in ["position", "left", "top", "width", "height"] {
            style.retain(|d| d.prop != p);
        }
        let name = el
            .attr("data-vb-name")
            .map(str::to_string)
            .or_else(|| el.class_list().get(1).map(|c| c.to_string()))
            .unwrap_or_else(|| "画板".to_string());
        let classes: Vec<String> = el
            .class_list()
            .into_iter()
            .filter(|c| !ARTBOARD_CLASSES.contains(c))
            .map(str::to_string)
            .collect();
        let (attrs, sid) = split_attrs(el, self.doc);
        let comment = self.take_pending_comments();
        let id = self.new_artboard_named(
            &name,
            Geom {
                x: 0.0,
                y: 0.0,
                w,
                h,
            },
            classes,
            attrs,
            style,
            comment,
            sid,
        );
        if let Some(n) = self.doc.node_mut(id) {
            n.authored = authored_wh;
        }
        let child_refs: Vec<&HtmlNode> = el_node.children.iter().collect();
        self.build_children(id, &child_refs);
        self.ancestors.pop();
        id
    }

    fn merged_class_decls(&mut self, el: &Element) -> Vec<Decl> {
        let classes = el.class_list();
        let is_marker = |c: &str| {
            ARTBOARD_CLASSES.contains(&c)
                || LAYER_CLASSES.contains(&c)
                || GROUP_CLASSES.contains(&c)
        };
        let mut out = Vec::new();
        // CSS 级联:同特异度规则按**样式表出现顺序**后者胜;不同特异度按
        // (类数, tag数) 升序(特异性高的排后面,合并时后写胜)。
        // (b, c, 规则序, 声明) —— 简单类规则特异度 = (1, tag_qualified)。
        let mut matched: Vec<((usize, usize), &Vec<Decl>)> = Vec::new();
        for (c, tag_qualified, decls) in &self.sheet.class_rules {
            if classes.iter().any(|k| k == c) && !is_marker(c) {
                matched.push(((1usize, usize::from(*tag_qualified)), decls));
            }
        }
        // 链规则:自身复合匹配 + 祖先贪心向近到远
        for (ri, rule) in self.sheet.rules.iter().enumerate() {
            let last = rule.chain.len() - 1;
            if !compound_matches_el(&rule.chain[last], el) {
                continue;
            }
            if last == 0 {
                self.matched_rules.insert(ri);
                matched.push((rule.spec(), &rule.decls));
                continue;
            }
            let mut ci = last - 1;
            let mut hit = false;
            for (tag, anc_classes) in self.ancestors.iter().rev() {
                if compound_matches_ctx(&rule.chain[ci], tag, anc_classes) {
                    if ci == 0 {
                        hit = true;
                        break;
                    }
                    ci -= 1;
                }
            }
            if hit {
                self.matched_rules.insert(ri);
                matched.push((rule.spec(), &rule.decls));
            }
        }
        matched.sort_by_key(|(spec, _)| *spec);
        for (_, decls) in matched {
            out.extend(decls.iter().cloned());
        }
        out
    }

    /// 取走挂起的注释队列(合并为一条多行注释保内容;此前连续注释
    /// 只留最后一条)。空队列返回 None。
    fn take_pending_comments(&mut self) -> Vec<String> {
        // 多条注释逐条保真(此前单槽后到覆盖,连续注释只剩最后一条)
        std::mem::take(&mut self.pending_comments)
    }

    /// 把一组 body/画板子元素构建为节点(行内内容分组进富文本段)。
    fn build_children(&mut self, parent: NodeIdT, children: &[&HtmlNode]) {
        let mut g = InlineGroup::default();
        self.build_children_inner(Some(parent), children, &mut g);
        self.flush_group(parent, g);
    }

    fn build_children_inner(
        &mut self,
        parent: Option<NodeIdT>,
        children: &[&HtmlNode],
        g: &mut InlineGroup,
    ) {
        // parent=None:叶文本收集模式(调用方已保证无块级边界),只填组不建节点
        for child in children {
            match &child.data {
                NodeData::Comment(c) => {
                    // 连续注释进队列,下一个节点全部带走
                    if parent.is_some() {
                        self.pending_comments.push(c.clone());
                    }
                }
                NodeData::Text(t) => g.push_text(t),
                NodeData::Element(el) => {
                    match el.name.as_str() {
                        // 透传类(script):进入 trailing_raw,不参与画布
                        "script" => {
                            if let Some(p) = parent {
                                self.flush_group(p, std::mem::take(g));
                                let raw = serialize_node(child);
                                self.doc.trailing_raw.push(raw.trim().to_string());
                            }
                        }
                        // body 里的样式表/样式链接进 head_extra(HTML 侧透传)
                        "style" | "link" => {
                            if let Some(p) = parent {
                                self.flush_group(p, std::mem::take(g));
                                let raw = serialize_node(child);
                                self.doc.head_extra.push(raw.trim().to_string());
                            }
                        }
                        "br" => g.push_break(),
                        "wbr" => {}
                        "hr" | "img" => {
                            if let Some(p) = parent {
                                self.flush_group(p, std::mem::take(g));
                                self.build_node_into(p, child);
                            }
                        }
                        _ if self.is_block_boundary(el) => {
                            if let Some(p) = parent {
                                self.flush_group(p, std::mem::take(g));
                                self.build_node_into(p, child);
                            }
                        }
                        _ => {
                            // 行内元素:样式入栈,子内容续入同一分组
                            let st = self.inline_style_of(el);
                            let styled = st != SegStyle::default();
                            if styled {
                                g.push_style(st);
                            }
                            let inner: Vec<&HtmlNode> = child.children.iter().collect();
                            self.build_children_inner(parent, &inner, g);
                            if styled {
                                g.pop_style();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// 单个块级子元素 → 节点(含「无 left/top 定位」告警)。
    fn build_node_into(&mut self, parent: NodeIdT, node: &HtmlNode) {
        let id = self.build_node(node, parent);
        if id.is_some() && !has_explicit_position(node, self.sheet) {
            if let Some(el2) = node.as_element() {
                self.warnings.push(format!(
                    "元素 <{}> 无 left/top 定位(流式页面),已摆到 (0,0)",
                    el2.name
                ));
            }
        }
    }

    /// 分组定稿:修剪首尾空白(偏移平移),合并相邻同样式段;空文本返回 None。
    fn finalize_group(g: &InlineGroup) -> Option<(String, Vec<TextSeg>)> {
        let raw = &g.text;
        // 只修剪 ASCII 空白(NBSP 是可见内容)
        let text = raw.trim_matches(|c: char| c.is_ascii_whitespace());
        if text.is_empty() {
            return None;
        }
        let lead = raw.len() - raw.trim_start().len();
        let end_lim = lead + text.len();
        let mut segs: Vec<TextSeg> = Vec::new();
        for (s, e, st) in &g.segs {
            let (ns, ne) = (
                s.saturating_sub(lead).min(text.len()),
                (*e).clamp(lead, end_lim) - lead,
            );
            if ns >= ne {
                continue;
            }
            // 相邻同样式段合并(包括与无样式邻接的边界情形不做——无样式不记段)
            if let Some(last) = segs.last_mut() {
                if last.style == *st && last.end == ns {
                    last.end = ne;
                    continue;
                }
            }
            segs.push(TextSeg {
                start: ns,
                end: ne,
                style: st.clone(),
            });
        }
        Some((text.to_string(), segs))
    }

    fn flush_group(&mut self, parent: NodeIdT, g: InlineGroup) {
        let Some((text, segments)) = Self::finalize_group(&g) else {
            return;
        };
        let sid = self.doc.alloc_sid();
        let mut n = Node::new(
            NodeKind::Text {
                text,
                mode: TextMode::Point,
                segments,
            },
            "文本",
            sid,
        );
        n.tag = "#text".to_string();
        self.attach(parent, n);
    }

    /// 元素是否打断行内分组(显式 display 覆盖优先,span.tl{display:block} 是块)。
    fn is_block_boundary(&mut self, el: &Element) -> bool {
        // 链接保持独立节点(href/aria 等属性与身份不丢);冻结标签同理
        if el.name == "a" || FROZEN_TAGS.contains(&el.name.as_str()) {
            return true;
        }
        let mut decls = self.merged_class_decls(el);
        if let Some(s) = el.attr("style") {
            decls = merge_decls(decls, parse_decls(s));
        }
        // 定位框(absolute/fixed,或声明了 left/top)必须独立成节点:
        // 否则贴纸/角标这类定位行内元素会被并进文本段丢失定位与背景
        let mut has_left_top = false;
        for d in &decls {
            match d.prop.as_str() {
                "position" if matches!(d.value.trim(), "absolute" | "fixed") => return true,
                "left" | "top" => has_left_top = true,
                _ => {}
            }
        }
        if has_left_top {
            return true;
        }
        if let Some(d) = decls
            .iter()
            .find(|d| d.prop == "display")
            .map(|d| d.value.trim().to_string())
        {
            match d.as_str() {
                "inline" => {}
                // display:none 单独成节点(hidden 保真),同样打断分组
                _ => return true,
            }
        }
        BLOCK_TAGS.contains(&el.name.as_str())
    }

    /// 行内元素的样式覆盖(color/粗斜体/字号/字族)。
    /// 语义标签的 UA 默认样式(b/strong→粗,em/i→斜)在此落为显式覆盖。
    fn inline_style_of(&mut self, el: &Element) -> SegStyle {
        let mut decls = self.merged_class_decls(el);
        if let Some(s) = el.attr("style") {
            decls = merge_decls(decls, parse_decls(s));
        }
        let get = |p: &str| decls.iter().find(|d| d.prop == p).map(|d| d.value.clone());
        let semantic_bold =
            matches!(el.name.as_str(), "b" | "strong") && get("font-weight").is_none();
        let semantic_italic = matches!(el.name.as_str(), "em" | "i" | "cite" | "dfn" | "var")
            && get("font-style").is_none();
        SegStyle {
            color: get("color"),
            bold: get("font-weight")
                .map(|v| matches!(v.as_str(), "bold" | "600" | "700" | "800" | "900"))
                .or(if semantic_bold { Some(true) } else { None }),
            italic: get("font-style")
                .map(|v| v == "italic" || v == "oblique")
                .or(if semantic_italic { Some(true) } else { None }),
            font_size: get("font-size").and_then(|v| vb_common::units::parse_px(&v)),
            font_family: get("font-family"),
        }
    }

    /// 子内容是否全部可内联(决定元素成为叶文本还是容器 Box)。
    fn all_inline(&mut self, node: &HtmlNode) -> bool {
        node.children.iter().all(|c| match &c.data {
            NodeData::Text(_) | NodeData::Comment(_) => true,
            NodeData::Element(el) => match el.name.as_str() {
                "br" | "wbr" => true,
                "img" | "script" | "style" | "link" | "hr" => false,
                _ if self.is_block_boundary(el) => false,
                _ => self.all_inline(c),
            },
            _ => true,
        })
    }

    fn attach(&mut self, parent: NodeIdT, n: Node) -> NodeIdT {
        let id = self.doc.nodes.insert(n);
        self.doc.nodes.get_mut(parent).unwrap().children.push(id);
        self.doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        id
    }

    fn next_tag_name(&mut self, tag: &str) -> String {
        let c = self.tag_counter.entry(tag.to_string()).or_insert(0);
        *c += 1;
        match tag {
            "div" => format!("矩形 {}", c),
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "span" => format!("文本 {}", c),
            "img" => format!("图像 {}", c),
            _ => format!("{} {}", tag, c),
        }
    }

    fn build_node(&mut self, node: &HtmlNode, parent: NodeIdT) -> Option<NodeIdT> {
        let el = node.as_element()?;
        for c in el.class_list() {
            self.matched_classes.insert(c.to_string());
        }
        let class_rule_decls = self.merged_class_decls(el);
        let inline = el.attr("style").map(parse_decls).unwrap_or_default();
        let mut style = merge_decls(class_rule_decls, inline);
        expand_font_shorthand(&mut style);

        let (attrs, sid) = split_attrs(el, self.doc);
        let name = el
            .attr("data-vb-name")
            .map(str::to_string)
            .or_else(|| el.class_list().first().map(|c| prettify_class(c)))
            .unwrap_or_else(|| self.next_tag_name(&el.name));
        let comment = self.take_pending_comments();

        let kind: NodeKind;
        let mut tag = el.name.clone();

        if FROZEN_TAGS.contains(&el.name.as_str()) {
            let raw = serialize_node(node);
            kind = NodeKind::Frozen { html: raw };
            tag = "#frozen".to_string();
        } else if el.name == "img" {
            let src = el.attr("src").unwrap_or("").to_string();
            if src.is_empty() {
                self.warnings.push("<img> 缺少 src".into());
            }
            kind = NodeKind::Image { src };
        } else if matches!(el.name.as_str(), "br" | "hr" | "wbr") {
            // 换行/分隔线等空布局元素:冻结原样保留(此前建成
            // 100×100 幽灵 Box,还生成 position:absolute 规则)
            let raw = serialize_node(node);
            kind = NodeKind::Frozen { html: raw };
        } else {
            let all_text = collect_text(node);
            let has_text = !trim_html_ws(&all_text).is_empty();
            let inline_only = self.all_inline(node);
            let is_box_tag = CONTAINER_BOX_TAGS.contains(&el.name.as_str());
            if !is_box_tag && inline_only && has_text {
                // 叶文本节点:行内子内容(<br>/<span> 等)吸收为富文本段
                let mut g = InlineGroup::default();
                let inner: Vec<&HtmlNode> = node.children.iter().collect();
                self.build_children_inner(None, &inner, &mut g);
                let (text, segments) = Self::finalize_group(&g)
                    .unwrap_or((trim_html_ws(&all_text).to_string(), Vec::new()));
                kind = NodeKind::Text {
                    text,
                    mode: TextMode::Point,
                    segments,
                };
            } else {
                kind = NodeKind::Box;
            }
        }

        let classes: Vec<String> = el
            .class_list()
            .into_iter()
            .filter(|c| {
                !ARTBOARD_CLASSES.contains(c)
                    && !LAYER_CLASSES.contains(c)
                    && !GROUP_CLASSES.contains(c)
            })
            .map(str::to_string)
            .collect();

        // 几何:style(合并后)的 left/top/width/height
        let get = |p: &str| style.iter().find(|d| d.prop == p).map(|d| d.value.clone());
        let px = |v: &Option<String>| v.as_deref().and_then(vb_common::units::parse_px);
        let x = px(&get("left")).unwrap_or(0.0);
        let y = px(&get("top"))
            .or_else(|| px(&get("margin-top")))
            .unwrap_or(0.0);
        let w = px(&get("width")).unwrap_or(match kind {
            NodeKind::Text { .. } => 200.0,
            _ => 100.0,
        });
        let h = px(&get("height"))
            .or_else(|| px(&get("min-height")))
            .unwrap_or_else(|| match &kind {
                NodeKind::Text { .. } => {
                    let fs = px(&get("font-size")).unwrap_or(16.0);
                    (fs * 1.6).ceil()
                }
                _ => 100.0,
            });
        let position_authored = get("position")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        // 作者显式声明的维度(布局层区分「显式」与「默认占位」)
        let authored = [
            px(&get("left")).is_some(),
            px(&get("top")).is_some(),
            px(&get("width")).is_some(),
            px(&get("height")).is_some(),
        ];

        // 几何属性从 style 中移除(导出时由 geom 字段重建,避免双写)
        for p in ["left", "top", "width", "height", "position"] {
            style.retain(|d| d.prop != p);
        }

        // `display: none` 回读为 hidden 标志,与导出侧「hidden → display:none」对称。
        // 不做这一步的话,隐藏对象 → 保存 → 重新打开,图层眼睛是睁着的但对象不可见
        // —— 又一处"界面说谎"。其余 display 值(flex/block…)原样保留。
        let hidden = style
            .iter()
            .any(|d| d.prop == "display" && d.value.trim() == "none");
        if hidden {
            style.retain(|d| !(d.prop == "display" && d.value.trim() == "none"));
        }

        let mut n = Node::new(kind, name, sid);
        n.tag = tag;
        n.classes = classes;
        n.attrs = attrs;
        n.style = style;
        n.hidden = hidden;
        n.comment_before = comment;
        n.geom = Geom { x, y, w, h };
        n.authored = authored;
        n.authored_position = position_authored.clone();
        let id = self.attach(parent, n);

        // 容器:递归子节点(行内内容分组进富文本段);压栈自身上下文供
        // 后代链选择器匹配
        if self
            .doc
            .node(id)
            .map(|n| n.kind.is_container())
            .unwrap_or(false)
        {
            self.ancestors.push((
                el.name.clone(),
                el.class_list().iter().map(|s| s.to_string()).collect(),
            ));
            let children: Vec<&HtmlNode> = node.children.iter().collect();
            self.build_children(id, &children);
            self.ancestors.pop();
        }
        Some(id)
    }
}

/// class 名 → 展示名(kebab → 词首大写)。
fn prettify_class(c: &str) -> String {
    let mut out = String::new();
    for (i, part) in c.split(['-', '_']).filter(|p| !p.is_empty()).enumerate() {
        let mut ch = part.chars();
        let first = ch
            .next()
            .map(|f| f.to_uppercase().to_string())
            .unwrap_or_default();
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&first);
        out.push_str(ch.as_str());
    }
    if out.is_empty() {
        c.to_string()
    } else {
        out
    }
}

/// 拆分属性:data-vb-id → sid;data-vb-name → 名称;class/style 拿走;其余保留。
fn split_attrs(
    el: &Element,
    doc: &mut Document,
) -> (
    std::collections::BTreeMap<String, String>,
    vb_common::StableId,
) {
    let mut attrs = std::collections::BTreeMap::new();
    let mut sid: Option<vb_common::StableId> = None;
    for (k, v) in &el.attrs {
        match k.as_str() {
            "class" | "style" => {}
            "data-vb-id" => {
                // sid 是全文档唯一身份:手编 HTML 的重复 data-vb-id(或与
                // 短码撞码)会让 find_by_sid 命中错误节点,弃用并重分配
                let parsed = vb_common::StableId::parse(v).filter(|s| !doc.sid_in_use(s.as_str()));
                sid = Some(parsed.unwrap_or_else(|| doc.alloc_sid()));
            }
            "data-vb-name" => {}
            _ => {
                attrs.insert(k.clone(), v.clone());
            }
        }
    }
    (attrs, sid.unwrap_or_else(|| doc.alloc_sid()))
}

fn has_explicit_position(node: &HtmlNode, sheet: &Stylesheet) -> bool {
    let Some(el) = node.as_element() else {
        return false;
    };
    if el
        .attr("style")
        .map(|s| s.contains("left") || s.contains("top"))
        .unwrap_or(false)
    {
        return true;
    }
    for c in el.class_list() {
        for (_, _, decls) in &sheet.class_rules {
            if decls.iter().any(|d| d.prop == "left" || d.prop == "top")
                && matches!(simple_class_selector(&format!(".{c}")), Some((cl, _)) if cl == c)
            {
                return true;
            }
        }
    }
    false
}

/// class 规则在前,inline 在后覆盖(同 prop 保留后者)。
fn merge_decls(base: Vec<Decl>, over: Vec<Decl>) -> Vec<Decl> {
    // 同 prop 后者胜:类规则级联展开后 base 自身可能同 prop 多条
    // (如两个类都设 color),不折叠会导致 style_get 取首条而导出 CSS
    // 由浏览器取末条,画布与落盘语义漂移
    let mut out: Vec<Decl> = Vec::new();
    for d in base.into_iter().chain(over) {
        if let Some(existing) = out.iter_mut().find(|e| e.prop == d.prop) {
            *existing = d;
        } else {
            out.push(d);
        }
    }
    out
}

/// 展开font 简写为单项声明(导入期一次性;此后 L1 在展开形态上稳定)。
/// 语法:`[<style> || <variant> || <weight>]? <size>[/<line-height>]? <family>`。
/// artboard 模板大量使用 `font:700 26px/1 'MiSans'` 形态,不展开则字号全丢。
fn expand_font_shorthand(style: &mut Vec<Decl>) {
    let Some(pos) = style.iter().position(|d| d.prop == "font") else {
        return;
    };
    let raw = style[pos].value.trim().to_string();
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.len() < 2 {
        return;
    }
    let mut i = 0usize;
    let mut weight: Option<String> = None;
    let mut fstyle: Option<String> = None;
    while i < tokens.len() {
        let t = tokens[i];
        if t == "normal" {
            i += 1;
            continue;
        }
        if t == "italic" || t == "oblique" {
            fstyle = Some(t.to_string());
            i += 1;
            continue;
        }
        if t == "small-caps" {
            i += 1;
            continue;
        }
        if t == "bold" || t == "bolder" || t == "lighter" || t.parse::<u16>().is_ok() {
            weight = Some(t.to_string());
            i += 1;
            continue;
        }
        break;
    }
    if i >= tokens.len() {
        return;
    }
    // 字号 [/ 行高]
    let size_tok = tokens[i];
    let (size_raw, lh_raw) = match size_tok.split_once('/') {
        Some((s, l)) => (s, Some(l)),
        None => (size_tok, None),
    };
    let size_ok = size_raw
        .strip_suffix('%')
        .map(|_| true)
        .unwrap_or_else(|| vb_common::units::parse_px(size_raw).is_some());
    if !size_ok {
        return;
    }
    let family = tokens[i + 1..].join(" ");
    if family.is_empty() {
        return;
    }
    let d = |p: &str, v: String| Decl {
        prop: p.into(),
        value: v,
        important: false,
    };
    let mut expanded = Vec::new();
    expanded.push(d("font-size", size_raw.to_string()));
    if let Some(l) = lh_raw {
        expanded.push(d("line-height", l.to_string()));
    }
    if let Some(w) = weight {
        expanded.push(d("font-weight", w));
    }
    if let Some(st) = fstyle {
        expanded.push(d("font-style", st));
    }
    expanded.push(d("font-family", family));
    // 就地替换 font 声明
    style.splice(pos..=pos, expanded);
}

impl Document {
    /// 导入器内部使用的追加画板。
    fn doc_new_artboard(&mut self, name: &str) -> NodeIdT {
        self.new_artboard(name, 1440.0, 900.0)
    }
}
