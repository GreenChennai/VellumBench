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
use vb_html::{Element, HtmlDom, HtmlNode, NodeData};

use crate::model::{Document, Geom, Node, NodeKind, TextMode};
use crate::Result;
use crate::VbError;

/// 画板标记 class(ADR-0014:vb- 统一,兼容旧前缀)。
const ARTBOARD_CLASSES: &[&str] = &["vb-artboard", "vs-artboard", "vsm-artboard"];
const LAYER_CLASSES: &[&str] = &["vb-layer", "vs-layer", "vsm-layer"];
const GROUP_CLASSES: &[&str] = &["vb-group", "vs-group", "vsm-group"];
/// 无法建模为可编辑对象的标签 → 冻结块 / 透传。
const FROZEN_TAGS: &[&str] = &[
    "svg", "iframe", "video", "audio", "canvas", "object", "embed", "template", "map", "math",
];

pub struct ImportResult {
    pub doc: Document,
    pub warnings: Vec<String>,
    /// 文档根目录(index.html 所在目录)。
    pub project_dir: PathBuf,
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

    if let Some(html_el) = dom.root.as_element() {
        if let Some(l) = html_el.attr("lang") {
            lang = l.to_string();
        }
    }
    if let Some(head) = dom.head() {
        for child in &head.children {
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
                    css_texts.push(collect_text(child));
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

    // ---- 样式表 → 类规则 ----
    let mut sheet = Stylesheet::default();
    for t in &css_texts {
        sheet.extend(parse_stylesheet(t));
    }

    // ---- body → 画板/节点 ----
    let mut doc = Document::new_empty(title.trim(), &lang);
    doc.head_extra = head_extra;
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
        pending_comment: None,
        matched_classes: Default::default(),
    };

    let mut artboard_nodes: Vec<NodeIdT> = Vec::new();
    let mut loose: Vec<&HtmlNode> = Vec::new();

    for child in &body.children {
        match &child.data {
            NodeData::Comment(c) => importer.pending_comment = Some(c.clone()),
            NodeData::Element(el) => {
                if is_artboard(el) {
                    let id = importer.build_artboard(child);
                    artboard_nodes.push(id);
                } else {
                    loose.push(child);
                }
            }
            NodeData::Text(t) if !t.trim().is_empty() => loose.push(child),
            _ => {}
        }
    }

    if artboard_nodes.is_empty() {
        // 无画板标记:整个 body 内容收进一个合成画板
        let name = if doc_title.is_empty() {
            "画板 1".to_string()
        } else {
            doc_title.clone()
        };
        let ab = importer.doc.doc_new_artboard(&name);
        for child in loose {
            importer.build_into(ab, child);
        }
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
        importer
            .warnings
            .push("未找到 vb-artboard 画板标记:已合成单一画板".to_string());
    } else if !loose.is_empty() {
        let first = artboard_nodes[0];
        for child in loose {
            importer.build_into(first, child);
        }
        importer
            .warnings
            .push("画板外存在游离内容:已并入第一个画板".to_string());
    }

    // 结束 importer 对 doc 的可变借用
    let matched = std::mem::take(&mut importer.matched_classes);
    drop(importer);

    // 孤儿类规则(没有任何元素使用)也必须保留,否则丢失(unknown 保底语义)
    for (cls, decls) in &sheet.class_rules {
        if !matched.contains(cls) {
            let body = decls
                .iter()
                .map(|d| d.to_css())
                .collect::<Vec<_>>()
                .join("; ");
            doc.raw_css.push(format!(".{cls} {{{body}}}"));
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
    })
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
    pub class_rules: Vec<(String, Vec<Decl>)>,
    /// at-rules / 复杂选择器(verbatim)。
    pub raw_blocks: Vec<String>,
    /// `:root` 中的 CSS 变量。
    pub root_vars: Vec<(String, String)>,
}

impl Stylesheet {
    fn extend(&mut self, other: Stylesheet) {
        self.class_rules.extend(other.class_rules);
        self.raw_blocks.extend(other.raw_blocks);
        self.root_vars.extend(other.root_vars);
    }

    fn decls_for_class(&self, class: &str) -> Vec<Decl> {
        let mut out = Vec::new();
        for (c, decls) in &self.class_rules {
            if c == class {
                out.extend(decls.iter().cloned());
            }
        }
        out
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
            while i < n {
                match chars[i] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    ';' if depth == 0 => {
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
        while i < n && chars[i] != '{' {
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
        while i < n {
            match chars[i] {
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
        if let Some(class) = simple_class_selector(selector) {
            sheet.class_rules.push((class, parse_decls(&body)));
        } else if !selector.is_empty() {
            sheet
                .raw_blocks
                .push(format!("{selector} {{{}}}", body.trim()));
        }
    }
    sheet
}

/// `.foo` / `tag.foo` → Some("foo");其余 None(逗号/组合器/伪类都不算)。
fn simple_class_selector(sel: &str) -> Option<String> {
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
            return Some(cls.to_string());
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
            return Some(cls.to_string());
        }
    }
    None
}

fn valid_class(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// ---------- 节点构建 ----------

struct NodeImporter<'a> {
    doc: &'a mut Document,
    sheet: &'a Stylesheet,
    warnings: &'a mut Vec<String>,
    tag_counter: std::collections::HashMap<String, u32>,
    pending_comment: Option<String>,

    ///79c16709:51fa73b08fc77684 class(5b64513f89c4521956de586b7528)
    matched_classes: std::collections::BTreeSet<String>,
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
        comment: Option<String>,
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
        let class_rule_decls = self.merged_class_decls(el);
        let inline = el.attr("style").map(parse_decls).unwrap_or_default();
        let style = merge_decls(class_rule_decls, inline);
        let get = |p: &str| style.iter().find(|d| d.prop == p).map(|d| d.value.clone());
        let w = vb_common::units::parse_px(get("width").as_deref().unwrap_or("")).unwrap_or(1440.0);
        let h = vb_common::units::parse_px(get("height").as_deref().unwrap_or("")).unwrap_or(900.0);
        // 几何属性从 style 移除(导出时由 geom 重建,避免双写)
        let mut style = style;
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
        let comment = self.pending_comment.take();
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
        for child in &el_node.children {
            self.build_into(id, child);
        }
        id
    }

    fn merged_class_decls(&self, el: &Element) -> Vec<Decl> {
        let mut out = Vec::new();
        for c in el.class_list() {
            // 标记类的规则是导出器样板(基规则),由导出层重建,不吸收进节点样式
            if ARTBOARD_CLASSES.contains(&c)
                || LAYER_CLASSES.contains(&c)
                || GROUP_CLASSES.contains(&c)
            {
                continue;
            }
            out.extend(self.sheet.decls_for_class(c));
        }
        out
    }

    /// 把一个 body/画板子元素构建为节点并挂到 parent 下。
    fn build_into(&mut self, parent: NodeIdT, node: &HtmlNode) {
        match &node.data {
            NodeData::Comment(c) => {
                self.pending_comment = Some(c.clone());
            }
            NodeData::Text(t) => {
                if !t.trim().is_empty() {
                    let sid = self.doc.alloc_sid();
                    let mut n = Node::new(
                        NodeKind::Text {
                            text: t.clone(),
                            mode: TextMode::Point,
                        },
                        "文本",
                        sid,
                    );
                    n.tag = "#text".to_string();
                    self.attach(parent, n);
                }
            }
            NodeData::Element(el) => {
                // 透传类(script):进入 trailing_raw,不参与画布
                if el.name == "script" {
                    let raw = serialize_node(node);
                    self.doc.trailing_raw.push(raw.trim().to_string());
                    return;
                }
                if el.name == "style" || el.name == "link" {
                    let raw = serialize_node(node);
                    self.doc.raw_css.push(raw);
                    return;
                }
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
            _ => {}
        }
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
        let style = merge_decls(class_rule_decls, inline);

        let (attrs, sid) = split_attrs(el, self.doc);
        let name = el
            .attr("data-vb-name")
            .map(str::to_string)
            .or_else(|| el.class_list().first().map(|c| prettify_class(c)))
            .unwrap_or_else(|| self.next_tag_name(&el.name));
        let comment = self.pending_comment.take();

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
        } else {
            let has_element_children = node.children.iter().any(|c| c.as_element().is_some());
            let all_text = collect_text(node);
            if !has_element_children
                && !all_text.trim().is_empty()
                && !matches!(el.name.as_str(), "div" | "section" | "li" | "ul" | "form")
            {
                kind = NodeKind::Text {
                    text: all_text.trim().to_string(),
                    mode: TextMode::Point,
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

        // 几何属性从 style 中移除(导出时由 geom 字段重建,避免双写)
        let mut style = style;
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
        let id = self.attach(parent, n);

        // 容器:递归子节点
        if self
            .doc
            .node(id)
            .map(|n| n.kind.is_container())
            .unwrap_or(false)
        {
            let children: Vec<&HtmlNode> = node.children.iter().collect();
            for c in children {
                self.build_into(id, c);
            }
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
            "data-vb-id" => sid = vb_common::StableId::parse(v),
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
        for (_, decls) in &sheet.class_rules {
            if decls.iter().any(|d| d.prop == "left" || d.prop == "top")
                && simple_class_selector(&format!(".{c}")) == Some(c.to_string())
            {
                return true;
            }
        }
    }
    false
}

/// class 规则在前,inline 在后覆盖(同 prop 保留后者)。
fn merge_decls(base: Vec<Decl>, over: Vec<Decl>) -> Vec<Decl> {
    let mut out = base;
    for d in over {
        if let Some(existing) = out.iter_mut().find(|e| e.prop == d.prop) {
            *existing = d;
        } else {
            out.push(d);
        }
    }
    out
}

impl Document {
    /// 导入器内部使用的追加画板。
    fn doc_new_artboard(&mut self, name: &str) -> NodeIdT {
        self.new_artboard(name, 1440.0, 900.0)
    }
}
