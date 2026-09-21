//! `vb_layout` — 文档流布局求值(vb_doc 文档 → 具体矩形)。
//!
//! v0.1 导入器假定「矢量模式」页面(元素全部 position/left/top 定位);
//! artboard 等外部产物是文档流 + flex 布局。本 crate 以 taffy 为引擎,
//! 在导入后把 flow/flex/absolute 页面解析为具体矩形并写回 `Node::geom`
//! (父相对语义,与 encode 沿祖先累加一致;导出侧 geom_decls 随之产出
//! 规范化绝对定位 HTML,与既有 L1 幂等设计同向)。
//!
//! 求值范围(M1):
//! - var() 令牌替换(:root tokens + 元素自定义属性继承链)+ calc(px) 求值
//!   + em/rem(font-size 相对)+ %/pt
//! - display block/flex/none、position absolute/relative、inset、
//!   margin/padding、width/height/min/max、flex 全家、gap
//! - 文本叶:按盒宽贪心断行(与 cpu.rs 渲染同策略,行高 1.32 默认)
//! - 绝对定位包含块:最近 positioned 祖先(taffy 树内重挂载,文档树序不变)
//!
//! 不做(记录于流程表 M1.9):text-wrap balance/pretty、竖排、浮动、grid。

use std::collections::HashMap;
use std::path::Path;

use taffy::prelude::{auto, length, percent, zero};
use taffy::{
    AvailableSpace, Display, LayoutInput, LayoutOutput, LengthPercentage, LengthPercentageAuto,
    Position, Rect, Size, Style, TaffyTree, TraversePartialTree,
};
use vb_common::units::parse_px;
use vb_doc::model::{Document, Geom, Node, NodeId, NodeKind};

mod calc;

// ---------- transform 平移折算 ----------

/// 节点**自身** `transform` 里的平移分量(世界单位)。
///
/// 为什么必须折:浏览器里 `translate(-50%,-50%)` 是视觉居中,画布若只按
/// `left/top` 摆位,视觉位置会差半宽/半高(`s2-kv` 的轨道环、
/// `show2-card` 的四角都是这种形态)—— 判据 A「画布与浏览器一致」因此不成立。
///
/// **实现已上收到 `vb_common::transform`**:导出侧要用**相反符号**把这份
/// 平移补偿回去(见 `vb_doc::export::geom_decls`),两处必须同源,否则
/// "编辑一次跳一次、每存一轮漂一截"(2026-09-21 总验收实测)。
pub use vb_common::transform::parse_translate;

/// 从节点声明里取平移分量(无 `transform` → 零)。
fn own_translate(style: &[vb_css::Decl], w: f64, h: f64) -> (f64, f64) {
    match style.iter().find(|d| d.prop == "transform") {
        Some(d) => parse_translate(&d.value, w, h),
        None => (0.0, 0.0),
    }
}

// ---------- 公开接口 ----------

/// 布局告警(解析失败降级等;不阻断导出)。
pub struct LayoutOutcome {
    /// 每节点画板本地**绝对**矩形(sid → [x, y, w, h])。
    pub rects: HashMap<String, [f64; 4]>,
    pub warnings: Vec<String>,
}

/// 导入后对**全部画板**求值布局(P0-1:画布与几何查询用的坐标在内存计算)。
///
/// 无标记稿件的声明几何(百分比锚/inset/right|bottom/流式)经 taffy 解析为
/// 具体矩形写回 `Node::geom`;声明本身保留在 `Node::style`(`geom_declared`),
/// 保存时不烤入。`synthetic` 透传给[`apply_to_doc`](仅首个画板,与导入器
/// 「无标记才合成」的语义一致)。
pub fn apply_import_layout(
    doc: &mut Document,
    project_dir: Option<&Path>,
    synthetic: bool,
) -> Vec<String> {
    let mut ws = Vec::new();
    let boards = doc.artboards.clone();
    for (i, ab) in boards.into_iter().enumerate() {
        ws.extend(apply_to_doc(doc, ab, project_dir, synthetic && i == 0));
    }
    ws
}

/// 在导入后求值指定画板的布局,并把结果写回 `doc` 的节点几何
/// (父相对:子绝对 − 父绝对,encode 沿祖先累加后复原绝对位置)。
/// 返回告警。矢量模式文档(全部显式 left/top)不受影响:显式定位原样保留。
pub fn apply_to_doc(
    doc: &mut Document,
    artboard: NodeId,
    project_dir: Option<&Path>,
    synthetic: bool,
) -> Vec<String> {
    // 图像固有尺寸探测的项目根(量测叶用)
    set_image_dir(project_dir);
    let outcome = match compute_outcome(doc, artboard) {
        Ok(o) => o,
        Err(e) => return vec![format!("vb_layout:布局求值失败({e}),保持导入几何")],
    };
    // 父绝对坐标必须取自**未改写**的绝对表:若边遍历边写 geom,
    // 先改写的父节点会让后处理的子节点读到父相对值,几何混叠
    type AbsRow = (NodeId, Geom, Option<(f64, f64)>);
    let abs: Vec<AbsRow> = outcome
        .rects
        .iter()
        .filter_map(|(sid, r)| {
            let id = doc.find_by_sid(sid)?;
            let parent_abs = doc.node(id).and_then(|n| n.parent).and_then(|pid| {
                doc.node(pid)
                    .and_then(|pn| outcome.rects.get(pn.sid.as_str()))
                    .map(|pr| (pr[0], pr[1]))
            });
            Some((
                id,
                Geom {
                    x: r[0],
                    y: r[1],
                    w: r[2],
                    h: r[3],
                },
                parent_abs,
            ))
        })
        .collect();
    let mut applied = 0usize;
    for (id, g, parent_abs) in abs {
        let Some(node) = doc.node(id) else { continue };
        if matches!(node.kind, NodeKind::Artboard) {
            continue;
        }
        let (px0, py0) = parent_abs.unwrap_or((0.0, 0.0));
        // 自身 `transform` 的平移分量折进画布几何(见 `own_translate`):
        // 祖先的平移经"父相对坐标"自然继承(每个祖先自己也折了),
        // 因此这里只需加自己的那一份。
        let (tx, ty) = own_translate(&node.style, g.w, g.h);
        if let Some(n) = doc.node_mut(id) {
            n.geom = Geom {
                x: g.x - px0 + tx,
                y: g.y - py0 + ty,
                w: g.w,
                h: g.h,
            };
        }
        applied += 1;
    }
    let mut ws = outcome.warnings;
    ws.push(format!("vb_layout:已求值 {applied} 个节点几何"));
    // 合成画板(无 vb-artboard 标记):画布尺寸 = 内容包围盒(浏览器 full_page
    // 语义)。包围盒计算**尊重 overflow:hidden 裁剪**——`.poster` 这类显式
    // 尺寸 + 裁剪容器声明画布真值,其子内容溢出不撑大画布(此前 250px 溢出
    // 把 A4 高度回填成 2004,存量项目静默失真)。
    if synthetic {
        let ab_sid = doc.node(artboard).map(|n| n.sid.as_str().to_string());
        // 画板显式几何(body width/max-width/height 摘自 CSS)是硬约束:
        // 回填尺寸不得超过(浏览器 overflow-x:hidden 语义)
        let (ab_cap_w, ab_cap_h) = doc
            .node(artboard)
            .map(|n| {
                (
                    n.authored[2].then_some(n.geom.w),
                    n.authored[3].then_some(n.geom.h),
                )
            })
            .unwrap_or((None, None));
        let (mut max_w, mut max_h) = (0.0f64, 0.0f64);
        let mut clipped = false;
        for (sid, r) in &outcome.rects {
            if Some(sid.as_str()) == ab_sid.as_deref() {
                continue;
            }
            let Some(id) = doc.find_by_sid(sid) else {
                continue;
            };
            // 沿祖先链找 overflow:hidden 容器,把矩形裁进其计算矩形
            let mut rect = *r;
            let mut cur = doc.node(id).and_then(|n| n.parent);
            while let Some(pid) = cur {
                let Some(pn) = doc.node(pid) else { break };
                let clips = pn.style.iter().any(|d| {
                    (d.prop == "overflow" || d.prop == "overflow-x" || d.prop == "overflow-y")
                        && d.value.trim() == "hidden"
                });
                if clips {
                    if let Some(pr) = outcome.rects.get(pn.sid.as_str()) {
                        let (x1, y1) = (rect[0].max(pr[0]), rect[1].max(pr[1]));
                        let (x2, y2) = (
                            (rect[0] + rect[2]).min(pr[0] + pr[2]),
                            (rect[1] + rect[3]).min(pr[1] + pr[3]),
                        );
                        rect = [x1, y1, (x2 - x1).max(0.0), (y2 - y1).max(0.0)];
                        if rect[2] == 0.0 || rect[3] == 0.0 {
                            break;
                        }
                    }
                }
                cur = pn.parent;
            }
            if rect[2] == 0.0 || rect[3] == 0.0 {
                continue;
            }
            let before = (r[0] + r[2]).max(r[1] + r[3]);
            let after = (rect[0] + rect[2]).max(rect[1] + rect[3]);
            if after + 0.5 < before {
                clipped = true;
            }
            max_w = max_w.max(rect[0] + rect[2]);
            max_h = max_h.max(rect[1] + rect[3]);
        }
        if let Some(cap) = ab_cap_w {
            max_w = max_w.min(cap);
        }
        if let Some(cap) = ab_cap_h {
            max_h = max_h.min(cap);
        }
        if max_w > 0.0 && max_h > 0.0 {
            if let Some(n) = doc.node_mut(artboard) {
                n.geom.w = max_w;
                n.geom.h = max_h;
            }
            ws.push(format!("vb_layout:合成画板尺寸回填 {max_w:.0}x{max_h:.0}"));
            if clipped {
                ws.push("vb_layout:内容溢出已按 overflow:hidden 裁剪参与回填".into());
            }
        }
    }
    ws
}

fn compute_outcome(doc: &Document, artboard: NodeId) -> Result<LayoutOutcome, String> {
    let ab = doc.node(artboard).ok_or_else(|| "画板不存在".to_string())?;
    if !matches!(ab.kind, NodeKind::Artboard) {
        return Err("目标不是画板".to_string());
    }

    let mut ctx = BuildCtx {
        doc,
        tree: TaffyTree::new(),
        rects: HashMap::new(),
        warnings: Vec::new(),
        taffy_of: HashMap::new(),
        children_of: HashMap::new(),
        vars_of: HashMap::new(),
        font_of: HashMap::new(),
    };

    // 自顶向下:令牌/字体继承链(先于 taffy 样式求值)
    let root_vars: Vec<(String, String)> = doc
        .tokens
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let font0 = FontCtx {
        size: 16.0,
        family: String::new(),
        weight: 400,
        line_height: 0.0,
        letter_spacing: 0.0,
    };
    ctx.inherit(artboard, root_vars, font0);

    // taffy 树:先决定每个节点的 taffy 父(绝对定位重挂载到最近 positioned 祖先)
    ctx.decide_parents(artboard, artboard, true);
    let root = ctx.build(artboard);

    let available = Size {
        width: AvailableSpace::Definite(ab.geom.w as f32),
        height: AvailableSpace::Definite(ab.geom.h as f32),
    };
    ctx.tree
        .compute_layout_with_measure(root, available, |input, _node, node_ctx, _style| {
            measure_leaf(input, node_ctx)
        })
        .map_err(|e| format!("taffy 求值失败:{e}"))?;

    ctx.collect(root);
    if std::env::var("KILN_DUMP_RECTS").is_ok() {
        let mut names: Vec<String> = Vec::new();
        for (id, tid) in &ctx.taffy_of {
            if let Some(n) = ctx.doc.node(*id) {
                if let Some(r) = ctx.rects.get(n.sid.as_str()) {
                    names.push(format!(
                        "{:?} name={} rect=({:.0},{:.0},{:.0},{:.0}) geom=({:.0},{:.0},{:.0},{:.0}) authored={:?} pos={:?}",
                        n.kind.kind_name(),
                        n.name,
                        r[0],
                        r[1],
                        r[2],
                        r[3],
                        n.geom.x,
                        n.geom.y,
                        n.geom.w,
                        n.geom.h,
                        n.authored,
                        n.style_get("position"),
                    ));
                }
            }
            let _ = tid;
        }
        names.sort();
        for n in &names {
            eprintln!("[rect] {n}");
        }
    }
    Ok(LayoutOutcome {
        rects: ctx.rects,
        warnings: ctx.warnings,
    })
}

// ---------- 叶量测 ----------

#[derive(Debug, Clone)]
enum LeafCtx {
    Text {
        text: String,
        font_size: f64,
        family: String,
        weight: u16,
        line_height: f64,
        letter_spacing: f64,
        nowrap: bool,
    },
    Image {
        src: String,
    },
    Fixed {
        w: f64,
        h: f64,
    },
}

fn measure_leaf(input: LayoutInput, node_ctx: Option<&mut LeafCtx>) -> LayoutOutput {
    let Some(ctx) = node_ctx else {
        return LayoutOutput::DEFAULT;
    };
    let known_w = input.known_dimensions.width;
    let known_h = input.known_dimensions.height;
    let max_w = known_w.unwrap_or(match input.available_space.width {
        AvailableSpace::Definite(x) => x,
        _ => f32::MAX,
    });

    let (w, h) = match ctx {
        LeafCtx::Text {
            text,
            font_size,
            family,
            weight,
            line_height,
            letter_spacing,
            nowrap,
        } => {
            let lh = if *line_height > 0.0 {
                *line_height
            } else {
                *font_size * 1.32
            } as f32;
            let ls = *letter_spacing as f32;
            if *nowrap {
                let (mw, _n) = vb_render::text::measure_text_weighted(
                    text,
                    family,
                    *font_size as f32,
                    *weight,
                    f32::MAX,
                    ls,
                );
                (
                    known_w.unwrap_or(mw.max(1.0)),
                    known_h.unwrap_or(lh.max(1.0).ceil()),
                )
            } else {
                let (mw, lines) = vb_render::text::measure_text_weighted(
                    text,
                    family,
                    *font_size as f32,
                    *weight,
                    max_w,
                    ls,
                );
                let total_h = lines as f32 * lh;
                (
                    known_w.unwrap_or(mw.max(1.0)),
                    known_h.unwrap_or(total_h.max(lh).ceil()),
                )
            }
        }
        LeafCtx::Image { src } => {
            let natural = probe_image(src).unwrap_or((320.0, 180.0));
            let w = known_w.unwrap_or(natural.0 as f32);
            let h = known_h.unwrap_or(if known_w.is_some() {
                natural.1 as f32 * (w / natural.0 as f32)
            } else {
                natural.1 as f32
            });
            (w, h)
        }
        LeafCtx::Fixed { w, h } => (known_w.unwrap_or(*w as f32), known_h.unwrap_or(*h as f32)),
    };
    LayoutOutput::from_sizes(
        Size {
            width: w.max(0.0),
            height: h.max(0.0),
        },
        Rect::ZERO,
    )
}

thread_local! {
    static IMG_DIR: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
    static IMG_CACHE: std::cell::RefCell<HashMap<String, (f64, f64)>> =
        std::cell::RefCell::new(HashMap::new());
}

fn set_image_dir(dir: Option<&Path>) {
    IMG_DIR.with(|d| *d.borrow_mut() = dir.map(|p| p.to_path_buf()));
}

fn probe_image(src: &str) -> Option<(f64, f64)> {
    if let Some(v) = IMG_CACHE.with(|c| c.borrow().get(src).copied()) {
        return Some(v);
    }
    let dir = IMG_DIR.with(|d| d.borrow().clone())?;
    let (w0, h0) = image::image_dimensions(dir.join(src)).ok()?;
    let v = (w0 as f64, h0 as f64);
    IMG_CACHE.with(|c| c.borrow_mut().insert(src.to_string(), v));
    Some(v)
}

// ---------- 继承与求值上下文 ----------

#[derive(Debug, Clone)]
struct FontCtx {
    size: f64,
    family: String,
    weight: u16,
    /// px;0 = 未指定(渲染期默认 1.32×size)
    line_height: f64,
    letter_spacing: f64,
}

struct BuildCtx<'a> {
    doc: &'a Document,
    tree: TaffyTree<LeafCtx>,
    rects: HashMap<String, [f64; 4]>,
    warnings: Vec<String>,
    /// doc NodeId → taffy NodeId
    taffy_of: HashMap<NodeId, taffy::NodeId>,
    /// taffy 父决策后的子列表(保文档序;绝对定位子已改挂包含块)
    children_of: HashMap<NodeId, Vec<NodeId>>,
    /// doc NodeId → 继承的令牌表(含自身定义)
    vars_of: HashMap<NodeId, Vec<(String, String)>>,
    /// doc NodeId → 继承字体上下文
    font_of: HashMap<NodeId, FontCtx>,
}

impl<'a> BuildCtx<'a> {
    fn inherit(&mut self, id: NodeId, mut vars: Vec<(String, String)>, font: FontCtx) {
        let node = self.doc.node(id).expect("node");
        // 自身自定义属性覆盖继承
        for d in &node.style {
            if let Some(name) = d.prop.strip_prefix("--") {
                vars.retain(|(k, _)| k != name);
                vars.push((name.to_string(), d.value.clone()));
            }
        }
        let get = |p: &str| {
            node.style
                .iter()
                .find(|d| d.prop == p)
                .map(|d| d.value.clone())
        };
        let mut f = font.clone();
        if let Some(fs) = get("font-size").and_then(|v| self.resolve_len(&v, &vars, f.size)) {
            if fs > 0.0 {
                f.size = fs;
            }
        }
        if let Some(fam) = get("font-family") {
            f.family = fam;
        }
        if let Some(w) = get("font-weight").and_then(|w| match w.trim() {
            "bold" => Some(700u16),
            "normal" => Some(400u16),
            other => other.parse::<u16>().ok(),
        }) {
            f.weight = w;
        }
        if let Some(lh) = get("line-height") {
            let t = lh.trim();
            if let Ok(n) = t.parse::<f64>() {
                // 纯数字 = 字号倍数(CSS 无单位 line-height)
                f.line_height = n * f.size;
            } else if let Some(px) = self.resolve_len(t, &vars, f.size) {
                f.line_height = px;
            }
        }
        if let Some(ls) = get("letter-spacing").and_then(|v| self.resolve_len(&v, &vars, f.size)) {
            f.letter_spacing = ls;
        }
        self.vars_of.insert(id, vars.clone());
        self.font_of.insert(id, f.clone());
        for c in node.children.clone() {
            self.inherit(c, vars.clone(), f.clone());
        }
    }

    /// var() 替换(calc 求值与 em/rem 换算见 calc 模块;此处只做令牌链)。
    pub fn resolve_var(&self, raw: &str, vars: &[(String, String)]) -> String {
        let mut cur = raw.trim().to_string();
        for _ in 0..8 {
            let Some(inner) = cur
                .trim()
                .strip_prefix("var(--")
                .and_then(|s| s.strip_suffix(')'))
            else {
                break;
            };
            let (name, fallback) = match inner.split_once(',') {
                Some((n, f)) => (n.trim(), Some(f.trim())),
                None => (inner.trim(), None),
            };
            if let Some((_, v)) = vars.iter().find(|(k, _)| k == name) {
                cur = v.clone();
            } else if let Some(f) = fallback {
                cur = f.to_string();
            } else {
                break;
            }
        }
        cur
    }

    /// 长度求值:px/pt/em/rem(/calc);% 返回 None(taffy 原生处理)。
    pub fn resolve_len(&self, raw: &str, vars: &[(String, String)], font_size: f64) -> Option<f64> {
        let v = self.resolve_var(raw, vars);
        let t = v.trim();
        if let Some(inner) = t.strip_prefix("calc(").and_then(|s| s.strip_suffix(')')) {
            return calc::eval(inner, vars, font_size, self);
        }
        if t.ends_with('%') {
            return None;
        }
        if let Some(n) = t.strip_suffix("em") {
            return n.trim().parse::<f64>().ok().map(|n| n * font_size);
        }
        if let Some(n) = t.strip_suffix("rem") {
            return n.trim().parse::<f64>().ok().map(|n| n * 16.0);
        }
        parse_px(t)
    }

    // ---------- taffy 父决策(绝对定位包含块) ----------

    fn positioned(&self, id: NodeId) -> bool {
        let node = self.doc.node(id).expect("node");
        match node.authored_position.as_deref() {
            Some(p) => p != "static",
            None => node.authored[0] || node.authored[1],
        }
    }

    fn decide_parents(&mut self, id: NodeId, containing_block: NodeId, is_root: bool) {
        let node = self.doc.node(id).expect("node");
        let own_cb = if is_root || self.positioned(id) {
            id
        } else {
            containing_block
        };
        let kids = node.children.clone();
        let mut normal = Vec::new();
        for &c in &kids {
            let cnode = self.doc.node(c).expect("child");
            let c_abs = !cnode.hidden
                && cnode
                    .style_get("position")
                    .map(|p| p == "absolute" || p == "fixed")
                    .unwrap_or(false);
            self.decide_parents(c, own_cb, false);
            if c_abs && own_cb != id {
                // 绝对定位子改挂包含块(最近 positioned 祖先)
                self.children_of.entry(own_cb).or_default().push(c);
            } else {
                normal.push(c);
            }
        }
        self.children_of.insert(id, normal);
    }

    fn build(&mut self, id: NodeId) -> taffy::NodeId {
        let node = self.doc.node(id).expect("node");
        let vars = self.vars_of.get(&id).cloned().unwrap_or_default();
        let font = self.font_of.get(&id).cloned().unwrap_or(FontCtx {
            size: 16.0,
            family: String::new(),
            weight: 400,
            line_height: 0.0,
            letter_spacing: 0.0,
        });
        let style = self.taffy_style(node, &vars, &font);
        let kids = self.children_of.get(&id).cloned().unwrap_or_default();
        let tid = if kids.is_empty() {
            let ctx = self.leaf_ctx(node, &font);
            self.tree.new_leaf_with_context(style, ctx).expect("leaf")
        } else {
            let child_ids: Vec<taffy::NodeId> = kids.iter().map(|&c| self.build(c)).collect();
            self.tree
                .new_with_children(style, &child_ids)
                .expect("node")
        };
        self.taffy_of.insert(id, tid);
        tid
    }

    fn leaf_ctx(&self, node: &Node, font: &FontCtx) -> LeafCtx {
        match &node.kind {
            NodeKind::Text { text, .. } => {
                let nowrap = node
                    .style_get("white-space")
                    .map(|w| w == "nowrap")
                    .unwrap_or(false);
                LeafCtx::Text {
                    text: text.clone(),
                    font_size: font.size,
                    family: font.family.clone(),
                    weight: font.weight,
                    line_height: font.line_height,
                    letter_spacing: font.letter_spacing,
                    nowrap,
                }
            }
            NodeKind::Image { src } => LeafCtx::Image { src: src.clone() },
            _ => LeafCtx::Fixed {
                w: node.geom.w.max(1.0),
                h: node.geom.h.max(1.0),
            },
        }
    }

    // ---------- 样式 → taffy ----------

    fn taffy_style(&mut self, node: &Node, vars: &[(String, String)], font: &FontCtx) -> Style {
        let get = |p: &str| {
            node.style
                .iter()
                .find(|d| d.prop == p)
                .map(|d| d.value.clone())
        };
        let lp = |v: &str| -> Option<LengthPercentage> {
            if let Some(px) = self.resolve_len(v, vars, font.size) {
                return Some(length(px as f32));
            }
            let t = self.resolve_var(v, vars);
            t.trim()
                .strip_suffix('%')
                .and_then(|n| n.trim().parse::<f32>().ok())
                .map(|n| percent(n / 100.0))
        };
        let lpa = |v: &str| -> Option<LengthPercentageAuto> {
            if self.resolve_var(v, vars).trim() == "auto" {
                return Some(auto());
            }
            lp(v).map(LengthPercentageAuto::from)
        };

        let hidden = node.hidden || get("display").map(|d| d.trim() == "none").unwrap_or(false);
        let display = if hidden {
            Display::None
        } else {
            match get("display").as_deref() {
                Some("flex") | Some("inline-flex") => Display::Flex,
                Some("grid") | Some("inline-grid") => Display::Grid,
                _ => Display::Block,
            }
        };
        // CSS Grid 模板解析(taffy parse feature;失败降级 Block + 告警)
        let mut grid_warning: Option<String> = None;
        let (grid_template_columns, grid_template_rows) = if display == Display::Grid {
            use taffy::style::{GridTemplateComponent, GridTemplateTracks};
            type Tracks = Vec<GridTemplateComponent<String>>;
            let parse_tracks = |v: &str| -> Result<Tracks, String> {
                let v = v.trim();
                if v.is_empty() {
                    return Ok(Vec::new());
                }
                let parsed: GridTemplateTracks<String, GridTemplateComponent<String>> =
                    v.parse().map_err(|e| format!("{e}"))?;
                Ok(parsed.tracks)
            };
            let cols_owned = get("grid-template-columns").map(|v| v.trim().to_string());
            let cols = cols_owned.as_deref().filter(|v| !v.is_empty());
            let rows_owned = get("grid-template-rows").map(|v| v.trim().to_string());
            let rows = rows_owned.as_deref().filter(|v| !v.is_empty());
            let mut gc = Vec::new();
            let mut gr = Vec::new();
            if let Some(v) = cols {
                match parse_tracks(v) {
                    Ok(t) => gc = t,
                    Err(e) => {
                        grid_warning = Some(format!("grid-template-columns '{v}' 未识别({e})"))
                    }
                }
            }
            if let Some(v) = rows {
                match parse_tracks(v) {
                    Ok(t) => gr = t,
                    Err(e) => grid_warning = Some(format!("grid-template-rows '{v}' 未识别({e})")),
                }
            }
            if grid_warning.is_some() {
                // 模板不可解析:整容器降级块布局,不静默塌单列
                (Vec::new(), Vec::new())
            } else {
                (gc, gr)
            }
        } else {
            (Vec::new(), Vec::new())
        };
        let display = if grid_warning.is_some() {
            Display::Block
        } else {
            display
        };
        // 定位:authored_position(导入记录)优先;否则按 authored left/top 推断
        let position = match node.authored_position.as_deref() {
            Some("absolute") | Some("fixed") => Position::Absolute,
            Some("static") => Position::Relative,
            _ => {
                if node.authored[0] || node.authored[1] {
                    Position::Absolute
                } else {
                    Position::Relative
                }
            }
        };

        // 显式尺寸:style 声明优先;导入期摘进 geom 的(authored)回退读取
        let size = Size {
            width: get("width")
                .and_then(|v| dim_of(self, &v, vars, font.size))
                .or_else(|| {
                    node.authored[2].then_some(taffy::Dimension::length(node.geom.w as f32))
                })
                .unwrap_or(taffy::Dimension::auto()),
            height: get("height")
                .and_then(|v| dim_of(self, &v, vars, font.size))
                .or_else(|| {
                    node.authored[3].then_some(taffy::Dimension::length(node.geom.h as f32))
                })
                .unwrap_or(taffy::Dimension::auto()),
        };
        let min_size = Size {
            width: get("min-width")
                .and_then(|v| lpa_of(self, &v, vars, font.size))
                .unwrap_or(auto()),
            height: get("min-height")
                .and_then(|v| lpa_of(self, &v, vars, font.size))
                .unwrap_or(auto()),
        };
        let max_size = Size {
            width: get("max-width")
                .and_then(|v| lpa_of(self, &v, vars, font.size))
                .unwrap_or(auto()),
            height: get("max-height")
                .and_then(|v| lpa_of(self, &v, vars, font.size))
                .unwrap_or(auto()),
        };

        let margin = expand_raw(
            get("margin").as_deref(),
            get("margin-top").as_deref(),
            get("margin-right").as_deref(),
            get("margin-bottom").as_deref(),
            get("margin-left").as_deref(),
        )
        .map(|r| Rect {
            left: lpa(&r.left).unwrap_or(auto()),
            right: lpa(&r.right).unwrap_or(auto()),
            top: lpa(&r.top).unwrap_or(auto()),
            bottom: lpa(&r.bottom).unwrap_or(auto()),
        })
        .unwrap_or(Rect {
            left: auto(),
            right: auto(),
            top: auto(),
            bottom: auto(),
        });
        let padding = expand_raw(
            get("padding").as_deref(),
            get("padding-top").as_deref(),
            get("padding-right").as_deref(),
            get("padding-bottom").as_deref(),
            get("padding-left").as_deref(),
        )
        .map(|r| Rect {
            left: lp(&r.left).unwrap_or_else(zero),
            right: lp(&r.right).unwrap_or_else(zero),
            top: lp(&r.top).unwrap_or_else(zero),
            bottom: lp(&r.bottom).unwrap_or_else(zero),
        })
        .unwrap_or(Rect {
            left: zero(),
            right: zero(),
            top: zero(),
            bottom: zero(),
        });

        // inset 简写(1-4 值)作为各边回退
        let inset_all: Vec<String> = get("inset")
            .as_deref()
            .map(|v| {
                let toks: Vec<&str> = v.split_whitespace().collect();
                let order: Vec<&str> = match toks.len() {
                    1 => vec![toks[0]; 4],
                    2 => vec![toks[0], toks[1], toks[0], toks[1]],
                    3 => vec![toks[0], toks[1], toks[2], toks[1]],
                    _ => toks.iter().take(4).copied().collect(),
                };
                order.into_iter().map(String::from).collect()
            })
            .unwrap_or_default();
        let side = |v: Option<&str>, i: usize| -> Option<LengthPercentageAuto> {
            v.or(inset_all.get(i).map(|s| s.as_str())).and_then(&lpa)
        };
        // 绝对定位:authored 的 left/top 已在 geom(相对包含块)回退读取
        let inset = Rect {
            left: side(get("left").as_deref(), 0)
                .or_else(|| {
                    node.authored[0].then_some(LengthPercentageAuto::length(node.geom.x as f32))
                })
                .unwrap_or(auto()),
            right: side(get("right").as_deref(), 1).unwrap_or(auto()),
            top: side(get("top").as_deref(), 3)
                .or_else(|| {
                    node.authored[1].then_some(LengthPercentageAuto::length(node.geom.y as f32))
                })
                .unwrap_or(auto()),
            bottom: side(get("bottom").as_deref(), 2).unwrap_or(auto()),
        };

        let flex_direction = match get("flex-direction").as_deref() {
            Some("row-reverse") => taffy::FlexDirection::RowReverse,
            Some("column") => taffy::FlexDirection::Column,
            Some("column-reverse") => taffy::FlexDirection::ColumnReverse,
            _ => taffy::FlexDirection::Row,
        };
        let flex_wrap = match get("flex-wrap").as_deref() {
            Some("wrap") => taffy::FlexWrap::Wrap,
            Some("wrap-reverse") => taffy::FlexWrap::WrapReverse,
            _ => taffy::FlexWrap::NoWrap,
        };
        let justify_content = match get("justify-content").as_deref() {
            Some("center") => Some(taffy::JustifyContent::CENTER),
            Some("flex-end") | Some("end") => Some(taffy::JustifyContent::FLEX_END),
            Some("space-between") => Some(taffy::JustifyContent::SPACE_BETWEEN),
            Some("space-around") => Some(taffy::JustifyContent::SPACE_AROUND),
            Some("space-evenly") => Some(taffy::JustifyContent::SPACE_EVENLY),
            _ => None,
        };
        let align_items = match get("align-items").as_deref() {
            Some("center") => Some(taffy::AlignItems::CENTER),
            Some("flex-end") | Some("end") => Some(taffy::AlignItems::FLEX_END),
            Some("flex-start") | Some("start") => Some(taffy::AlignItems::FLEX_START),
            Some("baseline") => Some(taffy::AlignItems::BASELINE),
            Some("stretch") => Some(taffy::AlignItems::STRETCH),
            _ => None,
        };
        let align_self = match get("align-self").as_deref() {
            Some("center") => Some(taffy::AlignSelf::CENTER),
            Some("flex-end") | Some("end") => Some(taffy::AlignSelf::FLEX_END),
            Some("flex-start") | Some("start") => Some(taffy::AlignSelf::FLEX_START),
            Some("stretch") => Some(taffy::AlignSelf::STRETCH),
            _ => None,
        };
        let gap = {
            let g = get("gap");
            let (r, c) = match g.as_deref() {
                Some(s) if !s.trim().is_empty() => {
                    let mut it = s.split_whitespace();
                    let a = it.next().unwrap_or("0");
                    let b = it.next().unwrap_or(a);
                    (a, b)
                }
                _ => ("0", "0"),
            };
            Size {
                width: lp(c).unwrap_or_else(zero),
                height: lp(r).unwrap_or_else(zero),
            }
        };
        // flex 简写
        let (flex_grow, flex_shrink, flex_basis) = match get("flex").as_deref().map(str::trim) {
            Some("none") => (0.0_f32, 0.0_f32, taffy::Dimension::auto()),
            Some("auto") => (1.0, 1.0, taffy::Dimension::auto()),
            Some(v) if !v.is_empty() && v.parse::<f32>().is_ok() => {
                (v.parse::<f32>().unwrap(), 1.0, taffy::Dimension::auto())
            }
            Some(other) => {
                let mut it = other.split_whitespace();
                let grow = it.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                let shrink = it.next().and_then(|v| v.parse::<f32>().ok()).unwrap_or(1.0);
                let basis = it
                    .next()
                    .and_then(|v| dim_of(self, v, vars, font.size))
                    .unwrap_or(taffy::Dimension::auto());
                (grow, shrink, basis)
            }
            None => (0.0, 1.0, taffy::Dimension::auto()),
        };

        if let Some(w) = &grid_warning {
            self.warnings
                .push(format!("vb_layout:{w};该 grid 容器按块布局降级"));
        }
        Style {
            display,
            position,
            inset,
            size,
            min_size,
            max_size,
            margin,
            padding,
            flex_direction,
            flex_wrap,
            justify_content,
            align_items,
            align_self,
            gap,
            flex_grow,
            flex_shrink,
            flex_basis,
            grid_template_columns,
            grid_template_rows,
            ..Default::default()
        }
    }

    // ---------- 结果回收 ----------

    fn collect(&mut self, tid_root: taffy::NodeId) {
        let id_by_taffy: HashMap<taffy::NodeId, NodeId> =
            self.taffy_of.iter().map(|(d, t)| (*t, *d)).collect();
        let mut stack: Vec<(taffy::NodeId, f64, f64)> = vec![(tid_root, 0.0, 0.0)];
        while let Some((tid, ox, oy)) = stack.pop() {
            let layout = self.tree.layout(tid).expect("layout");
            let ax = ox + layout.location.x as f64;
            let ay = oy + layout.location.y as f64;
            if let Some(&doc_id) = id_by_taffy.get(&tid) {
                if let Some(n) = self.doc.node(doc_id) {
                    let sid = n.sid.as_str().to_string();
                    self.rects.insert(
                        sid,
                        [ax, ay, layout.size.width as f64, layout.size.height as f64],
                    );
                }
            }
            for c in self.tree.child_ids(tid) {
                stack.push((c, ax, ay));
            }
        }
    }
}

// ---------- 辅助 ----------

fn dim_of(
    ctx: &BuildCtx,
    v: &str,
    vars: &[(String, String)],
    font_size: f64,
) -> Option<taffy::Dimension> {
    let t = ctx.resolve_var(v, vars);
    let t = t.trim();
    if t == "auto" || t.is_empty() {
        return Some(taffy::Dimension::auto());
    }
    if let Some(px) = ctx.resolve_len(v, vars, font_size) {
        return Some(length(px as f32));
    }
    if let Some(n) = t.strip_suffix('%').and_then(|n| n.parse::<f32>().ok()) {
        return Some(percent(n / 100.0));
    }
    None
}

fn lpa_of(
    ctx: &BuildCtx,
    v: &str,
    vars: &[(String, String)],
    font_size: f64,
) -> Option<LengthPercentageAuto> {
    use taffy::style_helpers::TaffyZero as _;
    match dim_of(ctx, v, vars, font_size).map(|d| d.expand()) {
        Some(taffy::ExpandedDimension::Length(n)) => Some(LengthPercentageAuto::length(n)),
        Some(taffy::ExpandedDimension::Percent(n)) => Some(LengthPercentageAuto::percent(n)),
        _ => Some(LengthPercentageAuto::ZERO),
    }
}

/// 盒简写 1-4 值展开(返回原始值串;调用方按属性类型解析)。
/// CSS 级联语义:同侧**长边声明覆盖简写**(`margin:0; margin-top:34px` 的
/// 顶边是 34px)——此前简写存在时直接短路,长边被忽略,flow 间距全塌。
fn expand_raw(
    all: Option<&str>,
    top: Option<&str>,
    right: Option<&str>,
    bottom: Option<&str>,
    left: Option<&str>,
) -> Option<Rect<String>> {
    let own = |v: Option<&str>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let Some(a) = own(all) else {
        return Some(Rect {
            top: own(top).unwrap_or_else(|| "0".into()),
            right: own(right).unwrap_or_else(|| "0".into()),
            bottom: own(bottom).unwrap_or_else(|| "0".into()),
            left: own(left).unwrap_or_else(|| "0".into()),
        });
    };
    let toks: Vec<&str> = a.split_whitespace().collect();
    let order: Vec<&str> = match toks.len() {
        1 => vec![toks[0]; 4],
        2 => vec![toks[0], toks[1], toks[0], toks[1]],
        3 => vec![toks[0], toks[1], toks[2], toks[1]],
        _ => vec![toks[0], toks[1], toks[2], toks[3]],
    };
    Some(Rect {
        top: own(top).unwrap_or_else(|| order[0].to_string()),
        right: own(right).unwrap_or_else(|| order[1].to_string()),
        bottom: own(bottom).unwrap_or_else(|| order[2].to_string()),
        left: own(left).unwrap_or_else(|| order[3].to_string()),
    })
}
