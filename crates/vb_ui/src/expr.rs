//! 数值框数学表达式解析器(02-6-1;design/03 §四)。
//!
//! 纯函数、零 egui 依赖 —— 解析逻辑与 UI 彻底分离,可完整单测。
//!
//! ## 语法
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := unary (('*' | '/') unary)*
//! unary   := ('-' | '+')* primary
//! primary := number '%'? | '(' expr ')' '%'?
//! ```
//!
//! ## 百分号语义(02-6-1:相对什么由调用方给基准)
//!
//! - 给了基准 `base`:`50%` = `base × 0.5`(面板里基准 = 画板宽/高);
//! - 未给基准:`50%` = `0.5`(纯数值语义)。
//!
//! `%` 可缀在括号后:`(30+20)%` = `50%`。
//!
//! ## 容错(中文输入法友好)
//!
//! `×`(U+00D7)/`·`? 不收 —— 只收 `×``÷` 与全角括号 `（）`,
//! 其余字符报「无法识别的字符」并**指名是哪个字**。

use std::fmt;

/// 表达式求值错误(文案即面向用户的中文提示,NumField 直接 toast)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprError {
    /// 输入为空(只有空白)。
    Empty,
    /// 无法识别的字符。
    InvalidChar(char),
    /// 表达式不完整(如 `1+`、`(2`)。
    UnexpectedEnd,
    /// 此处不该出现的记号(如 `*3`、`2 3`)。
    UnexpectedToken(char),
    /// 括号不匹配(如 `2)`、`(2+3`)。
    UnmatchedParen,
    /// 除数为 0。
    DivideByZero,
    /// 结果不是有限数(溢出)。
    NotFinite,
}

impl fmt::Display for ExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "表达式为空"),
            Self::InvalidChar(c) => write!(f, "无法识别的字符「{c}」"),
            Self::UnexpectedEnd => write!(f, "表达式不完整(缺数字或右括号)"),
            Self::UnexpectedToken(c) => write!(f, "「{c}」处无法解析"),
            Self::UnmatchedParen => write!(f, "括号不匹配"),
            Self::DivideByZero => write!(f, "除数为 0"),
            Self::NotFinite => write!(f, "结果超出可表示范围"),
        }
    }
}

impl std::error::Error for ExprError {}

/// 求值数学表达式。
///
/// `percent_base`:百分号的相对基准(见模块注释);`None` 时 `N%` = `N/100`。
pub fn eval_expr(input: &str, percent_base: Option<f64>) -> Result<f64, ExprError> {
    let tokens = tokenize(input)?;
    if tokens.is_empty() {
        return Err(ExprError::Empty);
    }
    let mut p = Parser {
        tokens: &tokens,
        pos: 0,
        percent_base,
    };
    let v = p.expr()?;
    // 整串消费完才算合法("2 3"、"2)"都会在这里露出尾巴)
    if p.pos != p.tokens.len() {
        return match p.tokens[p.pos] {
            Tok::Close => Err(ExprError::UnmatchedParen),
            ref t => Err(ExprError::UnexpectedToken(t.head_char())),
        };
    }
    if v.is_finite() {
        Ok(v)
    } else {
        Err(ExprError::NotFinite)
    }
}

/// 记号:数字 / 运算符 / 百分号 / 括号。
#[derive(Debug, Clone, Copy, PartialEq)]
enum Tok {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Open,
    Close,
}

impl Tok {
    /// 错误提示用「头字符」。
    fn head_char(&self) -> char {
        match self {
            Tok::Num(_) => '0',
            Tok::Plus => '+',
            Tok::Minus => '-',
            Tok::Star => '*',
            Tok::Slash => '/',
            Tok::Percent => '%',
            Tok::Open => '(',
            Tok::Close => ')',
        }
    }
}

/// 词法:跳过空白;`×`→`*`、`÷`→`/`、全角括号→半角;其余原样。
fn tokenize(input: &str) -> Result<Vec<Tok>, ExprError> {
    let mut out = Vec::new();
    let mut chars = input.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\u{3000}' => {
                chars.next();
            }
            '+' => {
                out.push(Tok::Plus);
                chars.next();
            }
            '-' => {
                out.push(Tok::Minus);
                chars.next();
            }
            '*' | '×' => {
                out.push(Tok::Star);
                chars.next();
            }
            '/' | '÷' => {
                out.push(Tok::Slash);
                chars.next();
            }
            '%' => {
                out.push(Tok::Percent);
                chars.next();
            }
            '(' | '（' => {
                out.push(Tok::Open);
                chars.next();
            }
            ')' | '）' => {
                out.push(Tok::Close);
                chars.next();
            }
            '0'..='9' | '.' => {
                // 连读一个数字(含小数点;多个小数点交给 parse 失败)
                let mut s = String::new();
                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' {
                        s.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let v: f64 = s.parse().map_err(|_| ExprError::InvalidChar('.'))?;
                out.push(Tok::Num(v));
            }
            other => return Err(ExprError::InvalidChar(other)),
        }
    }
    Ok(out)
}

struct Parser<'a> {
    tokens: &'a [Tok],
    pos: usize,
    percent_base: Option<f64>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }

    fn take(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).copied();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// `N%` 的落值:有基准 = 基准 × N/100;无基准 = N/100。
    fn apply_percent(&self, v: f64) -> f64 {
        let frac = v / 100.0;
        match self.percent_base {
            Some(base) => base * frac,
            None => frac,
        }
    }

    fn expr(&mut self) -> Result<f64, ExprError> {
        let mut v = self.term()?;
        while let Some(Tok::Plus | Tok::Minus) = self.peek() {
            let op = self.take();
            let rhs = self.term()?;
            v = if matches!(op, Some(Tok::Plus)) {
                v + rhs
            } else {
                v - rhs
            };
        }
        Ok(v)
    }

    fn term(&mut self) -> Result<f64, ExprError> {
        let mut v = self.unary()?;
        while let Some(Tok::Star | Tok::Slash) = self.peek() {
            let op = self.take();
            let rhs = self.unary()?;
            if matches!(op, Some(Tok::Slash)) {
                if rhs == 0.0 {
                    return Err(ExprError::DivideByZero);
                }
                v /= rhs;
            } else {
                v *= rhs;
            }
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<f64, ExprError> {
        let mut sign = 1.0;
        while let Some(Tok::Plus | Tok::Minus) = self.peek() {
            if matches!(self.take(), Some(Tok::Minus)) {
                sign = -sign;
            }
        }
        Ok(sign * self.primary()?)
    }

    fn primary(&mut self) -> Result<f64, ExprError> {
        match self.take() {
            None => Err(ExprError::UnexpectedEnd),
            Some(Tok::Num(v)) => Ok(self.maybe_percent(v)),
            Some(Tok::Open) => {
                let v = self.expr()?;
                match self.take() {
                    Some(Tok::Close) => Ok(self.maybe_percent(v)),
                    _ => Err(ExprError::UnmatchedParen),
                }
            }
            Some(Tok::Close) => Err(ExprError::UnmatchedParen),
            Some(t) => Err(ExprError::UnexpectedToken(t.head_char())),
        }
    }

    /// `primary` 后允许紧跟一个 `%`(`50%` / `(30+20)%`)。
    fn maybe_percent(&mut self, v: f64) -> f64 {
        if matches!(self.peek(), Some(Tok::Percent)) {
            self.pos += 1;
            self.apply_percent(v)
        } else {
            v
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(s: &str, base: Option<f64>) -> Result<f64, ExprError> {
        eval_expr(s, base)
    }

    // ── 算术(顺序/结合/括号/一元) ──

    #[test]
    fn arithmetic_basics() {
        assert_eq!(eval("320/2", None).unwrap(), 160.0);
        assert_eq!(eval("12*3+4", None).unwrap(), 40.0);
        assert_eq!(eval("2+3*4", None).unwrap(), 14.0, "先乘除后加减");
        assert_eq!(eval("(2+3)*4", None).unwrap(), 20.0);
        assert_eq!(eval("100-30-20", None).unwrap(), 50.0, "左结合");
        assert_eq!(eval("2*(3+4)*(5-1)", None).unwrap(), 56.0);
        assert_eq!(eval("-5+3", None).unwrap(), -2.0, "一元负号");
        assert_eq!(eval("--5", None).unwrap(), 5.0, "双重负号");
        assert_eq!(eval("+5-+2", None).unwrap(), 3.0, "一元正号");
        assert_eq!(eval("  12  *  3 ", None).unwrap(), 36.0, "容忍空白");
    }

    #[test]
    fn fullwidth_and_typo_operators() {
        assert_eq!(eval("320÷2", None).unwrap(), 160.0, "÷ 按除号");
        assert_eq!(eval("2×3", None).unwrap(), 6.0, "× 按乘号");
        assert_eq!(eval("（1+2）", None).unwrap(), 3.0, "全角括号");
    }

    // ── 百分号 ──

    #[test]
    fn percent_with_base() {
        // 面板里基准 = 画板宽/高
        assert_eq!(eval("50%", Some(1440.0)).unwrap(), 720.0);
        assert_eq!(eval("25%", Some(900.0)).unwrap(), 225.0);
        assert_eq!(eval("(30+20)%", Some(1440.0)).unwrap(), 720.0, "括号缀 %");
        assert_eq!(eval("50%+10", Some(1440.0)).unwrap(), 730.0);
        // % 在出现处立即换算成基准值:`10*50%`(base=100)= 10×50 = 500
        assert_eq!(eval("10*50%", Some(100.0)).unwrap(), 500.0);
    }

    #[test]
    fn percent_without_base_is_fraction() {
        assert_eq!(eval("50%", None).unwrap(), 0.5);
        assert_eq!(eval("100%", None).unwrap(), 1.0);
    }

    // ── 非法输入(每类错误都有用例) ──

    #[test]
    fn errors_empty_and_garbage() {
        assert_eq!(eval("", None), Err(ExprError::Empty));
        assert_eq!(eval("   ", None), Err(ExprError::Empty));
        assert_eq!(
            eval("12+abc", None),
            Err(ExprError::InvalidChar('a')),
            "字母报具体字符"
        );
        assert_eq!(eval("3..5", None), Err(ExprError::InvalidChar('.')));
        assert_eq!(eval("*3", None), Err(ExprError::UnexpectedToken('*')));
        assert_eq!(eval("2 3", None), Err(ExprError::UnexpectedToken('0')));
    }

    #[test]
    fn errors_incomplete_and_parens() {
        assert_eq!(eval("1+", None), Err(ExprError::UnexpectedEnd));
        assert_eq!(eval("(2+3", None), Err(ExprError::UnmatchedParen));
        assert_eq!(eval("2+3)", None), Err(ExprError::UnmatchedParen));
        assert_eq!(eval(")", None), Err(ExprError::UnmatchedParen));
    }

    #[test]
    fn errors_div_zero_and_overflow() {
        assert_eq!(eval("5/0", None), Err(ExprError::DivideByZero));
        assert_eq!(eval("5/(2-2)", None), Err(ExprError::DivideByZero));
        assert_eq!(
            eval("1e300*1e300", None),
            Err(ExprError::InvalidChar('e')),
            "科学计数法未收录,报具体字符而不是静默错值"
        );
    }

    // ── 中文错误文案(直接进 toast,必须非空且指名问题) ──

    #[test]
    fn error_messages_are_user_facing_chinese() {
        let cases = [
            ExprError::Empty,
            ExprError::InvalidChar('x'),
            ExprError::UnexpectedEnd,
            ExprError::UnexpectedToken('*'),
            ExprError::UnmatchedParen,
            ExprError::DivideByZero,
            ExprError::NotFinite,
        ];
        for e in cases {
            let s = e.to_string();
            assert!(!s.is_empty(), "{e:?} 文案为空");
            assert!(!s.is_ascii(), "{e:?} 文案应是中文:{s}");
        }
        assert_eq!(ExprError::DivideByZero.to_string(), "除数为 0");
    }

    #[test]
    fn finite_check() {
        // 超大字面量:parse 成 inf,必须在 eval 出口被拦成 NotFinite
        let big = "9".repeat(400);
        assert_eq!(eval(&big, None), Err(ExprError::NotFinite));
        assert_eq!(
            eval(&format!("{big}*{big}"), None),
            Err(ExprError::NotFinite)
        );
    }
}
