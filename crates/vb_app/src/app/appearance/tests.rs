//! 外观单元测试(编解码 / 映射 / 解析)。
//! 06-1 自 `appearance.rs` 拆出(纯搬移,零行为变化)。

use super::*;
use vb_css::Decl;
use vb_doc::model::NodeKind;

fn solid(v: &str) -> FillBody {
    FillBody::Solid {
        value: v.to_string(),
    }
}

fn fill(v: &str) -> AppearanceItem {
    AppearanceItem::Fill(FillItem {
        enabled: true,
        blend: None,
        body: solid(v),
    })
}

/// 效果映射表(design/06 §4.6):六种效果的确定性 CSS 片段。
#[test]
fn effect_mapping_table_produces_exact_css() {
    let cases: Vec<(Effect, &str)> = vec![
        (
            Effect::DropShadow {
                x: 8.0,
                y: 12.0,
                blur: 24.0,
                spread: 4.0,
                color: "#00000059".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            },
            "8px 12px 24px 4px #00000059",
        ),
        (
            Effect::InnerShadow {
                x: 0.0,
                y: 2.0,
                blur: 6.0,
                spread: 0.0,
                color: "#00000066".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            },
            "inset 0 2px 6px 0 #00000066",
        ),
        (
            Effect::OuterGlow {
                blur: 12.0,
                color: "#2e86ff80".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            },
            "0 0 12px #2e86ff80",
        ),
        (
            Effect::InnerGlow {
                blur: 10.0,
                color: "#ffffff40".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            },
            "inset 0 0 10px #ffffff40",
        ),
    ];
    for (e, seg) in cases {
        assert_eq!(shadow_segment(&e).unwrap(), seg, "{:?}", e.label());
    }
    assert_eq!(Effect::GaussianBlur { radius: 4.0 }.own_tag(), OWN_BLUR,);
    // 模糊与羽化的完整声明由 compile 生成(见 multi_effects_compile_in_order)
}

/// 多条效果编译:同类按模型序拼接(box-shadow 逗号 / filter 空格),
/// 顺序 = 条目顺序(验收 05-1-4/门 2)。
#[test]
fn multi_effects_compile_in_order() {
    let items = vec![
        AppearanceItem::Effect(EffectItem {
            enabled: true,
            blend: None,
            effect: Effect::DropShadow {
                x: 2.0,
                y: 2.0,
                blur: 4.0,
                spread: 0.0,
                color: "#00000080".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            },
        }),
        AppearanceItem::Effect(EffectItem {
            enabled: true,
            blend: None,
            effect: Effect::GaussianBlur { radius: 3.0 },
        }),
        AppearanceItem::Effect(EffectItem {
            enabled: true,
            blend: None,
            effect: Effect::InnerGlow {
                blur: 8.0,
                color: "#ffffff40".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            },
        }),
    ];
    let st = compile_style(&NodeKind::Box, &[], &[], &items);
    let get = |p: &str| get(&st, p).unwrap().to_string();
    assert_eq!(
        get("box-shadow"),
        "2px 2px 4px 0 #00000080, inset 0 0 8px #ffffff40",
        "阴影段序 = 条目序"
    );
    assert_eq!(get("filter"), "blur(3px)");
    assert_eq!(get("filter"), "blur(3px)");
}

/// 多填充编译:纯色底条 → background-color,其余 → background-image
/// 层(纯色以同色双 stop 渐变承载,像素恒等);层序 = 条目序。
#[test]
fn multi_fill_compile_layer_order() {
    let items = vec![
        fill("#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        AppearanceItem::Fill(FillItem {
            enabled: true,
            blend: None,
            body: FillBody::Gradient {
                value: "linear-gradient(90deg, #204060 0%, #90c0f0 100%)".into(),
            },
        }),
        fill("#102030"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    ];
    let st = compile_style(&NodeKind::Box, &[], &[], &items);
    assert_eq!(
        get(&st, "background-color").unwrap(),
        "#102030", // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        "最底层纯色写 background-color"
    );
    assert_eq!(
        get(&st, "background-image").unwrap(),
        "linear-gradient(#ff0000, #ff0000), linear-gradient(90deg, #204060 0%, #90c0f0 100%)",
        "层序 = 条目序(首层最上)"
    );
    // 解码回模型:渐变层还原为渐变条目,background-color 还原为底层纯色
    let m = fallback_decode(&NodeKind::Box, &st);
    assert_eq!(m.items.len(), 3);
    assert_eq!(m.items[0].summary(), "填充 #ff0000");
    assert!(matches!(
        &m.items[1],
        AppearanceItem::Fill(FillItem {
            body: FillBody::Gradient { .. },
            ..
        })
    ));
    assert_eq!(m.items[2].summary(), "填充 #102030");
    // 编码 ∘ 解码恒等(无损往返的最小单元)
    let st2 = compile_style(&NodeKind::Box, &[], &m.own, &m.items);
    assert_eq!(st, st2, "decode∘encode 必须恒等");
}

/// 禁用条目不进编译产物但保留在模型(眼睛 = 临时禁用,05-1-1)。
#[test]
fn disabled_items_are_compiled_out_but_kept_in_model() {
    let mut items = vec![fill("#ff0000"), fill("#00ff00")]; // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    items[1].set_enabled(false);
    let st = compile_style(&NodeKind::Box, &[], &[], &items);
    assert_eq!(get(&st, "background-color").unwrap(), "#ff0000"); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    assert!(get(&st, "background-image").is_none(), "禁用层不落盘");
    // 全部禁用 + 接管 → background-color 移除(清空 = 无填充)
    for it in &mut items {
        it.set_enabled(false);
    }
    let st = compile_style(&NodeKind::Box, &[], &["fill".to_string()], &items);
    assert!(get(&st, "background-color").is_none());
}

/// 条目级混合模式:填充 → background-blend-mode 逐层,往返保真。
#[test]
fn fill_blend_roundtrips_via_background_blend_mode() {
    let items = vec![
        AppearanceItem::Fill(FillItem {
            enabled: true,
            blend: Some("multiply".into()),
            body: solid("#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        }),
        AppearanceItem::Fill(FillItem {
            enabled: true,
            blend: None,
            body: solid("#102030"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        }),
    ];
    let st = compile_style(&NodeKind::Box, &[], &[], &items);
    assert_eq!(
        get(&st, "background-blend-mode").unwrap(),
        "multiply",
        "blend 列表只覆盖 background-image 层(底层纯色 excluded)"
    );
    let m = fallback_decode(&NodeKind::Box, &st);
    assert_eq!(m.items[0].blend().unwrap(), "multiply");
    assert!(m.items[1].blend().is_none());
    let st2 = compile_style(&NodeKind::Box, &[], &m.own, &m.items);
    assert_eq!(st, st2, "blend 往返恒等");
}

/// 阴影段解析:inset/颜色前置/发光归类;残段不认领(调用方落 Other)。
#[test]
fn shadow_segment_parse_roundtrip() {
    let (inset, lens, color) = parse_shadow_segment("inset 0 2px 6px 0 #00000066").unwrap();
    assert!(inset);
    assert_eq!(lens, [0.0, 2.0, 6.0, 0.0]);
    assert_eq!(color, "#00000066"); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                                    // 颜色前置(CSS 允许;rgba() 原样保留,canonical 不转 hex)
    let (_, lens, color) = parse_shadow_segment("rgba(0, 0, 0, 0.4) 1px 2px").unwrap();
    assert_eq!(lens, [1.0, 2.0, 0.0, 0.0]);
    assert_eq!(color, "rgba(0, 0, 0, 0.4)");
    // 手写 CSS → 解码归类:x=y=spread=0 → 发光
    let e = shadow_effect_of(false, [0.0, 0.0, 8.0, 0.0], "#ff0".into()); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    assert!(matches!(e, Effect::OuterGlow { .. }));
    let e = shadow_effect_of(true, [3.0, 3.0, 5.0, 1.0], "#000".into()); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    assert!(matches!(e, Effect::InnerShadow { .. }));
    // 残段:长度不足 → None
    assert!(parse_shadow_segment("1px").is_none());
}

/// 羽化模板 ⇄ 解析对称;手写 mask 不匹配模板 → Other 保真。
#[test]
fn feather_template_roundtrip() {
    let v = feather_template(12.0);
    assert_eq!(parse_feather(&v), Some(12.0));
    assert_eq!(
        v,
        "radial-gradient(circle, #000 calc(100% - 12px), transparent)"
    );
    assert_eq!(
        parse_feather("radial-gradient(circle at 30% 30%, #000, transparent)"),
        None,
        "非模板 mask 不认领"
    );
}

/// CSS 启发式解码:border/outline/stroke/-webkit-text-stroke 四落点。
#[test]
fn fallback_decode_claims_strokes_by_target() {
    let mk = |decls: &[(&str, &str)]| -> Vec<Decl> {
        decls
            .iter()
            .map(|(p, v)| Decl {
                prop: p.to_string(),
                value: v.to_string(),
                important: false,
            })
            .collect()
    };
    // 盒:border → 内侧描边条目
    let st = mk(&[
        ("border-width", "2px"),
        ("border-style", "solid"),
        ("border-color", "#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    ]);
    let m = fallback_decode(&NodeKind::Box, &st);
    assert!(
        matches!(&m.items[0], AppearanceItem::Stroke(s) if s.spec.align == StrokeAlign::Inside && s.spec.width == 2.0)
    );
    // 盒:outline → 外侧描边条目
    let st = mk(&[
        ("outline-width", "3px"),
        ("outline-style", "solid"),
        ("outline-color", "#00ff00"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    ]);
    let m = fallback_decode(&NodeKind::Box, &st);
    assert!(
        matches!(&m.items[0], AppearanceItem::Stroke(s) if s.spec.align == StrokeAlign::Outside)
    );
    // 矢量:stroke 系全字段
    let st = mk(&[
        ("stroke", "#1a1a1a"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        ("stroke-width", "1.5px"),
        ("stroke-linecap", "round"),
        ("stroke-linejoin", "bevel"),
        ("stroke-miterlimit", "6"),
        ("stroke-dasharray", "6px, 3px"),
    ]);
    let m = fallback_decode(&NodeKind::Vector { path: kurbo_stub() }, &st);
    match &m.items[0] {
        AppearanceItem::Stroke(s) => {
            assert_eq!(s.spec.width, 1.5);
            assert_eq!(s.spec.cap, StrokeCap::Round);
            assert_eq!(s.spec.join, StrokeJoin::Bevel);
            assert_eq!(s.spec.miter_limit, 6.0);
            assert_eq!(s.spec.dash, vec![6.0, 3.0]);
        }
        other => panic!("应为描边条目:{other:?}"),
    }
    // 文字:-webkit-text-stroke-width/color
    let st = mk(&[
        ("-webkit-text-stroke-width", "2px"),
        ("-webkit-text-stroke-color", "#3366ff"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    ]);
    let m = fallback_decode(
        &NodeKind::Text {
            text: String::new(),
            mode: Default::default(),
            segments: Vec::new(),
        },
        &st,
    );
    match &m.items[0] {
        AppearanceItem::Stroke(s) => {
            assert_eq!(s.spec.width, 2.0);
            assert_eq!(s.spec.color.as_deref(), Some("#3366ff")); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        }
        other => panic!("应为文字描边条目:{other:?}"),
    }
}

/// 文字效果:阴影落 text-shadow 段;内阴影被构建器拒绝(05-6-1)。
#[test]
fn text_effects_use_text_shadow_and_inner_is_rejected() {
    let text_kind = NodeKind::Text {
        text: String::new(),
        mode: Default::default(),
        segments: Vec::new(),
    };
    let items = vec![AppearanceItem::Effect(EffectItem {
        enabled: true,
        blend: None,
        effect: Effect::DropShadow {
            x: 2.0,
            y: 2.0,
            blur: 3.0,
            spread: 0.0,
            color: "#00000080".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        },
    })];
    let st = compile_style(&text_kind, &[], &[], &items);
    assert_eq!(get(&st, "text-shadow").unwrap(), "2px 2px 3px #00000080");
    assert!(get(&st, "box-shadow").is_none(), "文字不得落 box-shadow");
}

/// 模型 JSON 序列化往返(v/own/items 全保真)。
#[test]
fn model_json_roundtrip() {
    let m = AppearanceModel {
        v: 1,
        own: vec![OWN_FILL.into(), OWN_SHADOW.into()],
        items: vec![
            fill("#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            AppearanceItem::Effect(EffectItem {
                enabled: false,
                blend: Some("screen".into()),
                effect: Effect::RoundCorners { radius: 8.0 },
            }),
        ],
    };
    let json = encode_model(&m);
    let m2: AppearanceModel = serde_json::from_str(&json).unwrap();
    assert_eq!(m, m2);
    assert!(json.contains("\"kind\":\"fill\""), "紧凑 JSON:{json}");
}

/// 不支持清单:六项齐备、文案非空、按 id 可查(design/06 §七)。
#[test]
fn unsupported_registry_covers_design06() {
    let ids: Vec<&str> = UNSUPPORTED.iter().map(|f| f.id).collect();
    assert_eq!(
        ids,
        vec![
            "live_paint",
            "mesh_gradient",
            "image_trace",
            "perspective_3d",
            "symbols",
            "variables",
        ]
    );
    for f in UNSUPPORTED {
        assert!(!f.label.is_empty());
        assert_eq!(
            unsupported_message(f.id),
            Some(f.message),
            "{} 应可按 id 查到提示",
            f.id
        );
        assert!(f.message.chars().count() >= 8, "{} 文案过短", f.id);
    }
    assert!(unsupported_message("no_such").is_none());
}

/// 校验器:圆角对路径/文字拒绝并给提示;内阴影对文字拒绝(05-6-1)。
#[test]
fn effect_validation_rejects_with_messages() {
    let vector = NodeKind::Vector { path: kurbo_stub() };
    let text = NodeKind::Text {
        text: String::new(),
        mode: Default::default(),
        segments: Vec::new(),
    };
    let rc = Effect::RoundCorners { radius: 8.0 };
    assert_eq!(
        validate_effect(&vector, &rc).unwrap_err(),
        "圆角仅对盒对象有效(路径/文字对象不支持)"
    );
    assert_eq!(
        validate_effect(&text, &rc).unwrap_err(),
        "圆角仅对盒对象有效(路径/文字对象不支持)"
    );
    let ish = Effect::InnerShadow {
        x: 0.0,
        y: 0.0,
        blur: 4.0,
        spread: 0.0,
        color: "#000".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
    };
    assert!(validate_effect(&text, &ish).unwrap_err().contains("内阴影"));
    assert!(validate_effect(&NodeKind::Box, &rc).is_ok());
}

/// 空路径(测试夹具用;真实 Vector 由钢笔/导入产生)。
fn kurbo_stub() -> vb_common::geom::BezPath {
    let mut p = vb_common::geom::BezPath::new();
    p.move_to(vb_common::geom::Point::new(0.0, 0.0));
    p.line_to(vb_common::geom::Point::new(10.0, 0.0));
    p
}
