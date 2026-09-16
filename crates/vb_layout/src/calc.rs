//! calc() 求值:令牌化 + 递归下降(+ − × ÷ 与括号)。
//! 长度项递归解析(px/pt);百分比需要包含块语境,量测期整条放弃。

use super::BuildCtx;

#[derive(Debug, Clone, PartialEq)]
enum CalcTok {
    Num(f64),
    Add,
    Sub,
    Mul,
    Div,
    LParen,
    RParen,
    /// 长度项(已换算 px)
    Len(f64),
    /// 百分比(需要包含块尺寸;量测期视作失败)
    Pct(f64),
}

pub(super) fn eval(
    expr: &str,
    vars: &[(String, String)],
    font_size: f64,
    ctx: &BuildCtx,
) -> Option<f64> {
    let toks = tokenize(expr)?;
    let mut pos = 0usize;
    let v = expr_inner(&toks, &mut pos, vars, font_size, ctx)?;
    if pos != toks.len() {
        return None;
    }
    Some(v)
}

fn tokenize(expr: &str) -> Option<Vec<CalcTok>> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' => i += 1,
            '+' => {
                out.push(CalcTok::Add);
                i += 1;
            }
            '-' => {
                // 语境判负号:运算符/左括号后或表达式开头 = 一元负号
                let num_ctx = matches!(
                    out.last(),
                    None | Some(CalcTok::Add)
                        | Some(CalcTok::Sub)
                        | Some(CalcTok::Mul)
                        | Some(CalcTok::Div)
                        | Some(CalcTok::LParen)
                );
                if num_ctx {
                    let (tok, ni) = scan_num(&chars, i)?;
                    out.push(tok);
                    i = ni;
                } else {
                    out.push(CalcTok::Sub);
                    i += 1;
                }
            }
            '*' => {
                out.push(CalcTok::Mul);
                i += 1;
            }
            '/' => {
                out.push(CalcTok::Div);
                i += 1;
            }
            '(' => {
                out.push(CalcTok::LParen);
                i += 1;
            }
            ')' => {
                out.push(CalcTok::RParen);
                i += 1;
            }
            _ => {
                let (tok, ni) = scan_num(&chars, i)?;
                out.push(tok);
                i = ni;
            }
        }
    }
    Some(out)
}

fn scan_num(chars: &[char], start: usize) -> Option<(CalcTok, usize)> {
    let mut i = start;
    let mut s = String::new();
    if matches!(chars.get(i), Some('-') | Some('+')) {
        s.push(*chars.get(i)?);
        i += 1;
    }
    while i < chars.len() && (chars[i].is_ascii_digit() || (chars[i] == '.' && !s.contains('.'))) {
        s.push(chars[i]);
        i += 1;
    }
    let n: f64 = s.parse().ok()?;
    let mut u = String::new();
    while i < chars.len() && (chars[i].is_ascii_alphabetic() || chars[i] == '%') {
        u.push(chars[i]);
        i += 1;
    }
    let tok = match u.as_str() {
        "" => CalcTok::Num(n),
        "px" => CalcTok::Len(n),
        "pt" => CalcTok::Len(n * 4.0 / 3.0),
        "%" => CalcTok::Pct(n),
        // em/rem 需要字号语境;此处无 → 整条放弃
        _ => return None,
    };
    Some((tok, i))
}

fn expr_inner(
    toks: &[CalcTok],
    pos: &mut usize,
    vars: &[(String, String)],
    font_size: f64,
    ctx: &BuildCtx,
) -> Option<f64> {
    fn term(
        toks: &[CalcTok],
        pos: &mut usize,
        vars: &[(String, String)],
        font_size: f64,
        ctx: &BuildCtx,
    ) -> Option<f64> {
        let mut v = factor(toks, pos, vars, font_size, ctx)?;
        while *pos < toks.len() {
            match toks[*pos] {
                CalcTok::Mul => {
                    *pos += 1;
                    v *= factor(toks, pos, vars, font_size, ctx)?;
                }
                CalcTok::Div => {
                    *pos += 1;
                    let d = factor(toks, pos, vars, font_size, ctx)?;
                    if d == 0.0 {
                        return None;
                    }
                    v /= d;
                }
                _ => break,
            }
        }
        Some(v)
    }
    fn factor(
        toks: &[CalcTok],
        pos: &mut usize,
        _vars: &[(String, String)],
        _font_size: f64,
        _ctx: &BuildCtx,
    ) -> Option<f64> {
        let t = toks.get(*pos)?;
        *pos += 1;
        match t {
            CalcTok::Num(n) | CalcTok::Len(n) => Some(*n),
            CalcTok::Pct(_) => None,
            CalcTok::LParen => {
                let v = expr_inner(toks, pos, _vars, _font_size, _ctx)?;
                match toks.get(*pos) {
                    Some(CalcTok::RParen) => {
                        *pos += 1;
                        Some(v)
                    }
                    _ => None,
                }
            }
            CalcTok::Sub => {
                let v = factor(toks, pos, _vars, _font_size, _ctx)?;
                Some(-v)
            }
            _ => None,
        }
    }
    let mut v = term(toks, pos, vars, font_size, ctx)?;
    while *pos < toks.len() {
        match toks[*pos] {
            CalcTok::Add => {
                *pos += 1;
                v += term(toks, pos, vars, font_size, ctx)?;
            }
            CalcTok::Sub => {
                *pos += 1;
                v -= term(toks, pos, vars, font_size, ctx)?;
            }
            _ => break,
        }
    }
    Some(v)
}
