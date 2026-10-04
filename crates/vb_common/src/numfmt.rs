//! 数值格式化单源(EXP-07):PDF(`vb_kiln::pdf::fnum`)与 SVG
//! (`vb_export`)两车道共用的坐标精度函数。
//!
//! f64 最短往返 Display 会把浮点噪声直接写进产物
//! (`123.45000000000002`),既膨胀文件也造成车道间同点坐标字面不一致;
//! 此处统一 3 位小数舍入 + 整数省尾 `.0`(与 PDF 侧历史口径逐字一致)。

/// 可精度的数值:f64 原样,f32 升宽(避免 `as` 散落)。
pub trait FnumVal {
    fn val(self) -> f64;
}
impl FnumVal for f64 {
    fn val(self) -> f64 {
        self
    }
}
impl FnumVal for f32 {
    fn val(self) -> f64 {
        self as f64
    }
}

/// 3 位小数舍入;整数省尾 `.0`。历史口径出自 PDF 写入器,SVG 车道自此
/// 同源(EXP-07:两车道统一精度函数,单一真相)。
pub fn fnum<V: FnumVal>(v: V) -> String {
    let r = (v.val() * 1000.0).round() / 1000.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_trims_trailing_zero() {
        assert_eq!(fnum(100.0), "100");
        assert_eq!(fnum(0.0), "0");
        assert_eq!(fnum(-42.0), "-42");
    }

    #[test]
    fn three_decimals_rounded() {
        assert_eq!(fnum(123.456789), "123.457");
        assert_eq!(fnum(0.1 + 0.2), "0.3");
    }

    /// EXP-07 反例:f64 最短往返噪声不得进入产物。
    #[test]
    fn float_noise_suppressed() {
        let noisy = 123.45_f64 * 1000.001 / 1000.001; // 制造 >15 位有效数字场景
        assert_eq!(fnum(noisy), fnum(123.45));
        assert!(!fnum(0.1 + 0.2).contains("0000000"), "{}", fnum(0.1 + 0.2));
    }

    #[test]
    fn f32_accepted() {
        assert_eq!(fnum(1.5f32), "1.5");
        assert_eq!(fnum(2f32), "2");
    }
}
