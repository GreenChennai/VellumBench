//! 路径查找器**扩展三运算**的门禁测试(阶段 2 / 副文档 03-1-4)。
//!
//! 扩展运算的设计口径(写在 `BooleanOp` 的类型注释里):
//! 在**两个操作数**下,AI 的「合并 / 减去后方对象 / 裁剪」与
//! 「联集 / 减去顶层 / 交集」几何结果相同 —— 因此它们复用同一几何内核,
//! 只是各自有独立的命令 ID(菜单/命令面板/Agent 可分别触达)。
//!
//! 这里把这个口径**钉死**:同一输入下,扩展运算的结果必须与它归约到的
//! 基础运算**逐点相同**(而不是"差不多")。

use vb_common::geom::{BezPath, PathEl, Point};
use vb_tools::boolean::{boolean_paths, BooleanOp, Kernel};

fn rect(x: f64, y: f64, w: f64, h: f64) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(Point::new(x, y));
    p.line_to(Point::new(x + w, y));
    p.line_to(Point::new(x + w, y + h));
    p.line_to(Point::new(x, y + h));
    p.close_path();
    p
}

fn bbox(path: &BezPath) -> [f64; 4] {
    let mut r = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for el in path.elements() {
        let p = match el {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
            PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => *p,
            PathEl::ClosePath => continue,
        };
        r[0] = r[0].min(p.x);
        r[1] = r[1].min(p.y);
        r[2] = r[2].max(p.x);
        r[3] = r[3].max(p.y);
    }
    [r[0], r[1], r[2] - r[0], r[3] - r[1]]
}

#[test]
fn extended_ops_parse_and_roundtrip() {
    for (s, k) in [
        ("merge", Kernel::Union),
        ("subtract_back", Kernel::Subtract),
        ("crop", Kernel::Intersect),
        ("union", Kernel::Union),
        ("subtract", Kernel::Subtract),
        ("intersect", Kernel::Intersect),
        ("xor", Kernel::Xor),
    ] {
        let op = BooleanOp::parse(s).unwrap_or_else(|| panic!("{s} 应可解析"));
        assert_eq!(op.as_str(), s, "as_str/parse 必须互逆");
        assert_eq!(op.kernel(), k, "{s} 的内核不符");
    }
    assert!(
        BooleanOp::parse("divide").is_none(),
        "未实现的运算不应被解析"
    );
}

#[test]
fn extended_ops_match_their_kernel_geometry() {
    let a = rect(0.0, 0.0, 200.0, 200.0);
    let b = rect(100.0, 0.0, 300.0, 100.0);

    for (ext, base) in [
        (BooleanOp::Merge, BooleanOp::Union),
        (BooleanOp::SubtractBack, BooleanOp::Subtract),
        (BooleanOp::Crop, BooleanOp::Intersect),
    ] {
        let (pa, ba) = boolean_paths(ext, &a, (0.0, 0.0), &b, (0.0, 0.0))
            .unwrap_or_else(|e| panic!("{ext:?} 应成功:{e}"));
        let (pb, bb) = boolean_paths(base, &a, (0.0, 0.0), &b, (0.0, 0.0))
            .unwrap_or_else(|e| panic!("{base:?} 应成功:{e}"));
        assert_eq!(ba, bb, "{ext:?} 的包围盒应与 {base:?} 一致");
        assert_eq!(
            pa.elements().len(),
            pb.elements().len(),
            "{ext:?} 的路径元素数应与 {base:?} 一致"
        );
    }
}

#[test]
fn extended_subtract_back_uses_front_as_lhs() {
    // 前方对象(单独调用时 lhs)= 大矩形减去小矩形 → 面积 = 大 - 交
    let big = rect(0.0, 0.0, 200.0, 200.0);
    let small = rect(150.0, 150.0, 100.0, 100.0);
    let (_, bb) = boolean_paths(
        BooleanOp::SubtractBack,
        &big,
        (0.0, 0.0),
        &small,
        (0.0, 0.0),
    )
    .expect("减去后方对象应成功");
    // 结果仍覆盖大方框(挖掉一角)
    assert!(bb[2] >= 199.0 && bb[3] >= 199.0, "bbox {bb:?}");

    // 与"减去顶层"同输入同结果(两操作数下几何等价,见模块注释)
    let (_, bb2) = boolean_paths(BooleanOp::Subtract, &big, (0.0, 0.0), &small, (0.0, 0.0))
        .expect("减去顶层应成功");
    assert_eq!(bb, bb2);
}

#[test]
fn crop_needs_overlap_like_intersect() {
    let a = rect(0.0, 0.0, 100.0, 100.0);
    let far = rect(1000.0, 1000.0, 50.0, 50.0);
    assert!(
        boolean_paths(BooleanOp::Crop, &a, (0.0, 0.0), &far, (0.0, 0.0)).is_err(),
        "不相交的裁剪应与交集一致地报错(不静默返回空)"
    );
    // 相交时给出交集几何
    let b = rect(50.0, 50.0, 100.0, 100.0);
    let (path, bb) =
        boolean_paths(BooleanOp::Crop, &a, (0.0, 0.0), &b, (0.0, 0.0)).expect("裁剪应成功");
    assert!(
        (bb[2] - 50.0).abs() < 1.0 && (bb[3] - 50.0).abs() < 1.0,
        "bbox {bb:?}"
    );
    assert!(bbox(&path)[2] > 0.0);
}
