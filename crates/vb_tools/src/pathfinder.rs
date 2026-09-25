//! 路径查找器多结果运算(05-3 / X-1:分割 / 修边 / 轮廓)。
//!
//! 与 [`crate::boolean`] 的四基本运算互补:那三条运算的产物是「一个结果
//! 节点」,而分割/修边/轮廓天然是「一次操作 → 多个节点」——几何层产出
//! **一组碎片**([`Piece`]),经统一入口 [`pathfinder_multi_cmds`] 折算成一条
//! 可撤销的 `vb_doc::Command::MultiResult` 事务(05-3 多结果底座)。
//!
//! **输出语义(对象菜单 tooltip / 文档同口径,不许"看起来有点像")**:
//! - 分割 Divide:全部形状互相求交后,重组出所有**原子闭合区域**(每个
//!   区域不被任何形状内部横穿);填充继承「覆盖它的最上层形状」。开放子
//!   路径按闭合参与(AI 同口径:隐式闭合成面)。
//! - 修边 Trim:每个形状减去**它上方的全部形状**,再**去掉描边**、把
//!   **同填充色的碎片合并**为一件(几何上求并)。保留填充。
//! - 轮廓 Outline:所有形状的边线在**与其他形状的交点处切开**,输出为
//!   无填充的开放描边路径(每段一件);描边继承来源形状,来源无描边时
//!   补 1px 黑描边。同形状自身的自交点不切(显式边界,见模块尾注)。
//!
//! 帧:输入/输出的 `Piece::path` 都在**公共帧**(调用方给的画板本地系);
//! 节点本地↔公共帧的换算在统一入口里完成。

use flo_curves::bezier::path::{
    path_add, path_intersect, path_sub, BezierPathBuilder, SimpleBezierPath,
};
use flo_curves::bezier::{curve_intersects_curve_clip, BezierCurve, BezierCurveFactory, Curve};
use flo_curves::geo::Coord2;

use vb_common::geom::{BezPath, PathEl, Point};

use crate::boolean::path_bbox;

/// flo 布尔/求交精度(与 `boolean` 模块同值)。
const ACC: f64 = 0.01;
/// 求交参数 t 的端点过滤阈值(共享端点处不算交点)。
const T_EPS: f64 = 1e-6;

/// 多结果路径查找器运算(X-1 缺的三项;其余七项走 `boolean::BooleanOp`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiOp {
    /// 分割:全部线段求交后重组闭合区域
    Divide,
    /// 修边:去被覆盖部分 + 去描边 + 同色合并
    Trim,
    /// 轮廓:交点切开的无填充描边线
    Outline,
}

impl MultiOp {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "divide" => Some(Self::Divide),
            "trim" => Some(Self::Trim),
            "outline" => Some(Self::Outline),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Divide => "divide",
            Self::Trim => "trim",
            Self::Outline => "outline",
        }
    }

    /// 中文运算名(命令 label / 状态栏;撤销菜单显示「撤销 路径查找器:分割」)。
    pub fn zh(self) -> &'static str {
        match self {
            Self::Divide => "分割",
            Self::Trim => "修边",
            Self::Outline => "轮廓",
        }
    }
}

/// 参与多结果运算的一个形状(公共帧)。
#[derive(Debug, Clone)]
pub struct ShapeInput {
    /// 形状路径(公共帧;可含多个子路径)。
    pub path: BezPath,
    /// 填充色 key(修边同色合并 / 填充继承;None = 无填充声明)。
    pub fill: Option<String>,
    /// 描边色 key(轮廓描边继承;None = 无描边声明)。
    pub stroke: Option<String>,
    /// 源节点完整样式(结果继承用;统一入口填,独立调用几何层可留空)。
    pub style: Vec<vb_css::Decl>,
}

/// 运算产物的一个碎片(公共帧)。
#[derive(Debug, Clone)]
pub struct Piece {
    /// 碎片路径(公共帧;可能是开放路径 —— 轮廓输出)。
    pub path: BezPath,
    /// 覆盖该碎片的最上层来源索引(填充继承;轮廓 = 来源)。
    pub topmost: usize,
    /// 产生该碎片的来源索引(轮廓描边继承)。
    pub source: usize,
}

// ─────────────────────────── 线段级 machinery(轮廓) ───────────────────────────

/// 三次贝塞尔段(直线以 `is_line` 标记;二次段在解析时升阶)。
#[derive(Debug, Clone, Copy)]
struct Seg {
    start: Point,
    c1: Point,
    c2: Point,
    end: Point,
    is_line: bool,
}

impl Seg {
    fn flo_curve(&self) -> Curve<Coord2> {
        Curve::from_points(
            Coord2(self.start.x, self.start.y),
            (Coord2(self.c1.x, self.c1.y), Coord2(self.c2.x, self.c2.y)),
            Coord2(self.end.x, self.end.y),
        )
    }

    fn from_flo_curve(c: &Curve<Coord2>, is_line: bool) -> Seg {
        let sp = c.start_point();
        let (h1, h2) = c.control_points();
        let ep = c.end_point();
        Seg {
            start: Point::new(sp.0, sp.1),
            c1: Point::new(h1.0, h1.1),
            c2: Point::new(h2.0, h2.1),
            end: Point::new(ep.0, ep.1),
            is_line,
        }
    }

    fn len2(&self) -> f64 {
        (self.end.x - self.start.x).hypot(self.end.y - self.start.y)
    }
}

/// BezPath → 子路径 × 线段(二次段升阶为三次;ClosePath 若首尾不重合
/// 补一段闭合直线)。
fn segments(path: &BezPath) -> Vec<Vec<Seg>> {
    let mut out: Vec<Vec<Seg>> = vec![];
    let mut cur: Vec<Seg> = vec![];
    let mut start = Point::ZERO;
    let mut prev = Point::ZERO;
    let mut open = false;
    let flush = |cur: &mut Vec<Seg>, out: &mut Vec<Vec<Seg>>| {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) => {
                flush(&mut cur, &mut out);
                start = *p;
                prev = *p;
                open = true;
            }
            PathEl::ClosePath => {
                // 闭合段:首尾不重合才补直线(flo 布尔隐式闭合,线段级
                // 视图必须显式补,否则闭合边的交点切不出来)
                if open && (prev.x - start.x).abs() > T_EPS || (prev.y - start.y).abs() > T_EPS {
                    cur.push(Seg {
                        start: prev,
                        c1: prev,
                        c2: start,
                        end: start,
                        is_line: true,
                    });
                }
                flush(&mut cur, &mut out);
                open = false;
                prev = start;
            }
            PathEl::LineTo(p) => {
                if open {
                    cur.push(Seg {
                        start: prev,
                        c1: prev,
                        c2: *p,
                        end: *p,
                        is_line: true,
                    });
                }
                prev = *p;
            }
            PathEl::QuadTo(c, p) => {
                if open {
                    // 二次 → 三次升阶(与 boolean::to_flo 同式)
                    cur.push(Seg {
                        start: prev,
                        c1: Point::new(
                            prev.x + 2.0 / 3.0 * (c.x - prev.x),
                            prev.y + 2.0 / 3.0 * (c.y - prev.y),
                        ),
                        c2: Point::new(
                            p.x + 2.0 / 3.0 * (c.x - p.x),
                            p.y + 2.0 / 3.0 * (c.y - p.y),
                        ),
                        end: *p,
                        is_line: false,
                    });
                }
                prev = *p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                if open {
                    cur.push(Seg {
                        start: prev,
                        c1: *c1,
                        c2: *c2,
                        end: *p,
                        is_line: false,
                    });
                }
                prev = *p;
            }
        }
    }
    // 尾部开放子路径(轮廓输入允许开放路径;布尔类输入会经 to_flo_multi
    // 隐式闭合)
    flush(&mut cur, &mut out);
    out
}

/// 在参数 t 处切开线段(t 已按升序、相对当前剩余段折算)。
fn split_segment(seg: &Seg, ts: &[f64]) -> Vec<Seg> {
    let mut out = vec![];
    let mut cur = seg.flo_curve();
    let mut t_prev = 0.0f64;
    for &t in ts {
        let t_local = ((t - t_prev) / (1.0 - t_prev)).clamp(0.0, 1.0);
        let (left, right) = cur.subdivide(t_local);
        out.push(Seg::from_flo_curve(&left, seg.is_line));
        cur = right;
        t_prev = t;
    }
    out.push(Seg::from_flo_curve(&cur, seg.is_line));
    // 滤掉切点贴端点产生的零长段
    out.retain(|s| s.len2() > T_EPS);
    out
}

/// 线段 → 开放路径碎片(MoveTo + 单段;不闭合)。
fn seg_to_open_path(s: &Seg) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(s.start);
    if s.is_line {
        p.line_to(s.end);
    } else {
        p.curve_to(s.c1, s.c2, s.end);
    }
    p
}

// ─────────────────────────── flo 互转(多子路径) ───────────────────────────

/// kurbo BezPath → flo 路径列表(**每个子路径一条**;flo 布尔把每条
/// 隐式闭合)。无任何子路径时返回 None。
pub(crate) fn to_flo_multi(src: &BezPath) -> Option<Vec<SimpleBezierPath>> {
    let mut out: Vec<SimpleBezierPath> = vec![];
    for sub in segments(src) {
        if sub.is_empty() {
            continue;
        }
        let mut builder =
            BezierPathBuilder::<SimpleBezierPath>::start(Coord2(sub[0].start.x, sub[0].start.y));
        for s in &sub {
            // 直线段压平:控制柄与端点重合时用 line_to(与 boolean 模块
            // 的直线判定同阈值),保住 flo 输出的直线判定
            let straight = (s.c1.x - s.start.x).abs() < 1e-6
                && (s.c1.y - s.start.y).abs() < 1e-6
                && (s.c2.x - s.end.x).abs() < 1e-6
                && (s.c2.y - s.end.y).abs() < 1e-6;
            builder = if straight {
                builder.line_to(Coord2(s.end.x, s.end.y))
            } else {
                builder.curve_to(
                    (Coord2(s.c1.x, s.c1.y), Coord2(s.c2.x, s.c2.y)),
                    Coord2(s.end.x, s.end.y),
                )
            };
        }
        out.push(builder.build());
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

// ─────────────────────────── 三运算几何 ───────────────────────────

/// 分割:增量原子分解 —— 逐形状把已有碎片按「与该形状的交 / 差」切开,
/// 再补上该形状未被覆盖的部分;产物两两互不重叠、并集 = 全形状并集。
pub fn divide(shapes: &[ShapeInput]) -> Result<Vec<Piece>, String> {
    if shapes.len() < 2 {
        return Err("分割:需要至少 2 个形状".into());
    }
    let flo_shapes = flo_of(shapes)?;
    let mut pieces: Vec<Piece> = vec![];
    for (i, flo_s) in flo_shapes.iter().enumerate() {
        // 分裂前的碎片并集快照(分裂保并集,S 减它 = S 的未覆盖部分)
        let prev_union: Vec<SimpleBezierPath> = pieces
            .iter()
            .filter_map(|p| to_flo_multi(&p.path))
            .flatten()
            .collect();
        let mut next: Vec<Piece> = vec![];
        for p in &pieces {
            let flo_p = to_flo_multi(&p.path).ok_or("碎片路径无法转换")?;
            let inter = path_intersect::<SimpleBezierPath>(&flo_p, flo_s, ACC);
            let rest = path_sub::<SimpleBezierPath>(&flo_p, flo_s, ACC);
            if !inter.is_empty() {
                next.push(Piece {
                    path: piece_from_flo(&inter),
                    topmost: i,
                    source: i,
                });
            }
            if !rest.is_empty() {
                next.push(Piece {
                    path: piece_from_flo(&rest),
                    topmost: p.topmost,
                    source: p.source,
                });
            }
        }
        // S − union(已有碎片):还没有任何碎片时就是 S 自身
        let new_part = if prev_union.is_empty() {
            flo_s.clone()
        } else {
            path_sub::<SimpleBezierPath>(flo_s, &prev_union, ACC)
        };
        if !new_part.is_empty() {
            next.push(Piece {
                path: piece_from_flo(&new_part),
                topmost: i,
                source: i,
            });
        }
        pieces = next;
    }
    if pieces.is_empty() {
        return Err("分割:没有产出任何区域".into());
    }
    Ok(pieces)
}

/// 修边:每个形状减去其上方全部形状;同填充色的碎片再求并合并为一件
/// (AI 语义:去描边在调用方落样式,几何层只管区域与同色合并)。
pub fn trim(shapes: &[ShapeInput]) -> Result<Vec<Piece>, String> {
    if shapes.len() < 2 {
        return Err("修边:需要至少 2 个形状".into());
    }
    let flo_shapes = flo_of(shapes)?;
    let mut pieces: Vec<Piece> = vec![];
    for (i, flo_s) in flo_shapes.iter().enumerate() {
        // 逐个形状相减。flo 的路径列表是「单个 even-odd 区域」,把相互
        // 重叠的上层形状并成一个列表会先被偶奇 XOR(交叠处翻成外部),
        // 必须按形状逐次 path_sub(基区带孔洞以独立子路径参与 even-odd,
        // 正是 flo 的输入约定)。
        let mut cur: Vec<SimpleBezierPath> = flo_s.clone();
        for flo_above in flo_shapes.iter().skip(i + 1) {
            if cur.is_empty() {
                break;
            }
            cur = path_sub::<SimpleBezierPath>(&cur, flo_above, ACC);
        }
        if !cur.is_empty() {
            pieces.push(Piece {
                path: piece_from_flo(&cur),
                topmost: i,
                source: i,
            });
        }
    }
    if pieces.is_empty() {
        return Err("修边:没有产出任何区域".into());
    }
    Ok(merge_same_fill(shapes, pieces))
}

/// 同填充色碎片求并合并(fill key 相同;key 顺序按首次出现,输出稳定)。
fn merge_same_fill(shapes: &[ShapeInput], pieces: Vec<Piece>) -> Vec<Piece> {
    let mut group_order: Vec<Option<&str>> = vec![];
    let mut groups: std::collections::HashMap<Option<&str>, Vec<usize>> =
        std::collections::HashMap::new();
    for (k, p) in pieces.iter().enumerate() {
        let key = shapes[p.topmost].fill.as_deref();
        if !groups.contains_key(&key) {
            group_order.push(key);
        }
        groups.entry(key).or_default().push(k);
    }
    let mut out = vec![];
    for key in group_order {
        let idxs = &groups[&key];
        if idxs.len() == 1 {
            out.push(pieces[idxs[0]].clone());
        } else {
            // 同色相邻碎片求并(flo 输出的多子路径即合并结果)
            let flo: Vec<SimpleBezierPath> = idxs
                .iter()
                .filter_map(|&k| to_flo_multi(&pieces[k].path))
                .flatten()
                .collect();
            let merged = path_add::<SimpleBezierPath>(&flo, &Vec::<SimpleBezierPath>::new(), ACC);
            let path = if merged.is_empty() {
                // 求并退化(理论不可达;保底拼接原碎片,不静默丢件)
                let mut all = BezPath::new();
                for &k in idxs {
                    all.extend(pieces[k].path.elements().iter().copied());
                }
                all
            } else {
                piece_from_flo(&merged)
            };
            out.push(Piece {
                path,
                topmost: pieces[idxs[0]].topmost,
                source: pieces[idxs[0]].source,
            });
        }
    }
    out
}

/// 轮廓:全部边线在「与其他形状的交点」处切开,输出开放描边路径
/// (每段一件;同形状自交不切 —— 显式边界,见模块头注)。
pub fn outline(shapes: &[ShapeInput]) -> Result<Vec<Piece>, String> {
    if shapes.is_empty() {
        return Err("轮廓:需要至少 1 个形状".into());
    }
    let segs: Vec<Vec<Vec<Seg>>> = shapes.iter().map(|s| segments(&s.path)).collect();
    // 交点参数表:ts[形状][子路径][线段] = 需要切的 t(升序去重)
    let mut ts: Vec<Vec<Vec<Vec<f64>>>> = segs
        .iter()
        .map(|sp| sp.iter().map(|s| vec![vec![]; s.len()]).collect())
        .collect();
    for i in 0..shapes.len() {
        for j in (i + 1)..shapes.len() {
            for (si, subs) in segs[i].iter().enumerate() {
                for (ai, a) in subs.iter().enumerate() {
                    for (sj, subt) in segs[j].iter().enumerate() {
                        for (bj, b) in subt.iter().enumerate() {
                            for (t1, t2) in
                                curve_intersects_curve_clip(&a.flo_curve(), &b.flo_curve(), ACC)
                            {
                                if t1 > T_EPS && t1 < 1.0 - T_EPS {
                                    ts[i][si][ai].push(t1);
                                }
                                if t2 > T_EPS && t2 < 1.0 - T_EPS {
                                    ts[j][sj][bj].push(t2);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // 切分输出
    let mut out = vec![];
    for (i, subs) in segs.iter().enumerate() {
        for (si, sub) in subs.iter().enumerate() {
            for (ai, seg) in sub.iter().enumerate() {
                let mut cuts = ts[i][si][ai].clone();
                cuts.retain(|t| *t > T_EPS && *t < 1.0 - T_EPS);
                cuts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                for piece in split_segment(seg, &cuts) {
                    out.push(Piece {
                        path: seg_to_open_path(&piece),
                        topmost: i,
                        source: i,
                    });
                }
            }
        }
    }
    if out.is_empty() {
        return Err("轮廓:没有产出任何线段".into());
    }
    Ok(out)
}

/// flo 结果 → kurbo 路径 + **绕向规范化**。
///
/// flo 布尔输出的孔洞子路径绕向不保证与外环相反(nonzero 填充下会把孔
/// 渲染成实体)。这里按「包含深度」定绕向:被偶数层包含 = 外环(正向),
/// 奇数层 = 孔(反向)—— 面积因此等于直观面积,渲染 nonzero 正确。
fn piece_from_flo(raw: &[SimpleBezierPath]) -> BezPath {
    let path = crate::boolean::from_flo(raw, 0.0, 0.0);
    normalize_windings(&path)
}

/// 子路径展平(每段采样 32 点)+ 有向面积(shoelace)。
fn flatten_signed(sub: &[Seg]) -> (Vec<Point>, f64) {
    let mut pts: Vec<Point> = vec![];
    if sub.is_empty() {
        return (pts, 0.0);
    }
    pts.push(sub[0].start);
    for s in sub {
        if s.is_line {
            pts.push(s.end);
            continue;
        }
        let a = s.start;
        let (c1, c2, b) = (s.c1, s.c2, s.end);
        for k in 1..=32i32 {
            let t = k as f64 / 32.0;
            let w0 = (1.0 - t) * (1.0 - t) * (1.0 - t);
            let w1 = 3.0 * (1.0 - t) * (1.0 - t) * t;
            let w2 = 3.0 * (1.0 - t) * t * t;
            let w3 = t * t * t;
            pts.push(Point::new(
                w0 * a.x + w1 * c1.x + w2 * c2.x + w3 * b.x,
                w0 * a.y + w1 * c1.y + w2 * c2.y + w3 * b.y,
            ));
        }
    }
    let mut s_area = 0.0;
    for i in 0..pts.len() {
        let p = pts[i];
        let q = pts[(i + 1) % pts.len()];
        s_area += p.x * q.y - q.x * p.y;
    }
    (pts, s_area / 2.0)
}

/// 偶奇射线法:p 是否在多边形内。
fn point_in_poly(p: Point, poly: &[Point]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let pi = poly[i];
        let pj = poly[j];
        if (pi.y > p.y) != (pj.y > p.y) {
            let x_cross = pj.x + (p.y - pj.y) / (pi.y - pj.y) * (pi.x - pj.x);
            if p.x < x_cross {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// 子路径写回 BezPath(`rev = true` 时反向,绕向翻转)。
fn push_sub(out: &mut BezPath, sub: &[Seg], rev: bool) {
    if sub.is_empty() {
        return;
    }
    if !rev {
        out.move_to(sub[0].start);
        for s in sub {
            if s.is_line {
                out.line_to(s.end);
            } else {
                out.curve_to(s.c1, s.c2, s.end);
            }
        }
    } else {
        out.move_to(sub[sub.len() - 1].end);
        for s in sub.iter().rev() {
            if s.is_line {
                out.line_to(s.start);
            } else {
                out.curve_to(s.c2, s.c1, s.start);
            }
        }
    }
    out.close_path();
}

/// 绕向规范化(多子路径才有效;单子路径原样返回)。
fn normalize_windings(path: &BezPath) -> BezPath {
    let subs = segments(path);
    if subs.len() <= 1 {
        return path.clone();
    }
    let flat: Vec<(Vec<Point>, f64)> = subs.iter().map(|s| flatten_signed(s)).collect();
    let mut out = BezPath::new();
    for (i, (pts, signed)) in flat.iter().enumerate() {
        if pts.is_empty() {
            continue;
        }
        // 参考点 = 该子路径首顶点;计数其它子路径对它的包含数
        let depth = flat
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .filter(|(_, (pj, _))| point_in_poly(pts[0], pj))
            .count();
        let want_positive = depth % 2 == 0;
        let is_positive = *signed >= 0.0;
        push_sub(&mut out, &subs[i], want_positive != is_positive);
    }
    out
}

/// 输入形状的 flo 路径(至少要有一条子路径)。
fn flo_of(shapes: &[ShapeInput]) -> Result<Vec<Vec<SimpleBezierPath>>, String> {
    shapes
        .iter()
        .enumerate()
        .map(|(i, s)| {
            to_flo_multi(&s.path)
                .ok_or_else(|| format!("第 {} 个形状无法转换(缺少路径数据)", i + 1))
        })
        .collect()
}

// ─────────────────────────── 统一入口(上层共用) ───────────────────────────

/// 统一入口的产物:一条可撤销的多结果事务 + 结果节点 sid(上层据此改选区)。
pub struct PathfinderPlan {
    pub command: vb_doc::commands::Command,
    pub result_sids: Vec<String>,
}

/// 对文档中选中的矢量节点执行多结果路径查找器运算(统一入口;vb_app /
/// vb_agent / 05-8 主件同步共用)。
///
/// 约束(不满足返回 Err,调用方给中文提示):
/// - 全部选中项是矢量路径节点(编组请先取消编组);
/// - 分割/修边 ≥2 个,轮廓 ≥1 个;全部同画板;
/// - `selection` 顺序不限,内部按画板文档序(z 序)自底向上重排,
///   **锚点 = z 序最低的源**,结果整体替换其槽位(`ReplaceAnchor`)。
///
/// 帧:源路径从节点本地系平移到画板本地系(公共帧)参与运算;结果碎片
/// 再重定基到锚点父级(画板/层 = 原点,组 = 组原点)。填充继承「覆盖
/// 碎片的最上层源」;轮廓置 `fill: none` 并继承描边(无描边补 1px 黑)。
pub fn pathfinder_multi_cmds(
    doc: &mut vb_doc::model::Document,
    op: MultiOp,
    selection: &[String],
) -> Result<PathfinderPlan, String> {
    use vb_doc::commands::{Command, MultiResultSlot};
    use vb_doc::model::{Geom, NodeKind, NodeTree};

    let min_n = match op {
        MultiOp::Outline => 1,
        _ => 2,
    };
    if selection.len() < min_n {
        return Err(format!(
            "路径查找器:{}:需要至少选中 {min_n} 个矢量路径",
            op.zh()
        ));
    }
    // ① 校验 + 收集(顺序先随 selection)
    let mut srcs: Vec<(String, vb_doc::model::NodeId)> = vec![];
    for sid in selection {
        let Some(id) = doc.find_by_sid(sid) else {
            return Err(format!("路径查找器:找不到对象 {sid}"));
        };
        let kind_is_vector = matches!(
            doc.nodes.get(id).map(|n| &n.kind),
            Some(NodeKind::Vector { .. })
        );
        if !kind_is_vector {
            return Err(format!(
                "路径查找器:{}:只支持矢量路径对象(编组请先取消编组)",
                op.zh()
            ));
        }
        srcs.push((sid.clone(), id));
    }
    // ② 同画板 + z 序自底向上(画板子树 DFS 序 = 文档序)
    let ab = crate::artboard_of(doc, srcs[0].1).ok_or("路径查找器:找不到所属画板")?;
    let mut order_ids = vec![];
    doc.subtree(ab, &mut order_ids);
    for (_, id) in &srcs {
        if crate::artboard_of(doc, *id) != Some(ab) {
            return Err("路径查找器:选中的对象必须属于同一画板".into());
        }
    }
    srcs.sort_by_key(|(_, id)| {
        order_ids
            .iter()
            .position(|&o| o == *id)
            .unwrap_or(usize::MAX)
    });

    // ③ 源路径 → 公共帧 + 填充/描边 key
    let mut shapes: Vec<ShapeInput> = vec![];
    let mut src_names: Vec<String> = vec![];
    for (_sid, id) in &srcs {
        let n = doc.nodes.get(*id).ok_or("路径查找器:节点丢失")?;
        let NodeKind::Vector { path } = &n.kind else {
            return Err("路径查找器:节点已不是矢量路径".into());
        };
        let abs = crate::abs_bbox(doc, *id).ok_or("路径查找器:取不到对象几何")?;
        let mut common = path.clone();
        common.apply_affine(vb_common::geom::Affine::translate((abs.x0, abs.y0)));
        shapes.push(ShapeInput {
            path: common,
            fill: n
                .style
                .iter()
                .find(|d| d.prop == "fill")
                .map(|d| d.value.trim().to_string()),
            stroke: n
                .style
                .iter()
                .find(|d| d.prop == "stroke")
                .map(|d| d.value.trim().to_string()),
            style: n.style.clone(),
        });
        src_names.push(n.name.clone());
    }

    // ④ 几何运算(公共帧)
    let pieces = match op {
        MultiOp::Divide => divide(&shapes)?,
        MultiOp::Trim => trim(&shapes)?,
        MultiOp::Outline => outline(&shapes)?,
    };

    // ⑤ 结果重定基到锚点父级并建树(sid 预分配;重做复用)
    let anchor_id = srcs[0].1;
    let anchor_parent = doc.nodes.get(anchor_id).unwrap().parent.unwrap_or(doc.root);
    let offset = anchor_frame_offset(doc, anchor_parent);
    let mut results: Vec<NodeTree> = vec![];
    let mut result_sids: Vec<String> = vec![];
    for p in &pieces {
        let sid = doc.alloc_sid();
        let bbox = path_bbox(&p.path);
        let mut local = p.path.clone();
        local.apply_affine(vb_common::geom::Affine::translate((-bbox[0], -bbox[1])));
        let src = &shapes[p.source];
        let top = &shapes[p.topmost];
        let mut n = vb_doc::model::Node::new(
            NodeKind::Vector { path: local },
            format!("{} {}", src_names[p.source], op.zh()),
            sid.clone(),
        );
        n.geom = Geom {
            x: bbox[0] - offset.0,
            y: bbox[1] - offset.1,
            w: bbox[2],
            h: bbox[3],
        };
        // 样式继承:
        // - 分割/修边:取覆盖碎片的最上层源(修边去描边在下面统一落);
        // - 轮廓:fill: none + 继承描边(无描边补 1px 黑)。
        let mut style = style_of(top);
        if op == MultiOp::Trim {
            style.retain(|d| !d.prop.starts_with("stroke"));
        }
        if op == MultiOp::Outline {
            style.retain(|d| !d.prop.starts_with("fill"));
            style.push(decl("fill", "none"));
            if src.stroke.is_none() {
                style.push(decl("stroke", "#000000"));
                style.push(decl("stroke-width", "1"));
            }
        }
        n.style = style;
        // 碎片是新几何:导出按 geom 写显式 px(声明层保持默认 false)
        n.geom_declared = false;
        let tree = NodeTree {
            node: n,
            children: vec![],
        };
        result_sids.push(sid.as_str().to_string());
        results.push(tree);
    }

    let command = Command::MultiResult {
        op: format!("路径查找器:{}", op.zh()),
        src_sids: srcs.iter().map(|(s, _)| s.clone()).collect(),
        results,
        slot: MultiResultSlot::ReplaceAnchor,
        captured: None,
    };
    Ok(PathfinderPlan {
        command,
        result_sids,
    })
}

// ── 入口内部小工具 ──

/// 锚点父级的**子级帧原点**(画板/层的孩子 = 画板本地;组/盒的孩子 = 容器本地)。
fn anchor_frame_offset(doc: &vb_doc::model::Document, parent: vb_doc::model::NodeId) -> (f64, f64) {
    use vb_doc::model::NodeKind;
    match doc.nodes.get(parent).map(|n| &n.kind) {
        Some(NodeKind::Artboard) | Some(NodeKind::Layer) | None => (0.0, 0.0),
        _ => {
            // Box/Group 等容器:孩子的 geom 相对容器原点
            let n = doc.nodes.get(parent).unwrap();
            (n.geom.x, n.geom.y)
        }
    }
}

fn decl(prop: &str, value: &str) -> vb_css::Decl {
    vb_css::Decl {
        prop: prop.into(),
        value: value.into(),
        important: false,
    }
}

/// 结果节点样式:继承源形状的**完整样式**(碎片几何以 geom 为准,滤掉
/// 遗留几何声明防导出双写)。`fill`/`background-color` 等通道都随源走,
/// 渲染端认哪条都不断档。
fn style_of(s: &ShapeInput) -> Vec<vb_css::Decl> {
    const GEOM_PROPS: &[&str] = &[
        "position", "left", "top", "right", "bottom", "inset", "width", "height",
    ];
    s.style
        .iter()
        .filter(|d| !GEOM_PROPS.contains(&d.prop.as_str()))
        .cloned()
        .collect()
}

// ─────────────────────────── 几何测试(几何只能靠测试锁) ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use vb_common::geom::PathEl;

    const KAPPA: f64 = 0.552_284_749_830_793_3;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to(Point::new(x, y));
        p.line_to(Point::new(x + w, y));
        p.line_to(Point::new(x + w, y + h));
        p.line_to(Point::new(x, y + h));
        p.close_path();
        p
    }

    /// 逼近面积(每段采样 32 点的多边形面积;测试断言足够)。
    fn area(path: &BezPath) -> f64 {
        let mut pts: Vec<Point> = vec![];
        let mut cur = Point::ZERO;
        for el in path.elements() {
            match el {
                PathEl::MoveTo(p) => {
                    cur = *p;
                    pts.push(*p);
                }
                PathEl::LineTo(p) => {
                    cur = *p;
                    pts.push(*p);
                }
                PathEl::QuadTo(c, p) => {
                    for k in 1..=32i32 {
                        let t = k as f64 / 32.0;
                        let a = (1.0 - t) * (1.0 - t);
                        let b = 2.0 * (1.0 - t) * t;
                        pts.push(Point::new(
                            a * cur.x + b * c.x + t * t * p.x,
                            a * cur.y + b * c.y + t * t * p.y,
                        ));
                    }
                    cur = *p;
                }
                PathEl::CurveTo(c1, c2, p) => {
                    for k in 1..=32i32 {
                        let t = k as f64 / 32.0;
                        let a = (1.0 - t) * (1.0 - t) * (1.0 - t);
                        let b = 3.0 * (1.0 - t) * (1.0 - t) * t;
                        let c = 3.0 * (1.0 - t) * t * t;
                        let d = t * t * t;
                        pts.push(Point::new(
                            a * cur.x + b * c1.x + c * c2.x + d * p.x,
                            a * cur.y + b * c1.y + c * c2.y + d * p.y,
                        ));
                    }
                    cur = *p;
                }
                PathEl::ClosePath => {}
            }
        }
        let mut s = 0.0;
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            s += a.x * b.y - b.x * a.y;
        }
        (s / 2.0).abs()
    }

    fn bbox(path: &BezPath) -> [f64; 4] {
        path_bbox(path)
    }

    fn assert_bbox(p: &BezPath, x: f64, y: f64, w: f64, h: f64, what: &str) {
        let b = bbox(p);
        for (got, want) in b.iter().zip([x, y, w, h]) {
            assert!(
                (got - want).abs() < 1.0,
                "{what}: bbox {b:?} 应为 [{x}, {y}, {w}, {h}]"
            );
        }
    }

    fn shape(path: BezPath, fill: &str) -> ShapeInput {
        ShapeInput {
            path,
            fill: Some(fill.into()),
            stroke: None,
            style: vec![],
        }
    }

    /// 4 段三次曲线的圆(圆心 cx,cy 半径 r)。
    fn circle(cx: f64, cy: f64, r: f64) -> BezPath {
        let k = r * KAPPA;
        let mut p = BezPath::new();
        p.move_to(Point::new(cx + r, cy));
        p.curve_to(
            Point::new(cx + r, cy + k),
            Point::new(cx + k, cy + r),
            Point::new(cx, cy + r),
        );
        p.curve_to(
            Point::new(cx - k, cy + r),
            Point::new(cx - r, cy + k),
            Point::new(cx - r, cy),
        );
        p.curve_to(
            Point::new(cx - r, cy - k),
            Point::new(cx - k, cy - r),
            Point::new(cx, cy - r),
        );
        p.curve_to(
            Point::new(cx + k, cy - r),
            Point::new(cx + r, cy - k),
            Point::new(cx + r, cy),
        );
        p.close_path();
        p
    }

    // ── 分割 ──

    /// 两交叉矩形 → 3 块:交叠条 / 左侧 L 块 / 右侧方块;填充继承最上层。
    #[test]
    fn divide_two_crossing_rects() {
        let shapes = [
            shape(rect(0.0, 0.0, 200.0, 200.0), "red"),
            shape(rect(100.0, 50.0, 200.0, 100.0), "blue"),
        ];
        let pieces = divide(&shapes).expect("分割成功");
        assert_eq!(pieces.len(), 3, "两交叉矩形应产出 3 块");
        // 产出顺序锁定(实现顺序 = 确定性的一部分)
        assert_bbox(&pieces[0].path, 100.0, 50.0, 100.0, 100.0, "交叠条");
        assert_bbox(&pieces[1].path, 0.0, 0.0, 200.0, 200.0, "左侧 A−B 块");
        assert_bbox(&pieces[2].path, 200.0, 50.0, 100.0, 100.0, "右侧 B−A 块");
        assert_eq!(pieces[0].topmost, 1, "交叠块填充继承上层");
        assert_eq!(pieces[1].topmost, 0);
        assert_eq!(pieces[2].topmost, 1);
        // 面积守恒:3 块之和 = 两形状并集(40000 + 20000 - 10000 = 50000)
        let total: f64 = pieces.iter().map(|p| area(&p.path)).sum();
        assert!(
            (total - 50000.0).abs() < 30.0,
            "并集面积 {total} 应为 50000"
        );
    }

    /// 包含与分离:内块/远块都不再细分。
    #[test]
    fn divide_contained_and_disjoint() {
        // B 完全在 A 内 → 2 块(B 与 A−B 环)
        let shapes = [
            shape(rect(0.0, 0.0, 300.0, 300.0), "red"),
            shape(rect(100.0, 100.0, 50.0, 50.0), "blue"),
        ];
        let pieces = divide(&shapes).expect("包含分割成功");
        assert_eq!(pieces.len(), 2, "包含形状应产出 2 块");
        let ring = pieces.iter().find(|p| p.topmost == 0).expect("应有外环块");
        let total = area(&ring.path);
        assert!(
            (total - (300.0 * 300.0 - 50.0 * 50.0)).abs() < 60.0,
            "外环面积 {total} 应为 87500"
        );
        // 远离 → 2 块各自原样
        let shapes = [
            shape(rect(0.0, 0.0, 100.0, 100.0), "red"),
            shape(rect(500.0, 500.0, 80.0, 80.0), "blue"),
        ];
        let pieces = divide(&shapes).expect("分离分割成功");
        assert_eq!(pieces.len(), 2, "分离形状应产出 2 块");
        assert_bbox(&pieces[0].path, 0.0, 0.0, 100.0, 100.0, "分离块 A");
        assert_bbox(&pieces[1].path, 500.0, 500.0, 80.0, 80.0, "分离块 B");
    }

    /// 曲线形状参与分割:圆内接于矩形 → 2 块;环块 ≈ 矩形 − 圆。
    #[test]
    fn divide_curve_inside_rect() {
        let shapes = [
            shape(rect(0.0, 0.0, 300.0, 300.0), "red"),
            shape(circle(150.0, 150.0, 80.0), "blue"),
        ];
        let pieces = divide(&shapes).expect("圆+矩形分割成功");
        assert_eq!(pieces.len(), 2, "圆在矩形内应产出 2 块");
        let circle_piece = pieces.iter().find(|p| p.topmost == 1).expect("应有圆形块");
        assert!((area(&circle_piece.path) - std::f64::consts::PI * 80.0 * 80.0).abs() < 120.0);
        let ring = pieces.iter().find(|p| p.topmost == 0).expect("应有外环块");
        let want = 300.0 * 300.0 - std::f64::consts::PI * 80.0 * 80.0;
        assert!(
            (area(&ring.path) - want).abs() < 200.0,
            "外环面积应为矩形−圆"
        );
    }

    /// 开放输入按闭合参与(AI 同口径):开口矩形与矩形交叉仍产出 3 块。
    #[test]
    fn divide_open_input_treated_closed() {
        let mut open_a = BezPath::new();
        open_a.move_to(Point::new(0.0, 0.0));
        open_a.line_to(Point::new(200.0, 0.0));
        open_a.line_to(Point::new(200.0, 200.0));
        open_a.line_to(Point::new(0.0, 200.0));
        // 无 ClosePath:首尾不闭合
        let shapes = [
            shape(open_a, "red"),
            shape(rect(100.0, 50.0, 200.0, 100.0), "blue"),
        ];
        let pieces = divide(&shapes).expect("开放输入分割成功");
        assert_eq!(pieces.len(), 3, "开放输入按闭合参与,应产出 3 块");
    }

    // ── 修边 ──

    /// 异色:下层减去上层,上层完整保留。
    #[test]
    fn trim_two_colors() {
        let shapes = [
            shape(rect(0.0, 0.0, 200.0, 200.0), "red"),
            shape(rect(100.0, 50.0, 200.0, 100.0), "blue"),
        ];
        let pieces = trim(&shapes).expect("修边成功");
        assert_eq!(pieces.len(), 2, "异色修边应产出 2 块");
        let lower = pieces.iter().find(|p| p.topmost == 0).expect("应有下层块");
        assert!(
            (area(&lower.path) - (200.0 * 200.0 - 100.0 * 100.0)).abs() < 30.0,
            "下层可见面积应为 30000"
        );
        assert_bbox(&pieces[1].path, 100.0, 50.0, 200.0, 100.0, "上层完整保留");
    }

    /// 同色相邻:合并为一件(并集面积)。
    #[test]
    fn trim_same_color_merges() {
        let shapes = [
            shape(rect(0.0, 0.0, 200.0, 200.0), "red"),
            shape(rect(100.0, 50.0, 200.0, 100.0), "red"),
        ];
        let pieces = trim(&shapes).expect("同色修边成功");
        assert_eq!(pieces.len(), 1, "同色修边应合并为 1 件");
        assert!(
            (area(&pieces[0].path) - 50000.0).abs() < 30.0,
            "合并件面积应为并集 50000"
        );
    }

    /// 三层堆叠:每层只减去其上方形状。
    #[test]
    fn trim_three_stack() {
        let shapes = [
            shape(rect(0.0, 0.0, 300.0, 100.0), "a"),
            shape(rect(100.0, 0.0, 100.0, 100.0), "b"),
            shape(rect(150.0, 0.0, 100.0, 50.0), "c"),
        ];
        let pieces = trim(&shapes).expect("三层修边成功");
        assert_eq!(pieces.len(), 3, "三层异色应产出 3 块");
        // a 层可见 = 300*100 − (b∪c=10000+5000−2500) = 17500
        // b 层可见 = 100*100 − c 的覆盖(50*50) = 7500;c 层 = 5000
        let a = pieces.iter().find(|p| p.topmost == 0).expect("a 块");
        assert!(
            (area(&a.path) - 17500.0).abs() < 30.0,
            "a 块面积 {}",
            area(&a.path)
        );
        let b = pieces.iter().find(|p| p.topmost == 1).expect("b 块");
        assert!(
            (area(&b.path) - 7500.0).abs() < 30.0,
            "b 块面积 {}",
            area(&b.path)
        );
        let c = pieces.iter().find(|p| p.topmost == 2).expect("c 块");
        assert!((area(&c.path) - 5000.0).abs() < 30.0);
    }

    // ── 轮廓 ──

    /// 两交叉矩形:12 段开放线(仅 A 的右边与 B 的上下边被切)。
    #[test]
    fn outline_crossing_rects_12_segments() {
        let shapes = [
            shape(rect(0.0, 0.0, 200.0, 200.0), "red"),
            shape(rect(100.0, 50.0, 200.0, 100.0), "blue"),
        ];
        let pieces = outline(&shapes).expect("轮廓成功");
        assert_eq!(pieces.len(), 12, "两交叉矩形应切出 12 段(各含闭合边)");
        for p in &pieces {
            assert_eq!(p.path.elements().len(), 2, "轮廓段应为 M+L 两元素");
            assert!(
                matches!(p.path.elements()[1], PathEl::LineTo(_)),
                "矩形轮廓只含直线段"
            );
        }
        let from_a = pieces.iter().filter(|p| p.source == 0).count();
        assert_eq!(from_a, 6, "A 贡献 6 段(右边切 3 + 其余 3)");
        let from_b = pieces.iter().filter(|p| p.source == 1).count();
        assert_eq!(from_b, 6, "B 贡献 6 段(上下边各切 2 + 左右各 1)");
    }

    /// 单形状 = 段数(矩形 4 段,含闭合边)。
    #[test]
    fn outline_single_shape() {
        let shapes = [shape(rect(0.0, 0.0, 100.0, 100.0), "red")];
        let pieces = outline(&shapes).expect("单形状轮廓成功");
        assert_eq!(pieces.len(), 4, "矩形轮廓应为 4 段");
    }

    /// 曲线形状:圆在矩形内无交点 → 8 段(4 直 + 4 曲),曲线段保留 CurveTo。
    #[test]
    fn outline_curve_contained() {
        let shapes = [
            shape(rect(0.0, 0.0, 300.0, 300.0), "red"),
            shape(circle(150.0, 150.0, 80.0), "blue"),
        ];
        let pieces = outline(&shapes).expect("圆+矩形轮廓成功");
        assert_eq!(pieces.len(), 8, "无交点时段数 = 输入段数(4+4)");
        let curves = pieces
            .iter()
            .filter(|p| matches!(p.path.elements()[1], PathEl::CurveTo(..)))
            .count();
        assert_eq!(curves, 4, "圆的 4 段应保留曲线");
        assert!(pieces.iter().all(|p| p.path.elements().len() == 2));
    }

    /// 曲线交叉:圆与矩形相交 → 段数增加且全部为开放单段。
    #[test]
    fn outline_curve_intersecting_splits() {
        // 圆心 (0,0) 半径 100;矩形 (80,-30,60,60) 横穿圆的右缘
        let shapes = [
            shape(circle(0.0, 0.0, 100.0), "blue"),
            shape(rect(80.0, -30.0, 60.0, 60.0), "red"),
        ];
        let pieces = outline(&shapes).expect("曲线交叉轮廓成功");
        assert!(
            pieces.len() > 8,
            "交点切开应增加段数(实际 {})",
            pieces.len()
        );
        assert!(pieces.iter().all(|p| p.path.elements().len() == 2));
    }

    /// MultiOp parse/as_str/zh 互逆与展示名。
    #[test]
    fn multiop_parse_roundtrip() {
        for (s, zh) in [("divide", "分割"), ("trim", "修边"), ("outline", "轮廓")] {
            let op = MultiOp::parse(s).unwrap_or_else(|| panic!("{s} 应可解析"));
            assert_eq!(op.as_str(), s);
            assert_eq!(op.zh(), zh);
        }
        assert!(
            MultiOp::parse("union").is_none(),
            "基础四运算归 boolean 模块"
        );
    }
}
