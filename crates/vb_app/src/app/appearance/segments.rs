//! 效果 ↔ CSS 片段映射(纯函数:shadow / feather / 填充层的字符串编解码)。
//!
//! 06-1 自 `appearance.rs` 按职责拆出(纯搬移,零行为变化)。

use super::model::*;

// ═══════════════════════ 2. 纯函数:效果 → CSS 片段(映射表) ═══════════════════════

/// 规范 px:`0` 不带单位(与 vb_css canonical_value 一致,保证 L1 幂等)。
pub(super) fn px(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{}px", vb_common::units::fmt_num(v))
    }
}

/// 阴影类效果 → `box-shadow` 一个逗号段(启用条目按模型序拼接)。
pub fn shadow_segment(e: &Effect) -> Option<String> {
    match e {
        Effect::DropShadow {
            x,
            y,
            blur,
            spread,
            color,
        } => Some(format!(
            "{} {} {} {} {color}",
            px(*x),
            px(*y),
            px(*blur),
            px(*spread)
        )),
        Effect::InnerShadow {
            x,
            y,
            blur,
            spread,
            color,
        } => Some(format!(
            "inset {} {} {} {} {color}",
            px(*x),
            px(*y),
            px(*blur),
            px(*spread)
        )),
        Effect::OuterGlow { blur, color } => {
            Some(format!("{} {} {} {color}", px(0.0), px(0.0), px(*blur)))
        }
        Effect::InnerGlow { blur, color } => Some(format!(
            "inset {} {} {} {color}",
            px(0.0),
            px(0.0),
            px(*blur)
        )),
        _ => None,
    }
}

/// 阴影类效果 → `text-shadow` 段(无 inset/spread 语义;内阴影/内发光
/// 对文字无效,由构建器拒绝)。
pub fn text_shadow_segment(e: &Effect) -> Option<String> {
    match e {
        Effect::DropShadow {
            x, y, blur, color, ..
        } => Some(format!("{} {} {} {color}", px(*x), px(*y), px(*blur))),
        Effect::OuterGlow { blur, color } => {
            Some(format!("{} {} {} {color}", px(0.0), px(0.0), px(*blur)))
        }
        _ => None,
    }
}

/// 解析一个 box-shadow 逗号段(inset 前缀 + 2..4 个长度 + 颜色在首/尾)。
/// 只认领可完整表达的段;残段由调用方落 `Effect::Other` 保真。
pub fn parse_shadow_segment(seg: &str) -> Option<(bool, [f64; 4], String)> {
    let seg = seg.trim();
    let (inset, rest) = match seg.strip_prefix("inset") {
        Some(r) => (true, r.trim_start()),
        None => (false, seg),
    };
    let toks: Vec<String> = vb_css::split_top_level(rest, ' ')
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if toks.len() < 3 || toks.len() > 5 {
        return None;
    }
    // 颜色 token = 可被解析为颜色且位于首/尾
    let color_idx = if vb_common::color::parse_color(&toks[toks.len() - 1]).is_some() {
        toks.len() - 1
    } else if vb_common::color::parse_color(&toks[0]).is_some() {
        0
    } else {
        return None;
    };
    let color = toks[color_idx].clone();
    let lens: Vec<f64> = toks
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != color_idx)
        .map(|(_, t)| vb_common::units::parse_px(t))
        .collect::<Option<Vec<_>>>()?;
    if lens.len() < 2 || lens.len() > 4 {
        return None;
    }
    Some((
        inset,
        [
            lens[0],
            lens[1],
            *lens.get(2).unwrap_or(&0.0),
            *lens.get(3).unwrap_or(&0.0),
        ],
        color,
    ))
}

/// 分段 → 效果条目体(x=y=spread=0 归类为发光;否则阴影)。
pub(super) fn shadow_effect_of(inset: bool, l: [f64; 4], color: String) -> Effect {
    let [x, y, blur, spread] = l;
    match (inset, x == 0.0 && y == 0.0 && spread == 0.0) {
        (false, true) => Effect::OuterGlow { blur, color },
        (true, true) => Effect::InnerGlow { blur, color },
        (false, false) => Effect::DropShadow {
            x,
            y,
            blur,
            spread,
            color,
        },
        (true, false) => Effect::InnerShadow {
            x,
            y,
            blur,
            spread,
            color,
        },
    }
}

/// 羽化的确定性模板(mask-image)。模板必须与 [`parse_feather`] 对称。
pub(super) fn feather_template(radius: f64) -> String {
    format!(
        "radial-gradient(circle, #000 calc(100% - {}), transparent)",
        px(radius)
    )
}

pub(super) fn parse_feather(v: &str) -> Option<f64> {
    let inner = v
        .strip_prefix("radial-gradient(circle, #000 calc(100% - ")?
        .strip_suffix("), transparent)")?;
    vb_common::units::parse_px(inner)
}

/// 纯色填充的渐变承载(像素恒等):`linear-gradient(c, c)`。
pub(super) fn solid_as_layer(c: &str) -> String {
    format!("linear-gradient({c}, {c})")
}

/// 渐变层 → 纯色(同色双 stop;其余 None)。
pub(super) fn layer_as_solid(layer: &str) -> Option<String> {
    let body = layer.strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let segs = vb_css::split_top_level(body, ',');
    if segs.len() == 2 && segs[0].trim() == segs[1].trim() {
        Some(segs[0].trim().to_string())
    } else {
        None
    }
}

pub(super) fn is_gradient_layer(layer: &str) -> bool {
    layer.starts_with("linear-gradient(") || layer.starts_with("radial-gradient(")
}
