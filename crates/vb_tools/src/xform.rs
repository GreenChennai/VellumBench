//! 变换与自由绘制几何纯函数(阶段 5 / 05-2,X-4 变换工具族 + X-5 铅笔)。
//!
//! 全部**无副作用**:输入几何/点列,输出几何/点列 —— 工具层(vb_app)
//! 只负责把结果折算成命令。AI 语义以 `docs/design/06` §3.10 / §3.7 为准:
//! - 旋转/缩放:单击设中心,拖拽按中心施加;
//! - 镜像:拖动方向决定镜像轴(水平拖 → 竖直轴左右镜像);
//! - 铅笔:自由绘制后按保真度容差抽稀(RDP)为路径。

use vb_doc::model::Geom;

/// 镜像轴:轴是一条过变换中心的直线。
/// `Vertical` = 竖直轴(左右镜像,翻 X);`Horizontal` = 水平轴(上下镜像,翻 Y)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorAxis {
    Vertical,
    Horizontal,
}

impl MirrorAxis {
    /// 拖动方向 → 镜像轴(06 篇 §3.10「拖动决定镜像轴」):
    /// 以水平/竖直分量较大者为准 —— 水平拖动是左右镜像(竖直轴)。
    pub fn from_drag(dx: f64, dy: f64) -> Self {
        if dx.abs() >= dy.abs() {
            MirrorAxis::Vertical
        } else {
            MirrorAxis::Horizontal
        }
    }
}

/// 点相对中心的角度(弧度,屏幕系 y 向下,CSS 顺时针为正)。
pub fn angle_of(center: (f64, f64), p: (f64, f64)) -> f64 {
    (p.1 - center.1).atan2(p.0 - center.0)
}

/// 缩放系数:起点/当前点到中心的**逐轴**距离比(近零距离保护为 1)。
pub fn scale_factors(center: (f64, f64), start: (f64, f64), cur: (f64, f64)) -> (f64, f64) {
    let axis_k = |c: f64, s: f64, p: f64| {
        let d0 = s - c;
        let d1 = p - c;
        if d0.abs() < 1e-6 {
            1.0
        } else {
            (d1 / d0).clamp(0.01, 100.0)
        }
    };
    (
        axis_k(center.0, start.0, cur.0),
        axis_k(center.1, start.1, cur.1),
    )
}

/// 绕中心按 (kx, ky) 缩放几何(等比由调用方先行归一)。
pub fn scale_geom_about(g: Geom, center: (f64, f64), kx: f64, ky: f64) -> Geom {
    // 左上角与右下角各自关于中心缩放,再归一(允许负系数语义)
    let (x0, y0, x1, y1) = (g.x, g.y, g.x + g.w, g.y + g.h);
    let nx0 = center.0 + (x0 - center.0) * kx;
    let nx1 = center.0 + (x1 - center.0) * kx;
    let ny0 = center.1 + (y0 - center.1) * ky;
    let ny1 = center.1 + (y1 - center.1) * ky;
    Geom {
        x: nx0.min(nx1),
        y: ny0.min(ny1),
        w: (nx1 - nx0).abs().max(1.0),
        h: (ny1 - ny0).abs().max(1.0),
    }
}

/// 几何关于过 `center` 的镜像轴做反射(位置翻到轴对侧;
/// 内容翻转由调用方以 `scaleX/scaleY(-1)` 变换承载)。
pub fn mirror_geom(g: Geom, axis: MirrorAxis, center: (f64, f64)) -> Geom {
    match axis {
        MirrorAxis::Vertical => Geom {
            x: 2.0 * center.0 - (g.x + g.w),
            y: g.y,
            w: g.w,
            h: g.h,
        },
        MirrorAxis::Horizontal => Geom {
            x: g.x,
            y: 2.0 * center.1 - (g.y + g.h),
            w: g.w,
            h: g.h,
        },
    }
}

/// 两点距离。
pub fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// 点到线段距离(RDP 抽稀的判定量)。
fn point_segment_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let len2 = vx * vx + vy * vy;
    if len2 < 1e-12 {
        return dist(p, a);
    }
    // 投影参数截断到 [0,1]
    let t = (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len2).clamp(0.0, 1.0);
    dist(p, (a.0 + t * vx, a.1 + t * vy))
}

/// Ramer–Douglas–Peucker 抽稀(06 篇 §3.7 铅笔「保真度参数 0–20px」):
/// 容差越大点越少(路径越糙),越小越贴合原笔迹。首尾点恒保留。
pub fn rdp_simplify(points: &[(f64, f64)], tol: f64) -> Vec<(f64, f64)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0usize, points.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (points[a], points[b]);
        let mut far = a;
        let mut dmax = -1.0;
        for (i, p) in points.iter().enumerate().take(b).skip(a + 1) {
            let d = point_segment_dist(*p, pa, pb);
            if d > dmax {
                dmax = d;
                far = i;
            }
        }
        if dmax > tol {
            keep[far] = true;
            stack.push((a, far));
            stack.push((far, b));
        }
    }
    (0..points.len())
        .filter(|&i| keep[i])
        .map(|i| points[i])
        .collect()
}

/// 曲率拟合(06 篇 §3.5「点击后曲线自动拟合」):把折线锚点拟合成
/// 平滑三次贝塞尔(Catmull-Rom → Bezier)。锚点全部保留;开放路径首尾
/// 退化为单侧手柄。返回 `None` 表示无可拟合(锚点 < 2)。
pub fn smooth_polyline(pts: &[(f64, f64)], closed: bool) -> Option<BezPath> {
    use vb_common::geom::Point;
    let n = pts.len();
    if n < 2 {
        return None;
    }
    let at = |i: i64| -> (f64, f64) {
        if closed {
            pts[i.rem_euclid(n as i64) as usize]
        } else {
            pts[i.clamp(0, n as i64 - 1) as usize]
        }
    };
    let p = |t: (f64, f64)| Point::new(t.0, t.1);
    let mut path = BezPath::new();
    path.move_to(p(pts[0]));
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let i = i as i64;
        let p0 = at(i - 1);
        let p1 = at(i);
        let p2 = at(i + 1);
        let p3 = at(i + 2);
        // Catmull-Rom 张力 1/6:控制点 = 邻点差 × 1/6
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        path.curve_to(
            vb_common::geom::Point::new(c1.0, c1.1),
            vb_common::geom::Point::new(c2.0, c2.1),
            p(p2),
        );
    }
    if closed {
        path.close_path();
    }
    Some(path)
}

use vb_common::geom::BezPath;

/// 从矢量路径提取锚点序列(终点序)与闭合标志;无锚点返回 None。
/// 曲率工具据此拟合平滑曲线(SetVector 回写)。
pub fn path_anchors(path: &BezPath) -> Option<(Vec<(f64, f64)>, bool)> {
    use vb_common::geom::PathEl;
    let mut pts = Vec::new();
    let mut closed = false;
    for el in path.elements() {
        match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => pts.push((p.x, p.y)),
            PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => pts.push((p.x, p.y)),
            PathEl::ClosePath => closed = true,
        }
    }
    if pts.len() < 2 {
        return None;
    }
    Some((pts, closed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(x: f64, y: f64, w: f64, h: f64) -> Geom {
        Geom { x, y, w, h }
    }

    /// X-4 旋转:角度围绕设定中心度量(单击设中心语义的几何前提)。
    #[test]
    fn angle_of_measures_around_custom_center() {
        // 中心 (100,100),点在正右方 → 0;正下方(CSS y 向下)→ +90°
        assert!((angle_of((100.0, 100.0), (200.0, 100.0)) - 0.0).abs() < 1e-9);
        let a = angle_of((100.0, 100.0), (100.0, 200.0)).to_degrees();
        assert!((a - 90.0).abs() < 1e-9, "y 向下系中正下方应为 +90°:{a}");
    }

    /// X-4 缩放:逐轴距离比;以对象中心外的自定义中心缩放时,
    /// 盒子到中心的近边/远边按同一比例收缩(位置随比例平移)。
    #[test]
    fn scale_about_center_keeps_ratio_and_repositions() {
        let g = geom(100.0, 100.0, 100.0, 50.0);
        // 中心在盒子左侧 (0,125):水平方向 k=0.5,竖直 k=1
        let out = scale_geom_about(g, (0.0, 125.0), 0.5, 1.0);
        // 盒子两角 x:100→50, 200→100 → 新盒 (50,125)-(100,175)
        assert_eq!((out.x, out.y, out.w, out.h), (50.0, 100.0, 50.0, 50.0));
    }

    /// X-4 缩放系数:拖近中心 → <1;拖远 → >1;起点与中心重合 → 保护为 1。
    #[test]
    fn scale_factors_guard_zero_baseline() {
        let (kx, ky) = scale_factors((0.0, 0.0), (100.0, 50.0), (50.0, 25.0));
        assert!((kx - 0.5).abs() < 1e-9 && (ky - 0.5).abs() < 1e-9);
        let (kx, _) = scale_factors((10.0, 10.0), (10.0, 10.0), (30.0, 10.0));
        assert!((kx - 1.0).abs() < 1e-9, "基线距离为 0 时不得除零");
    }

    /// X-4 镜像:竖直轴左右镜像 = x 关于轴对称;水平轴上下镜像 = y 对称。
    #[test]
    fn mirror_geom_reflects_across_axis() {
        let g = geom(120.0, 50.0, 80.0, 40.0);
        // 竖直轴过 x=100:盒 (120..200) → (0..80)
        let m = mirror_geom(g, MirrorAxis::Vertical, (100.0, 0.0));
        assert_eq!((m.x, m.y, m.w, m.h), (0.0, 50.0, 80.0, 40.0));
        // 水平轴过 y=100:盒 y (50..90) → (110..150)
        let m = mirror_geom(g, MirrorAxis::Horizontal, (0.0, 100.0));
        assert_eq!((m.x, m.y, m.w, m.h), (120.0, 110.0, 80.0, 40.0));
    }

    /// X-4 镜像轴判定:水平拖 → 竖直轴(左右镜像);竖直拖 → 水平轴。
    #[test]
    fn mirror_axis_follows_drag_direction() {
        assert_eq!(MirrorAxis::from_drag(80.0, 5.0), MirrorAxis::Vertical);
        assert_eq!(MirrorAxis::from_drag(5.0, 80.0), MirrorAxis::Horizontal);
    }

    /// X-5 铅笔抽稀:笔直段上的中间点被吸收;拐点保留;容差越大点越少。
    #[test]
    fn rdp_simplify_keeps_corners_and_drops_collinear() {
        // 一条 L 形笔迹:中间共线点应被吸收,拐点 (50,50) 保留
        let pts = vec![
            (0.0, 0.0),
            (20.0, 0.0),
            (40.0, 0.0),
            (50.0, 10.0),
            (50.0, 30.0),
            (50.0, 50.0),
        ];
        let out = rdp_simplify(&pts, 2.0);
        // 拐点 (50,10) 与横段远端 (40,0) 都偏离首尾连线 2px 以上 → 保留
        assert_eq!(
            out,
            vec![(0.0, 0.0), (40.0, 0.0), (50.0, 10.0), (50.0, 50.0)]
        );
        // 容差拉大后拐点也被抹平(只剩首尾)
        let rough = rdp_simplify(&pts, 100.0);
        assert_eq!(rough.len(), 2);
        // 噪声抖动(±1px)在 2px 容差下被抹平
        let noisy: Vec<(f64, f64)> = (0..=50)
            .map(|i| (i as f64, if i % 2 == 0 { 0.5 } else { -0.5 }))
            .collect();
        assert_eq!(rdp_simplify(&noisy, 2.0).len(), 2);
    }

    /// X-5 曲率拟合:锚点数守恒、闭合标志保留、控制点确实产出曲线元素。
    #[test]
    fn smooth_polyline_preserves_anchors_and_closes() {
        let pts = vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)];
        let open = smooth_polyline(&pts, false).unwrap();
        use vb_common::geom::PathEl;
        let els = open.elements().to_vec();
        assert!(matches!(els[0], PathEl::MoveTo(_)));
        assert_eq!(
            els.iter()
                .filter(|e| matches!(e, PathEl::CurveTo(..)))
                .count(),
            3,
            "开放路径 n 点 = n-1 段曲线"
        );
        // 终点 = 原末锚点
        match els.last().unwrap() {
            PathEl::CurveTo(_, _, p) => {
                assert!((p.x - 0.0).abs() < 1e-9 && (p.y - 100.0).abs() < 1e-9);
            }
            _ => panic!("末元素应为 CurveTo"),
        }
        let closed = smooth_polyline(&pts, true).unwrap();
        assert!(matches!(closed.elements().last(), Some(PathEl::ClosePath)));
        assert!(smooth_polyline(&pts[..1], false).is_none(), "单点不可拟合");
    }

    /// X-5 path_anchors:从路径反提锚点(曲率工具二次点击的输入)。
    #[test]
    fn path_anchors_extracts_endpoints() {
        let pts = vec![(0.0, 0.0), (100.0, 40.0), (30.0, 90.0)];
        let path = smooth_polyline(&pts, false).unwrap();
        let (anchors, closed) = path_anchors(&path).unwrap();
        assert!(!closed);
        assert_eq!(anchors.len(), 3);
        assert_eq!(anchors[2], (30.0, 90.0));
    }
}
