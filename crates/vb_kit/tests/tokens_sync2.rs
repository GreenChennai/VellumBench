//! **G-UI1 tokens_sync2**(vb_kit 侧;22 篇 §8):`vb-ui-tokens.json` ↔
//! Rust 常量 ↔ `vb_colors` 注入值逐项比对,任何漂移红。
//!
//! 机制对齐 vb_ui theme.rs 的 tokens_sync(设计真相 JSON + Rust 镜像 +
//! CI 同步测试);数据全部来自 JSON,本文件不维护第二份数值。

use sable::gpui::{rgba, Hsla};
use sable::widgets::theme::ThemeMode;
use vb_kit::tokens::{
    self as vbt, font_size, hex, layout, line_height, motion_ms, radius, space, stroke,
};

/// 设计真相文件(仓库根 `docs/design/assets/vb-ui-tokens.json`;
/// CARGO_MANIFEST_DIR = `crates/vb_kit`,故上两级)。
fn tokens_json() -> serde_json::Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/assets/vb-ui-tokens.json"
    );
    let raw =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("读设计真相失败 {path}: {e}"));
    serde_json::from_str(&raw).expect("vb-ui-tokens.json 不是合法 JSON")
}

/// `#RRGGBB` → `0xRRGGBBFF`(补满不透明)。
fn parse_hex(s: &str) -> u32 {
    let body = s
        .strip_prefix('#')
        .unwrap_or_else(|| panic!("颜色值缺 # 前缀:{s}"));
    assert_eq!(body.len(), 6, "仅支持 #RRGGBB 形态,得:{s}");
    let rgb = u32::from_str_radix(body, 16).expect("非法 hex 颜色");
    (rgb << 8) | 0xFF
}

/// `rgba(r,g,b,a)` → `0xRRGGBBAA`(α = round(a × 255),与 tokens.rs 同规)。
fn parse_rgba_fn(s: &str) -> u32 {
    let inner = s
        .strip_prefix("rgba(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or_else(|| panic!("仅支持 rgba(r,g,b,a) 形态,得:{s}"));
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    assert_eq!(parts.len(), 4, "rgba 需要 4 个分量:{s}");
    let r: u32 = parts[0].parse().expect("r");
    let g: u32 = parts[1].parse().expect("g");
    let b: u32 = parts[2].parse().expect("b");
    let a: f64 = parts[3].parse().expect("a");
    assert!((0.0..=1.0).contains(&a), "alpha 出域:{s}");
    (r << 24) | (g << 16) | (b << 8) | (a * 255.0).round() as u32
}

/// JSON 颜色值(自动识别 `#…` 与 `rgba(…)` 形态)。
fn parse_color(s: &str) -> u32 {
    if s.starts_with('#') {
        parse_hex(s)
    } else {
        parse_rgba_fn(s)
    }
}

/// `"12px"` → `12.0`。
fn parse_px(s: &str) -> f32 {
    s.strip_suffix("px")
        .unwrap_or_else(|| panic!("长度值缺 px 后缀:{s}"))
        .parse()
        .expect("非法 px 数值")
}

/// `"80ms"` → `80.0`。
fn parse_ms(s: &str) -> f32 {
    s.strip_suffix("ms")
        .unwrap_or_else(|| panic!("时长值缺 ms 后缀:{s}"))
        .parse()
        .expect("非法 ms 数值")
}

fn jstr<'a>(v: &'a serde_json::Value, pointer: &str) -> &'a str {
    v.pointer(pointer)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("JSON 缺 {pointer}"))
}

fn jf32(v: &serde_json::Value, pointer: &str) -> f32 {
    parse_px(jstr(v, pointer))
}

/// JSON 键集(排序;`$` 开头的元键如 $note/$description 剔除)。
fn sorted_keys(v: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = v
        .as_object()
        .expect("应为 JSON object")
        .keys()
        .filter(|k| !k.starts_with('$'))
        .cloned()
        .collect();
    keys.sort();
    keys
}

// ---------------------------------------------------------------------------
// 一、hex 常量 ↔ JSON(逐值)
// ---------------------------------------------------------------------------

#[test]
fn dark_hex_constants_match_json() {
    let j = tokens_json();
    let dark = &j["color"]["dark"];
    let cases: [(&str, u32); 17] = [
        ("bg-canvas", hex::DARK_BG_CANVAS),
        ("bg-panel", hex::DARK_BG_PANEL),
        ("bg-raised", hex::DARK_BG_RAISED),
        ("bg-input", hex::DARK_BG_INPUT),
        ("bg-hover", hex::DARK_BG_HOVER),
        ("bg-active", hex::DARK_BG_ACTIVE),
        ("border", hex::DARK_BORDER),
        ("border-strong", hex::DARK_BORDER_STRONG),
        ("text", hex::DARK_TEXT),
        ("text-2", hex::DARK_TEXT_2),
        ("text-3", hex::DARK_TEXT_3),
        ("accent", hex::DARK_ACCENT),
        ("accent-hover", hex::DARK_ACCENT_HOVER),
        ("accent-dim", hex::DARK_ACCENT_DIM),
        ("danger", hex::DARK_DANGER),
        ("warn", hex::DARK_WARN),
        ("success", hex::DARK_SUCCESS),
    ];
    for (name, constant) in cases {
        let json_value = parse_color(jstr(dark, &format!("/{name}/value")));
        assert_eq!(
            constant, json_value,
            "color.dark.{name} 漂移(JSON 是唯一真相)"
        );
    }
}

#[test]
fn light_hex_constants_match_json() {
    let j = tokens_json();
    let light = &j["color"]["light"];
    let cases: [(&str, u32); 16] = [
        ("bg-canvas", hex::LIGHT_BG_CANVAS),
        ("bg-panel", hex::LIGHT_BG_PANEL),
        ("bg-raised", hex::LIGHT_BG_RAISED),
        ("bg-input", hex::LIGHT_BG_INPUT),
        ("bg-hover", hex::LIGHT_BG_HOVER),
        ("bg-active", hex::LIGHT_BG_ACTIVE),
        ("border", hex::LIGHT_BORDER),
        ("border-strong", hex::LIGHT_BORDER_STRONG),
        ("text", hex::LIGHT_TEXT),
        ("text-2", hex::LIGHT_TEXT_2),
        ("text-3", hex::LIGHT_TEXT_3),
        ("accent", hex::LIGHT_ACCENT),
        ("accent-dim", hex::LIGHT_ACCENT_DIM),
        ("danger", hex::LIGHT_DANGER),
        ("warn", hex::LIGHT_WARN),
        ("success", hex::LIGHT_SUCCESS),
    ];
    for (name, constant) in cases {
        let json_value = parse_color(jstr(light, &format!("/{name}/value")));
        assert_eq!(
            constant, json_value,
            "color.light.{name} 漂移(JSON 是唯一真相)"
        );
    }
}

#[test]
fn semantic_constants_match_json() {
    let j = tokens_json();
    let sem = &j["color"]["semantic"];
    let cases: [(&str, u32); 4] = [
        ("guide-smart", hex::SEMANTIC_GUIDE_SMART),
        ("select-box", hex::SEMANTIC_SELECT_BOX),
        ("hover-box", hex::SEMANTIC_HOVER_BOX),
        ("guide-grid", hex::SEMANTIC_GUIDE_GRID),
    ];
    for (name, constant) in cases {
        let json_value = parse_color(jstr(sem, &format!("/{name}/value")));
        assert_eq!(constant, json_value, "color.semantic.{name} 漂移");
    }
    // 浅色变体(semantic.*.light)
    assert_eq!(
        hex::SEMANTIC_GUIDE_SMART_LIGHT,
        parse_color(jstr(sem, "/guide-smart/light")),
        "semantic.guide-smart 浅色漂移"
    );
    assert_eq!(
        hex::SEMANTIC_GUIDE_GRID_LIGHT,
        parse_color(jstr(sem, "/guide-grid/light")),
        "semantic.guide-grid 浅色漂移"
    );
}

// ---------------------------------------------------------------------------
// 二、JSON 键集覆盖(新加令牌未镜像即红,防单向漂移)
// ---------------------------------------------------------------------------

#[test]
fn json_key_sets_are_fully_mirrored() {
    let j = tokens_json();
    // 17 个深色令牌
    assert_eq!(
        sorted_keys(&j["color"]["dark"]),
        vec![
            "accent",
            "accent-dim",
            "accent-hover",
            "bg-active",
            "bg-canvas",
            "bg-hover",
            "bg-input",
            "bg-panel",
            "bg-raised",
            "border",
            "border-strong",
            "danger",
            "success",
            "text",
            "text-2",
            "text-3",
            "warn"
        ]
    );
    // 16 个浅色令牌(JSON 浅色无 accent-hover)
    assert_eq!(
        sorted_keys(&j["color"]["light"]),
        vec![
            "accent",
            "accent-dim",
            "bg-active",
            "bg-canvas",
            "bg-hover",
            "bg-input",
            "bg-panel",
            "bg-raised",
            "border",
            "border-strong",
            "danger",
            "success",
            "text",
            "text-2",
            "text-3",
            "warn"
        ]
    );
    // 间距 8 档 / 圆角 4 档 / 动效 5 档 / 字号 6 档
    assert_eq!(
        sorted_keys(&j["space"]),
        vec!["1", "2", "3", "4", "5", "6", "8", "9"]
    );
    assert_eq!(sorted_keys(&j["radius"]), vec!["lg", "md", "sm", "xl"]);
    assert_eq!(
        sorted_keys(&j["motion"]),
        vec!["hover", "instant", "panel", "popup", "state"]
    );
    assert_eq!(
        sorted_keys(&j["font"]["size"]),
        vec!["body", "body-strong", "caption", "label", "mono", "title"]
    );
}

// ---------------------------------------------------------------------------
// 三、注入投影 ↔ JSON(vb_colors 的 15 槽位逐项)
// ---------------------------------------------------------------------------

/// 逐槽位断言:`tokens.surface_0 == Hsla(parse(json["color"][mode][名]))`。
fn assert_slot(slot: &Hsla, json: &serde_json::Value, section: &str, name: &str, what: &str) {
    let expected: Hsla = rgba(parse_color(
        json.pointer(&format!("/color/{section}/{name}/value"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("JSON 缺 color.{section}.{name}")),
    ))
    .into();
    assert_eq!(slot, &expected, "{what} 漂移(color.{section}.{name})");
}

#[test]
fn injected_dark_tokens_match_json() {
    let j = tokens_json();
    let t = vbt::vb_colors(ThemeMode::Dark);
    assert_slot(&t.surface_0, &j, "dark", "bg-canvas", "surface_0");
    assert_slot(&t.surface_1, &j, "dark", "bg-panel", "surface_1");
    assert_slot(&t.surface_2, &j, "dark", "bg-input", "surface_2");
    assert_slot(&t.surface_3, &j, "dark", "bg-hover", "surface_3");
    assert_slot(&t.surface_4, &j, "dark", "bg-active", "surface_4");
    assert_slot(&t.border_subtle, &j, "dark", "border", "border_subtle");
    assert_slot(
        &t.border_strong,
        &j,
        "dark",
        "border-strong",
        "border_strong",
    );
    assert_slot(&t.text_primary, &j, "dark", "text", "text_primary");
    assert_slot(&t.text_secondary, &j, "dark", "text-2", "text_secondary");
    assert_slot(&t.text_disabled, &j, "dark", "text-3", "text_disabled");
    assert_slot(&t.accent, &j, "dark", "accent", "accent");
    assert_slot(&t.accent_muted, &j, "dark", "accent-dim", "accent_muted");
    assert_slot(&t.danger, &j, "dark", "danger", "danger");
    assert_slot(&t.warning, &j, "dark", "warn", "warning");
    assert_slot(&t.success, &j, "dark", "success", "success");
}

#[test]
fn injected_light_tokens_match_json() {
    let j = tokens_json();
    let t = vbt::vb_colors(ThemeMode::Light);
    assert_slot(&t.surface_0, &j, "light", "bg-canvas", "surface_0");
    assert_slot(&t.surface_1, &j, "light", "bg-panel", "surface_1");
    assert_slot(&t.surface_2, &j, "light", "bg-input", "surface_2");
    assert_slot(&t.surface_3, &j, "light", "bg-hover", "surface_3");
    assert_slot(&t.surface_4, &j, "light", "bg-active", "surface_4");
    assert_slot(&t.border_subtle, &j, "light", "border", "border_subtle");
    assert_slot(
        &t.border_strong,
        &j,
        "light",
        "border-strong",
        "border_strong",
    );
    assert_slot(&t.text_primary, &j, "light", "text", "text_primary");
    assert_slot(&t.text_secondary, &j, "light", "text-2", "text_secondary");
    assert_slot(&t.text_disabled, &j, "light", "text-3", "text_disabled");
    assert_slot(&t.accent, &j, "light", "accent", "accent");
    assert_slot(&t.accent_muted, &j, "light", "accent-dim", "accent_muted");
    assert_slot(&t.danger, &j, "light", "danger", "danger");
    assert_slot(&t.warning, &j, "light", "warn", "warning");
    assert_slot(&t.success, &j, "light", "success", "success");
}

// ---------------------------------------------------------------------------
// 四、数值令牌:间距 / 圆角 / 描边 / 动效 / 字号 / 行高 / 结构常量
// ---------------------------------------------------------------------------

#[test]
fn space_constants_match_json() {
    let j = tokens_json();
    let s = &j["space"];
    assert_eq!(space::S1, jf32(s, "/1/value"));
    assert_eq!(space::S2, jf32(s, "/2/value"));
    assert_eq!(space::S3, jf32(s, "/3/value"));
    assert_eq!(space::S4, jf32(s, "/4/value"));
    assert_eq!(space::S5, jf32(s, "/5/value"));
    assert_eq!(space::S6, jf32(s, "/6/value"));
    assert_eq!(space::S8, jf32(s, "/8/value"));
    assert_eq!(space::S9, jf32(s, "/9/value"));
}

#[test]
fn radius_constants_match_json() {
    let j = tokens_json();
    let r = &j["radius"];
    assert_eq!(radius::SM, jf32(r, "/sm/value"));
    assert_eq!(radius::MD, jf32(r, "/md/value"));
    assert_eq!(radius::LG, jf32(r, "/lg/value"));
    assert_eq!(radius::XL, jf32(r, "/xl/value"));
}

#[test]
fn stroke_constants_match_json() {
    let j = tokens_json();
    assert_eq!(stroke::HAIRLINE, jf32(&j["stroke"], "/hairline/value"));
    assert_eq!(stroke::FOCUS, jf32(&j["stroke"], "/focus/value"));
}

#[test]
fn motion_constants_match_json() {
    let j = tokens_json();
    let m = &j["motion"];
    let ms = |pointer: &str| parse_ms(jstr(m, pointer));
    assert_eq!(motion_ms::INSTANT, ms("/instant/value"));
    assert_eq!(motion_ms::HOVER, ms("/hover/value"));
    assert_eq!(motion_ms::STATE, ms("/state/value"));
    assert_eq!(motion_ms::POPUP, ms("/popup/value"));
    assert_eq!(motion_ms::PANEL, ms("/panel/value"));
}

#[test]
fn font_size_and_line_height_match_json() {
    let j = tokens_json();
    let sizes = &j["font"]["size"];
    assert_eq!(font_size::CAPTION, jf32(sizes, "/caption/size"));
    assert_eq!(font_size::LABEL, jf32(sizes, "/label/size"));
    assert_eq!(font_size::BODY, jf32(sizes, "/body/size"));
    assert_eq!(font_size::BODY_STRONG, jf32(sizes, "/body-strong/size"));
    assert_eq!(font_size::TITLE, jf32(sizes, "/title/size"));
    assert_eq!(font_size::MONO, jf32(sizes, "/mono/size"));
    assert_eq!(line_height::CAPTION, jf32(sizes, "/caption/line_height"));
    assert_eq!(line_height::LABEL, jf32(sizes, "/label/line_height"));
    assert_eq!(line_height::BODY, jf32(sizes, "/body/line_height"));
    assert_eq!(
        line_height::BODY_STRONG,
        jf32(sizes, "/body-strong/line_height")
    );
    assert_eq!(line_height::TITLE, jf32(sizes, "/title/line_height"));
    assert_eq!(line_height::MONO, jf32(sizes, "/mono/line_height"));
}

#[test]
fn layout_constants_match_json() {
    let j = tokens_json();
    let l = &j["layout"];
    assert_eq!(layout::MENU_BAR, jf32(l, "/menu-bar/height"));
    assert_eq!(layout::STATUS_BAR, jf32(l, "/status-bar/height"));
    assert_eq!(layout::RULER, jf32(l, "/ruler/height"));
    assert_eq!(
        layout::RIGHT_DOCK_DEFAULT,
        jf32(l, "/right-dock/width_default")
    );
    assert_eq!(layout::RIGHT_DOCK_MIN, jf32(l, "/right-dock/width_min"));
    assert_eq!(layout::RIGHT_DOCK_MAX, jf32(l, "/right-dock/width_max"));
    assert_eq!(
        layout::RIGHT_DOCK_COLLAPSED,
        jf32(l, "/right-dock/collapsed")
    );
    assert_eq!(layout::COLLAPSE_BELOW, jf32(l, "/collapse-below/value"));
    assert_eq!(layout::ROW_HEIGHT_LAYER, jf32(l, "/row-height/layer"));
    assert_eq!(layout::ROW_HEIGHT_LIST, jf32(l, "/row-height/list_item"));
    // 最小窗口 "1024x640"
    let min_window = jstr(l, "/min-window/value");
    let (w, h) = min_window.split_once('x').expect("min-window 形态 WxH");
    assert_eq!(layout::MIN_WINDOW_W, w.parse::<f32>().expect("w"));
    assert_eq!(layout::MIN_WINDOW_H, h.parse::<f32>().expect("h"));
}

// ---------------------------------------------------------------------------
// 五、语义 px 值(吸附阈值 / 手柄命中)
// ---------------------------------------------------------------------------

#[test]
fn semantic_screen_px_constants_match_json() {
    let j = tokens_json();
    let sem = &j["color"]["semantic"];
    assert_eq!(vbt::SNAP_THRESHOLD_PX, jf32(sem, "/snap-threshold/value"));
    assert_eq!(
        vbt::HANDLE_HIT_RADIUS_PX,
        jf32(sem, "/handle-hit-radius/value")
    );
}
