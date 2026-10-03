//! VB 设计令牌 → sable 主题注入。
//!
//! **唯一真相是 `docs/design/assets/vb-ui-tokens.json`**(22 篇 §3.4:
//! 本工程不另造表,落地方式 = `SableTheme::inject(vb 调色板)`);本文件是
//! 它的 Rust 镜像 + 投影层,同步由门禁测试机械保证:
//!
//! - **G-UI1 tokens_sync2**(`tests/tokens_sync2.rs`):JSON ↔ 本文件常量
//!   ↔ `vb_colors` 产出的 sable `ColorTokens` 逐值比对,任何漂移红;
//! - **G-UI2 hex 纪律**(`tests/shell_purity.rs`):hex 颜色字面量只允许
//!   出现在本文件,组件一律经主题(`theme(cx)`)或令牌取色。
//!
//! # 17 色 → sable 15 槽位投影([`vb_colors`])
//!
//! sable `ColorTokens` 只有 15 个 UI 色槽,VB 的 17 色(深)/16 色(浅,
//! 无 accent-hover)按语义投影:
//!
//! | VB 令牌 | sable 槽位 |
//! |---|---|
//! | bg-canvas | surface_0 |
//! | bg-panel | surface_1 |
//! | bg-input | surface_2(sable 语义"卡片/输入框") |
//! | bg-hover | surface_3 |
//! | bg-active | surface_4 |
//! | border | border_subtle |
//! | border-strong | border_strong |
//! | text | text_primary |
//! | text-2 | text_secondary |
//! | text-3 | text_disabled |
//! | accent | accent |
//! | accent-dim | accent_muted |
//! | danger / warn / success | danger / warning / success |
//!
//! **bg-raised 与 accent-hover 无直接槽位(如实声明)**:弹出层/下拉/Tab
//! 等 chrome 由 gpui-component 自身主题承担(宿主在 `sable::dock::init` 后
//! `Theme::change` 固定深色);需要这两个色的 vb_kit 组件经 [`VbPalette`]
//! 直接取(同样受 G-UI1 逐值校验)。跨主题**语义色**(智能参考线品红等)
//! 不进 UI 主题,经 [`semantic`] 取 —— 与 JSON `color.semantic` 的
//! "禁止当装饰色使用"纪律一致。

use sable::gpui::{rgba, App, Hsla};
use sable::widgets::theme::{self as sable_theme, ThemeMode};
use sable::widgets::tokens::ColorTokens;

// ---------------------------------------------------------------------------
// hex 常量(全仓 UI 侧唯一允许颜色字面量的文件;G-UI1 逐值比对的对象)
// ---------------------------------------------------------------------------

/// 原始色值(`0xRRGGBBAA`)。与 JSON `color.dark` / `color.light` /
/// `color.semantic` 逐值同步;alpha 字节 = `round(alpha × 255)`。
pub mod hex {
    // ── color.dark(17)──
    pub const DARK_BG_CANVAS: u32 = 0x1C1C1EFF;
    pub const DARK_BG_PANEL: u32 = 0x2C2C2EFF;
    pub const DARK_BG_RAISED: u32 = 0x3A3A3CFF;
    pub const DARK_BG_INPUT: u32 = 0x38383AFF;
    pub const DARK_BG_HOVER: u32 = 0x48484AFF;
    pub const DARK_BG_ACTIVE: u32 = 0x545457FF;
    pub const DARK_BORDER: u32 = 0x3F3F42FF;
    pub const DARK_BORDER_STRONG: u32 = 0x545457FF;
    pub const DARK_TEXT: u32 = 0xFFFFFFFF;
    pub const DARK_TEXT_2: u32 = 0xB8B8BDFF;
    pub const DARK_TEXT_3: u32 = 0x96969BFF;
    pub const DARK_ACCENT: u32 = 0x0D99FFFF;
    pub const DARK_ACCENT_HOVER: u32 = 0x3AAEFFFF;
    /// rgba(13,153,255,0.16) → α = round(0.16 × 255) = 41 = 0x29
    pub const DARK_ACCENT_DIM: u32 = 0x0D99FF29;
    pub const DARK_DANGER: u32 = 0xF24822FF;
    pub const DARK_WARN: u32 = 0xFFC700FF;
    pub const DARK_SUCCESS: u32 = 0x14AE5CFF;

    // ── color.light(16;JSON 浅色未定义 accent-hover)──
    pub const LIGHT_BG_CANVAS: u32 = 0xF5F5F5FF;
    pub const LIGHT_BG_PANEL: u32 = 0xFFFFFFFF;
    pub const LIGHT_BG_RAISED: u32 = 0xFFFFFFFF;
    pub const LIGHT_BG_INPUT: u32 = 0xF0F0F0FF;
    pub const LIGHT_BG_HOVER: u32 = 0xE8E8E8FF;
    pub const LIGHT_BG_ACTIVE: u32 = 0xDEDEDEFF;
    pub const LIGHT_BORDER: u32 = 0xE6E6E6FF;
    pub const LIGHT_BORDER_STRONG: u32 = 0xC9C9C9FF;
    pub const LIGHT_TEXT: u32 = 0x1E1E1EFF;
    pub const LIGHT_TEXT_2: u32 = 0x6B6B6BFF;
    pub const LIGHT_TEXT_3: u32 = 0x767676FF;
    pub const LIGHT_ACCENT: u32 = 0x0D99FFFF;
    /// rgba(13,153,255,0.12) → α = round(0.12 × 255) = 31 = 0x1F
    pub const LIGHT_ACCENT_DIM: u32 = 0x0D99FF1F;
    pub const LIGHT_DANGER: u32 = 0xD93025FF;
    pub const LIGHT_WARN: u32 = 0x8F6700FF;
    pub const LIGHT_SUCCESS: u32 = 0x0E7C42FF;

    // ── color.semantic(跨主题;immutable 者不可改,禁止当装饰色)──
    pub const SEMANTIC_GUIDE_SMART: u32 = 0xFF00FFFF;
    pub const SEMANTIC_GUIDE_SMART_LIGHT: u32 = 0xE000E0FF;
    pub const SEMANTIC_SELECT_BOX: u32 = 0x0D99FFFF;
    /// rgba(13,153,255,0.6) → α = 153 = 0x99
    pub const SEMANTIC_HOVER_BOX: u32 = 0x0D99FF99;
    pub const SEMANTIC_GUIDE_GRID: u32 = 0x3A3A3AFF;
    pub const SEMANTIC_GUIDE_GRID_LIGHT: u32 = 0xDADADAFF;
}

fn color(hex: u32) -> Hsla {
    rgba(hex).into()
}

// ---------------------------------------------------------------------------
// 全量调色板(17 色 × 深/浅 + 语义色;无槽位令牌的取色出口)
// ---------------------------------------------------------------------------

/// VB 全量 UI 调色板(与 JSON 17/16 令牌一一对应)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VbPalette {
    pub bg_canvas: Hsla,
    pub bg_panel: Hsla,
    pub bg_raised: Hsla,
    pub bg_input: Hsla,
    pub bg_hover: Hsla,
    pub bg_active: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_2: Hsla,
    pub text_3: Hsla,
    pub accent: Hsla,
    /// accent 悬停(JSON 浅色未定义 → `None`;悬停加亮策略 R2 面板批次
    /// 与浅色主题一起裁决,不许先拍)。
    pub accent_hover: Option<Hsla>,
    pub accent_dim: Hsla,
    pub danger: Hsla,
    pub warn: Hsla,
    pub success: Hsla,
}

impl VbPalette {
    /// 深色(JSON `color.dark`,17 项)。
    pub fn dark() -> Self {
        VbPalette {
            bg_canvas: color(hex::DARK_BG_CANVAS),
            bg_panel: color(hex::DARK_BG_PANEL),
            bg_raised: color(hex::DARK_BG_RAISED),
            bg_input: color(hex::DARK_BG_INPUT),
            bg_hover: color(hex::DARK_BG_HOVER),
            bg_active: color(hex::DARK_BG_ACTIVE),
            border: color(hex::DARK_BORDER),
            border_strong: color(hex::DARK_BORDER_STRONG),
            text: color(hex::DARK_TEXT),
            text_2: color(hex::DARK_TEXT_2),
            text_3: color(hex::DARK_TEXT_3),
            accent: color(hex::DARK_ACCENT),
            accent_hover: Some(color(hex::DARK_ACCENT_HOVER)),
            accent_dim: color(hex::DARK_ACCENT_DIM),
            danger: color(hex::DARK_DANGER),
            warn: color(hex::DARK_WARN),
            success: color(hex::DARK_SUCCESS),
        }
    }

    /// 浅色(JSON `color.light`,16 项)。
    pub fn light() -> Self {
        VbPalette {
            bg_canvas: color(hex::LIGHT_BG_CANVAS),
            bg_panel: color(hex::LIGHT_BG_PANEL),
            bg_raised: color(hex::LIGHT_BG_RAISED),
            bg_input: color(hex::LIGHT_BG_INPUT),
            bg_hover: color(hex::LIGHT_BG_HOVER),
            bg_active: color(hex::LIGHT_BG_ACTIVE),
            border: color(hex::LIGHT_BORDER),
            border_strong: color(hex::LIGHT_BORDER_STRONG),
            text: color(hex::LIGHT_TEXT),
            text_2: color(hex::LIGHT_TEXT_2),
            text_3: color(hex::LIGHT_TEXT_3),
            accent: color(hex::LIGHT_ACCENT),
            accent_hover: None,
            accent_dim: color(hex::LIGHT_ACCENT_DIM),
            danger: color(hex::LIGHT_DANGER),
            warn: color(hex::LIGHT_WARN),
            success: color(hex::LIGHT_SUCCESS),
        }
    }
}

/// 按主题取全量调色板。
pub fn palette(mode: ThemeMode) -> VbPalette {
    match mode {
        ThemeMode::Dark => VbPalette::dark(),
        ThemeMode::Light => VbPalette::light(),
    }
}

/// 跨主题语义色(JSON `color.semantic`;不随主题大幅变化,禁止当装饰色)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VbSemantic {
    /// 智能参考线(AI 品红;`immutable: true`,浅色下 #E000E0 保对比)。
    pub guide_smart: Hsla,
    /// 边界框 / 8 手柄(与 accent 同值是设计意图)。
    pub select_box: Hsla,
    /// 悬停对象轮廓(accent 60%)。
    pub hover_box: Hsla,
    /// 网格线。
    pub guide_grid: Hsla,
}

/// 语义色按主题取值(`light` 变体为 JSON `color.semantic.*.light`)。
pub fn semantic(mode: ThemeMode) -> VbSemantic {
    match mode {
        ThemeMode::Dark => VbSemantic {
            guide_smart: color(hex::SEMANTIC_GUIDE_SMART),
            select_box: color(hex::SEMANTIC_SELECT_BOX),
            hover_box: color(hex::SEMANTIC_HOVER_BOX),
            guide_grid: color(hex::SEMANTIC_GUIDE_GRID),
        },
        ThemeMode::Light => VbSemantic {
            guide_smart: color(hex::SEMANTIC_GUIDE_SMART_LIGHT),
            select_box: color(hex::SEMANTIC_SELECT_BOX),
            hover_box: color(hex::SEMANTIC_HOVER_BOX),
            guide_grid: color(hex::SEMANTIC_GUIDE_GRID_LIGHT),
        },
    }
}

/// 吸附阈值(屏幕空间 px,不随缩放变化;JSON `semantic.snap-threshold`)。
pub const SNAP_THRESHOLD_PX: f32 = 6.0;
/// 手柄命中半径(屏幕空间 px;JSON `semantic.handle-hit-radius`)。
pub const HANDLE_HIT_RADIUS_PX: f32 = 6.0;

// ---------------------------------------------------------------------------
// 数值令牌:间距 / 圆角 / 描边 / 动效 / 字号 / 结构常量(全部 4 基数纪律)
// ---------------------------------------------------------------------------

/// 间距(JSON `space.*`;4 基数,禁止 5/7/10/15/20 等离格值)。
pub mod space {
    pub const S1: f32 = 2.0;
    pub const S2: f32 = 4.0;
    pub const S3: f32 = 6.0;
    pub const S4: f32 = 8.0;
    pub const S5: f32 = 12.0;
    pub const S6: f32 = 16.0;
    pub const S8: f32 = 24.0;
    pub const S9: f32 = 32.0;
}

/// 圆角(JSON `radius.*`;4/6/8/12,与 sable RadiusTokens 档位一致)。
pub mod radius {
    /// 输入框/小按钮
    pub const SM: f32 = 4.0;
    /// 按钮/Tab/下拉(全局默认)
    pub const MD: f32 = 6.0;
    /// 卡片/分组块/menu
    pub const LG: f32 = 8.0;
    /// 工具条/浮层/window
    pub const XL: f32 = 12.0;
}

/// 描边宽度(JSON `stroke.*`)。
pub mod stroke {
    pub const HAIRLINE: f32 = 1.0;
    /// 焦点环(22 篇 §4 R3:焦点环 border-strong 1.5px 全局可见)
    pub const FOCUS: f32 = 1.5;
}

/// 动效时长 ms(JSON `motion.*`;"绝不做的动效"纪律见 JSON `$note`)。
pub mod motion_ms {
    /// 画布缩放/平移/拖动:直接赋值,不走动画
    pub const INSTANT: f32 = 0.0;
    /// 悬停变色
    pub const HOVER: f32 = 80.0;
    /// 选中/开关/展开收起
    pub const STATE: f32 = 120.0;
    /// 下拉/tooltip 淡入
    pub const POPUP: f32 = 150.0;
    /// 面板折叠/Tab 切换
    pub const PANEL: f32 = 200.0;
}

/// 字号 px(JSON `font.size.*`)。
pub mod font_size {
    pub const CAPTION: f32 = 11.0;
    pub const LABEL: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const BODY_STRONG: f32 = 13.0;
    pub const TITLE: f32 = 15.0;
    /// 十六进制/代码视图(等宽)
    pub const MONO: f32 = 12.0;
}

/// 行高 px(JSON `font.size.*.line_height`;控件高度派生制的输入,
/// 裁决一:名义档 22/26/32 保留,文字控件高度 = 行高 + 8、下限 24)。
pub mod line_height {
    pub const CAPTION: f32 = 16.0;
    pub const LABEL: f32 = 18.0;
    pub const BODY: f32 = 20.0;
    pub const BODY_STRONG: f32 = 20.0;
    pub const TITLE: f32 = 22.0;
    pub const MONO: f32 = 18.0;
}

/// 结构常量(JSON `layout.*`;R0 消费子集)。
pub mod layout {
    pub const MENU_BAR: f32 = 32.0;
    pub const STATUS_BAR: f32 = 28.0;
    pub const RULER: f32 = 20.0;
    /// 右坞默认/最小/最大/折叠宽
    pub const RIGHT_DOCK_DEFAULT: f32 = 280.0;
    pub const RIGHT_DOCK_MIN: f32 = 240.0;
    pub const RIGHT_DOCK_MAX: f32 = 420.0;
    pub const RIGHT_DOCK_COLLAPSED: f32 = 40.0;
    /// 低于此宽度右侧坞自动折叠为图标条(22 篇裁决二:新宿主保留)
    pub const COLLAPSE_BELOW: f32 = 1200.0;
    /// 图层行 / 列表项行高
    pub const ROW_HEIGHT_LAYER: f32 = 24.0;
    pub const ROW_HEIGHT_LIST: f32 = 24.0;
    /// 最小窗口 1024×640
    pub const MIN_WINDOW_W: f32 = 1024.0;
    pub const MIN_WINDOW_H: f32 = 640.0;
}

// ---------------------------------------------------------------------------
// 投影与注入
// ---------------------------------------------------------------------------

/// VB 调色板 → sable `ColorTokens` 的 15 槽位投影(映射表见模块文档;
/// bg-raised / accent-hover 不经此路,组件取 [`palette`])。
pub fn vb_colors(mode: ThemeMode) -> ColorTokens {
    let p = palette(mode);
    ColorTokens {
        surface_0: p.bg_canvas,
        surface_1: p.bg_panel,
        surface_2: p.bg_input,
        surface_3: p.bg_hover,
        surface_4: p.bg_active,
        border_subtle: p.border,
        border_strong: p.border_strong,
        text_primary: p.text,
        text_secondary: p.text_2,
        text_disabled: p.text_3,
        accent: p.accent,
        accent_muted: p.accent_dim,
        danger: p.danger,
        warning: p.warn,
        success: p.success,
    }
}

/// 把 VB 调色板灌进 sable 全局主题(**默认深色**,应用启动调用一次)。
///
/// 内部即 `sable_theme::inject(cx, vb_colors(mode), mode)`;调用方需已跑
/// `sable::dock::init`(其含 sable theme 初始化与 gpui_component 初始化)。
/// 注意注入会取消进行中的主题过渡(sable inject 语义),R0 宿主固定深色,
/// 不受影响。
pub fn inject_vb_theme(cx: &mut App) {
    inject_vb_theme_mode(cx, ThemeMode::Dark);
}

/// 指定模式注入(R1 浅色主题接入时的入口;R0 只有深色被调用)。
pub fn inject_vb_theme_mode(cx: &mut App, mode: ThemeMode) {
    sable_theme::inject(cx, vb_colors(mode), mode);
}
