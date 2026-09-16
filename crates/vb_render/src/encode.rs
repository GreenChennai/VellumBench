//! Document → `DrawList`:引擎中立的绘制指令编码(CPU/GPU 单一来源,ADR-0016)。
//!
//! 坐标:画板本地 px(Y 向下);嵌套节点的绝对位置 = 沿祖先链累加 x/y
//! (导入/导出按"相对最近定位祖先"存储,见设计文档 04 篇 §二)。

use vb_doc::model::{Document, Node, NodeId, NodeKind};
use vb_doc::VbError;

use vb_css::split_top_level as vb_css_split_top_level;

#[derive(Debug, Clone)]
pub struct GradientStop {
    /// 0..=1
    pub pos: f32,
    pub color: [f32; 4],
}

#[derive(Debug, Clone)]
pub enum FillDef {
    Solid([f32; 4]),
    /// CSS 语义角度(0 = 向上,顺时针为正,与 ADR-0007 的 AI 语义相反 ——
    /// 此处存 CSS 语义,编码器已转换)。
    LinearGradient {
        angle_css: f64,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        /// 0..=1 相对宽高
        cx: f32,
        cy: f32,
        stops: Vec<GradientStop>,
    },
}

#[derive(Debug, Clone)]
pub struct BorderDef {
    pub width: f64,
    pub color: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawKind {
    VectorPath,
    Box,
    Text,
    Image,
    FrozenPlaceholder,
}

#[derive(Debug, Clone)]
pub struct DrawItem {
    /// 画板本地绝对坐标 [x, y, w, h]。
    pub rect: [f64; 4],
    pub ellipse: bool,
    /// 四角圆角 [tl, tr, br, bl]。
    pub radii: [f64; 4],
    pub fill: Option<FillDef>,
    pub border: Option<BorderDef>,
    pub opacity: f32,
    pub kind: DrawKind,
    /// 文本内容(近似渲染用,ADR-0017)。
    pub label: Option<TextHint>,
    /// 图像 src(解码用)。
    pub src: Option<String>,
    /// 旋转(CSS 顺时针度数,绕 rect 中心)。
    pub rot: f64,
    /// 矢量路径(P4 钢笔;画板本地坐标)。
    pub path: Option<vb_common::geom::BezPath>,
    /// 已解码位图(B3):由 attach_images 挂载;缺失时各端回退占位。
    pub image: Option<BitmapData>,
}

/// 已解码位图(RGBA8 直 alpha)。挂在 DrawItem 上供 GPU/SVG 消费;
/// CPU 端优先用它避免重复解码文件。
#[derive(Debug, Clone)]
pub struct BitmapData {
    pub width: u32,
    pub height: u32,
    pub rgba: std::sync::Arc<Vec<u8>>,
}

/// 解析器:src(相对路径)→ 位图。宿主可包一层缓存(GUI 逐帧编码,
/// 不缓存会每帧解码一次文件)。
pub type ImageLoader<'a> = &'a mut dyn FnMut(&str) -> Option<BitmapData>;

/// 把解析到的位图挂到 DrawList 的 Image 项上(就地修改)。
pub fn attach_images(list: &mut DrawList, loader: ImageLoader) {
    for item in &mut list.items {
        if item.kind != DrawKind::Image || item.image.is_some() {
            continue;
        }
        if let Some(src) = &item.src {
            item.image = loader(src);
        }
    }
}

/// 解析 transform 中的 rotate(θdeg) → 度(CSS 顺时针;引擎共用)。
pub fn parse_rotate_deg(v: &str) -> Option<f64> {
    let i = v.find("rotate(")? + "rotate(".len();
    let rest = &v[i..];
    let end = rest.find(')')?;
    rest[..end]
        .trim()
        .trim_end_matches("deg")
        .trim()
        .parse()
        .ok()
}

#[derive(Debug, Clone)]
pub struct TextHint {
    pub text: String,
    pub font_size: f64,
    pub color: [f32; 4],
    pub weight_bold: bool,
    /// CSS font-family 首族(C4 真文本管线)。
    pub font_family: String,
}

#[derive(Debug, Clone)]
pub struct DrawList {
    pub w: f64,
    pub h: f64,
    /// 画板背景色(透明导出时忽略)。
    pub background: [f32; 4],
    pub items: Vec<DrawItem>,
}

/// 解析 CSS 颜色值 → f32 rgba(支持单层 `var(--token)` 引用)。
fn parse_color_rgba_resolved(doc: &Document, v: &str) -> Option<[f32; 4]> {
    let resolved = resolve_var(doc, v);
    vb_common::color::parse_color(&resolved).map(|c| c.to_rgb_f32())
}

/// 解析 `var(--x)` → 令牌值(单层;链式引用逐层跟随,上限 8 层)。
pub fn resolve_var(doc: &Document, v: &str) -> String {
    let mut cur = v.trim().to_string();
    for _ in 0..8 {
        let t = cur.trim();
        if let Some(inner) = t.strip_prefix("var(--").and_then(|s| s.strip_suffix(')')) {
            let name = inner.split(',').next().unwrap_or(inner).trim();
            if let Some((_, val)) = doc.tokens.iter().find(|(n, _)| n == name) {
                cur = val.clone();
                continue;
            }
        }
        break;
    }
    cur
}

/// 解析 CSS 长度 → px。
fn px(v: &str) -> Option<f64> {
    vb_common::units::parse_px(v)
}

/// 解析 `linear-gradient(135deg, #a 0, #b 60%)` → (angle_css, stops)。
/// 方向关键字(`to right` / `to top right` 等)按 CSS css-images-3 换算:
/// 角关键字的方向垂直于目标角两邻角的连线,等价于 α = ±atan(h/w) 及其
/// 补角 —— 因此必须知道盒子尺寸,由 encode_node 传入。
pub fn parse_linear_gradient(
    doc: &Document,
    value: &str,
    w: f64,
    h: f64,
) -> Option<(f64, Vec<GradientStop>)> {
    let inner = value
        .trim()
        .strip_prefix("linear-gradient(")?
        .strip_suffix(')')?;
    let parts = vb_css_split_top_level(inner, ',');
    let mut angle = 180.0f64; // CSS 默认 to bottom
    let mut stops_raw: Vec<&str> = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let t = p.trim();
        if i == 0 {
            // 首段只认方向/角度;颜色(含命名色)一律当色标。
            // 此前 `to right` 因启发式误判被当色标吞掉,角度停在 180°。
            if let Some(d) = t
                .strip_suffix("deg")
                .and_then(|s| s.trim().parse::<f64>().ok())
            {
                angle = d;
                continue;
            }
            if let Some(d) = t
                .strip_suffix("turn")
                .and_then(|s| s.trim().parse::<f64>().ok())
            {
                angle = d * 360.0;
                continue;
            }
            if let Some(kw) = t.strip_prefix("to ") {
                // 未知方向:整条声明无效
                angle = resolve_direction(kw.trim(), w, h)?;
                continue;
            }
        }
        stops_raw.push(t);
    }
    let mut stops = Vec::new();
    let n = stops_raw.len().max(1);
    for (i, s) in stops_raw.iter().enumerate() {
        // "color pos" 或纯 "color"
        let mut segs = s.split_whitespace();
        let color_tok = segs.next().unwrap_or("");
        let Some(color) = parse_color_rgba_resolved(doc, color_tok) else {
            continue;
        };
        let pos = segs
            .next()
            .and_then(|p| {
                p.strip_suffix('%')
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v / 100.0)
            })
            .unwrap_or(if n <= 1 {
                0.0
            } else {
                i as f32 / (n - 1) as f32
            });
        stops.push(GradientStop { pos, color });
    }
    if stops.len() < 2 {
        return None;
    }
    Some((angle, stops))
}

/// `to <方向关键字>` → CSS 角度(0 = 向上,顺时针为正)。
/// 角关键字随盒子尺寸变化:方向垂直于目标角两邻角的连线。
fn resolve_direction(kw: &str, w: f64, h: f64) -> Option<f64> {
    let slope = (h.max(1e-6) / w.max(1e-6)).atan().to_degrees();
    match kw {
        "top" => Some(0.0),
        "right" => Some(90.0),
        "bottom" => Some(180.0),
        "left" => Some(270.0),
        "top right" | "right top" => Some(slope),
        "bottom right" | "right bottom" => Some(180.0 - slope),
        "bottom left" | "left bottom" => Some(180.0 + slope),
        "top left" | "left top" => Some(360.0 - slope),
        _ => None,
    }
}

/// 解析 `radial-gradient(circle at 35% 35%, #a 0, #b 70%)`。
pub fn parse_radial_gradient(doc: &Document, value: &str) -> Option<(f32, f32, Vec<GradientStop>)> {
    let inner = value
        .trim()
        .strip_prefix("radial-gradient(")?
        .strip_suffix(')')?;
    let parts = vb_css_split_top_level(inner, ',');
    let mut cx = 0.5f32;
    let mut cy = 0.5f32;
    let mut stops_raw: Vec<String> = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let t = p.trim().to_string();
        if i == 0 && !t.starts_with('#') && !t.contains("rgb") && t != "transparent" {
            // "circle at X% Y%" / "circle" / "ellipse ..."
            if let Some(at) = t.find("at") {
                let pos = t[at + 2..].trim();
                let mut it = pos.split_whitespace();
                if let Some(x) = it
                    .next()
                    .and_then(|v| v.strip_suffix('%'))
                    .and_then(|v| v.parse::<f32>().ok())
                {
                    cx = x / 100.0;
                }
                if let Some(y) = it
                    .next()
                    .and_then(|v| v.strip_suffix('%'))
                    .and_then(|v| v.parse::<f32>().ok())
                {
                    cy = y / 100.0;
                }
            }
            continue;
        }
        stops_raw.push(t);
    }
    let mut stops = Vec::new();
    let n = stops_raw.len().max(1);
    for (i, s) in stops_raw.iter().enumerate() {
        let mut segs = s.split_whitespace();
        let color_tok = segs.next().unwrap_or("");
        let Some(color) = parse_color_rgba_resolved(doc, color_tok) else {
            continue;
        };
        let pos = segs
            .next()
            .and_then(|p| {
                p.strip_suffix('%')
                    .and_then(|v| v.parse::<f32>().ok())
                    .map(|v| v / 100.0)
            })
            .unwrap_or(if n <= 1 {
                0.0
            } else {
                i as f32 / (n - 1) as f32
            });
        stops.push(GradientStop { pos, color });
    }
    if stops.is_empty() {
        return None;
    }
    Some((cx, cy, stops))
}

fn parse_fill(doc: &Document, node: &Node, w: f64, h: f64) -> Option<FillDef> {
    // background 简写:`background: linear-gradient(...)` 等价写在
    // background-image(CSS 简写展开);此前只认 background-image,
    // 简写形式的渐变整块丢失(画板/卡片背景变白)。
    let shorthand = node.style_get("background");
    let bg_image = node
        .style_get("background-image")
        .or_else(|| {
            shorthand.and_then(|s| {
                let t = s.trim();
                (t.starts_with("linear-gradient") || t.starts_with("radial-gradient"))
                    .then_some(t)
            })
        });
    if let Some(bgi) = bg_image {
        if bgi.starts_with("linear-gradient") {
            if let Some((angle, stops)) = parse_linear_gradient(doc, bgi, w, h) {
                return Some(FillDef::LinearGradient {
                    angle_css: angle,
                    stops,
                });
            }
        }
        if bgi.starts_with("radial-gradient") {
            if let Some((cx, cy, stops)) = parse_radial_gradient(doc, bgi) {
                return Some(FillDef::RadialGradient { cx, cy, stops });
            }
        }
        // url(...) 图像背景 / conic-gradient 等 v0.1 不绘制:
        // 回退到 background-color 兜底(CSS 中 image 盖在 color 之上),
        // 此前直接 None 把填充整个丢掉
    }
    if let Some(bg) = node.style_get("background-color") {
        if let Some(c) = parse_color_rgba_resolved(doc, bg) {
            return Some(FillDef::Solid(c));
        }
    }
    node.fill_color().map(|c| FillDef::Solid(c.to_rgb_f32()))
}

/// 解析 border-radius → [tl, tr, br, bl]。
/// 支持 1-4 值(CSS 简写展开)与百分比(按 min(w,h) 近似 —— CSS 语义是
/// 横向按宽、纵向按高的椭圆圆角,引擎 v0.2 只有单半径,取短边);
/// 单值 50%(或四角均 ≥ 半短边)时返回 INFINITY 哨兵 = 椭圆。
/// 椭圆斜杠语法 `a / b` 取斜杠前部分。
fn parse_radii(node: &Node, w: f64, h: f64) -> [f64; 4] {
    let Some(r) = node.style_get("border-radius") else {
        return [0.0; 4];
    };
    let r = r.split('/').next().unwrap_or(r).trim();
    let short = w.min(h);
    let mut vals: Vec<f64> = Vec::new();
    let mut all_pct_50 = true;
    for tok in r.split_whitespace() {
        if let Some(p) = tok.strip_suffix('%') {
            let f: f64 = p.trim().parse().unwrap_or(0.0);
            if f < 50.0 {
                all_pct_50 = false;
            }
            vals.push(f * short / 100.0);
        } else if let Some(v) = px(tok) {
            all_pct_50 = false;
            vals.push(v);
        } else {
            return [0.0; 4];
        }
    }
    if vals.is_empty() {
        return [0.0; 4];
    }
    let expanded = match vals.len() {
        1 => [vals[0], vals[0], vals[0], vals[0]],
        2 => [vals[0], vals[1], vals[0], vals[1]],
        3 => [vals[0], vals[1], vals[2], vals[1]],
        4 => [vals[0], vals[1], vals[2], vals[3]],
        _ => return [0.0; 4],
    };
    if matches!(expanded, [v, ..] if v.is_infinite())
        || (all_pct_50
            && expanded.iter().all(|&v| v >= short / 2.0)
            && short.is_finite()
            && short > 0.0)
    {
        return [f64::INFINITY; 4];
    }
    expanded
}

fn parse_border(doc: &Document, node: &Node) -> Option<BorderDef> {
    let width = node.style_get("border-width").and_then(px).or_else(|| {
        node.style_get("border")
            .and_then(|b| {
                b.split_whitespace()
                    .find(|t| t.ends_with("px") || t.parse::<f64>().is_ok())
            })
            .and_then(px)
    })?;
    // width 为 0 时不画
    if width <= 0.0 {
        return None;
    }
    let style = node
        .style_get("border-style")
        .or_else(|| node.style_get("border").map(|_| "solid"))?;
    if style != "solid" {
        // dashed/dotted v0.1 按实线近似
    }
    let color = node
        .style_get("border-color")
        .and_then(|v| parse_color_rgba_resolved(doc, v))
        .or_else(|| {
            node.style_get("border").and_then(|b| {
                b.split_whitespace()
                    .find_map(|t| parse_color_rgba_resolved(doc, t))
            })
        })
        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
    Some(BorderDef { width, color })
}

/// 编码单个画板为 DrawList(含画板底色矩形;GPU 画布据此画出画板矩形)。
pub fn encode_artboard(doc: &Document, artboard: NodeId) -> Result<DrawList, VbError> {
    encode_artboard_opts(doc, artboard, false)
}

/// 透明导出走 `transparent = true`:不铺底色矩形,CPU/SVG 导出与
/// 画布共用同一编码路径。
pub fn encode_artboard_opts(
    doc: &Document,
    artboard: NodeId,
    transparent: bool,
) -> Result<DrawList, VbError> {
    let ab = doc
        .nodes
        .get(artboard)
        .ok_or(VbError::NoSuchNode("artboard".into()))?;
    if !matches!(ab.kind, NodeKind::Artboard) {
        return Err(VbError::Unsupported("目标不是画板".into()));
    }
    let background = ab
        .style_get("background-color")
        .and_then(|v| parse_color_rgba_resolved(doc, v))
        .or_else(|| ab.fill_color().map(|c| c.to_rgb_f32()))
        .unwrap_or([1.0, 1.0, 1.0, 1.0]);
    let mut list = DrawList {
        w: ab.geom.w,
        h: ab.geom.h,
        background,
        items: Vec::new(),
    };
    if !transparent {
        // 画板自身背景走 parse_fill(渐变简写/背景图降级链路与普通节点一致);
        // 此前只认纯色,`background: linear-gradient(...)` 的画板整版白底。
        let bg_fill = parse_fill(doc, ab, ab.geom.w, ab.geom.h)
            .unwrap_or(FillDef::Solid(background));
        list.items.push(crate::DrawItem {
            rect: [0.0, 0.0, ab.geom.w, ab.geom.h],
            ellipse: false,
            radii: [0.0; 4],
            fill: Some(bg_fill),
            border: None,
            opacity: 1.0,
            kind: crate::DrawKind::Box,
            label: None,
            src: None,
            rot: 0.0,
            path: None,
            image: None,
        });
    }
    let children = ab.children.clone();
    for c in children {
        encode_node(doc, c, 0.0, 0.0, 1.0, &mut list);
    }
    Ok(list)
}

/// 从任意节点(编组)开始编码子树为 DrawList(无背景)。
/// 隔离模式用:内容坐标为该节点所处画板的本地坐标系(与 encode_artboard 同帧),
/// 宿主在遮罩层之上叠加本列表即可让隔离内容保持全亮。
pub fn encode_subtree(doc: &Document, root: NodeId) -> Result<DrawList, VbError> {
    let n = doc
        .nodes
        .get(root)
        .ok_or(VbError::NoSuchNode("subtree root".into()))?;
    let mut list = DrawList {
        w: n.geom.w,
        h: n.geom.h,
        background: [0.0, 0.0, 0.0, 0.0],
        items: Vec::new(),
    };
    encode_node(doc, root, 0.0, 0.0, 1.0, &mut list);
    Ok(list)
}

fn encode_node(
    doc: &Document,
    id: NodeId,
    off_x: f64,
    off_y: f64,
    opacity: f32,
    list: &mut DrawList,
) {
    let Some(node) = doc.nodes.get(id) else {
        return;
    };
    if node.hidden {
        return;
    }
    let op = opacity
        * node
            .style_get("opacity")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0);
    let x = off_x + node.geom.x;
    let y = off_y + node.geom.y;
    let w = node.geom.w;
    let h = node.geom.h;

    let is_container = node.kind.is_container();
    if !is_container || has_own_visual(node) {
        let radii = parse_radii(node, w, h);
        let ellipse = radii[0].is_infinite();
        let radii = if ellipse { [0.0; 4] } else { radii };
        list.items.push(DrawItem {
            rect: [x, y, w, h],
            ellipse,
            radii,
            fill: parse_fill(doc, node, w, h),
            border: parse_border(doc, node),
            opacity: op,
            kind: match &node.kind {
                NodeKind::Text { .. } => DrawKind::Text,
                NodeKind::Image { .. } => DrawKind::Image,
                NodeKind::Vector { .. } => DrawKind::VectorPath,
                NodeKind::Frozen { .. } => DrawKind::FrozenPlaceholder,
                _ => DrawKind::Box,
            },
            label: match &node.kind {
                NodeKind::Text { text, .. } => {
                    let fs = node.style_get("font-size").and_then(px).unwrap_or(16.0);
                    let bold = node
                        .style_get("font-weight")
                        .map(|w| {
                            w == "bold" || w == "700" || w == "600" || w == "800" || w == "900"
                        })
                        .unwrap_or(matches!(node.tag.as_str(), "h1" | "h2" | "h3"));
                    Some(TextHint {
                        text: text.clone(),
                        font_size: fs,
                        color: node
                            .style_get("color")
                            .and_then(|v| parse_color_rgba_resolved(doc, v))
                            .unwrap_or([0.1, 0.1, 0.1, 1.0]),
                        weight_bold: bold,
                        font_family: node
                            .style_get("font-family")
                            .unwrap_or_default()
                            .to_string(),
                    })
                }
                _ => None,
            },
            src: match &node.kind {
                NodeKind::Image { src } => Some(src.clone()),
                _ => None,
            },
            path: match &node.kind {
                NodeKind::Vector { path } => Some(path.clone()),
                _ => None,
            },
            rot: node
                .style_get("transform")
                .and_then(parse_rotate_deg)
                .unwrap_or(0.0),
            image: None,
        });
    }
    let children = node.children.clone();
    for c in children {
        encode_node(doc, c, x, y, op, list);
    }
}

/// 容器自身是否带视觉样式(填充/边框/圆角)。
fn has_own_visual(node: &Node) -> bool {
    node.fill_color().is_some()
        || node.style_get("background-image").is_some()
        || node.style_get("border").is_some()
        || node.style_get("border-width").is_some()
}
