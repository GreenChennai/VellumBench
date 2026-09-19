//! CSS 动画时间轴(M2):`@keyframes` 解析 + `animation` 求值 + 逐帧属性插值。
//!
//! 设计:关键帧在**导出期**从 `raw_css` 逐字块解析(不动场景图模型、
//! 不影响 L1 幂等);节点上的 `animation` 声明(含 var()/calc() 时序)
//! 解析为实例,帧时刻 t 求值出属性集(transform 平移/缩放/旋转、opacity、
//! clip-path inset),由 context 逐帧应用到 DrawList 副本。
//!
//! 覆盖(Q10=B):L1 transform/opacity 全量;L2 clip-path 常用形;
//! L3 filter blur/brightness/saturate(CPU 栅格路径);
//! 缓动 cubic-bezier 系 + linear() + 关键字;fill-mode both/forwards/
//! backwards;finite/alternate;@property 注册自定义属性按静态终值降级。
//! 不做:JS/rAF 动画(WPI 语义)、steps()(artboard 教义禁用)。

use std::collections::HashMap;

use vb_css::{parse_decls, split_top_level, Decl};
use vb_doc::model::{Document, Node, NodeId};

use vb_common::units;

// ---------- 关键帧模型 ----------

/// 一条 @keyframes:名字 → 帧表(按 offset 升序)。
#[derive(Debug, Clone)]
pub struct Keyframes {
    pub name: String,
    /// (offset 0..=1, 属性声明)
    pub frames: Vec<(f32, Vec<Decl>)>,
}

/// 一条动画实例(节点上的单个 animation)。
#[derive(Debug, Clone)]
pub struct AnimInstance {
    pub name: String,
    pub duration: f64,
    pub delay: f64,
    /// 迭代次数;f64::INFINITY = infinite。
    pub iterations: f64,
    pub fill_backwards: bool,
    pub fill_forwards: bool,
    pub alternate: bool,
    pub timing: Timing,
}

/// 缓动。
#[derive(Debug, Clone, Copy)]
pub enum Timing {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier(f64, f64, f64, f64),
}

impl Timing {
    fn eval(self, p: f64) -> f64 {
        match self {
            Timing::Linear | Timing::Ease => {
                // ease 近似贝塞尔;v1 用预设曲线
                match self {
                    Timing::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, p),
                    _ => p,
                }
            }
            Timing::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, p),
            Timing::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, p),
            Timing::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, p),
            Timing::CubicBezier(x1, y1, x2, y2) => cubic_bezier(x1, y1, x2, y2, p),
        }
    }
}

/// cubic-bezier(x1,y1,x2,y2) 求值:牛顿迭代 x → t,y(t)。
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bez_x =
        |t: f64| 3.0 * t * (1.0 - t) * (1.0 - t) * x1 + 3.0 * t * t * (1.0 - t) * x2 + t * t * t;
    let bez_y =
        |t: f64| 3.0 * t * (1.0 - t) * (1.0 - t) * y1 + 3.0 * t * t * (1.0 - t) * y2 + t * t * t;
    let bez_x_d = |t: f64| {
        3.0 * (1.0 - t) * (1.0 - t) * x1
            + 6.0 * t * (1.0 - t) * (x2 - x1)
            + 3.0 * t * t * (1.0 - x2)
    };
    // 牛顿 8 次 + 二分兜底
    let mut t = x;
    for _ in 0..8 {
        let e = bez_x(t) - x;
        if e.abs() < 1e-6 {
            break;
        }
        let d = bez_x_d(t);
        if d.abs() < 1e-7 {
            break;
        }
        t -= e / d;
        t = t.clamp(0.0, 1.0);
    }
    let mut lo = 0.0f64;
    let mut hi = 1.0f64;
    if (bez_x(t) - x).abs() > 1e-4 {
        for _ in 0..40 {
            let mid = (lo + hi) / 2.0;
            if bez_x(mid) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        t = (lo + hi) / 2.0;
    }
    bez_y(t)
}

// ---------- @keyframes 解析(export 期,from raw_css) ----------

/// 从文档 raw_css 解析全部 @keyframes(名字小写)。
pub fn parse_keyframes(raw_css: &[String]) -> HashMap<String, Keyframes> {
    let mut out: HashMap<String, Keyframes> = HashMap::new();
    for block in raw_css {
        let trimmed = block.trim_start();
        let Some(rest) = trimmed.strip_prefix("@keyframes") else {
            continue;
        };
        let Some(brace) = rest.find('{') else {
            continue;
        };
        let name = rest[..brace].trim().to_ascii_lowercase();
        // 去掉尾 }
        let body = rest[brace + 1..].trim_end().trim_end_matches('}').trim();
        let mut frames: Vec<(f32, Vec<Decl>)> = Vec::new();
        // 帧级扫描:SELECTOR { decls }(括号/字符串感知)
        let chars: Vec<char> = body.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if i >= chars.len() {
                break;
            }
            let sel_start = i;
            while i < chars.len() && chars[i] != '{' {
                i += 1;
            }
            if i >= chars.len() {
                break;
            }
            let sel: String = chars[sel_start..i].iter().collect();
            i += 1; // {
            let body_start = i;
            let mut depth = 1usize;
            while i < chars.len() {
                match chars[i] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            let fbody: String = chars[body_start..i.min(chars.len())].iter().collect();
            i += 1; // }
            let decls = parse_decls(&fbody);
            for part in sel.split(',') {
                let off: Option<f32> = match part.trim() {
                    "from" => Some(0.0f32),
                    "to" => Some(1.0f32),
                    p => p
                        .trim()
                        .strip_suffix('%')
                        .and_then(|v| v.parse::<f32>().ok())
                        .map(|v| v / 100.0),
                };
                if let Some(off) = off {
                    frames.push((off.clamp(0.0, 1.0), decls.clone()));
                }
            }
        }
        frames.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if !frames.is_empty() {
            out.insert(name.clone(), Keyframes { name, frames });
        }
    }
    out
}

// ---------- animation 简写解析 ----------

/// 解析一条 animation 简写(单个动画;逗号分隔的多动画由调用方先拆)。
/// 时序值里的 var()/calc() 用 `vars` 解析。
pub fn parse_animation_shorthand(raw: &str, vars: &[(String, String)]) -> Option<AnimInstance> {
    // 复合值里的 var()/calc() 必须先展开求值:此前 resolve_vars 只认
    // 「整个值恰为 var(--x)」,`rise-in var(--in) calc(var(--t0)+...)` 这类
    // 写法全部拿不到时长 → 节点被当静态(5 张动画卡单帧即此因)。
    let resolved = eval_calcs(&substitute_vars(raw, vars));
    let tokens: Vec<String> = split_top_level(&resolved, ' ')
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut name = String::new();
    let mut times: Vec<f64> = Vec::new();
    let mut timing = Timing::Linear;
    let mut iterations = 1.0f64;
    let mut fill_backwards = false;
    let mut fill_forwards = false;
    let mut alternate = false;
    for tok in &tokens {
        let t = tok.as_str();
        if t == "infinite" {
            iterations = f64::INFINITY;
            continue;
        }
        if matches!(t, "both" | "forwards" | "backwards" | "none") {
            fill_backwards |= matches!(t, "both" | "backwards");
            fill_forwards |= matches!(t, "both" | "forwards");
            continue;
        }
        if matches!(t, "alternate" | "alternate-reverse") {
            alternate = true;
            continue;
        }
        if matches!(t, "normal" | "reverse" | "running" | "paused") {
            continue;
        }
        if matches!(
            t,
            "ease" | "ease-in" | "ease-out" | "ease-in-out" | "linear" | "step-start" | "step-end"
        ) {
            timing = match t {
                "ease" => Timing::Ease,
                "ease-in" => Timing::EaseIn,
                "ease-out" => Timing::EaseOut,
                "ease-in-out" => Timing::EaseInOut,
                _ => Timing::Linear,
            };
            continue;
        }
        if t.starts_with("cubic-bezier(") {
            let inner = t
                .strip_prefix("cubic-bezier(")
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or("");
            let nums: Vec<f64> = inner
                .split(',')
                .filter_map(|v| v.trim().parse::<f64>().ok())
                .collect();
            if nums.len() == 4 {
                timing = Timing::CubicBezier(nums[0], nums[1], nums[2], nums[3]);
            }
            continue;
        }
        if t.starts_with("linear(") {
            // linear() 弹簧:分段线性;v1 用首尾斜率近似为 linear
            timing = Timing::Linear;
            continue;
        }
        if t.ends_with('s') || t.ends_with('m') {
            if let Some(v) = parse_time(t) {
                times.push(v);
                continue;
            }
        }
        if let Ok(n) = t.parse::<f64>() {
            if t.ends_with("0") || !t.contains('.') || times.is_empty() {
                iterations = n;
                continue;
            }
        }
        if name.is_empty() {
            name = t.to_ascii_lowercase();
        }
    }
    if name.is_empty() {
        return None;
    }
    let duration = times.first().copied().unwrap_or(0.0);
    let delay = times.get(1).copied().unwrap_or(0.0);
    if duration <= 0.0 {
        return None;
    }
    Some(AnimInstance {
        name,
        duration,
        delay,
        iterations,
        fill_backwards,
        fill_forwards,
        alternate,
        timing,
    })
}

/// 全量 var() 替换:复合值内嵌的 var(如 `var(--in) var(--ease-in)
/// calc(var(--t0) + …)`)逐个展开,最多 8 轮防链式循环;
/// 查不到且无 fallback 的 var 原样保留(后续解析自然失败,与旧口径一致)。
fn substitute_vars(raw: &str, vars: &[(String, String)]) -> String {
    let mut cur = raw.trim().to_string();
    for _ in 0..8 {
        let Some(start) = cur.find("var(--") else {
            break;
        };
        let Some(open_rel) = cur[start..].find('(') else {
            break;
        };
        let open = start + open_rel;
        let Some(close) = match_paren(&cur, open) else {
            break;
        };
        let inner = cur[open + 1..close].to_string();
        let (name, fallback) = match inner.split_once(',') {
            Some((n, f)) => (n.trim().trim_start_matches("--"), Some(f.trim())),
            None => (inner.trim().trim_start_matches("--"), None),
        };
        let rep = vars
            .iter()
            .find(|(k, _)| k.as_str() == name)
            .map(|(_, v)| v.clone())
            .or_else(|| fallback.map(|f| f.to_string()));
        let Some(rep) = rep else { break };
        cur = format!("{}{}{}", &cur[..start], rep, &cur[close + 1..]);
    }
    cur
}

/// `s[..]` 中 open 指向 '(' 的匹配右括号下标。
fn match_paren(s: &str, open: usize) -> Option<usize> {
    let b = s.as_bytes();
    if b.get(open) != Some(&b'(') {
        return None;
    }
    let mut depth = 0i32;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// 值中所有 calc(...) 求值为时间/数值文本(先算最内层,逐层外推;
/// 含 % 等无法求值的表达式原样保留)。
fn eval_calcs(raw: &str) -> String {
    let mut cur = raw.to_string();
    loop {
        let mut replaced = false;
        let mut search = 0usize;
        while let Some(rel) = cur[search..].find("calc(") {
            let start = search + rel;
            let open = start + 4;
            let Some(close) = match_paren(&cur, open) else {
                break;
            };
            let inner = cur[open + 1..close].to_string();
            if inner.contains("calc(") {
                search = open + 1; // 先算嵌套内层
                continue;
            }
            match eval_time_expr(&inner) {
                Some(v) => {
                    cur = format!("{}{}{}", &cur[..start], v, &cur[close + 1..]);
                    replaced = true;
                    search = 0;
                }
                None => search = close + 1,
            }
        }
        if !replaced {
            break;
        }
    }
    cur
}

/// calc 内的标量。
#[derive(Clone, Copy, PartialEq)]
enum CalcVal {
    Num(f64),
    /// 秒
    Time(f64),
}

/// 求值简单时间算术:`0.1s + 0.60*1s` → `Some("0.7s")`。
/// 支持 + - * /(含一元负号)与 s/ms;时间只能与时间相加减、与数相乘除;
/// 其它单位(%)或非法结构返回 None。
fn eval_time_expr(expr: &str) -> Option<String> {
    #[derive(Clone)]
    enum Tok {
        Val(CalcVal),
        Op(char),
    }
    let b = expr.as_bytes();
    let mut toks: Vec<Tok> = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if matches!(c, '+' | '-' | '*' | '/') {
            toks.push(Tok::Op(c));
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' || c == '-' {
            // 一元负号:仅在开头或运算符之后
            let neg = c == '-';
            let unary_ok = toks.is_empty() || matches!(toks.last(), Some(Tok::Op(_)));
            if neg && !unary_ok {
                return None;
            }
            let s0 = if neg { i + 1 } else { i };
            let mut j = s0;
            while j < b.len() && ((b[j] as char).is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            let num: f64 = expr[s0..j].parse().ok()?;
            let u0 = j;
            while j < b.len() && (b[j] as char).is_ascii_alphabetic() {
                j += 1;
            }
            let unit = &expr[u0..j];
            let v = match unit {
                "" => CalcVal::Num(if neg { -num } else { num }),
                "s" => CalcVal::Time(if neg { -num } else { num }),
                "ms" => CalcVal::Time(if neg { -num / 1000.0 } else { num / 1000.0 }),
                _ => return None,
            };
            toks.push(Tok::Val(v));
            i = j;
            continue;
        }
        return None;
    }
    if toks.is_empty() {
        return None;
    }
    // 先 * / 后 + -
    let mut pass1: Vec<Tok> = Vec::new();
    let mut it = toks.into_iter();
    let mut acc = match it.next()? {
        Tok::Val(v) => v,
        Tok::Op(_) => return None,
    };
    while let Some(t) = it.next() {
        match t {
            Tok::Val(_) => return None,
            Tok::Op(op @ ('*' | '/')) => {
                let rhs = match it.next()? {
                    Tok::Val(v) => v,
                    Tok::Op(_) => return None,
                };
                acc = match (acc, op, rhs) {
                    (CalcVal::Num(a), '*', CalcVal::Num(b)) => CalcVal::Num(a * b),
                    (CalcVal::Num(a), '*', CalcVal::Time(b)) => CalcVal::Time(a * b),
                    (CalcVal::Time(a), '*', CalcVal::Num(b)) => CalcVal::Time(a * b),
                    (CalcVal::Time(a), '/', CalcVal::Num(b)) => CalcVal::Time(a / b),
                    (CalcVal::Num(a), '/', CalcVal::Num(b)) => CalcVal::Num(a / b),
                    _ => return None,
                };
            }
            Tok::Op(op) => {
                pass1.push(Tok::Val(acc));
                pass1.push(Tok::Op(op));
                acc = match it.next()? {
                    Tok::Val(v) => v,
                    Tok::Op(_) => return None,
                };
            }
        }
    }
    pass1.push(Tok::Val(acc));
    let mut result = match pass1[0] {
        Tok::Val(v) => v,
        Tok::Op(_) => return None,
    };
    let mut idx = 1;
    while idx < pass1.len() {
        let op = match pass1[idx] {
            Tok::Op(o) => o,
            Tok::Val(_) => return None,
        };
        let rhs = match pass1.get(idx + 1) {
            Some(Tok::Val(v)) => *v,
            _ => return None,
        };
        result = match (result, op, rhs) {
            (CalcVal::Time(a), '+', CalcVal::Time(b)) => CalcVal::Time(a + b),
            (CalcVal::Time(a), '-', CalcVal::Time(b)) => CalcVal::Time(a - b),
            (CalcVal::Num(a), '+', CalcVal::Num(b)) => CalcVal::Num(a + b),
            (CalcVal::Num(a), '-', CalcVal::Num(b)) => CalcVal::Num(a - b),
            _ => return None,
        };
        idx += 2;
    }
    Some(match result {
        CalcVal::Time(t) => format!("{}s", fmt_num(t)),
        CalcVal::Num(n) => fmt_num(n),
    })
}

/// 去尾零的数字文本(0.7000 → "0.7";整数带 .0 → "1")。
fn fmt_num(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// 时间字面量 → 秒。
fn parse_time(t: &str) -> Option<f64> {
    if let Some(v) = t.strip_suffix("ms") {
        return v.parse::<f64>().ok().map(|v| v / 1000.0);
    }
    if let Some(v) = t.strip_suffix('s') {
        return v.parse::<f64>().ok();
    }
    None
}

// ---------- 节点动画收集 ----------

/// 节点继承令牌(含自身定义 + :root tokens)。
fn node_vars(doc: &Document, id: NodeId) -> Vec<(String, String)> {
    // 沿祖先链收集(近者覆盖远者)
    let mut chain: Vec<&Node> = Vec::new();
    let mut cur = Some(id);
    while let Some(cid) = cur {
        if let Some(n) = doc.node(cid) {
            chain.push(n);
            cur = n.parent;
        } else {
            break;
        }
    }
    let mut vars: Vec<(String, String)> = doc.tokens.clone();
    for n in chain.iter().rev() {
        for d in &n.style {
            if let Some(name) = d.prop.strip_prefix("--") {
                vars.retain(|(k, _)| k != name);
                vars.push((name.to_string(), d.value.clone()));
            }
        }
    }
    vars
}

/// 一条属性轨道的关键帧值(按序)。
#[derive(Debug, Clone)]
pub struct Track {
    /// 属性名(小写:opacity / transform / clip-path / filter)。
    pub prop: String,
    /// (offset, 值原文)
    pub frames: Vec<(f32, String)>,
}

/// 节点的全部动画实例(已绑关键帧)。
pub struct NodeAnim {
    pub instances: Vec<(AnimInstance, Keyframes)>,
}

/// 解析节点的动画(支持逗号分隔多动画)。
pub fn resolve_node_anim(
    doc: &Document,
    id: NodeId,
    kf: &HashMap<String, Keyframes>,
) -> Option<NodeAnim> {
    let node = doc.node(id)?;
    let anim_raw = node.style_get("animation")?;
    if anim_raw.trim().is_empty() {
        return None;
    }
    let vars = node_vars(doc, id);
    let mut instances = Vec::new();
    for one in split_top_level(anim_raw, ',') {
        let Some(inst) = parse_animation_shorthand(&one, &vars) else {
            continue;
        };
        if let Some(frames) = kf.get(&inst.name) {
            instances.push((inst, frames.clone()));
        }
    }
    if instances.is_empty() {
        None
    } else {
        Some(NodeAnim { instances })
    }
}

// ---------- 时刻求值 ----------

/// 单个动画在时刻 t(秒)对某属性给出的值(已含 fill/delay/iteration);
/// 返回 None = 该时刻动画不生效(用元素静态样式)。
pub fn track_value_at(
    inst: &AnimInstance,
    frames: &Keyframes,
    prop: &str,
    t: f64,
) -> Option<String> {
    let local = t - inst.delay;
    if local < 0.0 {
        return if inst.fill_backwards {
            first_value(frames, prop)
        } else {
            None
        };
    }
    if inst.iterations == 0.0 {
        return None;
    }
    let raw_phase = local / inst.duration;
    let done = raw_phase >= inst.iterations;
    let cycle = raw_phase.floor();
    let mut phase = raw_phase - cycle;
    if inst.iterations.is_finite() && done {
        // 动画结束:forwards/both 保持终值,否则静态
        if inst.fill_forwards {
            phase = if inst.alternate && (inst.iterations as usize).is_multiple_of(2) {
                0.0
            } else {
                1.0
            };
        } else {
            return None;
        }
    }
    if inst.alternate {
        let cyc = cycle as usize;
        if cyc % 2 == 1 {
            phase = 1.0 - phase;
        }
    }
    // 找相邻关键帧
    let mut values: Vec<(f32, String)> = frames
        .frames
        .iter()
        .filter_map(|(off, decls)| {
            decls
                .iter()
                .find(|d| d.prop == prop)
                .map(|d| (*off, d.value.clone()))
        })
        .collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let p = phase as f32;
    // 边界:phase 小于首帧 → 首帧值;大于末帧 → 末帧值
    if p <= values[0].0 {
        return Some(values[0].1.clone());
    }
    if p >= values[values.len() - 1].0 {
        return Some(values[values.len() - 1].1.clone());
    }
    for w in values.windows(2) {
        let (o0, v0) = &w[0];
        let (o1, v1) = &w[1];
        if p >= *o0 && p <= *o1 {
            let span = (o1 - o0).max(1e-6);
            let local = (p - o0) / span;
            let e = inst.timing.eval(local as f64) as f32;
            return Some(lerp_value(v0, v1, e));
        }
    }
    Some(values[values.len() - 1].1.clone())
}

fn first_value(frames: &Keyframes, prop: &str) -> Option<String> {
    frames.frames.iter().find_map(|(_, decls)| {
        decls
            .iter()
            .find(|d| d.prop == prop)
            .map(|d| d.value.clone())
    })
}

/// 值插值:数值 / 变换函数列表 / 颜色(退化为不插值取端点)。
fn lerp_value(a: &str, b: &str, t: f32) -> String {
    if t >= 1.0 {
        return b.to_string();
    }
    if t <= 0.0 {
        return a.to_string();
    }
    // 纯数值
    if let (Ok(va), Ok(vb)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>()) {
        return format!("{}", va + (vb - va) * t as f64);
    }
    // 变换函数列表
    if a.contains('(') || b.contains('(') {
        return lerp_transform(a, b, t as f64);
    }
    // 其它(clip-path 百分比组等):逐数值段插值
    let lerp_nums = |s: &str| -> Option<Vec<f64>> {
        let mut out = Vec::new();
        let mut num = String::new();
        for c in s.chars() {
            if c.is_ascii_digit() || c == '.' || c == '-' {
                num.push(c);
            } else if !num.is_empty() {
                if let Ok(v) = num.parse::<f64>() {
                    out.push(v);
                }
                num.clear();
            }
        }
        if !num.is_empty() {
            if let Ok(v) = num.parse::<f64>() {
                out.push(v);
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    };
    if let (Some(na), Some(nb)) = (lerp_nums(a), lerp_nums(b)) {
        if na.len() == nb.len() {
            let mixed: Vec<f64> = na
                .iter()
                .zip(nb.iter())
                .map(|(&x, &y)| x + (y - x) * t as f64)
                .collect();
            // 保留非数字骨架:以 a 的骨架逐个替换
            let mut out = String::new();
            let mut idx = 0usize;
            let mut num = String::new();
            for c in a.chars() {
                if c.is_ascii_digit() || c == '.' || c == '-' {
                    num.push(c);
                } else {
                    if !num.is_empty() {
                        out.push_str(&format!("{}", mixed[idx].min(1e9)));
                        idx += 1;
                        num.clear();
                    }
                    out.push(c);
                }
            }
            if !num.is_empty() && idx < mixed.len() {
                out.push_str(&format!("{}", mixed[idx]));
            }
            return out;
        }
    }
    a.to_string()
}

/// transform 函数列表插值:translateX/Y(px)、scale(sx[,sy])、rotate(deg)。
fn lerp_transform(a: &str, b: &str, t: f64) -> String {
    let fa = parse_transform_funcs(a);
    let fb = parse_transform_funcs(b);
    let mut out = String::new();
    let mut has = false;
    let names = [
        "translateX",
        "translateY",
        "scaleX",
        "scaleY",
        "scale",
        "rotate",
    ];
    for name in names {
        let va = fa
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.first().copied());
        let vb = fb
            .iter()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.first().copied());
        let (Some(va), Some(vb)) = (va, vb) else {
            continue;
        };
        let v = va + (vb - va) * t;
        has = true;
        match name {
            "translateX" => out.push_str(&format!("translateX({}px) ", fmt(v))),
            "translateY" => out.push_str(&format!("translateY({}px) ", fmt(v))),
            "rotate" => out.push_str(&format!("rotate({}deg) ", fmt(v))),
            "scale" => out.push_str(&format!("scale({}) ", fmt(v))),
            _ => {}
        }
    }
    // scaleX/scaleY 组合成 scale(sx,sy)
    let pick = |f: &Vec<(String, Vec<f64>)>, want: &str| -> Option<f64> {
        f.iter()
            .find(|(n, _)| n == want)
            .and_then(|(_, v)| v.first().copied())
    };
    let sx_a = pick(&fa, "scaleX").or_else(|| pick(&fa, "scale"));
    let sx_b = pick(&fb, "scaleX").or_else(|| pick(&fb, "scale"));
    let sy_a = pick(&fa, "scaleY").or_else(|| pick(&fa, "scale"));
    let sy_b = pick(&fb, "scaleY").or_else(|| pick(&fb, "scale"));
    if let (Some(ox), Some(oy)) = (sx_a.zip(sx_b), sy_a.zip(sy_b)) {
        let sx = ox.0 + (ox.1 - ox.0) * t;
        let sy = oy.0 + (oy.1 - oy.0) * t;
        if (sx - 1.0).abs() > 1e-4 || (sy - 1.0).abs() > 1e-4 {
            has = true;
            out.push_str(&format!("scale({},{}) ", fmt(sx), fmt(sy)));
        }
    }
    if has {
        out.trim().to_string()
    } else {
        "none".to_string()
    }
}

/// transform 函数解析(数值参数列表;px/deg 单位剥除)。
fn parse_transform_funcs(s: &str) -> Vec<(String, Vec<f64>)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'-') {
                i += 1;
            }
            let name = s[start..i].to_string();
            if i < bytes.len() && bytes[i] == b'(' {
                let pstart = i + 1;
                let mut j = pstart;
                while j < bytes.len() && bytes[j] != b')' {
                    j += 1;
                }
                let inner = &s[pstart..j.min(s.len())];
                let nums: Vec<f64> = inner
                    .split(',')
                    .filter_map(|v| {
                        units::fmt_num(0.0);
                        v.trim()
                            .trim_end_matches("px")
                            .trim_end_matches("deg")
                            .trim()
                            .parse::<f64>()
                            .ok()
                    })
                    .collect();
                if !nums.is_empty() {
                    out.push((name, nums));
                }
                i = j + 1;
                continue;
            }
        } else {
            i += 1;
        }
    }
    out
}

/// 把 transform 文本应用到绘制项(平移 + 中心缩放 + 旋转累加)。
pub fn apply_transform(item: &mut vb_render::encode::DrawItem, transform: &str) {
    let mut tx = 0.0f64;
    let mut ty = 0.0f64;
    let mut sx = 1.0f64;
    let mut sy = 1.0f64;
    let mut rot = 0.0f64;
    for (name, args) in parse_transform_funcs(transform) {
        let v = args[0];
        match name.as_str() {
            "translateX" => tx += v,
            "translateY" => ty += v,
            "rotate" => rot += v,
            "scale" => {
                sx = v;
                sy = args.get(1).copied().unwrap_or(v);
            }
            "scaleX" => sx = v,
            "scaleY" => sy = v,
            _ => {}
        }
    }
    item.rect[0] += tx;
    item.rect[1] += ty;
    if (sx - 1.0).abs() > 1e-6 || (sy - 1.0).abs() > 1e-6 {
        let cx = item.rect[0] + item.rect[2] / 2.0;
        let cy = item.rect[1] + item.rect[3] / 2.0;
        item.rect[2] *= sx;
        item.rect[3] *= sy;
        item.rect[0] = cx - item.rect[2] / 2.0;
        item.rect[1] = cy - item.rect[3] / 2.0;
    }
    if rot.abs() > 1e-6 {
        item.rot += rot;
    }
}

fn fmt(v: f64) -> String {
    units::fmt_num(v)
}

/// 单动画时刻求值的属性集(供 context 应用)。
#[derive(Debug, Clone, Default)]
pub struct FrameState {
    pub opacity: Option<f64>,
    /// transform 文本(none = 无)
    pub transform: Option<String>,
    /// clip-path 文本(动画帧间插值)
    pub clip_path: Option<String>,
    /// filter 文本
    pub filter: Option<String>,
}

/// 节点全部动画在 t 时刻合成状态。
pub fn eval_node(node_anim: &NodeAnim, t: f64) -> FrameState {
    let mut opacity: Option<f64> = None;
    let mut transform: Option<String> = None;
    let mut clip_path: Option<String> = None;
    let mut filter: Option<String> = None;
    for (inst, kf) in &node_anim.instances {
        if let Some(v) = track_value_at(inst, kf, "opacity", t) {
            if let Ok(o) = v.trim().parse::<f64>() {
                opacity = Some(opacity.unwrap_or(1.0) * o);
            }
        }
        if let Some(v) = track_value_at(inst, kf, "transform", t) {
            if v != "none" {
                transform = Some(v);
            }
        }
        if let Some(v) = track_value_at(inst, kf, "clip-path", t) {
            clip_path = Some(v);
        }
        if let Some(v) = track_value_at(inst, kf, "filter", t) {
            filter = Some(v);
        }
    }
    FrameState {
        opacity,
        transform,
        clip_path,
        filter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    /// 动画卡家族的真实写法:名称 + var 时长 + var 缓动 + calc(var 延迟)。
    /// 回归:此前复合值里的 var/calc 不展开 → duration=0 → 整卡被当静态单帧。
    #[test]
    fn shorthand_resolves_vars_and_calc_in_timing() {
        let v = vars(&[
            ("in", ".4s"),
            ("ease-in", "cubic-bezier(0,0,0,1)"),
            ("t0", "0.1s"),
            ("d", "0.60"),
        ]);
        let a = parse_animation_shorthand(
            "rise-in var(--in) var(--ease-in) calc(var(--t0) + var(--d,0)*1s) both",
            &v,
        )
        .expect("var/calc 时序必须可解析");
        assert!((a.duration - 0.4).abs() < 1e-9, "duration={}", a.duration);
        assert!((a.delay - 0.7).abs() < 1e-9, "delay={}", a.delay);
        assert!(matches!(a.timing, Timing::CubicBezier(..)));
        assert!(a.fill_forwards && a.fill_backwards);
    }

    /// 字面量时长 + calc 延迟 + 迭代次数(混合写法,不得回归)。
    #[test]
    fn shorthand_literal_with_calc_delay_and_iterations() {
        let a = parse_animation_shorthand("blink 1s ease-in-out calc(0.1s + 0.2s) 3 both", &[])
            .expect("字面量 + calc 延迟必须可解析");
        assert!((a.duration - 1.0).abs() < 1e-9);
        assert!((a.delay - 0.3).abs() < 1e-9);
        assert!((a.iterations - 3.0).abs() < 1e-9);
    }

    /// var 时长(节点内联 token,如 hudgrow var(--hd))。
    #[test]
    fn shorthand_var_duration_from_inline_token() {
        let v = vars(&[("hd", "8.74s")]);
        let a = parse_animation_shorthand("hudgrow var(--hd) linear both", &v)
            .expect("var 时长必须可解析");
        assert!((a.duration - 8.74).abs() < 1e-9);
        assert!((a.delay - 0.0).abs() < 1e-9);
    }

    /// 查不到且无 fallback 的 var:保持旧行为(解析失败)。
    #[test]
    fn shorthand_unknown_var_without_fallback_fails() {
        assert!(parse_animation_shorthand("x var(--nope) linear both", &[]).is_none());
    }

    /// ms 单位与除法。
    #[test]
    fn calc_ms_and_division() {
        let v = vars(&[("w", "800ms")]);
        let a = parse_animation_shorthand("t var(--w) linear calc(1s/2) both", &v)
            .expect("ms/除法必须可解析");
        assert!((a.duration - 0.8).abs() < 1e-9);
        assert!((a.delay - 0.5).abs() < 1e-9);
    }
}
