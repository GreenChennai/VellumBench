//! 单位与数值格式化。
//!
//! 约定(ADR-0007):内部存储与计算只用 px(CSS px);
//! 输出字符串最多 4 位小数并去尾随 0;`-0` 归一为 `0`。

/// 1pt = 4/3 px
pub const PX_PER_PT: f64 = 4.0 / 3.0;
/// 1 inch = 96 px
pub const PX_PER_INCH: f64 = 96.0;

/// pt → px
pub fn pt_to_px(pt: f64) -> f64 {
    pt * PX_PER_PT
}

/// px → pt
pub fn px_to_pt(px: f64) -> f64 {
    px / PX_PER_PT
}

/// f64 → 输出字符串:round 到 4 位小数,去尾随 0(`1.0` → `1`,`-0.0` → `0`)。
pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let r = (v * 10_000.0).round() / 10_000.0;
    if r == 0.0 {
        return "0".to_string();
    }
    // Rust 的 Display 对 f64 输出最短表示:1.0 → "1",12.05 → "12.05"
    format!("{r}")
}

/// 解析 CSS px 长度:`"12px"` / `"12.5"` / `"0"` / `"-3PX"` → f64。
/// 拒绝百分比与其他单位(导入器按"矢量模式"只接受 px)。
pub fn parse_px(s: &str) -> Option<f64> {
    let t = s.trim();
    let (num, unit) = split_number_unit(t)?;
    match unit.as_str() {
        "" | "px" => num.parse::<f64>().ok(),
        "pt" => num.parse::<f64>().ok().map(pt_to_px),
        _ => None,
    }
}

/// 拆分数字与单位(单位 lowercase)。
fn split_number_unit(t: &str) -> Option<(&str, String)> {
    let bytes = t.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
        i += 1;
    }
    let mut seen_dot = false;
    while i < bytes.len() {
        match bytes[i] {
            b'0'..=b'9' => i += 1,
            b'.' if !seen_dot => {
                seen_dot = true;
                i += 1;
            }
            _ => break,
        }
    }
    if i == 0 || (i == 1 && (bytes[0] == b'-' || bytes[0] == b'+')) {
        return None;
    }
    Some((&t[..i], t[i..].trim().to_ascii_lowercase()))
}

/// 角度:内部 AI 语义 —— 0° 为 3 点钟方向,**逆时针为正**(与 Illustrator 一致)。
///
/// CSS `rotate()` 顺时针为正,仅在序列化层经 [`AngleDeg::to_css`] 取负。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngleDeg(pub f64);

impl AngleDeg {
    /// 写入 CSS 时取负(CSS 顺时针为正)。
    pub fn to_css(self) -> f64 {
        -self.0
    }
    /// 从 CSS 角度换算回内部 AI 语义。
    pub fn from_css(css_deg: f64) -> Self {
        Self(-css_deg)
    }
    /// 归一化到 (-180, 180]。
    pub fn normalized(self) -> Self {
        let mut a = self.0 % 360.0;
        if a > 180.0 {
            a -= 360.0;
        }
        if a <= -180.0 {
            a += 360.0;
        }
        Self(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_num_trims_and_normalizes() {
        assert_eq!(fmt_num(1.0), "1");
        assert_eq!(fmt_num(12.05), "12.05");
        assert_eq!(fmt_num(-0.0), "0");
        assert_eq!(fmt_num(0.30000000000000004), "0.3");
        assert_eq!(fmt_num(1.00005), "1"); // 4 位内舍入
        assert_eq!(fmt_num(f64::NAN), "0");
    }

    #[test]
    fn parse_px_variants() {
        assert_eq!(parse_px("12px"), Some(12.0));
        assert_eq!(parse_px("12.5"), Some(12.5));
        assert_eq!(parse_px("0"), Some(0.0));
        assert_eq!(parse_px("-3PX"), Some(-3.0));
        assert_eq!(parse_px("2pt"), Some(8.0 / 3.0));
        assert_eq!(parse_px("50%"), None);
        assert_eq!(parse_px("auto"), None);
    }

    #[test]
    fn angle_css_sign_convention() {
        // AI 逆时针 +45° → CSS 顺时针 -45°,双向可逆
        let a = AngleDeg(45.0);
        assert_eq!(a.to_css(), -45.0);
        assert_eq!(AngleDeg::from_css(-45.0), a);
        assert_eq!(AngleDeg(-270.0).normalized().0, 90.0);
    }
}
