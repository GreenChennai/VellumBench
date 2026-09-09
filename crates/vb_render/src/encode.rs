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
}

#[derive(Debug, Clone)]
pub struct TextHint {
    pub text: String,
    pub font_size: f64,
    pub color: [f32; 4],
    pub weight_bold: bool,
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
pub fn parse_linear_gradient(doc: &Document, value: &str) -> Option<(f64, Vec<GradientStop>)> {
    let inner = value
        .trim()
        .strip_prefix("linear-gradient(")?
        .strip_suffix(')')?;
    let parts = vb_css_split_top_level(inner, ',');
    let mut angle = 180.0f64; // CSS 默认 to bottom
    let mut stops_raw: Vec<&str> = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        let t = p.trim();
        if i == 0
            && (t.ends_with("deg")
                || t.ends_with("turn")
                || !t.starts_with('#') && !t.contains('('))
        {
            if let Some(d) = t.strip_suffix("deg") {
                angle = d.trim().parse().unwrap_or(180.0);
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

fn parse_fill(doc: &Document, node: &Node) -> Option<FillDef> {
    let bg_image = node.style_get("background-image");
    if let Some(bgi) = bg_image {
        if bgi.starts_with("linear-gradient") {
            if let Some((angle, stops)) = parse_linear_gradient(doc, bgi) {
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
        // url(...) 图像背景:v0.1 暂不绘制(见 warnings)
        return None;
    }
    if let Some(bg) = node.style_get("background-color") {
        if let Some(c) = parse_color_rgba_resolved(doc, bg) {
            return Some(FillDef::Solid(c));
        }
    }
    node.fill_color().map(|c| FillDef::Solid(c.to_rgb_f32()))
}

fn parse_radii(node: &Node) -> [f64; 4] {
    if let Some(r) = node.style_get("border-radius") {
        if r == "50%" {
            // 椭圆(圆)标记:由调用方按 w/h 处理
            return [f64::INFINITY; 4];
        }
        if let Some(v) = px(r) {
            return [v, v, v, v];
        }
    }
    [0.0; 4]
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

/// 编码单个画板为 DrawList。
pub fn encode_artboard(doc: &Document, artboard: NodeId) -> Result<DrawList, VbError> {
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
    // 画板自身底色(GPU 画布据此画出画板矩形)
    list.items.push(crate::DrawItem {
        rect: [0.0, 0.0, ab.geom.w, ab.geom.h],
        ellipse: false,
        radii: [0.0; 4],
        fill: Some(FillDef::Solid(background)),
        border: None,
        opacity: 1.0,
        kind: crate::DrawKind::Box,
        label: None,
        src: None,
    });
    let children = ab.children.clone();
    for c in children {
        encode_node(doc, c, 0.0, 0.0, 1.0, &mut list);
    }
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
        let radii = parse_radii(node);
        let ellipse = radii[0].is_infinite();
        let radii = if ellipse { [0.0; 4] } else { radii };
        list.items.push(DrawItem {
            rect: [x, y, w, h],
            ellipse,
            radii,
            fill: parse_fill(doc, node),
            border: parse_border(doc, node),
            opacity: op,
            kind: match &node.kind {
                NodeKind::Text { .. } => DrawKind::Text,
                NodeKind::Image { .. } => DrawKind::Image,
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
                    })
                }
                _ => None,
            },
            src: match &node.kind {
                NodeKind::Image { src } => Some(src.clone()),
                _ => None,
            },
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
