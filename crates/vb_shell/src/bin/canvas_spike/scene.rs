//! 代表性画布场景生成(ADR-0047 spike)。
//!
//! 确定性(LCG,无 rand 依赖)生成覆盖 22 篇 §4 R0 判据全要素的 DrawList:
//! 矩形(≥3000)+ 贝塞尔曲线(开放描边 / 闭合填充)+ 文字(含 CJK)+
//! 多停靠点线性渐变 + 分层不透明度 + 少量旋转/圆角/椭圆。
//! 默认 10000 节点对齐「1080p 下 1 万节点 60fps」判据。

use sable::kurbo::{BezPath, Point as KPoint};
use vb_render::encode::{BorderDef, DrawItem, DrawKind, DrawList, FillDef, GradientStop, TextHint};

/// 画板尺寸(世界坐标 px)。
pub const ARTBOARD: [f64; 2] = [1920.0, 1080.0];

/// LCG 确定性伪随机(64 位 Parks–Miller 变体;种子固定保证可复现)。
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed | 1)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// [0, 1) 均匀。
    fn f(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// [lo, hi) 均匀。
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.f() * (hi - lo)
    }
    fn pick<'a, T>(&mut self, slice: &'a [T]) -> &'a T {
        &slice[(self.next_u64() % slice.len() as u64) as usize]
    }
}

/// 一批高饱和填充色(区分相邻矩形)。
const PALETTE: [[f32; 4]; 10] = [
    [0.85, 0.32, 0.28, 1.0],
    [0.95, 0.62, 0.25, 1.0],
    [0.99, 0.88, 0.35, 1.0],
    [0.45, 0.78, 0.35, 1.0],
    [0.26, 0.68, 0.64, 1.0],
    [0.27, 0.55, 0.85, 1.0],
    [0.45, 0.37, 0.80, 1.0],
    [0.78, 0.40, 0.73, 1.0],
    [0.92, 0.94, 0.96, 1.0],
    [0.20, 0.24, 0.30, 1.0],
];

/// CJK + 拉丁混合文案池(字形覆盖检查用)。
const TEXTS: [&str; 6] = [
    "vellum 画布渲染",
    "羊皮纸 VellumBench 通道裁定",
    "缩放无糊 4x AaBb 0123",
    "vello·parley·skrifa 矢量文本",
    "GPUI 原生描线 Spike",
    "硬骨头 #7:4096 上限",
];

/// 节点桶(场景配比;与 DrawKind 解耦,生成器内部用)。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bucket {
    Text,
    SolidBox,
    GradientBox,
    RoundedBorderBox,
    Ellipse,
    StrokeCurve,
    Blob,
}

/// 生成 `nodes` 个节点的确定性场景。
///
/// 配比(对齐判据全要素):实心矩形 55% / 线性渐变矩形 10% / 圆角描边矩形
/// 10% / 椭圆 10% / 贝塞尔描边曲线 8% / 贝塞尔填充斑 4% / 文本 3%;
/// 其中约 1/8 项带 ±30° 旋转,不透明度 0.25–1.0 逐项分布。
pub fn build_draw_list(nodes: usize) -> DrawList {
    let mut rng = Lcg::new(0x9E37_79B9_7F4A_7C15);
    let [aw, ah] = ARTBOARD;
    let mut items: Vec<DrawItem> = Vec::with_capacity(nodes);
    for i in 0..nodes {
        let t = rng.f();
        let rot = if rng.f() < 0.125 {
            rng.range(-30.0, 30.0)
        } else {
            0.0
        };
        let opacity = rng.range(0.25, 1.0) as f32;
        let w = rng.range(24.0, 180.0);
        let h = rng.range(24.0, 180.0);
        let x = rng.range(0.0, aw - w);
        let y = rng.range(0.0, ah - h);
        let color = *rng.pick(&PALETTE);

        let bucket = if i.is_multiple_of(33) {
            Bucket::Text
        } else {
            pick_bucket(t)
        };
        let place = Placement {
            x,
            y,
            w,
            h,
            rot,
            opacity,
            color,
        };

        let item = match bucket {
            Bucket::Text => text_item(&mut rng, x, y, color, opacity),
            Bucket::StrokeCurve => stroke_curve(&mut rng, x, y, w, h, color, opacity),
            Bucket::Blob => filled_blob(&mut rng, x, y, w, h, color, opacity),
            _ => box_item(&mut rng, i, bucket, place),
        };
        items.push(item);
    }

    DrawList {
        w: aw,
        h: ah,
        background: [0.08, 0.09, 0.10, 1.0],
        items,
    }
}

/// t ∈ [0,1) → 节点桶(区间口径,与模块注释配比一致)。
fn pick_bucket(t: f64) -> Bucket {
    match t {
        x if (0.0..0.55).contains(&x) => Bucket::SolidBox,
        x if (0.55..0.65).contains(&x) => Bucket::GradientBox,
        x if (0.65..0.75).contains(&x) => Bucket::RoundedBorderBox,
        x if (0.75..0.85).contains(&x) => Bucket::Ellipse,
        x if (0.85..0.93).contains(&x) => Bucket::StrokeCurve,
        _ => Bucket::Blob,
    }
}

/// 一次放置(几何 + 样式打包,压函数参数数)。
struct Placement {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    rot: f64,
    opacity: f32,
    color: [f32; 4],
}

fn box_item(rng: &mut Lcg, i: usize, bucket: Bucket, p: Placement) -> DrawItem {
    let mut item = DrawItem {
        sid: format!("spike-{i}"),
        rect: [p.x, p.y, p.w, p.h],
        ellipse: bucket == Bucket::Ellipse,
        radii: [0.0; 4],
        fill: Some(FillDef::Solid(p.color)),
        border: None,
        opacity: p.opacity,
        layer: 0,
        kind: DrawKind::Box,
        label: None,
        src: None,
        rot: p.rot,
        path: None,
        image: None,
        clip: None,
        overflow_clip: None,
        filter: None,
    };
    if bucket == Bucket::RoundedBorderBox {
        // 圆角 + 描边
        let r = rng.range(6.0, p.w.min(p.h) / 3.0);
        item.radii = [r; 4];
        item.border = Some(BorderDef {
            width: rng.range(1.0, 3.0),
            color: [0.06, 0.07, 0.09, 1.0],
        });
    } else if bucket == Bucket::GradientBox {
        // 4 停靠点线性渐变(CSS 角度语义;路线 (c) 只能 2 停靠点近似,
        // 端点取首尾停靠点 —— 保真差有意保留给 ADR 记录)。
        item.fill = Some(FillDef::LinearGradient {
            angle_css: *rng.pick(&[0.0, 45.0, 90.0, 135.0, 180.0]),
            stops: vec![
                GradientStop {
                    pos: 0.0,
                    color: p.color,
                },
                GradientStop {
                    pos: 0.35,
                    color: [1.0, 1.0, 1.0, 0.9],
                },
                GradientStop {
                    pos: 0.7,
                    color: shade(p.color, 0.55),
                },
                GradientStop {
                    pos: 1.0,
                    color: [0.05, 0.06, 0.08, 1.0],
                },
            ],
        });
    }
    item
}

/// 颜色乘系数(渐变中段用)。
fn shade(c: [f32; 4], k: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, c[3]]
}

/// 开放贝塞尔描边曲线(三次段 ×3,屏宽描边 1–3px)。
fn stroke_curve(
    rng: &mut Lcg,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    color: [f32; 4],
    opacity: f32,
) -> DrawItem {
    let mut path = BezPath::new();
    path.move_to(KPoint::new(rng.range(0.0, w), rng.range(0.0, h)));
    for _ in 0..3 {
        let c1 = KPoint::new(rng.range(0.0, w), rng.range(0.0, h));
        let c2 = KPoint::new(rng.range(0.0, w), rng.range(0.0, h));
        let to = KPoint::new(rng.range(0.0, w), rng.range(0.0, h));
        path.curve_to(c1, c2, to);
    }
    DrawItem {
        sid: String::new(),
        rect: [x, y, w, h],
        ellipse: false,
        radii: [0.0; 4],
        fill: None,
        border: Some(BorderDef {
            width: rng.range(1.0, 3.0),
            color,
        }),
        opacity,
        layer: 0,
        kind: DrawKind::VectorPath,
        label: None,
        src: None,
        rot: 0.0,
        path: Some(path),
        image: None,
        clip: None,
        overflow_clip: None,
        filter: None,
    }
}

/// 闭合贝塞尔填充斑(三次段闭合环)。
fn filled_blob(
    _rng: &mut Lcg,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    color: [f32; 4],
    opacity: f32,
) -> DrawItem {
    let mut path = BezPath::new();
    let p0 = KPoint::new(w / 2.0, 0.0);
    path.move_to(p0);
    let k = 0.55;
    // 四段三次闭合(菱形锚点 + 切向控制柄,近似平滑环)。
    let anchors = [
        KPoint::new(w / 2.0, 0.0),
        KPoint::new(w, h / 2.0),
        KPoint::new(w / 2.0, h),
        KPoint::new(0.0, h / 2.0),
    ];
    for a in 0..4 {
        let cur = anchors[a];
        let nxt = anchors[(a + 1) % 4];
        let c1 = KPoint::new(cur.x + k * (nxt.x - cur.x), cur.y + k * (nxt.y - cur.y));
        let c2 = KPoint::new(nxt.x - k * (nxt.x - cur.x), nxt.y - k * (nxt.y - cur.y));
        path.curve_to(c1, c2, nxt);
    }
    path.close_path();
    DrawItem {
        sid: String::new(),
        rect: [x, y, w, h],
        ellipse: false,
        radii: [0.0; 4],
        fill: Some(FillDef::Solid(color)),
        border: None,
        opacity,
        layer: 0,
        kind: DrawKind::VectorPath,
        label: None,
        src: None,
        rot: 0.0,
        path: Some(path),
        image: None,
        clip: None,
        overflow_clip: None,
        filter: None,
    }
}

/// 文本项(CJK;DrawKind::Text + TextHint,gpu 编码器画占位、
/// 路线 (b)/(c) 各自出真文本 —— 这正是三路文本能力对比点)。
fn text_item(rng: &mut Lcg, x: f64, y: f64, color: [f32; 4], opacity: f32) -> DrawItem {
    let text = *rng.pick(&TEXTS);
    let size = *rng.pick(&[14.0, 18.0, 24.0, 32.0]);
    let bold = rng.f() < 0.3;
    DrawItem {
        sid: String::new(),
        rect: [x, y, size * 8.0, size * 1.4],
        ellipse: false,
        radii: [0.0; 4],
        fill: None,
        border: None,
        opacity,
        layer: 0,
        kind: DrawKind::Text,
        label: Some(TextHint {
            text: text.to_string(),
            font_size: size,
            color,
            weight_bold: bold,
            weight: if bold { 700 } else { 400 },
            font_family: "Segoe UI".to_string(),
            line_height: size * 1.4,
            letter_spacing: 0.0,
            segments: Vec::new(),
        }),
        src: None,
        rot: 0.0,
        path: None,
        image: None,
        clip: None,
        overflow_clip: None,
        filter: None,
    }
}
