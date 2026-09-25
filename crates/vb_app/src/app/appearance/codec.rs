//! 外观编解码:模型 → style 声明(编译)与 CSS/属性 → 模型(解码)。
//!
//! 06-1 自 `appearance.rs` 按职责拆出(纯搬移,零行为变化);
//! shadow / feather 片段的字符串映射在 `segments`。

use vb_css::Decl;
use vb_doc::model::NodeKind;

use super::model::*;
use super::segments::*;

// ═══════════════════════ 4. 编译(模型 → style 声明) ═══════════════════════

/// 从声明列表移除一组属性。
fn remove_props(style: &mut Vec<Decl>, props: &[&str]) {
    style.retain(|d| !props.contains(&d.prop.as_str()));
}

fn set_prop(style: &mut Vec<Decl>, prop: &str, value: String) {
    if let Some(d) = style.iter_mut().find(|d| d.prop == prop) {
        d.value = value;
    } else {
        style.push(Decl {
            prop: prop.to_string(),
            value,
            important: false,
        });
    }
}

/// 编译:按模型把受管属性组重写进节点样式(未接管的手写声明不动)。
///
/// `prev_own` = 上一次的接管集(来自既有属性/解码);删除到 0 条也保持
/// 接管(清空 = 移除声明)。返回新样式。
pub fn compile_style(
    kind: &NodeKind,
    style: &[Decl],
    prev_own: &[String],
    items: &[AppearanceItem],
) -> Vec<Decl> {
    let t = target_of(kind);
    if t == AppearanceTarget::Frozen {
        return style.to_vec();
    }
    let mut own: Vec<String> = prev_own.to_vec();
    for it in items {
        let tag = item_own_tag(it);
        AppearanceModel::own_add(&mut own, &tag);
    }
    let mut st = style.to_vec();

    // ── 填充 ──
    if own.iter().any(|o| o == OWN_FILL) {
        let fills: Vec<&FillItem> = items
            .iter()
            .filter_map(|i| match i {
                AppearanceItem::Fill(f) => Some(f),
                _ => None,
            })
            .collect();
        let on: Vec<&&FillItem> = fills.iter().filter(|f| f.enabled).collect();
        match t {
            AppearanceTarget::Box => {
                remove_props(
                    &mut st,
                    &[
                        "background-color",
                        "background-image",
                        "background-blend-mode",
                    ],
                );
                // 最底层启用纯色 → background-color(单填充文档与既有行为一致)
                let bottom_solid = on.last().and_then(|f| match &f.body {
                    FillBody::Solid { value } => Some(value.clone()),
                    _ => None,
                });
                if let Some(c) = &bottom_solid {
                    set_prop(&mut st, "background-color", c.clone());
                }
                // 其余条目 → background-image 层(模型序 = 层序,首层最上)
                let mut layers: Vec<String> = Vec::new();
                let mut blends: Vec<String> = Vec::new();
                for (i, f) in on.iter().enumerate() {
                    let is_bottom_solid =
                        i == on.len() - 1 && matches!(f.body, FillBody::Solid { .. });
                    if is_bottom_solid {
                        continue;
                    }
                    layers.push(match &f.body {
                        FillBody::Solid { value } => solid_as_layer(value),
                        FillBody::Gradient { value } => value.clone(),
                        FillBody::Raw { value } => value.clone(),
                    });
                    blends.push(f.blend.clone().unwrap_or_else(|| "normal".into()));
                }
                if !layers.is_empty() {
                    set_prop(&mut st, "background-image", layers.join(", "));
                    if blends.iter().any(|b| b != "normal") {
                        set_prop(&mut st, "background-blend-mode", blends.join(", "));
                    }
                }
            }
            AppearanceTarget::Text => {
                remove_props(&mut st, &["color"]);
                // 文字:仅最底层纯色落 `color`;其余条目冻结登记(报告)
                if let Some(Some(f)) = on.last().map(|f| match &f.body {
                    FillBody::Solid { value } => Some(value.clone()),
                    _ => None,
                }) {
                    set_prop(&mut st, "color", f);
                }
            }
            AppearanceTarget::Vector => {
                remove_props(&mut st, &["fill"]);
                if let Some(Some(f)) = on.last().map(|f| match &f.body {
                    FillBody::Solid { value } => Some(value.clone()),
                    _ => None,
                }) {
                    set_prop(&mut st, "fill", f);
                }
            }
            AppearanceTarget::Frozen => {}
        }
    }

    // ── 描边 ──
    if own.iter().any(|o| o == OWN_STROKE) {
        let strokes: Vec<&StrokeItem> = items
            .iter()
            .filter_map(|i| match i {
                AppearanceItem::Stroke(s) => Some(s),
                _ => None,
            })
            .collect();
        remove_props(&mut st, stroke_props(t));
        if let Some(s) = strokes.iter().find(|s| s.enabled) {
            // 主描边 = 启用条目中的第 1 条;其余条目冻结登记(报告)
            let spec = &s.spec;
            match t {
                AppearanceTarget::Box => match spec.align {
                    StrokeAlign::Outside => {
                        set_prop(&mut st, "outline-width", px(spec.width));
                        set_prop(&mut st, "outline-style", dash_style(&spec.dash).into());
                        if let Some(c) = &spec.color {
                            set_prop(&mut st, "outline-color", c.clone());
                        }
                    }
                    _ => {
                        // 内侧/居中:border 恒内侧;居中为降级落盘(报告注记)
                        set_prop(&mut st, "border-width", px(spec.width));
                        set_prop(&mut st, "border-style", dash_style(&spec.dash).into());
                        if let Some(c) = &spec.color {
                            set_prop(&mut st, "border-color", c.clone());
                        }
                    }
                },
                AppearanceTarget::Vector => {
                    if let Some(c) = &spec.color {
                        set_prop(&mut st, "stroke", c.clone());
                    }
                    set_prop(&mut st, "stroke-width", px(spec.width));
                    set_prop(&mut st, "stroke-linecap", spec.cap.css().into());
                    set_prop(&mut st, "stroke-linejoin", spec.join.css().into());
                    if spec.join == StrokeJoin::Miter {
                        set_prop(
                            &mut st,
                            "stroke-miterlimit",
                            vb_common::units::fmt_num(spec.miter_limit),
                        );
                    }
                    if !spec.dash.is_empty() {
                        let v: Vec<String> = spec.dash.iter().map(|d| px(*d)).collect();
                        set_prop(&mut st, "stroke-dasharray", v.join(", "));
                    }
                    // 对齐:SVG 恒居中(Inside/Outside 冻结登记);箭头不落盘
                }
                AppearanceTarget::Text => {
                    set_prop(&mut st, "-webkit-text-stroke-width", px(spec.width));
                    if let Some(c) = &spec.color {
                        set_prop(&mut st, "-webkit-text-stroke-color", c.clone());
                    }
                }
                AppearanceTarget::Frozen => {}
            }
        }
    }

    // ── 效果(按受管属性收集启用段;模型序 = 同类段序) ──
    let effects: Vec<&EffectItem> = items
        .iter()
        .filter_map(|i| match i {
            AppearanceItem::Effect(e) => Some(e),
            _ => None,
        })
        .collect();
    if effects.is_empty() {
        return st;
    }
    // 触及集:模型中存在该类条目(含禁用)→ 该属性归编译管
    let mut touched: Vec<String> = Vec::new();
    for e in &effects {
        AppearanceModel::own_add(&mut touched, &e.effect.own_tag());
    }
    let shadow_prop = match t {
        AppearanceTarget::Text => "text-shadow",
        _ => "box-shadow",
    };
    let mut shadows: Vec<String> = Vec::new();
    let mut filters: Vec<String> = Vec::new();
    let mut radius: Option<f64> = None;
    let mut mask: Option<String> = None;
    let mut others: Vec<(String, String)> = Vec::new();
    for e in effects.iter().filter(|e| e.enabled) {
        let is_text = t == AppearanceTarget::Text;
        match &e.effect {
            Effect::DropShadow { .. } | Effect::OuterGlow { .. } => {
                // 文字 → text-shadow(无 inset/spread);其余 → box-shadow
                let seg = if is_text {
                    text_shadow_segment(&e.effect)
                } else {
                    shadow_segment(&e.effect)
                };
                if let Some(seg) = seg {
                    shadows.push(seg);
                }
            }
            Effect::InnerShadow { .. } | Effect::InnerGlow { .. } => {
                // 文字无内阴影/内发光落点(构建器已拒绝;编译兜底跳过,
                // 不静默产生非法 CSS)
                if is_text {
                    continue;
                }
                if let Some(seg) = shadow_segment(&e.effect) {
                    shadows.push(seg);
                }
            }
            Effect::GaussianBlur { radius } => filters.push(format!("blur({})", px(*radius))),
            Effect::RoundCorners { radius: r } => radius = Some(*r),
            Effect::Feather { radius } => mask = Some(feather_template(*radius)),
            Effect::Other { prop, value } => others.push((prop.clone(), value.clone())),
        }
    }
    let apply = |st: &mut Vec<Decl>, prop: &str, touched: bool, segs: &[String], join: &str| {
        if !touched {
            return;
        }
        remove_props(st, &[prop]);
        if !segs.is_empty() {
            set_prop(st, prop, segs.join(join));
        }
    };
    apply(
        &mut st,
        shadow_prop,
        touched.iter().any(|t| t == OWN_SHADOW),
        &shadows,
        ", ",
    );
    apply(
        &mut st,
        "filter",
        touched.iter().any(|t| t == OWN_BLUR),
        &filters,
        " ",
    );
    if touched.iter().any(|t| t == OWN_RADIUS) {
        remove_props(&mut st, &["border-radius"]);
        if let Some(r) = radius {
            set_prop(&mut st, "border-radius", px(r));
        }
    }
    if touched.iter().any(|t| t == OWN_MASK) {
        remove_props(&mut st, &["mask-image"]);
        if let Some(m) = mask {
            set_prop(&mut st, "mask-image", m);
        }
    }
    for (prop, value) in others {
        let tag = format!("css:{prop}");
        if touched.contains(&tag) {
            set_prop(&mut st, &prop, value);
        }
    }
    st
}

/// 虚线 → 盒对象 border-style 近似(有虚线 → dashed;CSS 无自定义 dash)。
fn dash_style(dash: &[f64]) -> &'static str {
    if dash.is_empty() {
        "solid"
    } else {
        "dashed"
    }
}

pub(super) fn item_own_tag(it: &AppearanceItem) -> String {
    match it {
        AppearanceItem::Fill(_) => OWN_FILL.to_string(),
        AppearanceItem::Stroke(_) => OWN_STROKE.to_string(),
        AppearanceItem::Effect(e) => e.effect.own_tag(),
    }
}

// ═══════════════════════ 5. 解码(CSS/属性 → 模型) ═══════════════════════

/// 解码外观模型:优先 `data-vb-appearance`(无损);无/损坏 → CSS 启发式
/// (只认领可完整表达的声明;其余声明保持原样不受管)。
pub fn decode_model(
    kind: &NodeKind,
    style: &[Decl],
    attrs: &[(String, String)],
) -> AppearanceModel {
    if let Some(json) = attrs
        .iter()
        .find(|(k, _)| k == APPEARANCE_ATTR)
        .map(|(_, v)| v)
    {
        if let Ok(m) = serde_json::from_str::<AppearanceModel>(json) {
            if m.v == 1 {
                return m;
            }
        }
    }
    fallback_decode(kind, style)
}

pub(super) fn get<'a>(style: &'a [Decl], prop: &str) -> Option<&'a str> {
    style
        .iter()
        .find(|d| d.prop == prop)
        .map(|d| d.value.as_str())
}

pub(super) fn fallback_decode(kind: &NodeKind, style: &[Decl]) -> AppearanceModel {
    let t = target_of(kind);
    let mut m = AppearanceModel::empty();
    // ── 填充 ──
    match t {
        AppearanceTarget::Box => {
            let layers: Vec<String> = get(style, "background-image")
                .map(|v| {
                    vb_css::split_top_level(v, ',')
                        .iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            for l in &layers {
                let body = if let Some(c) = layer_as_solid(l) {
                    FillBody::Solid { value: c }
                } else if is_gradient_layer(l) {
                    FillBody::Gradient { value: l.clone() }
                } else {
                    FillBody::Raw { value: l.clone() }
                };
                m.items.push(AppearanceItem::Fill(FillItem {
                    enabled: true,
                    blend: None,
                    body,
                }));
            }
            if let Some(c) = get(style, "background-color") {
                m.items.push(AppearanceItem::Fill(FillItem {
                    enabled: true,
                    blend: None,
                    body: FillBody::Solid {
                        value: c.to_string(),
                    },
                }));
            }
            // 逐层混合(background-blend-mode 第 i 项 ↔ 第 i 填充;
            // 编码端 blend 列表只覆盖 image 层,故严格按位取,不做 repeat 兜底)
            if let Some(bm) = get(style, "background-blend-mode") {
                let list: Vec<String> = vb_css::split_top_level(bm, ',')
                    .iter()
                    .map(|s| s.trim().to_string())
                    .collect();
                for (i, it) in m.items.iter_mut().enumerate() {
                    if let AppearanceItem::Fill(f) = it {
                        if let Some(b) = list.get(i) {
                            if b != "normal" {
                                f.blend = Some(b.clone());
                            }
                        }
                    }
                }
            }
            if !layers.is_empty() || get(style, "background-color").is_some() {
                m.own.push(OWN_FILL.into());
            }
        }
        AppearanceTarget::Text => {
            if let Some(c) = get(style, "color") {
                m.items.push(AppearanceItem::Fill(FillItem {
                    enabled: true,
                    blend: None,
                    body: FillBody::Solid {
                        value: c.to_string(),
                    },
                }));
                m.own.push(OWN_FILL.into());
            }
        }
        AppearanceTarget::Vector => {
            if let Some(c) = get(style, "fill") {
                m.items.push(AppearanceItem::Fill(FillItem {
                    enabled: true,
                    blend: None,
                    body: FillBody::Solid {
                        value: c.to_string(),
                    },
                }));
                m.own.push(OWN_FILL.into());
            }
        }
        AppearanceTarget::Frozen => {}
    }
    // ── 描边(只认领可完整表达的组合) ──
    match t {
        AppearanceTarget::Box => {
            if let Some(spec) = decode_box_stroke(style, StrokeAlign::Inside) {
                m.items.push(AppearanceItem::Stroke(StrokeItem {
                    enabled: true,
                    blend: None,
                    spec,
                }));
                m.own.push(OWN_STROKE.into());
            } else if let Some(spec) = decode_box_stroke(style, StrokeAlign::Outside) {
                m.items.push(AppearanceItem::Stroke(StrokeItem {
                    enabled: true,
                    blend: None,
                    spec,
                }));
                m.own.push(OWN_STROKE.into());
            }
        }
        AppearanceTarget::Vector => {
            if get(style, "stroke").is_some() || get(style, "stroke-width").is_some() {
                m.items.push(AppearanceItem::Stroke(StrokeItem {
                    enabled: true,
                    blend: None,
                    spec: decode_vector_stroke(style),
                }));
                m.own.push(OWN_STROKE.into());
            }
        }
        AppearanceTarget::Text => {
            if let Some(w) =
                get(style, "-webkit-text-stroke-width").and_then(vb_common::units::parse_px)
            {
                m.items.push(AppearanceItem::Stroke(StrokeItem {
                    enabled: true,
                    blend: None,
                    spec: StrokeSpec {
                        width: w,
                        color: get(style, "-webkit-text-stroke-color").map(str::to_string),
                        ..StrokeSpec::default()
                    },
                }));
                m.own.push(OWN_STROKE.into());
            } else if let Some(v) = get(style, "-webkit-text-stroke") {
                // 简写 "Npx C"
                let toks: Vec<&str> = v.split_whitespace().collect();
                if let (Some(w), c) = (
                    toks.first().and_then(|t| vb_common::units::parse_px(t)),
                    toks.get(1).copied(),
                ) {
                    m.items.push(AppearanceItem::Stroke(StrokeItem {
                        enabled: true,
                        blend: None,
                        spec: StrokeSpec {
                            width: w,
                            color: c.map(str::to_string),
                            ..StrokeSpec::default()
                        },
                    }));
                    m.own.push(OWN_STROKE.into());
                }
            }
        }
        AppearanceTarget::Frozen => {}
    }
    // ── 效果 ──
    let shadow_prop = match t {
        AppearanceTarget::Text => "text-shadow",
        _ => "box-shadow",
    };
    let push_effect = |m: &mut AppearanceModel, e: Effect| {
        let tag = e.own_tag();
        m.items.push(AppearanceItem::Effect(EffectItem {
            enabled: true,
            blend: None,
            effect: e,
        }));
        AppearanceModel::own_add(&mut m.own, &tag);
    };
    if let Some(v) = get(style, shadow_prop) {
        for seg in vb_css::split_top_level(v, ',') {
            let seg = seg.trim();
            if seg.is_empty() {
                continue;
            }
            match parse_shadow_segment(seg) {
                Some((inset, lens, color)) => {
                    if t == AppearanceTarget::Text {
                        // text-shadow 无 inset/spread;按投影/外发光归类
                        let blur = lens[2];
                        push_effect(
                            &mut m,
                            if lens[0] == 0.0 && lens[1] == 0.0 {
                                Effect::OuterGlow { blur, color }
                            } else {
                                Effect::DropShadow {
                                    x: lens[0],
                                    y: lens[1],
                                    blur,
                                    spread: 0.0,
                                    color,
                                }
                            },
                        );
                    } else {
                        push_effect(&mut m, shadow_effect_of(inset, lens, color));
                    }
                }
                None => push_effect(
                    &mut m,
                    Effect::Other {
                        prop: shadow_prop.to_string(),
                        value: seg.to_string(),
                    },
                ),
            }
        }
    }
    // 文字对象上的手写 box-shadow:整体保真为 Other(不被面板接管)
    if t == AppearanceTarget::Text {
        if let Some(v) = get(style, "box-shadow") {
            push_effect(
                &mut m,
                Effect::Other {
                    prop: "box-shadow".into(),
                    value: v.to_string(),
                },
            );
        }
    }
    if let Some(v) = get(style, "filter") {
        for f in vb_css::split_top_level(v, ' ') {
            let f = f.trim();
            if f.is_empty() {
                continue;
            }
            if let Some(r) = f
                .strip_prefix("blur(")
                .and_then(|x| x.strip_suffix(')'))
                .and_then(vb_common::units::parse_px)
            {
                push_effect(&mut m, Effect::GaussianBlur { radius: r });
            } else {
                push_effect(
                    &mut m,
                    Effect::Other {
                        prop: "filter".into(),
                        value: f.to_string(),
                    },
                );
            }
        }
    }
    if let Some(v) = get(style, "border-radius") {
        if let Some(r) = vb_common::units::parse_px(v) {
            push_effect(&mut m, Effect::RoundCorners { radius: r });
        } else {
            push_effect(
                &mut m,
                Effect::Other {
                    prop: "border-radius".into(),
                    value: v.to_string(),
                },
            );
        }
    }
    if let Some(v) = get(style, "mask-image") {
        match parse_feather(v) {
            Some(r) => push_effect(&mut m, Effect::Feather { radius: r }),
            None => push_effect(
                &mut m,
                Effect::Other {
                    prop: "mask-image".into(),
                    value: v.to_string(),
                },
            ),
        }
    }
    m
}

/// 解码盒对象描边(align 指定读 border 系还是 outline 系);
/// 四边值不同等不可表达情形返回 None(不认领,声明原样保留)。
fn decode_box_stroke(style: &[Decl], align: StrokeAlign) -> Option<StrokeSpec> {
    let (wp, sp, cp) = match align {
        StrokeAlign::Outside => ("outline-width", "outline-style", "outline-color"),
        _ => ("border-width", "border-style", "border-color"),
    };
    let w = get(style, wp).and_then(vb_common::units::parse_px)?;
    let s = get(style, sp)?;
    // 四边不同(width "2px 4px")不可表达 → 不认领
    if s.split_whitespace().count() > 1 || s == "none" {
        return None;
    }
    Some(StrokeSpec {
        width: w,
        color: get(style, cp).map(str::to_string),
        dash: if s == "dashed" || s == "dotted" {
            vec![6.0, 3.0]
        } else {
            Vec::new()
        },
        align,
        ..StrokeSpec::default()
    })
}

fn decode_vector_stroke(style: &[Decl]) -> StrokeSpec {
    StrokeSpec {
        width: get(style, "stroke-width")
            .and_then(vb_common::units::parse_px)
            .unwrap_or(1.0),
        color: get(style, "stroke").map(str::to_string),
        cap: get(style, "stroke-linecap")
            .map(StrokeCap::from_css)
            .unwrap_or_default(),
        join: get(style, "stroke-linejoin")
            .map(StrokeJoin::from_css)
            .unwrap_or_default(),
        miter_limit: get(style, "stroke-miterlimit")
            .and_then(|v| v.parse().ok())
            .unwrap_or(4.0),
        dash: get(style, "stroke-dasharray")
            .map(|v| {
                vb_css::split_top_level(v, ',')
                    .iter()
                    .filter_map(|s| vb_common::units::parse_px(s.trim()))
                    .collect()
            })
            .unwrap_or_default(),
        align: StrokeAlign::Center,
        arrow_start: Arrowhead::default(),
        arrow_end: Arrowhead::default(),
    }
}

/// 序列化模型(紧凑 JSON;落 `data-vb-appearance`)。
pub fn encode_model(m: &AppearanceModel) -> String {
    serde_json::to_string(m).unwrap_or_else(|_| "{\"v\":1,\"own\":[],\"items\":[]}".to_string())
}
