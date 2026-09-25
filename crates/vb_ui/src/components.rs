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
//! | [`LayerRow`] | 图层树里手绘行 + 一串 `small_button` |
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
        let base = if self.active {
            t.accent_dim
        } else {
            Color32::TRANSPARENT
        };
        let hovered = if self.active {
            blend(t.accent_dim, t.accent, hover_t * 0.25)
        } else {
            blend(Color32::TRANSPARENT, t.bg_hover, hover_t)
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
        0.1
    } else if shift {
        10.0
    } else {
        1.0
    };
    dx as f64 * speed * k
}

/// 数值格式化(输入框回显):整数不带小数点,小数最多 2 位去尾零。
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
        ui.painter().rect_filled(
            lrect,
            theme::radius::sm(),
            blend(Color32::TRANSPARENT, t.bg_hover, hover_t * 0.6),
        );
        ui.painter().text(
            pos2(lrect.right() - 2.0, lrect.center().y),
            egui::Align2::RIGHT_CENTER,
            self.label,
            fonts::font(12.0, fonts::Weight::Medium),
            blend(t.text_2, t.text, hover_t),
        );
        if lresp.hovered() {
            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::ResizeHorizontal);
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
        let edit = ui.add_sized(
            Vec2::new(self.width, row_h),
            egui::TextEdit::singleline(&mut buf)
                .id(field_id)
                .font(fonts::font(12.0, fonts::Weight::Regular))
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

        // ── 键盘步进(聚焦时;全局方向键走 TextEdit 上下文不会抢)──
        if edit.has_focus()
            && ui
                .ctx()
                .input(|i| i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowDown))
        {
            let (up, shift) = ui
                .ctx()
                .input(|i| (i.key_pressed(egui::Key::ArrowUp), i.modifiers.shift));
            let step = self.step.unwrap_or(self.speed) * if shift { 10.0 } else { 1.0 };
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
                fonts::font(11.0, fonts::Weight::Regular),
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
            ui.painter().rect_stroke(
                rect,
                theme::radius::sm(),
                Stroke::new(theme::stroke::HAIRLINE, blend(t.border, t.text, hover_t)),
                egui::StrokeKind::Inside,
            );
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

        // ── 完整模式(Alt+点击):RGB / HSL / 令牌色板 ──
        if st.full {
            ui.separator();
            let [r, g, b, a] = value.to_srgba_unmultiplied();
            let (mut h, mut s, mut l) = rgb_to_hsl(r, g, b);
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
        ((gf - bf) / d + if gf < bf { 6.0 } else { 0.0 }) * 60.0
    } else if max == gf {
        ((bf - rf) / d + 2.0) * 60.0
    } else {
        ((rf - gf) / d + 4.0) * 60.0
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
                pos2(rect.left() + 7.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                chev.glyph().to_string(),
                icons::font(14.0),
                t.text_2,
            );
            icon_x += 16.0;
        }

        if let Some(ic) = self.icon {
            ui.painter().text(
                pos2(icon_x + 7.0, rect.center().y),
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

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (i, name) in self.labels.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::click());
                let is_active = i == *self.active;
                let hover_t = ui.ctx().animate_bool_with_time(
                    ui.id().with(("vbtab", name)),
                    resp.hovered() && !is_active,
                    theme::anim_time(ui.ctx(), theme::motion::HOVER),
                );
                // 选中底：accent 的 16% 透明度
                let fill = if is_active {
                    t.accent_dim
                } else {
                    blend(Color32::TRANSPARENT, t.bg_hover, hover_t)
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
                if is_active {
                    // 下划线指示器
                    let bar = egui::Rect::from_min_max(
                        pos2(rect.left() + theme::space::S3, rect.bottom() - 2.0),
                        pos2(rect.right() - theme::space::S3, rect.bottom()),
                    );
                    ui.painter().rect_filled(bar, theme::radius::sm(), t.accent);
                }
                if resp.clicked() && !is_active {
                    *self.active = i;
                    changed = true;
                }
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
        out.changed = changed;
        out
    }
}

// ──────────────────────────── 6. LayerRow ────────────────────────────

/// 图层行的交互结果。
#[derive(Debug, Default, Clone, Copy)]
pub struct LayerRowResponse {
    /// 单击（选中）。
    pub clicked: bool,
    /// 双击（通常是重命名或聚焦）。
    pub double_clicked: bool,
    /// 点了眼睛。
    pub toggle_hidden: bool,
    /// 点了锁。
    pub toggle_locked: bool,
}

/// 图层树的一行：缩进 + 展开箭头 + 类型图标 + 名称 + 眼睛 + 锁。
///
/// 行高固定 24px、悬停改底色、选中用 accent 底 —— 三者都由组件保证，
/// 图层树那边不再自己画行。
pub struct LayerRow<'a> {
    icon: icons::Name,
    name: &'a str,
    depth: u8,
    selected: bool,
    hidden: bool,
    locked: bool,
    has_children: bool,
    expanded: bool,
}

impl<'a> LayerRow<'a> {
    /// 新建图层行。
    pub fn new(icon: icons::Name, name: &'a str) -> Self {
        Self {
            icon,
            name,
            depth: 0,
            selected: false,
            hidden: false,
            locked: false,
            has_children: false,
            expanded: false,
        }
    }

    /// 缩进层级。
    pub fn depth(mut self, d: u8) -> Self {
        self.depth = d;
        self
    }

    /// 是否选中。
    pub fn selected(mut self, v: bool) -> Self {
        self.selected = v;
        self
    }

    /// 是否隐藏。
    pub fn hidden(mut self, v: bool) -> Self {
        self.hidden = v;
        self
    }

    /// 是否锁定。
    pub fn locked(mut self, v: bool) -> Self {
        self.locked = v;
        self
    }

    /// 是否有子节点（决定是否画展开箭头）。
    pub fn has_children(mut self, v: bool) -> Self {
        self.has_children = v;
        self
    }

    /// 是否已展开。
    pub fn expanded(mut self, v: bool) -> Self {
        self.expanded = v;
        self
    }

    /// 画出行，返回交互结果。
    pub fn ui(self, ui: &mut Ui) -> LayerRowResponse {
        let t = theme::tokens(ui.ctx());
        let mut out = LayerRowResponse::default();
        let (rect, resp) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), theme::space::ROW_HEIGHT),
            Sense::click(),
        );

        let hover_t = ui.ctx().animate_bool_with_time(
            ui.id().with(("vblayer", self.name, self.depth)),
            resp.hovered() && !self.selected,
            theme::anim_time(ui.ctx(), theme::motion::HOVER),
        );

        // 行底：选中 > 悬停 > 透明
        let fill = if self.selected {
            t.accent_dim
        } else {
            blend(Color32::TRANSPARENT, t.bg_hover, hover_t)
        };
        ui.painter().rect_filled(
            rect.shrink2(vec2(theme::space::S1, 1.0)),
            theme::radius::sm(),
            fill,
        );

        let indent = theme::space::S2 + self.depth as f32 * theme::space::S6;
        let mut x = rect.left() + indent;

        // 展开箭头
        if self.has_children {
            let chev = if self.expanded {
                icons::Name::Expanded
            } else {
                icons::Name::Collapsed
            };
            ui.painter().text(
                pos2(x + 6.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                chev.glyph().to_string(),
                icons::font(12.0),
                t.text_3,
            );
        }
        x += 16.0;

        // 类型图标
        ui.painter().text(
            pos2(x + 7.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            self.icon.glyph().to_string(),
            icons::font(icons::Name::size_in_panel()),
            t.text_2,
        );
        x += 18.0;

        // 右侧两个开关占位，先算出来给名字留宽度
        let btn = 20.0;
        let lock_x = rect.right() - theme::space::S2 - btn * 0.5;
        let eye_x = lock_x - btn;

        // 名称
        let name_color = if self.hidden { t.text_3 } else { t.text };
        let clip =
            egui::Rect::from_min_max(pos2(x, rect.top()), pos2(eye_x - btn * 0.6, rect.bottom()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(clip));
        child.add(
            egui::Label::new(
                RichText::new(self.name)
                    .font(fonts::font(12.0, fonts::Weight::Regular))
                    .color(name_color),
            )
            .truncate()
            .selectable(false),
        );

        // 显示 / 隐藏（常显，隐藏时高亮）
        ui.painter().text(
            pos2(eye_x, rect.center().y),
            egui::Align2::CENTER_CENTER,
            if self.hidden {
                icons::Name::Hidden.glyph().to_string()
            } else {
                icons::Name::Visible.glyph().to_string()
            },
            icons::font(14.0),
            if self.hidden { t.warn } else { t.text_3 },
        );
        // 锁（仅锁定时显示，避免行内视觉噪音）
        if self.locked {
            ui.painter().text(
                pos2(lock_x, rect.center().y),
                egui::Align2::CENTER_CENTER,
                icons::Name::Locked.glyph().to_string(),
                icons::font(14.0),
                t.warn,
            );
        }

        // 用子区域把"眼睛/锁"的点击从"选中"里切出来
        let eye_rect = egui::Rect::from_center_size(pos2(eye_x, rect.center().y), Vec2::splat(btn));
        let lock_rect =
            egui::Rect::from_center_size(pos2(lock_x, rect.center().y), Vec2::splat(btn));
        let eye_resp = ui.interact(
            eye_rect,
            ui.id().with(("eye", self.name, self.depth)),
            Sense::click(),
        );
        let lock_resp = ui.interact(
            lock_rect,
            ui.id().with(("lock", self.name, self.depth)),
            Sense::click(),
        );

        out.toggle_hidden = eye_resp.clicked();
        out.toggle_locked = lock_resp.clicked();
        // 点在开关上不算"选中"
        out.clicked = resp.clicked() && !out.toggle_hidden && !out.toggle_locked;
        out.double_clicked = resp.double_clicked();

        eye_resp.on_hover_text(if self.hidden { "显示" } else { "隐藏" });
        lock_resp.on_hover_text(if self.locked { "解锁" } else { "锁定" });

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
    resp.on_hover_text(tooltip)
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
}
