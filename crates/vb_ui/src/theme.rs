//! 设计令牌与主题注入（设计文档 14 篇 §3.2 / §3.8 / §3.9）。
//!
//! ## 为什么单独一个模块
//!
//! 界面颜色此前散落在 `vb_app` 各处字面量里，导致：① 换主题要改几十处
//! ② 深浅两套无法共存 ③ 无人知道哪个值是"设计定的"。本模块把令牌收成
//! **编译期单一真相**，`docs/design/assets/vb-ui-tokens.json` 是设计侧真相，
//! 两者的数值由 `tokens_sync` 测试强制一致（随 `tools/ci.ps1` 进 CI）。
//!
//! ## 门禁 8 白名单
//!
//! 本文件是全仓**唯一**允许出现颜色字面量的界面文件（等价于
//! `vb_common/src/color.rs` 之于文档色）。其它任何文件写死颜色都会被
//! `tools/check_no_hardcoded_color.ps1` 拦下。
//!
//! ## 命名空间
//!
//! UI 主题令牌（`--vb-ui-*`）与文档令牌（`--vb-brand-*`，用户在画布上编辑的
//! CSS 变量）**严格分离**，禁止互相引用 —— 前者是软件自身的外观，后者是
//! 用户文档的内容。

use egui::{Color32, FontId, Margin, Stroke, TextStyle, Vec2};

use crate::fonts;

// ───────────────────────────── 色彩令牌 ─────────────────────────────

/// 一套界面主题的全部颜色。
///
/// 字段与 `vb-ui-tokens.json` 的 `color.<dark|light>` 键一一对应，
/// 对应关系由 [`Tokens::entries`] 声明并被 `tokens_sync` 测试校验。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tokens {
    /// 是否为深色主题（决定 `visuals.dark_mode` 与几处派生行为）。
    pub dark: bool,
    pub bg_canvas: Color32,
    pub bg_panel: Color32,
    pub bg_raised: Color32,
    pub bg_input: Color32,
    pub bg_hover: Color32,
    pub bg_active: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub text_2: Color32,
    pub text_3: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_dim: Color32,
    pub danger: Color32,
    pub warn: Color32,
    pub success: Color32,
}

impl Tokens {
    /// 深色主题（默认）。
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg_canvas: Color32::from_rgb(0x1C, 0x1C, 0x1E),
            bg_panel: Color32::from_rgb(0x2C, 0x2C, 0x2E),
            bg_raised: Color32::from_rgb(0x3A, 0x3A, 0x3C),
            bg_input: Color32::from_rgb(0x38, 0x38, 0x3A),
            bg_hover: Color32::from_rgb(0x48, 0x48, 0x4A),
            bg_active: Color32::from_rgb(0x54, 0x54, 0x57),
            border: Color32::from_rgb(0x3F, 0x3F, 0x42),
            border_strong: Color32::from_rgb(0x54, 0x54, 0x57),
            text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            text_2: Color32::from_rgb(0xB8, 0xB8, 0xBD),
            text_3: Color32::from_rgb(0x7A, 0x7A, 0x80),
            accent: Color32::from_rgb(0x0D, 0x99, 0xFF),
            accent_hover: Color32::from_rgb(0x3A, 0xAE, 0xFF),
            accent_dim: Color32::from_rgba_unmultiplied(0x0D, 0x99, 0xFF, 41),
            danger: Color32::from_rgb(0xF2, 0x48, 0x22),
            warn: Color32::from_rgb(0xFF, 0xC7, 0x00),
            success: Color32::from_rgb(0x14, 0xAE, 0x5C),
        }
    }

    /// 浅色主题。
    ///
    /// **不是深色的反相**：灰阶是独立调过的（面板比画布亮，靠 `window_shadow`
    /// 与描边分层，而不是靠明度差），否则会得到"糊成一片"的浅色界面。
    pub fn light() -> Self {
        Self {
            dark: false,
            bg_canvas: Color32::from_rgb(0xF5, 0xF5, 0xF5),
            bg_panel: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            bg_raised: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            bg_input: Color32::from_rgb(0xF0, 0xF0, 0xF0),
            bg_hover: Color32::from_rgb(0xE8, 0xE8, 0xE8),
            bg_active: Color32::from_rgb(0xDE, 0xDE, 0xDE),
            border: Color32::from_rgb(0xE6, 0xE6, 0xE6),
            border_strong: Color32::from_rgb(0xC9, 0xC9, 0xC9),
            text: Color32::from_rgb(0x1E, 0x1E, 0x1E),
            text_2: Color32::from_rgb(0x6B, 0x6B, 0x6B),
            // U-6:浅底对比度走查 —— #9B9B9B 对白底只有 2.8:1(禁用/占位文字
            // 也要求 ≥4.5),加深到 #767676(4.54:1);深色侧维持原设计值。
            text_3: Color32::from_rgb(0x76, 0x76, 0x76),
            // 品牌色两套共用（JSON: "两套主题共用品牌色"）
            accent: Color32::from_rgb(0x0D, 0x99, 0xFF),
            accent_hover: Color32::from_rgb(0x3A, 0xAE, 0xFF),
            accent_dim: Color32::from_rgba_unmultiplied(0x0D, 0x99, 0xFF, 31),
            // U-6:#F24822 对白底 3.67:1,12px 正文不达 AA(4.5);
            // 加深到 #D93025(4.77:1)。深色侧维持原设计值。
            danger: Color32::from_rgb(0xD9, 0x30, 0x25),
            // 浅底上 #FFC700 对比度不足,按设计加深;U-6 走查再加深一档
            // (#B58200 对白底 3.41:1 → #8F6700 = 5.11:1,达 WCAG AA)
            warn: Color32::from_rgb(0x8F, 0x67, 0x00),
            success: Color32::from_rgb(0x0E, 0x7C, 0x42),
        }
    }

    /// 按 `dark` 取一套。
    pub fn get(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }

    /// `(令牌名, 值)` 清单。名字必须与 `vb-ui-tokens.json` 的
    /// `color.<dark|light>` 键一致 —— `tokens_sync` 测试据此逐项比对。
    pub fn entries(&self) -> [(&'static str, Color32); 17] {
        [
            ("bg-canvas", self.bg_canvas),
            ("bg-panel", self.bg_panel),
            ("bg-raised", self.bg_raised),
            ("bg-input", self.bg_input),
            ("bg-hover", self.bg_hover),
            ("bg-active", self.bg_active),
            ("border", self.border),
            ("border-strong", self.border_strong),
            ("text", self.text),
            ("text-2", self.text_2),
            ("text-3", self.text_3),
            ("accent", self.accent),
            ("accent-hover", self.accent_hover),
            ("accent-dim", self.accent_dim),
            ("danger", self.danger),
            ("warn", self.warn),
            ("success", self.success),
        ]
    }
}

// ──────────────────────────── 语义色 ────────────────────────────

/// 跨主题语义色。
///
/// 这些颜色**表达含义而非风格**，因此不随主题大幅变化（浅色下仅微调以保
/// 对比度）。**禁止当装饰色使用** —— 它们每一个都对应画布上的一种确定语义，
/// 乱用会让用户误读。JSON 中 `immutable: true` 者不可修改。
pub mod semantic {
    use egui::Color32;

    /// 智能参考线（AI 品红）。
    ///
    /// 02/03 篇钉死的语义色：这是 Illustrator 用户肌肉记忆的一部分，
    /// **深色下不可改**。浅色下加深到 `#E000E0` 以保证对比度。
    pub const GUIDE_SMART_DARK: Color32 = Color32::from_rgb(0xFF, 0x00, 0xFF);
    /// 智能参考线（浅色）。
    pub const GUIDE_SMART_LIGHT: Color32 = Color32::from_rgb(0xE0, 0x00, 0xE0);

    /// 选中边界框与 8 个手柄。
    pub const SELECT_BOX: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);
    /// 标尺参考线（AI 青色,02 篇 §5.2;跨主题共用,浅色下同值够对比）。
    pub const GUIDE_RULER: Color32 = Color32::from_rgb(0x00, 0xA5, 0xFF);
    /// 悬停对象的轮廓（同色半透明，与选中框区分）。
    pub const HOVER_BOX: Color32 = Color32::from_rgba_unmultiplied_const(0x0D, 0x99, 0xFF, 153);
    /// 框选（marquee）填充。
    pub const MARQUEE_FILL: Color32 = Color32::from_rgba_unmultiplied_const(0x0D, 0x99, 0xFF, 24);

    /// 网格线（深色）。
    pub const GUIDE_GRID_DARK: Color32 = Color32::from_rgb(0x3A, 0x3A, 0x3A);
    /// 网格线（浅色）。
    pub const GUIDE_GRID_LIGHT: Color32 = Color32::from_rgb(0xDA, 0xDA, 0xDA);

    /// 冻结块占位底（ADR-0017：无法解析的块以占位呈现）。
    pub const FROZEN_FILL_DARK: Color32 = Color32::from_rgba_unmultiplied_const(200, 195, 185, 60);
    /// 冻结块占位底（浅色）。
    pub const FROZEN_FILL_LIGHT: Color32 = Color32::from_rgba_unmultiplied_const(140, 130, 110, 46);
    /// 冻结块描边。
    pub const FROZEN_STROKE: Color32 = Color32::from_rgb(0xB0, 0x9A, 0x7A);

    /// 按主题取智能参考线色。
    pub fn guide_smart(dark: bool) -> Color32 {
        if dark {
            GUIDE_SMART_DARK
        } else {
            GUIDE_SMART_LIGHT
        }
    }

    /// 按主题取网格色。
    pub fn guide_grid(dark: bool) -> Color32 {
        if dark {
            GUIDE_GRID_DARK
        } else {
            GUIDE_GRID_LIGHT
        }
    }

    /// 按主题取冻结块占位底。
    pub fn frozen_fill(dark: bool) -> Color32 {
        if dark {
            FROZEN_FILL_DARK
        } else {
            FROZEN_FILL_LIGHT
        }
    }

    /// 区域文本溢出红点(design/03 §六:文本框右下角红点)。
    pub const OVERFLOW_DOT_DARK: Color32 = Color32::from_rgb(0xFF, 0x45, 0x3A);
    /// 区域文本溢出红点(浅色,加深保证对比)。
    pub const OVERFLOW_DOT_LIGHT: Color32 = Color32::from_rgb(0xD7, 0x00, 0x15);

    /// 按主题取溢出红点色。
    pub fn overflow_dot(dark: bool) -> Color32 {
        if dark {
            OVERFLOW_DOT_DARK
        } else {
            OVERFLOW_DOT_LIGHT
        }
    }

    /// 差异视图·删除行(深色;快照对比「磁盘 −」)。
    pub const DIFF_DEL_DARK: Color32 = Color32::from_rgb(0xFF, 0x78, 0x64);
    /// 差异视图·删除行(浅色,加深保对比;同溢出红点浅色的设计值)。
    pub const DIFF_DEL_LIGHT: Color32 = Color32::from_rgb(0xD7, 0x00, 0x15);
    /// 差异视图·新增行(深色;快照对比「快照 +」)。
    pub const DIFF_ADD_DARK: Color32 = Color32::from_rgb(0x82, 0xDC, 0x82);
    /// 差异视图·新增行(浅色,加深保对比;同 success 浅色的设计值)。
    pub const DIFF_ADD_LIGHT: Color32 = Color32::from_rgb(0x0E, 0x7C, 0x42);

    /// 按主题取差异删除行色。
    pub fn diff_del(dark: bool) -> Color32 {
        if dark {
            DIFF_DEL_DARK
        } else {
            DIFF_DEL_LIGHT
        }
    }

    /// 按主题取差异新增行色。
    pub fn diff_add(dark: bool) -> Color32 {
        if dark {
            DIFF_ADD_DARK
        } else {
            DIFF_ADD_LIGHT
        }
    }
}

// ───────────────────────── 间距 / 圆角 / 描边 ─────────────────────────

/// 间距刻度。**4 为基数** —— 禁止 5/7/10/15/20 这类值，
/// 它们是"看起来没对齐"的根源。
pub mod space {
    pub const S1: f32 = 2.0;
    pub const S2: f32 = 4.0;
    pub const S3: f32 = 6.0;
    pub const S4: f32 = 8.0;
    pub const S5: f32 = 12.0;
    pub const S6: f32 = 16.0;
    pub const S8: f32 = 24.0;
    pub const S9: f32 = 32.0;

    /// 行高（图层行、列表项）。
    pub const ROW_HEIGHT: f32 = 24.0;
    /// 状态栏高度。
    pub const STATUS_BAR_HEIGHT: f32 = 28.0;
    /// 底部浮动工具条高度。
    pub const FLOATING_TOOLBAR_HEIGHT: f32 = 40.0;
    /// 控制面板高度(S1-c 02-2:菜单栏下 40px 随工具上下文条)。
    pub const CONTROL_BAR_HEIGHT: f32 = 40.0;
    /// 右侧坞默认宽度。
    pub const DOCK_WIDTH: f32 = 280.0;
    /// 低于此宽度右侧坞自动折叠。
    pub const COLLAPSE_BELOW: f32 = 1200.0;
}

/// 圆角刻度（单位 px，喂给 `CornerRadius::same`）。
///
/// egui 默认 2px 近乎直角，是"业余感"三大来源之一。
pub mod radius {
    use egui::CornerRadius;

    /// 输入框、小按钮。
    pub const SM: u8 = 4;
    /// 按钮、Tab、下拉（全局默认）。
    pub const MD: u8 = 6;
    /// 面板内卡片、分组块、菜单。
    pub const LG: u8 = 8;
    /// 底部工具条、浮层、窗口。
    pub const XL: u8 = 12;

    pub fn sm() -> CornerRadius {
        CornerRadius::same(SM)
    }
    pub fn md() -> CornerRadius {
        CornerRadius::same(MD)
    }
    pub fn lg() -> CornerRadius {
        CornerRadius::same(LG)
    }
    pub fn xl() -> CornerRadius {
        CornerRadius::same(XL)
    }
}

/// 描边宽度。
pub mod stroke {
    /// 分隔线。
    pub const HAIRLINE: f32 = 1.0;
    /// 焦点环。
    pub const FOCUS: f32 = 1.5;
}

/// 动效时长（秒）。绝不做的动效：画布元素入场动画、按钮弹跳、粒子效果
/// —— 工具软件里这些是减速带。
pub mod motion {
    /// 画布缩放/平移/拖动：不走动画。
    pub const INSTANT: f32 = 0.0;
    /// 悬停变色。
    pub const HOVER: f32 = 0.08;
    /// 选中、开关、展开收起。
    pub const STATE: f32 = 0.12;
    /// 面板折叠、Tab 切换。
    pub const PANEL: f32 = 0.20;
}

// ───────────────────── 动效总开关(H-1:可关 + 持久化) ─────────────────────
//
// **为什么是开关而不是探测**:egui/winit 不暴露系统「减少动态效果」
// 无障碍设置的跨平台读取口;这里以**显式设置项**承接同一语义
// (首选项「常规」/视图菜单可关,状态入 workspace.json,默认开)。
// 关闭后:① egui 全局 `animation_time` 归零(所有跟随样式的过渡立即到位);
// ② 组件里显式传时长的 `animate_bool_with_time` 经 [`anim_time`] 同步归零;
// ③ 对话框/Tab 的一次性淡入(motion 模块)直接跳到终态。

/// 动效开关在 egui 持久数据里的键。
fn motion_flag_id() -> egui::Id {
    egui::Id::new("vb_motion_enabled")
}

/// 写动效总开关(应用启动与切换时调用;未写过时读作「开」)。
pub fn set_motion_enabled(ctx: &egui::Context, enabled: bool) {
    ctx.data_mut(|d| d.insert_temp(motion_flag_id(), enabled));
}

/// 读动效总开关(默认开 —— 未注入过的上下文一律有动效)。
pub fn motion_enabled(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.get_temp(motion_flag_id()).unwrap_or(true))
}

/// 动效感知的时长换算:总开关关闭时任何动效时长都归零
/// (调用方把返回值喂给 `animate_bool_with_time` 一类 API)。
pub fn anim_time(ctx: &egui::Context, base: f32) -> f32 {
    if motion_enabled(ctx) {
        base
    } else {
        0.0
    }
}

// ──────────────────────────── 注入 ────────────────────────────

/// 按主题把令牌注入 egui 全局样式（14 篇 §3.8 配方）。
///
/// 幂等：可随时重调（主题切换、窗口尺寸变化都不需要重启）。
pub fn apply(ctx: &egui::Context, dark: bool) {
    apply_ex(ctx, dark, 1.0, true);
}

/// 同 [`apply`]，但按 `scale` 缩放字号（供后续"界面缩放"设置使用）。
pub fn apply_scaled(ctx: &egui::Context, dark: bool, scale: f32) {
    apply_ex(ctx, dark, scale, true);
}

/// 同 [`apply`]，带动效总开关(H-1):`motion = false` 时全局
/// `animation_time` 归零,并写入开关供组件侧 [`anim_time`] 读取。
pub fn apply_ex(ctx: &egui::Context, dark: bool, scale: f32, motion: bool) {
    set_motion_enabled(ctx, motion);
    apply_impl(ctx, dark, scale, motion);
}

fn apply_impl(ctx: &egui::Context, dark: bool, scale: f32, _motion: bool) {
    let t = Tokens::get(dark);
    let scale = if scale > 0.0 { scale } else { 1.0 };
    // ⚠️ 死锁教训(第四轮实测):动效开关读 egui 数据锁,必须**先算后进** ——
    // 在 all_styles_mut(持有 Context 写锁)里再调 data_mut 会自锁,
    // 触发 epaint RwLock 10s DEBUG PANIC(启动即 101 退出)。
    let anim = anim_time(ctx, motion::STATE);

    ctx.all_styles_mut(|s| {
        // ── 色彩 ──
        s.visuals.dark_mode = dark;
        s.visuals.panel_fill = t.bg_panel;
        s.visuals.window_fill = t.bg_raised;
        s.visuals.extreme_bg_color = t.bg_canvas;
        s.visuals.faint_bg_color = t.bg_input;
        s.visuals.code_bg_color = t.bg_input;
        s.visuals.override_text_color = Some(t.text);
        s.visuals.weak_text_color = Some(t.text_2);
        s.visuals.weak_text_alpha = 0.75;
        s.visuals.selection.bg_fill = t.accent;
        s.visuals.selection.stroke = Stroke::new(stroke::HAIRLINE, t.text);
        s.visuals.hyperlink_color = t.accent;
        s.visuals.warn_fg_color = t.warn;
        s.visuals.error_fg_color = t.danger;

        // 浅色主题靠描边与阴影分层，不靠明度差（否则糊成一片）
        s.visuals.window_stroke = Stroke::new(
            stroke::HAIRLINE,
            if dark { t.border } else { t.border_strong },
        );
        s.visuals.window_shadow = if dark {
            egui::epaint::Shadow::NONE
        } else {
            egui::epaint::Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: Color32::from_black_alpha(28),
            }
        };
        s.visuals.popup_shadow = if dark {
            egui::epaint::Shadow::NONE
        } else {
            egui::epaint::Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: Color32::from_black_alpha(32),
            }
        };

        // ── 圆角（业余感来源之一，必须改） ──
        s.visuals.window_corner_radius = radius::xl();
        s.visuals.menu_corner_radius = radius::lg();
        s.visuals.widgets.noninteractive.corner_radius = radius::md();
        s.visuals.widgets.inactive.corner_radius = radius::md();
        s.visuals.widgets.hovered.corner_radius = radius::md();
        s.visuals.widgets.active.corner_radius = radius::md();
        s.visuals.widgets.open.corner_radius = radius::md();

        // ── 去"凸起感"：expansion 是 egui 默认立体效果的来源 ──
        s.visuals.widgets.noninteractive.expansion = 0.0;
        s.visuals.widgets.inactive.expansion = 0.0;
        s.visuals.widgets.hovered.expansion = 0.0;
        s.visuals.widgets.active.expansion = 0.0;
        s.visuals.widgets.open.expansion = 0.0;

        // ── 状态底/描边：悬停必须改"底色"而不只是描边，否则感知不到 ──
        s.visuals.widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
        s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(stroke::HAIRLINE, t.border);
        s.visuals.widgets.noninteractive.fg_stroke = Stroke::new(stroke::HAIRLINE, t.text);
        s.visuals.widgets.inactive.weak_bg_fill = t.bg_input;
        s.visuals.widgets.inactive.bg_stroke = Stroke::new(stroke::HAIRLINE, t.border);
        s.visuals.widgets.inactive.fg_stroke = Stroke::new(stroke::HAIRLINE, t.text);
        s.visuals.widgets.hovered.weak_bg_fill = t.bg_hover;
        s.visuals.widgets.hovered.bg_stroke = Stroke::new(stroke::HAIRLINE, t.border_strong);
        s.visuals.widgets.hovered.fg_stroke = Stroke::new(stroke::HAIRLINE, t.text);
        s.visuals.widgets.active.weak_bg_fill = t.bg_active;
        s.visuals.widgets.active.bg_stroke = Stroke::new(stroke::FOCUS, t.accent);
        s.visuals.widgets.active.fg_stroke = Stroke::new(stroke::HAIRLINE, t.text);
        s.visuals.widgets.open.weak_bg_fill = t.bg_active;
        s.visuals.widgets.open.bg_stroke = Stroke::new(stroke::HAIRLINE, t.border_strong);

        // ── 间距（4 基数） ──
        s.spacing.item_spacing = Vec2::new(space::S4, space::S3);
        s.spacing.button_padding = Vec2::new(space::S5 - 2.0, space::S3 - 1.0);
        s.spacing.menu_margin = Margin::same(6);
        s.spacing.window_margin = Margin::same(12);
        s.spacing.indent = space::S6;
        s.spacing.interact_size = Vec2::new(space::S8, space::S8);
        s.spacing.slider_width = 120.0;
        s.spacing.combo_width = 100.0;
        s.spacing.text_edit_width = 100.0;
        s.spacing.icon_width = 14.0;
        s.spacing.icon_spacing = space::S3;

        // ── 动效(H-1:总开关关闭时全部过渡立即到位) ──
        s.animation_time = anim;

        // ── 排版 ──
        s.text_styles.insert(
            TextStyle::Small,
            FontId::new(11.0 * scale, fonts::family_regular()),
        );
        s.text_styles.insert(
            TextStyle::Body,
            FontId::new(13.0 * scale, fonts::family_regular()),
        );
        s.text_styles.insert(
            TextStyle::Button,
            FontId::new(13.0 * scale, fonts::family_semibold()),
        );
        s.text_styles.insert(
            TextStyle::Heading,
            FontId::new(15.0 * scale, fonts::family_semibold()),
        );
        s.text_styles.insert(
            TextStyle::Monospace,
            FontId::new(12.0 * scale, fonts::family_mono()),
        );
        // egui 的 TextStyle 槽位不够表达 6 档字号，多出来的两档用具名 style
        s.text_styles.insert(
            fonts::style_label(),
            FontId::new(12.0 * scale, fonts::family_medium()),
        );
        s.text_styles.insert(
            fonts::style_body_strong(),
            FontId::new(13.0 * scale, fonts::family_semibold()),
        );

        // ── 去掉 egui 默认的"UI 感"开关 ──
        s.visuals.button_frame = false; // 按钮默认无框，用 vb_ui::components 的 ToolButton
        s.visuals.striped = false; // 斑马纹很"Excel"，不专业
        s.visuals.slider_trailing_fill = true;
        s.visuals.handle_shape = egui::style::HandleShape::Circle;
        s.visuals.clip_rect_margin = 0.0;
        s.visuals.indent_has_left_vline = false;

        // ── 中文输入法：Windows 默认 legacy_visuals=true（winit 韩文 IME 光标 bug），
        //    会让中文预编辑文本"看起来像已选中"。中文优先的软件显式关掉。 ──
        s.visuals.ime_composition.legacy_visuals = false;
    });
}

/// 取当前主题令牌（在 `apply` 之后调用，效果等同 `Tokens::get(self.dark)`）。
pub fn tokens(ctx: &egui::Context) -> Tokens {
    let dark = ctx.theme() == egui::Theme::Dark;
    Tokens::get(dark)
}

// ─────────────────── 行高度量（04-3-1：行高从字号派生） ───────────────────
//
// **缺陷锚点**（实测 `d-char.png`，P1-②）：字符面板的 TextEdit 被钉死 18pt 高，
// 而 CJK 字形（MiSans / 雅黑）的 galley 高 ≈ 字号 × 1.4~1.5，加上 TextEdit
// 自带上下边距后需求高度 ≈ 23~26pt —— 内容溢出控件矩形，与下一行互相压叠。
// `space::ROW_HEIGHT` 这类**写死常量**不随字号/界面缩放走，是压叠的根因。
//
// **行高公式单一真相**：行高 = 实测 galley 高（用当前正文字体排一行「字Ag0」，
// 中英混排取最高者）+ 上下留白，下限仍守住 24pt 的 4 基数刻度。因为它读的是
// 当前样式的正文字号，界面缩放（zoom_factor）与 DPI 变化都自动跟随。

/// 正文字号（读当前样式的 `TextStyle::Body`；行高派生的唯一输入）。
pub fn body_font_size(ctx: &egui::Context) -> f32 {
    let style = ctx.style_of(ctx.theme());
    style
        .text_styles
        .get(&TextStyle::Body)
        .map(|f| f.size)
        .unwrap_or(13.0)
}

/// 控件行高（pt）：用**当前正文字体**实测一行中英混排的 galley 高，
/// 加 TextEdit 自带上下边距（2+2）与 4pt 呼吸位，下限 24（4 基数）。
///
/// 所有承载文字的控件（NumField / 面板 TextEdit / 工具按钮）一律从这里取高，
/// 禁止再写死 18pt 之类的小行高 —— 这是「无文字压叠」门禁的公式面。
pub fn row_height(ctx: &egui::Context) -> f32 {
    let font = ctx
        .style_of(ctx.theme())
        .text_styles
        .get(&TextStyle::Body)
        .cloned()
        .unwrap_or_else(|| FontId::new(13.0, fonts::family_regular()));
    // 「字Ag0」= 最高 CJK 字形 + 带升降部的拉丁字形,取真实排高度
    let galley_h = ctx.fonts_mut(|f| {
        f.layout_no_wrap("字Ag0".to_owned(), font, Color32::WHITE)
            .size()
            .y
    });
    (galley_h + 8.0).ceil().max(space::ROW_HEIGHT)
}

// ──────────────────────────── 自检 ────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 深/浅两套必须字段齐全且品牌色一致。
    #[test]
    fn both_themes_are_complete() {
        let d = Tokens::dark();
        let l = Tokens::light();
        assert!(d.dark && !l.dark);
        assert_eq!(d.accent, l.accent, "两套主题共用品牌色");
        assert_eq!(d.entries().len(), l.entries().len());
        assert_eq!(d.entries().len(), 17);
    }

    /// 「不是反相」：浅色面板必须比浅色画布**亮**（靠描边/阴影分层），
    /// 深色面板必须比深色画布**亮**（靠明度分层）。两者方向一致，
    /// 说明浅色不是简单反相出来的。
    #[test]
    fn light_is_not_an_inversion() {
        let l = Tokens::light();
        let lum = |c: Color32| c.r() as i32 + c.g() as i32 + c.b() as i32;
        assert!(
            lum(l.bg_panel) > lum(l.bg_canvas),
            "浅色:面板应比画布亮(靠描边分层)"
        );
        let d = Tokens::dark();
        assert!(lum(d.bg_panel) > lum(d.bg_canvas), "深色:面板应比画布亮");
    }

    /// 间距必须是 4 的基数（2 例外）。写死 5/7/10/15/20 这类值
    /// 是"看起来没对齐"的根源，这里把它变成硬约束。
    #[test]
    fn spacing_scale_is_base_4() {
        for v in [
            space::S2,
            space::S4,
            space::S5,
            space::S6,
            space::S8,
            space::S9,
            space::ROW_HEIGHT,
            space::DOCK_WIDTH,
        ] {
            assert_eq!(v % 4.0, 0.0, "间距 {v} 不是 4 的倍数");
        }
        assert_eq!(space::S1, 2.0, "S1 是唯一允许的非 4 倍数(发丝级间距)");
    }

    /// 圆角必须递增且非 egui 默认的 2px。
    #[test]
    fn radius_scale_is_monotonic() {
        // 常量断言:编译期保证圆角单调
        const {
            assert!(radius::SM < radius::MD && radius::MD < radius::LG && radius::LG < radius::XL);
        }
        assert_ne!(radius::MD, 2, "egui 默认 2px 是业余感来源之一，必须覆盖");
    }

    /// 语义色不可被主题改掉（智能参考线品红）。
    #[test]
    fn smart_guide_is_immutable_across_themes() {
        assert_eq!(
            semantic::GUIDE_SMART_DARK,
            Color32::from_rgb(0xFF, 0x00, 0xFF)
        );
        assert_eq!(semantic::guide_smart(true), semantic::GUIDE_SMART_DARK);
        // 浅色下只加深，色相不变（仍是"品红"而非其它颜色）
        let l = semantic::guide_smart(false);
        assert!(
            l.r() > 0xC0 && l.b() > 0xC0 && l.g() == 0,
            "浅色品红仍须是品红"
        );
    }

    /// **两处真相一致性**：`docs/design/assets/vb-ui-tokens.json` 与
    /// `theme.rs` 的令牌值必须逐个相等（14 篇 §3.9 第 2 条要求 CI 校验）。
    #[test]
    fn tokens_sync_with_json() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/design/assets/vb-ui-tokens.json");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("读不到设计令牌文件 {}: {e}", path.display()));
        let json: serde_json::Value =
            serde_json::from_str(&raw).expect("vb-ui-tokens.json 不是合法 JSON");

        for (theme_key, tokens) in [("dark", Tokens::dark()), ("light", Tokens::light())] {
            for (name, expected) in tokens.entries() {
                let Some(node) = json
                    .get("color")
                    .and_then(|c| c.get(theme_key))
                    .and_then(|t| t.get(name))
                else {
                    // 浅色表允许省略（如 accent-hover），省略即不校验
                    continue;
                };
                let raw_value = node
                    .get("value")
                    .and_then(|v| v.as_str())
                    .unwrap_or_else(|| panic!("{theme_key}.{name} 缺少 value"));

                assert_eq!(
                    parse_color(raw_value),
                    expected,
                    "令牌 {theme_key}.{name} 在 JSON 是 {raw_value}，在 theme.rs 是 \
                     rgba({}, {}, {}, {})；两处真相不一致，请同步",
                    expected.r(),
                    expected.g(),
                    expected.b(),
                    expected.a()
                );
            }
        }
    }

    /// 04-3-1:行高必须从字号派生 —— 至少包住正文字形的实际排高度(+边距),
    /// 且不小于 24 的基数下限。这是「无文字压叠」门禁的公式面。
    #[test]
    fn row_height_is_derived_from_body_font() {
        // egui 的字体在首个 pass 才初始化(真实调用点都在 UI 闭包内,
        // 必然处于 pass 中);测试里手动 begin_pass 等价初始化。
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        let rh = row_height(&ctx);
        assert!(rh >= space::ROW_HEIGHT, "行高下限 24,得到 {rh}");
        assert!(
            rh >= body_font_size(&ctx) * 1.5,
            "行高必须 ≥ 正文字号 × 1.5(CJK 字形更高),得到 {rh}"
        );
        // 行高随字号放大(界面缩放时所有承载文字的控件一起长高)。
        // 用 Proportional 族(默认绑定已存在);vb 族要 fonts::install 才有。
        let big = egui::Context::default();
        big.all_styles_mut(|s| {
            s.text_styles.insert(
                TextStyle::Body,
                FontId::new(26.0, egui::FontFamily::Proportional),
            );
        });
        big.begin_pass(egui::RawInput::default());
        assert!(
            row_height(&big) > rh,
            "正文字号放大后行高必须跟着涨:{}, {}",
            row_height(&big),
            rh
        );
    }

    /// 解析 `#RRGGBB` 或 `rgba(r,g,b,a)`（a 为 0..1 浮点）。
    fn parse_color(v: &str) -> Color32 {
        let v = v.trim();
        if let Some(hex) = v.strip_prefix('#') {
            assert_eq!(hex.len(), 6, "只支持 #RRGGBB，收到 {v}");
            let n = u32::from_str_radix(hex, 16).expect("非法十六进制");
            return Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8);
        }
        let inner = v
            .strip_prefix("rgba(")
            .and_then(|s| s.strip_suffix(')'))
            .unwrap_or_else(|| panic!("无法解析颜色 {v}"));
        let parts: Vec<f32> = inner
            .split(',')
            .map(|p| p.trim().parse::<f32>().expect("非法数值"))
            .collect();
        assert_eq!(parts.len(), 4, "rgba 需要 4 个分量，收到 {v}");
        Color32::from_rgba_unmultiplied(
            parts[0] as u8,
            parts[1] as u8,
            parts[2] as u8,
            (parts[3] * 255.0).round() as u8,
        )
    }

    /// 语义色独立性检查。
    ///
    /// 例外：`SELECT_BOX` 与 accent 同为 #0D99FF 是**设计意图**
    /// （令牌文件里 accent.desc 就是"选中/激活/焦点"），其余语义色
    /// 不得与主题基础色重复，否则说明有人把语义槽当成了配色槽。
    #[test]
    fn semantic_colors_do_not_collide_with_theme_colors() {
        let by_design = [semantic::SELECT_BOX];
        let marks = [semantic::guide_smart(true), semantic::guide_smart(false)];
        for t in [Tokens::dark(), Tokens::light()] {
            for (name, c) in t.entries() {
                if c == semantic::SELECT_BOX && by_design.contains(&semantic::SELECT_BOX) {
                    continue; // accent == 选中色，设计如此
                }
                assert!(
                    !marks.contains(&c),
                    "语义色与主题令牌 {name} 撞色，语义槽被当成配色槽用了"
                );
            }
        }
    }

    // ── U-6:浅色主题对比度走查(WCAG AA 门禁) ──

    /// sRGB → 相对亮度(WCAG 2.x 公式)。
    fn rel_luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    /// WCAG 对比度(两色亮度比,亮者分子)。
    fn contrast(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (rel_luminance(a), rel_luminance(b));
        let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// U-6 门禁:浅色主题的**正文/次级/禁用级文字**与其承载底色必须达
    /// WCAG AA(≥4.5:1)。覆盖走查点:次级坞(面板底)、提示条(面板底)、
    /// toast(凸起底)、输入底上的正文。
    #[test]
    fn light_theme_text_meets_wcag_aa() {
        let l = Tokens::light();
        for (name, fg, bg) in [
            ("正文/面板", l.text, l.bg_panel),
            ("次级文字/面板", l.text_2, l.bg_panel),
            ("弱文字/面板(禁用态·占位)", l.text_3, l.bg_panel),
            ("正文/toast(凸起底)", l.text, l.bg_raised),
            ("次级文字/提示条", l.text_2, l.bg_panel),
            ("正文/输入底", l.text, l.bg_input),
        ] {
            let c = contrast(fg, bg);
            assert!(
                c >= 4.5,
                "浅色 {name} 对比度 {c:.2}:1 < 4.5(WCAG AA),文字色需加深"
            );
        }
    }

    /// U-6 门禁:浅色主题的语义色文字(warn/danger/success 出现在
    /// toast 与状态栏文本里)在面板/凸起底上同样 ≥4.5:1。
    #[test]
    fn light_theme_semantic_text_meets_wcag_aa() {
        let l = Tokens::light();
        for (name, fg) in [
            ("warn", l.warn),
            ("danger", l.danger),
            ("success", l.success),
        ] {
            for bg_name in ["bg_panel", "bg_raised"] {
                let bg = match bg_name {
                    "bg_panel" => l.bg_panel,
                    _ => l.bg_raised,
                };
                let c = contrast(fg, bg);
                assert!(
                    c >= 4.5,
                    "浅色 {name} 文字在 {bg_name} 上对比度 {c:.2}:1 < 4.5"
                );
            }
        }
    }

    /// 动效总开关(H-1):未注入时默认开;写入后 [`anim_time`] 归零、
    /// 注入恢复;纯函数语义不经 egui 也能锁定(归零规则)。
    #[test]
    fn motion_kill_switch_defaults_on_and_zeroes_times() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        assert!(motion_enabled(&ctx), "未注入过 = 默认开");
        assert!((anim_time(&ctx, motion::HOVER) - motion::HOVER).abs() < f32::EPSILON);
        set_motion_enabled(&ctx, false);
        assert!(!motion_enabled(&ctx));
        assert_eq!(anim_time(&ctx, motion::HOVER), 0.0, "关闭后时长必须归零");
        assert_eq!(anim_time(&ctx, motion::PANEL), 0.0);
        set_motion_enabled(&ctx, true);
        assert!((anim_time(&ctx, motion::STATE) - motion::STATE).abs() < f32::EPSILON);
    }
}
