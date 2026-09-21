//! 导出:`Document` → canonical HTML + CSS(设计文档 04 篇 §五)。
//!
//! 输出保证:属性顺序固定、CSS 声明按 PROP_ORDER、数值 ≤4 位小数、LF 结尾
//! —— diff 最小、L1 幂等。导出经 `vb_html` 的 canonical 序列化器完成。

use std::path::Path;

use vb_common::units::fmt_num;
use vb_css::sort_decls;
use vb_html::{HtmlDom, HtmlNode, NodeData};

use crate::model::{Document, Node, NodeId, NodeKind, OutputMode, SegStyle, TextSeg};
use crate::Result;

pub struct ExportResult {
    /// (相对路径, 内容)
    pub files: Vec<(String, String)>,
}

/// 生成 index.html 与 styles/main.css(不落盘)。
pub fn render_project(doc: &Document) -> ExportResult {
    // 类名定稿(生成/去重)必须在 CSS 与 HTML 渲染之前
    let mut doc = doc.clone();
    finalize_classes(&mut doc);
    let css = render_css(&doc);
    let html = render_html(&mut doc, &css);
    let mut files = vec![("index.html".to_string(), html)];
    if matches!(doc.meta.output, OutputMode::ExternalCss) {
        files.push(("styles/main.css".to_string(), css));
    }
    ExportResult { files }
}

/// 类名定稿:无 class 的节点按命名策略生成;primary class(首类)全文档唯一
/// —— CSS 规则选择器 = 首类,冲突节点把生成的唯一类插到首位。
/// 幂等性:再导入时首类已唯一,不会再动。
fn finalize_classes(doc: &mut Document) {
    use std::collections::HashSet;
    let mut used: HashSet<String> = HashSet::new();
    let ids = ordered_source(doc);

    // ② 占名预扫(P0-2):Frozen 节点参与「类名占额」但**不造类/不改名**。
    // Frozen 的 HTML 是原样裸片段(无处写 class 属性),然而 `render_css` 对
    // 「非 `#text` 且 class 非空」的节点一律出规则 —— 它的原生类会真写在同名
    // 选择器下。若不让它占额,后面的普通节点会认领同名类作规则选择器,于是
    // 两条规则同选择器互相折叠:重导入时 Frozen 也吃到该规则样式,下一轮便
    // 多出一条重复规则(两步收敛,单步 L1 门假绿)。
    for &id in &ids {
        if let Some(n) = doc.nodes.get(id) {
            if matches!(n.kind, NodeKind::Frozen { .. }) {
                for c in &n.classes {
                    used.insert(c.clone());
                }
            }
        }
    }

    // ① 造类 / ③ 改名:首类(规则选择器)类成员级全局唯一。
    for id in ids {
        let node = match doc.nodes.get_mut(id) {
            Some(n) => n,
            None => continue,
        };
        // `#text`(行内文本段)在 HTML 里被内联进父元素正文,没有 class 属性可挂,
        // 因此不得为它生成占位类——否则导出的 CSS 会带上一条无人引用的规则,
        // 二次导入时被判为"孤儿类规则"塞进 raw_css,破坏 L1 幂等。
        // Frozen 节点同理(P0-2):导出走原样 HTML 片段,造出的类没有 HTML
        // 载体,每轮成为孤儿规则再重新生成(规则增殖);一律不造类/不改名。
        if node.tag == "#text" || matches!(node.kind, NodeKind::Frozen { .. }) {
            continue;
        }
        if node.classes.is_empty() {
            let slug = slugify(&node.name);
            let base = if slug.is_empty() {
                format!("vb-el-{}", node.sid.as_str())
            } else {
                slug
            };
            node.classes.push(base);
        }
        let primary = node.classes[0].clone();
        if used.contains(&primary) {
            let gen = format!("vb-el-{}", node.sid.as_str());
            if !node.classes.iter().any(|c| c == &gen) {
                node.classes.insert(0, gen.clone());
            }
            used.insert(gen);
            // 主类冲突即移除冲突类(P0-1 L1 稳定化的另一半):节点样式已
            // 包含该类合并后的全部声明,唯一类规则完整承载;若保留冲突类,
            // 下一轮导入会把「首个认领者的完整规则」(含其几何)误并入本
            // 节点的级联 —— 共享类漂移,L1 字节幂等被打破。
            node.classes.retain(|c| c != &primary);
        } else {
            used.insert(primary);
        }
    }
}

/// 落盘到项目目录(原子性:v0.1 直接写,断电安全的临时文件方案 v0.2 随自动保存落地)。
pub fn write_project(doc: &Document, dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let res = render_project(doc);
    let mut written = Vec::new();
    for (rel, content) in &res.files {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p, content)?;
        written.push(p);
    }
    Ok(written)
}

// ---------- HTML ----------

fn render_html(doc: &mut Document, css: &str) -> String {
    let mut html_attrs = vec![("lang".into(), doc.meta.lang.clone())];
    html_attrs.extend(doc.extra_html_attrs.clone());
    let mut html_el = HtmlNode::element("html", html_attrs);
    let mut head = HtmlNode::element("head", vec![]);
    head.children.push(HtmlNode::element(
        "meta",
        vec![("charset".into(), "UTF-8".into())],
    ));
    head.children.push(HtmlNode::element(
        "meta",
        vec![
            ("name".into(), "viewport".into()),
            (
                "content".into(),
                "width=device-width, initial-scale=1.0".into(),
            ),
        ],
    ));
    if !doc.meta.title.is_empty() {
        let mut title = HtmlNode::element("title", vec![]);
        title.children.push(HtmlNode::text(doc.meta.title.clone()));
        head.children.push(title);
    }
    for raw in &doc.head_extra {
        head.children.push(HtmlNode::raw(raw.clone()));
    }
    match doc.meta.output {
        OutputMode::ExternalCss => {
            head.children.push(HtmlNode::element(
                "link",
                vec![
                    ("rel".into(), "stylesheet".into()),
                    ("href".into(), "styles/main.css".into()),
                ],
            ));
        }
        OutputMode::InlineCss | OutputMode::SingleFile => {
            let mut style = HtmlNode::element("style", vec![]);
            style.children.push(HtmlNode::text(format!("\n{css}")));
            head.children.push(style);
        }
    }

    let mut body = HtmlNode::element("body", doc.extra_body_attrs.clone());
    let artboard_ids = doc.artboards.clone();
    for ab in artboard_ids {
        if let Some(n) = doc.nodes.get(ab) {
            for c in &n.comment_before {
                body.children.push(HtmlNode {
                    data: NodeData::Comment(c.clone()),
                    children: vec![],
                });
            }
            let n = n.clone();
            body.children.extend(render_node(doc, ab, &n));
        }
    }
    let trailing = doc.trailing_raw.clone();
    for raw in &trailing {
        body.children.push(HtmlNode::raw(raw.clone()));
    }

    html_el.children.push(head);
    html_el.children.push(body);
    let dom = HtmlDom {
        doctype: Some("html".into()),
        leading_comments: vec![],
        root: html_el,
    };
    dom.serialize()
}

/// 确定节点导出的 class 列表(已由 [`finalize_classes`] 定稿)。
fn export_classes(_doc: &mut Document, _id: NodeId, node: &Node) -> Vec<String> {
    node.classes.clone()
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    while out.starts_with(|c: char| c.is_ascii_digit()) {
        out.remove(0);
    }
    // 退化(纯数字/无字母)→ 交由调用方回退到 vb-el-<sid>
    if !out.chars().any(|c| c.is_ascii_alphabetic()) {
        return String::new();
    }
    out
}

fn render_node(doc: &mut Document, id: NodeId, node: &Node) -> Vec<HtmlNode> {
    match &node.kind {
        NodeKind::Frozen { html } => vec![HtmlNode::raw(html.clone())],
        NodeKind::Text { text, segments, .. } if node.tag == "#text" => {
            // 无段无换行 → 裸文本(与旧格式逐字节一致)
            if segments.is_empty() && !text.contains('\n') {
                return vec![HtmlNode::text(text.clone())];
            }
            text_fragment(text, segments)
        }
        _ => {
            let mut classes = export_classes(doc, id, node);
            // 容器标记类(导入器据此识别;ADR-0014)
            let marker = match node.kind {
                NodeKind::Artboard => Some("vb-artboard"),
                NodeKind::Layer => Some("vb-layer"),
                NodeKind::Group => Some("vb-group"),
                _ => None,
            };
            if let Some(m) = marker {
                if !classes.iter().any(|c| c == m) {
                    classes.insert(0, m.to_string());
                }
            }
            let tag = if node.tag == "#frozen" || node.tag.is_empty() {
                "div"
            } else {
                node.tag.as_str()
            };
            let mut attrs: Vec<(String, String)> = vec![("class".into(), classes.join(" "))];
            if let Some(id_attr) = node.attrs.get("id") {
                attrs.push(("id".into(), id_attr.clone()));
            }
            attrs.push(("data-vb-id".into(), node.sid.as_str().to_string()));
            attrs.push(("data-vb-name".into(), node.name.clone()));
            for (k, v) in &node.attrs {
                if k == "id" {
                    continue;
                }
                attrs.push((k.clone(), v.clone()));
            }
            let mut el = HtmlNode::element(tag, attrs);
            if let NodeKind::Text { text, segments, .. } = &node.kind {
                el.children.extend(text_fragment(text, segments));
            }
            let child_ids = node.children.clone();
            for &c in &child_ids {
                if let Some(cn) = doc.nodes.get(c) {
                    for cm in &cn.comment_before {
                        el.children.push(HtmlNode {
                            data: NodeData::Comment(cm.clone()),
                            children: vec![],
                        });
                    }
                    let cn = cn.clone();
                    el.children.extend(render_node(doc, c, &cn));
                }
            }
            vec![el]
        }
    }
}

/// 富文本 → HTML 片段:无样式区段 → 文本;样式区段 → `<span style>`;`\n` → `<br>`。
fn text_fragment(text: &str, segments: &[TextSeg]) -> Vec<HtmlNode> {
    let mut out: Vec<HtmlNode> = Vec::new();
    let mut pos = 0usize;
    for seg in segments {
        let (s, e) = (seg.start.min(text.len()), seg.end.min(text.len()));
        if s > pos {
            push_text_with_breaks(&text[pos..s], &mut out);
        }
        if e > s {
            let mut span =
                HtmlNode::element("span", vec![("style".into(), seg_style_attr(&seg.style))]);
            span.children = {
                let mut inner = Vec::new();
                push_text_with_breaks(&text[s..e], &mut inner);
                inner
            };
            out.push(span);
        }
        pos = pos.max(e);
    }
    if pos < text.len() {
        push_text_with_breaks(&text[pos..], &mut out);
    }
    if out.is_empty() {
        out.push(HtmlNode::text(String::new()));
    }
    out
}

fn push_text_with_breaks(s: &str, out: &mut Vec<HtmlNode>) {
    for (i, part) in s.split('\n').enumerate() {
        if i > 0 {
            out.push(HtmlNode::element("br", vec![]));
        }
        if !part.is_empty() {
            out.push(HtmlNode::text(part.to_string()));
        }
    }
}

/// 段样式 → 内联 style 值(固定属性顺序,保证 L1 幂等)。
/// 与 import `inline_style_of` 的捕获集对称:发什么捕什么,捕什么发什么。
fn seg_style_attr(st: &SegStyle) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(c) = &st.color {
        parts.push(format!("color: {c}"));
    }
    if let Some(fs) = st.font_size {
        parts.push(format!("font-size: {}px", vb_common::units::fmt_num(fs)));
    }
    if st.bold == Some(true) {
        parts.push("font-weight: 700".into());
    }
    if st.italic == Some(true) {
        parts.push("font-style: italic".into());
    }
    if let Some(ff) = &st.font_family {
        parts.push(format!("font-family: {ff}"));
    }
    if let Some(lh) = st.line_height {
        parts.push(format!("line-height: {}px", vb_common::units::fmt_num(lh)));
    }
    if let Some(ls) = st.letter_spacing {
        parts.push(format!(
            "letter-spacing: {}px",
            vb_common::units::fmt_num(ls)
        ));
    }
    if let Some(bs) = st.baseline_shift {
        parts.push(format!(
            "vertical-align: {}px",
            vb_common::units::fmt_num(bs)
        ));
    }
    let mut deco: Vec<&str> = Vec::new();
    if st.underline == Some(true) {
        deco.push("underline");
    }
    if st.strikethrough == Some(true) {
        deco.push("line-through");
    }
    if !deco.is_empty() {
        parts.push(format!("text-decoration: {}", deco.join(" ")));
    }
    parts.join("; ")
}

// ---------- CSS ----------

/// 节点**自身** `transform` 的平移分量(与 `vb_layout` 同源解析)。
///
/// `vb_doc` 不能依赖 `vb_layout`(方向相反),故实现放在 `vb_common::transform`。
fn node_own_translate(node: &Node) -> (f64, f64) {
    let tf = node
        .style
        .iter()
        .find(|d| d.prop == "transform")
        .map(|d| d.value.as_str());
    vb_common::transform::own_translate(tf, node.geom.w, node.geom.h)
}

fn geom_decls(node: &Node, is_artboard: bool) -> Vec<vb_css::Decl> {
    let mut d = Vec::new();
    let decl = |p: &str, v: String| vb_css::Decl {
        prop: p.into(),
        value: v,
        important: false,
    };
    if !is_artboard {
        d.push(decl("position", "absolute".into()));
    }
    if !is_artboard {
        // 自身 `transform` 的平移分量**补偿**:`geom` 承载的是**视觉盒**
        // (导入时已把 `left/top + translate` 折进来,见 `vb_layout`),而
        // `transform` 声明在下面会被原样保留 —— 浏览器最终位置 =
        // `left/top + translate`,故这里必须写 `geom − translate`,写 `geom`
        // 会让对象在浏览器里整体偏掉一个 translate(对齐到画板左边会跳出去),
        // 且重导入后每存一轮再漂一次,直接打穿 L1 判据 C。
        let (tx, ty) = node_own_translate(node);
        d.push(decl("left", format!("{}px", fmt_num(node.geom.x - tx))));
        d.push(decl("top", format!("{}px", fmt_num(node.geom.y - ty))));
    }
    d.push(decl("width", format!("{}px", fmt_num(node.geom.w))));
    d.push(decl("height", format!("{}px", fmt_num(node.geom.h))));
    d
}

/// 规范化(非声明)节点的 style 中要过滤的几何属性:这些节点的定位/尺寸
/// 已由 `geom` 显式承载,style 中遗留的原始几何声明(百分比锚/inset 等,
/// 声明几何节点被显式移动后遗留)不得参与级联,否则与显式值双写冲突。
const GEOM_STYLE_PROPS: &[&str] = &[
    "position", "left", "top", "right", "bottom", "inset", "width", "height",
];

/// 节点导出声明(P0-1 保真往返核心):
/// - 画板:声明尺寸由 geom 重建(width/height),其余声明原样;
/// - `geom_declared`(声明几何,未编辑):**只写原始声明**——百分比锚 /
///   inset / right|bottom / 流式语义原样往返,内存求值的 geom 不烤进 CSS;
/// - 规范化节点(显式 px,或已被显式几何编辑 materialize):由 geom 写
///   显式 px,并过滤 style 中遗留的几何声明(防双写/级联覆盖)。
fn node_decls(node: &Node) -> Vec<vb_css::Decl> {
    if matches!(node.kind, NodeKind::Artboard) {
        let mut d = geom_decls(node, true);
        d.extend(node.style.iter().cloned());
        return d;
    }
    if node.geom_declared {
        return node.style.clone();
    }
    let mut d = geom_decls(node, false);
    d.extend(
        node.style
            .iter()
            .filter(|decl| !GEOM_STYLE_PROPS.contains(&decl.prop.as_str()))
            .cloned(),
    );
    d
}

fn render_css(doc: &Document) -> String {
    let mut out = String::new();

    // :root 令牌
    if !doc.tokens.is_empty() {
        out.push_str(":root {\n");
        for (k, v) in &doc.tokens {
            out.push_str(&format!("  --{k}: {v};\n"));
        }
        out.push_str("}\n\n");
    }
    // 画板基类
    out.push_str(".vb-artboard {\n  position: relative;\n  overflow: hidden;\n}\n\n");

    // raw 块(at-rules/复杂选择器,verbatim)
    for b in &doc.raw_css {
        out.push_str(b.trim_end());
        out.push_str("\n\n");
    }

    // 每节点规则(画板序 = 文档序,DFS);class 唯一性已由 render_node 阶段保证
    let ids = ordered_source(doc);
    let mut ordered: Vec<&Node> = Vec::new();
    for &id in &ids {
        if let Some(n) = doc.nodes.get(id) {
            ordered.push(n);
        }
    }

    for node in ordered {
        // `#text` 没有 class 属性(见 finalize_classes),自然也不该有 CSS 规则。
        if node.tag == "#text" {
            continue;
        }
        if node.classes.is_empty() {
            continue;
        }
        let mut decls = node_decls(node);
        if node.hidden {
            decls.push(vb_css::Decl {
                prop: "display".into(),
                value: "none".into(),
                important: false,
            });
        }
        // 同 prop 去重(级联语义:后者覆盖前者)
        {
            let mut kept: Vec<vb_css::Decl> = Vec::with_capacity(decls.len());
            for d in decls.into_iter() {
                if let Some(existing) = kept
                    .iter_mut()
                    .find(|e: &&mut vb_css::Decl| e.prop == d.prop)
                {
                    *existing = d;
                } else {
                    kept.push(d);
                }
            }
            decls = kept;
        }
        sort_decls(&mut decls);
        if decls.is_empty() {
            continue;
        }
        // 选择器 = 首类(finalize_classes 保证唯一)
        let selector = format!(".{}", node.classes[0]);
        out.push_str(&format!("{selector} {{\n"));
        for d in &decls {
            out.push_str(&format!("  {};\n", d.to_css()));
        }
        out.push_str("}\n\n");
    }

    // 文件末尾单换行(LF)
    while out.ends_with("\n\n") {
        out.pop();
    }
    out.push('\n');
    out
}

fn ordered_source(doc: &Document) -> Vec<NodeId> {
    let mut v = Vec::new();
    for &ab in &doc.artboards {
        doc.subtree(ab, &mut v);
    }
    v
}
