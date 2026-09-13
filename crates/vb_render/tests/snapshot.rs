//! 渲染快照门禁(设计文档 10 篇门禁 4 简版):
//! 固定场景 → CPU 渲染 → 确定性字节断言 + 关键像素色值检查。
//! 与 golden PNG 对比方案待基线确立后接入(vello_cpu 统一)。

use vb_render::encode::{BorderDef, DrawItem, DrawKind, DrawList, FillDef, GradientStop};

fn fixed_scene() -> DrawList {
    let mut items = vec![
        // 背景
        DrawItem {
            rect: [0.0, 0.0, 200.0, 100.0],
            ellipse: false,
            radii: [0.0; 4],
            fill: Some(FillDef::Solid([1.0, 0.0, 0.0, 1.0])),
            border: None,
            opacity: 1.0,
            kind: DrawKind::Box,
            label: None,
            src: None,
            rot: 0.0,
            path: None,
            image: None,
        },
        // 蓝色圆角矩形
        DrawItem {
            rect: [20.0, 20.0, 80.0, 60.0],
            ellipse: false,
            radii: [8.0; 4],
            fill: Some(FillDef::Solid([0.0, 0.4, 1.0, 1.0])),
            border: Some(BorderDef {
                width: 2.0,
                color: [0.0, 0.0, 0.3, 1.0],
            }),
            opacity: 1.0,
            kind: DrawKind::Box,
            label: None,
            src: None,
            rot: 0.0,
            path: None,
            image: None,
        },
    ];
    items.push(DrawItem {
        rect: [120.0, 20.0, 60.0, 60.0],
        ellipse: true,
        radii: [0.0; 4],
        fill: Some(FillDef::LinearGradient {
            angle_css: 90.0,
            stops: vec![
                GradientStop {
                    pos: 0.0,
                    color: [1.0, 1.0, 0.0, 1.0],
                },
                GradientStop {
                    pos: 1.0,
                    color: [0.0, 1.0, 0.0, 1.0],
                },
            ],
        }),
        border: None,
        opacity: 1.0,
        kind: DrawKind::Box,
        label: None,
        src: None,
        rot: 0.0,
        path: None,
        image: None,
    });
    DrawList {
        w: 200.0,
        h: 100.0,
        background: [1.0, 1.0, 1.0, 1.0],
        items,
    }
}

#[test]
fn snapshot_deterministic_and_content() {
    let list = fixed_scene();
    let a = vb_render::cpu::render_png(&list, 2.0, false, None).expect("渲染 A");
    let b = vb_render::cpu::render_png(&list, 2.0, false, None).expect("渲染 B");
    // 确定性:同场景两次渲染字节一致
    assert_eq!(a.png, b.png, "CPU 渲染必须确定性");
    // PNG 头 + 尺寸
    assert_eq!(&a.png[..8], b"\x89PNG\r\n\x1a\n");
    let img = image::load_from_memory(&a.png).expect("解码");
    assert_eq!(img.width(), 400);
    assert_eq!(img.height(), 200);
    // 关键像素:背景红
    let rgba = img.to_rgba8();
    let px = |x: u32, y: u32| {
        let p = rgba.get_pixel(x, y);
        (p[0], p[1], p[2])
    };
    let (r, g, b) = px(5, 195);
    assert!(r > 200 && g < 100 && b < 100, "背景应偏红: {r},{g},{b}");
    // 蓝色圆角矩形中心
    let (r, g, b) = px(120, 100);
    assert!(b > 200 && r < 100, "矩形应偏蓝: {r},{g},{b}");
}

#[test]
fn snapshot_transparency() {
    let list = fixed_scene();
    let out = vb_render::cpu::render_png(&list, 1.0, true, None).expect("渲染");
    let img = image::load_from_memory(&out.png).expect("解码");
    // 透明模式尺寸不变
    assert_eq!(img.width(), 200);
    assert_eq!(img.height(), 100);
}
