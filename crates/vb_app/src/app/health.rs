//! 项目健康检查(阶段 7 / 07-E;副文档 07 §2.1)。
//!
//! 一键体检六类问题(纯函数 [`check`] 可门禁直测):
//! ① 缺失资源 —— `img src` 指向的文件在项目里不存在;
//! ② 失效链接 —— `href` 指向的本地路径不存在;
//! ③ 冻结块 —— `vb-freeze` 语义的 [`NodeKind::Frozen`] 数量与位置;
//! ④ 未使用资产 —— `assets/` 里没有被任何 src/href/CSS `url()` 引用的文件;
//! ⑤ 超长文件 —— 单文件超 [`MAX_FILE_BYTES`] 或单行超 [`MAX_LINE_CHARS`];
//! ⑥ 无障碍(07-N,只提示不强制)—— 缺 `alt` 的图片、无可访问名称的
//!    交互元素(button/link 角色)、文本对比度低于 WCAG AA(粗判)。
//!
//! **与既有告警的关系**(不写两套):导入期的「样式表缺失」告警
//! (`vb_doc::import`)与布局期图像探测(`vb_layout::probe_image` 回退)
//! 仍是打开时的即时通道;本模块是**聚合盘点**,同一套"项目相对路径解析"
//! 约定(相对项目根、跳过远程/锚点),结论互补不重复。
//!
//! 报告窗口在文件尾部 `impl VellumApp` 块;每条问题可点击定位
//! (节点 → 选中图层;文件 → 状态栏展示路径)。

use std::path::{Path, PathBuf};

use vb_common::color::parse_color;
use vb_doc::model::{Document, NodeKind};

use super::VellumApp;

/// 超长文件阈值:单文件字节数(2MB)。
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// 超长文件阈值:单行字符数(手写 HTML/CSS 出现 8k+ 单行基本是事故)。
pub const MAX_LINE_CHARS: usize = 8192;
/// 参与行长检查的文本扩展名(图像等二进制不做逐行扫描)。
const TEXT_EXTS: &[&str] = &["html", "htm", "css", "js", "json", "svg", "txt", "md"];

/// 问题类别(报告分组着色用)。
///
/// 07-N 起新增三类**无障碍检查项**(扩展 07-E 体检;只提示不强制):
/// 缺 `alt` 的图片、无可访问名称的交互元素、文本对比度不足(WCAG AA 粗判)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HealthKind {
    MissingAsset,
    BrokenLink,
    Frozen,
    UnusedAsset,
    LargeFile,
    /// 无障碍:`<img>` 缺 `alt` 属性(空 `alt` = 装饰图,合规,不报)。
    A11yAlt,
    /// 无障碍:交互元素(button / link 角色)无可访问名称。
    A11yName,
    /// 无障碍:文本与背景对比度低于 WCAG AA 阈值(4.5:1;大字 3:1)。
    A11yContrast,
}

impl HealthKind {
    pub fn label(self) -> &'static str {
        match self {
            HealthKind::MissingAsset => "缺失资源",
            HealthKind::BrokenLink => "失效链接",
            HealthKind::Frozen => "冻结块",
            HealthKind::UnusedAsset => "未使用资产",
            HealthKind::LargeFile => "超长文件",
            HealthKind::A11yAlt => "无障碍·缺 alt",
            HealthKind::A11yName => "无障碍·可访问名",
            HealthKind::A11yContrast => "无障碍·对比度",
        }
    }

    /// 报告行的着色(问题红 / 提示橙;冻结块与无障碍项是提示不是错)。
    ///
    /// 07-I 双主题核对修正:此前硬编码浅橙在浅色主题下不可读 —— 提示级
    /// 改走主题 `warn` 令牌(浅色自动加深),问题级统一走 `danger` 令牌
    /// (门禁 8:深色下不再另保亮一份字面量红,与状态栏「外部已改动」
    /// 的问题级着色同一口径)。
    pub fn color(self, dark: bool) -> egui::Color32 {
        let t = vb_ui::theme::Tokens::get(dark);
        match self {
            HealthKind::Frozen
            | HealthKind::A11yAlt
            | HealthKind::A11yName
            | HealthKind::A11yContrast => t.warn,
            _ => t.danger,
        }
    }
}

/// 定位信息(点击报告行 → 跳转目标)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locate {
    /// 选中该图层(sid)。
    Node(String),
    /// 展示文件路径。
    File(String),
}

/// 一条健康问题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthIssue {
    pub kind: HealthKind,
    pub message: String,
    pub locate: Locate,
}

/// 远程 / 锚点 / 空引用不参与存在性检查。
pub fn is_external(ref_url: &str) -> bool {
    let u = ref_url.trim();
    u.is_empty()
        || u.starts_with('#')
        || u.starts_with("//")
        || ["http://", "https://", "data:", "mailto:", "tel:"]
            .iter()
            .any(|p| u.starts_with(p))
}

/// 引用规范化:统一 `/` 分隔符、去 `./` 前缀(与写入侧的相对路径约定一致)。
pub fn normalize_ref(r: &str) -> String {
    let mut s = r.trim().replace('\\', "/");
    while let Some(stripped) = s.strip_prefix("./") {
        s = stripped.to_string();
    }
    s
}

/// CSS 文本里的 `url(...)` 引用(去引号;只收项目相对路径)。
pub(crate) fn css_urls(css: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = css;
    while let Some(i) = rest.find("url(") {
        let after = &rest[i + 4..];
        let Some(end) = after.find(')') else { break };
        let raw = after[..end].trim().trim_matches(|c| c == '"' || c == '\'');
        out.push(raw.to_string());
        rest = &after[end..];
    }
    out
}

/// 项目相对键(比较用:小写 + `/` 分隔;Windows 文件系统大小写不敏感)。
pub(crate) fn rel_key(p: &str) -> String {
    normalize_ref(p).to_lowercase()
}

/// 递归列目录下全部文件(相对 `base` 的路径;目录不可读按空处理)。
pub(crate) fn list_files(base: &Path, rel: &Path, out: &mut Vec<PathBuf>) {
    let cur = base.join(rel);
    let Ok(rd) = std::fs::read_dir(&cur) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let child = rel.join(&name);
        if entry.path().is_dir() {
            list_files(base, &child, out);
        } else {
            out.push(child);
        }
    }
}

/// 收集文档里的本地引用:`(sid, 属性名, 相对路径)`。
/// `src`(含 Image kind)→ 资源;`href` → 链接。
pub(crate) fn collect_refs(doc: &Document) -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    let mut ids = Vec::new();
    for &ab in &doc.artboards {
        doc.subtree(ab, &mut ids);
    }
    for id in ids {
        let Some(n) = doc.nodes.get(id) else { continue };
        let sid = n.sid.as_str().to_string();
        if let NodeKind::Image { src } = &n.kind {
            out.push((sid.clone(), "src", src.clone()));
        }
        for (k, v) in &n.attrs {
            if k == "src" || k == "href" {
                out.push((
                    sid.clone(),
                    if k == "src" { "src" } else { "href" },
                    v.clone(),
                ));
            }
        }
    }
    out
}

/// 一条引用命中(07-K 资产面板与 07-E 体检共用):哪个节点的哪个属性
/// 指向了资产。`via` = "src" / "href" / "css url()"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefHit {
    pub sid: String,
    pub via: &'static str,
}

/// 收集文档的「资产 → 引用它的节点」映射(07-K/07-E 单一来源)。
///
/// 键 = `rel_key`(规范化小写相对路径);值 = 引用列表(src/href 属性与
/// CSS `url()`)。此前健康检查内联收集,资产面板要展示「被引用关系」,
/// 收敛到这里,**不写两套**(引用约定漂移会同时破坏体检与面板)。
pub fn asset_reference_map(doc: &Document) -> std::collections::BTreeMap<String, Vec<RefHit>> {
    let mut out: std::collections::BTreeMap<String, Vec<RefHit>> = Default::default();
    // 导入的 <img> 在 kind 与 attrs 各存一份同一引用(体检侧靠报告去重);
    // 面板展示同键下按 (sid, via) 去重,避免同一引用列出两行。
    let mut push = |key: String, hit: RefHit| {
        let list = out.entry(key).or_default();
        if !list.iter().any(|h| h.sid == hit.sid && h.via == hit.via) {
            list.push(hit);
        }
    };
    for (sid, attr, raw) in collect_refs(doc) {
        if is_external(&raw) {
            continue;
        }
        push(rel_key(&raw), RefHit { sid, via: attr });
    }
    for block in &doc.raw_css {
        for u in css_urls(block) {
            if !is_external(&u) {
                push(
                    rel_key(&u),
                    RefHit {
                        sid: String::new(),
                        via: "css url()",
                    },
                );
            }
        }
    }
    out
}

// ─────────────────────── 07-N 无障碍检查(纯函数) ───────────────────────

/// WCAG AA 正文阈值(普通文本 ≥ 4.5:1;大字 ≥ 3:1)。
pub const CONTRAST_NORMAL: f64 = 4.5;
/// WCAG AA 大字阈值(≥24px,或 ≥18.66px 且粗体)。
pub const CONTRAST_LARGE: f64 = 3.0;

/// 交互元素的判定(design/06 口径:button / link 两类角色):
/// `tag` 为 `button`/`a`,或显式 `role` 声明为 `button`/`link`。
pub(crate) fn is_interactive(
    tag: &str,
    attrs: &std::collections::BTreeMap<String, String>,
) -> bool {
    matches!(tag, "button" | "a")
        || attrs
            .get("role")
            .is_some_and(|r| matches!(r.as_str(), "button" | "link"))
}

/// 节点子树是否有「可辨识文本」:文本节点内容,或后代 `<img>` 的 `alt`
/// (图标按钮内嵌图片的可访问名来源)。
pub(crate) fn subtree_has_text(doc: &Document, id: vb_doc::model::NodeId) -> bool {
    let Some(n) = doc.nodes.get(id) else {
        return false;
    };
    if n.text().is_some_and(|t| !t.trim().is_empty()) {
        return true;
    }
    if matches!(n.kind, NodeKind::Image { .. })
        && n.attrs.get("alt").is_some_and(|a| !a.trim().is_empty())
    {
        return true;
    }
    n.children.iter().any(|&c| subtree_has_text(doc, c))
}

/// 交互元素是否已有可访问名称:`aria-label` / `aria-labelledby` / `title`
/// 任一存在,或子树自带可辨识文本( WCAG「name from content」)。
pub(crate) fn has_accessible_name(doc: &Document, id: vb_doc::model::NodeId) -> bool {
    let Some(n) = doc.nodes.get(id) else {
        return false;
    };
    ["aria-label", "aria-labelledby", "title"]
        .iter()
        .any(|k| n.attrs.contains_key(*k))
        || subtree_has_text(doc, id)
}

/// WCAG 2.x 相对亮度(sRGB;阈值 0.03928 与 12.92)。
fn rel_luminance(c: vb_common::Rgba) -> f64 {
    let ch = |v: u8| {
        let v = v as f64 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

/// 对比度(WCAG 公式):(亮 + 0.05) / (暗 + 0.05)。
pub fn contrast_ratio(a: vb_common::Rgba, b: vb_common::Rgba) -> f64 {
    let (la, lb) = (rel_luminance(a), rel_luminance(b));
    let (hi, lo) = (la.max(lb), la.min(lb));
    (hi + 0.05) / (lo + 0.05)
}

/// 解析 `var(--x)`(查 `doc.tokens`;一层链式 var 也兜住,深度封顶 5)。
fn resolve_var(doc: &Document, value: &str) -> String {
    let mut cur = value.trim().to_string();
    for _ in 0..5 {
        let Some(inner) = cur.strip_prefix("var(").and_then(|s| s.strip_suffix(')')) else {
            break;
        };
        let name = inner.split(',').next().unwrap_or("").trim();
        let name = name.strip_prefix("--").unwrap_or(name);
        let Some(v) = doc
            .tokens
            .iter()
            .find(|(k, _)| k.as_str() == name)
            .map(|(_, v)| v.clone())
        else {
            break; // 变量未定义:原样返回,交给上层解析失败处理
        };
        cur = v;
    }
    cur
}

/// 从声明表取一条可解析为颜色的属性值(先整值解析,失败再取首个空格段,
/// 兜住 `background: #fff no-repeat` 这类复合值;渐变/url 解析失败自然跳过)。
fn color_of_decl(doc: &Document, decls: &[vb_css::Decl], prop: &str) -> Option<vb_common::Rgba> {
    decls
        .iter()
        .find(|d| d.prop == prop)
        .map(|d| resolve_var(doc, &d.value))
        .and_then(|v| {
            let v = v.trim().to_string();
            parse_color(&v).or_else(|| v.split_whitespace().next().and_then(parse_color))
        })
}

/// 文本节点的生效样式粗判(继承语义:沿祖先链取**最近**声明):
/// (前景色, 背景色, 字号 px, 是否粗体)。背景色不继承,但取最近祖先
/// 的实底作对比对象;全部缺省 = 浏览器默认(黑字 / 白底 / 16px)。
pub(crate) fn resolve_text_style(
    doc: &Document,
    id: vb_doc::model::NodeId,
) -> (vb_common::Rgba, vb_common::Rgba, f64, bool) {
    let mut fg = None;
    let mut bg = None;
    let mut size = None;
    let mut bold = false;
    let mut cur = Some(id);
    while let Some(cid) = cur {
        let Some(n) = doc.nodes.get(cid) else {
            break;
        };
        if fg.is_none() {
            fg = color_of_decl(doc, &n.style, "color");
        }
        if bg.is_none() {
            bg = color_of_decl(doc, &n.style, "background-color")
                .or_else(|| color_of_decl(doc, &n.style, "background"));
        }
        if size.is_none() {
            size = n
                .style
                .iter()
                .find(|d| d.prop == "font-size")
                .map(|d| resolve_var(doc, &d.value))
                .and_then(|v| {
                    let v = v.trim();
                    let num = v.strip_suffix("px").unwrap_or(v).trim();
                    num.parse::<f64>().ok()
                });
        }
        if !bold {
            bold = n
                .style
                .iter()
                .find(|d| d.prop == "font-weight")
                .map(|d| resolve_var(doc, &d.value))
                .and_then(|v| v.trim().parse::<f64>().ok())
                .map(|w| w >= 600.0)
                .unwrap_or(false);
        }
        if fg.is_some() && bg.is_some() && size.is_some() {
            break; // 四项齐了不必再向上
        }
        cur = n.parent;
    }
    (
        fg.unwrap_or(vb_common::Rgba::BLACK),
        bg.unwrap_or(vb_common::Rgba::WHITE),
        size.unwrap_or(16.0),
        bold,
    )
}

/// 无障碍三查(07-N;追加进体检报告):
/// ① `<img>` 缺 `alt`(空 alt = 装饰图,合规);
/// ② button / link 角色无可访问名称;
/// ③ 文本对比度低于 WCAG AA(节点级样式粗判,只提示不强制)。
pub(crate) fn a11y_issues(doc: &Document) -> Vec<HealthIssue> {
    let mut out = Vec::new();
    let mut ids = Vec::new();
    for &ab in &doc.artboards {
        doc.subtree(ab, &mut ids);
    }
    for id in ids {
        let Some(n) = doc.nodes.get(id) else { continue };
        let sid = n.sid.as_str().to_string();
        // ① 缺 alt 的图片(Image kind 或 tag=img)
        let is_img = matches!(n.kind, NodeKind::Image { .. }) || n.tag.eq_ignore_ascii_case("img");
        if is_img && !n.attrs.contains_key("alt") {
            out.push(HealthIssue {
                kind: HealthKind::A11yAlt,
                message: format!(
                    "图片「{}」({})缺 alt 属性 —— 加 alt 说明内容;纯装饰图给空 alt=\"\"",
                    n.name, sid
                ),
                locate: Locate::Node(sid.clone()),
            });
        }
        // ② 交互元素无可访问名称
        if is_interactive(&n.tag, &n.attrs) && !has_accessible_name(doc, id) {
            out.push(HealthIssue {
                kind: HealthKind::A11yName,
                message: format!(
                    "交互元素「{}」({})无可访问名称 —— 加 aria-label 或可见文本",
                    n.name, sid
                ),
                locate: Locate::Node(sid.clone()),
            });
        }
        // ③ 对比度(只对非空文本节点判;大字阈值放宽)
        if let Some(text) = n.text().filter(|t| !t.trim().is_empty()) {
            let (fg, bg, size, bold) = resolve_text_style(doc, id);
            let large = size >= 24.0 || (bold && size >= 18.66);
            let threshold = if large {
                CONTRAST_LARGE
            } else {
                CONTRAST_NORMAL
            };
            let ratio = contrast_ratio(fg, bg);
            if ratio < threshold {
                out.push(HealthIssue {
                    kind: HealthKind::A11yContrast,
                    message: format!(
                        "文本「{}」({})对比度 {:.1}:1,低于 WCAG AA 建议 ≥{threshold}:1(按节点级样式粗判,仅提示)",
                        truncate_text(text),
                        sid,
                        ratio
                    ),
                    locate: Locate::Node(sid.clone()),
                });
            }
        }
    }
    out
}

/// 报告文案里的文本截断(长段只留前 12 字)。
fn truncate_text(t: &str) -> String {
    let t = t.trim();
    if t.chars().count() <= 12 {
        t.to_string()
    } else {
        let mut s: String = t.chars().take(12).collect();
        s.push('…');
        s
    }
}

/// 一键体检(纯函数;测试构造临时项目夹具直接打这里)。
pub fn check(doc: &Document, project: &Path) -> Vec<HealthIssue> {
    let mut out = Vec::new();
    let refs = collect_refs(doc);

    // ① ② 缺失资源 / 失效链接
    for (sid, attr, raw) in &refs {
        if is_external(raw) {
            continue;
        }
        let rel = normalize_ref(raw);
        if rel.starts_with('/') {
            continue; // 站点根绝对路径:无项目根语义,不误报
        }
        if !project.join(&rel).exists() {
            let kind = if *attr == "src" {
                HealthKind::MissingAsset
            } else {
                HealthKind::BrokenLink
            };
            out.push(HealthIssue {
                kind,
                message: format!("{attr} 指向的「{rel}」不存在(节点 {sid})"),
                locate: Locate::Node(sid.clone()),
            });
        }
    }

    // ③ 冻结块统计(数量 + 位置)
    let mut ids = Vec::new();
    for &ab in &doc.artboards {
        doc.subtree(ab, &mut ids);
    }
    for id in &ids {
        if let Some(n) = doc.nodes.get(*id) {
            if matches!(n.kind, NodeKind::Frozen { .. }) {
                out.push(HealthIssue {
                    kind: HealthKind::Frozen,
                    message: format!(
                        "冻结块「{}」({})—— 内部不可编辑,样式由原样 HTML 承载",
                        n.name,
                        n.sid.as_str()
                    ),
                    locate: Locate::Node(n.sid.as_str().to_string()),
                });
            }
        }
    }

    // ④ 未使用资产:assets/ 下未被 src/href/CSS url() 引用的文件
    // (07-K 起引用收集收敛到 `asset_reference_map`,体检与资产面板同一套)
    let referenced: std::collections::BTreeSet<String> =
        asset_reference_map(doc).keys().cloned().collect();
    let mut files = Vec::new();
    list_files(project, Path::new("assets"), &mut files);
    for f in &files {
        let key = rel_key(&f.to_string_lossy());
        if !referenced.contains(&key) {
            out.push(HealthIssue {
                kind: HealthKind::UnusedAsset,
                message: format!(
                    "assets/ 里的「{}」没有被任何引用(可归档或删除)",
                    f.to_string_lossy()
                ),
                locate: Locate::File(project.join(f).to_string_lossy().to_string()),
            });
        }
    }

    // ⑤ 超长文件:全部项目文本文件(index.html / styles/main.css / assets 文本)
    let mut scan = vec![
        PathBuf::from("index.html"),
        PathBuf::from("styles/main.css"),
    ];
    scan.extend(files);
    for rel in scan {
        let p = project.join(&rel);
        let Ok(meta) = std::fs::metadata(&p) else {
            continue;
        };
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();
        let name = rel.to_string_lossy().to_string();
        if meta.len() > MAX_FILE_BYTES {
            out.push(HealthIssue {
                kind: HealthKind::LargeFile,
                message: format!(
                    "{name} 有 {} MB(阈值 {} MB)—— 影响打开/导出速度",
                    meta.len() / (1024 * 1024),
                    MAX_FILE_BYTES / (1024 * 1024)
                ),
                locate: Locate::File(p.to_string_lossy().to_string()),
            });
        }
        if TEXT_EXTS.contains(&ext.as_str()) {
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Some((no, len)) = text
                    .lines()
                    .enumerate()
                    .find(|(_, l)| l.chars().count() > MAX_LINE_CHARS)
                {
                    out.push(HealthIssue {
                        kind: HealthKind::LargeFile,
                        message: format!(
                            "{name} 第 {} 行超长({} 字符 > {MAX_LINE_CHARS})—— 多半是内联大图/压缩产物",
                            no + 1,
                            len.chars().count()
                        ),
                        locate: Locate::File(p.to_string_lossy().to_string()),
                    });
                }
            }
        }
    }

    // ⑥ 无障碍三查(07-N:缺 alt / 可访问名 / 对比度;只提示不强制)
    out.extend(a11y_issues(doc));

    // 同一引用被 kind 与 attrs 重复收集 → 报告去重(同类别同文案只留一条)
    out.sort_by(|a, b| (&a.kind, &a.message).cmp(&(&b.kind, &b.message)));
    out.dedup_by(|a, b| a.kind == b.kind && a.message == b.message);
    out
}

// ─────────────────────────── 报告窗口(文件 → 项目健康检查) ───────────────────────────

impl VellumApp {
    /// 项目健康检查报告窗口(07-E)。
    pub(crate) fn show_health_window(&mut self, ui: &mut egui::Ui) {
        if !self.health_open {
            return;
        }
        let mut open = true;
        egui::Window::new("项目健康检查")
            .open(&mut open)
            .collapsible(false)
            .default_size([620.0, 420.0])
            .show(ui.ctx(), |ui| {
                let Some(dir) = self.project_dir.clone() else {
                    ui.label("当前文档没有项目目录(先保存或打开一个项目)。");
                    return;
                };
                ui.horizontal(|ui| {
                    if ui.button("重新体检").clicked() {
                        self.health_report = Some(check(&self.doc, &dir));
                    }
                    ui.label(format!(
                        "项目:{}",
                        crate::recent::display_name(&dir)
                    ));
                });
                ui.separator();
                let issues = self.health_report.clone().unwrap_or_else(|| {
                    let r = check(&self.doc, &dir);
                    self.health_report = Some(r.clone());
                    r
                });
                if issues.is_empty() {
                    ui.colored_label(
                        vb_ui::theme::Tokens::get(self.theme_dark).success,
                        "未发现问题(缺失资源 / 失效链接 / 冻结块 / 未使用资产 / 超长文件 / 无障碍 全部通过)。",
                    );
                    return;
                }
                ui.label(format!(
                    "发现 {} 项(冻结块与无障碍为提示项,其余建议处理):",
                    issues.len()
                ));
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for it in &issues {
                        ui.horizontal(|ui| {
                            ui.colored_label(it.kind.color(self.theme_dark), it.kind.label());
                            let resp = ui.button(it.message.clone());
                            let tip = match &it.locate {
                                Locate::Node(sid) => {
                                    format!("点击选中图层({sid})")
                                }
                                Locate::File(p) => format!("点击查看路径:{p}"),
                            };
                            if resp.on_hover_text(tip).clicked() {
                                match &it.locate {
                                    Locate::Node(sid) => {
                                        if self.doc.find_by_sid(sid).is_some() {
                                            self.selection = vec![sid.clone()];
                                            self.say(format!("健康检查:已定位图层({sid})"));
                                        } else {
                                            self.say("该图层已不存在(文档可能已变更)");
                                        }
                                    }
                                    Locate::File(p) => {
                                        self.say(format!("健康检查:{p}"));
                                    }
                                }
                            }
                        });
                    }
                });
            });
        self.health_open = open;
    }
}

// ─────────────────────── 单测(临时坏项目夹具逐项检出) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时项目夹具:index.html + assets/{hero.png, old.png}(hero 被引用)。
    fn fixture(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vb-health-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("assets")).unwrap();
        std::fs::write(d.join("index.html"), "<html><body>ok</body></html>").unwrap();
        std::fs::write(d.join("assets/hero.png"), b"png").unwrap();
        std::fs::write(d.join("assets/old.png"), b"png").unwrap();
        d
    }

    /// 带节点的文档:画板上挂 src/href 属性(Image kind 由节点 kind 承载)。
    /// 注意 `new_default()` 自带一块默认画板 —— 直接挂它,不再新建。
    fn doc_with_refs() -> Document {
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let n = doc.nodes.get_mut(ab).unwrap();
        n.attrs.insert("href".into(), "about.html".into());
        doc
    }

    #[test]
    fn missing_asset_and_broken_link_are_detected() {
        let proj = fixture("miss");
        let mut doc = doc_with_refs();
        let ab = doc.artboards[0];
        // src 指向缺失资源;href 指向缺失页面
        let n = doc.nodes.get_mut(ab).unwrap();
        n.kind = NodeKind::Image {
            src: "assets/nope.png".into(),
        };
        n.attrs.insert("src".into(), "assets/nope.png".into());
        // 存在的引用不报
        n.attrs.insert("href".into(), "assets/hero.png".into());

        let issues = check(&doc, &proj);
        assert!(
            issues.iter().any(
                |i| i.kind == HealthKind::MissingAsset && i.message.contains("assets/nope.png")
            ),
            "缺失 src 必须检出:{issues:?}"
        );
        assert!(
            !issues.iter().any(|i| i.kind == HealthKind::BrokenLink),
            "存在的 href 不得误报失效:{issues:?}"
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn broken_link_detects_missing_href_target() {
        let proj = fixture("link");
        let doc = doc_with_refs(); // href → about.html(不存在)
        let issues = check(&doc, &proj);
        assert!(
            issues
                .iter()
                .any(|i| i.kind == HealthKind::BrokenLink && i.message.contains("about.html")),
            "失效 href 必须检出:{issues:?}"
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn external_refs_are_never_flagged() {
        let proj = fixture("ext");
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let n = doc.nodes.get_mut(ab).unwrap();
        n.attrs.insert("href".into(), "https://example.com".into());
        n.attrs
            .insert("src".into(), "data:image/png;base64,AAAA".into());
        let issues = check(&doc, &proj);
        assert!(
            !issues
                .iter()
                .any(|i| matches!(i.kind, HealthKind::MissingAsset | HealthKind::BrokenLink)),
            "远程/data 引用不得报缺失:{issues:?}"
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn frozen_blocks_are_counted_with_location() {
        let proj = fixture("frozen");
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let n = doc.nodes.get_mut(ab).unwrap();
        n.kind = NodeKind::Frozen {
            html: "<svg>…</svg>".into(),
        };
        let sid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
        let issues = check(&doc, &proj);
        let hits: Vec<_> = issues
            .iter()
            .filter(|i| i.kind == HealthKind::Frozen)
            .collect();
        assert_eq!(hits.len(), 1, "冻结块必须按位置逐条列出:{issues:?}");
        assert_eq!(hits[0].locate, Locate::Node(sid));
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn unused_assets_are_detected_and_used_ones_not() {
        let proj = fixture("unused");
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let n = doc.nodes.get_mut(ab).unwrap();
        n.kind = NodeKind::Image {
            src: "assets/hero.png".into(),
        };
        n.attrs.insert("src".into(), "assets/hero.png".into());
        let issues = check(&doc, &proj);
        assert!(
            issues
                .iter()
                .any(|i| i.kind == HealthKind::UnusedAsset && i.message.contains("old.png")),
            "未被引用的 assets 必须检出:{issues:?}"
        );
        assert!(
            !issues
                .iter()
                .any(|i| i.kind == HealthKind::UnusedAsset && i.message.contains("hero.png")),
            "被引用的资产不得误报未使用:{issues:?}"
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn css_url_counts_as_reference() {
        let proj = fixture("cssref");
        let mut doc = Document::new_default();
        doc.raw_css
            .push(".bg { background: url('assets/old.png'); }".into());
        let issues = check(&doc, &proj);
        assert!(
            !issues
                .iter()
                .any(|i| i.kind == HealthKind::UnusedAsset && i.message.contains("old.png")),
            "CSS url() 引用算数:{issues:?}"
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn long_line_and_oversize_files_are_detected() {
        let proj = fixture("large");
        let long_line = "x".repeat(MAX_LINE_CHARS + 1);
        std::fs::write(proj.join("index.html"), format!("<p>{long_line}</p>")).unwrap();
        let doc = Document::new_default();
        let issues = check(&doc, &proj);
        assert!(
            issues
                .iter()
                .any(|i| i.kind == HealthKind::LargeFile && i.message.contains("第 1 行超长")),
            "超长单行必须检出:{issues:?}"
        );
        // 超大文件(>2MB):直接写一个
        std::fs::write(proj.join("assets/big.css"), "a{}".repeat(1024 * 1024)).unwrap();
        let issues = check(&doc, &proj);
        assert!(
            issues
                .iter()
                .any(|i| i.kind == HealthKind::LargeFile && i.message.contains("big.css")),
            "超 2MB 文件必须检出:{issues:?}"
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn healthy_project_reports_nothing() {
        let proj = fixture("clean");
        // 全绿夹具:唯一资产被引用(删掉未引用的 old.png)
        std::fs::remove_file(proj.join("assets/old.png")).unwrap();
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let n = doc.nodes.get_mut(ab).unwrap();
        n.kind = NodeKind::Image {
            src: "assets/hero.png".into(),
        };
        n.attrs.insert("src".into(), "assets/hero.png".into());
        // 07-N:img 带 alt(空 alt = 装饰图同样合规)才不会报无障碍提示
        n.attrs.insert("alt".into(), "主视觉".into());
        assert!(check(&doc, &proj).is_empty(), "全绿项目不得有误报");
        std::fs::remove_dir_all(&proj).unwrap();
    }

    // ── 07-K:资产引用收集(单一来源,资产面板与体检共用)──

    /// src(Image kind + attrs 双写去重)/ href / CSS url() 都入映射;
    /// 远程与 data 引用不入;路径分隔符与大小写归一。
    #[test]
    fn asset_reference_map_collects_src_href_and_css() {
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let n = doc.nodes.get_mut(ab).unwrap();
        let sid = n.sid.as_str().to_string();
        n.kind = NodeKind::Image {
            src: "assets\\Hero.PNG".into(),
        };
        n.attrs.insert("src".into(), "assets\\Hero.PNG".into());
        n.attrs.insert("href".into(), "about.html".into());
        doc.raw_css
            .push(".bg { background: url('assets/old.png') }".into());

        let map = asset_reference_map(&doc);
        // kind + attrs 双写只出一行;键归一(小写 + `/`)
        let hero = map.get("assets/hero.png").expect("src 引用必须入映射");
        assert_eq!(hero.len(), 1, "同一引用的 kind/attrs 双写必须去重:{hero:?}");
        assert_eq!(hero[0].sid, sid);
        assert_eq!(hero[0].via, "src");
        let css = map.get("assets/old.png").expect("css url() 必须入映射");
        assert_eq!(css[0].via, "css url()");
        let link = map
            .get("about.html")
            .expect("href 引用同样入映射(定位共用)");
        assert_eq!(link[0].via, "href");
        // 远程引用不入
        let n = doc.nodes.get_mut(ab).unwrap();
        n.attrs.insert("src".into(), "https://cdn/x.png".into());
        assert!(!asset_reference_map(&doc).contains_key("https://cdn/x.png"));
    }

    /// 引用不存在时映射为空 → 资产全部判「未使用」(07-K 标记与 07-E ④同源)。
    #[test]
    fn asset_reference_map_empty_for_unreferenced_doc() {
        let doc = Document::new_default();
        assert!(asset_reference_map(&doc).is_empty());
    }

    // ── 07-N:无障碍检出(缺 alt / 可访问名 / 对比度) ──

    use vb_common::Rgba;
    use vb_doc::model::{Node, TextMode};

    /// 往默认画板挂一个节点,返回其 sid。
    fn add_node(doc: &mut Document, kind: NodeKind, name: &str) -> (String, vb_doc::model::NodeId) {
        let sid = doc.alloc_sid();
        let n = Node::new(kind, name, sid);
        let parent = doc.artboards[0];
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        (doc.nodes.get(id).unwrap().sid.as_str().to_string(), id)
    }

    fn text_node(text: &str) -> NodeKind {
        NodeKind::Text {
            text: text.into(),
            mode: TextMode::Point,
            segments: vec![],
        }
    }

    fn decl(p: &str, v: &str) -> vb_css::Decl {
        vb_css::Decl {
            prop: p.into(),
            value: v.into(),
            important: false,
        }
    }

    #[test]
    fn contrast_ratio_math_matches_wcag_reference() {
        // 黑/白 = 21:1 是 WCAG 文档的基准值
        let ratio = contrast_ratio(Rgba::BLACK, Rgba::WHITE);
        assert!((ratio - 21.0).abs() < 0.1, "黑白对比度应≈21:1,得 {ratio}");
        // 同色 = 1:1
        assert!((contrast_ratio(Rgba::WHITE, Rgba::WHITE) - 1.0).abs() < 1e-9);
        // 中灰对白在 3~4.5 之间(介于大字与正文阈值)
        let gray = Rgba::new(128, 128, 128, 255);
        let r = contrast_ratio(gray, Rgba::WHITE);
        assert!(r > CONTRAST_LARGE && r < CONTRAST_NORMAL, "中灰/白 = {r}");
    }

    #[test]
    fn missing_alt_flagged_but_empty_alt_is_decorative_ok() {
        let mut doc = Document::new_default();
        // 缺 alt → 报
        let (sid, _) = add_node(
            &mut doc,
            NodeKind::Image {
                src: "assets/a.png".into(),
            },
            "横幅",
        );
        // 空 alt = 装饰图 → 不报;有 alt → 不报
        let (_, id2) = add_node(
            &mut doc,
            NodeKind::Image {
                src: "assets/b.png".into(),
            },
            "装饰",
        );
        doc.nodes
            .get_mut(id2)
            .unwrap()
            .attrs
            .insert("alt".into(), String::new());
        let issues = a11y_issues(&doc);
        let alts: Vec<_> = issues
            .iter()
            .filter(|i| i.kind == HealthKind::A11yAlt)
            .collect();
        assert_eq!(alts.len(), 1, "只有缺 alt 的一张图被报:{alts:?}");
        assert!(alts[0].message.contains(&sid));
        assert!(matches!(&alts[0].locate, Locate::Node(s) if s == &sid));
    }

    #[test]
    fn interactive_without_accessible_name_is_flagged() {
        let mut doc = Document::new_default();
        // 图标按钮:button 无文本无 aria → 报
        let (_, id_btn) = add_node(&mut doc, NodeKind::Box, "图标按钮");
        {
            let n = doc.nodes.get_mut(id_btn).unwrap();
            n.tag = "button".into();
        }
        // 链接带可见文本(name from content)→ 不报
        let (_, id_a) = add_node(&mut doc, NodeKind::Box, "购买链接");
        {
            let n = doc.nodes.get_mut(id_a).unwrap();
            n.tag = "a".into();
            let sid = doc.alloc_sid();
            let child = Node::new(text_node("立即购买"), "链接文字", sid);
            let cid = doc.nodes.insert(child);
            doc.nodes.get_mut(cid).unwrap().parent = Some(id_a);
            doc.nodes.get_mut(id_a).unwrap().children.push(cid);
        }
        // aria-label 兜底 → 不报
        let (_, id_icon) = add_node(&mut doc, NodeKind::Box, "菜单按钮");
        {
            let n = doc.nodes.get_mut(id_icon).unwrap();
            n.tag = "button".into();
            n.attrs.insert("aria-label".into(), "打开菜单".into());
        }
        // role=button 的 div 无名 → 报
        let (_, id_role) = add_node(&mut doc, NodeKind::Box, "角色按钮");
        {
            let n = doc.nodes.get_mut(id_role).unwrap();
            n.attrs.insert("role".into(), "button".into());
        }
        let issues = a11y_issues(&doc);
        let names: Vec<_> = issues
            .iter()
            .filter(|i| i.kind == HealthKind::A11yName)
            .map(|i| match &i.locate {
                Locate::Node(s) => s.clone(),
                _ => String::new(),
            })
            .collect();
        assert_eq!(
            names.len(),
            2,
            "只报无名按钮(图标按钮 + 角色按钮):{names:?}"
        );
    }

    #[test]
    fn low_contrast_text_flagged_with_large_text_relaxed() {
        let mut doc = Document::new_default();
        // 白字白底:4.5 阈值必炸
        let (sid_bad, id_bad) = add_node(&mut doc, text_node("看不见的文字"), "隐身标题");
        {
            let n = doc.nodes.get_mut(id_bad).unwrap();
            // vb-token-ok: 测试夹具(文档内容色,非 UI 皮肤)
            n.style = vec![decl("color", "#fff"), decl("background-color", "#fff")];
        }
        // 深灰字白底:常规通过
        let (_, id_ok) = add_node(&mut doc, text_node("正常文字"), "正文");
        {
            let n = doc.nodes.get_mut(id_ok).unwrap();
            // vb-token-ok: 测试夹具(文档内容色,非 UI 皮肤)
            n.style = vec![decl("color", "#333")];
        }
        // 大字放宽(3:1):中灰 24px 大标题通过;同色 16px 正文报
        let (_, id_large) = add_node(&mut doc, text_node("大标题"), "大标题");
        {
            let n = doc.nodes.get_mut(id_large).unwrap();
            // vb-token-ok: 测试夹具(文档内容色,非 UI 皮肤)
            n.style = vec![decl("color", "#808080"), decl("font-size", "24px")];
        }
        let (_, id_small) = add_node(&mut doc, text_node("小灰字"), "小灰字");
        {
            let n = doc.nodes.get_mut(id_small).unwrap();
            // vb-token-ok: 测试夹具(文档内容色,非 UI 皮肤)
            n.style = vec![decl("color", "#808080")];
        }
        let issues = a11y_issues(&doc);
        let flagged: Vec<&HealthIssue> = issues
            .iter()
            .filter(|i| i.kind == HealthKind::A11yContrast)
            .collect();
        assert_eq!(flagged.len(), 2, "只报隐身文字与小灰字:{flagged:?}");
        assert!(flagged
            .iter()
            .any(|i| matches!(&i.locate, Locate::Node(s) if s == &sid_bad)));
        // 大字 24px 灰不报、正文 #333 不报(含在上面的 len 断言里)
    }

    #[test]
    fn contrast_resolves_css_variables() {
        let mut doc = Document::new_default();
        // vb-token-ok: 测试夹具(文档内容令牌,非 UI 皮肤)
        doc.tokens.push(("ink".into(), "#111111".into()));
        let (_, id) = add_node(&mut doc, text_node("令牌文字"), "令牌文字");
        doc.nodes.get_mut(id).unwrap().style = vec![
            decl("color", "var(--ink)"),
            decl("background-color", "var(--paper)"), // 未定义变量:忽略走默认白
        ];
        let issues = a11y_issues(&doc);
        assert!(
            !issues.iter().any(|i| i.kind == HealthKind::A11yContrast),
            "var 解析命中令牌 #111 对白底 17:1+,不应报:{issues:?}"
        );
    }

    #[test]
    fn a11y_issues_flow_into_health_check() {
        let proj = fixture("a11y");
        let mut doc = Document::new_default();
        let (_, id) = add_node(
            &mut doc,
            NodeKind::Image {
                src: "assets/hero.png".into(),
            },
            "横幅",
        );
        let _ = id;
        let issues = check(&doc, &proj);
        assert!(
            issues.iter().any(|i| i.kind == HealthKind::A11yAlt),
            "无障碍项必须进统一体检报告:{issues:?}"
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }
}
