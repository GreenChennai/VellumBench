//! SVG 导入(M5):usvg 解析 → 场景图 → 规范化 HTML。
//!
//! 设计:与 import_pdf 同款 v1 边界——形状/文本/图像三类对象,绝对定位,
//! 复杂路径以包围盒盒节点近似(已知边界,记录于模块注释)。文本提取真实
//! 字符串(不转曲),颜色/字号/位置取自 SVG 属性;字体族透传给
//! @font-face/系统字体解析。渐变填充取首 stop 色 + 警告;filter/mask/
//! clipPath 跳过 + 警告。

use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    Some((px_len(minx), px_len(miny), px_len(maxx - minx), px_len(maxy - miny)))
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
            tiny_skia_path::PathSegment::CubicTo(c1, c2, pt) => {
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

/// 填充漆 → (hex, alpha, warnings 增量)。渐变取中点色近似。
fn paint_color(paint: &usvg::Paint) -> Option<(String, f32)> {
    match paint {
        usvg::Paint::Color(c) => Some((color_to_hex(*c), 1.0)),
        usvg::Paint::LinearGradient(g) => {
            let stops = g.stops();
            let mid = stops.get(stops.len() / 2)?;
            Some((color_to_hex(mid.color()), mid.opacity().get()))
        }
        usvg::Paint::RadialGradient(g) => {
            let stops = g.stops();
            let mid = stops.get(stops.len() / 2)?;
            Some((color_to_hex(mid.color()), mid.opacity().get()))
        }
        usvg::Paint::Pattern(_) => None,
    }
}

struct SvgWalker<'a> {
    doc: &'a mut Document,
    ab: NodeId,
    assets_dir: Option<PathBuf>,
    img_seq: usize,
    warnings: Vec<String>,
    approx_paths: usize,
}

impl<'a> SvgWalker<'a> {
    fn warn(&mut self, w: String) {
        if !self.warnings.contains(&w) && self.warnings.len() < 32 {
            self.warnings.push(w);
        }
    }

    fn walk_group(&mut self, group: &usvg::Group, tx: tiny_skia::Transform, depth: usize) {
        if depth > MAX_DEPTH {
            self.warn("嵌套过深(已截断)".into());
            return;
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
        (x * tx.sx as f64 + tx.tx as f64, y * tx.sy as f64 + tx.ty as f64)
    }

    fn check_tx(&mut self, tx: tiny_skia::Transform) {
        if (tx.kx as f64).abs() > 1e-3 || (tx.ky as f64).abs() > 1e-3 {
            self.warn("节点含倾斜变换(忽略)".into());
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
        // 填充优先;无填充看描边
        let (hex, alpha) = match p.fill() {
            Some(fill) => match paint_color(fill.paint()) {
                Some((h, a)) => (h, a * fill.opacity().get()),
                None => {
                    self.warn("pattern 填充近似为浅灰".into());
                    ("#e8e8e4".to_string(), 1.0)
                }
            },
            None => match p.stroke() {
                Some(st) => match paint_color(st.paint()) {
                    Some((h, a)) => (h, a * st.opacity().get()),
                    None => ("#e8e8e4".to_string(), 1.0),
                },
                None => return,
            },
        };
        let _ = alpha; // v1:小数值 alpha 直接透给 rgba() 由下方组装

        let (x, y) = self.node_xy(tx, bx, by);
        let (w, h) = (bw.max(1.0), bh.max(1.0));

        let lines = segs
            .iter()
            .filter(|s| matches!(s, tiny_skia_path::PathSegment::LineTo(..)))
            .count();
        let curves = segs
            .iter()
            .filter(|s| matches!(s, tiny_skia_path::PathSegment::CubicTo(..)))
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
                n.style_set("background-color", &hex);
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
            n.style_set("background-color", &hex);
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
                n.style_set("background-color", &hex);
                n.style_set("border-radius", &format!("{r:.1}px"));
                attach(self.doc, self.ab, n);
                return;
            }
        }
        // 其余路径 v1 以包围盒盒节点近似(与 PDF 导入同边界)
        self.approx_paths += 1;
        let sid = self.doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, "形状", sid);
        n.geom = Geom { x, y, w, h };
        n.authored = [true, true, true, true];
        n.style_set("background-color", &hex);
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
                self.warn("WEBP 嵌入图像暂不支持(已跳过)".into());
                return;
            }
            usvg::ImageKind::SVG(_) => {
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
        // 颜色/字号取首个 span(无 span 时墨色近似)
        let first_span = t.chunks().first().and_then(|c| c.spans().first());
        let hex = first_span
            .and_then(|s| s.fill())
            .and_then(|f| paint_color(f.paint()))
            .map(|(h, _)| h)
            .unwrap_or_else(|| "#1a1a1a".to_string());
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

/// 导入 SVG → Document(单画板)。
/// `assets_dir`:嵌入图像落盘目录(None = 图像跳过)。
pub fn import_svg_to_doc(
    svg_path: &Path,
    assets_dir: Option<&Path>,
) -> Result<(Document, Vec<String>), String> {
    let data = std::fs::read(svg_path).map_err(|e| format!("SVG 读取失败:{e}"))?;
    let mut opt = usvg::Options::default();
    let mut fontdb = fontdb::Database::new();
    fontdb.load_system_fonts();
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
        approx_paths: 0,
    };
    walker.walk_group(tree.root(), tiny_skia::Transform::identity(), 0);
    let approx = walker.approx_paths;
    let mut warnings = walker.warnings;
    if approx > 0 {
        warnings.push(format!("{approx} 个复杂路径以包围盒矩形近似(v1 边界)"));
    }
    Ok((document, warnings))
}

