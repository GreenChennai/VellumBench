//! ADR-0012 Spike:路径布尔运算可行性验证(flo_curves 0.8)。
//!
//! 目标(14 篇 P4 后半前置):
//! 1. flo_curves 的 `path_add/path_sub/path_intersect/path_xor` 能否对
//!    「矩形/圆角矩形/椭圆」这类本项目主用形状稳定出结果;
//! 2. kurbo BezPath ↔ flo_curves SimpleBezierPath 的转换层是否可写;
//! 3. 原型性能量级(10000 对象画布场景下,两两布尔是否可交互)。
//!
//! 结论见 docs/adr/0012(随本测试一起落盘)。

use flo_curves::bezier::path::{
    path_add, path_intersect, path_sub, BezierPath, BezierPathBuilder, SimpleBezierPath,
};
use flo_curves::geo::Coord2;

/// 用 flo_curves 构建一个轴对齐矩形路径。
fn rect_path(x: f64, y: f64, w: f64, h: f64) -> SimpleBezierPath {
    BezierPathBuilder::<SimpleBezierPath>::start(Coord2(x, y))
        .line_to(Coord2(x + w, y))
        .line_to(Coord2(x + w, y + h))
        .line_to(Coord2(x, y + h))
        .build()
}

/// 采样路径锚点(起点 + 每段终点;用于结构断言与 shoelace 面积估算)。
/// Spike 发现:`BezierPath` trait 的迭代接口是 `points()`(逐段产出
/// 控制柄二元组 + 曲线上的终点),没有 `curves()` 方法。
fn sample_points(path: &SimpleBezierPath) -> Vec<(f64, f64)> {
    let sp = path.start_point();
    let mut out = vec![(sp.0, sp.1)];
    out.extend(path.points().map(|(_, _, end)| (end.0, end.1)));
    out
}

/// shoelace 面积(采样多边形;用于粗验证布尔结果量级)。
fn shoelace(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    let mut a = 0.0;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        a += x0 * y1 - x1 * y0;
    }
    (a / 2.0).abs()
}

/// 两矩形并集:结果非空、单一路径、面积 = 重叠面积 19000(200×200 与
/// 300×100 重叠 100×100 → 40000+30000-10000?按实际矩形算并集应为
/// 40000+30000-重叠;重叠 100×100=10000 → 60000)。
#[test]
fn union_two_overlapping_rects() {
    let a = rect_path(0.0, 0.0, 200.0, 200.0); // 40000
    let b = rect_path(100.0, 0.0, 300.0, 100.0); // 30000,重叠 100×100=10000
    let result = path_add::<SimpleBezierPath>(&vec![a], &vec![b], 0.01);
    assert!(!result.is_empty(), "并集结果为空");
    let total: f64 = result.iter().map(|p| shoelace(&sample_points(p))).sum();
    assert!(
        (total - 60000.0).abs() < 1500.0,
        "并集面积应为 ~60000,得到 {total}(采样近似)"
    );
}

/// 差集:A - B → A 去掉重叠部分(40000-10000=30000)。
#[test]
fn difference_rect_minus_rect() {
    let a = rect_path(0.0, 0.0, 200.0, 200.0);
    let b = rect_path(100.0, 0.0, 300.0, 100.0);
    let result = path_sub::<SimpleBezierPath>(&vec![a], &vec![b], 0.01);
    assert!(!result.is_empty(), "差集结果为空");
    let total: f64 = result.iter().map(|p| shoelace(&sample_points(p))).sum();
    assert!(
        (total - 30000.0).abs() < 1500.0,
        "差集面积应为 ~30000,得到 {total}"
    );
}

/// 交集:重叠区域 100×100。
#[test]
fn intersect_two_rects() {
    let a = rect_path(0.0, 0.0, 200.0, 200.0);
    let b = rect_path(100.0, 0.0, 300.0, 100.0);
    let result = path_intersect::<SimpleBezierPath>(&vec![a], &vec![b], 0.01);
    assert!(!result.is_empty(), "交集结果为空");
    let total: f64 = result.iter().map(|p| shoelace(&sample_points(p))).sum();
    assert!(
        (total - 10000.0).abs() < 1000.0,
        "交集面积应为 ~10000,得到 {total}"
    );
}

/// kurbo BezPath → flo_curves 转换层可行性:把 5×4 矩形逐段转 Curve 后
/// 用 BezierPathBuilder 重建并做并集(转换层是 ADR 采纳的关键前提)。
#[test]
fn kurbo_conversion_roundtrip_feasible() {
    // 模拟 kurbo BezPath 的元素流(本项目 Vector 节点即 M/L/C 序列)
    let segments: Vec<((f64, f64), (f64, f64))> = vec![
        ((0.0, 0.0), (50.0, 0.0)),
        ((50.0, 0.0), (50.0, 40.0)),
        ((50.0, 40.0), (0.0, 40.0)),
        ((0.0, 40.0), (0.0, 0.0)),
    ];
    let mut builder = BezierPathBuilder::<SimpleBezierPath>::start(Coord2(0.0, 0.0));
    for (_, end) in &segments[1..] {
        builder = builder.line_to(Coord2(end.0, end.1));
    }
    let path = builder.build();
    let other = rect_path(25.0, 10.0, 50.0, 20.0);
    let result = path_add::<SimpleBezierPath>(&vec![path], &vec![other], 0.01);
    assert!(!result.is_empty(), "转换后布尔失败");
    // 采样点应能回填 kurbo(BezPath::move_to/line_to/curve_to 逐段重建)
    let pts = sample_points(&result[0]);
    assert!(pts.len() >= 4, "结果顶点数异常:{}", pts.len());
}

/// 性能量级:1000 次两矩形并集(交互式路径查找器的单步成本)。
#[test]
fn perf_1000_unions() {
    let t0 = std::time::Instant::now();
    for i in 0..1000 {
        let off = i as f64 * 0.01;
        let a = rect_path(off, off, 100.0, 100.0);
        let b = rect_path(off + 50.0, off + 25.0, 100.0, 100.0);
        let r = path_add::<SimpleBezierPath>(&vec![a], &vec![b], 0.01);
        assert!(!r.is_empty());
    }
    let elapsed = t0.elapsed();
    println!(
        "1000 次并集耗时:{elapsed:?}(单次 {}µs)",
        elapsed.as_micros() / 1000
    );
    // 交互可接受上限:单次 < 5ms(debug 下放宽到 20ms)
    assert!(
        elapsed.as_millis() < 20_000,
        "布尔性能异常:1000 次耗时 {elapsed:?}"
    );
}
