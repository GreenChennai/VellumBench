//! VB-1~VB-5 可观测性回归(第四轮迭代 B2,验收 D–H)。
//!
//! 判据对应(计划文档 §3):
//! - D:native/矢量道丢弃不支持原语 → `UnsupportedPropertyDropped` 带属性名与车道;
//! - E:矢量输出中内联 SVG 栅格化 → `InlineSvgRasterized` 带格式名;
//! - F:命中 D/E 时 `degraded == true`;
//! - G:同属性同车道聚合计数(10 个同属性元素 → 1 条带 count);
//! - 另覆盖 VB-3 动画覆盖矩阵、VB-4 导入跳过强类型、VB-5 按类别聚合。

use vb_doc::model::{Document, Geom, NodeKind};
use vb_kiln::domexport::meta_warnings;
use vb_kiln::dompaint::paintlist_to_document;
use vb_kiln::{ExportRequest, Format, KilnWarning};

/// 造一个带指定内联样式的盒节点画板;`style` 为 (prop, value) 对。
fn doc_with_styles(styles: &[(&str, &str)]) -> Document {
    let mut doc = Document::new_empty("VB 观测样例", "zh-CN");
    let ab = doc.new_artboard("画板 1", 400.0, 300.0);
    let sid = doc.alloc_sid();
    let box_node = doc
        .nodes
        .insert(vb_doc::model::Node::new(NodeKind::Box, "盒", sid));
    {
        let n = doc.nodes.get_mut(box_node).unwrap();
        n.geom = Geom {
            x: 20.0,
            y: 20.0,
            w: 200.0,
            h: 120.0,
        };
        n.style_set("background-color", "#3b82f6");
        for (k, v) in styles {
            n.style_set(k, v);
        }
        doc.nodes.get_mut(ab).unwrap().children.push(box_node);
        doc.nodes.get_mut(box_node).unwrap().parent = Some(ab);
    }
    doc
}

fn req(format: Format) -> ExportRequest {
    ExportRequest {
        format,
        ..Default::default()
    }
}

/// 找到第一条指定种类的 UnsupportedPropertyDropped。
fn dropped<'a>(warnings: &'a [KilnWarning], prop: &str) -> Option<(&'a str, usize, &'a str)> {
    warnings.iter().find_map(|w| match w {
        KilnWarning::UnsupportedPropertyDropped {
            prop: p,
            count,
            lane,
        } if p == prop => Some((p.as_str(), *count, *lane)),
        _ => None,
    })
}

// ---------------------------------------------------------------- 验收 D

/// 验收 D(native 车道):3D transform 丢弃必须带属性名与车道。
#[test]
fn d_native_3d_transform_dropped_with_prop_and_lane() {
    let doc = doc_with_styles(&[("transform", "rotateY(20deg) translateZ(12px)")]);
    let (_, report) =
        vb_kiln::export_artboard(&doc, doc.artboards[0], &req(Format::Png), None).unwrap();
    let hit = dropped(&report.warnings, "transform").expect("transform 丢弃告警缺失");
    assert_eq!(
        hit.2, "native",
        "光栅格式 lane 应为 native:{:?}",
        report.warnings
    );
    assert_eq!(hit.1, 1);
}

/// 验收 D:mask / box-shadow / mix-blend-mode 各自聚合成一条(带属性名)。
#[test]
fn d_native_mask_shadow_blend_each_warn_once() {
    let doc = doc_with_styles(&[
        ("mask", "linear-gradient(#000 50%, transparent 50%)"),
        ("box-shadow", "0 8px 24px rgba(0,0,0,.35)"),
        ("mix-blend-mode", "multiply"),
    ]);
    let (_, report) =
        vb_kiln::export_artboard(&doc, doc.artboards[0], &req(Format::Png), None).unwrap();
    for prop in ["mask", "box-shadow", "mix-blend-mode"] {
        assert!(
            dropped(&report.warnings, prop).is_some(),
            "{prop} 丢弃告警缺失:{:?}",
            report.warnings
        );
    }
    assert_eq!(
        report
            .warnings
            .iter()
            .filter(|w| matches!(w, KilnWarning::UnsupportedPropertyDropped { .. }))
            .count(),
        3,
        "三种属性应各聚合一条"
    );
}

/// 验收 D(矢量车道):同一构造在矢量写出器下 lane = vector;
/// 非矩形 clip-path(path())在编码期退化 → ClipShapeApproximated。
#[test]
fn d_vector_lane_and_unparseable_clip_shape() {
    let doc = doc_with_styles(&[
        ("transform", "perspective(600px) rotateX(15deg)"),
        ("clip-path", "path('M10 10 L 90 90')"),
    ]);
    let (_, report) =
        vb_kiln::export_artboard(&doc, doc.artboards[0], &req(Format::Svg), None).unwrap();
    let hit = dropped(&report.warnings, "transform").expect("transform 丢弃告警缺失");
    assert_eq!(hit.2, "vector", "矢量格式 lane 应为 vector");
    assert!(
        report.warnings.iter().any(
            |w| matches!(w, KilnWarning::ClipShapeApproximated { shape, count: 1 }
                if shape.contains("path("))
        ),
        "path() 裁剪形状退化告警缺失:{:?}",
        report.warnings
    );
}

/// 矢量写出器(SVG/EPS/Ai/PPTX)不表达 clip-path:编码成功解析的
/// inset 裁剪在 SVG 输出中静默丢失 → 交付前聚合告警(PDF 有发射,不告警)。
#[test]
fn vector_writers_warn_on_dropped_clip_path() {
    let doc = doc_with_styles(&[("clip-path", "inset(10px 20px)")]);
    let (_, svg) =
        vb_kiln::export_artboard(&doc, doc.artboards[0], &req(Format::Svg), None).unwrap();
    assert_eq!(
        dropped(&svg.warnings, "clip-path").map(|(_, c, lane)| (c, lane)),
        Some((1, "vector")),
        "SVG 输出丢 clip-path 必须告警:{:?}",
        svg.warnings
    );
    let (_, pdf) =
        vb_kiln::export_artboard(&doc, doc.artboards[0], &req(Format::Pdf), None).unwrap();
    assert!(
        dropped(&pdf.warnings, "clip-path").is_none(),
        "PDF 支持 clip-path,不应告警"
    );
}

// ---------------------------------------------------------------- 验收 E

/// 验收 E:采集计数 → InlineSvgRasterized{format}(domexport 定型层)。
#[test]
fn e_inline_svg_rasterized_with_format() {
    let meta = vb_kiln::dompaint::DomPaintMeta {
        inline_svg_rasterized: 2,
        ..Default::default()
    };
    let w = meta_warnings(&meta, Format::Svg, &[]);
    assert!(w.iter().any(|x| matches!(
        x,
        KilnWarning::InlineSvgRasterized {
            count: 2,
            format: "svg"
        }
    )));
    // PDF 输出 → format 名随目标格式
    let w = meta_warnings(&meta, Format::Pdf, &[]);
    assert!(w
        .iter()
        .any(|x| matches!(x, KilnWarning::InlineSvgRasterized { format: "pdf", .. })));
}

/// 验收 E(采集端):paintlist 中内联 <svg>(kind=svg)落入栅格分支 → 计数。
#[test]
fn e_dompaint_counts_inline_svg_rasterization() {
    let paintlist = serde_json::json!({
        "viewport": [400, 300],
        "bodyWidth": 400.0,
        "items": [
            { "kind": "svg", "rect": [10, 10, 100, 80], "markup": "<svg></svg>" },
            { "kind": "box", "rect": [0, 0, 400, 300], "bg": [1, 1, 1, 1] }
        ],
        "textLineCount": 0,
        "clipDemand": 0
    });
    let dir = std::env::temp_dir().join(format!("vb-obs-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let painted = paintlist_to_document(
        &paintlist,
        &dir,
        None, // 无整页截图 → 走占位分支,但栅格化判定先于截图
        "http://127.0.0.1:0/",
        1.0,
        0.0,
        400.0,
    )
    .unwrap();
    assert_eq!(painted.meta.inline_svg_rasterized, 1, "内联 svg 必须计数");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- 验收 F

/// 验收 F:命中 D/E 时 degraded == true。
#[test]
fn f_degraded_true_on_dropped_or_rasterized() {
    let doc = doc_with_styles(&[("transform", "rotateY(20deg)")]);
    let (_, report) =
        vb_kiln::export_artboard(&doc, doc.artboards[0], &req(Format::Png), None).unwrap();
    assert!(report.degraded, "原语丢弃必须置 degraded");

    let meta = vb_kiln::dompaint::DomPaintMeta {
        inline_svg_rasterized: 1,
        ..Default::default()
    };
    let w = meta_warnings(&meta, Format::Ai, &[]);
    assert!(w.iter().any(KilnWarning::is_degrading));
    // 无降级告警的普通导出不误报
    let plain = doc_with_styles(&[]);
    let (_, report) =
        vb_kiln::export_artboard(&plain, plain.artboards[0], &req(Format::Png), None).unwrap();
    assert!(!report.degraded);
}

// ---------------------------------------------------------------- 验收 G

/// 验收 G:10 个同属性元素 → 1 条告警,count = 10。
#[test]
fn g_same_prop_aggregates_to_one_warning_with_count() {
    let mut doc = Document::new_empty("G 聚合样例", "zh-CN");
    let ab = doc.new_artboard("画板 1", 600.0, 800.0);
    for i in 0..10 {
        let sid = doc.alloc_sid();
        let n = doc.nodes.insert(vb_doc::model::Node::new(
            NodeKind::Box,
            format!("盒{i}"),
            sid,
        ));
        let x = (i % 5) as f64 * 110.0 + 20.0;
        let y = (i / 5) as f64 * 160.0 + 20.0;
        let node = doc.nodes.get_mut(n).unwrap();
        node.geom = Geom {
            x,
            y,
            w: 100.0,
            h: 60.0,
        };
        node.style_set("background-color", "#ef4444");
        node.style_set("box-shadow", "0 4px 12px rgba(0,0,0,.3)");
        doc.nodes.get_mut(ab).unwrap().children.push(n);
        doc.nodes.get_mut(n).unwrap().parent = Some(ab);
    }
    let (_, report) = vb_kiln::export_artboard(&doc, ab, &req(Format::Png), None).unwrap();
    let hits: Vec<_> = report
        .warnings
        .iter()
        .filter(|w| matches!(w, KilnWarning::UnsupportedPropertyDropped { prop, .. } if prop == "box-shadow"))
        .collect();
    assert_eq!(hits.len(), 1, "同属性必须只发一条:{:?}", report.warnings);
    assert_eq!(
        dropped(&report.warnings, "box-shadow").map(|(_, c, _)| c),
        Some(10),
        "count 应聚合为 10"
    );
}

// ---------------------------------------------------------------- VB-3

/// VB-3:覆盖矩阵分类 —— browser 全量动画;native 四类轨道动画,
/// 其余静态回退,`--*` 自定义属性 unsupported。
#[test]
fn anim_coverage_classification_by_lane() {
    let css = [
        "@keyframes rise { 0% { opacity: 0; transform: translatey(40px); } 100% { opacity: 1; transform: none; } }",
        "@keyframes wipe { 0% { clip-path: inset(0 100% 0 0); width: 10px; } 100% { clip-path: inset(0 0 0 0); width: 200px; } }",
        "@keyframes counter { from { --n: 0; } to { --n: 99; } }",
        "@keyframes dash { to { stroke-dashoffset: 0; } }",
    ]
    .map(String::from)
    .to_vec();
    let kf = vb_kiln::anim::parse_keyframes(&css);
    assert_eq!(kf.len(), 4, "四条关键帧都应解析");

    let native = vb_kiln::anim::AnimCoverage::from_keyframes(&kf, "native").unwrap();
    assert_eq!(native.lane, "native");
    assert_eq!(
        native.animated,
        vec!["clip-path", "opacity", "transform"],
        "native 动画化属性:{:?}",
        native.animated
    );
    assert_eq!(
        native.static_fallback,
        vec!["stroke-dashoffset", "width"],
        "静态回退属性:{:?}",
        native.static_fallback
    );
    assert_eq!(native.unsupported, vec!["--n"]);

    let browser = vb_kiln::anim::AnimCoverage::from_keyframes(&kf, "browser").unwrap();
    assert_eq!(browser.lane, "browser");
    assert_eq!(browser.animated.len(), 6, "浏览器道全量动画");
    assert!(browser.static_fallback.is_empty() && browser.unsupported.is_empty());

    assert!(vb_kiln::anim::AnimCoverage::from_keyframes(
        &vb_kiln::anim::parse_keyframes(&[]),
        "native"
    )
    .is_none());
    // JSON 形状(计划文档 VB-3)
    let j = native.to_json();
    assert_eq!(j["lane"], "native");
    assert!(j["animated"]
        .as_array()
        .unwrap()
        .contains(&"opacity".into()));
    assert!(j.get("static_fallback").is_some() && j.get("unsupported").is_some());
}

/// VB-3:无动画声明的导出 anim_coverage 为 None;有动画的导出走 K
/// 车道时报告携带矩阵(native 分类)。
#[test]
fn anim_coverage_attached_to_report() {
    let plain = doc_with_styles(&[]);
    let (_, report) =
        vb_kiln::export_artboard(&plain, plain.artboards[0], &req(Format::Png), None).unwrap();
    assert!(report.anim_coverage.is_none());

    let mut animated = doc_with_styles(&[("animation", "pulse 1s linear both")]);
    // 补 @keyframes(raw_css;与 ExportContext::build 同一解析入口)
    animated
        .raw_css
        .push("@keyframes pulse { 0% { opacity: 0.4; } 100% { opacity: 1; } }".to_string());
    let (_, report) =
        vb_kiln::export_artboard(&animated, animated.artboards[0], &req(Format::Png), None)
            .unwrap();
    let cov = report.anim_coverage.as_ref().expect("应有覆盖矩阵");
    assert_eq!(cov.lane, "native");
    assert!(
        cov.animated.contains(&"opacity".to_string()),
        "{:?}",
        cov.animated
    );
}

// ---------------------------------------------------------------- VB-4

/// VB-4:导入跳过类别 → 强类型 ImportObjectSkipped(计数聚合)。
#[test]
fn import_skips_become_typed_warnings() {
    let dir = std::env::temp_dir().join(format!("vb-obs-import-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let svg = dir.join("in.svg");
    std::fs::write(
        &svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="120">
            <filter id="f"><feGaussianBlur stdDeviation="2"/></filter>
            <mask id="m"><rect width="120" height="120" fill="#fff"/></mask>
            <g filter="url(#f)" mask="url(#m)" transform="skewX(12)">
                <rect x="5" y="5" width="40" height="40" fill="#123456"/>
            </g>
        </svg>"##,
    )
    .unwrap();
    let (_, _, typed) = vb_kiln::import_svg::import_svg_to_doc(&svg, None).unwrap();
    let get = |kind: &str| {
        typed.iter().find_map(|w| match w {
            KilnWarning::ImportObjectSkipped { kind: k, count } if k == kind => Some(*count),
            _ => None,
        })
    };
    assert_eq!(get("filter"), Some(1));
    assert_eq!(get("mask"), Some(1));
    assert!(get("倾斜变换").is_some(), "倾斜变换应计入跳过:{typed:?}");
    assert!(typed.iter().all(KilnWarning::is_degrading));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- VB-5

/// VB-5:warnings_by_kind / count_of 按稳定键聚合。
#[test]
fn warnings_by_kind_aggregates() {
    let doc = doc_with_styles(&[
        ("transform", "rotateY(20deg)"),
        ("box-shadow", "0 2px 6px rgba(0,0,0,.5)"),
    ]);
    let (_, report) =
        vb_kiln::export_artboard(&doc, doc.artboards[0], &req(Format::Png), None).unwrap();
    let by_kind = report.warnings_by_kind();
    assert_eq!(by_kind.get("unsupported_dropped"), Some(&2));
    // 序列化即 JSON 对象(门禁直读)
    let j = serde_json::to_value(&by_kind).unwrap();
    assert_eq!(j["unsupported_dropped"], 2);
    assert_eq!(report.count_of("unsupported_dropped"), 2);
    assert_eq!(report.count_of("asset_not_found"), 0);
    // 稳定键回归(下游门禁依赖,不得静默改名)
    for (w, kind) in [
        (
            KilnWarning::StaticCanvasAnimation,
            "static_canvas_animation",
        ),
        (
            KilnWarning::AssetNotFound {
                src: "a.ttf".into(),
            },
            "asset_not_found",
        ),
        (
            KilnWarning::UnsupportedPropertyDropped {
                prop: "mask".into(),
                count: 1,
                lane: "native",
            },
            "unsupported_dropped",
        ),
        (
            KilnWarning::InlineSvgRasterized {
                count: 1,
                format: "svg",
            },
            "inline_svg_rasterized",
        ),
        (
            KilnWarning::ClipShapeApproximated {
                shape: "path()".into(),
                count: 1,
            },
            "clip_shape_approximated",
        ),
        (
            KilnWarning::ImportObjectSkipped {
                kind: "mask".into(),
                count: 1,
            },
            "import_object_skipped",
        ),
    ] {
        assert_eq!(w.kind(), kind, "{w:?}");
    }
}

// ---------------------------------------------------------------- VB-1(车道 K 侧定型)

/// VB-1:404 资源定型为 AssetNotFound 且消息含资源路径。
#[test]
fn not_found_maps_to_asset_not_found() {
    let meta = vb_kiln::dompaint::DomPaintMeta::default();
    let w = meta_warnings(&meta, Format::Png, &[]);
    assert!(w.is_empty(), "无 404/降级计数时不产生告警");
    let w = meta_warnings(
        &meta,
        Format::Ai,
        &["fonts/x.woff2".to_string(), "js/echarts.js".to_string()],
    );
    assert!(w.iter().any(|x| matches!(
        x,
        KilnWarning::AssetNotFound { src } if src == "fonts/x.woff2"
    )));
    let msg = w
        .iter()
        .find_map(|x| match x {
            KilnWarning::AssetNotFound { src } => Some(x.message() + src),
            _ => None,
        })
        .unwrap();
    assert!(msg.contains("静态资源 404"));
}
