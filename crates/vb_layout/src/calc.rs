//! calc() 求值:令牌化 + 递归下降(+ − × ÷ 与括号)。
//! 长度项递归解析(px/pt);百分比需要包含块语境,量测期整条放弃。
//!
//! DOC-01/RB-02:括号深度在**令牌化期**计数(迭代,无栈风险),超
/// [`vb_common::MAX_TREE_DEPTH`] 整条放弃求值(返回 None = 该声明
/// 不参与布局,与「百分比无语境整条放弃」同一降级语义);递归下降
/// 内部再带深度参数兜底 —— 双保险,异常深输入到不了深递归。
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
    let v = expr_inner(&toks, &mut pos, vars, font_size, ctx, 0)?;
    if pos != toks.len() {
        return None;
    }
    Some(v)
}

fn tokenize(expr: &str) -> Option<Vec<CalcTok>> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    // DOC-01:括号深度计数(超限整条放弃,不让递归下降见到深输入)
    let mut paren_depth = 0usize;
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
                paren_depth += 1;
                if paren_depth > vb_common::MAX_TREE_DEPTH {
                    return None;
                }
                out.push(CalcTok::LParen);
                i += 1;
            }
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
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
    depth: usize,
) -> Option<f64> {
    fn term(
        toks: &[CalcTok],
        pos: &mut usize,
        vars: &[(String, String)],
        font_size: f64,
        ctx: &BuildCtx,
        depth: usize,
    ) -> Option<f64> {
        let mut v = factor(toks, pos, vars, font_size, ctx, depth)?;
        while *pos < toks.len() {
            match toks[*pos] {
                CalcTok::Mul => {
                    *pos += 1;
                    v *= factor(toks, pos, vars, font_size, ctx, depth)?;
                }
                CalcTok::Div => {
                    *pos += 1;
                    let d = factor(toks, pos, vars, font_size, ctx, depth)?;
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
        depth: usize,
    ) -> Option<f64> {
        let t = toks.get(*pos)?;
        *pos += 1;
        match t {
            CalcTok::Num(n) | CalcTok::Len(n) => Some(*n),
            CalcTok::Pct(_) => None,
            CalcTok::LParen => {
                // 递归兜底上限(令牌化已挡超限输入;此处防御未来新入口)
                if depth >= vb_common::MAX_TREE_DEPTH {
                    return None;
                }
                let v = expr_inner(toks, pos, _vars, _font_size, _ctx, depth + 1)?;
                match toks.get(*pos) {
                    Some(CalcTok::RParen) => {
                        *pos += 1;
                        Some(v)
                    }
                    _ => None,
                }
            }
            CalcTok::Sub => {
                let v = factor(toks, pos, _vars, _font_size, _ctx, depth)?;
                Some(-v)
            }
            _ => None,
        }
    }
    let mut v = term(toks, pos, vars, font_size, ctx, depth)?;
    while *pos < toks.len() {
        match toks[*pos] {
            CalcTok::Add => {
                *pos += 1;
                v += term(toks, pos, vars, font_size, ctx, depth)?;
            }
            CalcTok::Sub => {
                *pos += 1;
                v -= term(toks, pos, vars, font_size, ctx, depth)?;
            }
            _ => break,
        }
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DOC-01/RB-02(验收):10 万层嵌套括号的 calc 必须**整条放弃**
    /// (返回 None),不得栈溢出崩溃。
    #[test]
    fn deep_parens_give_up_instead_of_overflow() {
        let n = 100_000usize;
        let mut expr = String::with_capacity(n * 4);
        for _ in 0..n {
            expr.push_str("(1");
        }
        expr.push_str("+0");
        for _ in 0..n {
            expr.push(')');
        }
        assert_eq!(tokenize(&expr), None, "超限深括号必须在令牌化期放弃");
    }

    /// 上限内的常规表达式照常令牌化(降级不影响合法输入)。
    #[test]
    fn normal_expressions_still_tokenize() {
        let toks = tokenize("(1px + 2px) * 3").expect("常规表达式必须可令牌化");
        assert!(toks.contains(&CalcTok::LParen));
        assert_eq!(toks.last(), Some(&CalcTok::Num(3.0)));
        // 恰好上限深度仍可令牌化(降级只在超限时发生)
        let ok = format!(
            "{}1{}",
            "(".repeat(vb_common::MAX_TREE_DEPTH),
            ")".repeat(vb_common::MAX_TREE_DEPTH)
        );
        assert!(tokenize(&ok).is_some(), "上限深度必须仍可求值");
    }
}
