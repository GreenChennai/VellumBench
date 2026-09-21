//! 外观面板(`⇧F6`,05-1)与描边面板(`^F10`,05-3)+ 效果写回(05-6)。
//!
//! 结构与控制面板同纪律(ADR-VB-U04):**纯函数模型/编解码/构建器 + 门禁
//! 测试**,渲染层只做「投影 → 构建命令 → [`VellumApp::exec`]/[`num_commit`]」,
//! 不在面板私存文档状态。
//!
//! # 外观条目模型与 CSS 落盘机制(05-1 核心设计)
//!
//! AI 的「外观」是条目列表:填充/描边/效果可多条、可排序、可禁用;而 CSS
//! 天然单背景/双边框。本模块的裁定(详见 `05a` 报告):
//!
//! 1. **模型真值** = 元素属性 `data-vb-appearance`(紧凑 JSON,经
//!    `SetAttrs` 落盘;导入/导出对未知 `data-*` 属性原样往返,故条目的
//!    顺序/眼睛/混合模式/冻结字段**无损**穿越 HTML 管线);
//! 2. **渲染真值** = 按映射表从启用条目**编译**出的 CSS 声明
//!    (经 `SetStyle` 落盘):多条填充 → `background-image` 逗号多层叠加
//!    (纯色层以同色双 stop 渐变承载,像素恒等)、多条阴影 → `box-shadow`
//!    逗号分段、多个模糊 → `filter` 空格串联;
//! 3. **接管集 `own`** 记录「哪些属性组已由模型接管」:编译只重写接管组
//!    (清空 = 移除声明),未接管的手写声明(如属性面板直改的
//!    `border-radius`)绝不动 —— 不静默丢弃、不双写打架;
//! 4. **无损裁定**:接管组内启用条目编译 ⇄ 解码恒等(单元测试锁);
//!    无 CSS 落点的字段(描边箭头、条目级混合对描边/效果、跨类效果顺序)
//!    保存在模型中并如实标注「未落盘」。
//!
//! 解码优先级:有 `data-vb-appearance` → 纯 JSON(模型无损);无 → 从
//! CSS 启发式解码(导入既有文件;只认领可完整表达的声明)。
//!
//! 命令路径:每次条目操作 = `Compound[SetStyle(重编译), SetAttrs(模型)]`
//! 一条 undo;`merge_target` 已放行同目标 `SetStyle+SetAttrs` 混合
//! Compound,NumField 会话合并(拖数值只产生一条 undo)照常生效。

use serde::{Deserialize, Serialize};

use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeId, NodeKind};

mod ui;

// ═══════════════════════ 1. 模型(serde;紧凑 JSON 落盘) ═══════════════════════

/// 模型落盘属性名。
pub const APPEARANCE_ATTR: &str = "data-vb-appearance";

/// 接管集成员(编译器据此决定重写/移除哪些属性组)。
pub const OWN_FILL: &str = "fill";
pub const OWN_STROKE: &str = "stroke";
pub const OWN_SHADOW: &str = "shadow";
pub const OWN_BLUR: &str = "blur";
pub const OWN_RADIUS: &str = "radius";
pub const OWN_MASK: &str = "mask";

/// 外观条目模型(节点级)。`own` = 已接管属性组(只增不减,见模块注释 3)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceModel {
    /// 模式版本(解码时校验;非 1 视为损坏,走 CSS 启发式)。
    pub v: u32,
    /// 已接管的属性组/属性名(见模块注释 3)。
    #[serde(default)]
    pub own: Vec<String>,
    /// 条目列表(**首条 = 最上层**,与 AI 面板一致)。
    #[serde(default)]
    pub items: Vec<AppearanceItem>,
}

impl AppearanceModel {
    fn empty() -> Self {
        AppearanceModel {
            v: 1,
            own: Vec::new(),
            items: Vec::new(),
        }
    }

    fn own_add(own: &mut Vec<String>, tag: &str) {
        if !own.iter().any(|o| o == tag) {
            own.push(tag.to_string());
        }
    }
}
/// 外观条目:填充 / 描边 / 效果(内部 tag = `kind`,snake_case)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppearanceItem {
    Fill(FillItem),
    Stroke(StrokeItem),
    Effect(EffectItem),
}

impl AppearanceItem {
    pub fn enabled(&self) -> bool {
        match self {
            AppearanceItem::Fill(f) => f.enabled,
            AppearanceItem::Stroke(s) => s.enabled,
            AppearanceItem::Effect(e) => e.enabled,
        }
    }

    fn set_enabled(&mut self, v: bool) {
        match self {
            AppearanceItem::Fill(f) => f.enabled = v,
            AppearanceItem::Stroke(s) => s.enabled = v,
            AppearanceItem::Effect(e) => e.enabled = v,
        }
    }

    pub fn blend(&self) -> Option<&str> {
        match self {
            AppearanceItem::Fill(f) => f.blend.as_deref(),
            AppearanceItem::Stroke(s) => s.blend.as_deref(),
            AppearanceItem::Effect(e) => e.blend.as_deref(),
        }
    }

    fn set_blend(&mut self, v: Option<String>) {
        match self {
            AppearanceItem::Fill(f) => f.blend = v,
            AppearanceItem::Stroke(s) => s.blend = v,
            AppearanceItem::Effect(e) => e.blend = v,
        }
    }

    /// 行摘要(面板条目行 / 测试断言)。
    pub fn summary(&self) -> String {
        match self {
            AppearanceItem::Fill(f) => match &f.body {
                FillBody::Solid { value } => format!("填充 {value}"),
                FillBody::Gradient { value } => format!("填充 渐变({value})"),
                FillBody::Raw { value } => format!("填充 {value}"),
            },
            AppearanceItem::Stroke(s) => {
                format!("描边 {}px", vb_common::units::fmt_num(s.spec.width))
            }
            AppearanceItem::Effect(e) => format!("效果 {}", e.effect.label()),
        }
    }
}

/// 填充条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FillItem {
    pub enabled: bool,
    /// 条目级混合(填充 → `background-blend-mode` 逐层,无损;
    /// 见 `BLEND_MODES`)。
    #[serde(default)]
    pub blend: Option<String>,
    pub body: FillBody,
}

/// 填充体。`Raw` 承载导入 CSS 中非渐变层(`url()` 等),原样保真。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FillBody {
    Solid { value: String },
    Gradient { value: String },
    Raw { value: String },
}

/// 描边条目(05-3 全字段;各字段对不同目标的支持度见 [`StrokeFieldSupport`])。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeItem {
    pub enabled: bool,
    #[serde(default)]
    pub blend: Option<String>,
    pub spec: StrokeSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeCap {
    /// 平头(CSS/SVG 默认)
    #[default]
    Butt,
    Round,
    Square,
}

impl StrokeCap {
    pub fn label(self) -> &'static str {
        match self {
            StrokeCap::Butt => "平头",
            StrokeCap::Round => "圆头",
            StrokeCap::Square => "方头",
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            StrokeCap::Butt => "butt",
            StrokeCap::Round => "round",
            StrokeCap::Square => "square",
        }
    }
    pub fn from_css(v: &str) -> StrokeCap {
        match v {
            "round" => StrokeCap::Round,
            "square" => StrokeCap::Square,
            _ => StrokeCap::Butt,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeJoin {
    /// 斜接(CSS/SVG 默认)
    #[default]
    Miter,
    Round,
    Bevel,
}

impl StrokeJoin {
    pub fn label(self) -> &'static str {
        match self {
            StrokeJoin::Miter => "斜接",
            StrokeJoin::Round => "圆角",
            StrokeJoin::Bevel => "斜切",
        }
    }
    pub fn css(self) -> &'static str {
        match self {
            StrokeJoin::Miter => "miter",
            StrokeJoin::Round => "round",
            StrokeJoin::Bevel => "bevel",
        }
    }
    pub fn from_css(v: &str) -> StrokeJoin {
        match v {
            "round" => StrokeJoin::Round,
            "bevel" => StrokeJoin::Bevel,
            _ => StrokeJoin::Miter,
        }
    }
}

/// 描边对齐(AI 语义)。SVG 描边恒居中;CSS `border` 恒内侧。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrokeAlign {
    /// 居中(AI 默认;矢量落 SVG 恒居中,盒对象落 border + 降级注记)
    #[default]
    Center,
    /// 内侧(盒对象 → `border-*`,无损)
    Inside,
    /// 外侧(盒对象 → `outline-*`,不占布局;虚线仅 solid/dashed 近似)
    Outside,
}

impl StrokeAlign {
    pub fn label(self) -> &'static str {
        match self {
            StrokeAlign::Center => "居中",
            StrokeAlign::Inside => "内侧",
            StrokeAlign::Outside => "外侧",
        }
    }
}

/// 箭头(CSS/SVG marker 未建模 → 冻结登记:模型保留,不落盘,UI 提示)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Arrowhead {
    #[default]
    None,
    Arrow,
    Circle,
    Tick,
}

impl Arrowhead {
    pub fn label(self) -> &'static str {
        match self {
            Arrowhead::None => "无",
            Arrowhead::Arrow => "箭头",
            Arrowhead::Circle => "圆点",
            Arrowhead::Tick => "短线",
        }
    }
}

/// 描边参数(05-3-1/2 全字段)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeSpec {
    pub width: f64,
    /// None = currentColor 语义(不写颜色声明)。
    pub color: Option<String>,
    #[serde(default)]
    pub cap: StrokeCap,
    #[serde(default)]
    pub join: StrokeJoin,
    #[serde(default = "default_miter")]
    pub miter_limit: f64,
    /// 虚线 pattern(on/off 交替,单位 px;**最多 6 组** = 3 对值/间隙)。
    #[serde(default)]
    pub dash: Vec<f64>,
    #[serde(default)]
    pub align: StrokeAlign,
    #[serde(default)]
    pub arrow_start: Arrowhead,
    #[serde(default)]
    pub arrow_end: Arrowhead,
}

fn default_miter() -> f64 {
    4.0
}

impl Default for StrokeSpec {
    fn default() -> Self {
        StrokeSpec {
            width: 1.0,
            color: None,
            cap: StrokeCap::default(),
            join: StrokeJoin::default(),
            miter_limit: default_miter(),
            dash: Vec::new(),
            align: StrokeAlign::default(),
            arrow_start: Arrowhead::default(),
            arrow_end: Arrowhead::default(),
        }
    }
}

/// 效果条目。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectItem {
    pub enabled: bool,
    #[serde(default)]
    pub blend: Option<String>,
    pub effect: Effect,
}

/// 六种效果(design/06 §4.6 映射表)+ 羽化 + 未识别段保真。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// 投影 → `box-shadow: X Y blur spread color`
    DropShadow {
        x: f64,
        y: f64,
        blur: f64,
        spread: f64,
        color: String,
    },
    /// 内阴影 → `box-shadow: inset X Y blur spread color`
    InnerShadow {
        x: f64,
        y: f64,
        blur: f64,
        spread: f64,
        color: String,
    },
    /// 外发光 → `box-shadow: 0 0 blur color`
    OuterGlow { blur: f64, color: String },
    /// 内发光 → `box-shadow: inset 0 0 blur color`
    InnerGlow { blur: f64, color: String },
    /// 高斯模糊 → `filter: blur(Npx)`
    GaussianBlur { radius: f64 },
    /// 圆角 → `border-radius`(仅盒对象;路径/文字拒绝并提示)
    RoundCorners { radius: f64 },
    /// 羽化 → `mask-image: radial-gradient(...)` 近似
    Feather { radius: f64 },
    /// 未识别段原样保真(手写 `filter: saturate(2)`、任意 mask 等)。
    /// 落盘 = 同属性原值;`blend` 同样冻结登记。
    Other { prop: String, value: String },
}

impl Effect {
    pub fn label(&self) -> &'static str {
        match self {
            Effect::DropShadow { .. } => "投影",
            Effect::InnerShadow { .. } => "内阴影",
            Effect::OuterGlow { .. } => "外发光",
            Effect::InnerGlow { .. } => "内发光",
            Effect::GaussianBlur { .. } => "高斯模糊",
            Effect::RoundCorners { .. } => "圆角",
            Effect::Feather { .. } => "羽化",
            Effect::Other { .. } => "自定义",
        }
    }

    /// 该效果归属的接管组(`Effect::Other` 归属其属性名本身)。
    fn own_tag(&self) -> String {
        match self {
            Effect::DropShadow { .. }
            | Effect::InnerShadow { .. }
            | Effect::OuterGlow { .. }
            | Effect::InnerGlow { .. } => OWN_SHADOW.to_string(),
            Effect::GaussianBlur { .. } => OWN_BLUR.to_string(),
            Effect::RoundCorners { .. } => OWN_RADIUS.to_string(),
            Effect::Feather { .. } => OWN_MASK.to_string(),
            Effect::Other { prop, .. } => format!("css:{prop}"),
        }
    }
}

/// 条目级混合模式候选(CSS `<blend-mode>` 16 项;填充落
/// `background-blend-mode` 逐层无损;描边/效果条目冻结登记)。
pub const BLEND_MODES: [&str; 16] = [
    "normal",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "color-dodge",
    "color-burn",
    "hard-light",
    "soft-light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
];

// ═══════════════════════ 2. 纯函数:效果 → CSS 片段(映射表) ═══════════════════════

/// 规范 px:`0` 不带单位(与 vb_css canonical_value 一致,保证 L1 幂等)。
fn px(v: f64) -> String {
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{}px", vb_common::units::fmt_num(v))
    }
}

/// 阴影类效果 → `box-shadow` 一个逗号段(启用条目按模型序拼接)。
pub fn shadow_segment(e: &Effect) -> Option<String> {
    match e {
        Effect::DropShadow {
            x,
            y,
            blur,
            spread,
            color,
        } => Some(format!(
            "{} {} {} {} {color}",
            px(*x),
            px(*y),
            px(*blur),
            px(*spread)
        )),
        Effect::InnerShadow {
            x,
            y,
            blur,
            spread,
            color,
        } => Some(format!(
            "inset {} {} {} {} {color}",
            px(*x),
            px(*y),
            px(*blur),
            px(*spread)
        )),
        Effect::OuterGlow { blur, color } => {
            Some(format!("{} {} {} {color}", px(0.0), px(0.0), px(*blur)))
        }
        Effect::InnerGlow { blur, color } => Some(format!(
            "inset {} {} {} {color}",
            px(0.0),
            px(0.0),
            px(*blur)
        )),
        _ => None,
    }
}

/// 阴影类效果 → `text-shadow` 段(无 inset/spread 语义;内阴影/内发光
/// 对文字无效,由构建器拒绝)。
pub fn text_shadow_segment(e: &Effect) -> Option<String> {
    match e {
        Effect::DropShadow {
            x, y, blur, color, ..
        } => Some(format!("{} {} {} {color}", px(*x), px(*y), px(*blur))),
        Effect::OuterGlow { blur, color } => {
            Some(format!("{} {} {} {color}", px(0.0), px(0.0), px(*blur)))
        }
        _ => None,
    }
}

/// 解析一个 box-shadow 逗号段(inset 前缀 + 2..4 个长度 + 颜色在首/尾)。
/// 只认领可完整表达的段;残段由调用方落 `Effect::Other` 保真。
pub fn parse_shadow_segment(seg: &str) -> Option<(bool, [f64; 4], String)> {
    let seg = seg.trim();
    let (inset, rest) = match seg.strip_prefix("inset") {
        Some(r) => (true, r.trim_start()),
        None => (false, seg),
    };
    let toks: Vec<String> = vb_css::split_top_level(rest, ' ')
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if toks.len() < 3 || toks.len() > 5 {
        return None;
    }
    // 颜色 token = 可被解析为颜色且位于首/尾
    let color_idx = if vb_common::color::parse_color(&toks[toks.len() - 1]).is_some() {
        toks.len() - 1
    } else if vb_common::color::parse_color(&toks[0]).is_some() {
        0
    } else {
        return None;
    };
    let color = toks[color_idx].clone();
    let lens: Vec<f64> = toks
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != color_idx)
        .map(|(_, t)| vb_common::units::parse_px(t))
        .collect::<Option<Vec<_>>>()?;
    if lens.len() < 2 || lens.len() > 4 {
        return None;
    }
    Some((
        inset,
        [
            lens[0],
            lens[1],
            *lens.get(2).unwrap_or(&0.0),
            *lens.get(3).unwrap_or(&0.0),
        ],
        color,
    ))
}

/// 分段 → 效果条目体(x=y=spread=0 归类为发光;否则阴影)。
fn shadow_effect_of(inset: bool, l: [f64; 4], color: String) -> Effect {
    let [x, y, blur, spread] = l;
    match (inset, x == 0.0 && y == 0.0 && spread == 0.0) {
        (false, true) => Effect::OuterGlow { blur, color },
        (true, true) => Effect::InnerGlow { blur, color },
        (false, false) => Effect::DropShadow {
            x,
            y,
            blur,
            spread,
            color,
        },
        (true, false) => Effect::InnerShadow {
            x,
            y,
            blur,
            spread,
            color,
        },
    }
}

/// 羽化的确定性模板(mask-image)。模板必须与 [`parse_feather`] 对称。
fn feather_template(radius: f64) -> String {
    format!(
        "radial-gradient(circle, #000 calc(100% - {}), transparent)",
        px(radius)
    )
}

fn parse_feather(v: &str) -> Option<f64> {
    let inner = v
        .strip_prefix("radial-gradient(circle, #000 calc(100% - ")?
        .strip_suffix("), transparent)")?;
    vb_common::units::parse_px(inner)
}

/// 纯色填充的渐变承载(像素恒等):`linear-gradient(c, c)`。
fn solid_as_layer(c: &str) -> String {
    format!("linear-gradient({c}, {c})")
}

/// 渐变层 → 纯色(同色双 stop;其余 None)。
fn layer_as_solid(layer: &str) -> Option<String> {
    let body = layer.strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let segs = vb_css::split_top_level(body, ',');
    if segs.len() == 2 && segs[0].trim() == segs[1].trim() {
        Some(segs[0].trim().to_string())
    } else {
        None
    }
}

fn is_gradient_layer(layer: &str) -> bool {
    layer.starts_with("linear-gradient(") || layer.starts_with("radial-gradient(")
}

// ═══════════════════════ 3. 目标类型与受管属性组 ═══════════════════════

/// 外观条目的落点目标(决定受管属性组)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceTarget {
    /// 盒/容器/图像:填充 → background 三件套;描边 → border/outline 系
    Box,
    /// 文字:填充 → color;描边 → -webkit-text-stroke;阴影 → text-shadow
    Text,
    /// 矢量路径:填充 → fill;描边 → stroke 系(SVG 表现属性)
    Vector,
    /// 冻结块:导出走原样片段,样式声明不参与渲染 → 面板整体禁用
    Frozen,
}

pub fn target_of(kind: &NodeKind) -> AppearanceTarget {
    match kind {
        NodeKind::Text { .. } => AppearanceTarget::Text,
        NodeKind::Vector { .. } => AppearanceTarget::Vector,
        NodeKind::Frozen { .. } => AppearanceTarget::Frozen,
        // Box / Image / Slice / 容器(Artboard/Layer/Group):CSS 盒语义
        _ => AppearanceTarget::Box,
    }
}

/// 盒对象判定(05-6-1:圆角仅对盒对象有效)。
pub fn accepts_round_corners(kind: &NodeKind) -> bool {
    target_of(kind) == AppearanceTarget::Box
}

/// 描边组受管属性(重编译前整组清除,防残留)。
const STROKE_PROPS_BOX: &[&str] = &[
    "border-width",
    "border-style",
    "border-color",
    "outline-width",
    "outline-style",
    "outline-color",
];
const STROKE_PROPS_VECTOR: &[&str] = &[
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
];
const STROKE_PROPS_TEXT: &[&str] = &[
    "-webkit-text-stroke",
    "-webkit-text-stroke-width",
    "-webkit-text-stroke-color",
];

fn stroke_props(t: AppearanceTarget) -> &'static [&'static str] {
    match t {
        AppearanceTarget::Box => STROKE_PROPS_BOX,
        AppearanceTarget::Vector => STROKE_PROPS_VECTOR,
        AppearanceTarget::Text => STROKE_PROPS_TEXT,
        AppearanceTarget::Frozen => &[],
    }
}

// ═══════════════════════ 4. 编译(模型 → style 声明) ═══════════════════════

/// 从声明列表移除一组属性。
fn remove_props(style: &mut Vec<Decl>, props: &[&str]) {
    style.retain(|d| !props.contains(&d.prop.as_str()));
}

fn set_prop(style: &mut Vec<Decl>, prop: &str, value: String) {
    if let Some(d) = style.iter_mut().find(|d| d.prop == prop) {
        d.value = value;
    } else {
        style.push(Decl {
            prop: prop.to_string(),
            value,
            important: false,
        });
    }
}

/// 编译:按模型把受管属性组重写进节点样式(未接管的手写声明不动)。
///
/// `prev_own` = 上一次的接管集(来自既有属性/解码);删除到 0 条也保持
/// 接管(清空 = 移除声明)。返回新样式。
pub fn compile_style(
    kind: &NodeKind,
    style: &[Decl],
    prev_own: &[String],
    items: &[AppearanceItem],
) -> Vec<Decl> {
    let t = target_of(kind);
    if t == AppearanceTarget::Frozen {
        return style.to_vec();
    }
    let mut own: Vec<String> = prev_own.to_vec();
    for it in items {
        let tag = item_own_tag(it);
        AppearanceModel::own_add(&mut own, &tag);
    }
    let mut st = style.to_vec();

    // ── 填充 ──
    if own.iter().any(|o| o == OWN_FILL) {
        let fills: Vec<&FillItem> = items
            .iter()
            .filter_map(|i| match i {
                AppearanceItem::Fill(f) => Some(f),
                _ => None,
            })
            .collect();
        let on: Vec<&&FillItem> = fills.iter().filter(|f| f.enabled).collect();
        match t {
            AppearanceTarget::Box => {
                remove_props(
                    &mut st,
                    &[
                        "background-color",
                        "background-image",
                        "background-blend-mode",
                    ],
                );
                // 最底层启用纯色 → background-color(单填充文档与既有行为一致)
                let bottom_solid = on.last().and_then(|f| match &f.body {
                    FillBody::Solid { value } => Some(value.clone()),
                    _ => None,
                });
                if let Some(c) = &bottom_solid {
                    set_prop(&mut st, "background-color", c.clone());
                }
                // 其余条目 → background-image 层(模型序 = 层序,首层最上)
                let mut layers: Vec<String> = Vec::new();
                let mut blends: Vec<String> = Vec::new();
                for (i, f) in on.iter().enumerate() {
                    let is_bottom_solid =
                        i == on.len() - 1 && matches!(f.body, FillBody::Solid { .. });
                    if is_bottom_solid {
                        continue;
                    }
                    layers.push(match &f.body {
                        FillBody::Solid { value } => solid_as_layer(value),
                        FillBody::Gradient { value } => value.clone(),
                        FillBody::Raw { value } => value.clone(),
                    });
                    blends.push(f.blend.clone().unwrap_or_else(|| "normal".into()));
                }
                if !layers.is_empty() {
                    set_prop(&mut st, "background-image", layers.join(", "));
                    if blends.iter().any(|b| b != "normal") {
                        set_prop(&mut st, "background-blend-mode", blends.join(", "));
                    }
                }
            }
            AppearanceTarget::Text => {
                remove_props(&mut st, &["color"]);
                // 文字:仅最底层纯色落 `color`;其余条目冻结登记(报告)
                if let Some(Some(f)) = on.last().map(|f| match &f.body {
                    FillBody::Solid { value } => Some(value.clone()),
                    _ => None,
                }) {
                    set_prop(&mut st, "color", f);
                }
            }
            AppearanceTarget::Vector => {
                remove_props(&mut st, &["fill"]);
                if let Some(Some(f)) = on.last().map(|f| match &f.body {
                    FillBody::Solid { value } => Some(value.clone()),
                    _ => None,
                }) {
                    set_prop(&mut st, "fill", f);
                }
            }
            AppearanceTarget::Frozen => {}
        }
    }

    // ── 描边 ──
    if own.iter().any(|o| o == OWN_STROKE) {
        let strokes: Vec<&StrokeItem> = items
            .iter()
            .filter_map(|i| match i {
                AppearanceItem::Stroke(s) => Some(s),
                _ => None,
            })
            .collect();
        remove_props(&mut st, stroke_props(t));
        if let Some(s) = strokes.iter().find(|s| s.enabled) {
            // 主描边 = 启用条目中的第 1 条;其余条目冻结登记(报告)
            let spec = &s.spec;
            match t {
                AppearanceTarget::Box => match spec.align {
                    StrokeAlign::Outside => {
                        set_prop(&mut st, "outline-width", px(spec.width));
                        set_prop(&mut st, "outline-style", dash_style(&spec.dash).into());
                        if let Some(c) = &spec.color {
                            set_prop(&mut st, "outline-color", c.clone());
                        }
                    }
                    _ => {
                        // 内侧/居中:border 恒内侧;居中为降级落盘(报告注记)
                        set_prop(&mut st, "border-width", px(spec.width));
                        set_prop(&mut st, "border-style", dash_style(&spec.dash).into());
                        if let Some(c) = &spec.color {
                            set_prop(&mut st, "border-color", c.clone());
                        }
                    }
                },
                AppearanceTarget::Vector => {
                    if let Some(c) = &spec.color {
                        set_prop(&mut st, "stroke", c.clone());
                    }
                    set_prop(&mut st, "stroke-width", px(spec.width));
                    set_prop(&mut st, "stroke-linecap", spec.cap.css().into());
                    set_prop(&mut st, "stroke-linejoin", spec.join.css().into());
                    if spec.join == StrokeJoin::Miter {
                        set_prop(
                            &mut st,
                            "stroke-miterlimit",
                            vb_common::units::fmt_num(spec.miter_limit),
                        );
                    }
                    if !spec.dash.is_empty() {
                        let v: Vec<String> = spec.dash.iter().map(|d| px(*d)).collect();
                        set_prop(&mut st, "stroke-dasharray", v.join(", "));
                    }
                    // 对齐:SVG 恒居中(Inside/Outside 冻结登记);箭头不落盘
                }
                AppearanceTarget::Text => {
                    set_prop(&mut st, "-webkit-text-stroke-width", px(spec.width));
                    if let Some(c) = &spec.color {
                        set_prop(&mut st, "-webkit-text-stroke-color", c.clone());
                    }
                }
                AppearanceTarget::Frozen => {}
            }
        }
    }

    // ── 效果(按受管属性收集启用段;模型序 = 同类段序) ──
    let effects: Vec<&EffectItem> = items
        .iter()
        .filter_map(|i| match i {
            AppearanceItem::Effect(e) => Some(e),
            _ => None,
        })
        .collect();
    if effects.is_empty() {
        return st;
    }
    // 触及集:模型中存在该类条目(含禁用)→ 该属性归编译管
    let mut touched: Vec<String> = Vec::new();
    for e in &effects {
        AppearanceModel::own_add(&mut touched, &e.effect.own_tag());
    }
    let shadow_prop = match t {
        AppearanceTarget::Text => "text-shadow",
        _ => "box-shadow",
    };
    let mut shadows: Vec<String> = Vec::new();
    let mut filters: Vec<String> = Vec::new();
    let mut radius: Option<f64> = None;
    let mut mask: Option<String> = None;
    let mut others: Vec<(String, String)> = Vec::new();
    for e in effects.iter().filter(|e| e.enabled) {
        let is_text = t == AppearanceTarget::Text;
        match &e.effect {
            Effect::DropShadow { .. } | Effect::OuterGlow { .. } => {
                // 文字 → text-shadow(无 inset/spread);其余 → box-shadow
                let seg = if is_text {
                    text_shadow_segment(&e.effect)
                } else {
                    shadow_segment(&e.effect)
                };
                if let Some(seg) = seg {
                    shadows.push(seg);
                }
            }
            Effect::InnerShadow { .. } | Effect::InnerGlow { .. } => {
                // 文字无内阴影/内发光落点(构建器已拒绝;编译兜底跳过,
                // 不静默产生非法 CSS)
                if is_text {
                    continue;
                }
                if let Some(seg) = shadow_segment(&e.effect) {
                    shadows.push(seg);
                }
            }
            Effect::GaussianBlur { radius } => filters.push(format!("blur({})", px(*radius))),
            Effect::RoundCorners { radius: r } => radius = Some(*r),
            Effect::Feather { radius } => mask = Some(feather_template(*radius)),
            Effect::Other { prop, value } => others.push((prop.clone(), value.clone())),
        }
    }
    let apply = |st: &mut Vec<Decl>, prop: &str, touched: bool, segs: &[String], join: &str| {
        if !touched {
            return;
        }
        remove_props(st, &[prop]);
        if !segs.is_empty() {
            set_prop(st, prop, segs.join(join));
        }
    };
    apply(
        &mut st,
        shadow_prop,
        touched.iter().any(|t| t == OWN_SHADOW),
        &shadows,
        ", ",
    );
    apply(
        &mut st,
        "filter",
        touched.iter().any(|t| t == OWN_BLUR),
        &filters,
        " ",
    );
    if touched.iter().any(|t| t == OWN_RADIUS) {
        remove_props(&mut st, &["border-radius"]);
        if let Some(r) = radius {
            set_prop(&mut st, "border-radius", px(r));
        }
    }
    if touched.iter().any(|t| t == OWN_MASK) {
        remove_props(&mut st, &["mask-image"]);
        if let Some(m) = mask {
            set_prop(&mut st, "mask-image", m);
        }
    }
    for (prop, value) in others {
        let tag = format!("css:{prop}");
        if touched.contains(&tag) {
            set_prop(&mut st, &prop, value);
        }
    }
    st
}

/// 虚线 → 盒对象 border-style 近似(有虚线 → dashed;CSS 无自定义 dash)。
fn dash_style(dash: &[f64]) -> &'static str {
    if dash.is_empty() {
        "solid"
    } else {
        "dashed"
    }
}

// ═══════════════════════ 5. 解码(CSS/属性 → 模型) ═══════════════════════

/// 解码外观模型:优先 `data-vb-appearance`(无损);无/损坏 → CSS 启发式
/// (只认领可完整表达的声明;其余声明保持原样不受管)。
pub fn decode_model(
    kind: &NodeKind,
    style: &[Decl],
    attrs: &[(String, String)],
) -> AppearanceModel {
    if let Some(json) = attrs
        .iter()
        .find(|(k, _)| k == APPEARANCE_ATTR)
        .map(|(_, v)| v)
    {
        if let Ok(m) = serde_json::from_str::<AppearanceModel>(json) {
            if m.v == 1 {
                return m;
            }
        }
    }
    fallback_decode(kind, style)
}

fn get<'a>(style: &'a [Decl], prop: &str) -> Option<&'a str> {
    style
        .iter()
        .find(|d| d.prop == prop)
        .map(|d| d.value.as_str())
}

fn fallback_decode(kind: &NodeKind, style: &[Decl]) -> AppearanceModel {
    let t = target_of(kind);
    let mut m = AppearanceModel::empty();
    // ── 填充 ──
    match t {
        AppearanceTarget::Box => {
            let layers: Vec<String> = get(style, "background-image")
                .map(|v| {
                    vb_css::split_top_level(v, ',')
                        .iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            for l in &layers {
                let body = if let Some(c) = layer_as_solid(l) {
                    FillBody::Solid { value: c }
                } else if is_gradient_layer(l) {
                    FillBody::Gradient { value: l.clone() }
                } else {
                    FillBody::Raw { value: l.clone() }
                };
                m.items.push(AppearanceItem::Fill(FillItem {
                    enabled: true,
                    blend: None,
                    body,
                }));
            }
            if let Some(c) = get(style, "background-color") {
                m.items.push(AppearanceItem::Fill(FillItem {
                    enabled: true,
                    blend: None,
                    body: FillBody::Solid {
                        value: c.to_string(),
                    },
                }));
            }
            // 逐层混合(background-blend-mode 第 i 项 ↔ 第 i 填充;
            // 编码端 blend 列表只覆盖 image 层,故严格按位取,不做 repeat 兜底)
            if let Some(bm) = get(style, "background-blend-mode") {
                let list: Vec<String> = vb_css::split_top_level(bm, ',')
                    .iter()
                    .map(|s| s.trim().to_string())
                    .collect();
                for (i, it) in m.items.iter_mut().enumerate() {
                    if let AppearanceItem::Fill(f) = it {
                        if let Some(b) = list.get(i) {
                            if b != "normal" {
                                f.blend = Some(b.clone());
                            }
                        }
                    }
                }
            }
            if !layers.is_empty() || get(style, "background-color").is_some() {
                m.own.push(OWN_FILL.into());
            }
        }
        AppearanceTarget::Text => {
            if let Some(c) = get(style, "color") {
                m.items.push(AppearanceItem::Fill(FillItem {
                    enabled: true,
                    blend: None,
                    body: FillBody::Solid {
                        value: c.to_string(),
                    },
                }));
                m.own.push(OWN_FILL.into());
            }
        }
        AppearanceTarget::Vector => {
            if let Some(c) = get(style, "fill") {
                m.items.push(AppearanceItem::Fill(FillItem {
                    enabled: true,
                    blend: None,
                    body: FillBody::Solid {
                        value: c.to_string(),
                    },
                }));
                m.own.push(OWN_FILL.into());
            }
        }
        AppearanceTarget::Frozen => {}
    }
    // ── 描边(只认领可完整表达的组合) ──
    match t {
        AppearanceTarget::Box => {
            if let Some(spec) = decode_box_stroke(style, StrokeAlign::Inside) {
                m.items.push(AppearanceItem::Stroke(StrokeItem {
                    enabled: true,
                    blend: None,
                    spec,
                }));
                m.own.push(OWN_STROKE.into());
            } else if let Some(spec) = decode_box_stroke(style, StrokeAlign::Outside) {
                m.items.push(AppearanceItem::Stroke(StrokeItem {
                    enabled: true,
                    blend: None,
                    spec,
                }));
                m.own.push(OWN_STROKE.into());
            }
        }
        AppearanceTarget::Vector => {
            if get(style, "stroke").is_some() || get(style, "stroke-width").is_some() {
                m.items.push(AppearanceItem::Stroke(StrokeItem {
                    enabled: true,
                    blend: None,
                    spec: decode_vector_stroke(style),
                }));
                m.own.push(OWN_STROKE.into());
            }
        }
        AppearanceTarget::Text => {
            if let Some(w) =
                get(style, "-webkit-text-stroke-width").and_then(vb_common::units::parse_px)
            {
                m.items.push(AppearanceItem::Stroke(StrokeItem {
                    enabled: true,
                    blend: None,
                    spec: StrokeSpec {
                        width: w,
                        color: get(style, "-webkit-text-stroke-color").map(str::to_string),
                        ..StrokeSpec::default()
                    },
                }));
                m.own.push(OWN_STROKE.into());
            } else if let Some(v) = get(style, "-webkit-text-stroke") {
                // 简写 "Npx C"
                let toks: Vec<&str> = v.split_whitespace().collect();
                if let (Some(w), c) = (
                    toks.first().and_then(|t| vb_common::units::parse_px(t)),
                    toks.get(1).copied(),
                ) {
                    m.items.push(AppearanceItem::Stroke(StrokeItem {
                        enabled: true,
                        blend: None,
                        spec: StrokeSpec {
                            width: w,
                            color: c.map(str::to_string),
                            ..StrokeSpec::default()
                        },
                    }));
                    m.own.push(OWN_STROKE.into());
                }
            }
        }
        AppearanceTarget::Frozen => {}
    }
    // ── 效果 ──
    let shadow_prop = match t {
        AppearanceTarget::Text => "text-shadow",
        _ => "box-shadow",
    };
    let push_effect = |m: &mut AppearanceModel, e: Effect| {
        let tag = e.own_tag();
        m.items.push(AppearanceItem::Effect(EffectItem {
            enabled: true,
            blend: None,
            effect: e,
        }));
        AppearanceModel::own_add(&mut m.own, &tag);
    };
    if let Some(v) = get(style, shadow_prop) {
        for seg in vb_css::split_top_level(v, ',') {
            let seg = seg.trim();
            if seg.is_empty() {
                continue;
            }
            match parse_shadow_segment(seg) {
                Some((inset, lens, color)) => {
                    if t == AppearanceTarget::Text {
                        // text-shadow 无 inset/spread;按投影/外发光归类
                        let blur = lens[2];
                        push_effect(
                            &mut m,
                            if lens[0] == 0.0 && lens[1] == 0.0 {
                                Effect::OuterGlow { blur, color }
                            } else {
                                Effect::DropShadow {
                                    x: lens[0],
                                    y: lens[1],
                                    blur,
                                    spread: 0.0,
                                    color,
                                }
                            },
                        );
                    } else {
                        push_effect(&mut m, shadow_effect_of(inset, lens, color));
                    }
                }
                None => push_effect(
                    &mut m,
                    Effect::Other {
                        prop: shadow_prop.to_string(),
                        value: seg.to_string(),
                    },
                ),
            }
        }
    }
    // 文字对象上的手写 box-shadow:整体保真为 Other(不被面板接管)
    if t == AppearanceTarget::Text {
        if let Some(v) = get(style, "box-shadow") {
            push_effect(
                &mut m,
                Effect::Other {
                    prop: "box-shadow".into(),
                    value: v.to_string(),
                },
            );
        }
    }
    if let Some(v) = get(style, "filter") {
        for f in vb_css::split_top_level(v, ' ') {
            let f = f.trim();
            if f.is_empty() {
                continue;
            }
            if let Some(r) = f
                .strip_prefix("blur(")
                .and_then(|x| x.strip_suffix(')'))
                .and_then(vb_common::units::parse_px)
            {
                push_effect(&mut m, Effect::GaussianBlur { radius: r });
            } else {
                push_effect(
                    &mut m,
                    Effect::Other {
                        prop: "filter".into(),
                        value: f.to_string(),
                    },
                );
            }
        }
    }
    if let Some(v) = get(style, "border-radius") {
        if let Some(r) = vb_common::units::parse_px(v) {
            push_effect(&mut m, Effect::RoundCorners { radius: r });
        } else {
            push_effect(
                &mut m,
                Effect::Other {
                    prop: "border-radius".into(),
                    value: v.to_string(),
                },
            );
        }
    }
    if let Some(v) = get(style, "mask-image") {
        match parse_feather(v) {
            Some(r) => push_effect(&mut m, Effect::Feather { radius: r }),
            None => push_effect(
                &mut m,
                Effect::Other {
                    prop: "mask-image".into(),
                    value: v.to_string(),
                },
            ),
        }
    }
    m
}

/// 解码盒对象描边(align 指定读 border 系还是 outline 系);
/// 四边值不同等不可表达情形返回 None(不认领,声明原样保留)。
fn decode_box_stroke(style: &[Decl], align: StrokeAlign) -> Option<StrokeSpec> {
    let (wp, sp, cp) = match align {
        StrokeAlign::Outside => ("outline-width", "outline-style", "outline-color"),
        _ => ("border-width", "border-style", "border-color"),
    };
    let w = get(style, wp).and_then(vb_common::units::parse_px)?;
    let s = get(style, sp)?;
    // 四边不同(width "2px 4px")不可表达 → 不认领
    if s.split_whitespace().count() > 1 || s == "none" {
        return None;
    }
    Some(StrokeSpec {
        width: w,
        color: get(style, cp).map(str::to_string),
        dash: if s == "dashed" || s == "dotted" {
            vec![6.0, 3.0]
        } else {
            Vec::new()
        },
        align,
        ..StrokeSpec::default()
    })
}

fn decode_vector_stroke(style: &[Decl]) -> StrokeSpec {
    StrokeSpec {
        width: get(style, "stroke-width")
            .and_then(vb_common::units::parse_px)
            .unwrap_or(1.0),
        color: get(style, "stroke").map(str::to_string),
        cap: get(style, "stroke-linecap")
            .map(StrokeCap::from_css)
            .unwrap_or_default(),
        join: get(style, "stroke-linejoin")
            .map(StrokeJoin::from_css)
            .unwrap_or_default(),
        miter_limit: get(style, "stroke-miterlimit")
            .and_then(|v| v.parse().ok())
            .unwrap_or(4.0),
        dash: get(style, "stroke-dasharray")
            .map(|v| {
                vb_css::split_top_level(v, ',')
                    .iter()
                    .filter_map(|s| vb_common::units::parse_px(s.trim()))
                    .collect()
            })
            .unwrap_or_default(),
        align: StrokeAlign::Center,
        arrow_start: Arrowhead::default(),
        arrow_end: Arrowhead::default(),
    }
}

/// 序列化模型(紧凑 JSON;落 `data-vb-appearance`)。
pub fn encode_model(m: &AppearanceModel) -> String {
    serde_json::to_string(m).unwrap_or_else(|_| "{\"v\":1,\"own\":[],\"items\":[]}".to_string())
}

// ═══════════════════════ 6. 命令构建器(门禁测试走这里) ═══════════════════════

/// 构建器错误 = 用户可见提示文案(UI 直接 toast;构建器测试断言文案)。
pub type AppearanceResult = Result<Option<Command>, String>;

/// 条目的接管组标记。
fn item_own_tag(it: &AppearanceItem) -> String {
    match it {
        AppearanceItem::Fill(_) => OWN_FILL.to_string(),
        AppearanceItem::Stroke(_) => OWN_STROKE.to_string(),
        AppearanceItem::Effect(e) => e.effect.own_tag(),
    }
}

/// 条目写回单命令:`Compound[SetStyle(重编译), SetAttrs(模型)]`。
/// `m` 为**新**模型(own 已含 prev 的接管集;删到 0 条也保持接管)。
pub fn appearance_write_cmd(doc: &Document, sid: &str, m: AppearanceModel) -> AppearanceResult {
    let nid = doc
        .find_by_sid(sid)
        .ok_or_else(|| format!("对象不存在:{sid}"))?;
    let n = doc.node(nid).ok_or_else(|| format!("对象不存在:{sid}"))?;
    let mut own = m.own.clone();
    for it in &m.items {
        let tag = item_own_tag(it);
        AppearanceModel::own_add(&mut own, &tag);
    }
    let style = compile_style(&n.kind, &n.style, &own, &m.items);
    let mut attrs = n.attrs.clone();
    attrs.insert(APPEARANCE_ATTR.to_string(), encode_model(&m));
    let attr_list: Vec<(String, String)> = attrs.into_iter().collect();
    Ok(Some(Command::Compound {
        cmds: vec![
            Command::SetStyle {
                sid: sid.to_string(),
                new: style,
                old: None,
            },
            Command::SetAttrs {
                sid: sid.to_string(),
                new: attr_list,
                old: None,
            },
        ],
    }))
}

fn load_model(doc: &Document, sid: &str) -> Option<(NodeId, AppearanceModel)> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.node(nid)?;
    let attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Some((nid, decode_model(&n.kind, &n.style, &attrs)))
}

fn validate_effect(kind: &NodeKind, e: &Effect) -> Result<(), String> {
    match e {
        Effect::RoundCorners { .. }
            if !accepts_round_corners(kind) && target_of(kind) != AppearanceTarget::Frozen =>
        {
            Err("圆角仅对盒对象有效(路径/文字对象不支持)".into())
        }
        Effect::RoundCorners { .. } if target_of(kind) == AppearanceTarget::Frozen => {
            Err("冻结块内部不可编辑(样式作用于原样保留的 HTML 片段,无渲染落点)".into())
        }
        Effect::InnerShadow { .. } | Effect::InnerGlow { .. }
            if target_of(kind) == AppearanceTarget::Text =>
        {
            Err("文字不支持内阴影/内发光(CSS text-shadow 无 inset 语义)".into())
        }
        _ => Ok(()),
    }
}

// ── 填充 ──

/// 添加填充(默认半透明灰,入栈可撤销)。
pub fn add_fill_cmd(doc: &Document, sid: &str, body: FillBody) -> AppearanceResult {
    let (nid, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    let kind = doc.node(nid).unwrap().kind.clone();
    match (&body, target_of(&kind)) {
        (FillBody::Gradient { .. }, AppearanceTarget::Vector) => {
            return Err("矢量路径的渐变填充暂不支持(SVG defs 未建模,计划 v2);可先用纯色".into())
        }
        (FillBody::Gradient { .. }, AppearanceTarget::Text) => {
            return Err("文字渐变填充暂不支持(计划 v2);可先用纯色".into())
        }
        _ => {}
    }
    // AI 语义:「+ 填充」新条目在最上层(index 0 = 最上,CSS background-image 首层)
    m.items.insert(
        0,
        AppearanceItem::Fill(FillItem {
            enabled: true,
            blend: None,
            body,
        }),
    );
    appearance_write_cmd(doc, sid, m)
}

/// 编辑第 `index` 条填充。
pub fn set_fill_body_cmd(
    doc: &Document,
    sid: &str,
    index: usize,
    body: FillBody,
) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    if let AppearanceItem::Fill(f) = &mut m.items[index] {
        f.body = body;
    } else {
        return Ok(None);
    }
    appearance_write_cmd(doc, sid, m)
}

// ── 描边 ──

/// 为任意对象添加描边(05-3-3 通用入口;默认 1px 黑内侧)。
pub fn add_stroke_cmd(doc: &Document, sid: &str) -> AppearanceResult {
    let (_nid, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    // AI 语义:新描边条目在最上层
    m.items.insert(
        0,
        AppearanceItem::Stroke(StrokeItem {
            enabled: true,
            blend: None,
            spec: StrokeSpec {
                color: Some("#1a1a1a".into()), // vb-token-ok: 默认描边色(文档内容,非 UI 皮肤)
                ..StrokeSpec::default()
            },
        }),
    );
    appearance_write_cmd(doc, sid, m)
}

/// 编辑第 `index` 条描边(描边面板全字段走这里)。
pub fn set_stroke_spec_cmd(
    doc: &Document,
    sid: &str,
    index: usize,
    spec: StrokeSpec,
) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if spec.dash.len() > 6 {
        return Err("虚线最多 6 组(3 对值/间隙)".into());
    }
    if index >= m.items.len() {
        return Ok(None);
    }
    if let AppearanceItem::Stroke(s) = &mut m.items[index] {
        s.spec = spec;
    } else {
        return Ok(None);
    }
    appearance_write_cmd(doc, sid, m)
}

// ── 效果 ──

/// 添加效果(六种映射 + 羽化;圆角/内阴影对非支持目标的拒绝含提示)。
pub fn add_effect_cmd(doc: &Document, sid: &str, effect: Effect) -> AppearanceResult {
    let (nid, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    let kind = doc.node(nid).unwrap().kind.clone();
    validate_effect(&kind, &effect)?;
    // AI 语义:新效果条目在最上层
    m.items.insert(
        0,
        AppearanceItem::Effect(EffectItem {
            enabled: true,
            blend: None,
            effect,
        }),
    );
    appearance_write_cmd(doc, sid, m)
}

/// 编辑第 `index` 条效果(参数数值化;校验同添加)。
pub fn set_effect_cmd(doc: &Document, sid: &str, index: usize, effect: Effect) -> AppearanceResult {
    let (nid, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    let kind = doc.node(nid).unwrap().kind.clone();
    validate_effect(&kind, &effect)?;
    if index >= m.items.len() {
        return Ok(None);
    }
    if let AppearanceItem::Effect(e) = &mut m.items[index] {
        e.effect = effect;
    } else {
        return Ok(None);
    }
    appearance_write_cmd(doc, sid, m)
}

// ── 条目操作(排序 / 禁用 / 复制 / 删除 / 混合模式) ──

/// 上移/下移(05-1-1 条目顺序 = CSS 叠加顺序;dir -1 = 上移)。
pub fn move_item_cmd(doc: &Document, sid: &str, index: usize, dir: i32) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    let to = index as i32 + dir;
    if index >= m.items.len() || to < 0 || to as usize >= m.items.len() {
        return Ok(None);
    }
    m.items.swap(index, to as usize);
    appearance_write_cmd(doc, sid, m)
}

/// 眼睛开关(临时禁用 = 从编译产物移除该条,模型保留)。
pub fn toggle_item_cmd(doc: &Document, sid: &str, index: usize, enabled: bool) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    m.items[index].set_enabled(enabled);
    appearance_write_cmd(doc, sid, m)
}

/// 复制条目(插到原条目上方;AI 行为)。
pub fn duplicate_item_cmd(doc: &Document, sid: &str, index: usize) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    let copy = m.items[index].clone();
    m.items.insert(index, copy);
    appearance_write_cmd(doc, sid, m)
}

/// 删除条目(删到 0 条保持接管:清空 = 移除受管声明,不静默残留)。
pub fn remove_item_cmd(doc: &Document, sid: &str, index: usize) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    m.items.remove(index);
    appearance_write_cmd(doc, sid, m)
}

/// 条目混合模式(填充 → background-blend-mode 逐层;描边/效果 → 冻结登记)。
pub fn set_item_blend_cmd(
    doc: &Document,
    sid: &str,
    index: usize,
    blend: Option<String>,
) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    m.items[index].set_blend(blend);
    appearance_write_cmd(doc, sid, m)
}

/// 只改描边粗细(外观面板条目编辑器快调用;避免整份 spec 来回克隆的
/// 取值竞态:粗细数值框每帧回读模型)。
pub fn update_stroke_width(
    doc: &Document,
    sid: &str,
    index: usize,
    width: f64,
) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    if let AppearanceItem::Stroke(s) = &mut m.items[index] {
        s.spec.width = width;
    } else {
        return Ok(None);
    }
    appearance_write_cmd(doc, sid, m)
}

// ═══════════════════════ 7. 不支持处理(05-6;design/06 §七) ═══════════════════════

/// 明确不支持的能力:点击给提示而非沉默(design/06 §七)。
/// 文案与 design/06 §七 一致,并按 05-6-2 补「计划于 vX」。
pub struct UnsupportedFeature {
    pub id: &'static str,
    pub label: &'static str,
    pub message: &'static str,
}

pub const UNSUPPORTED: &[UnsupportedFeature] = &[
    UnsupportedFeature {
        id: "live_paint",
        label: "实时上色",
        message: "实时上色在 v1 不支持;可用路径查找器或形状生成器代替",
    },
    UnsupportedFeature {
        id: "mesh_gradient",
        label: "渐变网格",
        message: "渐变网格无 HTML 对应;建议用多层径向渐变叠加模拟(计划于 v2 支持)",
    },
    UnsupportedFeature {
        id: "image_trace",
        label: "图像描摹",
        message: "图像描摹暂不支持(计划于 v2 支持)",
    },
    UnsupportedFeature {
        id: "perspective_3d",
        label: "3D / 透视",
        message: "3D 与透视网格不支持",
    },
    UnsupportedFeature {
        id: "symbols",
        label: "符号",
        message: "符号将于 v2 以『组件』形式提供",
    },
    UnsupportedFeature {
        id: "variables",
        label: "变量",
        message: "变量已以『设计令牌』提供:右侧面板坞 · 令牌 Tab(F4 切换)",
    },
];

/// 按 id 查不支持提示文案(门禁测试 + UI 共用)。
pub fn unsupported_message(id: &str) -> Option<&'static str> {
    UNSUPPORTED.iter().find(|f| f.id == id).map(|f| f.message)
}

// ═══════════════════════ 9. 单元测试(编解码/映射/解析) ═══════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(v: &str) -> FillBody {
        FillBody::Solid {
            value: v.to_string(),
        }
    }

    fn fill(v: &str) -> AppearanceItem {
        AppearanceItem::Fill(FillItem {
            enabled: true,
            blend: None,
            body: solid(v),
        })
    }

    /// 效果映射表(design/06 §4.6):六种效果的确定性 CSS 片段。
    #[test]
    fn effect_mapping_table_produces_exact_css() {
        let cases: Vec<(Effect, &str)> = vec![
            (
                Effect::DropShadow {
                    x: 8.0,
                    y: 12.0,
                    blur: 24.0,
                    spread: 4.0,
                    color: "#00000059".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                },
                "8px 12px 24px 4px #00000059",
            ),
            (
                Effect::InnerShadow {
                    x: 0.0,
                    y: 2.0,
                    blur: 6.0,
                    spread: 0.0,
                    color: "#00000066".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                },
                "inset 0 2px 6px 0 #00000066",
            ),
            (
                Effect::OuterGlow {
                    blur: 12.0,
                    color: "#2e86ff80".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                },
                "0 0 12px #2e86ff80",
            ),
            (
                Effect::InnerGlow {
                    blur: 10.0,
                    color: "#ffffff40".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                },
                "inset 0 0 10px #ffffff40",
            ),
        ];
        for (e, seg) in cases {
            assert_eq!(shadow_segment(&e).unwrap(), seg, "{:?}", e.label());
        }
        assert_eq!(Effect::GaussianBlur { radius: 4.0 }.own_tag(), OWN_BLUR,);
        // 模糊与羽化的完整声明由 compile 生成(见 multi_effects_compile_in_order)
    }

    /// 多条效果编译:同类按模型序拼接(box-shadow 逗号 / filter 空格),
    /// 顺序 = 条目顺序(验收 05-1-4/门 2)。
    #[test]
    fn multi_effects_compile_in_order() {
        let items = vec![
            AppearanceItem::Effect(EffectItem {
                enabled: true,
                blend: None,
                effect: Effect::DropShadow {
                    x: 2.0,
                    y: 2.0,
                    blur: 4.0,
                    spread: 0.0,
                    color: "#00000080".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                },
            }),
            AppearanceItem::Effect(EffectItem {
                enabled: true,
                blend: None,
                effect: Effect::GaussianBlur { radius: 3.0 },
            }),
            AppearanceItem::Effect(EffectItem {
                enabled: true,
                blend: None,
                effect: Effect::InnerGlow {
                    blur: 8.0,
                    color: "#ffffff40".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                },
            }),
        ];
        let st = compile_style(&NodeKind::Box, &[], &[], &items);
        let get = |p: &str| get(&st, p).unwrap().to_string();
        assert_eq!(
            get("box-shadow"),
            "2px 2px 4px 0 #00000080, inset 0 0 8px #ffffff40",
            "阴影段序 = 条目序"
        );
        assert_eq!(get("filter"), "blur(3px)");
        assert_eq!(get("filter"), "blur(3px)");
    }

    /// 多填充编译:纯色底条 → background-color,其余 → background-image
    /// 层(纯色以同色双 stop 渐变承载,像素恒等);层序 = 条目序。
    #[test]
    fn multi_fill_compile_layer_order() {
        let items = vec![
            fill("#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            AppearanceItem::Fill(FillItem {
                enabled: true,
                blend: None,
                body: FillBody::Gradient {
                    value: "linear-gradient(90deg, #204060 0%, #90c0f0 100%)".into(),
                },
            }),
            fill("#102030"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        ];
        let st = compile_style(&NodeKind::Box, &[], &[], &items);
        assert_eq!(
            get(&st, "background-color").unwrap(),
            "#102030", // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            "最底层纯色写 background-color"
        );
        assert_eq!(
            get(&st, "background-image").unwrap(),
            "linear-gradient(#ff0000, #ff0000), linear-gradient(90deg, #204060 0%, #90c0f0 100%)",
            "层序 = 条目序(首层最上)"
        );
        // 解码回模型:渐变层还原为渐变条目,background-color 还原为底层纯色
        let m = fallback_decode(&NodeKind::Box, &st);
        assert_eq!(m.items.len(), 3);
        assert_eq!(m.items[0].summary(), "填充 #ff0000");
        assert!(matches!(
            &m.items[1],
            AppearanceItem::Fill(FillItem {
                body: FillBody::Gradient { .. },
                ..
            })
        ));
        assert_eq!(m.items[2].summary(), "填充 #102030");
        // 编码 ∘ 解码恒等(无损往返的最小单元)
        let st2 = compile_style(&NodeKind::Box, &[], &m.own, &m.items);
        assert_eq!(st, st2, "decode∘encode 必须恒等");
    }

    /// 禁用条目不进编译产物但保留在模型(眼睛 = 临时禁用,05-1-1)。
    #[test]
    fn disabled_items_are_compiled_out_but_kept_in_model() {
        let mut items = vec![fill("#ff0000"), fill("#00ff00")]; // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        items[1].set_enabled(false);
        let st = compile_style(&NodeKind::Box, &[], &[], &items);
        assert_eq!(get(&st, "background-color").unwrap(), "#ff0000"); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        assert!(get(&st, "background-image").is_none(), "禁用层不落盘");
        // 全部禁用 + 接管 → background-color 移除(清空 = 无填充)
        for it in &mut items {
            it.set_enabled(false);
        }
        let st = compile_style(&NodeKind::Box, &[], &["fill".to_string()], &items);
        assert!(get(&st, "background-color").is_none());
    }

    /// 条目级混合模式:填充 → background-blend-mode 逐层,往返保真。
    #[test]
    fn fill_blend_roundtrips_via_background_blend_mode() {
        let items = vec![
            AppearanceItem::Fill(FillItem {
                enabled: true,
                blend: Some("multiply".into()),
                body: solid("#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            }),
            AppearanceItem::Fill(FillItem {
                enabled: true,
                blend: None,
                body: solid("#102030"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            }),
        ];
        let st = compile_style(&NodeKind::Box, &[], &[], &items);
        assert_eq!(
            get(&st, "background-blend-mode").unwrap(),
            "multiply",
            "blend 列表只覆盖 background-image 层(底层纯色 excluded)"
        );
        let m = fallback_decode(&NodeKind::Box, &st);
        assert_eq!(m.items[0].blend().unwrap(), "multiply");
        assert!(m.items[1].blend().is_none());
        let st2 = compile_style(&NodeKind::Box, &[], &m.own, &m.items);
        assert_eq!(st, st2, "blend 往返恒等");
    }

    /// 阴影段解析:inset/颜色前置/发光归类;残段不认领(调用方落 Other)。
    #[test]
    fn shadow_segment_parse_roundtrip() {
        let (inset, lens, color) = parse_shadow_segment("inset 0 2px 6px 0 #00000066").unwrap();
        assert!(inset);
        assert_eq!(lens, [0.0, 2.0, 6.0, 0.0]);
        assert_eq!(color, "#00000066"); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                                        // 颜色前置(CSS 允许;rgba() 原样保留,canonical 不转 hex)
        let (_, lens, color) = parse_shadow_segment("rgba(0, 0, 0, 0.4) 1px 2px").unwrap();
        assert_eq!(lens, [1.0, 2.0, 0.0, 0.0]);
        assert_eq!(color, "rgba(0, 0, 0, 0.4)");
        // 手写 CSS → 解码归类:x=y=spread=0 → 发光
        let e = shadow_effect_of(false, [0.0, 0.0, 8.0, 0.0], "#ff0".into()); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        assert!(matches!(e, Effect::OuterGlow { .. }));
        let e = shadow_effect_of(true, [3.0, 3.0, 5.0, 1.0], "#000".into()); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        assert!(matches!(e, Effect::InnerShadow { .. }));
        // 残段:长度不足 → None
        assert!(parse_shadow_segment("1px").is_none());
    }

    /// 羽化模板 ⇄ 解析对称;手写 mask 不匹配模板 → Other 保真。
    #[test]
    fn feather_template_roundtrip() {
        let v = feather_template(12.0);
        assert_eq!(parse_feather(&v), Some(12.0));
        assert_eq!(
            v,
            "radial-gradient(circle, #000 calc(100% - 12px), transparent)"
        );
        assert_eq!(
            parse_feather("radial-gradient(circle at 30% 30%, #000, transparent)"),
            None,
            "非模板 mask 不认领"
        );
    }

    /// CSS 启发式解码:border/outline/stroke/-webkit-text-stroke 四落点。
    #[test]
    fn fallback_decode_claims_strokes_by_target() {
        let mk = |decls: &[(&str, &str)]| -> Vec<Decl> {
            decls
                .iter()
                .map(|(p, v)| Decl {
                    prop: p.to_string(),
                    value: v.to_string(),
                    important: false,
                })
                .collect()
        };
        // 盒:border → 内侧描边条目
        let st = mk(&[
            ("border-width", "2px"),
            ("border-style", "solid"),
            ("border-color", "#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        ]);
        let m = fallback_decode(&NodeKind::Box, &st);
        assert!(
            matches!(&m.items[0], AppearanceItem::Stroke(s) if s.spec.align == StrokeAlign::Inside && s.spec.width == 2.0)
        );
        // 盒:outline → 外侧描边条目
        let st = mk(&[
            ("outline-width", "3px"),
            ("outline-style", "solid"),
            ("outline-color", "#00ff00"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        ]);
        let m = fallback_decode(&NodeKind::Box, &st);
        assert!(
            matches!(&m.items[0], AppearanceItem::Stroke(s) if s.spec.align == StrokeAlign::Outside)
        );
        // 矢量:stroke 系全字段
        let st = mk(&[
            ("stroke", "#1a1a1a"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            ("stroke-width", "1.5px"),
            ("stroke-linecap", "round"),
            ("stroke-linejoin", "bevel"),
            ("stroke-miterlimit", "6"),
            ("stroke-dasharray", "6px, 3px"),
        ]);
        let m = fallback_decode(&NodeKind::Vector { path: kurbo_stub() }, &st);
        match &m.items[0] {
            AppearanceItem::Stroke(s) => {
                assert_eq!(s.spec.width, 1.5);
                assert_eq!(s.spec.cap, StrokeCap::Round);
                assert_eq!(s.spec.join, StrokeJoin::Bevel);
                assert_eq!(s.spec.miter_limit, 6.0);
                assert_eq!(s.spec.dash, vec![6.0, 3.0]);
            }
            other => panic!("应为描边条目:{other:?}"),
        }
        // 文字:-webkit-text-stroke-width/color
        let st = mk(&[
            ("-webkit-text-stroke-width", "2px"),
            ("-webkit-text-stroke-color", "#3366ff"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        ]);
        let m = fallback_decode(
            &NodeKind::Text {
                text: String::new(),
                mode: Default::default(),
                segments: Vec::new(),
            },
            &st,
        );
        match &m.items[0] {
            AppearanceItem::Stroke(s) => {
                assert_eq!(s.spec.width, 2.0);
                assert_eq!(s.spec.color.as_deref(), Some("#3366ff")); // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            }
            other => panic!("应为文字描边条目:{other:?}"),
        }
    }

    /// 文字效果:阴影落 text-shadow 段;内阴影被构建器拒绝(05-6-1)。
    #[test]
    fn text_effects_use_text_shadow_and_inner_is_rejected() {
        let text_kind = NodeKind::Text {
            text: String::new(),
            mode: Default::default(),
            segments: Vec::new(),
        };
        let items = vec![AppearanceItem::Effect(EffectItem {
            enabled: true,
            blend: None,
            effect: Effect::DropShadow {
                x: 2.0,
                y: 2.0,
                blur: 3.0,
                spread: 0.0,
                color: "#00000080".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
            },
        })];
        let st = compile_style(&text_kind, &[], &[], &items);
        assert_eq!(get(&st, "text-shadow").unwrap(), "2px 2px 3px #00000080");
        assert!(get(&st, "box-shadow").is_none(), "文字不得落 box-shadow");
    }

    /// 模型 JSON 序列化往返(v/own/items 全保真)。
    #[test]
    fn model_json_roundtrip() {
        let m = AppearanceModel {
            v: 1,
            own: vec![OWN_FILL.into(), OWN_SHADOW.into()],
            items: vec![
                fill("#ff0000"), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
                AppearanceItem::Effect(EffectItem {
                    enabled: false,
                    blend: Some("screen".into()),
                    effect: Effect::RoundCorners { radius: 8.0 },
                }),
            ],
        };
        let json = encode_model(&m);
        let m2: AppearanceModel = serde_json::from_str(&json).unwrap();
        assert_eq!(m, m2);
        assert!(json.contains("\"kind\":\"fill\""), "紧凑 JSON:{json}");
    }

    /// 不支持清单:六项齐备、文案非空、按 id 可查(design/06 §七)。
    #[test]
    fn unsupported_registry_covers_design06() {
        let ids: Vec<&str> = UNSUPPORTED.iter().map(|f| f.id).collect();
        assert_eq!(
            ids,
            vec![
                "live_paint",
                "mesh_gradient",
                "image_trace",
                "perspective_3d",
                "symbols",
                "variables",
            ]
        );
        for f in UNSUPPORTED {
            assert!(!f.label.is_empty());
            assert_eq!(
                unsupported_message(f.id),
                Some(f.message),
                "{} 应可按 id 查到提示",
                f.id
            );
            assert!(f.message.chars().count() >= 8, "{} 文案过短", f.id);
        }
        assert!(unsupported_message("no_such").is_none());
    }

    /// 校验器:圆角对路径/文字拒绝并给提示;内阴影对文字拒绝(05-6-1)。
    #[test]
    fn effect_validation_rejects_with_messages() {
        let vector = NodeKind::Vector { path: kurbo_stub() };
        let text = NodeKind::Text {
            text: String::new(),
            mode: Default::default(),
            segments: Vec::new(),
        };
        let rc = Effect::RoundCorners { radius: 8.0 };
        assert_eq!(
            validate_effect(&vector, &rc).unwrap_err(),
            "圆角仅对盒对象有效(路径/文字对象不支持)"
        );
        assert_eq!(
            validate_effect(&text, &rc).unwrap_err(),
            "圆角仅对盒对象有效(路径/文字对象不支持)"
        );
        let ish = Effect::InnerShadow {
            x: 0.0,
            y: 0.0,
            blur: 4.0,
            spread: 0.0,
            color: "#000".into(), // vb-token-ok: 测试/映射数据(文档内容色,非 UI 皮肤)
        };
        assert!(validate_effect(&text, &ish).unwrap_err().contains("内阴影"));
        assert!(validate_effect(&NodeKind::Box, &rc).is_ok());
    }

    /// 空路径(测试夹具用;真实 Vector 由钢笔/导入产生)。
    fn kurbo_stub() -> vb_common::geom::BezPath {
        let mut p = vb_common::geom::BezPath::new();
        p.move_to(vb_common::geom::Point::new(0.0, 0.0));
        p.line_to(vb_common::geom::Point::new(10.0, 0.0));
        p
    }
}
