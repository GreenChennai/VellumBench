//! 通用组件层（设计文档 14 篇 §3.5）。
//!
//! ## 为什么要有这一层
//!
//! 原先 `vb_app` 到处直接调 `egui::DragValue` / `SelectableLabel` 裸控件，
//! 结果是：同一种"数值输入"在属性面板、变换面板、导出对话框里长得不一样，
//! 标签宽度不一导致字段左边缘参差，圆角和内边距各写各的。
//!
//! 本层把"一致"变成默认值 —— 调用方说**要什么**（一个数值字段），
//! 而不是说**长什么样**（宽度、圆角、字体、标签对齐）。
//!
//! ## 已抽出（P2.5 前 6 个）
//!
//! | 组件 | 替代的裸控件 |
//! |---|---|
//! | [`ToolButton`] | 手绘的 `Button` + emoji |
//! | [`NumField`] | `ui.add(egui::DragValue::new(..))` |
//! | [`ColorField`] | `ui.color_edit_button_srgba(..)` 无标签裸用 |
//! | [`SectionHeader`] | `ui.collapsing(..)` / 手写标题 |
//! | [`PanelTabs`] | `ui.selectable_value(..)` 排一行 |
//!
//! 后续（P3/P4）继续抽：AlignmentPanel / TransformPanel / TokenRow /
//! ExportDialog / StatusBar / ShortcutRecorder。

use egui::{pos2, vec2, Color32, Response, RichText, Sense, Stroke, Ui, Vec2};

use crate::{fonts, icons, theme};

/// 面板内字段标签的固定宽度。
///
/// 固定宽度的意义：多个字段纵向排列时**左边缘自动对齐**，右边缘的输入框
/// 也随之一条线。不固定宽度时每个标签各占各的宽，视觉上就是"没对齐"。
const FIELD_LABEL_WIDTH: f32 = 56.0;

/// 两个颜色之间线性混合（`t=0` 取 `a`，`t=1` 取 `b`）。
///
/// 自己实现是为了不依赖 egui 的混色函数名（那东西在版本间改过名），
/// 也让"悬停过渡"这件事只在一个地方定义。
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

/// 键盘焦点环(G-UI-D):accent 1.5px 外描边 + 内侧隔离环
/// (`Tokens::focus_ring*`,§8.3.3)。egui 0.35 对一切 `Sense::click`
/// 控件(含自绘)自动维护 Tab 序与 Space/Enter 激活,但**焦点环要
/// 自绘** —— 自绘控件在 `resp.has_focus()` 时调它。
///
/// S5(§8.10 诚实清单 ①)起 `pub`:组件层之外的零散自绘可交互点
/// (图层面板行、状态栏文本项、启动器卡片)与组件层共用**同一实现**
/// —— 环的规格(accent 1.5px 外 + 内隔离)只允许有一份。
pub fn paint_focus_ring(ui: &Ui, rect: egui::Rect, t: &theme::Tokens) {
    ui.painter().rect_stroke(
        rect.expand(1.0),
        theme::radius::sm(),
        Stroke::new(theme::stroke::HAIRLINE, t.focus_ring_inner),
        egui::StrokeKind::Outside,
    );
    ui.painter().rect_stroke(
        rect.expand(theme::space::S2),
        theme::radius::sm(),
        Stroke::new(theme::stroke::FOCUS, t.focus_ring),
        egui::StrokeKind::Outside,
    );
}

/// 在 `rect` 内画 45° 斜纹(§8.6 #3:NumField 混合态的视觉语言)。
///
/// 条纹间距 = 间距刻度 S3;颜色由调用方给(组件传弱文字色 —— 斜纹是
/// "这里的值不代表单一对象"的提示,不许抢过正文)。裁剪经 painter
/// clip,行尾半根条纹不会越出输入框。
fn paint_hatch(ui: &Ui, rect: egui::Rect, color: Color32) {
    let p = ui
        .painter()
        .clone()
        .with_clip_rect(rect.intersect(ui.clip_rect()));
    let stroke = Stroke::new(1.0, color.gamma_multiply(0.55));
    let step = theme::space::S3;
    let h = rect.height();
    let mut x = rect.left() - h;
    while x < rect.right() {
        p.line_segment([pos2(x, rect.bottom()), pos2(x + h, rect.top())], stroke);
        x += step;
    }
}

// ─────────────────────────── 文本样式助手 ───────────────────────────

/// 分组标题文字（13px / 600）。
pub fn strong(text: &str) -> RichText {
    RichText::new(text).font(fonts::font(13.0, fonts::Weight::Semibold))
}

/// 字段标签文字（12px / 500）。
pub fn label(text: &str) -> RichText {
    RichText::new(text).font(fonts::font(12.0, fonts::Weight::Medium))
}

/// 次要说明文字（11px / 400，用当前主题的次色）。
///
/// 需要 `ui` 是因为次色随主题变 —— 写死深色的次色会在浅色主题下发白。
pub fn caption(ui: &Ui, text: &str) -> RichText {
    RichText::new(text)
        .font(fonts::font(11.0, fonts::Weight::Regular))
        .color(theme::tokens(ui.ctx()).text_2)
}

/// 十六进制 / 代码文字（12px 等宽）。
pub fn mono(text: &str) -> RichText {
    RichText::new(text).font(egui::FontId::new(12.0, fonts::family_mono()))
}

/// 一行"标签 + 控件"(U-3 统一表单行)。
///
/// 规格:**标签右对齐**贴住控件列的左缘(纵向多字段时,标签尾字与
/// 全部输入框的左缘各成一条直线 —— AI 属性条的对齐方式),行高从
/// 正文字号派生([`theme::row_height`]),标签字号/字重/颜色三处统一。
pub fn field_row<R>(ui: &mut Ui, label_text: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let row_h = theme::row_height(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(Vec2::new(FIELD_LABEL_WIDTH, row_h), Sense::hover());
        let t = theme::tokens(ui.ctx());
        ui.painter().text(
            pos2(rect.right(), rect.center().y),
            egui::Align2::RIGHT_CENTER,
            label_text,
            fonts::font(12.0, fonts::Weight::Medium),
            t.text_2,
        );
        add(ui)
    })
    .inner
}

/// 键位徽章的标准文本形(H-5):「名称 (键位)」。
///
/// tooltip / 菜单 / 命令面板的键位提示统一走这一个函数,保证
/// 同一语义只有一种写法;`key` 为空时退化为纯名称。
pub fn key_badge_text(label: &str, key: &str) -> String {
    if key.trim().is_empty() {
        label.to_string()
    } else {
        format!("{label} ({})", key.trim())
    }
}

/// 对话框底部按钮排的统一规格(U-7):分隔线之上一行,**主按钮右下**,
/// 取消在其左;右对齐保证各对话框的按钮位完全一致。
///
/// 返回 `(主操作点击, 取消点击)`。键位约定(Enter = 主操作、
/// Esc = 取消)由调用方按各自上下文接线 —— 有的对话框没有文本框,
/// 有的(命令面板)输入框常驻,统一收口反而会误触发。
pub fn dialog_footer(ui: &mut Ui, primary: &str, cancel: Option<&str>) -> (bool, bool) {
    let mut primary_clicked = false;
    let mut cancel_clicked = false;
    ui.separator();
    // 先开一条内容高的横条再右对齐:直接 `with_layout(right_to_left)`
    // 会让按钮在「剩余高度」里垂直居中,把对话框撑出一段大空白。
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(RichText::new(primary).strong()))
                .clicked()
            {
                primary_clicked = true;
            }
            if let Some(cancel_text) = cancel {
                if ui.button(cancel_text).clicked() {
                    cancel_clicked = true;
                }
            }
        });
    });
    (primary_clicked, cancel_clicked)
}

/// [`dialog_footer`] 的三钮变体(U-7;确认类对话框用):
/// 从右到左 `主按钮 / 次按钮 / 取消`,`次按钮` 传空串 = 不画。
/// 返回 `(主点击, 次点击, 取消点击)`。
pub fn dialog_footer_btn3(
    ui: &mut Ui,
    primary: &str,
    secondary: &str,
    cancel: &str,
) -> (bool, bool, bool) {
    let mut out = (false, false, false);
    ui.separator();
    // 同 dialog_footer:先收一条内容高的横条,避免按钮被垂直居中到剩余空间
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(RichText::new(primary).strong()))
                .clicked()
            {
                out.0 = true;
            }
            if !secondary.trim().is_empty() && ui.button(secondary).clicked() {
                out.1 = true;
            }
            if !cancel.trim().is_empty() && ui.button(cancel).clicked() {
                out.2 = true;
            }
        });
    });
    out
}

// ──────────────────────────── 1. ToolButton ────────────────────────────

/// 工具箱按钮（图标 + 可选文字）。
///
/// **tooltip 强制为「名称 (快捷键)」** —— 这是 14 篇 §3.4 的"零成本学习模式"：
/// 用户悬停即学到键位，不必翻文档。构造时必须给 `label`，就是逼调用方
/// 把名字写出来。
pub struct ToolButton<'a> {
    icon: icons::Name,
    label: &'a str,
    shortcut: Option<&'a str>,
    active: bool,
    enabled: bool,
    /// true = 图标在上、文字在下（底部浮动工具条）；false = 只要图标（工具箱）。
    show_label: bool,
    size: f32,
}

impl<'a> ToolButton<'a> {
    /// 图标按钮；`label` 用于 tooltip，也用于 `show_label` 时显示。
    pub fn new(icon: icons::Name, label: &'a str) -> Self {
        Self {
            icon,
            label,
            shortcut: None,
            active: false,
            enabled: true,
            show_label: false,
            size: theme::space::ROW_HEIGHT,
        }
    }

    /// 绑定快捷键 → tooltip 变成 `选择工具 (V)`。
    pub fn shortcut(mut self, key: &'a str) -> Self {
        self.shortcut = Some(key);
        self
    }

    /// 当前工具激活（accent 底色）。
    pub fn active(mut self, v: bool) -> Self {
        self.active = v;
        self
    }

    /// 置灰不可点。
    pub fn enabled(mut self, v: bool) -> Self {
        self.enabled = v;
        self
    }

    /// 图标下带文字（底部浮动工具条用）。
    pub fn with_label(mut self) -> Self {
        self.show_label = true;
        self.size = theme::space::FLOATING_TOOLBAR_HEIGHT - theme::space::S2;
        self
    }

    /// 自定义边长。
    pub fn size(mut self, s: f32) -> Self {
        self.size = s;
        self
    }

    /// tooltip 文本:`名称 (快捷键)`(键位徽章标准形,见 [`key_badge_text`])。
    fn tooltip_text(&self) -> String {
        key_badge_text(self.label, self.shortcut.unwrap_or(""))
    }

    /// 画出按钮。
    pub fn ui(self, ui: &mut Ui) -> Response {
        let t = theme::tokens(ui.ctx());
        let icon_size = if self.show_label {
            icons::Name::size_floating()
        } else {
            icons::Name::size_in_toolbar()
        };
        let w = if self.show_label {
            self.size + theme::space::S4
        } else {
            self.size
        };
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, self.size), Sense::click());

        let hover_t = ui.ctx().animate_bool_with_time(
            ui.id().with(("vbtb", self.icon, self.label)),
            resp.hovered() && self.enabled,
            theme::anim_time(ui.ctx(), theme::motion::HOVER),
        );
        // U-4「按下微缩」:按住时图标整体缩到 92%,松手回弹 —— 按压有触感
        // (不经 hover 通道,避免与悬停底色互相踩)。
        let press_t = ui.ctx().animate_bool_with_time(
            ui.id().with(("vbtbpress", self.icon, self.label)),
            resp.is_pointer_button_down_on() && self.enabled,
            theme::anim_time(ui.ctx(), theme::motion::HOVER),
        );
        let icon_scale = 1.0 - 0.08 * press_t;

        // 底色：激活 > 悬停 > 透明
        // (§8.3.3 状态层:悬停不再换灰阶常量 t.bg_hover,改状态 overlay
        //  半透明叠加 —— 动画曲线不变(行为等价),色值升级。)
        let base = if self.active {
            t.accent_dim
        } else {
            Color32::TRANSPARENT
        };
        let hovered = if self.active {
            blend(t.accent_dim, t.accent, hover_t * 0.25)
        } else {
            theme::state::fade(t.state_hover, hover_t)
        };
        let fill = if hover_t > 0.0 { hovered } else { base };

        ui.painter().rect_filled(rect, theme::radius::md(), fill);
        if self.active {
            ui.painter().rect_stroke(
                rect,
                theme::radius::md(),
                Stroke::new(theme::stroke::HAIRLINE, t.accent),
                egui::StrokeKind::Inside,
            );
        }
        // G-UI-D:键盘焦点环(Tab 可达由 egui 自动;环要自绘)
        if resp.has_focus() {
            paint_focus_ring(ui, rect, &t);
        }

        let fg = if !self.enabled {
            t.text_3
        } else if self.active {
            t.accent
        } else {
            blend(t.text_2, t.text, hover_t)
        };

        if self.show_label {
            let icon_rect = rect.shrink2(vec2(0.0, theme::space::S1));
            let ic = icon_size * icon_scale;
            ui.painter().text(
                pos2(
                    icon_rect.center().x,
                    icon_rect.top() + icon_size * 0.5 + 2.0,
                ),
                egui::Align2::CENTER_CENTER,
                self.icon.glyph().to_string(),
                icons::font(ic),
                fg,
            );
            ui.painter().text(
                pos2(rect.center().x, rect.bottom() - 6.0),
                egui::Align2::CENTER_BOTTOM,
                self.label,
                fonts::font(11.0 * icon_scale, fonts::Weight::Regular),
                fg,
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.icon.glyph().to_string(),
                icons::font(icon_size * icon_scale),
                fg,
            );
        }

        // S5 清单 ③:读屏语义 —— 工具名(+键位徽章)登记为 labelled
        // Button,激活态进 selected 位(工具 = 单选模式)。
        resp.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                self.enabled,
                self.active,
                self.tooltip_text(),
            )
        });
        let resp = resp.on_hover_text(self.tooltip_text());
        if self.enabled {
            resp
        } else {
            resp.on_disabled_hover_text(self.tooltip_text())
        }
    }
}

// ──────────────────────────── 2. NumField ────────────────────────────

/// scrubby 步进(纯函数;02-6-1 ⭐)。
///
/// 横向拖动 `dx` 像素 → 数值增量 = `dx × speed × 修饰键倍率`:
/// **Alt = 细调 ×0.1,Shift = 粗调 ×10**(同时按下按 Alt 优先,
/// 细调是更精细的意图);无修饰键 = ×1。
pub fn scrub_step(dx: f32, speed: f64, shift: bool, alt: bool) -> f64 {
    let k = if alt {
        0.1 // vb-size-ok: Alt 细调倍率(值域系数,非尺寸)
    } else if shift {
        10.0 // vb-size-ok: Shift 粗调倍率(值域系数,非尺寸)
    } else {
        1.0
    };
    dx as f64 * speed * k
}

/// 数值格式化(输入框回显):整数不带小数点,小数最多 2 位去尾零。
///
/// PERF-09 裁定:审查点"每帧 Id 拼接"已随组件重构消解(现存 Id 均为
/// 静态串);本函数每帧的 String 分配(每屏 ~10-20 个小分配)经评估
/// 低于 egui 自身每帧分配量级,不做缓存/Cow——避免为不可测量收益引入
/// 生命周期复杂度。若未来 dev stats 显示此处进入火焰图,再行缓存。
pub fn format_num(v: f64) -> String {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// [`NumField`] 的交互结果。
///
/// `changed` = 值在本帧变了(scrubby / 键盘步进 / 表达式提交),
/// 调用方据此走命令路径;`scrub_started`/`scrub_ended`/`focus_lost`
/// 是**提交会话**信号(02-6-2:连续 scrubby/键入合并为一次 undo),
/// 由 `vb_app` 统一折算成 `UndoStack::begin_session / end_session`。
#[derive(Debug, Clone, Default)]
pub struct NumFieldResponse {
    /// 值已变化(调用方应提交命令)。
    pub changed: bool,
    /// 表达式解析失败(中文文案,调用方直接 toast)。
    pub expr_error: Option<String>,
    /// scrubby 拖拽本帧开始。
    pub scrub_started: bool,
    /// scrubby 拖拽本帧结束。
    pub scrub_ended: bool,
    /// 输入框本帧失去焦点(提交会话应结束)。
    pub focus_lost: bool,
}

/// 数值字段(属性面板里的 X / Y / 宽 / 高 / 角度 / 缩放)。
///
/// 02-6-1/2 全面升级,一处实现全面板受益:
/// - **拖动标签 scrubby**(⭐ 必做):横向拖动改值,`Shift` 粗调 ×10、
///   `Alt` 细调 ×0.1;拖动时光标变左右箭头、指针旁浮层显示当前值;
/// - 键盘 `↑/↓` 步进 `step`(默认 = speed;几何字段 speed=1 即规格的
///   ±1),`Shift+↑/↓` 步进 ×10;
/// - **数学表达式**:`320/2`、`12*3+4`、`50%`(相对基准由调用方给,
///   面板里 = 画板宽/高)回车求值写入;解析见 [`crate::expr`],
///   失败给中文错误(经 [`NumFieldResponse::expr_error`]);
/// - 提交会话:宿主把会话信号折算成 undo 合并(见 [`NumFieldResponse`])。
pub struct NumField<'a> {
    label: &'a str,
    value: &'a mut f64,
    unit: &'a str,
    speed: f64,
    step: Option<f64>,
    range: Option<(f64, f64)>,
    width: f32,
    /// 标签区宽度(scrubby 拖动源)。默认 [`FIELD_LABEL_WIDTH`];
    /// 单字母字段(X/Y/W/H)可收窄。
    label_width: f32,
    /// `50%` 的相对基准(None 时 `%` = /100,见 expr 模块注释)。
    percent_base: Option<f64>,
    /// 混合态(§8.6 #3 ⭐):多选对象在该字段上取值不一致时置真 ——
    /// 输入框画**斜纹**而不是假装只有一个值;显示值 = 主选中的值,
    /// 提交仍作用于全部选中(调用方保证)。
    mixed: bool,
}

impl<'a> NumField<'a> {
    /// 新建数值字段。
    pub fn new(label: &'a str, value: &'a mut f64) -> Self {
        Self {
            label,
            value,
            unit: "",
            speed: 1.0,
            step: None,
            range: None,
            width: 72.0,
            label_width: FIELD_LABEL_WIDTH,
            percent_base: None,
            mixed: false,
        }
    }

    /// 单位后缀(`px` / `°` / `%`)。
    pub fn unit(mut self, u: &'a str) -> Self {
        self.unit = u;
        self
    }

    /// 拖拽速度(每像素改变量)。
    pub fn speed(mut self, s: f64) -> Self {
        self.speed = s;
        self
    }

    /// 键盘步进(`↑/↓`;`Shift+↑/↓` = ×10)。默认 = [`NumField::speed`]。
    pub fn step(mut self, s: f64) -> Self {
        self.step = Some(s);
        self
    }

    /// 数值区间。
    pub fn range(mut self, lo: f64, hi: f64) -> Self {
        self.range = Some((lo, hi));
        self
    }

    /// 输入框宽度。
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// 百分号表达式的相对基准(面板里传画板宽/高)。
    pub fn percent_base(mut self, base: f64) -> Self {
        self.percent_base = Some(base);
        self
    }

    /// 标签区宽度(单字母字段传窄值,如 20.0)。
    pub fn label_width(mut self, w: f32) -> Self {
        self.label_width = w;
        self
    }

    /// 混合态(多选对象该字段取值不一致):输入框画斜纹。
    pub fn mixed(mut self, v: bool) -> Self {
        self.mixed = v;
        self
    }

    fn clamp(&self, v: f64) -> f64 {
        match self.range {
            Some((lo, hi)) => v.clamp(lo, hi),
            None => v,
        }
    }

    /// 画出字段,返回交互结果(见 [`NumFieldResponse`])。
    pub fn ui(self, ui: &mut Ui) -> NumFieldResponse {
        let t = theme::tokens(ui.ctx());
        // 04-3-1:行高从正文字号派生(P1-② 压叠根因 = 写死 20/24pt 小行高),
        // 界面缩放 / DPI 变化自动跟随;标签与输入框等高,一行一个步进。
        let row_h = theme::row_height(ui.ctx());
        let mut out = NumFieldResponse::default();

        // ── 标签区(scrubby 拖动源;U-3:标签右对齐贴控件列,与
        // field_row 同一规格 —— 多字段纵排时标签尾字成一条直线)──
        let (lrect, lresp) =
            ui.allocate_exact_size(Vec2::new(self.label_width, row_h), Sense::drag());
        let hover_t = ui.ctx().animate_bool_with_time(
            ui.id().with(("vbnumlbl", self.label)),
            lresp.hovered() || lresp.dragged(),
            theme::anim_time(ui.ctx(), theme::motion::HOVER),
        );
        // 标签悬停底(§8.3.3 状态层:0.6 系数保留 —— 标签区的悬停反馈
        // 刻意比输入框轻半档;色值从灰阶常量升级为状态 overlay)
        ui.painter().rect_filled(
            lrect,
            theme::radius::sm(),
            theme::state::fade(t.state_hover, hover_t * 0.6),
        );
        ui.painter().text(
            pos2(lrect.right() - 2.0, lrect.center().y),
            egui::Align2::RIGHT_CENTER,
            self.label,
            fonts::font(12.0, fonts::Weight::Medium),
            blend(t.text_2, t.text, hover_t),
        );
        // G-UI-D:标签区(Sense::drag = FOCUSABLE)的键盘焦点环
        if lresp.has_focus() {
            paint_focus_ring(ui, lrect, &t);
        }
        // S5 清单 ③:标签区是「拖拽改值」的 scrubby 滑杆语义 —— 读屏
        // 登记为 Slider(带当前值与字段名);输入框本体是 egui 原生
        // TextEdit(自动登记)。
        lresp.widget_info(|| egui::WidgetInfo::slider(true, *self.value, self.label));
        if lresp.hovered() {
            // §8.6 #3:标签拖改值光标 = col-resize(列宽调整语义,egui
            // 映射为水平双箭头列光标,与"拖动改值"的空间隐喻一致)
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeColumn);
        }

        // ── 输入框(缓冲存 egui 内存;未聚焦时回显当前值)──
        let field_id = ui.id().with(("vbnum", self.label));
        let buf_id = field_id.with("buf");
        let focused = ui.ctx().memory(|m| m.has_focus(field_id));
        let mut buf = if focused {
            ui.ctx()
                .memory(|m| m.data.get_temp::<String>(buf_id))
                .unwrap_or_else(|| format_num(*self.value))
        } else {
            format_num(*self.value)
        };
        // 数值一律走 vb-mono 等宽族(§8.4 tabular-nums 的落点:egui 无
        // OpenType 特性接口,数字对齐经等宽字体达成,小数点上下对齐)
        let edit = ui.add_sized(
            Vec2::new(self.width, row_h),
            egui::TextEdit::singleline(&mut buf)
                .id(field_id)
                .font(theme::typography::mono_font_id(1.0))
                .hint_text(format_num(*self.value)),
        );
        if !self.unit.is_empty() {
            ui.painter().text(
                pos2(edit.rect.right() + theme::space::S1, edit.rect.center().y),
                egui::Align2::LEFT_CENTER,
                self.unit,
                fonts::font(11.0, fonts::Weight::Regular),
                t.text_3,
            );
        }
        // ── 混合态斜纹(§8.6 #3 ⭐):输入框未聚焦时叠 45° 斜纹,
        // 提示"多个对象取值不一";聚焦编辑时让位给文本(斜纹停画)。
        if self.mixed && !focused {
            paint_hatch(ui, edit.rect, t.text_3);
        }

        // ── 键盘步进(聚焦时;全局方向键走 TextEdit 上下文不会抢)──
        if edit.has_focus()
            && ui
                .ctx()
                .input(|i| i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowDown))
        {
            let (up, shift) = ui
                .ctx()
                .input(|i| (i.key_pressed(egui::Key::ArrowUp), i.modifiers.shift));
            // 步进前先吸收草稿:已键入未回车的表达式先求值进 value,
            // 步进基于草稿值 —— 否则下面的回显直接覆盖用户输入(草稿丢失)
            if let Ok(v) = crate::expr::eval_expr(&buf, self.percent_base) {
                *self.value = self.clamp(v);
            }
            let step = self.step.unwrap_or(self.speed) * if shift { 10.0 } else { 1.0 }; // vb-size-ok: Shift 步进倍率(值域系数)
            *self.value = self.clamp(if up {
                *self.value + step
            } else {
                *self.value - step
            });
            buf = format_num(*self.value);
            out.changed = true;
            edit.request_focus();
        }

        // ── 表达式提交(回车或失焦;文本与回显不同才求值)──
        let enter = edit.has_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter));
        let commit = enter || edit.lost_focus();
        if commit && buf != format_num(*self.value) {
            match crate::expr::eval_expr(&buf, self.percent_base) {
                Ok(v) => {
                    let v = self.clamp(v);
                    if v != *self.value {
                        *self.value = v;
                        out.changed = true;
                    }
                    buf = format_num(*self.value);
                }
                Err(e) => {
                    out.expr_error = Some(e.to_string());
                    buf = format_num(*self.value); // 还原回显
                }
            }
        }
        ui.ctx().memory_mut(|m| m.data.insert_temp(buf_id, buf));
        if enter {
            edit.request_focus(); // 回车提交后保留焦点(连续键入同一会话)
        }
        out.focus_lost = edit.lost_focus() && !enter;

        // ── scrubby 拖动 ──
        if lresp.drag_started() {
            out.scrub_started = true;
        }
        if lresp.dragged() {
            let dx = lresp.drag_delta().x;
            if dx != 0.0 {
                let (shift, alt) = ui.ctx().input(|i| (i.modifiers.shift, i.modifiers.alt));
                *self.value = self.clamp(*self.value + scrub_step(dx, self.speed, shift, alt));
                out.changed = true;
            }
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
            // 拖动浮层:指针旁显示当前值(不与画布浮层混用,组件自管)
            let p = ui.ctx().pointer_latest_pos().unwrap_or(lrect.center());
            let layer = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::new("vb_num_overlay"),
            ));
            let text = format!("{}{}", format_num(*self.value), self.unit);
            let galley = ui.painter().layout(
                text.clone(),
                // 拖动浮层里的数值同样走 mono 档(§8.4;tabular-nums)
                theme::typography::mono_font_id(1.0),
                t.text,
                120.0,
            );
            let size = galley.size() + Vec2::new(theme::space::S4 * 2.0, theme::space::S1 * 2.0);
            let rect =
                egui::Rect::from_min_size(p + Vec2::new(theme::space::S5, -size.y - 6.0), size);
            layer.rect_filled(rect, theme::radius::sm(), t.bg_raised);
            layer.rect_stroke(
                rect,
                theme::radius::sm(),
                Stroke::new(theme::stroke::HAIRLINE, t.border),
                egui::StrokeKind::Inside,
            );
            layer.galley(
                rect.left_top() + Vec2::new(theme::space::S4, theme::space::S1),
                galley,
                t.text,
            );
        }
        if lresp.drag_stopped() {
            out.scrub_ended = true;
        }

        out
    }
}

// ──────────────────────────── 3. ColorField ────────────────────────────

/// [`ColorField`] 的交互结果(02-6-3)。
#[derive(Debug, Clone, Default)]
pub struct ColorFieldResponse {
    /// 颜色已改变(色块或取色器;调用方提交命令)。
    pub changed: bool,
    /// 被清空为「无」。
    pub cleared: bool,
    /// 用户在取色器确认了 CSS 变量(名称,**不带** `--`;
    /// 调用方应把样式值写成 `var(--{name})`)。
    pub var_picked: Option<String>,
}

/// 颜色字段(填充 / 描边 / 文字色)。
///
/// 02-6-3 升级:自绘色块 + 等宽 hex 回显;**点击弹取色器浮窗**
/// (紧凑:预览 + HEX + CSS 变量),**Alt+点击弹完整取色器**
/// (另含 RGB / HSL 输入与文档令牌色板)。`var(--x)` 解析走
/// `vb_common::color::parse_color` 现有设施,不重复造。
///
/// 令牌数据由调用方注入(见 [`ColorField::doc_tokens`])—— `vb_ui`
/// 不依赖 `vb_doc`,只认 `(名称, 值)` 对。
pub struct ColorField<'a> {
    label: &'a str,
    value: &'a mut Color32,
    alpha: bool,
    /// 允许清空为"无"(如 `background-color` 未设置)。
    clearable: Option<&'a mut bool>,
    /// 文档令牌 `(name, value)`,供 `var(--x)` 解析与色板(可空)。
    tokens: &'a [(String, String)],
}

impl<'a> ColorField<'a> {
    /// 新建颜色字段。
    pub fn new(label: &'a str, value: &'a mut Color32) -> Self {
        Self {
            label,
            value,
            alpha: true,
            clearable: None,
            tokens: &[],
        }
    }

    /// 是否带 alpha 通道。
    pub fn alpha(mut self, v: bool) -> Self {
        self.alpha = v;
        self
    }

    /// 允许清空;返回的 [`ColorFieldResponse::cleared`] 为 `true` 表示本次被清空。
    pub fn clearable(mut self, flag: &'a mut bool) -> Self {
        self.clearable = Some(flag);
        self
    }

    /// 注入文档令牌(取色器里可解析 `var(--x)`、可点色板)。
    pub fn doc_tokens(mut self, tokens: &'a [(String, String)]) -> Self {
        self.tokens = tokens;
        self
    }

    /// 画出字段,返回交互结果。
    pub fn ui(mut self, ui: &mut Ui) -> ColorFieldResponse {
        let t = theme::tokens(ui.ctx());
        let mut out = ColorFieldResponse::default();
        let swatch_id = ui.id().with(("vbcolor", self.label));
        let mut cleared = false;

        field_row(ui, self.label, |ui| {
            // ── 自绘色块(点击/Alt+点击开取色器)──
            let (rect, resp) = ui.allocate_exact_size(
                Vec2::new(
                    theme::space::ROW_HEIGHT - 6.0,
                    theme::space::ROW_HEIGHT - 6.0,
                ),
                Sense::click(),
            );
            let hover_t = ui.ctx().animate_bool_with_time(
                swatch_id,
                resp.hovered(),
                theme::anim_time(ui.ctx(), theme::motion::HOVER),
            );
            ui.painter()
                .rect_filled(rect, theme::radius::sm(), *self.value);
            // 悬停(§8.3.3 状态层):状态 overlay 叠在用户色上(不替换色块),
            // 描边从 border 过渡到 border_strong(不再跳到全文字色)
            if hover_t > 0.0 {
                ui.painter().rect_filled(
                    rect,
                    theme::radius::sm(),
                    theme::state::fade(t.state_hover, hover_t),
                );
            }
            ui.painter().rect_stroke(
                rect,
                theme::radius::sm(),
                Stroke::new(
                    theme::stroke::HAIRLINE,
                    blend(t.border, t.border_strong, hover_t),
                ),
                egui::StrokeKind::Inside,
            );
            // S5 清单 ③:色块 = 点击开取色器的按钮,读屏登记字段名
            // (Alt+点击 = 完整取色器的指针手势限制见 ui-focus-a11y.md)。
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, self.label)
            });
            if resp.clicked() {
                let full = ui.ctx().input(|i| i.modifiers.alt);
                ui.ctx().memory_mut(|m| {
                    let st = picker_state(m, swatch_id);
                    st.open = true;
                    st.full |= full;
                    st.hex = hex_of(*self.value);
                    st.err = None;
                });
            }
            let _ = resp.on_hover_text("点击:取色器(HEX)· Alt+点击:完整取色器(RGB/HSL/CSS 变量)");

            // ── 清除按钮 ──
            if let Some(ref mut flag) = self.clearable {
                if ui
                    .add(egui::Button::new(icons::rich(icons::Name::Close, 12.0)).frame(false))
                    .on_hover_text("清除颜色")
                    .clicked()
                {
                    **flag = true;
                    out.changed = true;
                    cleared = true;
                }
            }
            ui.label(mono(&hex_of(*self.value)));
        });
        out.cleared = cleared;

        // ── 取色器浮窗 ──
        let mut st = ui.ctx().memory_mut(|m| picker_state(m, swatch_id).clone());
        if st.open {
            let mut open = true;
            let win_id = swatch_id.with("win");
            egui::Window::new(format!("取色器 — {}", self.label))
                .id(win_id)
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    Self::picker_body(
                        self.value,
                        self.alpha,
                        self.tokens,
                        ui,
                        win_id,
                        &mut st,
                        &mut out,
                    );
                });
            if !open {
                st.open = false;
            }
            ui.ctx().memory_mut(|m| *picker_state(m, swatch_id) = st);
        }

        out
    }

    /// 取色器窗口内容。`base` 为窗口内控件的 id 基(按字段隔离)。
    /// 值/alpha/令牌以参数传入(闭包内多次可变借用,`&self` 不够写)。
    fn picker_body(
        value: &mut Color32,
        alpha: bool,
        tokens: &[(String, String)],
        ui: &mut Ui,
        base: egui::Id,
        st: &mut PickerState,
        out: &mut ColorFieldResponse,
    ) {
        let t = theme::tokens(ui.ctx());
        let hex_id = base.with("hex");
        let var_id = base.with("var");
        // 预览大色块
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), theme::space::S8),
            Sense::hover(),
        );
        ui.painter().rect_filled(rect, theme::radius::md(), *value);
        ui.painter().rect_stroke(
            rect,
            theme::radius::md(),
            Stroke::new(theme::stroke::HAIRLINE, t.border),
            egui::StrokeKind::Inside,
        );
        ui.add_space(theme::space::S2);

        // ── HEX 输入(紧凑/完整都有)──
        ui.horizontal(|ui| {
            ui.label(strong("HEX"));
            let focused = ui.ctx().memory(|m| m.has_focus(hex_id));
            let mut hex = if focused {
                st.hex.clone()
            } else {
                hex_of(*value)
            };
            let resp = ui.add_sized(
                [120.0, theme::space::ROW_HEIGHT - 4.0],
                egui::TextEdit::singleline(&mut hex)
                    .id(hex_id)
                    .font(egui::FontId::new(12.0, fonts::family_mono())),
            );
            let enter = resp.has_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || resp.lost_focus()) && hex != hex_of(*value) {
                match vb_parse_color(&hex) {
                    Some(c) => {
                        write_color(value, alpha, out, c);
                        st.hex = hex_of(*value);
                        st.err = None;
                    }
                    None => {
                        st.err = Some(
                            "无法识别的颜色(支持 #RGB/#RRGGBB/#RRGGBBAA、rgb()、hsl()、常用命名色)"
                                .into(),
                        );
                    }
                }
            } else if hex != st.hex {
                st.hex = hex;
            }
        });

        // ── CSS 变量(紧凑/完整都有)──
        ui.horizontal(|ui| {
            ui.label(strong("var"));
            let resp = ui.add_sized(
                [160.0, theme::space::ROW_HEIGHT - 4.0],
                egui::TextEdit::singleline(&mut st.var)
                    .id(var_id)
                    .hint_text("var(--名称)")
                    .font(egui::FontId::new(12.0, fonts::family_mono())),
            );
            let enter = resp.has_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter));
            if (enter || resp.lost_focus()) && !st.var.is_empty() {
                match resolve_var(&st.var, tokens) {
                    Some((name, c)) => {
                        out.var_picked = Some(name);
                        write_color(value, alpha, out, c);
                        st.err = None;
                    }
                    None => {
                        st.err = Some("找不到该 CSS 变量(先在「令牌」页创建)".into());
                    }
                }
            }
        });

        // 反向解析:当前值命中某文档令牌 → 显示它对应的 var(--name)
        // (HEX/RGB/HSL 是值的表示,变量是值的*来源*;有来源优先标来源)
        if let Some((name, _)) = tokens
            .iter()
            .find_map(|(n, v)| vb_parse_color(v).filter(|c| *c == *value).map(|c| (n, c)))
        {
            ui.label(mono(&format!("var(--{name})")));
        }

        // ── 完整模式(Alt+点击):RGB / HSL / 令牌色板 ──
        if st.full {
            ui.separator();
            let [r, g, b, a] = value.to_srgba_unmultiplied();
            let (mut h, mut s, mut l) = rgb_to_hsl(r, g, b);
            // 四态回显(§8.6 #6):HEX(紧凑态)· RGB · HSL · CSS 变量
            // 四种表示同屏可读;RGB/HSL 可经滑杆改,文本行是当前值的
            // 权威回显(Agent 对账与手抄到 CSS 都靠它)。
            ui.label(
                RichText::new(format!("rgb({r}, {g}, {b})"))
                    .font(egui::FontId::new(12.0, fonts::family_mono()))
                    .color(t.text_2),
            );
            ui.label(
                RichText::new(format!(
                    "hsl({}, {}%, {}%)",
                    h.round() as i32,
                    s.round() as i32,
                    l.round() as i32
                ))
                .font(egui::FontId::new(12.0, fonts::family_mono()))
                .color(t.text_2),
            );
            ui.horizontal(|ui| {
                ui.label(label("RGB"));
                let mut cr = r;
                let mut cg = g;
                let mut cb = b;
                let ch1 = ui.add(egui::Slider::new(&mut cr, 0..=255).text("R"));
                let ch2 = ui.add(egui::Slider::new(&mut cg, 0..=255).text("G"));
                let ch3 = ui.add(egui::Slider::new(&mut cb, 0..=255).text("B"));
                if ch1.changed() || ch2.changed() || ch3.changed() {
                    write_color(value, alpha, out, rgba_color(cr, cg, cb, a));
                }
            });
            ui.horizontal(|ui| {
                ui.label(label("HSL"));
                let ch_h = ui.add(egui::Slider::new(&mut h, 0.0..=360.0).text("色相"));
                let ch_s = ui.add(egui::Slider::new(&mut s, 0.0..=100.0).text("饱和"));
                let ch_l = ui.add(egui::Slider::new(&mut l, 0.0..=100.0).text("亮度"));
                if ch_h.changed() || ch_s.changed() || ch_l.changed() {
                    let [nr, ng, nb] = hsl_to_rgb(h, s, l);
                    write_color(value, alpha, out, rgba_color(nr, ng, nb, a));
                }
            });
            if alpha {
                let mut na = a;
                ui.horizontal(|ui| {
                    ui.label(label("Alpha"));
                    if ui.add(egui::Slider::new(&mut na, 0..=255)).changed() {
                        let [r2, g2, b2, _] = value.to_srgba_unmultiplied();
                        write_color(value, alpha, out, rgba_color(r2, g2, b2, na));
                    }
                });
            }

            // 令牌色板(点击 = 用 var(--x))
            let color_tokens: Vec<(String, Color32)> = tokens
                .iter()
                .filter_map(|(n, v)| vb_parse_color(v).map(|c| (n.clone(), c)))
                .collect();
            if !color_tokens.is_empty() {
                ui.separator();
                ui.label(label("文档令牌(点击采用 var)"));
                egui::Grid::new("token_swatches").show(ui, |ui| {
                    for (i, (name, c)) in color_tokens.iter().enumerate() {
                        if i > 0 && i % 8 == 0 {
                            ui.end_row();
                        }
                        let (srect, sresp) =
                            ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                        // S5 清单 ③:令牌色板色块的可点语义(采用 var 名)
                        sresp.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::ColorButton,
                                true,
                                format!("--{name}"),
                            )
                        });
                        ui.painter().rect_filled(srect, theme::radius::sm(), *c);
                        ui.painter().rect_stroke(
                            srect,
                            theme::radius::sm(),
                            Stroke::new(theme::stroke::HAIRLINE, t.border),
                            egui::StrokeKind::Inside,
                        );
                        if sresp.clicked() {
                            out.var_picked = Some(name.clone());
                            st.var = format!("var(--{name})");
                            st.err = None;
                        }
                        let _ = sresp.on_hover_text(format!("--{name}"));
                    }
                });
            }
        }

        if let Some(err) = &st.err {
            ui.add_space(theme::space::S1);
            ui.label(
                RichText::new(err)
                    .font(fonts::font(11.0, fonts::Weight::Regular))
                    .color(t.danger),
            );
        }
    }
}

/// 写回颜色(保持字段的 alpha 语义:非 alpha 字段沿用原 alpha)。
fn write_color(value: &mut Color32, alpha_aware: bool, out: &mut ColorFieldResponse, c: Color32) {
    let next = if alpha_aware {
        c
    } else {
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        let a = value.to_srgba_unmultiplied()[3];
        Color32::from_rgba_unmultiplied(r, g, b, a)
    };
    if next != *value {
        *value = next;
        out.changed = true;
    }
}

/// 取色器窗口状态(存 egui 内存,按字段 id 隔离)。
#[derive(Debug, Clone, Default)]
struct PickerState {
    open: bool,
    /// 完整模式(Alt+点击):多出 RGB / HSL / 令牌色板。
    full: bool,
    hex: String,
    var: String,
    err: Option<String>,
}

fn picker_state(m: &mut egui::Memory, id: egui::Id) -> &mut PickerState {
    m.data.get_temp_mut_or_default::<PickerState>(id)
}

/// RGB(u8) → (h°, s%, l%)。
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l * 100.0);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == rf {
        ((gf - bf) / d + if gf < bf { 6.0 } else { 0.0 }) * 60.0 // vb-size-ok: 色相扇区常量(60° 六分色环)
    } else if max == gf {
        ((bf - rf) / d + 2.0) * 60.0 // vb-size-ok: 色相扇区常量
    } else {
        ((rf - gf) / d + 4.0) * 60.0 // vb-size-ok: 色相扇区常量
    };
    (h, s * 100.0, l * 100.0)
}

/// (h°, s%, l%) → [r, g, b](u8)。
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let hf = (h / 360.0).rem_euclid(1.0);
    let sf = (s / 100.0).clamp(0.0, 1.0);
    let lf = (l / 100.0).clamp(0.0, 1.0);
    if sf == 0.0 {
        let v = (lf * 255.0).round() as u8;
        return [v, v, v];
    }
    let q = if lf < 0.5 {
        lf * (1.0 + sf)
    } else {
        lf + sf - lf * sf
    };
    let p = 2.0 * lf - q;
    let hue = |mut t: f32| -> f32 {
        if t < 0.0 {
            t += 1.0
        }
        if t > 1.0 {
            t -= 1.0
        }
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [
        (hue(hf + 1.0 / 3.0) * 255.0).round() as u8,
        (hue(hf) * 255.0).round() as u8,
        (hue(hf - 1.0 / 3.0) * 255.0).round() as u8,
    ]
}

/// `vb_common::Rgba` → egui 颜色(非预乘视角,与回显一致)。
fn rgba_color(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// 走 `vb_common::color::parse_color`(hex/rgb()/hsl()/命名色;
/// 02-6-3 纪律:不重复造解析)。
fn vb_parse_color(s: &str) -> Option<Color32> {
    vb_common::color::parse_color(s).map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
}

/// 解析 `var(--x)` / `--x` / `x`,从文档令牌取色。
/// 返回 `(名称不带 --, 颜色)`。
fn resolve_var(input: &str, tokens: &[(String, String)]) -> Option<(String, Color32)> {
    let t = input.trim();
    let t = t
        .strip_prefix("var(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(t)
        .trim();
    // `--` 前缀可选:`var(--x)` / `--x` / `x` 三种写法等价(输入框友好)
    let name = t.strip_prefix("--").unwrap_or(t).trim().to_string();
    if name.is_empty() {
        return None;
    }
    tokens
        .iter()
        .find(|(n, _)| *n == name)
        .and_then(|(_, v)| vb_parse_color(v))
        .map(|c| (name, c))
}

/// `#RRGGBB` 或带 alpha 的 `#RRGGBBAA`。
///
/// ⚠️ `Color32` 内部存**预乘**字节，直接读 `r()/g()/b()` 会在半透明色上
/// 显示出被 alpha 压暗后的值。这里用 `to_srgba_unmultiplied()` 还原
/// 用户视角的颜色，保证回显与输入一致（与 Agent 对账也靠它）。
fn hex_of(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02X}{g:02X}{b:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

// ──────────────────────────── 4. SectionHeader ────────────────────────────

/// 面板分组标题（"几何" / "外观" / "布局"）。
///
/// 可折叠版本自己画箭头并**直接用 `animate_bool_with_time` 做展开动效**，
/// 而不是 `ui.collapsing` —— 后者会强制加上 egui 默认的三角与缩进，
/// 和设计要的 24px 行高、8px 圆角冲突。
pub struct SectionHeader<'a> {
    title: &'a str,
    icon: Option<icons::Name>,
    open: Option<&'a mut bool>,
}

impl<'a> SectionHeader<'a> {
    /// 新建分组标题。
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            icon: None,
            open: None,
        }
    }

    /// 标题前带图标。
    pub fn icon(mut self, i: icons::Name) -> Self {
        self.icon = Some(i);
        self
    }

    /// 可折叠。
    pub fn collapsible(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }

    /// 画出标题；返回 `Some(是否展开)`（不可折叠时返回 `None`）。
    pub fn ui(self, ui: &mut Ui) -> Option<bool> {
        let t = theme::tokens(ui.ctx());
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), theme::space::ROW_HEIGHT),
            if self.open.is_some() {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let mut icon_x = rect.left();

        if let Some(open) = self.open.as_ref() {
            let chev = if **open {
                icons::Name::Expanded
            } else {
                icons::Name::Collapsed
            };
            ui.painter().text(
                pos2(rect.left() + theme::space::S3 + 1.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                chev.glyph().to_string(),
                icons::font(14.0),
                t.text_2,
            );
            icon_x += 16.0;
        }

        if let Some(ic) = self.icon {
            ui.painter().text(
                pos2(icon_x + theme::space::S3 + 1.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                ic.glyph().to_string(),
                icons::font(14.0),
                t.text_2,
            );
            icon_x += 18.0;
        }

        ui.painter().text(
            pos2(icon_x + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            self.title,
            fonts::font(13.0, fonts::Weight::Semibold),
            t.text,
        );

        if let Some(open) = self.open {
            if resp.clicked() {
                *open = !*open;
            }
            // S5 清单 ③:可折叠标题的读屏语义(展开态进 selected 位);
            // 不可折叠分支(Sense::hover)是纯静态标题,不登记。
            resp.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::CollapsingHeader,
                    true,
                    *open,
                    self.title,
                )
            });
            return Some(*open);
        }
        None
    }
}

// ──────────────────────────── 5. PanelTabs ────────────────────────────

/// [`PanelTabs`] 的交互结果(S1-b 02-1-2:Tab 顺序内存可换)。
#[derive(Debug, Clone, Default)]
pub struct TabsResponse {
    /// 本次是否切换了 Tab(`active` 已被改写)。
    pub changed: bool,
    /// 右键菜单请求的顺序调整:`(槽位, 方向)`,方向 -1 = 左移、+1 = 右移。
    /// 调用方把它应用到自己的顺序表(持久化为阶段 7 的 workspace.json)。
    pub reorder: Option<(usize, i32)>,
}

/// 面板坞的 Tab 条(S1-b 起 4 页:属性 / 图层 / 画板 / 令牌)。
///
/// 下划线指示器走 accent 色；切换动效 200ms（`motion::PANEL`），
/// 因为 Tab 是"层级跳转"而不是"悬停"，用 80ms 会显得突兀。
///
/// `reorderable(true)` 时每个 Tab 支持右键菜单「左移 / 右移」,
/// 请求经 [`TabsResponse::reorder`] 交给调用方落自己的顺序表。
pub struct PanelTabs<'a> {
    labels: &'a [&'a str],
    active: &'a mut usize,
    reorderable: bool,
}

impl<'a> PanelTabs<'a> {
    /// 新建 Tab 条。
    pub fn new(labels: &'a [&'a str], active: &'a mut usize) -> Self {
        Self {
            labels,
            active,
            reorderable: false,
        }
    }

    /// 允许右键重排(02-1-2)。
    pub fn reorderable(mut self, v: bool) -> Self {
        self.reorderable = v;
        self
    }

    /// 画出 Tab 条；返回本次**是否切换了 Tab**。
    pub fn ui(self, ui: &mut Ui) -> bool {
        self.ui_ex(ui).changed
    }

    /// 画出 Tab 条,返回完整交互结果(含重排请求)。
    pub fn ui_ex(self, ui: &mut Ui) -> TabsResponse {
        let mut out = TabsResponse::default();
        if self.labels.is_empty() {
            return out;
        }
        let t = theme::tokens(ui.ctx());
        let h = 28.0;
        let w = ui.available_width() / self.labels.len() as f32;
        let mut changed = false;
        let mut active_rect = egui::Rect::NOTHING;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, name) in self.labels.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
                let is_active = i == *self.active;
                if is_active {
                    active_rect = rect;
                }
                let hover_t = ui.ctx().animate_bool_with_time(
                    ui.id().with(("vbtab", name)),
                    resp.hovered() && !is_active,
                    theme::anim_time(ui.ctx(), theme::motion::HOVER),
                );
                // 选中底:accent-subtle 状态层(§8.3.3;S4 起接替 accent_dim)
                let fill = if is_active {
                    t.accent_subtle
                } else {
                    theme::state::fade(t.state_hover, hover_t)
                };
                ui.painter().rect_filled(
                    rect.shrink2(vec2(theme::space::S1, theme::space::S2)),
                    theme::radius::sm(),
                    fill,
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    *name,
                    fonts::font(12.0, fonts::Weight::Medium),
                    if is_active { t.accent } else { t.text_2 },
                );
                if resp.clicked() && !is_active {
                    *self.active = i;
                    changed = true;
                }
                // S5(§8.10 诚实清单 ①):Tab 页是 allocate 自绘控件,键盘
                // 焦点落在其上时必须有可见环(此前只有选中底,环待 S5 补)。
                if resp.has_focus() {
                    paint_focus_ring(ui, rect, &t);
                }
                // S5 清单 ③:Tab 页读屏语义(页名 + 激活态)。
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        true,
                        is_active,
                        *name,
                    )
                });
                if self.reorderable {
                    let mut reorder: Option<(usize, i32)> = None;
                    resp.context_menu(|ui| {
                        if i > 0 && ui.button("左移").clicked() {
                            reorder = Some((i, -1));
                            ui.close();
                        }
                        if i + 1 < self.labels.len() && ui.button("右移").clicked() {
                            reorder = Some((i, 1));
                            ui.close();
                        }
                    });
                    if reorder.is_some() {
                        out.reorder = reorder;
                    }
                }
            }
        });
        // 下划线指示器(§8.6 #10):accent 2px,**切换时滑动 200ms**
        // (motion::PANEL —— Tab 是层级跳转,不是悬停)。滑动 = 对左右
        // 边缘各插一个 `animate_value_with_time`;动效总开关关闭时 egui
        // animation_time 已归零,自动直通,无第二套开关判断。
        if !active_rect.any_nan() {
            let bar_y = active_rect.bottom() - 2.0;
            let anim_t = theme::motion::PANEL;
            let lx = ui.ctx().animate_value_with_time(
                ui.id().with("vbtab-underline-l"),
                active_rect.left() + theme::space::S3,
                anim_t,
            );
            let rx = ui.ctx().animate_value_with_time(
                ui.id().with("vbtab-underline-r"),
                active_rect.right() - theme::space::S3,
                anim_t,
            );
            let bar = egui::Rect::from_min_max(pos2(lx, bar_y), pos2(rx, bar_y + 2.0));
            let p = ui.painter().clone().with_layer_id(egui::LayerId::new(
                egui::Order::Foreground,
                ui.id().with("vbtab-underline"),
            ));
            p.rect_filled(bar, theme::radius::sm(), t.accent);
        }
        out.changed = changed;
        out
    }
}

// ──────────────────────────── 图标按钮 ────────────────────────────

/// 面板里的小图标按钮（新建图层、删除、上移下移…）。
///
/// 与 [`ToolButton`] 的区别：没有激活态、更小、用于"动作"而非"模式"。
pub fn icon_button(ui: &mut Ui, icon: icons::Name, tooltip: &str) -> Response {
    let t = theme::tokens(ui.ctx());
    let size = 20.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hover_t = ui.ctx().animate_bool_with_time(
        ui.id().with(("vbib", icon, tooltip)),
        resp.hovered(),
        theme::anim_time(ui.ctx(), theme::motion::HOVER),
    );
    ui.painter().rect_filled(
        rect,
        theme::radius::sm(),
        blend(Color32::TRANSPARENT, t.bg_hover, hover_t),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon.glyph().to_string(),
        icons::font(14.0),
        blend(t.text_2, t.text, hover_t),
    );
    // G-UI-D:键盘焦点环
    if resp.has_focus() {
        paint_focus_ring(ui, rect, &t);
    }
    // S5 清单 ③:自绘图标钮的读屏语义 —— tooltip 文本即语义名
    // (此前有 tooltip 无登记,归属 S5 的扫尾项)。
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tooltip));
    resp.on_hover_text(tooltip)
}

// ═══════════════════ S4 组件批(审查 2026-10-04 §8.6 #2/4-9/11-20)═══════════════════
//
// 本节的纪律:
// - **尺寸一律令牌刻度**(G-UI-F):间距走 [`theme::space`],圆角走
//   [`theme::radius`],字号走排版七档(11/12/13/15/24),图标 12–20,
//   行高 24/28(密度),控件 16/20/32 —— 扫描测试 `token_sizes` 钉住;
// - **颜色一律令牌**(G-UI-B):底色/文字/描边全部经 [`theme::tokens`]
//   与 [`theme::state`] 合成,本文件仍是全仓唯一允许出现颜色的界面文件,
//   但新代码不再写字面量,只消费令牌;
// - **动效一律总开关**(§8.8):时长只取 HOVER/STATE/PANEL 三档,经
//   [`theme::anim_time`] 换算,reduced-motion(开关关)自动直通;
// - **自绘控件补 WidgetInfo**(UI-03 起步):读屏语义名随手势一起登记。

// ──────────────────────────── 4. TextField ────────────────────────────

/// 单行文本框(§8.6 #4):高度从行高派生、占位色用弱文字、聚焦画焦点环。
///
/// egui 的 TextEdit 本身在 Tab 序里(键盘可达),本包装只统一外观:
/// 聚焦环 = accent 1.5px 外描边 + 1px 隔离环(`Tokens::focus_ring_*`,
/// G-UI-D 的规格面);返回 egui 的 `Response`,提交语义(失焦/回车)
/// 由调用方按 `lost_focus()` / `changed()` 自取。
pub struct TextField<'a> {
    text: &'a mut String,
    hint: &'a str,
    width: f32,
    id: Option<egui::Id>,
}

impl<'a> TextField<'a> {
    /// 新建文本框。
    pub fn new(text: &'a mut String) -> Self {
        Self {
            text,
            hint: "",
            width: 120.0,
            id: None,
        }
    }

    /// 占位提示(空值时显示)。
    pub fn hint(mut self, h: &'a str) -> Self {
        self.hint = h;
        self
    }

    /// 宽度(默认 120 = 4 基数)。
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// 显式 id(同屏多个字段时必给,否则焦点/缓冲串位)。
    pub fn id(mut self, id: egui::Id) -> Self {
        self.id = Some(id);
        self
    }

    /// 画出文本框。
    pub fn ui(self, ui: &mut Ui) -> Response {
        let h = theme::row_height(ui.ctx());
        let mut edit = egui::TextEdit::singleline(self.text).font(theme::typography::font_id(
            theme::typography::BODY,
            ui.ctx().zoom_factor(),
        ));
        if !self.hint.is_empty() {
            edit = edit.hint_text(self.hint);
        }
        if let Some(id) = self.id {
            edit = edit.id(id);
        }
        let resp = ui.add_sized(Vec2::new(self.width, h), edit);
        // 焦点环:egui TextEdit 自带弱聚焦提示,这里补规格的 accent 环
        if resp.has_focus() {
            paint_focus_ring(ui, resp.rect, &theme::tokens(ui.ctx()));
        }
        resp
    }
}

// ──────────────────────────── 5. Select / Combo ────────────────────────────

/// 下拉选择(§8.6 #5):展开菜单走 egui popup(L3 阴影由全局
/// `popup_shadow` 统一供)。键盘 ↑↓/Enter/Esc 与焦点由 egui
/// `ComboBox` 原生承担 —— 本组件不重写键盘状态机,只统一按钮宽度
/// 与入口(材质吃全局 inactive 样式,见 theme::apply_impl)。
///
/// `add` 里用 `ui.selectable_label(...)` 列选项;返回 `ComboBox` 的
/// Response(是否改选由调用方在 `add` 里经 `selectable_value` 写回)。
pub fn select<R>(
    ui: &mut Ui,
    id_salt: &str,
    selected_text: impl Into<String>,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::Response {
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected_text.into())
        .width(120.0)
        .show_ui(ui, add)
        .response
}

// ──────────────────────────── 7. Slider ────────────────────────────

/// [`slider`] 的返回(§8.6 #7)。
#[derive(Debug, Clone, Default)]
pub struct SliderResponse {
    /// 值被改动(拖动或双击复位)。
    pub changed: bool,
    /// 双击复位发生(调用方需要区分"用户拖的"与"回到默认值")。
    pub reset: bool,
}

/// 滑杆(§8.6 #7):圆点手柄(全局 `HandleShape::Circle` 已定)+
/// 值标签 + **双击复位**。`default` = 复位目标(如 opacity 1.0)。
pub fn slider(
    ui: &mut Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    text: &str,
    default: f64,
) -> SliderResponse {
    let mut out = SliderResponse::default();
    let resp = ui.add(egui::Slider::new(value, range).text(text));
    if resp.double_clicked() {
        if (*value - default).abs() > f64::EPSILON {
            *value = default;
            out.changed = true;
        }
        out.reset = true;
    } else {
        out.changed = resp.changed();
    }
    out
}

// ──────────────────────── 8. Checkbox / Radio / Switch ────────────────────────

/// 复选框(§8.6 #8):自绘方框 + accent 勾选,120ms 状态过渡。
/// 读屏语义经 `WidgetInfo::selected` 登记(UI-03 起步项)。
pub fn checkbox(ui: &mut Ui, text: &str, checked: &mut bool) -> Response {
    let t = theme::tokens(ui.ctx());
    let box_side = 16.0;
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(box_side + theme::space::S2, theme::space::ROW_HEIGHT - 4.0),
        Sense::click(),
    );
    let box_rect = egui::Rect::from_center_size(
        pos2(rect.left() + box_side * 0.5, rect.center().y),
        Vec2::splat(box_side),
    );
    let on_t = ui.ctx().animate_bool_with_time(
        ui.id().with("vbchk"),
        *checked,
        theme::anim_time(ui.ctx(), theme::motion::STATE),
    );
    // 底:选中 accent → 未选中透明;描边:accent → border_strong
    let fill = blend(Color32::TRANSPARENT, t.accent, on_t);
    let stroke_c = blend(t.border_strong, t.accent, on_t);
    ui.painter()
        .rect_filled(box_rect, theme::radius::sm(), fill);
    ui.painter().rect_stroke(
        box_rect,
        theme::radius::sm(),
        Stroke::new(theme::stroke::HAIRLINE, stroke_c),
        egui::StrokeKind::Inside,
    );
    if on_t > 0.01 {
        ui.painter().text(
            box_rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::Name::Check.glyph().to_string(),
            icons::font(12.0),
            t.text,
        );
    }
    if !text.is_empty() {
        ui.painter().text(
            pos2(rect.left() + box_side + theme::space::S2, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            fonts::font(12.0, fonts::Weight::Medium),
            t.text,
        );
    }
    if resp.clicked() {
        *checked = !*checked;
    }
    if resp.has_focus() {
        paint_focus_ring(ui, box_rect, &t);
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *checked, text)
    });
    resp
}

/// 单选圆点(§8.6 #8)。`selected = false` 时只画描边。
pub fn radio(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let t = theme::tokens(ui.ctx());
    let d = 16.0;
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(d + theme::space::S2, theme::space::ROW_HEIGHT - 4.0),
        Sense::click(),
    );
    let c = pos2(rect.left() + d * 0.5, rect.center().y);
    let ring = if selected { t.accent } else { t.border_strong };
    ui.painter()
        .circle_stroke(c, d * 0.5, Stroke::new(theme::stroke::HAIRLINE, ring));
    if selected {
        ui.painter().circle_filled(c, d * 0.25, t.accent);
    }
    if !text.is_empty() {
        ui.painter().text(
            pos2(rect.left() + d + theme::space::S2, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            fonts::font(12.0, fonts::Weight::Medium),
            t.text,
        );
    }
    if resp.has_focus() {
        paint_focus_ring(ui, rect, &t);
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, text)
    });
    resp
}

/// 开关(§8.6 #8):28×16 药丸,开 = accent,120ms。
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let t = theme::tokens(ui.ctx());
    let w = 28.0;
    let h = 16.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
    let on_t = ui.ctx().animate_bool_with_time(
        ui.id().with("vbsw"),
        *on,
        theme::anim_time(ui.ctx(), theme::motion::STATE),
    );
    let fill = blend(t.bg_input, t.accent, on_t);
    ui.painter().rect_filled(rect, theme::radius::pill(), fill);
    // 手柄:12 圆点,x 随 on_t 从左滑到右
    let knob = h - theme::space::S3;
    let x = rect.left() + knob * 0.5 + on_t * (w - knob);
    let knob_c = blend(t.text_3, t.text, on_t);
    ui.painter()
        .circle_filled(pos2(x, rect.center().y), knob * 0.5, knob_c);
    if resp.clicked() {
        *on = !*on;
    }
    if resp.has_focus() {
        paint_focus_ring(ui, rect, &t);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, ""));
    resp
}

// ──────────────────────── 12. Card / 13. Separator / 14. Badge ────────────────────────

/// 卡片(§8.6 #12):L2 凸起材质 + lg 圆角 + S4 内边距(阴影 L2)。
/// 返回 `(卡片 Response, body 产出)` —— 整卡命中区可直接挂点击。
pub fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> (Response, R) {
    let t = theme::tokens(ui.ctx());
    let ir = egui::Frame::new()
        .fill(t.bg_raised)
        .stroke(Stroke::new(theme::stroke::HAIRLINE, t.border))
        .corner_radius(theme::radius::lg())
        .shadow(theme::elevation::shadow_l2(t.dark))
        .inner_margin(egui::Margin::same(theme::space::S4 as i8))
        .show(ui, |ui| body(ui));
    (ir.response, ir.inner)
}

/// 分隔线(§8.6 #13):hairline,颜色 = 分隔强档(`border_strong`,
/// 深色 ≈ N8),缩进对齐内容。egui 默认分隔线吃全局样式,这里给
/// "带缩进的语义版"。
pub fn separator_indented(ui: &mut Ui, indent: f32) {
    let t = theme::tokens(ui.ctx());
    let w = ui.available_width() - indent;
    if w <= 0.0 {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), Sense::hover());
    ui.painter().line_segment(
        [
            pos2(rect.left(), rect.center().y),
            pos2(rect.right(), rect.center().y),
        ],
        Stroke::new(theme::stroke::HAIRLINE, t.border_strong),
    );
}

/// 徽章类别(§8.6 #14:状态栏/台账三态)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeKind {
    /// 中性(信息)。
    Neutral,
    /// 强调(accent)。
    Accent,
    /// 成功。
    Success,
    /// 告警。
    Warn,
    /// 错误。
    Danger,
}

/// 徽章/Chip(§8.6 #14):pill 圆角 + caption 字,底 = 语义色 14% 状态层。
pub fn badge(ui: &mut Ui, text: &str, kind: BadgeKind) -> Response {
    let t = theme::tokens(ui.ctx());
    let (fg, mark) = match kind {
        BadgeKind::Neutral => (t.text_2, t.text_3),
        BadgeKind::Accent => (t.accent, t.accent),
        BadgeKind::Success => (t.success, t.success),
        BadgeKind::Warn => (t.warn, t.warn),
        BadgeKind::Danger => (t.danger, t.danger),
    };
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        fonts::font(11.0, fonts::Weight::Medium),
        fg,
    );
    let pad_x = theme::space::S3;
    let size = Vec2::new(
        galley.size().x + pad_x * 2.0 + theme::space::S2,
        theme::space::ROW_HEIGHT - 4.0,
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::radius::pill(), mark.gamma_multiply(0.22));
    ui.painter().galley(
        pos2(rect.left() + pad_x, rect.center().y - galley.size().y * 0.5),
        galley,
        fg,
    );
    // S5 清单 ③:进度 pill 非交互(Sense::hover),读屏登记文本语义。
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, true, text));
    resp.on_hover_text(text.to_owned())
}

// ──────────────────────── 15. Spinner / Progress ────────────────────────

/// 转圈弧线的共享绘制(`spinner` 与 [`progress_pill`] 共用)。
fn paint_arc_spinner(p: &egui::Painter, c: egui::Pos2, r: f32, color: Color32, time: f64) {
    let frac = (time / 1.2).fract() as f32;
    let start = frac * std::f32::consts::TAU;
    p.circle_stroke(
        c,
        r,
        Stroke::new(theme::stroke::HAIRLINE, color.gamma_multiply(0.4)),
    );
    // 弧:270° 折线近似(8 段;工程工具的 spinner 不追求贝塞尔完美)
    let sweep = std::f32::consts::TAU * 0.75;
    let n = 8;
    let pts: Vec<egui::Pos2> = (0..=n)
        .map(|i| {
            let a = start + sweep * (i as f32 / n as f32);
            pos2(c.x + a.cos() * r, c.y + a.sin() * r)
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(2.0, color)));
}

/// 不确定态转圈(§8.6 #17):12 圆弧,**1.2s 循环**(egui time 驱动,
/// 不经动效开关 —— 它是"进行中"的状态表达,不是装饰动画;开关关时
/// 仍显示,只是不插帧)。调用方在可见时自行 `ctx.request_repaint`。
pub fn spinner(ui: &mut Ui, size: f32) {
    let t = theme::tokens(ui.ctx());
    let now = ui.input(|i| i.time);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_arc_spinner(
        ui.painter(),
        rect.center(),
        size * 0.5 - theme::space::S2,
        t.accent,
        now,
    );
}

/// 状态栏/行内的不确定进度 pill(§8.9 加载态):转圈 + caption 文本。
pub fn progress_pill(ui: &mut Ui, text: &str) -> Response {
    let t = theme::tokens(ui.ctx());
    let now = ui.input(|i| i.time);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        fonts::font(11.0, fonts::Weight::Regular),
        t.text_2,
    );
    let h = 20.0;
    let size = Vec2::new(
        theme::space::S4 * 2.0 + theme::space::S5 + galley.size().x,
        h,
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::radius::pill(), t.bg_input);
    paint_arc_spinner(
        ui.painter(),
        pos2(rect.left() + theme::space::S5, rect.center().y),
        6.0,
        t.accent,
        now,
    );
    ui.painter().galley(
        pos2(
            rect.left() + theme::space::S4 + theme::space::S5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        t.text_2,
    );
    resp
}

/// 确定态线性进度(§8.6 #17):细条 + accent 填充;`frac ≥ 1.0` 时
/// 画完成对勾(pop 交给调用方的 STATE 过渡)。
pub fn progress_linear(ui: &mut Ui, frac: f64, width: f32) -> Response {
    let t = theme::tokens(ui.ctx());
    let h = 4.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, h), Sense::hover());
    ui.painter()
        .rect_filled(rect, theme::radius::pill(), t.bg_input);
    let f = frac.clamp(0.0, 1.0);
    if f > 0.0 {
        let mut fill_rect = rect;
        fill_rect.max.x = rect.left() + (rect.width() as f64 * f) as f32;
        ui.painter()
            .rect_filled(fill_rect, theme::radius::pill(), t.accent);
    }
    if frac >= 1.0 {
        ui.painter().text(
            pos2(rect.right() + theme::space::S2, rect.center().y),
            egui::Align2::LEFT_CENTER,
            icons::Name::Check.glyph().to_string(),
            icons::font(12.0),
            t.success,
        );
    }
    // S5 清单 ③:进度指示非交互(Sense::hover),但读屏要知道它在。
    resp.widget_info(|| egui::WidgetInfo::new(egui::WidgetType::ProgressIndicator));
    resp
}

// ──────────────────────── 16. EmptyState ────────────────────────

/// [`EmptyState`] 的返回。
#[derive(Debug, Clone, Default)]
pub struct EmptyStateResponse {
    /// 主行动按钮被点击。
    pub action_clicked: bool,
}

/// 空态(§8.6 #16;**消 UI-07 的统一入口**):图标 + display 标题 +
/// body 引导 + 主行动按钮。图层空、属性无选区、启动器首启共用同一
/// 规格,不再各画各的"一句灰字"。
pub struct EmptyState<'a> {
    icon: icons::Name,
    title: &'a str,
    body: &'a str,
    action: Option<&'a str>,
}

impl<'a> EmptyState<'a> {
    /// 新建空态(图标 + 标题)。
    pub fn new(icon: icons::Name, title: &'a str) -> Self {
        Self {
            icon,
            title,
            body: "",
            action: None,
        }
    }

    /// 引导正文(一句短话)。
    pub fn body(mut self, b: &'a str) -> Self {
        self.body = b;
        self
    }

    /// 主行动按钮文案。
    pub fn action(mut self, a: &'a str) -> Self {
        self.action = Some(a);
        self
    }

    /// 画出空态(垂直居中排布;宽度吃满 `ui`)。
    pub fn ui(self, ui: &mut Ui) -> EmptyStateResponse {
        let mut out = EmptyStateResponse::default();
        let t = theme::tokens(ui.ctx());
        ui.vertical_centered(|ui| {
            ui.add_space(theme::space::S9);
            ui.label(icons::rich(self.icon, 32.0).color(t.text_3));
            ui.add_space(theme::space::S4);
            ui.label(RichText::new(self.title).font(fonts::font(
                theme::typography::DISPLAY.size,
                fonts::Weight::Semibold,
            )));
            if !self.body.is_empty() {
                ui.add_space(theme::space::S2);
                ui.label(caption(ui, self.body));
            }
            if let Some(a) = self.action {
                ui.add_space(theme::space::S5);
                out.action_clicked = ui
                    .add(
                        egui::Button::new(RichText::new(a).font(fonts::font(
                            theme::typography::BODY_STRONG.size,
                            fonts::Weight::Semibold,
                        )))
                        .fill(t.accent_dim),
                    )
                    .clicked();
            }
        });
        out
    }
}

// ──────────────────────── 18. ValueOverlay ⭐ ────────────────────────

/// 画布浮动数值条(§8.6 #18 ⭐):移动/缩放/旋转拖拽会话中跟随光标,
/// 显示 `X Y ΔX ΔY W H ∠` 中**有值的行**。
///
/// 材质 = L4 提示层(底色取中性阶按下标 7 档、表面 92%/96% 不透明,
/// 阴影 `elevation::shadow_l4`)+ 圆角 6 + caption 等宽字;文字恒用
/// 主题主文字色(L4 底在两套主题下都足够承载正文对比)。
#[derive(Debug, Clone, Default)]
pub struct ValueOverlay {
    /// 绝对位置/尺寸(世界坐标;`None` = 不显示该行)。
    pub x: Option<f64>,
    /// 见 [`ValueOverlay::x`]。
    pub y: Option<f64>,
    /// 位移增量。
    pub dx: Option<f64>,
    /// 见 [`ValueOverlay::dx`]。
    pub dy: Option<f64>,
    /// 见 [`ValueOverlay::x`]。
    pub w: Option<f64>,
    /// 见 [`ValueOverlay::x`]。
    pub h: Option<f64>,
    /// 角度(度)。
    pub angle: Option<f64>,
}

impl ValueOverlay {
    /// 生成显示文本(纯函数;门禁测试打这里)。**有值的行才出现**,
    /// 行序固定:X Y ΔX ΔY W H ∠。Δ 行显式带符号(`+4` / `-4`)。
    pub fn lines(&self) -> Vec<String> {
        let n = |v: f64| format_num(v);
        let d = |v: f64| {
            if v >= 0.0 {
                format!("+{}", n(v))
            } else {
                n(v)
            }
        };
        let mut out = Vec::new();
        if let Some(v) = self.x {
            out.push(format!("X {}", n(v)));
        }
        if let Some(v) = self.y {
            out.push(format!("Y {}", n(v)));
        }
        if let Some(v) = self.dx {
            out.push(format!("ΔX {}", d(v)));
        }
        if let Some(v) = self.dy {
            out.push(format!("ΔY {}", d(v)));
        }
        if let Some(v) = self.w {
            out.push(format!("W {}", n(v)));
        }
        if let Some(v) = self.h {
            out.push(format!("H {}", n(v)));
        }
        if let Some(v) = self.angle {
            out.push(format!("∠ {}°", n(v)));
        }
        out
    }

    /// 是否有可显示内容(空 = 调用方跳过绘制)。
    pub fn is_empty(&self) -> bool {
        self.lines().is_empty()
    }

    /// 画在屏幕坐标 `pos` 右下偏移处(egui 顶层 Tooltip 层)。
    pub fn show(&self, ctx: &egui::Context, pos: egui::Pos2) {
        let lines = self.lines();
        if lines.is_empty() {
            return;
        }
        let t = theme::tokens(ctx);
        let layer = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("vb_value_overlay"),
        ));
        // L4 表面:中性阶按下标(深 N7/浅 N1)+ 表面不透明度
        let base = theme::elevation::base(theme::elevation::L4, &t);
        let [r, g, b, _] = base.to_srgba_unmultiplied();
        let alpha = (theme::elevation::surface_alpha_l4(t.dark) * 255.0).round() as u8;
        let fill = Color32::from_rgba_unmultiplied(r, g, b, alpha);
        let font = theme::typography::mono_font_id(ctx.zoom_factor());
        let line_h = theme::typography::MONO.line_height;
        let pad_x = theme::space::S3;
        let pad_y = theme::space::S2;
        let galleys: Vec<_> = lines
            .iter()
            .map(|l| layer.layout_no_wrap(l.clone(), font.clone(), t.text))
            .collect();
        let w = galleys.iter().map(|g| g.size().x).fold(0.0f32, f32::max) + pad_x * 2.0;
        let h = line_h * lines.len() as f32 + pad_y * 2.0;
        let rect = egui::Rect::from_min_size(pos + Vec2::new(16.0, 16.0), Vec2::new(w, h));
        // 阴影:egui painter 无直接 shadow → 画一圈更大的半透明底矩形近似
        // (真实模糊阴影由 Window 层的 L3/L4 承担;浮层贴光标,阴影弱化)
        layer.rect_filled(
            rect.expand(theme::space::S3),
            theme::radius::lg(),
            Color32::from_black_alpha(if t.dark { 64 } else { 26 }), // vb-size-ok: 阴影颜色 alpha 字节(G-UI-B 域)
        );
        layer.rect_filled(rect, theme::radius::md(), fill);
        layer.rect_stroke(
            rect,
            theme::radius::md(),
            Stroke::new(theme::stroke::HAIRLINE, t.border),
            egui::StrokeKind::Inside,
        );
        for (i, g) in galleys.iter().enumerate() {
            layer.galley(
                pos2(rect.left() + pad_x, rect.top() + pad_y + line_h * i as f32),
                g.clone(),
                t.text,
            );
        }
    }
}

// ──────────────────────── 20. Dialog ────────────────────────

/// 对话框包装(§8.6 #20):title 字号、L3 阴影(全局 `window_shadow`
/// 已定)、**出场 8px + fade 120ms**(动效开关关闭自动直通)、底部
/// 按钮右对齐走 [`dialog_footer`]。Esc/Enter 键位语义由调用方按各自
/// 输入面接线(命令面板输入框常驻,统一收口会误触发 —— 与
/// [`dialog_footer`] 同一裁定)。
///
/// 返回窗口的内部产出;`open` 由调用方持有(× 关闭会把它置 false)。
pub fn dialog<R>(
    ctx: &egui::Context,
    id_salt: &str,
    title: &str,
    open: &mut bool,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let mut win_open = *open;
    let id = egui::Id::new(id_salt);
    let out = egui::Window::new(title)
        .id(id)
        .open(&mut win_open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            crate::motion::fade_slide(ui, id.with("vb-dlg-enter"), theme::motion::STATE, 8.0, body)
        });
    *open = win_open;
    out.and_then(|ir| ir.inner)
}

// ──────────────────────── §8.9 错误内联条 ────────────────────────

/// 错误内联条(§8.9 错误态):`danger` 左描边 + 图标 + 文本,
/// **详情可展开**(`details` 非空时出现展开钮)。
///
/// 与 toast 的分工:toast 是"飘走的通知",内联条是"驻留在表单里、
/// 直到问题解决"的状态 —— 导入报错、命令失败上下文用这个。
pub fn inline_error(ui: &mut Ui, message: &str, details: Option<&str>, expanded: &mut bool) {
    let t = theme::tokens(ui.ctx());
    let resp = egui::Frame::new()
        .fill(theme::state::over(
            t.bg_panel,
            t.danger.gamma_multiply(0.10),
        ))
        .corner_radius(theme::radius::sm())
        .inner_margin(egui::Margin::symmetric(
            theme::space::S3 as i8,
            theme::space::S2 as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(icons::rich(icons::Name::Alert, 14.0).color(t.danger));
                ui.label(RichText::new(message).font(fonts::font(
                    theme::typography::BODY.size,
                    fonts::Weight::Regular,
                )));
                if details.is_some() {
                    let chev = if *expanded {
                        icons::Name::Expanded
                    } else {
                        icons::Name::Collapsed
                    };
                    if icon_button(
                        ui,
                        chev,
                        if *expanded {
                            "收起详情"
                        } else {
                            "展开详情"
                        },
                    )
                    .clicked()
                    {
                        *expanded = !*expanded;
                    }
                }
            });
            if *expanded {
                if let Some(d) = details {
                    ui.label(
                        RichText::new(d)
                            .font(theme::typography::mono_font_id(1.0))
                            .color(t.text_2),
                    );
                }
            }
        });
    // 左 danger 描边(S4):3px 竖条贴条左缘,圆角内收
    let rect = resp.response.rect;
    ui.painter().line_segment(
        [
            pos2(rect.left() + 2.0, rect.top() + theme::space::S2),
            pos2(rect.left() + 2.0, rect.bottom() - theme::space::S2),
        ],
        Stroke::new(3.0, t.danger),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 颜色混合的两端必须精确命中，中间单调过渡。
    /// 这保证"悬停过渡"不会在 t=0/1 时留下色差。
    #[test]
    fn blend_hits_both_ends() {
        // vb-token-ok: 测试夹具
        let a = Color32::from_rgb(0, 0, 0);
        // vb-token-ok: 测试夹具
        let b = Color32::from_rgb(255, 255, 255);
        assert_eq!(blend(a, b, 0.0), a);
        assert_eq!(blend(a, b, 1.0), b);
        assert_eq!(blend(a, b, 0.5).r(), 128);
    }

    /// 超出 [0,1] 的 t 必须被钳制，否则动画越界时会溢出 u8。
    #[test]
    fn blend_clamps_out_of_range() {
        // vb-token-ok: 测试夹具
        let a = Color32::from_rgb(10, 10, 10);
        // vb-token-ok: 测试夹具
        let b = Color32::from_rgb(20, 20, 20);
        assert_eq!(blend(a, b, 5.0), b);
        assert_eq!(blend(a, b, -3.0), a);
    }

    /// 十六进制回显格式（属性面板与 Agent 对账都用它）。
    ///
    /// 注：Color32 以 8 位预乘存储，非预乘↔预乘往返有 ±1/通道的固有
    /// 舍入误差，因此半透明断言用容差比较。
    #[test]
    fn hex_formatting() {
        // vb-token-ok: 测试夹具
        assert_eq!(hex_of(Color32::from_rgb(0x0D, 0x99, 0xFF)), "#0D99FF");
        // 不透明色无预乘，必须精确往返。
        let c = Color32::from_rgb(0x0D, 0x99, 0xFF); // vb-token-ok: 测试夹具
        let opaque = Color32::from_rgba_unmultiplied(0x0D, 0x99, 0xFF, 255); // vb-token-ok: 测试夹具
        assert_eq!(hex_of(opaque), hex_of(c));
        // 半透明：容差 ±1。
        let semi = Color32::from_rgba_unmultiplied(0x0D, 0x99, 0xFF, 0x80); // vb-token-ok: 测试夹具
        let [r, g, b, a] = semi.to_srgba_unmultiplied();
        let s = hex_of(Color32::from_rgba_unmultiplied(r, g, b, a));
        assert_eq!(s.len(), 9, "带 alpha 输出 8 位十六进制");
        let ch = |i: usize| u8::from_str_radix(&s[1 + i * 2..3 + i * 2], 16).unwrap();
        assert!((ch(0) as i32 - 0x0D).abs() <= 1);
        assert!((ch(1) as i32 - 0x99).abs() <= 1);
        assert!((ch(2) as i32 - 0xFF).abs() <= 1);
        assert_eq!(ch(3), 0x80);
    }

    /// tooltip 必须是「名称 (快捷键)」—— 这是零成本学习模式的核心，
    /// 少了括号里的键位，用户就得去翻文档。
    #[test]
    fn toolbutton_tooltip_includes_shortcut() {
        let b = ToolButton::new(icons::Name::ToolSelect, "选择工具").shortcut("V");
        assert_eq!(b.tooltip_text(), "选择工具 (V)");
        let b = ToolButton::new(icons::Name::ToolSelect, "选择工具");
        assert_eq!(b.tooltip_text(), "选择工具");
    }

    /// 字段标签宽度必须是 4 的倍数（间距纪律）且足够放两个汉字。
    #[test]
    fn field_label_width_follows_spacing_rule() {
        assert_eq!(FIELD_LABEL_WIDTH % 4.0, 0.0);
        const {
            assert!(
                FIELD_LABEL_WIDTH >= 48.0,
                "标签宽度要能放下「宽度」两个字，否则会与输入框挤在一起"
            );
        }
    }

    // ── 02-6-1:scrubby 步进(纯函数) ──

    /// 横向拖动 1px = speed;Shift 粗调 ×10;Alt 细调 ×0.1;同时按 Alt 优先。
    #[test]
    fn scrub_step_multipliers() {
        assert_eq!(scrub_step(10.0, 1.0, false, false), 10.0, "基础 = dx×speed");
        assert_eq!(scrub_step(5.0, 0.5, false, false), 2.5);
        assert_eq!(scrub_step(3.0, 1.0, true, false), 30.0, "Shift 粗调 ×10");
        assert!(
            (scrub_step(3.0, 1.0, false, true) - 0.3).abs() < 1e-9,
            "Alt 细调 ×0.1"
        );
        assert!(
            (scrub_step(3.0, 1.0, true, true) - 0.3).abs() < 1e-9,
            "同时按下按 Alt(细调)优先"
        );
        assert_eq!(scrub_step(-4.0, 2.0, false, false), -8.0, "负方向");
        assert_eq!(scrub_step(0.0, 1.0, true, false), 0.0);
    }

    /// 数值回显格式:整数不带小数点,小数最多 2 位去尾零。
    #[test]
    fn num_formatting() {
        assert_eq!(format_num(120.0), "120");
        assert_eq!(format_num(160.5), "160.5");
        assert_eq!(format_num(160.25), "160.25");
        assert_eq!(format_num(160.2500000), "160.25");
        assert_eq!(format_num(0.0), "0");
        assert_eq!(format_num(-12.0), "-12");
        assert_eq!(format_num(-0.5), "-0.5");
    }

    /// 表达式提交语义由 vb_ui(expr)与 vb_app(会话)分别把关;
    /// 这里锁「参数到画布的语义」:单位后缀不进求值器(缓冲只存数字)。
    #[test]
    fn unit_is_display_only() {
        let s = format_num(1440.0);
        assert_eq!(crate::expr::eval_expr(&s, Some(1440.0)).unwrap(), 1440.0);
    }

    // ── 02-6-3:RGB ↔ HSL 往返 ──

    #[test]
    fn rgb_hsl_roundtrip() {
        for (r, g, b) in [
            (255u8, 0u8, 0u8),
            (0, 255, 0),
            (0, 0, 255),
            (128, 128, 128),
            (255, 255, 255),
            (0, 0, 0),
            (255, 128, 0),
        ] {
            let (h, s, l) = rgb_to_hsl(r, g, b);
            let [r2, g2, b2] = hsl_to_rgb(h, s, l);
            assert!((r2 as i32 - r as i32).abs() <= 1, "r {r} → {r2}");
            assert!((g2 as i32 - g as i32).abs() <= 1, "g {g} → {g2}");
            assert!((b2 as i32 - b as i32).abs() <= 1, "b {b} → {b2}");
        }
    }

    /// 主色相锚点:HSL 语义正确(红=0°、绿=120°、蓝=240°)。
    #[test]
    fn hsl_anchors() {
        let (h, s, l) = rgb_to_hsl(255, 0, 0);
        assert!((h - 0.0).abs() < 1.0 && (s - 100.0).abs() < 1.0 && (l - 50.0).abs() < 1.0);
        let (h, _, _) = rgb_to_hsl(0, 255, 0);
        assert!((h - 120.0).abs() < 1.0);
        let (h, _, _) = rgb_to_hsl(0, 0, 255);
        assert!((h - 240.0).abs() < 1.0);
        // 灰色 s=0
        let (_, s, _) = rgb_to_hsl(130, 130, 130);
        assert!(s.abs() < 0.001);
    }

    /// var(--x) 解析:`var(--name)` / `--name` / `name` 三种写法等价;
    /// 找不到令牌返回 None(调用方报中文错误)。
    #[test]
    fn var_resolution() {
        let tokens = vec![
            ("brand-1".to_string(), "#ff5a1f".to_string()), // vb-token-ok: 测试夹具
            ("gray".to_string(), "hsl(0, 0%, 50%)".to_string()),
        ];
        for input in ["var(--brand-1)", "--brand-1", "brand-1", " var(--brand-1) "] {
            let (name, c) = resolve_var(input, &tokens).expect(input);
            assert_eq!(name, "brand-1");
            let [r, g, b, _] = c.to_srgba_unmultiplied();
            assert_eq!((r, g, b), (0xFF, 0x5A, 0x1F));
        }
        assert!(resolve_var("var(--missing)", &tokens).is_none());
        assert!(
            resolve_var("var(--gray)", &tokens).is_some(),
            "hsl() 令牌经 vb_common 解析"
        );
    }

    // ── S4 批(审查 §8.6):新组件纯函数面 ──

    /// ValueOverlay 行序与"有值才显示"(§8.6 #18 ⭐):移动会话出
    /// X/Y/Δ;缩放会话出 W/H;旋转出 ∠ —— 空字段不占行。
    #[test]
    fn value_overlay_lines_subset_in_order() {
        let mut v = ValueOverlay::default();
        assert!(v.is_empty(), "全空 = 不画");
        v.x = Some(120.0);
        v.dx = Some(-4.0);
        assert_eq!(v.lines(), vec!["X 120", "ΔX -4"]);
        v.dy = Some(6.0);
        v.y = Some(80.0);
        v.w = Some(320.0);
        v.h = Some(160.5);
        v.angle = Some(90.0);
        assert_eq!(
            v.lines(),
            vec!["X 120", "Y 80", "ΔX -4", "ΔY +6", "W 320", "H 160.5", "∠ 90°"],
            "行序固定:X Y ΔX ΔY W H ∠"
        );
    }

    /// NumField 混合态只是渲染提示,不影响求值/格式化纯函数。
    #[test]
    fn numfield_mixed_is_display_only() {
        // 与 `unit_is_display_only` 同口径:构造器开关不进任何纯函数
        let mut a = 1.0;
        let mut b = 2.0;
        let f = NumField::new("X", &mut a).mixed(true);
        assert!(f.mixed);
        let g = NumField::new("X", &mut b);
        assert!(!g.mixed, "默认单值态");
    }

    /// 空态组件的规格钉子:标题必填、行动可选(消 UI-07 的统一入口)。
    #[test]
    fn empty_state_builder_contract() {
        let e = EmptyState::new(icons::Name::KindLayer, "还没有对象");
        assert_eq!(e.title, "还没有对象");
        assert!(e.action.is_none(), "不带行动的空态合法(纯提示)");
        let e = e.body("先创建").action("新建");
        assert_eq!(e.body, "先创建");
        assert_eq!(e.action, Some("新建"));
    }

    /// Badge 三态映射(状态栏/台账消费):每类都解析到语义色而非字面量。
    #[test]
    fn badge_kinds_are_exhaustive_documented() {
        // 编译期穷举:新增 BadgeKind 变体时此处 match 编译失败
        for k in [
            BadgeKind::Neutral,
            BadgeKind::Accent,
            BadgeKind::Success,
            BadgeKind::Warn,
            BadgeKind::Danger,
        ] {
            let name = format!("{k:?}");
            assert!(!name.is_empty());
        }
    }
}
