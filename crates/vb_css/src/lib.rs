//! `vb_css` — CSS 声明层的白名单、解析与规范化。
//!
//! 设计(ADR-0004):分级白名单 + `unknown` 保底 —— 白名单外的属性**永不丢弃**,
//! 原样保留原样写回("不编辑也不损坏")。
//!
//! 本 crate 只处理**声明**(prop: value)级别;规则/选择器层在 `vb_doc` 导入器中。

use vb_common::color::{parse_color, Rgba};

/// L1 白名单(v0.1,设计文档 04 篇 §三)。
/// 顺序即输出顺序(PROP_ORDER):位置 → 尺寸 → 盒 → 定位 → 背景 → 边框 → 阴影
/// → 文本 → 变换 → 视觉 → 交互。
pub const L1_PROPS: &[&str] = &[
    // 定位
    "position",
    "left",
    "top",
    "right",
    "bottom",
    "z-index",
    // 尺寸
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "aspect-ratio",
    // 盒模型
    "box-sizing",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    // 布局(flex 基础)
    "display",
    "flex-direction",
    "flex-wrap",
    "gap",
    "justify-content",
    "align-items",
    "align-self",
    "flex",
    // 背景
    "background-color",
    "background-image",
    "background-size",
    "background-position",
    "background-repeat",
    // 边框
    "border",
    "border-width",
    "border-style",
    "border-color",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-style",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
    "outline",
    // 阴影
    "box-shadow",
    "text-shadow",
    // 文本
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "letter-spacing",
    "text-align",
    "color",
    "text-decoration",
    "text-transform",
    "white-space",
    "overflow-wrap",
    // 变换
    "transform",
    "transform-origin",
    // 视觉
    "opacity",
    "mix-blend-mode",
    "overflow",
    "visibility",
    "clip-path",
    "object-fit",
    // 交互
    "cursor",
];

/// 属性输出顺序档位:白名单内 = 表内下标;白名单外 = 10_000 + 字母序(确定性,保证 L1 幂等)。
pub fn prop_rank(prop: &str) -> usize {
    L1_PROPS
        .iter()
        .position(|p| *p == prop)
        .unwrap_or(10_000 + prop.as_bytes().iter().map(|b| *b as usize).sum::<usize>() % 10_000)
}

pub fn is_known_prop(prop: &str) -> bool {
    L1_PROPS.iter().any(|p| *p == prop)
}

/// 一条 CSS 声明。`value` 一律为规范化后的形式。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decl {
    pub prop: String,
    pub value: String,
    pub important: bool,
}

impl Decl {
    /// 解析单条 `"prop: value"`(不含分号)。白名单外照样解析(unknown 保底)。
    pub fn parse(text: &str) -> Option<Decl> {
        let (prop, value) = text.split_once(':')?;
        let prop = prop.trim().to_ascii_lowercase();
        if prop.is_empty() || !prop.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return None;
        }
        if !prop.starts_with(|c: char| c.is_ascii_lowercase() || c == '-') {
            return None;
        }
        let (value, important) = strip_important(value);
        if value.is_empty() {
            return None;
        }
        Some(Decl {
            prop,
            value: canonical_value(&value),
            important,
        })
    }

    pub fn to_css(&self) -> String {
        let imp = if self.important { " !important" } else { "" };
        format!("{}: {}{imp}", self.prop, self.value)
    }

    pub fn is_unknown(&self) -> bool {
        !is_known_prop(&self.prop)
    }
}

fn strip_important(value: &str) -> (String, bool) {
    let t = value.trim();
    if let Some(pos) = t.rfind("!important") {
        // !important 通常在末尾(可能在 `red !important` 中)
        let head = t[..pos].trim_end();
        if head != t {
            return (head.to_string(), true);
        }
    }
    (t.to_string(), false)
}

/// 规范化属性值:
/// - 折叠空白;逗号后一个空格
/// - hex 颜色 → 小写最短;rgb()/rgba() → hex;命名色 → hex
/// - 数字保留 ≤4 位小数去尾 0;`0px` → `0`;单位小写
/// - 函数名小写;引号内不改动
pub fn canonical_value(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    let mut in_string: Option<char> = None;
    let mut last_ws = false;

    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = in_string {
            out.push(c);
            if c == q {
                in_string = None;
            }
            i += 1;
            continue;
        }
        match c {
            '"' | '\'' => {
                in_string = Some(c);
                out.push(c);
                last_ws = false;
                i += 1;
            }
            '(' => {
                out.push(c);
                last_ws = false;
                i += 1;
            }
            ')' => {
                out.push(c);
                last_ws = false;
                i += 1;
            }
            ',' => {
                out.push_str(", ");
                last_ws = true;
                i += 1;
                // 跳过逗号后空白
                while i < chars.len() && chars[i].is_whitespace() {
                    i += 1;
                }
            }
            c if c.is_whitespace() => {
                if !last_ws {
                    out.push(' ');
                    last_ws = true;
                }
                i += 1;
            }
            '#' => {
                // 颜色 token
                let start = i + 1;
                let mut end = start;
                while end < chars.len() && chars[end].is_ascii_hexdigit() {
                    end += 1;
                }
                let hexs: String = chars[start..end].iter().collect();
                if let Some(rgba) = parse_hex_len(&hexs) {
                    out.push_str(&rgba.to_shortest_hex());
                } else {
                    out.push('#');
                    out.push_str(&hexs);
                }
                i = end;
                last_ws = false;
            }
            c if c.is_ascii_digit()
                || ((c == '-' || c == '+')
                    && chars
                        .get(i + 1)
                        .map(|n| n.is_ascii_digit() || *n == '.')
                        .unwrap_or(false)
                    && (out.is_empty()
                        || out.ends_with(' ')
                        || out.ends_with('(')
                        || out.ends_with(',')))
                || (c == '.' && chars.get(i + 1).map(|n| n.is_ascii_digit()).unwrap_or(false)
                    && (out.is_empty()
                        || out.ends_with(' ')
                        || out.ends_with('(')
                        || out.ends_with(','))) =>
            {
                // 数字 token(含符号/小数点开头)
                let mut j = i;
                if chars[j] == '-' || chars[j] == '+' {
                    j += 1;
                }
                let mut seen_dot = false;
                while j < chars.len()
                    && (chars[j].is_ascii_digit()
                        || (chars[j] == '.' && !seen_dot)
                        || ((chars[j] == 'e' || chars[j] == 'E')
                            && j + 1 < chars.len()
                            && (chars[j + 1].is_ascii_digit() || chars[j + 1] == '-')))
                {
                    if chars[j] == '.' {
                        seen_dot = true;
                    }
                    j += 1;
                }
                let num_s: String = chars[i..j].iter().collect();
                // 单位
                let mut k = j;
                while k < chars.len() && (chars[k].is_ascii_alphabetic() || chars[k] == '%') {
                    k += 1;
                }
                let unit: String = chars[j..k].iter().collect::<String>().to_ascii_lowercase();
                match num_s.parse::<f64>() {
                    Ok(n) if n.is_finite() => {
                        if n == 0.0 {
                            // 0 不带单位(设计文档 04 §5.2)
                            out.push('0');
                        } else {
                            out.push_str(&vb_common::units::fmt_num(n));
                            out.push_str(&unit);
                        }
                    }
                    _ => {
                        out.push_str(&num_s);
                        out.push_str(&unit);
                    }
                }
                i = k;
                last_ws = false;
            }
            c => {
                // 函数名:ident 后紧跟 '(' → 小写
                if c.is_ascii_alphabetic() || c == '-' {
                    let mut j = i;
                    while j < chars.len()
                        && (chars[j].is_ascii_alphanumeric() || chars[j] == '-' || chars[j] == '_')
                    {
                        j += 1;
                    }
                    if j < chars.len() && chars[j] == '(' {
                        let name: String = chars[i..j].iter().collect::<String>().to_ascii_lowercase();
                        out.push_str(&name);
                        i = j;
                        continue;
                    } else {
                        let word: String = chars[i..j].iter().collect();
                        out.push_str(&word);
                        i = j.max(i + 1);
                        last_ws = false;
                        continue;
                    }
                }
                out.push(c);
                last_ws = false;
                i += 1;
            }
        }
    }
    out.trim().to_string()
}

fn parse_hex_len(h: &str) -> Option<Rgba> {
    match h.len() {
        3 => Some(Rgba::new(
            u8::from_str_radix(&h[0..1], 16).ok()? * 17,
            u8::from_str_radix(&h[1..2], 16).ok()? * 17,
            u8::from_str_radix(&h[2..3], 16).ok()? * 17,
            255,
        )),
        6 => Some(Rgba::new(
            u8::from_str_radix(&h[0..2], 16).ok()?,
            u8::from_str_radix(&h[2..4], 16).ok()?,
            u8::from_str_radix(&h[4..6], 16).ok()?,
            255,
        )),
        _ => parse_color(&format!("#{h}")).or(parse_color(h)),
    }
}

/// 解析一段声明列表(`"a:1px;b:2px"`),按 `;` 切分(括号/引号感知)。
pub fn parse_decls(css_text: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for part in split_top_level(css_text, ';') {
        if let Some(d) = Decl::parse(&part) {
            out.push(d);
        }
    }
    out
}

/// 在括号深度 0、不在字符串内时按 `sep` 切分。
pub fn split_top_level(text: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    for c in text.chars() {
        if let Some(q) = in_string {
            cur.push(c);
            if c == q {
                in_string = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                in_string = Some(c);
                cur.push(c);
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            c if c == sep && depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// 按输出顺序(PROP_ORDER)稳定排序;白名单外按确定性散列档位排在其后。
pub fn sort_decls(decls: &mut [Decl]) {
    decls.sort_by_key(|d| prop_rank(&d.prop));
}

/// 声明列表 → 内联 style 属性值(`"a: 1px; b: 2px"`)。
pub fn decls_to_style_attr(decls: &[Decl]) -> String {
    decls
        .iter()
        .map(|d| d.to_css())
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_canonicalize() {
        let d = Decl::parse("LEFT: 12.5000px").unwrap();
        assert_eq!(d.prop, "left");
        assert_eq!(d.value, "12.5");

        let d = Decl::parse("background-image: linear-gradient( 180deg , #2B1A12 0%, #6B3F24 100%)").unwrap();
        assert_eq!(
            d.value,
            "linear-gradient(180deg, #2b1a12 0%, #6b3f24 100%)"
        );

        let d = Decl::parse("width: 0px").unwrap();
        assert_eq!(d.value, "0");

        let d = Decl::parse("font-family: \"Inter\", sans-serif").unwrap();
        assert_eq!(d.value, "\"Inter\", sans-serif");

        let d = Decl::parse("color: red !important").unwrap();
        assert_eq!(d.value, "#f00");
        assert!(d.important);

        // unknown 保底
        let d = Decl::parse("backdrop-filter: blur(8px)").unwrap();
        assert!(d.is_unknown());
        assert_eq!(d.value, "blur(8px)");
    }

    #[test]
    fn decl_list_and_order() {
        let mut v = parse_decls("color:#fff; position:absolute; top:0px; backdrop-filter:blur(2px); left:10px");
        sort_decls(&mut v);
        let props: Vec<&str> = v.iter().map(|d| d.prop.as_str()).collect();
        assert_eq!(props, vec!["position", "left", "top", "color", "backdrop-filter"]);
        // 幂等
        let again = {
            let mut v2 = v.clone();
            sort_decls(&mut v2);
            v2
        };
        assert_eq!(v, again);
    }

    #[test]
    fn split_respects_parens_and_strings() {
        let parts = split_top_level("background-image:url(data:image/png;base64,xx), red; color:#fff", ';');
        assert_eq!(parts.len(), 2);
    }
}
