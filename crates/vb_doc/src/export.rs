//! 导出:`Document` → canonical HTML + CSS(设计文档 04 篇 §五)。
//!
//! 输出保证:属性顺序固定、CSS 声明按 PROP_ORDER、数值 ≤4 位小数、LF 结尾
//! —— diff 最小、L1 幂等。导出经 `vb_html` 的 canonical 序列化器完成。

use std::collections::HashMap;
use std::fmt::Write as _;
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
///
/// PERF-03(2026-10-05):**零整树克隆**。旧实现在这里 `doc.clone()`
/// (整棵 arena + 全部声明列表)只为让 `finalize_classes` 就地改类名;
/// 现在类名定稿改为**纯函数求解**( [`resolve_final_classes`] 产出
/// `NodeId → 定稿 classes` 侧表),渲染全程借用 `&Document` ——
/// 调用方的文档在导出期间零改动、零复制(只读导出路径;
/// 不可变语义由 `export_does_not_mutate_document` 单测钉住)。
pub fn render_project(doc: &Document) -> ExportResult {
    // 类名定稿(生成/去重)必须在 CSS 与 HTML 渲染之前
    let classes = resolve_final_classes(doc);
    let css = render_css(doc, &classes);
    let html = render_html(doc, &classes, &css);
    let mut files = vec![("index.html".to_string(), html)];
    if matches!(doc.meta().output, OutputMode::ExternalCss) {
        files.push(("styles/main.css".to_string(), css));
    }
    ExportResult { files }
}

/// 节点定稿类表:NodeId → 最终 classes(未参与定稿的节点不在表内,
/// 渲染层回退节点自身 classes)。
type FinalClasses = std::collections::HashMap<NodeId, Vec<String>>;

/// 按节点 id 取定稿 classes(表未命中 → 节点自身 classes)。
fn classes_of<'a>(
    resolved: &'a FinalClasses,
    doc: &'a Document,
    id: NodeId,
) -> Option<&'a [String]> {
    let n = doc.node(id)?;
    Some(
        resolved
            .get(&id)
            .map(|v| v.as_slice())
            .unwrap_or(n.classes.as_slice()),
    )
}

/// 类名定稿(纯函数版,PERF-03):无 class 的节点按命名策略生成;
/// primary class(首类)全文档唯一 —— CSS 规则选择器 = 首类,冲突节点把
/// 生成的唯一类插到首位。幂等性:再导入时首类已唯一,不会再动。
///
/// 旧实现(`finalize_classes`)在 `doc.clone()` 上**就地改** `classes`;
/// 本版把同一套规则改写为「读取 → 侧表」求解,返回
/// `NodeId → 定稿 classes`。逐分支与旧实现对齐(单测
/// `resolved_classes_match_legacy_inplace` 用就地版对照钉住等价)。
fn resolve_final_classes(doc: &Document) -> FinalClasses {
    use std::collections::HashSet;
    let mut used: HashSet<String> = HashSet::new();
    let ids = ordered_source(doc);
    let mut resolved: FinalClasses = HashMap::new();

    // ② 占名预扫(P0-2):Frozen 节点参与「类名占额」但**不造类/不改名**。
    // Frozen 的 HTML 是原样裸片段(无处写 class 属性),然而 `render_css` 对
    // 「非 `#text` 且 class 非空」的节点一律出规则 —— 它的原生类会真写在同名
    // 选择器下。若不让它占额,后面的普通节点会认领同名类作规则选择器,于是
    // 两条规则同选择器互相折叠:重导入时 Frozen 也吃到该规则样式,下一轮便
    // 多出一条重复规则(两步收敛,单步 L1 门假绿)。
    for &id in &ids {
        if let Some(n) = doc.node(id) {
            if matches!(n.kind, NodeKind::Frozen { .. }) {
                for c in &n.classes {
                    used.insert(c.clone());
                }
            }
        }
    }

    // ① 造类 / ③ 改名:首类(规则选择器)类成员级全局唯一。
    for id in ids {
        let node = match doc.node(id) {
            Some(n) => n,
            None => continue,
        };
        // `#text`(行内文本段)在 HTML 里被内联进父元素正文,没有 class 属性可挂,
        // 因此不得为它生成占位类——否则导出的 CSS 会带上一条无人引用的规则,
        // 二次导入时被判为"孤儿类规则"塞进 raw_css,破坏 L1 幂等。
        // Frozen 节点同理(P0-2):导出走原样 HTML 片段,造出的类没有 HTML
        // 载体,每轮成为孤儿规则再重新生成(规则增殖);一律不造类/不改名。
        // 05-8:主件定义容器同理 —— 标记类 vb-symbol-defs 由导出侧补写,
        // 容器本身不造类(多个容器同标记类会互相冲突触发改名,反而破坏
        // 定义区识别);其原型子树节点照常定稿。
        let is_def_container = node.parent == Some(doc.defs_root());
        if node.tag == "#text" || is_def_container || matches!(node.kind, NodeKind::Frozen { .. }) {
            continue;
        }
        // 工作副本:先继承当前 classes(可能已被前序定稿写入侧表)
        let mut classes: Vec<String> = resolved
            .get(&id)
            .cloned()
            .unwrap_or_else(|| node.classes.clone());
        if classes.is_empty() {
            let slug = slugify(&node.name);
            let base = if slug.is_empty() {
                format!("vb-el-{}", node.sid.as_str())
            } else {
                slug
            };
            classes.push(base);
        }
        let primary = classes[0].clone();
        if used.contains(&primary) {
            // 05-8 符号同步的产物:实例是主件子树的克隆,二者类列表相同;
            // 主件此前被定稿出的生成类(vb-el-<主件 sid>)会与克隆体撞名,
            // 旧实现「gen 已在 classes 里就不插入,再移除 primary」会把节点
            // 清成**空类** —— CSS 规则被跳过,样式在下一轮导入时丢失。
            // 修复:gen 与已有类/已用类冲突时,确定性追加序号后缀兜底。
            let mut gen = format!("vb-el-{}", node.sid.as_str());
            let mut seq = 2usize;
            while classes.iter().any(|c| c == &gen) || used.contains(&gen) {
                gen = format!("vb-el-{}-{seq}", node.sid.as_str());
                seq += 1;
            }
            classes.insert(0, gen.clone());
            used.insert(gen);
            // 主类冲突即移除冲突类(P0-1 L1 稳定化的另一半):节点样式已
            // 包含该类合并后的全部声明,唯一类规则完整承载;若保留冲突类,
            // 下一轮导入会把「首个认领者的完整规则」(含其几何)误并入本
            // 节点的级联 —— 共享类漂移,L1 字节幂等被打破。
            classes.retain(|c| c != &primary);
        } else {
            used.insert(primary);
        }
        resolved.insert(id, classes);
    }
    resolved
}

/// 落盘到项目目录(断电安全 · 原子写 v0.2,已落地)。
///
/// 纪律(与 `vb_app::autosave` 快照提交同口径的临时文件方案,但覆盖更强):
/// - **原子覆盖**:每个文件先写**同目录**临时文件 `.tmp-<原文件名>-<pid>-<线程id>`
///   (同目录保证与目标同盘,`rename` 才是原子的),再 `std::fs::rename`
///   覆盖目标 —— Windows 的 `fs::rename` 带 REPLACE_EXISTING 语义,可直接
///   覆盖已存在文件。任何时刻断电/崩溃,目标要么是完整旧文件、要么是完整
///   新文件;旧实现(v0.1)直接 `fs::write` 目标,保存/导出中途断电会留下
///   截断的 `index.html`,文档永久损坏。
/// - **index.html 压轴**:多文件项目(ExternalCss)css/资产先落,
///   `index.html` 最后写 —— 磁盘上「存在完整 index.html」即是「文档可打开」
///   的哨兵,把保存中途失败时的可打开概率最大化。
/// - **失败不留垃圾**:临时文件写失败或 rename 失败都会清理临时文件;
///   rename 失败的错误信息带目标路径(日志定位用)。
///
/// 返回按**落盘序**(css/资产在前,`index.html` 最后)排列的绝对路径。
pub fn write_project(doc: &Document, dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let res = render_project(doc);
    let mut written = Vec::new();
    // render_project 产物里 index.html 恒在首位 —— 倒序遍历即「资产先落、
    // index.html 压轴」。
    for (rel, content) in res.files.iter().rev() {
        let p = dir.join(rel);
        atomic_write(&p, content)?;
        written.push(p);
    }
    Ok(written)
}

/// 临时文件名后缀用的线程 id(DOC-08):pid 只隔离进程,同进程多线程
/// 并发导出同一文件时会共用同一临时名互相覆盖,削弱原子性。
///
/// 稳定写法:`ThreadId::as_u64()` 是 unstable API,且标准库不暴露数值;
/// `ThreadId` 的 `Debug`/`Hash` 均派生自其进程内唯一编号 —— 这里对
/// Debug 文本做 FNV-1a(确定性、跨次运行一致,不用随机种子的
/// DefaultHasher),同进程内不同线程 → 不同哈希,足够唯一(输入空间
/// 只有本进程线程数,64 位碰撞概率可忽略)。
fn current_thread_id_u64() -> u64 {
    let repr = format!("{:?}", std::thread::current().id());
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a 64-bit offset basis
    for b in repr.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3); // FNV prime
    }
    h
}

/// 单文件原子写:同目录临时文件 `.tmp-<名>-<pid>-<线程id>` → `rename` 原子覆盖目标。
fn atomic_write(target: &Path, content: &str) -> Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            crate::VbError::Io(std::io::Error::new(
                e.kind(),
                format!("创建目录 {} 失败:{e}", parent.display()),
            ))
        })?;
    }
    let file_name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // 临时文件与目标同目录(同盘 → rename 原子);带 pid + 线程 id,
    // 并发进程/并发线程互不踩踏(DOC-08)。
    let tmp = target.with_file_name(format!(
        ".tmp-{file_name}-{}-{:x}",
        std::process::id(),
        current_thread_id_u64()
    ));
    let result = std::fs::write(&tmp, content).and_then(|()| std::fs::rename(&tmp, target));
    if let Err(e) = result {
        // 失败清理:不留 .tmp 残留(清理本身失败不掩盖原始错误)。
        let _ = std::fs::remove_file(&tmp);
        return Err(crate::VbError::Io(std::io::Error::new(
            e.kind(),
            format!(
                "原子写 {} 失败(临时文件 {}):{e}",
                target.display(),
                tmp.display()
            ),
        )));
    }
    Ok(())
}

// ---------- HTML ----------

fn render_html(doc: &Document, resolved: &FinalClasses, css: &str) -> String {
    let meta = doc.meta();
    let mut html_attrs = vec![("lang".into(), meta.lang.clone())];
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
    if !meta.title.is_empty() {
        let mut title = HtmlNode::element("title", vec![]);
        title.children.push(HtmlNode::text(meta.title.clone()));
        head.children.push(title);
    }
    for raw in &doc.head_extra {
        head.children.push(HtmlNode::raw(raw.clone()));
    }
    match meta.output {
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
    let artboard_ids = doc.artboards().to_vec();
    for ab in artboard_ids {
        if let Some(n) = doc.node(ab) {
            for c in &n.comment_before {
                body.children.push(HtmlNode {
                    data: NodeData::Comment(c.clone()),
                    children: vec![],
                });
            }
            body.children.extend(render_node(doc, resolved, ab, n));
        }
    }
    // ── 05-8 主件定义区(ADR-VB-L10):画板内容之后、透传之前 ──
    // 容器 = `<div class="vb-symbol-defs" hidden …>`,hidden 为原生属性,
    // 浏览器不渲染;原型子树是真实节点,CSS 规则照常输出(见 ordered_source)。
    {
        let defs_root = doc.defs_root();
        let def_ids = doc
            .node(defs_root)
            .map(|r| r.children.clone())
            .unwrap_or_default();
        for did in def_ids {
            if let Some(n) = doc.node(did) {
                for c in &n.comment_before {
                    body.children.push(HtmlNode {
                        data: NodeData::Comment(c.clone()),
                        children: vec![],
                    });
                }
                body.children.extend(render_node(doc, resolved, did, n));
            }
        }
    }
    for raw in &doc.trailing_raw {
        body.children.push(HtmlNode::raw(raw.clone()));
    }

    html_el.children.push(head);
    html_el.children.push(body);
    let dom = HtmlDom {
        doctype: Some("html".into()),
        leading_comments: vec![],
        root: html_el,
        // 内存构造的导出树,来源是导入期已限深的文档 —— 不存在超限输入
        depth_exceeded: false,
    };
    dom.serialize()
}

/// 确定节点导出的 class 列表(已由 [`resolve_final_classes`] 定稿)。
/// PERF-03:定稿改走侧表,不再就地改树。
fn export_classes(resolved: &FinalClasses, doc: &Document, id: NodeId) -> Vec<String> {
    classes_of(resolved, doc, id)
        .map(<[String]>::to_vec)
        .unwrap_or_default()
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

fn render_node(doc: &Document, resolved: &FinalClasses, id: NodeId, node: &Node) -> Vec<HtmlNode> {
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
            let mut classes = export_classes(resolved, doc, id);
            // 容器标记类(导入器据此识别;ADR-0014)
            let marker = match node.kind {
                NodeKind::Artboard => Some("vb-artboard"),
                NodeKind::Layer => Some("vb-layer"),
                NodeKind::Group => Some("vb-group"),
                // 05-8:主件定义容器(defs_root 直接子节点)补写标记类 ——
                // 导入侧按 class 识别定义区,标记类不进节点 classes
                // (不参与类定稿与 CSS 级联),导出时在此统一回写。
                _ if node.parent == Some(doc.defs_root()) => Some(crate::symbol::SYMBOL_DEF_CLASS),
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
            for &c in &node.children {
                if let Some(cn) = doc.node(c) {
                    for cm in &cn.comment_before {
                        el.children.push(HtmlNode {
                            data: NodeData::Comment(cm.clone()),
                            children: vec![],
                        });
                    }
                    el.children.extend(render_node(doc, resolved, c, cn));
                }
            }
            vec![el]
        }
    }
}

/// 富文本 → HTML 片段:无样式区段 → 文本;样式区段 → `<span style>`;`\n` → `<br>`。
///
/// DOC-03:段区间来自命令路径校验(`commands::validate_segs`),但导出侧
/// **不复信** —— 区间按字节切片,非字符边界/越界即 panic。这里先做
/// char-boundary 复验,任一区间非法 → 显式告警 + 整段降级为纯文本
/// (样式丢弃可恢复,panic 丢整个导出不可接受)。
fn text_fragment(text: &str, segments: &[TextSeg]) -> Vec<HtmlNode> {
    let mut out: Vec<HtmlNode> = Vec::new();
    let segs_ok = segments.iter().all(|seg| {
        seg.start <= seg.end
            && seg.end <= text.len()
            && text.is_char_boundary(seg.start)
            && text.is_char_boundary(seg.end)
    });
    if !segs_ok {
        log::warn!(
            "富文本段区间越界/未对齐字符边界(段数 {}),整段降级为纯文本导出",
            segments.len()
        );
        push_text_with_breaks(text, &mut out);
        if out.is_empty() {
            out.push(HtmlNode::text(String::new()));
        }
        return out;
    }
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

fn render_css(doc: &Document, resolved: &FinalClasses) -> String {
    let mut out = String::new();

    // :root 令牌
    if !doc.tokens.is_empty() {
        out.push_str(":root {\n");
        for (k, v) in &doc.tokens {
            let _ = writeln!(out, "  --{k}: {v};");
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
        if let Some(n) = doc.node(id) {
            ordered.push(n);
        }
    }

    for (idx, node) in ordered.iter().enumerate() {
        let node_id = ids[idx];
        // `#text` 没有 class 属性(见 resolve_final_classes),自然也不该有 CSS 规则。
        if node.tag == "#text" {
            continue;
        }
        let classes = classes_of(resolved, doc, node_id).unwrap_or_default();
        if classes.is_empty() {
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
        // 选择器 = 首类(resolve_final_classes 保证唯一)
        let selector = format!(".{}", classes[0]);
        let _ = writeln!(out, "{selector} {{");
        for d in &decls {
            // PERF-08:声明直写缓冲,免逐条中间 String
            out.push_str("  ");
            d.push_css(&mut out);
            out.push_str(";\n");
        }
        out.push_str("}\n\n");
    }

    // ── 05-5:断点覆盖块(@media)──
    // 位置在节点规则**之后**:同特异性下后出者级联胜出 —— 覆盖语义成立
    // (raw 冻结块仍在节点规则之前,原保真纪律不变)。宽度降序排列:窄屏
    // 查询后出,多断点同时命中时窄屏覆盖宽屏(移动优先的级联方向)。
    {
        let mut widths: Vec<u32> = doc.media_rules.iter().map(|r| r.max_width).collect();
        widths.sort_unstable();
        widths.dedup();
        for w in widths.into_iter().rev() {
            let _ = writeln!(out, "@media (max-width: {w}px) {{");
            for r in doc.media_rules.iter().filter(|r| r.max_width == w) {
                let Some((selector, decls)) = media_rule_target(doc, resolved, r) else {
                    continue;
                };
                let _ = writeln!(out, "  {selector} {{");
                for d in &decls {
                    out.push_str("    ");
                    d.push_css(&mut out);
                    out.push_str(";\n");
                }
                out.push_str("  }\n");
            }
            out.push_str("}\n\n");
        }
    }
    // ── 05-5:伪类规则(最小闭环 :hover)──
    // 伪类特异性高于基规则,与位置无关;同样放在节点规则之后,canonical 一致。
    for pr in &doc.pseudo_rules {
        let Some((selector, decls)) = pseudo_rule_target(doc, resolved, pr) else {
            continue;
        };
        let _ = writeln!(out, "{selector} {{");
        for d in &decls {
            out.push_str("  ");
            d.push_css(&mut out);
            out.push_str(";\n");
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

/// 断点规则 → (选择器, 排序后的声明)。目标节点必须仍存在、有首类
/// (resolve_final_classes 之后必有),且不是 `#text` / 冻结块(无 class 载体)。
fn media_rule_target(
    doc: &Document,
    resolved: &FinalClasses,
    r: &crate::model::MediaRule,
) -> Option<(String, Vec<vb_css::Decl>)> {
    let id = doc.find_by_sid(&r.sid)?;
    let classes = classes_of(resolved, doc, id)?;
    if classes.is_empty() {
        return None;
    }
    let n = doc.node(id)?;
    if n.tag == "#text" || matches!(n.kind, NodeKind::Frozen { .. }) {
        return None;
    }
    let mut decls = r.decls.clone();
    sort_decls(&mut decls);
    Some((format!(".{}", classes[0]), decls))
}

/// 伪类规则 → (`.cls:hover` 选择器, 排序后的声明)。约束同上。
fn pseudo_rule_target(
    doc: &Document,
    resolved: &FinalClasses,
    pr: &crate::model::PseudoRule,
) -> Option<(String, Vec<vb_css::Decl>)> {
    let id = doc.find_by_sid(&pr.sid)?;
    let classes = classes_of(resolved, doc, id)?;
    if classes.is_empty() {
        return None;
    }
    let n = doc.node(id)?;
    if n.tag == "#text" || matches!(n.kind, NodeKind::Frozen { .. }) {
        return None;
    }
    let mut decls = pr.decls.clone();
    sort_decls(&mut decls);
    Some((format!(".{}:{}", classes[0], pr.pseudo), decls))
}

fn ordered_source(doc: &Document) -> Vec<NodeId> {
    let mut v = Vec::new();
    for &ab in doc.artboards() {
        doc.subtree(ab, &mut v);
    }
    // 05-8:主件定义区排在画板之后(CSS 规则序 = 页面内容优先;类定稿
    // 与 CSS 输出共用本序,保证定义区原型节点的类与规则一并定稿/输出)
    if let Some(r) = doc.node(doc.defs_root()) {
        let kids = r.children.clone();
        for c in kids {
            doc.subtree(c, &mut v);
        }
    }
    v
}

// ─────────────────────── 单测(断电安全原子写) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// PERF-03 对照基准:旧实现(`finalize_classes`)的就地类名定稿,
    /// 作用于克隆文档。新纯函数求解必须与其逐节点一致。
    fn legacy_finalize_classes_inplace(doc: &mut Document) {
        use std::collections::HashSet;
        let mut used: HashSet<String> = HashSet::new();
        let ids = ordered_source(doc);
        for &id in &ids {
            if let Some(n) = doc.nodes.get(id) {
                if matches!(n.kind, NodeKind::Frozen { .. }) {
                    for c in &n.classes {
                        used.insert(c.clone());
                    }
                }
            }
        }
        for id in ids {
            let node = match doc.nodes.get_mut(id) {
                Some(n) => n,
                None => continue,
            };
            let is_def_container = node.parent == Some(doc.defs_root);
            if node.tag == "#text"
                || is_def_container
                || matches!(node.kind, NodeKind::Frozen { .. })
            {
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
                let mut gen = format!("vb-el-{}", node.sid.as_str());
                let mut seq = 2usize;
                while node.classes.iter().any(|c| c == &gen) || used.contains(&gen) {
                    gen = format!("vb-el-{}-{seq}", node.sid.as_str());
                    seq += 1;
                }
                node.classes.insert(0, gen.clone());
                used.insert(gen);
                node.classes.retain(|c| c != &primary);
            } else {
                used.insert(primary);
            }
        }
    }

    /// 构造一个会触发全部定稿分支的文档:无名节点(造类)、同名双节点
    /// (冲突改名)、冻结块(占名不改名)、`#text`、主件定义区原型。
    fn attach(doc: &mut Document, parent: NodeId, n: Node) -> NodeId {
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        id
    }

    fn box_node(doc: &mut Document, name: &str, tag: &str) -> Node {
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, name, sid);
        n.tag = tag.to_string();
        n
    }

    fn doc_all_finalize_branches() -> Document {
        let mut doc = Document::new("定稿分支", "zh-CN");
        let ab = doc.artboards[0];
        // 同名两节点 → 第二个触发冲突改名
        for name in ["hero", "hero"] {
            let n = box_node(&mut doc, name, "div");
            attach(&mut doc, ab, n);
        }
        // 无名节点 → 造类
        let n = box_node(&mut doc, "", "p");
        attach(&mut doc, ab, n);
        // 冻结块:带与首节点相同的类(只占名)
        let sid = doc.alloc_sid();
        let mut frozen = Node::new(
            NodeKind::Frozen {
                html: "<hr>".into(),
            },
            "冻结",
            sid,
        );
        frozen.classes.push("hero".into());
        attach(&mut doc, ab, frozen);
        // #text 子片段(跳过定稿)
        let sid = doc.alloc_sid();
        let mut text = Node::new(
            NodeKind::Text {
                text: "文本".into(),
                mode: crate::model::TextMode::Point,
                segments: Vec::new(),
            },
            "文本",
            sid,
        );
        text.tag = "#text".into();
        attach(&mut doc, ab, text);
        // 主件定义区:容器 + 原型子节点
        let sid = doc.alloc_sid();
        let def_container = Node::new(NodeKind::Group, "主件A", sid);
        let dc = doc.nodes.insert(def_container);
        doc.nodes.get_mut(dc).unwrap().parent = Some(doc.defs_root);
        doc.nodes.get_mut(doc.defs_root).unwrap().children.push(dc);
        let proto = box_node(&mut doc, "", "div");
        attach(&mut doc, dc, proto);
        doc
    }

    /// PERF-03(等价钉住):纯函数侧表定稿 ≡ 旧就地定稿(逐节点类表一致),
    /// 且调用方文档零改动(不可变语义)。
    #[test]
    fn resolved_classes_match_legacy_inplace_and_doc_stays_immutable() {
        let doc = doc_all_finalize_branches();
        let before = doc.clone();

        // 旧语义:在克隆上就地定稿
        let mut legacy = doc.clone();
        legacy_finalize_classes_inplace(&mut legacy);

        // 新实现:侧表求解(渲染路径)
        let resolved = resolve_final_classes(&doc);
        for (id, legacy_node) in legacy.nodes.iter() {
            let want = &legacy_node.classes;
            let got = resolved
                .get(&id)
                .map(|v| v.as_slice())
                .unwrap_or(doc.node(id).map(|n| n.classes.as_slice()).unwrap_or(&[]));
            assert_eq!(got, want, "节点 {id:?} 定稿类表必须与旧就地版一致");
        }
        // 非平凡性:与冻结块占名撞类的 hero 节点们全部走冲突改名(vb-el-*);
        // 冻结块自身不在侧表内(原样导出,P0-2 占名不改名)。
        assert!(
            resolved
                .values()
                .any(|c| c.first().is_some_and(|s| s.starts_with("vb-el-"))),
            "冲突改名分支必须被触发:{resolved:?}"
        );
        let frozen_kept = doc
            .nodes
            .iter()
            .any(|(_, n)| matches!(n.kind, NodeKind::Frozen { .. }) && n.classes == ["hero"]);
        assert!(frozen_kept, "冻结块类名不得被定稿改写");

        // 渲染全程借用:调用方文档在导出前后必须逐字节相同
        let _ = render_project(&doc);
        let snapshot = |d: &Document| -> String {
            let mut parts: Vec<String> = d
                .nodes
                .iter()
                .map(|(id, n)| format!("{id:?}={:?}|{:?}", n.classes, n.name))
                .collect();
            parts.sort();
            format!("{:?}|{}", d.artboards, parts.join("|"))
        };
        assert_eq!(snapshot(&doc), snapshot(&before), "导出不得改动文档");
    }

    /// 独立临时项目目录(测试间互不串扰;与 `vb_app::autosave` 同款)。
    fn tmp_project(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("vb-export-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// ExternalCss 模式(双文件:index.html + styles/main.css)的最小文档。
    fn doc_with_title(title: &str) -> Document {
        let mut doc = Document::new_default();
        doc.meta.title = title.to_string();
        doc.meta.output = OutputMode::ExternalCss;
        doc
    }

    /// 目录树里全部 `.tmp-` 开头的**文件**(垃圾/残留检测;目录不算 ——
    /// 注入型故障的障碍物本身可能是目录)。
    fn tmp_files(dir: &Path) -> Vec<std::path::PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = entry.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p
                    .file_name()
                    .map(|n| n.to_string_lossy().starts_with(".tmp-"))
                    .unwrap_or(false)
                {
                    out.push(p);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, &mut out);
        out
    }

    /// 成功路径:双文件逐字节完整、index.html 最后落盘、零 `.tmp` 残留。
    #[test]
    fn write_project_is_complete_and_leaves_no_tmp_residue() {
        let dir = tmp_project("happy");
        let doc = doc_with_title("完整落盘");
        let expect = render_project(&doc);
        let written = write_project(&doc, &dir).unwrap();

        assert_eq!(written.len(), 2);
        // 落盘序契约:css 在前,index.html 恒为最后一个落盘的文件
        assert!(written.last().unwrap().ends_with("index.html"));
        for (rel, content) in &expect.files {
            let on_disk = std::fs::read_to_string(dir.join(rel)).unwrap();
            assert_eq!(on_disk, *content, "{rel} 必须与 canonical 序列化逐字节一致");
        }
        assert!(tmp_files(&dir).is_empty(), "成功写盘不得留 .tmp 残留");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 故障注入:css 的临时文件路径被预占为目录(`fs::write` 必败,
    /// Windows/Unix 皆然)—— 整个保存必须失败,但磁盘上的 index.html
    /// 保持**完整旧内容**(index.html 压轴纪律),且不留 `.tmp` 文件残留。
    #[test]
    fn css_tmp_failure_keeps_old_index_intact_and_clean() {
        let dir = tmp_project("fail-css");
        let old = doc_with_title("旧版完整文档");
        write_project(&old, &dir).unwrap();
        let old_index = std::fs::read_to_string(dir.join("index.html")).unwrap();

        // 临时名形如 .tmp-<原名>-<pid>-<线程id>,含本测试进程 pid 与当前
        // 线程 id(write_project 在本线程执行),可确定性预言;
        // 把 styles/main.css 的临时路径预占成目录 → 写临时文件必败。
        let obstacle = dir.join("styles").join(format!(
            ".tmp-main.css-{}-{:x}",
            std::process::id(),
            current_thread_id_u64()
        ));
        std::fs::create_dir_all(&obstacle).unwrap();

        let new = doc_with_title("新版未落盘文档");
        let err = write_project(&new, &dir).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("main.css"), "错误信息须带目标路径:{msg}");

        let on_disk = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert_eq!(on_disk, old_index, "css 写败后 index.html 必须原封不动");
        assert!(tmp_files(&dir).is_empty(), "失败不得留 .tmp 文件残留");
        assert!(obstacle.is_dir(), "残存的是测试预置的障碍目录,而非写盘垃圾");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// rename 目标被占用的近似测试(仅 Windows):目标 main.css 以无
    /// FILE_SHARE_DELETE 的共享模式打开时,覆盖 rename 必败 —— 错误信息带
    /// 目标路径、临时文件被清理、旧文件原封未动。Unix 上 rename 对被打开
    /// 文件恒成功,该场景不存在,测试不编译。
    #[cfg(windows)]
    #[test]
    fn rename_locked_target_reports_path_and_cleans_tmp() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tmp_project("locked");
        let old = doc_with_title("被占用前的旧版");
        write_project(&old, &dir).unwrap();
        let old_index = std::fs::read_to_string(dir.join("index.html")).unwrap();

        // 独占目标 main.css(仅共享读,无 WRITE/DELETE 共享)→ 覆盖 rename 必败
        let lock = std::fs::OpenOptions::new()
            .write(true)
            .share_mode(1) // FILE_SHARE_READ
            .open(dir.join("styles/main.css"))
            .unwrap();

        let new = doc_with_title("占用期间的新版");
        let err = write_project(&new, &dir).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("main.css"), "错误信息须带目标路径:{msg}");

        // 覆盖失败 = 目标保持旧内容;index.html 压轴,连尝试都不会发生
        let on_disk = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert_eq!(on_disk, old_index, "rename 失败后 index.html 必须原封不动");
        assert!(tmp_files(&dir).is_empty(), "rename 失败必须清理 .tmp");
        drop(lock);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// DOC-03(验收):段区间不在字符边界(中文字符的中间)时,导出
    /// 必须**不 panic** 且整段降级为纯文本(全文保留,无样式 span)。
    #[test]
    fn seg_off_char_boundary_degrades_to_plain_text() {
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let sid = doc.alloc_sid();
        let mut n = Node::new(
            NodeKind::Text {
                // "中文ab":每个汉字 3 字节;seg.start=1 落在「中」的中间
                text: "中文ab".to_string(),
                mode: crate::model::TextMode::Point,
                segments: vec![TextSeg {
                    start: 1,
                    end: 4,
                    style: SegStyle {
                        bold: Some(true),
                        ..Default::default()
                    },
                }],
            },
            "越界段",
            sid,
        );
        n.geom = crate::model::Geom {
            x: 10.0,
            y: 10.0,
            w: 200.0,
            h: 40.0,
        };
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(ab).unwrap().children.push(id);
        doc.nodes.get_mut(id).unwrap().parent = Some(ab);

        let res = render_project(&doc);
        let html = &res.files[0].1;
        // 全文保留(降级为整段纯文本,不因切片 panic / 不丢字符)
        assert!(html.contains("中文ab"), "降级导出必须保留全文:{html}");
        // 降级 = 无样式 span(区间被整段放弃)
        assert!(!html.contains("<span"), "降级后不得残留样式 span:{html}");
    }

    /// DOC-08(验收):并发线程各自 write_project 同一目录,临时文件名
    /// 必须互不相同(pid 相同 → 线程 id 必须区分),产物完整。
    #[test]
    fn concurrent_threads_use_distinct_tmp_names() {
        let dir = tmp_project("threads");
        let doc = doc_with_title("并发导出");
        let mut names = std::collections::HashSet::new();
        let mut handles = Vec::new();
        for _ in 0..4 {
            let dir = dir.clone();
            let doc = doc.clone();
            handles.push(std::thread::spawn(move || {
                let res = write_project(&doc, &dir).unwrap();
                res.last().unwrap().clone()
            }));
        }
        for h in handles {
            let _ = h.join().unwrap();
        }
        // 每个线程的临时名(pid 相同、线程 id 不同)必然互不相同:
        // 直接构造四个线程的临时名抽样验证
        for _ in 0..4 {
            let t = std::thread::spawn(current_thread_id_u64).join().unwrap();
            names.insert(format!(".tmp-index.html-{}-{t:x}", std::process::id()));
        }
        assert_eq!(names.len(), 4, "线程 id 必须区分临时名:{names:?}");
        let on_disk = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(on_disk.contains("并发导出"));
        assert!(tmp_files(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
