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
    // 背景层混合(S4 外观面板:多条填充条目级混合模式的落点)
    "background-blend-mode",
    // 边框
    "border",
    "border-width",
    "border-style",
    "border-color",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-radius",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
    // 轮廓(S4 描边面板:外侧对齐描边的落点;不占布局,画在 border 之外)
    "outline",
    "outline-width",
    "outline-style",
    "outline-color",
    "outline-offset",
    // 阴影
    "box-shadow",
    "text-shadow",
    // SVG 表现属性(S4 描边面板:矢量路径的通用描边落点;钢笔 v0.1
    // 已在写 fill/stroke/stroke-width,入白名单只为输出顺序稳定)
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-opacity",
    // 文本
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "line-height",
    "letter-spacing",
    "text-align",
    "text-align-last",
    "vertical-align",
    "text-indent",
    "color",
    // 文字描边(S4 外观面板:文字对象描边落点;厂商前缀事实标准)
    "-webkit-text-stroke",
    "-webkit-text-stroke-width",
    "-webkit-text-stroke-color",
    "text-decoration",
    "text-transform",
    "white-space",
    "overflow-wrap",
    // 中文排版与连字(04 阶段字符/段落面板;标准 CSS 属性,
    // unknown 保底本就原样保留,入白名单只为输出顺序稳定)
    "line-break",
    "hyphens",
    "hanging-punctuation",
    "text-wrap",
    // 抗锯齿(厂商前缀事实标准,design/03 §5.10 抗锯齿字段的落点)
    "-webkit-font-smoothing",
    // 变换
    "transform",
    "transform-origin",
    // 视觉
    "opacity",
    "mix-blend-mode",
    // 滤镜与蒙版(S4 外观面板:高斯模糊 → filter;羽化 → mask-image 近似)
    "filter",
    "mask-image",
    // 层叠上下文/分组隔离(S4-b 透明度面板:挖空组 → isolation:isolate)
    "isolation",
    "overflow",
    "visibility",
    "clip-path",
    "object-fit",
    // 交互
    "cursor",
];

/// 属性输出顺序档位:白名单内 = 表内下标;白名单外 = 统一档位 10_000
/// (DOC-16:同级次序由 [`sort_decls`] 的**真字典序**决胜 —— 此前用
/// 「字节和散列」冒充字母序,散列碰撞会让两未知属性的相对序随输入序
/// 漂移,与契约「确定性、保证 L1 幂等」不符)。
pub fn prop_rank(prop: &str) -> usize {
    L1_PROPS.iter().position(|p| *p == prop).unwrap_or(10_000)
}

pub fn is_known_prop(prop: &str) -> bool {
    L1_PROPS.contains(&prop)
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
        // 前导注释(`; /* c */ color:red` 里注释落在 prop 侧)先剥掉;
        // 未闭合注释 → 整条无效(与浏览器一致)
        let mut rest = text.trim_start();
        while let Some(r) = rest.strip_prefix("/*") {
            let end = r.find("*/")?;
            rest = r[end + 2..].trim_start();
        }
        let (prop, value) = rest.split_once(':')?;
        let raw = prop.trim();
        // 自定义属性(--*)区分大小写:--brandColor 与 --brandcolor 是两个变量,
        // 小写化会让 var(--brandColor) 引用断裂(CSS 规范行为)
        let is_custom = raw.starts_with("--");
        let prop = if is_custom {
            raw.to_string()
        } else {
            raw.to_ascii_lowercase()
        };
        let prop_ok = prop.chars().all(|c| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || c == '-'
                || (is_custom && (c.is_ascii_uppercase() || c == '_'))
        });
        if prop.is_empty() || !prop_ok {
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
        // PERF-08:按部件长度预留容量,单次分配(format! 多轮扫描)
        let mut s = String::with_capacity(self.prop.len() + self.value.len() + imp.len() + 2);
        s.push_str(&self.prop);
        s.push_str(": ");
        s.push_str(&self.value);
        s.push_str(imp);
        s
    }

    /// 直接写入调用方缓冲(PERF-08:导出热路径逐声明调用,免中间 String)。
    pub fn push_css(&self, out: &mut String) {
        out.push_str(&self.prop);
        out.push_str(": ");
        out.push_str(&self.value);
        if self.important {
            out.push_str(" !important");
        }
    }

    pub fn is_unknown(&self) -> bool {
        !is_known_prop(&self.prop)
    }
}

/// 剥离末尾 `!important`(CSS 规范:只能出现在声明值末尾、字符串之外)。
/// 此前 `rfind("!important")` 子串匹配不感知字符串,`content: "x !important"`
/// 的值被拦腰截断并误标 important。
fn strip_important(value: &str) -> (String, bool) {
    let t = value.trim();
    let mut in_string: Option<char> = None;
    let mut candidate: Option<usize> = None;
    let mut i = 0usize;
    while i < t.len() {
        let c = t[i..].chars().next().unwrap();
        if let Some(q) = in_string {
            if c == q {
                in_string = None;
            }
        } else {
            match c {
                '"' | '\'' => in_string = Some(c),
                '!' if t[i..].starts_with("!important") => candidate = Some(i),
                _ => {}
            }
        }
        i += c.len_utf8();
    }
    match candidate {
        Some(pos) => (t[..pos].trim_end().to_string(), true),
        None => (t.to_string(), false),
    }
}

/// `0` 也必须带单位的单位族(角度与时间)。
///
/// 「0 不带单位」是**长度**的规则(设计文档 04 §5.2):`0px` → `0` 合法。
/// 但角度/时间没有无单位形式 —— `linear-gradient(0, …)`、
/// `animation-duration: 0` 都是**非法 CSS**。此前一刀切会让 `0deg` 的渐变
/// 被规范化成非法值,并在二次导入时改变字节(L1 幂等破损)。
const ZERO_KEEPS_UNIT: &[&str] = &["deg", "grad", "rad", "turn", "s", "ms"];

/// 规范化属性值:
/// - 折叠空白;逗号后一个空格
/// - hex 颜色 → 小写最短;rgb()/rgba() → hex;命名色 → hex
/// - 数字保留 ≤4 位小数去尾 0;`0px` → `0`(角度/时间单位除外,见
///   [`ZERO_KEEPS_UNIT`]);单位小写
/// - 函数名小写;引号内不改动
///
/// PERF-08:字节扫描替代 `Vec<char>` 全量收集 —— 全部 token 判定都是
/// ASCII 谓词(多字节字符只出现在「原样推送」分支,按 UTF-8 逐字解码),
/// 字节下标与字符边界在此等价;token 切片直接借用 `raw`(不再逐 token
/// collect String),规范输出与旧实现逐字节一致。
pub fn canonical_value(raw: &str) -> String {
    /// 当前字节下标处的字符(i 恒落在 UTF-8 边界:ASCII 前进 1 字节,
    /// 多字节按 `len_utf8` 前进)。
    fn char_at(raw: &str, i: usize) -> Option<char> {
        raw.get(i..).and_then(|s| s.chars().next())
    }

    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0usize;
    let mut in_string: Option<char> = None;
    let mut last_ws = false;
    // 函数调用栈:url(#id) 里的 # 是 SVG 引用不是颜色,不能缩短
    let mut fn_stack: Vec<String> = Vec::new();

    while i < bytes.len() {
        let b = bytes[i];
        let c = if b.is_ascii() {
            b as char
        } else {
            match char_at(raw, i) {
                Some(c) => c,
                None => break,
            }
        };
        if let Some(q) = in_string {
            out.push(c);
            if c == q {
                in_string = None;
            }
            i += c.len_utf8();
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
                last_ws = true; // '(' 后不留空白
                i += 1;
            }
            ')' => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push(c);
                fn_stack.pop();
                last_ws = false;
                i += 1;
            }
            ',' => {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push_str(", ");
                last_ws = true;
                i += 1;
                // 跳过逗号后空白(Unicode 口径与旧实现一致)
                while i < bytes.len() {
                    match char_at(raw, i) {
                        Some(w) if w.is_whitespace() => i += w.len_utf8(),
                        _ => break,
                    }
                }
            }
            c if c.is_whitespace() => {
                if !last_ws {
                    out.push(' ');
                    last_ws = true;
                }
                i += c.len_utf8();
            }
            '#' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                    end += 1;
                }
                let hexs: &str = &raw[start..end];
                if fn_stack.last().map(|f| f == "url").unwrap_or(false) {
                    // url(#fragment):SVG/clip-path 引用,逐字保留
                    out.push('#');
                    out.push_str(hexs);
                } else if let Some(rgba) = parse_hex_len(hexs) {
                    out.push_str(&rgba.to_shortest_hex());
                } else {
                    out.push('#');
                    out.push_str(hexs);
                }
                i = end;
                last_ws = false;
            }
            c if c.is_ascii_digit()
                || ((c == '-' || c == '+')
                    && bytes
                        .get(i + 1)
                        .map(|n| n.is_ascii_digit() || *n == b'.')
                        .unwrap_or(false)
                    && (out.is_empty()
                        || out.ends_with(' ')
                        || out.ends_with('(')
                        || out.ends_with(',')))
                || (c == '.'
                    && bytes
                        .get(i + 1)
                        .map(|n| n.is_ascii_digit())
                        .unwrap_or(false)
                    && (out.is_empty()
                        || out.ends_with(' ')
                        || out.ends_with('(')
                        || out.ends_with(','))) =>
            {
                // 数字 token(含符号/小数点开头)
                let mut j = i;
                if bytes[j] == b'-' || bytes[j] == b'+' {
                    j += 1;
                }
                let mut seen_dot = false;
                while j < bytes.len()
                    && (bytes[j].is_ascii_digit()
                        || (bytes[j] == b'.' && !seen_dot)
                        || ((bytes[j] == b'e' || bytes[j] == b'E')
                            && j + 1 < bytes.len()
                            && (bytes[j + 1].is_ascii_digit() || bytes[j + 1] == b'-')))
                {
                    if bytes[j] == b'.' {
                        seen_dot = true;
                    }
                    j += 1;
                }
                let num_s: &str = &raw[i..j];
                // 单位
                let mut k = j;
                while k < bytes.len() && (bytes[k].is_ascii_alphabetic() || bytes[k] == b'%') {
                    k += 1;
                }
                let unit_raw: &str = &raw[j..k];
                // 单位小写(与旧实现同规):本就小写时零分配
                let unit_lower;
                let unit: &str = if unit_raw.bytes().any(|b| b.is_ascii_uppercase()) {
                    unit_lower = unit_raw.to_ascii_lowercase();
                    &unit_lower
                } else {
                    unit_raw
                };
                match num_s.parse::<f64>() {
                    Ok(n) if n.is_finite() => {
                        if n == 0.0 && !ZERO_KEEPS_UNIT.contains(&unit) {
                            // 0 不带单位(设计文档 04 §5.2)
                            out.push('0');
                        } else {
                            out.push_str(&vb_common::units::fmt_num(n));
                            out.push_str(unit);
                        }
                    }
                    _ => {
                        out.push_str(num_s);
                        out.push_str(unit);
                    }
                }
                i = k;
                last_ws = false;
            }
            c => {
                // 函数名:ident 后紧跟 '(' → 小写
                if c.is_ascii_alphabetic() || c == '-' {
                    let mut j = i;
                    while j < bytes.len()
                        && (bytes[j].is_ascii_alphanumeric()
                            || bytes[j] == b'-'
                            || bytes[j] == b'_')
                    {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b'(' {
                        let name: String = raw[i..j].to_ascii_lowercase();
                        out.push_str(&name);
                        fn_stack.push(name);
                        i = j;
                        continue;
                    } else {
                        out.push_str(&raw[i..j]);
                        i = j.max(i + 1);
                        last_ws = false;
                        continue;
                    }
                }
                out.push(c);
                last_ws = false;
                i += c.len_utf8();
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

/// 在括号深度 0、不在字符串/注释内时按 `sep` 切分;注释内容保留在所在
/// 分段里(值内注释是合法 CSS)。此前 `/* a;b */` 内的分号会吞掉后续声明。
pub fn split_top_level(text: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut in_string: Option<char> = None;
    let mut in_comment = false;
    let mut comment_close = false; // 注释内上一字符是 '*',可能构成 '*/'
    let mut pending_slash = false; // 字符串外的 '/',等下一字符判断是否开注释
    for c in text.chars() {
        if pending_slash {
            pending_slash = false;
            if c == '*' {
                in_comment = true;
                cur.push('/');
                cur.push('*');
                continue;
            }
            // '/' 是字面量(calc(1/2)、url 路径):补回,当前字符继续正常处理
            cur.push('/');
        }
        if in_comment {
            cur.push(c);
            if comment_close && c == '/' {
                in_comment = false;
                comment_close = false;
            } else {
                comment_close = c == '*';
            }
            continue;
        }
        if let Some(q) = in_string {
            cur.push(c);
            if c == q {
                in_string = None;
            }
            continue;
        }
        if c == '/' {
            pending_slash = true;
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
    if pending_slash {
        cur.push('/');
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// 按输出顺序(PROP_ORDER)稳定排序;白名单外同级按**真字典序**决胜
/// (DOC-16:同名属性之间次序与输入序无关,canonical 输出跨路径一致)。
pub fn sort_decls(decls: &mut [Decl]) {
    decls.sort_by(|a, b| {
        prop_rank(&a.prop)
            .cmp(&prop_rank(&b.prop))
            .then_with(|| a.prop.cmp(&b.prop))
    });
}

/// 声明列表 → 内联 style 属性值(`"a: 1px; b: 2px"`;PERF-08:单缓冲直写)。
pub fn decls_to_style_attr(decls: &[Decl]) -> String {
    let mut out = String::new();
    for (i, d) in decls.iter().enumerate() {
        if i > 0 {
            out.push_str("; ");
        }
        d.push_css(&mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_canonicalize() {
        let d = Decl::parse("LEFT: 12.5000px").unwrap();
        assert_eq!(d.prop, "left");
        assert_eq!(d.value, "12.5px");

        let d =
            Decl::parse("background-image: linear-gradient( 180deg , #2B1A12 0%, #6B3F24 100%)")
                .unwrap();
        assert_eq!(d.value, "linear-gradient(180deg, #2b1a12 0, #6b3f24 100%)");

        let d = Decl::parse("width: 0px").unwrap();
        assert_eq!(d.value, "0");

        let d = Decl::parse("font-family: \"Inter\", sans-serif").unwrap();
        assert_eq!(d.value, "\"Inter\", sans-serif");

        let d = Decl::parse("color: red !important").unwrap();
        assert_eq!(d.value, "red"); // 命名色保持原样(已是规范形式)
        assert!(d.important);

        // unknown 保底
        let d = Decl::parse("backdrop-filter: blur(8px)").unwrap();
        assert!(d.is_unknown());
        assert_eq!(d.value, "blur(8px)");
    }

    #[test]
    fn decl_list_and_order() {
        let mut v = parse_decls(
            "color:#fff; position:absolute; top:0px; backdrop-filter:blur(2px); left:10px",
        );
        sort_decls(&mut v);
        let props: Vec<&str> = v.iter().map(|d| d.prop.as_str()).collect();
        assert_eq!(
            props,
            vec!["position", "left", "top", "color", "backdrop-filter"]
        );
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
        let parts = split_top_level(
            "background-image:url(data:image/png;base64,xx), red; color:#fff",
            ';',
        );
        assert_eq!(parts.len(), 2);
    }

    /// 04 阶段(字符/段落面板)新增的文本属性必须入白名单:
    /// 已知属性获得稳定输出顺序(PROP_ORDER),L1 往返幂等依赖这一点。
    #[test]
    fn text_props_of_phase04_are_known() {
        for p in [
            "text-align-last",
            "vertical-align",
            "text-indent",
            "line-break",
            "hyphens",
            "hanging-punctuation",
            "text-wrap",
            "-webkit-font-smoothing",
        ] {
            assert!(is_known_prop(p), "{p} 应在 L1 白名单");
            let d = Decl::parse(&format!("{p}: none")).expect("应可解析");
            assert!(!d.is_unknown());
        }
    }

    /// S4(外观/描边面板)新增的外观属性必须入白名单:效果映射
    /// (filter/mask-image)、文字描边(-webkit-text-stroke*)、
    /// 背景层混合(background-blend-mode)、外描边(outline-*)、
    /// SVG 表现属性(fill/stroke 系)—— 每条带解析往返。
    #[test]
    fn appearance_props_of_phase_s4_are_known() {
        let cases: &[(&str, &str)] = &[
            ("filter", "blur(4px)"),
            ("mask-image", "radial-gradient(circle, #000, transparent)"),
            ("-webkit-text-stroke", "2px #1a1a1a"),
            ("-webkit-text-stroke-width", "2px"),
            ("-webkit-text-stroke-color", "#1a1a1a"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            ("background-blend-mode", "multiply, normal"),
            ("outline", "3px solid #ff0000"),
            ("outline-width", "3px"),
            ("outline-style", "dashed"),
            ("outline-color", "#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            ("outline-offset", "0px"),
            ("fill", "#d4d4d4"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            ("fill-opacity", "0.5"),
            ("fill-rule", "evenodd"),
            ("stroke", "#1a1a1a"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            ("stroke-width", "1.5px"),
            ("stroke-linecap", "round"),
            ("stroke-linejoin", "bevel"),
            ("stroke-miterlimit", "4"),
            ("stroke-dasharray", "6px, 3px"),
            ("stroke-dashoffset", "0"),
            ("stroke-opacity", "1"),
        ];
        for (p, v) in cases {
            assert!(is_known_prop(p), "{p} 应在 L1 白名单");
            let d = Decl::parse(&format!("{p}: {v}")).unwrap_or_else(|| panic!("{p} 应可解析"));
            assert!(!d.is_unknown());
            // 往返:to_css 再解析,声明等值(L1 幂等的最小单元)
            let d2 = Decl::parse(&d.to_css()).expect("to_css 应可再解析");
            assert_eq!(d, d2, "{p} 往返不等值");
        }
    }

    /// DOC-16(验收):白名单外属性按**真字典序**输出,且与输入序无关
    /// (旧实现按字节和散列,碰撞时次序随输入漂移)。
    #[test]
    fn unknown_props_sort_true_lexicographic_order_independent() {
        let mk = |src: &str| {
            let mut v = parse_decls(src);
            sort_decls(&mut v);
            v.iter().map(|d| d.prop.clone()).collect::<Vec<_>>()
        };
        // z-序 + a-序 两种输入序必须产出同一输出序(字典序)
        let forward = mk("z-index-unknown: 1; alpha-custom: 2; midx: 3; color: red");
        let backward = mk("color: red; midx: 3; alpha-custom: 2; z-index-unknown: 1");
        let expect_tail = vec![
            "color".to_string(),
            "alpha-custom".to_string(),
            "midx".to_string(),
            "z-index-unknown".to_string(),
        ];
        assert_eq!(forward, expect_tail, "白名单外按字典序:{forward:?}");
        assert_eq!(backward, forward, "次序必须与输入序无关");
        // 幂等
        assert_eq!(
            mk(&forward
                .iter()
                .map(|p| format!("{p}: 1"))
                .collect::<Vec<_>>()
                .join("; ")),
            forward
        );
    }

    /// 白名单内不得出现重复项:重复会让 sort_decls 的稳定序对同一文档
    /// 在不同编译路径下不可预测(此前 border-style/outline 曾重复)。
    #[test]
    fn l1_props_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for p in L1_PROPS {
            assert!(seen.insert(*p), "L1_PROPS 重复项:{p}");
        }
    }
}

#[cfg(test)]
mod a3_tests {
    use super::*;

    /// R1:`!important` 出现在字符串内不得截断值(此前 rfind 子串匹配)。
    #[test]
    fn important_inside_string_is_not_stripped() {
        let d = Decl::parse(r#"content: "x !important""#).unwrap();
        assert_eq!(d.value, r#""x !important""#);
        assert!(!d.important);

        // 字符串外的末尾 !important 正常剥离
        let d = Decl::parse("color: red !important").unwrap();
        assert_eq!(d.value, "red");
        assert!(d.important);

        // url 里的子串也不误伤
        let d = Decl::parse(r#"background-image: url("a!important.png")"#).unwrap();
        assert_eq!(d.value, r#"url("a!important.png")"#);
        assert!(!d.important);
    }

    /// R2:注释内的分号不得吞掉后续声明;未闭合注释不得泄漏到后续声明。
    #[test]
    fn comment_aware_decl_splitting() {
        let decls = parse_decls("width:10px; /* a;b */ color:red");
        let props: Vec<&str> = decls.iter().map(|d| d.prop.as_str()).collect();
        assert_eq!(props, vec!["width", "color"], "注释内分号后声明丢失");

        let decls = parse_decls("color:red /* note;here */; background:blue");
        let props: Vec<&str> = decls.iter().map(|d| d.prop.as_str()).collect();
        assert_eq!(props, vec!["color", "background"]);
        let color = &decls[0].value;
        assert!(color.contains("note"), "值内注释应保留:{color}");

        // 括号/引号感知不回退
        let parts = split_top_level(
            "background-image:url(data:image/png;base64,xx), red; color:#fff",
            ';',
        );
        assert_eq!(parts.len(), 2);
    }
}
