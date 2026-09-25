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

/// clip-path(L2 动画白名单;几何相对项矩形,px 已解析)。
#[derive(Debug, Clone)]
pub enum ClipDef {
    /// top right bottom left
    Inset(f64, f64, f64, f64),
    /// cx cy r
    Circle(f64, f64, f64),
    /// cx cy rx ry
    Ellipse(f64, f64, f64, f64),
    /// 多边形顶点(px)
    Polygon(Vec<(f64, f64)>),
}

/// filter(L3;CPU 栅格路径区域后处理)。
#[derive(Debug, Clone, Default)]
pub struct FilterDef {
    /// 高斯模糊半径 px。
    pub blur: f64,
    /// 亮度乘子(1 = 不变)。
    pub brightness: f64,
    /// 饱和度乘子(1 = 不变)。
    pub saturate: f64,
}

impl FilterDef {
    pub fn is_identity(&self) -> bool {
        self.blur <= 0.0
            && (self.brightness - 1.0).abs() < 1e-6
            && (self.saturate - 1.0).abs() < 1e-6
    }
}

/// 解析 clip-path 声明(相对项矩形;百分比按宽/高)。
pub fn parse_clip_path(v: &str, w: f64, h: f64) -> Option<ClipDef> {
    let t = v.trim();
    let args = |name: &str| -> Option<String> {
        t.strip_prefix(name)?
            .strip_suffix(')')
            .map(|s| s.to_string())
    };
    let _nums = |s: &str| -> Vec<f64> {
        s.split_whitespace()
            .filter_map(|tok| {
                if let Some(p) = tok.strip_suffix('%') {
                    p.parse::<f64>().ok()
                } else {
                    vb_common::units::parse_px(tok)
                }
            })
            .collect()
    };
    let pct = |tok: &str, base: f64| -> f64 {
        if let Some(p) = tok.strip_suffix('%') {
            p.parse::<f64>().unwrap_or(0.0) / 100.0 * base
        } else {
            vb_common::units::parse_px(tok).unwrap_or(0.0)
        }
    };
    if let Some(a) = args("inset(") {
        let toks: Vec<&str> = a.split_whitespace().collect();
        if toks.is_empty() {
            return None;
        }
        let one = |tok: &str, base: f64| pct(tok, base);
        let (top, right, bottom, left) = match toks.len() {
            1 => (toks[0], toks[0], toks[0], toks[0]),
            2 => (toks[0], toks[1], toks[0], toks[1]),
            3 => (toks[0], toks[1], toks[2], toks[1]),
            _ => (toks[0], toks[1], toks[2], toks[3]),
        };
        return Some(ClipDef::Inset(
            one(top, h),
            one(right, w),
            one(bottom, h),
            one(left, w),
        ));
    }
    if let Some(a) = args("circle(") {
        let mut it = a.split("at");
        let r_tok = it.next().unwrap_or("").trim();
        let r = if let Some(p) = r_tok.strip_suffix('%') {
            p.parse::<f64>().unwrap_or(50.0) / 100.0 * w.min(h)
        } else {
            vb_common::units::parse_px(r_tok).unwrap_or(w.min(h) / 2.0)
        };
        let pos = it.next().unwrap_or("50% 50%");
        let mut it2 = pos.split_whitespace();
        let cx = pct(it2.next().unwrap_or("50%"), w);
        let cy = pct(it2.next().unwrap_or("50%"), h);
        return Some(ClipDef::Circle(cx, cy, r));
    }
    if let Some(a) = args("ellipse(") {
        let mut it = a.split("at");
        let r_toks: Vec<&str> = it.next().unwrap_or("").split_whitespace().collect();
        let rx = pct(r_toks.first().copied().unwrap_or("50%"), w);
        let ry = pct(r_toks.get(1).copied().unwrap_or("50%"), h);
        let pos = it.next().unwrap_or("50% 50%");
        let mut it2 = pos.split_whitespace();
        let cx = pct(it2.next().unwrap_or("50%"), w);
        let cy = pct(it2.next().unwrap_or("50%"), h);
        return Some(ClipDef::Ellipse(cx, cy, rx, ry));
    }
    if let Some(a) = args("polygon(") {
        let mut pts = Vec::new();
        for pair in a.split(',') {
            let mut it = pair.split_whitespace();
            let x = pct(it.next()?, w);
            let y = pct(it.next()?, h);
            pts.push((x, y));
        }
        if pts.len() >= 3 {
            return Some(ClipDef::Polygon(pts));
        }
    }
    None
}

/// 解析 filter 声明(blur/brightness/saturate 组合)。
pub fn parse_filter(v: &str) -> Option<FilterDef> {
    let t = v.trim();
    if t.is_empty() || t == "none" {
        return None;
    }
    let mut f = FilterDef::default();
    let mut any = false;
    let bytes = t.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'-') {
                i += 1;
            }
            let name = &t[start..i];
            if i < bytes.len() && bytes[i] == b'(' {
                let pstart = i + 1;
                let mut j = pstart;
                while j < bytes.len() && bytes[j] != b')' {
                    j += 1;
                }
                let inner = &t[pstart..j.min(t.len())];
                let val = vb_common::units::parse_px(inner)
                    .unwrap_or_else(|| inner.trim().parse::<f64>().unwrap_or(1.0));
                match name {
                    "blur" => {
                        f.blur = val;
                        any = true;
                    }
                    "brightness" => {
                        f.brightness = val;
                        any = true;
                    }
                    "saturate" => {
                        f.saturate = val;
                        any = true;
                    }
                    _ => {}
                }
                i = j + 1;
                continue;
            }
        } else {
            i += 1;
        }
    }
    if any {
        Some(f)
    } else {
        None
    }
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
    /// 源节点稳定 id(动画轨道绑定用;合成背景为空)。
    pub sid: String,
    /// 画板本地绝对坐标 [x, y, w, h]。
    pub rect: [f64; 4],
    pub ellipse: bool,
    /// 四角圆角 [tl, tr, br, bl]。
    pub radii: [f64; 4],
    pub fill: Option<FillDef>,
    pub border: Option<BorderDef>,
    pub opacity: f32,
    pub layer: u32,
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
    /// clip-path(L2;动画逐帧可变)。
    pub clip: Option<ClipDef>,
    /// overflow 子树裁剪(05-2 / 09-B 剪切蒙版):祖先 `overflow:hidden`
    /// 容器求交后的裁剪矩形(画板本地 [x,y,w,h])。嵌套容器按交集折叠,
    /// 渲染端只需单层裁剪。
    pub overflow_clip: Option<[f64; 4]>,
    /// filter(L3;动画逐帧可变)。
    pub filter: Option<FilterDef>,
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
    /// CSS font-weight 数值(400/600/700…;选字与 SVG/PPTX 输出用)。
    pub weight: u16,
    /// CSS font-family 首族(C4 真文本管线)。
    pub font_family: String,
    /// 行高 px(0 = 未指定,渲染用 1.32×字号)。
    pub line_height: f64,
    /// 字距 px。
    pub letter_spacing: f64,
    /// 富文本段(字节区间样式覆盖;升序不重叠,区间外继承节点样式)。
    pub segments: Vec<TextSpanHint>,
}

/// 行内段样式(已解析为渲染就绪值)。
#[derive(Debug, Clone)]
pub struct TextSpanHint {
    pub start: usize,
    pub end: usize,
    pub color: Option<[f32; 4]>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub font_size: Option<f64>,
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
        // "color pos" 或纯 "color"。色标内函数(rgba(1, 2, 3, .5))经 CSS
        // 规范化后逗号带空格,必须用括号感知的顶层空格切分(此前
        // split_whitespace 在 "rgba(24," 处断开 → 整条渐变静默丢失)。
        let toks: Vec<String> = vb_css_split_top_level(s.trim(), ' ')
            .into_iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        let color_tok = toks.first().map(|t| t.as_str()).unwrap_or("");
        let Some(color) = parse_color_rgba_resolved(doc, color_tok) else {
            continue;
        };
        let pos = toks
            .get(1)
            .and_then(|p| parse_stop_pos(p))
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

/// 色标位置:`40%` → 0.4;规范化的裸 `0`(原 `0%`)→ 0.0;其余裸数字按
/// 0..1 区间接受(超界 clamp 在使用端)。
fn parse_stop_pos(p: &str) -> Option<f32> {
    if let Some(v) = p.strip_suffix('%') {
        return v.trim().parse::<f32>().ok().map(|v| v / 100.0);
    }
    p.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
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
    let shape_keywords = [
        "circle",
        "ellipse",
        "closest-side",
        "farthest-side",
        "closest-corner",
        "farthest-corner",
    ];
    for (i, p) in parts.iter().enumerate() {
        let t = p.trim().to_string();
        let looks_like_shape = i == 0
            && (shape_keywords.iter().any(|k| t.starts_with(k))
                || shape_keywords.iter().any(|k| t.contains(&format!(" {k}"))));
        if looks_like_shape {
            // "circle at X% Y%" / "ellipse ..." / "circle farthest-side"
            if let Some(at) = t.find(" at") {
                let pos = t[at + 3..].trim();
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
        // 与线性同款:括号感知切分(规范化后的 rgba 内含 ", ")
        let toks: Vec<String> = vb_css_split_top_level(s.trim(), ' ')
            .into_iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
        let color_tok = toks.first().map(|t| t.as_str()).unwrap_or("");
        let Some(color) = parse_color_rgba_resolved(doc, color_tok) else {
            continue;
        };
        let pos = toks
            .get(1)
            .and_then(|p| parse_stop_pos(p))
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
    let bg_image = node.style_get("background-image").or_else(|| {
        shorthand.and_then(|s| {
            let t = s.trim();
            (t.starts_with("linear-gradient") || t.starts_with("radial-gradient")).then_some(t)
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
    if let Some(bg) = node
        .style_get("background-color")
        .or_else(|| node.style_get("background"))
    {
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
    // 括号感知切分:空白 split 会把 "rgba(255, 207, 77, 1)" 切碎,
    // 颜色静默回退黑(21 篇 S1 实测)
    let border_tokens = |b: String| -> Vec<String> {
        vb_css::split_top_level(&b, ' ')
            .into_iter()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    };
    let width = node.style_get("border-width").and_then(px).or_else(|| {
        node.style_get("border")
            .map(|b| border_tokens(b.to_string()))
            .unwrap_or_default()
            .iter()
            .find(|t| t.ends_with("px") || t.parse::<f64>().is_ok())
            .and_then(|t| px(t))
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
                border_tokens(b.to_string())
                    .iter()
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
        let bg_fill =
            parse_fill(doc, ab, ab.geom.w, ab.geom.h).unwrap_or(FillDef::Solid(background));
        list.items.push(crate::DrawItem {
            layer: 0,
            sid: ab.sid.as_str().to_string(),
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
            clip: None,
            overflow_clip: None,
            filter: None,
        });
    }
    let children = ab.children.clone();
    // 图层上下文:画板直接子节点中的 Layer 节点按序 0..,其子孙继承该层号;
    // 非 Layer 直接子节点归 0 层(dompaint 的双图层结构由此驱动)
    let mut layer_seq = 0u32;
    for c in children {
        let layer = match doc.nodes.get(c).map(|n| &n.kind) {
            Some(NodeKind::Layer) => {
                let l = layer_seq;
                layer_seq += 1;
                l
            }
            _ => 0,
        };
        encode_node(doc, c, 0.0, 0.0, 1.0, layer, None, &mut list);
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
    encode_node(doc, root, 0.0, 0.0, 1.0, 0, None, &mut list);
    Ok(list)
}

#[allow(clippy::too_many_arguments)]
fn encode_node(
    doc: &Document,
    id: NodeId,
    off_x: f64,
    off_y: f64,
    opacity: f32,
    layer: u32,
    overflow_clip: Option<[f64; 4]>,
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

    // 图层标记与绘制序解耦(ADR-0021):节点自带 vb-layer 优先于结构层号
    let layer = node
        .style_get("vb-layer")
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(layer);
    let is_container = node.kind.is_container();
    if !is_container || has_own_visual(node) {
        let radii = parse_radii(node, w, h);
        let ellipse = radii[0].is_infinite();
        let radii = if ellipse { [0.0; 4] } else { radii };
        list.items.push(DrawItem {
            layer,
            sid: node.sid.as_str().to_string(),
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
                    // 文本节点(尤其 #text 分组节点)样式常为空:字体属性沿祖先链继承
                    let inherited = |prop: &str| -> Option<String> {
                        let mut cur = Some(id);
                        while let Some(cid) = cur {
                            let Some(n) = doc.nodes.get(cid) else { break };
                            if let Some(v) = n.style_get(prop) {
                                return Some(v.to_string());
                            }
                            cur = n.parent;
                        }
                        None
                    };
                    let fs = inherited("font-size").and_then(|v| px(&v)).unwrap_or(16.0);
                    let weight_raw = inherited("font-weight").unwrap_or_default();
                    let weight = weight_raw.parse::<u16>().unwrap_or(
                        if matches!(weight_raw.as_str(), "bold" | "bolder")
                            || matches!(node.tag.as_str(), "h1" | "h2" | "h3")
                        {
                            700
                        } else {
                            400
                        },
                    );
                    let bold = matches!(weight, 600..=900);
                    let line_height = match inherited("line-height") {
                        Some(v) => match v.trim().parse::<f64>() {
                            Ok(n) => n * fs,
                            Err(_) => px(&v).unwrap_or(0.0),
                        },
                        None => 0.0,
                    };
                    let letter_spacing = inherited("letter-spacing")
                        .and_then(|v| px(&v))
                        .unwrap_or(0.0);
                    let segments = match &node.kind {
                        NodeKind::Text { segments, .. } => segments
                            .iter()
                            .map(|seg| TextSpanHint {
                                start: seg.start,
                                end: seg.end,
                                color: seg
                                    .style
                                    .color
                                    .as_deref()
                                    .and_then(|c| parse_color_rgba_resolved(doc, c)),
                                bold: seg.style.bold,
                                italic: seg.style.italic,
                                font_size: seg.style.font_size,
                                font_family: seg.style.font_family.clone().unwrap_or_default(),
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    let _ = text;
                    Some(TextHint {
                        text: text.clone(),
                        font_size: fs,
                        color: inherited("color")
                            .and_then(|v| parse_color_rgba_resolved(doc, &v))
                            .unwrap_or([0.1, 0.1, 0.1, 1.0]),
                        weight_bold: bold,
                        weight,
                        font_family: inherited("font-family").unwrap_or_default(),
                        line_height,
                        letter_spacing,
                        segments,
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
            clip: node
                .style_get("clip-path")
                .and_then(|v| parse_clip_path(v, w, h)),
            overflow_clip,
            filter: node.style_get("filter").and_then(parse_filter),
        });
    }
    // overflow 子树裁剪(05-2 / 09-B 剪切蒙版):容器声明 `overflow:hidden`
    // 时,子孙项携带「本容器矩形 ∩ 祖先裁剪」的折叠矩形 —— CSS 语义里
    // overflow 裁剪的是**子孙**,容器自身背景不受影响。
    let child_clip = if node
        .style_get("overflow")
        .map(|v| v.trim() == "hidden")
        .unwrap_or(false)
        && !node.children.is_empty()
    {
        let own = [x, y, w, h];
        Some(match overflow_clip {
            Some([ax, ay, aw, ah]) => {
                let x0 = own[0].max(ax);
                let y0 = own[1].max(ay);
                let x1 = (own[0] + own[2]).min(ax + aw);
                let y1 = (own[1] + own[3]).min(ay + ah);
                [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)]
            }
            None => own,
        })
    } else {
        overflow_clip
    };
    let children = node.children.clone();
    for c in children {
        encode_node(doc, c, x, y, op, layer, child_clip, list);
    }
}

/// 容器自身是否带视觉样式(填充/边框/圆角)。
fn has_own_visual(node: &Node) -> bool {
    node.fill_color().is_some()
        || node.style_get("background-image").is_some()
        || node.style_get("border").is_some()
        || node.style_get("border-width").is_some()
        // background 简写任意形式都算自身视觉(渐变/纯色/var 引用)。
        // 此前只认渐变前缀,`background:var(--panel)` / `background:#fff`
        // 的容器被判"无自绘"整棵跳过 → 面板背景全丢(部署 GEO 实测)
        || node
            .style_get("background")
            .map(|v| !v.trim().is_empty() && v.trim() != "none")
            .unwrap_or(false)
}
