//! 渐变结构化模型 + 自绘色标条(阶段 4 / 副文档 05-2;V4 决策:**自绘**)。
//!
//! **为什么自绘**:`ADR-0003:7` 明确「渐变编辑器等复杂控件需自绘」;egui 生态没有
//! 可用的色标条控件,`vb_ui` 此前也没有任何渐变/拖拽句柄原语。
//!
//! **单一真相**:[`Gradient`] 是渐变的唯一结构化表示。此前色标只以
//! `Vec<String>`(色 + 位置合一的字符串)透传,无法承载「拖动 / 位置输入 /
//! 中点 / 不透明度」。解析与序列化保证 `build(parse(x)) == x`(对 canonical
//! 输入恒等),因此 L1 字节幂等不被破坏。
//!
//! **可测边界**:几何与命中判定全部是纯函数([`stop_x`] / [`pos_at_x`] /
//! [`hit_stop`] / [`hit_hint`] / [`sample_color`]),不依赖 GPU 与窗口;
//! [`gradient_bar`] 只负责把它们画出来并把指针事件翻译成语义信号。

use egui::{Color32, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use vb_common::color::{parse_color, Rgba};
use vb_common::units::fmt_num;

use crate::{icons, theme};

// ─────────────────────────── 1. 数据模型 ───────────────────────────

/// 渐变类型。`Conic`(角度渐变)按 `design/06 §3.11` 标 v2,不进本模型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GradKind {
    Linear,
    Radial,
}

impl GradKind {
    pub fn label(self) -> &'static str {
        match self {
            GradKind::Linear => "线性",
            GradKind::Radial => "径向",
        }
    }

    /// CSS 函数名前缀(白名单按属性名放行,值只做规范化,见 `vb_css`)。
    pub fn css_fn(self) -> &'static str {
        match self {
            GradKind::Linear => "linear-gradient",
            GradKind::Radial => "radial-gradient",
        }
    }
}

/// 一个色标:位置 + CSS 颜色串。
#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    /// 位置,`0.0..=1.0`(UI 以百分比呈现)。
    pub pos: f64,
    /// 源串是否带**显式**位置。`false` = CSS 默认等距停点 —— 回写时不得
    /// 凭空补上位置串,否则首次保存就会改写源文件(L0 破损)。
    pub explicit: bool,
    /// CSS 颜色串(hex / `rgb()` / 命名色 / `var(--x)`);不可解析者原样保真。
    pub color: String,
}

impl Stop {
    /// 新建一个**显式**位置的色标。
    pub fn new(pos: f64, color: impl Into<String>) -> Self {
        Self {
            pos: pos.clamp(0.0, 1.0),
            explicit: true,
            color: color.into(),
        }
    }

    /// 回写 CSS 片段:`#fff 25%` / 无显式位置时只有颜色。
    pub fn to_css(&self) -> String {
        if self.explicit {
            format!("{} {}%", self.color, fmt_num(self.pos * 100.0))
        } else {
            self.color.clone()
        }
    }
}

/// 结构化渐变。
///
/// - `angle_explicit`:源串是否写了 `Ndeg`。`false` 时角度是 CSS 缺省
///   (`to bottom` = 180°),**回写不得补出 `180deg`** —— 否则首帧保存就改写
///   源文件(L0 破损)。
/// - `head`:首段的非角度声明(线性 `to right` / 径向 `circle at 50% 50%`),
///   原样保真。
/// - `hints`:插值中点(`(色标索引 i, 位置)`),落 CSS 为第 i 个色标之后的**裸
///   百分比/零长**(`linear-gradient(#000 0, 25%, #fff 100%)` 里的 `25%`)。
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub kind: GradKind,
    /// 线性角度:0° 向上、90° 向右,顺时针递增。
    pub angle: f64,
    pub angle_explicit: bool,
    pub head: Option<String>,
    pub stops: Vec<Stop>,
    pub hints: Vec<(usize, f64)>,
}

impl Gradient {
    /// 线性渐变,给定**显式**角度与色标。
    pub fn linear(angle: f64, stops: Vec<Stop>) -> Self {
        Self {
            kind: GradKind::Linear,
            angle: angle.rem_euclid(360.0),
            angle_explicit: true,
            head: None,
            stops,
            hints: Vec::new(),
        }
    }

    /// 「现填充 → 白」双色回退(与画布拖方向、控制面板数值化同一落点形态)。
    pub fn fallback_linear(angle: f64, fill_hex: &str) -> Self {
        let end = "#ffffff"; // vb-token-ok: 文档内容色(渐变默认端点)
        Self::linear(angle, vec![Stop::new(0.0, fill_hex), Stop::new(1.0, end)])
    }

    /// 回写 CSS 值(可直接写 `background-image`)。
    ///
    /// **保证输出即 canonical**:整串过一遍 `vb_css::canonical_value`
    /// (最短 hex、`0` 不带单位、函数名小写)—— 与导出/重导入的规范化同一套,
    /// 因此「编辑 → 保存 → 重开 → 保存」逐字节相同(L1 幂等,判据 C/6)。
    pub fn to_css(&self) -> String {
        vb_css::canonical_value(&self.to_css_raw())
    }

    /// 未规范化的拼装(仅供 [`Self::to_css`] 与调试)。
    pub fn to_css_raw(&self) -> String {
        let mut segs: Vec<String> = Vec::new();
        match self.kind {
            GradKind::Linear => {
                if self.angle_explicit {
                    segs.push(format!("{}deg", fmt_num(self.angle)));
                } else if let Some(h) = &self.head {
                    segs.push(h.clone());
                }
            }
            GradKind::Radial => {
                if let Some(h) = &self.head {
                    segs.push(h.clone());
                }
            }
        }
        for (i, s) in self.stops.iter().enumerate() {
            segs.push(s.to_css());
            if let Some((_, p)) = self.hints.iter().find(|(hi, _)| *hi == i) {
                segs.push(format!("{}%", fmt_num(p * 100.0)));
            }
        }
        format!("{}({})", self.kind.css_fn(), segs.join(", "))
    }

    /// 中点(插值提示)默认位置:相邻两色标的中点。
    pub fn midpoint_default(&self, i: usize) -> Option<f64> {
        let a = self.stops.get(i)?.pos;
        let b = self.stops.get(i + 1)?.pos;
        Some((a + b) / 2.0)
    }

    /// 反向:线性 = 角度 +180°;径向 = 色标位置镜像(`p → 1-p` 后重排)。
    /// 中点提示跟随各自前置色标,镜像后索引重算。
    pub fn reverse(&mut self) {
        match self.kind {
            GradKind::Linear => {
                if self.angle_explicit {
                    self.angle = (self.angle + 180.0).rem_euclid(360.0);
                } else if self.head.is_some() {
                    // 方向关键字形态(`to bottom` → `to top`)只做角度等价替换
                    self.head = None;
                    self.angle = 0.0;
                    self.angle_explicit = true;
                } else {
                    // 缺省 to bottom(180°)→ to top(0°)
                    self.angle = 0.0;
                    self.angle_explicit = true;
                }
            }
            GradKind::Radial => {
                let n = self.stops.len();
                for s in &mut self.stops {
                    s.pos = 1.0 - s.pos;
                }
                self.stops.reverse();
                // 原提示 i(在 i 与 i+1 之间)→ 镜像后位于 (n-2-i, n-1-i) 之间
                self.hints = self
                    .hints
                    .iter()
                    .filter_map(|(i, p)| {
                        let ni = n.checked_sub(2)?.checked_sub(*i)?;
                        Some((ni, 1.0 - *p))
                    })
                    .collect();
                self.hints.sort_by_key(|(i, _)| *i);
            }
        }
    }

    /// 视觉预览用:按位置 `t ∈ [0,1]` 取插值色(sRGB 线性插值,与 CSS
    /// legacy 渐变语法默认插值空间一致)。中点提示参与映射。
    pub fn sample(&self, t: f64, tokens: &[(String, String)]) -> Color32 {
        let n = self.stops.len();
        if n == 0 {
            // vb-token-ok: 文档内容色(空渐变占位)
            return parse_color("#000000")
                .map(rgba_to_color32)
                .unwrap_or_default();
        }
        if n == 1 {
            return stop_color(&self.stops[0], tokens).unwrap_or_default();
        }
        let t = t.clamp(0.0, 1.0);
        let mut i = 0usize;
        while i + 2 < n && t > self.stops[i + 1].pos {
            i += 1;
        }
        let (a, b) = (&self.stops[i], &self.stops[i + 1]);
        let (p0, p1) = (a.pos, b.pos);
        if p1 <= p0 {
            return stop_color(b, tokens).unwrap_or_default();
        }
        // 中点提示:把 [p0,p1] 分成两段,提示处为 50% 混色(CSS hint 语义)
        let u = match self.hints.iter().find(|(hi, _)| *hi == i).map(|(_, p)| *p) {
            Some(h) if h > p0 && h < p1 => {
                if t <= h {
                    0.5 * (t - p0) / (h - p0)
                } else {
                    0.5 + 0.5 * (t - h) / (p1 - h)
                }
            }
            _ => (t - p0) / (p1 - p0),
        };
        let ca = stop_color(a, tokens).unwrap_or_default();
        let cb = stop_color(b, tokens).unwrap_or_default();
        lerp_color32(ca, cb, u as f32)
    }
}

/// 解析 `background-image` 的单个渐变值。非渐变(或多层叠加)返回 `None`。
pub fn parse(value: &str) -> Option<Gradient> {
    let v = value.trim();
    let (kind, body) = if let Some(b) = v.strip_prefix("linear-gradient(") {
        (GradKind::Linear, b)
    } else {
        (GradKind::Radial, v.strip_prefix("radial-gradient(")?)
    };
    // CSS 允许 `linear-gradient(45deg, …)` 无空格形态;`canonical_value` 会先
    // 规范化,此处再做一次容错。
    let body = body.strip_suffix(')')?;
    let segs = split_top_commas(body);
    if segs.is_empty() {
        return None;
    }
    let mut angle = 180.0; // CSS 缺省 = `to bottom`
    let mut angle_explicit = false;
    let mut head = None;
    let mut start = 0usize;
    match kind {
        GradKind::Linear => {
            let first = segs[0].trim();
            if let Some(a) = parse_angle(first) {
                angle = a;
                angle_explicit = true;
                start = 1;
            } else if first.to_ascii_lowercase().starts_with("to ") {
                // 方向关键字(`to right`/`to bottom left`):原样保真
                head = Some(first.to_string());
                start = 1;
            }
            // 否则首段就是色标(无角度声明)= CSS 缺省 to bottom
        }
        GradKind::Radial => {
            let first = segs[0].trim();
            if first.contains(" at ") || first.starts_with("circle") || first.starts_with("ellipse")
            {
                head = Some(first.to_string());
                start = 1;
            }
        }
    }
    let mut stops: Vec<Stop> = Vec::new();
    let mut hints: Vec<(usize, f64)> = Vec::new();
    for seg in &segs[start..] {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        if let Some(p) = parse_pos(seg) {
            // 裸位置段 = 插值中点(作用于前一个色标)
            if let Some(last) = stops.len().checked_sub(1) {
                hints.push((last, p));
            }
            continue;
        }
        let (color, pos) = split_stop(seg);
        stops.push(Stop {
            pos: pos.unwrap_or_else(|| default_pos(stops.len(), segs.len().saturating_sub(start))),
            explicit: pos.is_some(),
            color,
        });
    }
    if stops.len() < 2 {
        return None;
    }
    Some(Gradient {
        kind,
        angle,
        angle_explicit,
        head,
        stops,
        hints,
    })
}

/// 角度段(`45deg` / `-90deg`)→ 度;非角度返回 `None`。
fn parse_angle(seg: &str) -> Option<f64> {
    let n = seg.strip_suffix("deg")?.trim();
    let v: f64 = n.parse().ok()?;
    v.is_finite().then(|| v.rem_euclid(360.0))
}

/// 顶层逗号拆分(括号深度归零处才算分隔;`rgb(255, 0, 0) 0%` 保持一体)。
pub fn split_top_commas(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(ch);
            }
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur = String::new();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// 位置段(`50%` / `0`)→ 位置 `0..1`;其余 `None`。
///
/// CSS 里唯一合法的**无单位长度**是 `0`,而 `vb_css::canonical_value` 会把
/// `0%` 规范化成 `0` —— 解析必须接受它,否则规范化后的值解析不出位置,
/// 整个色标会退化成一串颜色文本(L1 随之中断)。
fn parse_pos(seg: &str) -> Option<f64> {
    let t = seg.trim();
    if let Some(n) = t.strip_suffix('%') {
        if n.trim().contains(' ') {
            return None;
        }
        return n.trim().parse::<f64>().ok().map(|p| p / 100.0);
    }
    (t == "0").then_some(0.0)
}

/// 色标段 → (颜色串, 显式位置)。位置取**最后一个空白分隔 token**,
/// 且必须形如 `N%` 或 `0`;其余(含 `12px`、函数内的空格)全部算颜色串,原样保真。
fn split_stop(seg: &str) -> (String, Option<f64>) {
    if let Some((head, tail)) = seg.rsplit_once(char::is_whitespace) {
        if let Some(p) = parse_pos(tail) {
            return (head.trim().to_string(), Some(p));
        }
    }
    (seg.to_string(), None)
}

/// CSS 未写位置的色标按等距分布(`n` 个色标里第 `i` 个)。
fn default_pos(i: usize, n: usize) -> f64 {
    if n <= 1 {
        0.0
    } else {
        i as f64 / (n - 1) as f64
    }
}

/// 色标颜色 → sRGB;`var(--x)` 先经文档令牌解析。
pub fn stop_color(stop: &Stop, tokens: &[(String, String)]) -> Option<Color32> {
    resolve_color(&stop.color, tokens).map(rgba_to_color32)
}

/// 解析颜色串(含 `var(--x)` 一层展开,取令牌 `(name, value)`)。
pub fn resolve_color(v: &str, tokens: &[(String, String)]) -> Option<Rgba> {
    resolve_color_impl(v, tokens)
}

/// 色标不透明度 `0..1`(不可解析色串 → 1.0)。
pub fn stop_alpha(stop: &Stop, tokens: &[(String, String)]) -> f64 {
    resolve_color_impl(&stop.color, tokens)
        .map(|c| c.a as f64 / 255.0)
        .unwrap_or(1.0)
}

/// 返回「同一颜色、指定不透明度」的最短 hex(不可解析 → 原样返回)。
pub fn with_alpha(stop: &Stop, alpha: f64, tokens: &[(String, String)]) -> String {
    match resolve_color_impl(&stop.color, tokens) {
        Some(c) => Rgba {
            a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ..c
        }
        .to_shortest_hex(),
        None => stop.color.clone(),
    }
}

fn resolve_color_impl(v: &str, tokens: &[(String, String)]) -> Option<Rgba> {
    let t = v.trim();
    if let Some(inner) = t.strip_prefix("var(").and_then(|s| s.strip_suffix(')')) {
        let name = inner
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .trim_start_matches("--")
            .trim();
        let raw = tokens
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())?;
        return parse_color(&raw);
    }
    parse_color(t)
}

fn rgba_to_color32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// sRGB 线性插值(两端精确命中)。
pub fn lerp_color32(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

// ─────────────────────── 2. 几何 / 命中(纯函数) ───────────────────────

/// 条内可用宽度(两端各留一个手柄半径,首末色标不贴边)。
pub const HANDLE_R: f32 = 5.5;
/// 渐变预览条高度。
pub const BAR_H: f32 = 22.0;
/// 色标手柄行高度(预览条下方)。
pub const HANDLE_H: f32 = 15.0;

/// 位置 `0..1` → 屏幕 x。`rail` 为整条矩形。
pub fn stop_x(pos: f64, rail: Rect) -> f32 {
    let lo = rail.left() + HANDLE_R;
    let hi = rail.right() - HANDLE_R;
    lo + (hi - lo) * pos.clamp(0.0, 1.0) as f32
}

/// 屏幕 x → 位置 `0..1`(钳制)。
pub fn pos_at_x(x: f32, rail: Rect) -> f64 {
    let lo = rail.left() + HANDLE_R;
    let hi = rail.right() - HANDLE_R;
    if hi <= lo {
        return 0.0;
    }
    ((x - lo) / (hi - lo)).clamp(0.0, 1.0) as f64
}

/// 画布渐变批注:起点/终点(屏幕坐标)→ 各色标的屏幕点(05-2-3)。
/// `a` 为起点、`b` 为终点;色标位置沿直线线性映射。
pub fn annot_points(a: (f32, f32), b: (f32, f32), g: &Gradient) -> Vec<(usize, f32, f32)> {
    g.stops
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let t = s.pos.clamp(0.0, 1.0) as f32;
            (i, a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        })
        .collect()
}

/// 命中画布批注上的色标(屏幕距离 ≤ `tol`),返回索引(取最近者)。
pub fn hit_annot_stop(
    a: (f32, f32),
    b: (f32, f32),
    g: &Gradient,
    p: (f32, f32),
    tol: f32,
) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, x, y) in annot_points(a, b, g) {
        let d = ((x - p.0).powi(2) + (y - p.1).powi(2)).sqrt();
        if d <= tol && best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// 命中色标:`|x - stop_x| <= HANDLE_R + 1`,返回索引(取最近者)。
pub fn hit_stop(g: &Gradient, x: f32, rail: Rect) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;
    for (i, s) in g.stops.iter().enumerate() {
        let d = (stop_x(s.pos, rail) - x).abs();
        if d <= HANDLE_R + 1.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// 命中间点菱形:位于第 `i`/`i+1` 个色标之间的提示(或默认中点)处。
pub fn hit_hint(g: &Gradient, x: f32, rail: Rect) -> Option<usize> {
    let n = g.stops.len();
    let mut best: Option<(usize, f32)> = None;
    for i in 0..n.saturating_sub(1) {
        let p = g
            .hints
            .iter()
            .find(|(hi, _)| *hi == i)
            .map(|(_, p)| *p)
            .or_else(|| g.midpoint_default(i))?;
        let d = (stop_x(p, rail) - x).abs();
        if d <= HANDLE_R + 1.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

// ─────────────────────── 3. 自绘色标条控件 ───────────────────────

/// [`gradient_bar`] 的交互结果。
#[derive(Debug, Clone, Default)]
pub struct GradientBarResponse {
    /// 色标/中点被改动(拖动中逐帧为 true)。
    pub changed: bool,
    /// 一次拖动开始 / 结束(NumField 会话合并同款信号,连续拖动 = 一条 undo)。
    pub drag_started: bool,
    pub drag_ended: bool,
    /// **离散**改动发生(新增/删除色标):调用方应作为独立 undo 提交。
    pub discrete: bool,
    /// 双击色标索引(调用方弹取色器)。
    pub double_clicked: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum DragTarget {
    Stop(usize),
    Hint(usize),
}

/// 自绘色标条:预览 + 可拖色标 + 中点菱形。
///
/// 交互(与 `design/03 §5.6` 对齐):
/// - 拖动色标 → 改位置;`Alt+点击`色标 → 删除(`stops > 2` 时);
/// - 点击空白 → 在该位置**新增**色标(离散操作);
/// - 双击色标 → 交给调用方弹取色器;
/// - 拖动中点菱形 → 改插值中点(`hints`)。
pub fn gradient_bar(
    ui: &mut Ui,
    g: &mut Gradient,
    selected: &mut Option<usize>,
    tokens: &[(String, String)],
) -> GradientBarResponse {
    let t = theme::tokens(ui.ctx());
    let mut out = GradientBarResponse::default();
    let width = ui.available_width().max(80.0);
    let (full, resp) =
        ui.allocate_exact_size(Vec2::new(width, BAR_H + HANDLE_H), Sense::click_and_drag());
    let bar = Rect::from_min_max(
        egui::pos2(full.left(), full.top()),
        egui::pos2(full.right(), full.top() + BAR_H),
    );
    let rail = Rect::from_min_max(
        egui::pos2(full.left(), bar.bottom()),
        egui::pos2(full.right(), full.bottom()),
    );

    // ── 预览:按 96 段采样,精确反映 stop + hint 的插值 ──
    let id = resp.id;
    let n = 96;
    for i in 0..n {
        let t0 = i as f64 / n as f64;
        let t1 = (i + 1) as f64 / n as f64;
        let x0 = bar.left() + bar.width() * t0 as f32;
        let x1 = bar.left() + bar.width() * t1 as f32;
        let c = g.sample((t0 + t1) / 2.0, tokens);
        ui.painter().rect_filled(
            Rect::from_min_max(
                egui::pos2(x0, bar.top()),
                egui::pos2(x1 + 0.5, bar.bottom()),
            ),
            0.0,
            c,
        );
    }
    // 透明棋盘提示(首末色标若有 alpha 可辨),再压描边
    ui.painter().rect_stroke(
        bar,
        theme::radius::sm(),
        Stroke::new(theme::stroke::HAIRLINE, t.border),
        StrokeKind::Inside,
    );

    // ── 事件:只在拖动开始时命中一次,拖动中沿用 ──
    let mut state = ui
        .data_mut(|d| d.get_temp::<DragTarget>(id))
        .unwrap_or(DragTarget::Stop(usize::MAX));

    if resp.drag_started() {
        if let Some(x) = resp.interact_pointer_pos().map(|p| p.x) {
            if let Some(i) = hit_stop(g, x, rail) {
                state = DragTarget::Stop(i);
                *selected = Some(i);
            } else if let Some(i) = hit_hint(g, x, rail) {
                state = DragTarget::Hint(i);
            } else {
                // 空白处拖动 = 新增色标并立即拖动它(离散操作)
                let p = pos_at_x(x, rail);
                let c = g.sample(p, tokens);
                g.stops.push(Stop::new(p, rgba_shortest(c)));
                g.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                let idx = g
                    .stops
                    .iter()
                    .position(|s| (s.pos - p).abs() < 1e-9)
                    .unwrap_or(g.stops.len().saturating_sub(1));
                // 排序会挪动索引:重建 hints 索引
                reindex_hints(g);
                state = DragTarget::Stop(idx);
                *selected = Some(idx);
                out.discrete = true;
                out.changed = true;
            }
            out.drag_started = true;
        }
    }

    if resp.dragged() {
        if let Some(x) = resp.interact_pointer_pos().map(|p| p.x) {
            let p = pos_at_x(x, rail);
            match state {
                DragTarget::Stop(i) if i < g.stops.len() => {
                    g.stops[i].pos = p;
                    g.stops[i].explicit = true;
                    out.changed = true;
                }
                DragTarget::Hint(i) => {
                    if let Some((lo, hi)) = bounds(g, i) {
                        let p = p.clamp(lo + 1e-3, hi - 1e-3);
                        match g.hints.iter_mut().find(|(hi_i, _)| *hi_i == i) {
                            Some(slot) => slot.1 = p,
                            None => g.hints.push((i, p)),
                        }
                        out.changed = true;
                    }
                }
                _ => {}
            }
        }
    }
    if resp.drag_stopped() {
        out.drag_ended = true;
    }

    if resp.clicked() {
        if let Some(x) = resp.interact_pointer_pos().map(|p| p.x) {
            let alt = ui.ctx().input(|i| i.modifiers.alt);
            if let Some(i) = hit_stop(g, x, rail) {
                if alt {
                    if g.stops.len() > 2 {
                        g.stops.remove(i);
                        reindex_hints(g);
                        *selected = Some(0);
                        out.discrete = true;
                    }
                } else {
                    *selected = Some(i);
                }
            } else if hit_hint(g, x, rail).is_none() {
                // 点空白:新增色标(离散操作;拖动路径已在 drag_started 覆盖)
                if !resp.dragged() {
                    let p = pos_at_x(x, rail);
                    let c = g.sample(p, tokens);
                    g.stops.push(Stop::new(p, rgba_shortest(c)));
                    g.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
                    reindex_hints(g);
                    let idx = g
                        .stops
                        .iter()
                        .position(|s| (s.explicit) && (s.pos - p).abs() < 1e-9)
                        .unwrap_or(0);
                    *selected = Some(idx);
                    out.discrete = true;
                }
            }
        }
    }
    if resp.double_clicked() {
        if let Some(x) = resp.interact_pointer_pos().map(|p| p.x) {
            if let Some(i) = hit_stop(g, x, rail) {
                *selected = Some(i);
                out.double_clicked = Some(i);
            }
        }
    }

    // ── 手柄行:色标圆点 + 中点菱形 + 选中环 ──
    let painter = ui.painter();
    for (i, s) in g.stops.iter().enumerate() {
        let cx = stop_x(s.pos, rail);
        let cy = rail.center().y;
        let col = stop_color(s, tokens).unwrap_or(t.text);
        let sel = *selected == Some(i);
        painter.circle_filled(egui::pos2(cx, cy), HANDLE_R, col);
        painter.circle_stroke(
            egui::pos2(cx, cy),
            HANDLE_R,
            Stroke::new(
                if sel {
                    theme::stroke::FOCUS
                } else {
                    theme::stroke::HAIRLINE
                },
                if sel { t.accent } else { t.border_strong },
            ),
        );
    }
    for i in 0..g.stops.len().saturating_sub(1) {
        let p = g
            .hints
            .iter()
            .find(|(hi, _)| *hi == i)
            .map(|(_, p)| *p)
            .or_else(|| g.midpoint_default(i))
            .unwrap_or(0.5);
        let cx = stop_x(p, rail);
        let cy = rail.center().y;
        let r = 3.5;
        let fill = if g.hints.iter().any(|(hi, _)| *hi == i) {
            t.accent
        } else {
            t.text_3
        };
        painter.add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(cx, cy - r),
                egui::pos2(cx + r, cy),
                egui::pos2(cx, cy + r),
                egui::pos2(cx - r, cy),
            ],
            fill,
            Stroke::new(theme::stroke::HAIRLINE, t.border),
        ));
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    ui.data_mut(|d| d.insert_temp(id, state));
    let _ = icons::Name::ToolGradient; // 保持 icons 依赖(面板标题复用)
    out
}

/// 第 `i` 个色标与第 `i+1` 个色标的位置界(供中点钳制)。
fn bounds(g: &Gradient, i: usize) -> Option<(f64, f64)> {
    let a = g.stops.get(i)?.pos;
    let b = g.stops.get(i + 1)?.pos;
    Some((a.min(b), a.max(b)))
}

/// 色标被增删/排序后,中点提示索引会失效 —— 按位置就近重挂。
fn reindex_hints(g: &mut Gradient) {
    if g.hints.is_empty() {
        return;
    }
    let old: Vec<(usize, f64)> = g.hints.clone();
    g.hints.clear();
    for (_, p) in old {
        // 找到包含该位置的两个相邻色标
        let mut idx = None;
        for i in 0..g.stops.len().saturating_sub(1) {
            let (lo, hi) = (g.stops[i].pos, g.stops[i + 1].pos);
            if p >= lo && p <= hi {
                idx = Some(i);
                break;
            }
        }
        if let Some(i) = idx {
            if !g.hints.iter().any(|(hi, _)| *hi == i) {
                g.hints.push((i, p));
            }
        }
    }
}

/// `Color32` → 最短 hex(新增色标时把插值结果固化)。
pub fn rgba_shortest(c: Color32) -> String {
    Rgba {
        r: c.r(),
        g: c.g(),
        b: c.b(),
        a: c.a(),
    }
    .to_shortest_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rail() -> Rect {
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(200.0, 10.0))
    }

    /// `to_css` 必须产出 **canonical** 形式(与 `vb_css::canonical_value` 同口径),
    /// 且再解析再回写逐字节相同 —— 这是 L1 字节幂等的前提。
    #[test]
    fn parse_build_is_canonical_and_idempotent() {
        for (src, want) in [
            // 最短 hex + `0%` → `0`
            (
                "linear-gradient(45deg, #ff5a1f 0%, #ffffff 100%)",
                "linear-gradient(45deg, #ff5a1f 0, #fff 100%)",
            ),
            // 中点提示保持裸段
            (
                "linear-gradient(90deg, #000 0%, 25%, #fff 100%)",
                "linear-gradient(90deg, #000 0, 25%, #fff 100%)",
            ),
            (
                "radial-gradient(circle at 50% 50%, rgb(255, 0, 0) 0%, #00ff00 100%)",
                "radial-gradient(circle at 50% 50%, rgb(255, 0, 0) 0, #0f0 100%)",
            ),
            // 无显式位置的色标:不得凭空补位置
            (
                "linear-gradient(180deg, #aaa, #bbb)",
                "linear-gradient(180deg, #aaa, #bbb)",
            ),
            // 方向关键字形态保真
            (
                "linear-gradient(to right, #000 0%, #fff 100%)",
                "linear-gradient(to right, #000 0, #fff 100%)",
            ),
            // 无角度声明(CSS 缺省 to bottom):不得补 180deg
            (
                "linear-gradient(#000 0%, #fff 100%)",
                "linear-gradient(#000 0, #fff 100%)",
            ),
        ] {
            let g = parse(src).unwrap_or_else(|| panic!("解析失败: {src}"));
            let got = g.to_css();
            assert_eq!(got, want, "canonical 形式不符({src})");
            assert_eq!(parse(&got).unwrap().to_css(), got, "to_css 不幂等:{got}");
        }
    }

    /// 无角度声明的线性渐变:角度语义 = CSS 缺省 180°(to bottom);
    /// 反向时才落地显式角度。
    #[test]
    fn angle_less_linear_keeps_default_direction() {
        let g = parse("linear-gradient(#000 0, #fff 100%)").unwrap();
        assert!(!g.angle_explicit);
        assert!((g.angle - 180.0).abs() < 1e-9, "angle={}", g.angle);
        assert_eq!(g.to_css(), "linear-gradient(#000 0, #fff 100%)");
        let mut r = g.clone();
        r.reverse();
        assert_eq!(r.to_css(), "linear-gradient(0deg, #000 0, #fff 100%)");
    }

    #[test]
    fn parse_rejects_non_gradient() {
        assert!(parse("solid #fff").is_none());
        assert!(parse("#ffffff").is_none()); // vb-token-ok: 测试夹具
        assert!(parse("linear-gradient(45deg, #fff)").is_none()); // 单色标不成渐变
    }

    #[test]
    fn hint_is_preserved_and_drives_interpolation() {
        let g = parse("linear-gradient(90deg, #000 0%, 80%, #fff 100%)").unwrap();
        assert_eq!(g.hints, vec![(0, 0.8)]);
        let toks: Vec<(String, String)> = Vec::new();
        // hint 在 80%:自此才达到 50% 混色 → 位置 0.8 处约等于 #808080
        let c = g.sample(0.8, &toks);
        assert!((c.r() as i32 - 128).abs() <= 2, "r={}", c.r());
    }

    #[test]
    fn reverse_linear_rotates_angle_only() {
        let mut g = parse("linear-gradient(45deg, #000 0%, #fff 100%)").unwrap();
        g.reverse();
        assert_eq!(g.to_css(), "linear-gradient(225deg, #000 0, #fff 100%)");
    }

    #[test]
    fn reverse_radial_mirrors_positions() {
        let mut g =
            parse("radial-gradient(circle at 50% 50%, #000 0%, #f00 25%, #fff 100%)").unwrap();
        g.reverse();
        assert_eq!(
            g.to_css(),
            "radial-gradient(circle at 50% 50%, #fff 0, #f00 75%, #000 100%)"
        );
    }

    #[test]
    fn geometry_roundtrips_positions() {
        let r = rail();
        for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let x = stop_x(p, r);
            let back = pos_at_x(x, r);
            assert!((back - p).abs() < 1e-6, "p={p} back={back}");
        }
    }

    #[test]
    fn hit_stop_picks_nearest_within_tolerance() {
        let g = parse("linear-gradient(90deg, #000 0%, #fff 100%)").unwrap();
        let r = rail();
        let x0 = stop_x(0.0, r);
        assert_eq!(hit_stop(&g, x0, r), Some(0));
        assert_eq!(hit_stop(&g, stop_x(1.0, r), r), Some(1));
        assert_eq!(hit_stop(&g, stop_x(0.5, r), r), None);
    }

    #[test]
    fn hit_hint_uses_default_midpoint_when_absent() {
        let g = parse("linear-gradient(90deg, #000 0%, #fff 100%)").unwrap();
        let r = rail();
        assert!(g.hints.is_empty());
        assert_eq!(hit_hint(&g, stop_x(0.5, r), r), Some(0));
    }

    #[test]
    fn sample_hits_both_ends_exactly() {
        let g = parse("linear-gradient(90deg, #000 0%, #fff 100%)").unwrap();
        let toks: Vec<(String, String)> = Vec::new();
        // vb-token-ok: 测试夹具
        let black = Color32::from_rgb(0, 0, 0);
        // vb-token-ok: 测试夹具
        let white = Color32::from_rgb(255, 255, 255);
        assert_eq!(g.sample(0.0, &toks), black);
        assert_eq!(g.sample(1.0, &toks), white);
    }

    #[test]
    fn var_color_resolves_from_tokens() {
        // vb-token-ok: 测试夹具
        let toks = vec![("vb-color-1".to_string(), "#ff0000".to_string())];
        let g = parse("linear-gradient(90deg, var(--vb-color-1) 0%, #fff 100%)").unwrap();
        let c = stop_color(&g.stops[0], &toks).unwrap();
        // vb-token-ok: 测试夹具
        assert_eq!(c, Color32::from_rgb(255, 0, 0));
        assert_eq!(
            g.to_css(),
            "linear-gradient(90deg, var(--vb-color-1) 0, #fff 100%)"
        );
    }
}
