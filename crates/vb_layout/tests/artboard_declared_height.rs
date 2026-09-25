//! P0-③ 回归(副文档 03 §2-1):画板声明高在**文档模型**与**画布绘制矩形**
//! 两层都必须等于 CSS 声明值,不得被内容流高覆盖。
//!
//! 最小复现(03-1-1):一个 `height:520px` 的画板,内含 `top:96px;
//! height:300px` 的卡片 —— 此前实测画布上画板只剩约 180~202px 可见,
//! 卡片被从中间截断,而同一文档 CPU 导出是正确的 520px。
//!
//! 断言口径(03-1-2):
//! ① 文档模型画板高度 = 520;
//! ② 画布绘制用的画板矩形(`encode_artboard` 的 DrawList 宽高与背景矩形)= 520;
//! ③ 绘制裁剪边界 = 画板矩形(内容矩形都落在画板矩形内,不存在第二裁剪矩形)。
//!
//! 根因备注:这三条几何断言在修复前即绿 —— 缺陷不在布局求值,而在 vb_app
//! 画布视口矩形(`canvas_rect_next`,旧实现"旧矩形∩当前矩形"只缩不涨,
//! GPU 纹理停在首帧尺寸,首屏之外的画板画不出来)。几何回归 + 视口回归
//! (`canvas_rect_tracks_window_growth`)两处共同锁死该缺陷。

use vb_doc::import::import_html;
use vb_doc::model::NodeKind;
use vb_render::encode::encode_artboard;

fn layout_all(html: &str) -> vb_doc::model::Document {
    let dir = std::env::temp_dir().join(format!(
        "vb-ab-h-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .elapsed()
            .unwrap_or_default()
            .as_nanos() as u64
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let r = import_html(html, &dir).expect("导入");
    let mut doc = r.doc;
    vb_layout::apply_import_layout(&mut doc, Some(&dir), r.synthetic_artboard);
    let _ = std::fs::remove_dir_all(&dir);
    doc
}

/// 03-1-1/03-1-2:最小复现 —— 520px 画板 + 300px 卡片。
#[test]
fn artboard_keeps_declared_height_minimal() {
    let html = r#"<!DOCTYPE html><html><head><meta charset="UTF-8"><style>
.ab { position:relative; overflow:hidden; width:1440px; height:520px; background:#faf6f1; }
.card { position:absolute; top:96px; width:360px; height:300px; background:#fff; }
</style></head><body>
<section class="vb-artboard ab" data-vb-name="A">
  <div class="card" data-vb-name="卡"></div>
</section>
</body></html>"#;
    let doc = layout_all(html);
    let ab = doc.artboards[0];

    // ① 文档模型:画板高 = 声明 520
    let abn = doc.node(ab).unwrap();
    assert!(
        (abn.geom.h - 520.0).abs() < 0.5,
        "画板文档模型高应为 520,实际 {}",
        abn.geom.h
    );
    assert!(
        (abn.geom.w - 1440.0).abs() < 0.5,
        "画板文档模型宽应为 1440,实际 {}",
        abn.geom.w
    );

    // ② 画布绘制矩形:DrawList 尺寸 = 画板声明尺寸,背景矩形 = 画板矩形
    let list = encode_artboard(&doc, ab).expect("编码画板");
    assert!(
        (list.h - 520.0).abs() < 0.5 && (list.w - 1440.0).abs() < 0.5,
        "画布绘制 DrawList 应为 1440x520,实际 {}x{}",
        list.w,
        list.h
    );
    let bg = list.items.first().expect("画板背景矩形");
    assert_eq!(
        bg.rect,
        [0.0, 0.0, 1440.0, 520.0],
        "画板背景矩形 = 画板矩形"
    );

    // ③ 裁剪边界 = 画板矩形:卡片(top:96 h:300 → 底 396)整体落在画板矩形内,
    //    画布上可见高度由画板矩形决定,不存在更小的内容流裁剪
    let card = doc
        .nodes
        .iter()
        .find(|(_, n)| n.classes.iter().any(|c| c == "card"))
        .map(|(_, n)| (n.geom.x, n.geom.y, n.geom.w, n.geom.h))
        .expect("卡片节点");
    assert_eq!(card, (0.0, 96.0, 360.0, 300.0), "卡片矩形 = 布局求值结果");
    assert!(
        card.0 >= 0.0 && card.1 >= 0.0 && card.0 + card.2 <= 1440.0 && card.1 + card.3 <= 520.0,
        "卡片必须整体落在画板矩形内(裁剪边界=画板矩形): {card:?}"
    );
}

/// 03-1-4/03-1-5:examples/landing 回归 —— 多画板时**每个**画板的
/// 原点、尺寸、绘制矩形、内容边界都正确(不止修第二个)。
#[test]
fn landing_artboards_match_declared_geometry() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let dir = std::path::Path::new(&manifest)
        .join("../../examples/landing")
        .canonicalize()
        .expect("examples/landing 存在");
    let r = vb_doc::import::import_project(&dir).expect("导入 landing");
    let mut doc = r.doc;
    vb_layout::apply_import_layout(&mut doc, Some(&dir), r.synthetic_artboard);

    assert_eq!(doc.artboards.len(), 2, "landing 应有两个画板");
    // 声明尺寸:Hero 1440x600、Features 1440x520
    let expect: [(f64, f64); 2] = [(1440.0, 600.0), (1440.0, 520.0)];
    let mut prev_bottom: Option<f64> = None;
    for (i, &ab) in doc.artboards.iter().enumerate() {
        let n = doc.node(ab).unwrap();
        let (ew, eh) = expect[i];
        assert!(
            (n.geom.w - ew).abs() < 0.5 && (n.geom.h - eh).abs() < 0.5,
            "画板 {i} 文档模型应为 {ew}x{eh},实际 {}x{}",
            n.geom.w,
            n.geom.h
        );
        // 原点:纵向堆叠不重叠(画板间 80px 间距)
        if let Some(pb) = prev_bottom {
            assert!(
                n.geom.y >= pb,
                "画板 {i} 原点应在上一个画板之下: y={} prev_bottom={pb}",
                n.geom.y
            );
        }
        prev_bottom = Some(n.geom.y + n.geom.h);

        // 画布绘制矩形与文档模型一致
        let list = encode_artboard(&doc, ab).expect("编码画板");
        assert!(
            (list.w - n.geom.w).abs() < 0.5 && (list.h - n.geom.h).abs() < 0.5,
            "画板 {i} 绘制矩形 {}x{} 应等于文档模型 {}x{}",
            list.w,
            list.h,
            n.geom.w,
            n.geom.h
        );

        // 该画板全部可见子矩形落在画板矩形内(overflow:hidden 裁剪语义)
        let mut ids = Vec::new();
        doc.subtree(ab, &mut ids);
        for id in ids {
            let sn = doc.node(id).unwrap();
            if sn.hidden || matches!(sn.kind, NodeKind::Artboard) {
                continue;
            }
            // 子几何是父相对坐标;沿祖先链累加得画板本地矩形
            let (mut x, mut y) = (sn.geom.x, sn.geom.y);
            let mut cur = sn.parent;
            while let Some(pid) = cur {
                if pid == ab {
                    break;
                }
                if let Some(pn) = doc.node(pid) {
                    x += pn.geom.x;
                    y += pn.geom.y;
                    cur = pn.parent;
                } else {
                    break;
                }
            }
            assert!(
                x >= -0.5 && y >= -0.5 && x + sn.geom.w <= n.geom.w + 0.5,
                "画板 {i} 子节点 `{}` 本地矩形 ({x},{y},{}, {}) 超出画板 {}x{}",
                sn.name,
                sn.geom.w,
                sn.geom.h,
                n.geom.w,
                n.geom.h
            );
        }
    }
    // Features(第二画板)专项:声明高 520,画布绘制矩形同值
    let features = doc.artboards[1];
    let fn_ = doc.node(features).unwrap();
    assert!((fn_.geom.h - 520.0).abs() < 0.5);
    let list = encode_artboard(&doc, features).expect("编码 Features");
    assert!((list.h - 520.0).abs() < 0.5, "Features 绘制高应为 520");
}
