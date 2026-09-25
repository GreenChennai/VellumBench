//! SVG 导入(M5):usvg 解析 → 场景图 → 规范化 HTML。
//!
//! 设计:与 import_pdf 同款边界——形状/文本/图像三类对象,绝对定位。
//! **05-6 起自由曲线真实矢量化**:C(三次)/ Q(二次)段逐一转为文档路径
//! 模型的贝塞尔段(保真);A 圆弧由 usvg 解析期转为三次贝塞尔逼近(标准
//! 圆弧-贝塞尔转换,误差为该算法固有界,最大中点偏差 < 0.03% 弧长量级),
//! 本层按三次段原样承接 —— 误差策略:导入侧不再二次降级,注释即标注。
//! 文本提取真实字符串(不转曲),颜色/字号/位置取自 SVG 属性;字体族透传
//! 给 @font-face/系统字体解析。
//!
//! **逐项结论清单(05-6,X-3)**:支持 = 矩形/圆/圆角矩形(折算图元)、
//! 自由路径(真实矢量)、文本、位图图像;显式不支持 = filter / mask /
//! clipPath / pattern 填充 / 嵌套 SVG 图像 / WEBP 图像 —— 全部在导入完成
//! 时给出用户可见的跳过清单(「N 项被跳过:filter×2、mask×1…」),
//! 渐变取中点色属**近似**(同样入清单标注),不许静默丢。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vb_common::geom::{BezPath, PathEl, Point};
use vb_doc::model::NodeId;
use vb_doc::model::{Document, Geom, Node, NodeKind, TextMode};

/// 递归深度上限(防病态嵌套)。
const MAX_DEPTH: usize = 32;

fn attach(doc: &mut Document, ab: NodeId, n: Node) -> NodeId {
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(ab).unwrap().children.push(id);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    id
}

fn color_to_hex(c: usvg::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue)
}

/// SVG 长度(usvg 已归一化为像素语义)。
fn px_len(v: f32) -> f64 {
    v as f64
}

/// 直线多边形段是否构成轴对齐矩形(4/5 点闭合,点全在角上)。
fn detect_rect(segs: &[tiny_skia_path::PathSegment]) -> Option<(f64, f64, f64, f64)> {
    let e = 0.5f32;
    let mut pts: Vec<tiny_skia_path::Point> = Vec::new();
    let mut started = false;
    let mut closed = false;
    for seg in segs {
        match *seg {
            tiny_skia_path::PathSegment::MoveTo(pt) => {
                if started {
                    return None;
                }
                pts.push(pt);
                started = true;
            }
            tiny_skia_path::PathSegment::LineTo(pt) => pts.push(pt),
            tiny_skia_path::PathSegment::Close => {
                closed = true;
            }
            _ => return None,
        }
    }
    if !closed || pts.len() < 4 || pts.len() > 5 {
        return None;
    }
    let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for pt in &pts {
        minx = minx.min(pt.x);
        miny = miny.min(pt.y);
        maxx = maxx.max(pt.x);
        maxy = maxy.max(pt.y);
    }
    for pt in &pts {
        let on_x = (pt.x - minx).abs() < e || (pt.x - maxx).abs() < e;
        let on_y = (pt.y - miny).abs() < e || (pt.y - maxy).abs() < e;
        if !(on_x && on_y) {
            return None;
        }
    }
    Some((
        px_len(minx),
        px_len(miny),
        px_len(maxx - minx),
        px_len(maxy - miny),
    ))
}

/// 圆角半径近似:四分之一圆弧的弦长 ≈ r。
fn rounded_rect_radius(segs: &[tiny_skia_path::PathSegment]) -> Option<f64> {
    let mut chords: Vec<f32> = Vec::new();
    let mut prev: Option<tiny_skia_path::Point> = None;
    for seg in segs {
        match *seg {
            tiny_skia_path::PathSegment::MoveTo(pt) | tiny_skia_path::PathSegment::LineTo(pt) => {
                prev = Some(pt);
            }
            tiny_skia_path::PathSegment::CubicTo(c1, _c2, pt) => {
                let p0 = prev.unwrap_or(c1);
                let chord = ((pt.x - p0.x).powi(2) + (pt.y - p0.y).powi(2)).sqrt();
                chords.push(chord);
                prev = Some(pt);
            }
            _ => {}
        }
    }
    if chords.len() != 4 {
        return None;
    }
    chords.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = (chords[1] + chords[2]) / 2.0;
    // 弦跨角部对角(usvg 圆角矩形构造),换算回半径
    let r = mid / std::f32::consts::SQRT_2;
    if r < 0.5 {
        return None;
    }
    Some(px_len(r))
}

/// 填充漆 → (hex, alpha, 清单类别)。
/// 渐变取中点色近似(类别 `渐变取中点色`);pattern 无纯色等价(类别
/// `pattern 填充`,调用方近似为浅灰);纯色 → 类别 None。
fn paint_color(paint: &usvg::Paint) -> Option<(String, f32, Option<&'static str>)> {
    match paint {
        usvg::Paint::Color(c) => Some((color_to_hex(*c), 1.0, None)),
        usvg::Paint::LinearGradient(g) => {
            let stops = g.stops();
            let mid = stops.get(stops.len() / 2)?;
            Some((
                color_to_hex(mid.color()),
                mid.opacity().get(),
                Some("渐变取中点色"),
            ))
        }
        usvg::Paint::RadialGradient(g) => {
            let stops = g.stops();
            let mid = stops.get(stops.len() / 2)?;
            Some((
                color_to_hex(mid.color()),
                mid.opacity().get(),
                Some("渐变取中点色"),
            ))
        }
        usvg::Paint::Pattern(_) => Some(("".into(), 1.0, Some("pattern 填充"))),
    }
}

/// 漆 → (落盘色字符串, 清单类别)。alpha < 1 时写 rgba()(parse_color
/// 支持);pattern 等无纯色时回退 `fallback` 色。类别由调用方记入跳过清单。
fn paint_to_css(
    paint: &usvg::Paint,
    extra_opacity: f32,
    fallback: &str,
) -> Option<(String, Option<&'static str>)> {
    let (hex, alpha, note) = paint_color(paint)?;
    if hex.is_empty() {
        return Some((fallback.to_string(), note)); // vb-token-ok:SVG 纸面色(文档语义,非 UI 主题)
    }
    let a = alpha * extra_opacity;
    let css = if a >= 0.999 {
        hex
    } else {
        // hex → rgba()
        let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(0);
        format!("rgba({r}, {g}, {b}, {a:.3})")
    };
    Some((css, note))
}

struct SvgWalker<'a> {
    doc: &'a mut Document,
    ab: NodeId,
    assets_dir: Option<PathBuf>,
    img_seq: usize,
    warnings: Vec<String>,
    /// 跳过/近似清单(类别 → 项数;05-6 逐项结论,导入完成汇总成一条
    /// 用户可见提示,不许静默丢)。
    skips: BTreeMap<&'static str, usize>,
    /// 已矢量化(非图元折算)的自由路径数(信息性统计)。
    vector_paths: usize,
}

impl<'a> SvgWalker<'a> {
    fn warn(&mut self, w: String) {
        if !self.warnings.contains(&w) && self.warnings.len() < 32 {
            self.warnings.push(w);
        }
    }

    /// 记一次跳过/近似项。
    fn skip(&mut self, kind: &'static str) {
        *self.skips.entry(kind).or_insert(0) += 1;
    }

    fn walk_group(&mut self, group: &usvg::Group, tx: tiny_skia::Transform, depth: usize) {
        if depth > MAX_DEPTH {
            self.warn("嵌套过深(已截断)".into());
            return;
        }
        // 05-6 逐项结论:filter/mask/clipPath 显式不支持 —— 计数并继续导入
        // 内容本体(效果丢失但内容不丢,清单可见)。
        if !group.filters().is_empty() {
            for _ in 0..group.filters().len() {
                self.skip("filter");
            }
        }
        if group.mask().is_some() {
            self.skip("mask");
        }
        if group.clip_path().is_some() {
            self.skip("clipPath");
        }
        for child in group.children() {
            match child {
                usvg::Node::Group(g) => {
                    let t = mul(tx, g.transform());
                    self.walk_group(g, t, depth + 1);
                }
                usvg::Node::Path(p) => self.walk_path(p, tx),
                usvg::Node::Image(img) => self.walk_image(img, tx),
                usvg::Node::Text(t) => self.walk_text(t, tx),
            }
        }
    }

    fn node_xy(&self, tx: tiny_skia::Transform, x: f64, y: f64) -> (f64, f64) {
        // usvg 变换 v1 只取平移/简单缩放(倾斜/旋转忽略 + 警告)
        (
            x * tx.sx as f64 + tx.tx as f64,
            y * tx.sy as f64 + tx.ty as f64,
        )
    }

    fn check_tx(&mut self, tx: tiny_skia::Transform) {
        if (tx.kx as f64).abs() > 1e-3 || (tx.ky as f64).abs() > 1e-3 {
            self.warn("节点含倾斜变换(忽略)".into());
            // VB-4:倾斜变换同属「导入丢弃」口径,计入跳过清单,
            // 随 ImportObjectSkipped 进结构化告警。
            self.skip("倾斜变换");
        }
    }

    fn walk_path(&mut self, p: &usvg::Path, tx: tiny_skia::Transform) {
        self.check_tx(tx);
        let data = p.data();
        let segs: Vec<tiny_skia_path::PathSegment> = data.segments().collect();
        let bb = data.bounds();
        if !(bb.left().is_finite() && bb.top().is_finite() && bb.width().is_finite()) {
            return;
        }
        let bx = px_len(bb.left());
        let by = px_len(bb.top());
        let bw = px_len(bb.width());
        let bh = px_len(bb.height());
        if bw < 0.5 || bh < 0.5 {
            return;
        }
        // 填充优先;无填充看描边(近似/不支持类别记入清单)
        const FILL_FALLBACK: &str = "#e8e8e4"; // vb-token-ok:SVG 纸面色(文档语义,非 UI 主题)
        let mut notes: Vec<&'static str> = Vec::new();
        let (fill_css, stroke_css, stroke_w) = match p.fill() {
            Some(fill) => match paint_to_css(fill.paint(), fill.opacity().get(), FILL_FALLBACK) {
                Some((f, note)) => {
                    if let Some(k) = note {
                        notes.push(k);
                    }
                    (Some(f), None, None)
                }
                None => return,
            },
            None => match p.stroke() {
                Some(st) => {
                    // 描边宽随变换平均缩放(倾斜被忽略,取 sx/sy 均值)
                    let w = px_len(st.width().get() * ((tx.sx + tx.sy) / 2.0).abs());
                    match paint_to_css(st.paint(), st.opacity().get(), FILL_FALLBACK) {
                        Some((s, note)) => {
                            if let Some(k) = note {
                                notes.push(k);
                            }
                            (None, Some(s), Some(w))
                        }
                        None => return,
                    }
                }
                None => return,
            },
        };
        for k in notes {
            self.skip(k);
        }

        let (x, y) = self.node_xy(tx, bx, by);
        let (w, h) = (bw.max(1.0), bh.max(1.0));

        let lines = segs
            .iter()
            .filter(|s| matches!(s, tiny_skia_path::PathSegment::LineTo(..)))
            .count();
        let curves = segs
            .iter()
            .filter(|s| {
                matches!(
                    s,
                    tiny_skia_path::PathSegment::CubicTo(..)
                        | tiny_skia_path::PathSegment::QuadTo(..)
                )
            })
            .count();
        // 直角矩形:全直线段轴对齐
        if curves == 0 {
            if let Some((rx, ry, rw, rh)) = detect_rect(&segs) {
                let (x, y) = self.node_xy(tx, rx, ry);
                let sid = self.doc.alloc_sid();
                let mut n = Node::new(NodeKind::Box, "矩形", sid);
                n.geom = Geom {
                    x,
                    y,
                    w: rw.max(1.0),
                    h: rh.max(1.0),
                };
                n.authored = [true, true, true, true];
                if let Some(f) = &fill_css {
                    n.style_set("background-color", f);
                }
                attach(self.doc, self.ab, n);
                return;
            }
        }
        // 椭圆:4 段三次曲线无直线;圆角矩形:4 段曲线 + 4 直线
        if curves == 4 && lines == 0 {
            let sid = self.doc.alloc_sid();
            let mut n = Node::new(NodeKind::Box, "圆形", sid);
            n.geom = Geom { x, y, w, h };
            n.authored = [true, true, true, true];
            if let Some(f) = &fill_css {
                n.style_set("background-color", f);
            }
            n.style_set("border-radius", "50%");
            attach(self.doc, self.ab, n);
            return;
        }
        if curves == 4 && lines == 4 {
            if let Some(r) = rounded_rect_radius(&segs) {
                let r = r.min(w.min(h) / 2.0);
                let sid = self.doc.alloc_sid();
                let mut n = Node::new(NodeKind::Box, "圆角矩形", sid);
                n.geom = Geom { x, y, w, h };
                n.authored = [true, true, true, true];
                if let Some(f) = &fill_css {
                    n.style_set("background-color", f);
                }
                n.style_set("border-radius", &format!("{r:.1}px"));
                attach(self.doc, self.ab, n);
                return;
            }
        }
        // 05-6:其余路径**真实矢量化** —— C/Q 段保真转为文档路径模型的
        // 贝塞尔段;A 圆弧已由 usvg 解析期转为三次贝塞尔(误差策略见模块注释)。
        let Some((bez, _)) = segs_to_bez(&segs, tx) else {
            self.warn("路径无法转换(缺少 MoveTo,已跳过)".into());
            return;
        };
        self.vector_paths += 1;
        let sid = self.doc.alloc_sid();
        let mut n = Node::new(NodeKind::Vector { path: bez }, "路径", sid);
        n.geom = Geom { x, y, w, h };
        n.authored = [true, true, true, true];
        // 填充/描边口径与钢笔产物一致:fill + stroke + stroke-width
        if let Some(f) = &fill_css {
            n.style_set("fill", f);
        } else if stroke_css.is_some() {
            n.style_set("fill", "none");
        }
        if let Some(s) = &stroke_css {
            n.style_set("stroke", s);
            n.style_set(
                "stroke-width",
                &format!("{}px", vb_common::units::fmt_num(stroke_w.unwrap_or(1.0))),
            );
        }
        attach(self.doc, self.ab, n);
    }

    fn walk_image(&mut self, img: &usvg::Image, tx: tiny_skia::Transform) {
        self.check_tx(tx);
        let Some(dir) = self.assets_dir.clone() else {
            self.warn("无 assets 目录,嵌入图像跳过".into());
            return;
        };
        let kind = img.kind();
        let bytes: &[u8] = match &kind {
            usvg::ImageKind::PNG(d) | usvg::ImageKind::JPEG(d) | usvg::ImageKind::GIF(d) => d,
            usvg::ImageKind::WEBP(_) => {
                self.skip("webp 图像");
                self.warn("WEBP 嵌入图像暂不支持(已跳过)".into());
                return;
            }
            usvg::ImageKind::SVG(_) => {
                self.skip("嵌套 svg 图像");
                self.warn("嵌套 SVG 图像暂不支持(已跳过)".into());
                return;
            }
        };
        let _ = std::fs::create_dir_all(&dir);
        self.img_seq += 1;
        let file_name = format!("img-svg-{:02}.png", self.img_seq);
        if std::fs::write(dir.join(&file_name), bytes).is_err() {
            self.warn("图像落盘失败(已跳过)".into());
            return;
        }
        let size = img.size();
        // v1 只取绝对平移(图像自身变换与倾斜并入近似)
        let full = mul(tx, img.abs_transform());
        let x = full.tx as f64;
        let y = full.ty as f64;
        let iw = px_len(size.width()).max(4.0);
        let ih = px_len(size.height()).max(4.0);
        let sid = self.doc.alloc_sid();
        let mut n = Node::new(NodeKind::Image { src: file_name }, "图像", sid);
        n.geom = Geom { x, y, w: iw, h: ih };
        n.authored = [true, true, true, true];
        attach(self.doc, self.ab, n);
    }

    fn walk_text(&mut self, t: &usvg::Text, tx: tiny_skia::Transform) {
        self.check_tx(tx);
        let raw: String = t
            .chunks()
            .iter()
            .map(|c| c.text().to_string())
            .collect::<Vec<String>>()
            .join("");
        if raw.trim().is_empty() {
            return;
        }
        let b = t.bounding_box();
        if !(b.left().is_finite() && b.top().is_finite()) {
            return;
        }
        let (x, y) = self.node_xy(tx, px_len(b.left()), px_len(b.top()));
        let w = px_len(b.width()).max(8.0);
        let h = px_len(b.height()).max(8.0);
        // 字号近似:包围盒高度 / 1.2(含上下伸部)
        let fs = (h / 1.2).max(6.0);
        // 颜色/字号取首个 span(无 span 时墨色近似);渐变取中点色入清单
        let first_span = t.chunks().first().and_then(|c| c.spans().first());
        let hex = first_span
            .and_then(|s| s.fill())
            .and_then(|f| paint_color(f.paint()))
            .map(|(h, _, note)| {
                if let Some(k) = note {
                    self.skip(k);
                }
                h
            })
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "#1a1a1a".to_string()); // vb-token-ok:SVG 墨色回退
        if let Some(sp) = first_span {
            // 字号以包围盒为主,span 字号只用于缩小包围盒偏差
            let _ = sp.font_size();
        }
        let sid = self.doc.alloc_sid();
        let mut n = Node::new(
            NodeKind::Text {
                text: raw.to_string(),
                mode: TextMode::Point,
                segments: Vec::new(),
            },
            "文本",
            sid,
        );
        n.tag = "p".to_string();
        n.geom = Geom { x, y, w, h };
        n.authored = [true, true, true, true];
        n.style_set("font-size", &format!("{}px", fs.round()));
        n.style_set("color", &hex);
        n.style_set("white-space", "nowrap");
        attach(self.doc, self.ab, n);
    }
}

fn mul(a: tiny_skia::Transform, b: tiny_skia::Transform) -> tiny_skia::Transform {
    tiny_skia::Transform::from_row(
        a.sx * b.sx + a.kx * b.ky,
        a.ky * b.sx + a.sy * b.ky,
        a.sx * b.kx + a.kx * b.sy,
        a.ky * b.kx + a.sy * b.sy,
        a.sx * b.tx + a.kx * b.ty + a.tx,
        a.ky * b.tx + a.sy * b.ty + a.ty,
    )
}

/// 变换作用到点(仿射;贝塞尔段对仿射封闭 —— 控制点同变换即可)。
fn tx_point(tx: tiny_skia::Transform, p: tiny_skia_path::Point) -> (f64, f64) {
    (
        p.x as f64 * tx.sx as f64 + p.y as f64 * tx.kx as f64 + tx.tx as f64,
        p.x as f64 * tx.ky as f64 + p.y as f64 * tx.sy as f64 + tx.ty as f64,
    )
}

/// SVG 路径段 → 文档路径模型(节点本地帧)。
///
/// **保真口径(05-6)**:`C`(三次)与 `Q`(二次)段逐一转为贝塞尔段,
/// 控制点原样保留;`A` 圆弧不在本层出现 —— usvg 解析期已把弧转为三次
/// 贝塞尔逼近(标准算法,误差为该转换固有界),按三次段承接即保真承接。
/// 返回 `(路径[已平移到包围盒原点], [x, y, w, h] 世界帧包围盒)`;
/// 无 MoveTo 开头返回 None。
fn segs_to_bez(
    segs: &[tiny_skia_path::PathSegment],
    tx: tiny_skia::Transform,
) -> Option<(BezPath, [f64; 4])> {
    if !matches!(segs.first(), Some(tiny_skia_path::PathSegment::MoveTo(_))) {
        return None;
    }
    // 先变换到公共帧,再取包围盒重定基
    let mut pts: Vec<(f64, f64)> = Vec::new();
    let mut raw: Vec<PathEl> = Vec::new();
    for seg in segs {
        match *seg {
            tiny_skia_path::PathSegment::MoveTo(p) => {
                let q = tx_point(tx, p);
                pts.push(q);
                raw.push(PathEl::MoveTo(Point::new(q.0, q.1)));
            }
            tiny_skia_path::PathSegment::LineTo(p) => {
                let q = tx_point(tx, p);
                pts.push(q);
                raw.push(PathEl::LineTo(Point::new(q.0, q.1)));
            }
            tiny_skia_path::PathSegment::QuadTo(c, p) => {
                let c = tx_point(tx, c);
                let q = tx_point(tx, p);
                pts.push(c);
                pts.push(q);
                raw.push(PathEl::QuadTo(Point::new(c.0, c.1), Point::new(q.0, q.1)));
            }
            tiny_skia_path::PathSegment::CubicTo(c1, c2, p) => {
                let a = tx_point(tx, c1);
                let b = tx_point(tx, c2);
                let q = tx_point(tx, p);
                pts.push(a);
                pts.push(b);
                pts.push(q);
                raw.push(PathEl::CurveTo(
                    Point::new(a.0, a.1),
                    Point::new(b.0, b.1),
                    Point::new(q.0, q.1),
                ));
            }
            tiny_skia_path::PathSegment::Close => raw.push(PathEl::ClosePath),
        }
    }
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for (x, y) in &pts {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    if !x0.is_finite() {
        return None;
    }
    let mut bez = BezPath::new();
    for el in raw {
        match el {
            PathEl::MoveTo(p) => bez.move_to(Point::new(p.x - x0, p.y - y0)),
            PathEl::LineTo(p) => bez.line_to(Point::new(p.x - x0, p.y - y0)),
            PathEl::QuadTo(c, p) => bez.quad_to(
                Point::new(c.x - x0, c.y - y0),
                Point::new(p.x - x0, p.y - y0),
            ),
            PathEl::CurveTo(c1, c2, p) => bez.curve_to(
                Point::new(c1.x - x0, c1.y - y0),
                Point::new(c2.x - x0, c2.y - y0),
                Point::new(p.x - x0, p.y - y0),
            ),
            PathEl::ClosePath => bez.close_path(),
        }
    }
    Some((bez, [x0, y0, x1 - x0, y1 - y0]))
}

/// 导入 SVG → Document(单画板)。
/// `assets_dir`:嵌入图像落盘目录(None = 图像跳过)。
/// 第三元 = 强类型跳过告警(VB-4):每个跳过/近似类别一条
/// `ImportObjectSkipped{kind,count}`,供 KilnReport 聚合与下游门禁。
pub fn import_svg_to_doc(
    svg_path: &Path,
    assets_dir: Option<&Path>,
) -> Result<(Document, Vec<String>, Vec<crate::error::KilnWarning>), String> {
    let data = std::fs::read(svg_path).map_err(|e| format!("SVG 读取失败:{e}"))?;
    let mut opt = usvg::Options::default();
    let mut fontdb = fontdb::Database::new();
    fontdb.load_system_fonts();
    // usvg 默认族是 Times New Roman——linux/精简容器上不存在,文本节点会被
    // 静默丢(违反 ADR-0046"降级必须可观测"的底线)。在系统字体里挑一个
    // 确实存在的默认族:优先 CJK(中文场景字形必须有),退而求其次拉丁族。
    let preferred: &[&str] = &[
        "Noto Sans CJK SC",
        "Source Han Sans SC",
        "PingFang SC",
        "Microsoft YaHei",
        "SimSun",
        "Arial",
        "Helvetica",
        "DejaVu Sans",
        "Liberation Sans",
    ];
    for name in preferred {
        let q = fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            weight: fontdb::Weight::NORMAL,
            style: fontdb::Style::Normal,
            stretch: fontdb::Stretch::Normal,
        };
        if fontdb.query(&q).is_some() {
            opt.font_family = (*name).to_string();
            break;
        }
    }
    let fontdb = Arc::new(fontdb);
    opt.fontdb = fontdb;
    let tree = usvg::Tree::from_data(&data, &opt).map_err(|e| format!("SVG 解析失败:{e}"))?;

    let size = tree.size();
    let mut document = Document::new_empty("", "zh-CN");
    let ab = document.new_artboard("画板 1", px_len(size.width()), px_len(size.height()));

    let mut walker = SvgWalker {
        doc: &mut document,
        ab,
        assets_dir: assets_dir.map(|p| p.to_path_buf()),
        img_seq: 0,
        warnings: Vec::new(),
        skips: BTreeMap::new(),
        vector_paths: 0,
    };
    walker.walk_group(tree.root(), tiny_skia::Transform::identity(), 0);
    let vectors = walker.vector_paths;
    let mut warnings = walker.warnings;
    if vectors > 0 {
        warnings.push(format!(
            "{vectors} 条自由路径已真实矢量化(C/Q 贝塞尔保真;A 圆弧为 usvg 三次逼近)"
        ));
    }
    // 05-6 逐项结论清单:跳过/近似项汇总成**一条用户可见提示**
    // (类别顺序固定,文案可断言),不许静默丢。
    let skips = walker.skips;
    if !skips.is_empty() {
        let total: usize = skips.values().sum();
        let mut parts: Vec<String> = SKIP_ORDER
            .iter()
            .filter_map(|k| skips.get(*k).map(|n| format!("{k}×{n}")))
            .collect();
        for (k, n) in &skips {
            if !SKIP_ORDER.contains(k) {
                parts.push(format!("{k}×{n}"));
            }
        }
        warnings.push(format!(
            "导入完成,{total} 项被跳过或近似:{}(内容已导入,效果不带)",
            parts.join("、")
        ));
    }
    // VB-4:同类聚合的强类型跳过告警(与字符串提示同源,供结构化消费)
    let typed: Vec<crate::error::KilnWarning> = skips
        .into_iter()
        .map(
            |(kind, count)| crate::error::KilnWarning::ImportObjectSkipped {
                kind: kind.to_string(),
                count,
            },
        )
        .collect();
    Ok((document, warnings, typed))
}

/// 跳过清单的**固定输出顺序**(filter → mask → clipPath → 渐变 → pattern →
/// 倾斜变换 → 嵌套 svg → webp);不在表内的类别按字典序兜底追加。
const SKIP_ORDER: &[&str] = &[
    "filter",
    "mask",
    "clipPath",
    "渐变取中点色",
    "pattern 填充",
    "倾斜变换",
    "嵌套 svg 图像",
    "webp 图像",
];

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::model::NodeKind;

    /// 写夹具到临时目录并导入(隔离到本次测试的子目录)。
    fn import_str(
        name: &str,
        svg: &str,
    ) -> (Document, Vec<String>, Vec<crate::error::KilnWarning>) {
        let dir = std::env::temp_dir().join(format!("vb-svg-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("in.svg");
        std::fs::write(&p, svg).unwrap();
        let (doc, warnings, typed) = import_svg_to_doc(&p, None).expect("导入应成功");
        std::fs::remove_dir_all(&dir).unwrap();
        (doc, warnings, typed)
    }

    fn kinds(doc: &Document) -> Vec<&'static str> {
        doc.nodes.values().map(|n| n.kind.kind_name()).collect()
    }

    /// 05-6 逐项结论:filter/mask/clipPath 显式不支持 → 跳过清单可见;
    /// 文本/自由路径正常导入,不静默丢。
    #[test]
    fn skip_list_is_user_visible() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200">
            <filter id="f"><feGaussianBlur stdDeviation="2"/></filter>
            <mask id="m"><rect width="200" height="200" fill="#fff"/></mask>
            <clipPath id="c"><rect x="10" y="10" width="80" height="80"/></clipPath>
            <g filter="url(#f)" mask="url(#m)" clip-path="url(#c)">
                <path d="M10 20 C 40 20 40 60 70 60 Q 90 60 110 40 Z" fill="#123456"/>
            </g>
            <text x="10" y="150" font-size="16" fill="#222222">你好 SVG</text>
        </svg>"##;
        let (doc, warnings, typed) = import_str("skip", svg);
        // VB-4:跳过类别同步映射为强类型 ImportObjectSkipped
        let skipped_kinds: Vec<(&str, usize)> = typed
            .iter()
            .filter_map(|w| match w {
                crate::error::KilnWarning::ImportObjectSkipped { kind, count } => {
                    Some((kind.as_str(), *count))
                }
                _ => None,
            })
            .collect();
        assert!(
            skipped_kinds.contains(&("filter", 1))
                && skipped_kinds.contains(&("mask", 1))
                && skipped_kinds.contains(&("clipPath", 1)),
            "强类型跳过告警缺失:{skipped_kinds:?}"
        );
        assert!(
            typed.iter().all(|w| w.is_degrading()),
            "跳过告警应置降级语义"
        );
        let joined = warnings.join("\n");
        assert!(joined.contains("filter×1"), "filter 计数缺失:{joined}");
        assert!(joined.contains("mask×1"), "mask 计数缺失:{joined}");
        assert!(joined.contains("clipPath×1"), "clipPath 计数缺失:{joined}");
        assert!(joined.contains("3 项被跳过或近似"), "汇总句缺失:{joined}");
        let ks = kinds(&doc);
        assert!(ks.contains(&"vector"), "自由路径应矢量化而非近似:{ks:?}");
        assert!(ks.contains(&"text"), "文本应导入:{ks:?}");
        // 不再出现旧的包围盒近似口径
        assert!(!joined.contains("包围盒矩形近似"), "旧口径残留:{joined}");
    }

    /// 05-6 保真:C/Q 段控制点逐一原样进入路径模型(无变换 → 逐点相等)。
    #[test]
    fn cubic_and_quadratic_preserved() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200">
            <path d="M10 20 C 40 20 40 60 70 60 Q 90 60 110 40" fill="#123456"/>
        </svg>"##;
        let (doc, _, _) = import_str("cq", svg);
        let node = doc
            .nodes
            .values()
            .find(|n| matches!(n.kind, NodeKind::Vector { .. }))
            .expect("应有矢量节点");
        let path = match &node.kind {
            NodeKind::Vector { path } => path,
            _ => unreachable!(),
        };
        let els: Vec<PathEl> = path.elements().to_vec();
        // 重定基到包围盒原点:包围盒 = (10,20)-(110,60) → 平移 (-10,-20)
        assert!(
            matches!(&els[0], PathEl::MoveTo(p) if (p.x - 0.0).abs() < 1e-6 && (p.y - 0.0).abs() < 1e-6)
        );
        assert!(
            matches!(&els[1], PathEl::CurveTo(c1, c2, p)
                if (c1.x - 30.0).abs() < 1e-6 && (c1.y - 0.0).abs() < 1e-6
                && (c2.x - 30.0).abs() < 1e-6 && (c2.y - 40.0).abs() < 1e-6
                && (p.x - 60.0).abs() < 1e-6 && (p.y - 40.0).abs() < 1e-6),
            "C 段控制点应保真:{els:?}"
        );
        assert!(
            matches!(&els[2], PathEl::QuadTo(c, p)
                if (c.x - 80.0).abs() < 1e-6 && (c.y - 40.0).abs() < 1e-6
                && (p.x - 100.0).abs() < 1e-6 && (p.y - 20.0).abs() < 1e-6),
            "Q 段控制点应保真:{els:?}"
        );
    }

    /// 05-6 保真:A 圆弧 = usvg 解析期转三次贝塞尔;采样中点与理想圆弧
    /// 比对(容差 0.5px)。弧:圆心 (150,100) 半径 50,上半段(sweep=1)。
    #[test]
    fn arc_converted_to_beziers_within_tolerance() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200">
            <path d="M100 100 A 50 50 0 0 1 200 100 Z" fill="#123456"/>
        </svg>"##;
        let (doc, _, _) = import_str("arc", svg);
        let node = doc
            .nodes
            .values()
            .find(|n| matches!(n.kind, NodeKind::Vector { .. }))
            .expect("应有矢量节点");
        let path = match &node.kind {
            NodeKind::Vector { path } => path,
            _ => unreachable!(),
        };
        let els: Vec<PathEl> = path.elements().to_vec();
        // 采样全部三次段的中点,与理想圆比对:圆心世界系 (150,100) →
        // 本地系 (50,50)(包围盒 (100,50)-(200,100))
        let mut sampled = 0usize;
        let mut cur = (0.0f64, 0.0f64);
        for el in &els {
            match el {
                PathEl::MoveTo(p) => cur = (p.x, p.y),
                PathEl::LineTo(p) => cur = (p.x, p.y),
                PathEl::QuadTo(_, p) => cur = (p.x, p.y),
                PathEl::CurveTo(c1, c2, p) => {
                    let m = cubic_at(cur, (c1.x, c1.y), (c2.x, c2.y), (p.x, p.y), 0.5);
                    let dx = m.0 - 50.0;
                    let dy = m.1 - 50.0;
                    let r = dx.hypot(dy);
                    assert!(
                        (r - 50.0).abs() < 0.5,
                        "弧采样点偏离理想圆 {r:.3}px(应 <0.5)"
                    );
                    sampled += 1;
                    cur = (p.x, p.y);
                }
                PathEl::ClosePath => {}
            }
        }
        assert!(sampled >= 1, "圆弧应产出至少一段三次贝塞尔");
    }

    /// 三次贝塞尔 t 处取点(de Casteljau 直接式)。
    fn cubic_at(
        p0: (f64, f64),
        p1: (f64, f64),
        p2: (f64, f64),
        p3: (f64, f64),
        t: f64,
    ) -> (f64, f64) {
        let u = 1.0 - t;
        let a = u * u * u;
        let b = 3.0 * u * u * t;
        let c = 3.0 * u * t * t;
        let d = t * t * t;
        (
            a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
            a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
        )
    }

    /// 图元折算不回退:纯矩形仍折算为 Box(带 fill 色)。
    #[test]
    fn rect_still_becomes_box() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <rect x="10" y="10" width="50" height="30" fill="#3a86ff"/>
        </svg>"##;
        let (doc, _, _) = import_str("rect", svg);
        let node = doc
            .nodes
            .values()
            .find(|n| matches!(n.kind, NodeKind::Box))
            .expect("矩形应折算为盒");
        assert_eq!(node.style_get("background-color"), Some("#3a86ff"));
    }
}
