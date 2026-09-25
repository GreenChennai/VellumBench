//! 外观条目模型(serde,紧凑 JSON 落盘)+ 目标类型与受管属性组。
//!
//! 06-1 自 `appearance.rs` 按「模型 / 编解码 / 命令 / 渲染」拆出
//! (纯搬移,零行为变化);CSS 编解码在 `codec`,片段映射在 `segments`。

use serde::{Deserialize, Serialize};

use vb_doc::model::NodeKind;

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
    pub(super) fn empty() -> Self {
        AppearanceModel {
            v: 1,
            own: Vec::new(),
            items: Vec::new(),
        }
    }

    pub(super) fn own_add(own: &mut Vec<String>, tag: &str) {
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

    pub(super) fn set_enabled(&mut self, v: bool) {
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

    pub(super) fn set_blend(&mut self, v: Option<String>) {
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
    pub(super) fn own_tag(&self) -> String {
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

pub(super) fn stroke_props(t: AppearanceTarget) -> &'static [&'static str] {
    match t {
        AppearanceTarget::Box => STROKE_PROPS_BOX,
        AppearanceTarget::Vector => STROKE_PROPS_VECTOR,
        AppearanceTarget::Text => STROKE_PROPS_TEXT,
        AppearanceTarget::Frozen => &[],
    }
}
