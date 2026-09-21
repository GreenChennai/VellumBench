//! `transform` 的**平移分量解析**(唯一实现)。
//!
//! 为什么放在 `vb_common`:同一件事有两个消费方向,必须在同一处定口径 ——
//! - **导入/布局**(`vb_layout`):画布几何 = 声明位置 **+** 自身 translate
//!   (`translate(-50%,-50%)` 是视觉居中,画布若只按 `left/top` 摆位会差半个身位);
//! - **导出**(`vb_doc`):`transform` 声明被原样保留,因此写回显式几何时必须
//!   **减去** 自身 translate,浏览器的"位置 = left + translate"才会等于模型里的
//!   视觉盒。两边符号必须相反、且必须来自同一份解析,否则就是
//!   "编辑一次跳一次、每存一轮漂一截"(2026-09-21 总验收实测:L1 被打穿)。
//!
//! 边界(如实记录):只解析 **translate / translateX / translateY**(`px` 与 `%`);
//! `rotate` / `skew` / `scale` 不在此列 —— 它们的形状无法用轴对齐矩形表达,
//! 原样保留在 CSS 里。

/// 取 CSS 函数 `name` 的实参列表(`name` 需带左括号,如 `translate(`)。
/// 函数名大小写不敏感(`vb_css::canonical_value` 会输出小写)。
pub fn fn_args(tf: &str, name: &str) -> Option<Vec<String>> {
    let tf = tf.to_ascii_lowercase();
    let i = tf.find(name)?;
    let rest = &tf[i + name.len()..];
    let end = rest.find(')')?;
    let args: Vec<String> = rest[..end]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (!args.is_empty()).then_some(args)
}

/// 解析长度:`12px` / `-50%`(百分比按 `base` 折算)→ 数值;其它单位 → `None`。
pub fn len_or_pct(v: &str, base: f64) -> Option<f64> {
    let t = v.trim();
    if let Some(p) = t.strip_suffix('%') {
        return p.trim().parse::<f64>().ok().map(|x| x / 100.0 * base);
    }
    crate::units::parse_px(t)
}

/// `transform` 里的平移分量 `(dx, dy)`(世界单位;`w`/`h` 为节点自身尺寸,
/// 供 `%` 折算)。
pub fn parse_translate(tf: &str, w: f64, h: f64) -> (f64, f64) {
    let (mut dx, mut dy) = (0.0, 0.0);
    if let Some(a) = fn_args(tf, "translate(") {
        if let Some(x) = a.first().and_then(|v| len_or_pct(v, w)) {
            dx += x;
        }
        if let Some(y) = a.get(1).and_then(|v| len_or_pct(v, h)) {
            dy += y;
        }
    }
    if let Some(a) = fn_args(tf, "translatex(") {
        if let Some(x) = a.first().and_then(|v| len_or_pct(v, w)) {
            dx += x;
        }
    }
    if let Some(a) = fn_args(tf, "translatey(") {
        if let Some(y) = a.first().and_then(|v| len_or_pct(v, h)) {
            dy += y;
        }
    }
    (dx, dy)
}

/// 从声明列表里取平移分量(无 `transform` → 零)。
///
/// 声明列表的类型由调用方决定(`vb_common` 不依赖 `vb_css`),故只收字符串。
pub fn own_translate(tf: Option<&str>, w: f64, h: f64) -> (f64, f64) {
    match tf {
        Some(v) => parse_translate(v, w, h),
        None => (0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_px_and_percent() {
        assert_eq!(
            parse_translate("translate(-410px, -40px)", 12.0, 12.0),
            (-410.0, -40.0)
        );
        // % 按自身尺寸折算(自身 100×50 → -50%,-50% = -50,-25)
        assert_eq!(
            parse_translate("translate(-50%, -50%)", 100.0, 50.0),
            (-50.0, -25.0)
        );
        assert_eq!(parse_translate("translateX(3px)", 1.0, 1.0), (3.0, 0.0));
        assert_eq!(parse_translate("translateY(7px)", 1.0, 1.0), (0.0, 7.0));
        // 函数名大小写不敏感(canonical 会输出小写)
        assert_eq!(parse_translate("TRANSLATEX(3px)", 1.0, 1.0), (3.0, 0.0));
        // 非平移函数不折
        assert_eq!(parse_translate("rotate(30deg)", 10.0, 10.0), (0.0, 0.0));
        assert_eq!(parse_translate("skew(10deg, 5deg)", 10.0, 10.0), (0.0, 0.0));
        // 复合:平移与旋转共存时只取平移
        assert_eq!(
            parse_translate("translate(2px, 3px) rotate(10deg)", 10.0, 10.0),
            (2.0, 3.0)
        );
    }

    #[test]
    fn unknown_units_are_not_guessed() {
        assert_eq!(
            parse_translate("translate(1em, 2rem)", 10.0, 10.0),
            (0.0, 0.0)
        );
        assert_eq!(
            parse_translate("translate(calc(1px + 2px))", 10.0, 10.0),
            (0.0, 0.0)
        );
        assert_eq!(parse_translate("", 10.0, 10.0), (0.0, 0.0));
    }

    #[test]
    fn own_translate_reads_decl() {
        assert_eq!(
            own_translate(Some("translate(1px, 2px)"), 10.0, 10.0),
            (1.0, 2.0)
        );
        assert_eq!(own_translate(None, 10.0, 10.0), (0.0, 0.0));
    }
}
