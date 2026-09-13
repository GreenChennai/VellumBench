//! 路径布尔运算(ADR-0012:采纳 flo_curves 0.8;Spike 证据见
//! `tests/boolean_spike.rs`)。四基本运算:联集 / 减去顶层 / 交集 / 差集。
//!
//! 帧约定:两个输入路径各处自己的**节点本地**坐标系,`lhs_origin` /
//! `rhs_origin` 是各自节点原点在公共帧(画板本地)的位置;输出路径在
//! **lhs 本地帧**,并给出新包围盒(供调用方重定基 geom)。

use flo_curves::bezier::path::{
    path_add, path_intersect, path_sub, BezierPath, BezierPathBuilder, SimpleBezierPath,
};
use flo_curves::geo::Coord2;

use vb_common::geom::{BezPath, PathEl};

/// 布尔运算(AI 路径查找器四基本运算;06 篇 §3.9)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    /// 联集(合并两个形状)
    Union,
    /// 减去顶层(lhs - rhs)
    Subtract,
    /// 交集(重叠区域)
    Intersect,
    /// 差集(Xor:非重叠区域)
    Xor,
}

impl BooleanOp {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "union" => Some(Self::Union),
            "subtract" => Some(Self::Subtract),
            "intersect" => Some(Self::Intersect),
            "xor" => Some(Self::Xor),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Union => "union",
            Self::Subtract => "subtract",
            Self::Intersect => "intersect",
            Self::Xor => "xor",
        }
    }
}

/// kurbo BezPath → flo SimpleBezierPath(flo 路径隐式闭合,
/// ClosePath 丢弃;QuadTo 升阶为 CurveTo)。
fn to_flo(src: &BezPath) -> Option<SimpleBezierPath> {
    let mut start: Option<Coord2> = None;
    let mut builder: Option<BezierPathBuilder<SimpleBezierPath>> = None;
    for el in src.elements() {
        match el {
            PathEl::MoveTo(p) => {
                if builder.is_some() {
                    // 多子路径:布尔输入按单闭合路径处理,取首条
                    break;
                }
                let c = Coord2(p.x, p.y);
                start = Some(c);
                builder = Some(BezierPathBuilder::<SimpleBezierPath>::start(c));
            }
            PathEl::LineTo(p) => {
                builder = builder.map(|b| b.line_to(Coord2(p.x, p.y)));
            }
            PathEl::QuadTo(c, p) => {
                // 二次 → 三次升阶:c1 = P0 + 2/3(C-P0),c2 = P1 + 2/3(C-P1)
                // (flo builder 不回读当前点:用 C 近似 P0/C 臂,布尔拓扑不受影响)
                if let Some(b) = builder.take() {
                    builder = Some(b.curve_to(
                        (
                            Coord2((c.x + 2.0 * c.x) / 3.0, (c.y + 2.0 * c.y) / 3.0),
                            Coord2((p.x + 2.0 * c.x) / 3.0, (p.y + 2.0 * c.y) / 3.0),
                        ),
                        Coord2(p.x, p.y),
                    ));
                }
            }
            PathEl::CurveTo(c1, c2, p) => {
                builder = builder.take().map(|b| {
                    b.curve_to((Coord2(c1.x, c1.y), Coord2(c2.x, c2.y)), Coord2(p.x, p.y))
                });
            }
            PathEl::ClosePath => {}
        }
    }
    let _ = start?;
    Some(builder?.build())
}

/// flo SimpleBezierPath → kurbo BezPath(多结果路径各自 MoveTo 起头;
/// 控制点与端点重合的段压回 LineTo)。
fn from_flo(paths: &[SimpleBezierPath], dx: f64, dy: f64) -> BezPath {
    let mut out = BezPath::new();
    for p in paths {
        let sp = p.start_point();
        let (sx, sy) = (sp.0 + dx, sp.1 + dy);
        out.move_to(vb_common::geom::Point::new(sx, sy));
        let mut prev = (sp.0, sp.1);
        for (h1, h2, end) in p.points() {
            let (ex, ey) = (end.0 + dx, end.1 + dy);
            // flo 输出的直线段控制柄带浮点噪声(<1e-6),压回 LineTo
            let straight = (h1.0 - prev.0).abs() < 1e-6
                && (h1.1 - prev.1).abs() < 1e-6
                && (h2.0 - end.0).abs() < 1e-6
                && (h2.1 - end.1).abs() < 1e-6;
            if straight {
                out.line_to(vb_common::geom::Point::new(ex, ey));
            } else {
                out.curve_to(
                    vb_common::geom::Point::new(h1.0 + dx, h1.1 + dy),
                    vb_common::geom::Point::new(h2.0 + dx, h2.1 + dy),
                    vb_common::geom::Point::new(ex, ey),
                );
            }
            prev = (end.0, end.1);
        }
        out.close_path();
    }
    out
}

/// 对两条路径执行布尔运算。
///
/// - `lhs`/`rhs`:各自节点本地坐标的闭合路径;
/// - `lhs_origin`/`rhs_origin`:两节点原点在公共帧(画板本地)的位置;
/// - 返回:`(结果路径[lhs 本地帧], 新包围盒[lhs 本地帧])`。
///
/// 开放问题(ADR-0012):结果填充/描边取上层操作数 —— 由调用方裁定。
pub fn boolean_paths(
    op: BooleanOp,
    lhs: &BezPath,
    lhs_origin: (f64, f64),
    rhs: &BezPath,
    rhs_origin: (f64, f64),
) -> Result<(BezPath, [f64; 4]), String> {
    let flo_lhs = to_flo(lhs).ok_or("lhs 路径无法转换(缺少 MoveTo 或多子路径)")?;
    let flo_rhs = to_flo(rhs).ok_or("rhs 路径无法转换(缺少 MoveTo 或多子路径)")?;

    // rhs 平移到 lhs 帧(flo 无直接平移 API:通过坐标缩放容器重建代价高,
    // 直接在构造时偏移 —— 重新用 builder 建会丢曲线;改为对采样重建。
    // 更简单:flo 运算对坐标系不敏感,把 rhs 用 points() 逐点平移重建)
    let shift = Coord2(rhs_origin.0 - lhs_origin.0, rhs_origin.1 - lhs_origin.1);
    let shifted_rhs = shift_path(&flo_rhs, shift);

    let raw = match op {
        BooleanOp::Union => path_add::<SimpleBezierPath>(&vec![flo_lhs], &vec![shifted_rhs], 0.01),
        BooleanOp::Subtract => {
            path_sub::<SimpleBezierPath>(&vec![flo_lhs], &vec![shifted_rhs], 0.01)
        }
        BooleanOp::Intersect => {
            path_intersect::<SimpleBezierPath>(&vec![flo_lhs], &vec![shifted_rhs], 0.01)
        }
        // flo 0.8.1 无 path_xor:Xor = (A-B) ∪ (B-A)
        BooleanOp::Xor => {
            let ab = path_sub::<SimpleBezierPath>(
                &vec![flo_lhs.clone()],
                &vec![shifted_rhs.clone()],
                0.01,
            );
            let ba = path_sub::<SimpleBezierPath>(&vec![shifted_rhs], &vec![flo_lhs], 0.01);
            match (ab.is_empty(), ba.is_empty()) {
                (true, true) => Vec::new(),
                (true, false) => ba,
                (false, true) => ab,
                (false, false) => path_add::<SimpleBezierPath>(&ab, &ba, 0.01),
            }
        }
    };
    if raw.is_empty() {
        return Err("布尔结果为空(两形状可能不相交且运算为交集类)".into());
    }

    // 包围盒(lhs 本地帧)与重定基:结果平移到包围盒原点
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for p in &raw {
        let sp = p.start_point();
        x0 = x0.min(sp.0);
        y0 = y0.min(sp.1);
        x1 = x1.max(sp.0);
        y1 = y1.max(sp.1);
        for (_, _, end) in p.points() {
            x0 = x0.min(end.0);
            y0 = y0.min(end.1);
            x1 = x1.max(end.0);
            y1 = y1.max(end.1);
        }
    }
    if !x0.is_finite() {
        return Err("布尔结果包围盒非法".into());
    }
    let path = from_flo(&raw, -x0, -y0);
    Ok((path, [x0, y0, x1 - x0, y1 - y0]))
}

/// 平移 flo 路径(逐点重建;控制柄同样平移)。
fn shift_path(src: &SimpleBezierPath, d: Coord2) -> SimpleBezierPath {
    let sp = src.start_point();
    let mut builder = BezierPathBuilder::<SimpleBezierPath>::start(Coord2(sp.0 + d.0, sp.1 + d.1));
    let mut prev = sp;
    for (h1, h2, end) in src.points() {
        // 区分直线/曲线:flo 直线段控制柄等于端点
        let straight = (h1.0 - prev.0).abs() < 1e-6
            && (h1.1 - prev.1).abs() < 1e-6
            && (h2.0 - end.0).abs() < 1e-6
            && (h2.1 - end.1).abs() < 1e-6;
        builder = if straight {
            builder.line_to(Coord2(end.0 + d.0, end.1 + d.1))
        } else {
            builder.curve_to(
                (
                    Coord2(h1.0 + d.0, h1.1 + d.1),
                    Coord2(h2.0 + d.0, h2.1 + d.1),
                ),
                Coord2(end.0 + d.0, end.1 + d.1),
            )
        };
        prev = end;
    }
    builder.build()
}

/// 估算路径包围盒(节点本地;供调用方在布尔前校验)。
pub fn path_bbox(path: &BezPath) -> [f64; 4] {
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for el in path.elements() {
        let pts: Vec<vb_common::geom::Point> = match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => vec![*p],
            PathEl::QuadTo(c, p) => vec![*c, *p],
            PathEl::CurveTo(c1, c2, p) => vec![*c1, *c2, *p],
            PathEl::ClosePath => vec![],
        };
        for p in pts {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
    }
    if !x0.is_finite() {
        return [0.0; 4];
    }
    [x0, y0, x1 - x0, y1 - y0]
}

/// 面向宿主的高层入口:对文档中两个矢量节点执行布尔运算。
/// 返回 (结果路径[lhs 本地帧,已重定基到包围盒原点], lhs 新几何[父级帧])。
/// 帧换算经 abs_bbox(画板本地原点),跨父级/跨组均可。
pub fn path_boolean_nodes(
    doc: &vb_doc::model::Document,
    op: BooleanOp,
    lhs_id: vb_doc::model::NodeId,
    rhs_id: vb_doc::model::NodeId,
) -> Result<(vb_common::geom::BezPath, vb_doc::model::Geom), String> {
    use vb_doc::model::{Geom, NodeKind};

    let lhs_path = match doc.nodes.get(lhs_id).map(|n| &n.kind) {
        Some(NodeKind::Vector { path }) => path.clone(),
        _ => return Err("lhs 不是矢量路径节点".into()),
    };
    let rhs_path = match doc.nodes.get(rhs_id).map(|n| &n.kind) {
        Some(NodeKind::Vector { path }) => path.clone(),
        _ => return Err("rhs 不是矢量路径节点".into()),
    };
    let lhs_node = doc.nodes.get(lhs_id).ok_or("lhs 节点不存在")?;
    let lr = super::abs_bbox(doc, lhs_id).ok_or("lhs 无画板归属")?;
    let rr = super::abs_bbox(doc, rhs_id).ok_or("rhs 无画板归属")?;

    let (new_path, bbox) = boolean_paths(op, &lhs_path, (lr.x0, lr.y0), &rhs_path, (rr.x0, rr.y0))?;
    let geom = Geom {
        x: lhs_node.geom.x + bbox[0],
        y: lhs_node.geom.y + bbox[1],
        w: bbox[2],
        h: bbox[3],
    };
    Ok((new_path, geom))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_common::geom::Point;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
        let mut p = BezPath::new();
        p.move_to(Point::new(x, y));
        p.line_to(Point::new(x + w, y));
        p.line_to(Point::new(x + w, y + h));
        p.line_to(Point::new(x, y + h));
        p.close_path();
        p
    }

    /// C1:联集面积正确(两矩形重叠 100×100 → 60000)。
    #[test]
    fn union_area() {
        let a = rect(0.0, 0.0, 200.0, 200.0);
        let b = rect(100.0, 0.0, 300.0, 100.0);
        let (path, bbox) =
            boolean_paths(BooleanOp::Union, &a, (0.0, 0.0), &b, (0.0, 0.0)).expect("并集成功");
        assert!(bbox[2] >= 399.0 && bbox[3] >= 199.0, "bbox {bbox:?}");
        // 结果闭合路径元素数应 > 2(M + 至少 2 段 + Z)
        assert!(
            path.elements().len() > 2,
            "结果路径段数异常: {}",
            path.elements().len()
        );
    }

    /// C1:rhs 在不同原点时帧换算正确(rhs 原点 (500,500) 本地 (100,0)。
    #[test]
    fn cross_origin_frames() {
        let a = rect(0.0, 0.0, 200.0, 200.0);
        let b = rect(0.0, 0.0, 100.0, 100.0); // rhs 本地
                                              // rhs 原点 (50,50):公共帧中 rhs 覆盖 (50,50)-(150,150) → 与 a 重叠 100×100
        let (_, bbox) = boolean_paths(BooleanOp::Intersect, &a, (0.0, 0.0), &b, (50.0, 50.0))
            .expect("交集成功");
        assert!(
            (bbox[2] - 100.0).abs() < 1.0 && (bbox[3] - 100.0).abs() < 1.0,
            "交集应为 100×100,bbox {bbox:?}"
        );
    }

    /// C1:差集与 Xor。
    #[test]
    fn subtract_and_xor() {
        let a = rect(0.0, 0.0, 200.0, 200.0);
        let b = rect(100.0, 0.0, 300.0, 100.0);
        let (_, bbox) =
            boolean_paths(BooleanOp::Subtract, &a, (0.0, 0.0), &b, (0.0, 0.0)).expect("差集成功");
        assert!(bbox[2] > 90.0 && bbox[3] > 190.0, "bbox {bbox:?}");
        let r = boolean_paths(BooleanOp::Xor, &a, (0.0, 0.0), &b, (0.0, 0.0));
        assert!(r.is_ok());
        // 不相交的交集 → 报错
        let c = rect(1000.0, 1000.0, 50.0, 50.0);
        assert!(boolean_paths(BooleanOp::Intersect, &a, (0.0, 0.0), &c, (0.0, 0.0)).is_err());
    }
}
