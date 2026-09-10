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

/// 一行"标签 + 控件"，标签固定宽度以保证纵向对齐。
pub fn field_row<R>(ui: &mut Ui, label_text: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(FIELD_LABEL_WIDTH, theme::space::ROW_HEIGHT),
            Sense::hover(),
        );
        let t = theme::tokens(ui.ctx());
        ui.painter().text(
            pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label_text,
            fonts::font(12.0, fonts::Weight::Medium),
            t.text_2,
        );
        add(ui)
    })
    .inner
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

    /// tooltip 文本：`名称 (快捷键)`。
    fn tooltip_text(&self) -> String {
        match self.shortcut {
            Some(k) => format!("{} ({})", self.label, k),
            None => self.label.to_string(),
        }
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
            theme::motion::HOVER,
        );

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
            ui.painter().text(
                pos2(
                    icon_rect.center().x,
                    icon_rect.top() + icon_size * 0.5 + 2.0,
                ),
                egui::Align2::CENTER_CENTER,
                self.icon.glyph().to_string(),
                icons::font(icon_size),
                fg,
            );
            ui.painter().text(
                pos2(rect.center().x, rect.bottom() - 6.0),
                egui::Align2::CENTER_BOTTOM,
                self.label,
                fonts::font(11.0, fonts::Weight::Regular),
                fg,
            );
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.icon.glyph().to_string(),
                icons::font(icon_size),
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

/// 数值字段（属性面板里的 X / Y / 宽 / 高 / 角度 / 缩放）。
///
/// 统一了：标签宽度、单位后缀、拖拽速度、圆角、对齐。调用方只描述**数值语义**。
pub struct NumField<'a> {
    label: &'a str,
    value: &'a mut f64,
    unit: &'a str,
    speed: f64,
    range: Option<(f64, f64)>,
    width: f32,
}

impl<'a> NumField<'a> {
    /// 新建数值字段。
    pub fn new(label: &'a str, value: &'a mut f64) -> Self {
        Self {
            label,
            value,
            unit: "",
            speed: 1.0,
            range: None,
            width: 72.0,
        }
    }

    /// 单位后缀（`px` / `°` / `%`）。
    pub fn unit(mut self, u: &'a str) -> Self {
        self.unit = u;
        self
    }

    /// 拖拽速度（每像素改变量）。
    pub fn speed(mut self, s: f64) -> Self {
        self.speed = s;
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

    /// 画出字段，返回内部控件的响应。
    pub fn ui(self, ui: &mut Ui) -> Response {
        field_row(ui, self.label, |ui| {
            let mut dv = egui::DragValue::new(self.value)
                .speed(self.speed)
                .max_decimals(2);
            if let Some((lo, hi)) = self.range {
                dv = dv.range(lo..=hi);
            }
            let resp = ui.add_sized(Vec2::new(self.width, theme::space::ROW_HEIGHT - 4.0), dv);
            if !self.unit.is_empty() {
                ui.label(
                    RichText::new(self.unit)
                        .font(fonts::font(11.0, fonts::Weight::Regular))
                        .color(theme::tokens(ui.ctx()).text_3),
                );
            }
            resp
        })
    }
}

// ──────────────────────────── 3. ColorField ────────────────────────────

/// 颜色字段（填充 / 描边 / 文字色）。
///
/// 比裸用 `color_edit_button_srgba` 多了：固定宽度标签、十六进制回显
/// （等宽字体，方便 Agent 与人类对照）、可选的"无"状态。
pub struct ColorField<'a> {
    label: &'a str,
    value: &'a mut Color32,
    alpha: bool,
    /// 允许清空为"无"（如 `background-color` 未设置）。
    clearable: Option<&'a mut bool>,
}

impl<'a> ColorField<'a> {
    /// 新建颜色字段。
    pub fn new(label: &'a str, value: &'a mut Color32) -> Self {
        Self {
            label,
            value,
            alpha: true,
            clearable: None,
        }
    }

    /// 是否带 alpha 通道。
    pub fn alpha(mut self, v: bool) -> Self {
        self.alpha = v;
        self
    }

    /// 允许清空；返回 `true` 表示本次被清空。
    pub fn clearable(mut self, flag: &'a mut bool) -> Self {
        self.clearable = Some(flag);
        self
    }

    /// 画出字段；返回 `(是否改变, 是否被清空)`。
    pub fn ui(mut self, ui: &mut Ui) -> (bool, bool) {
        let mut cleared = false;
        let changed = field_row(ui, self.label, |ui| {
            let mut changed = if self.alpha {
                ui.color_edit_button_srgba(self.value).changed()
            } else {
                ui.color_edit_button_srgb(&mut [self.value.r(), self.value.g(), self.value.b()])
                    .changed()
            };
            if let Some(ref mut flag) = self.clearable {
                if ui
                    .add(egui::Button::new(icons::rich(icons::Name::Close, 12.0)).frame(false))
                    .on_hover_text("清除颜色")
                    .clicked()
                {
                    **flag = true;
                    changed = true;
                    cleared = true;
                }
            }
            ui.label(mono(&hex_of(*self.value)));
            changed
        });
        (changed, cleared)
    }
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

/// 面板坞的 Tab 条（属性 / 图层 / 令牌 / 导出）。
///
/// 下划线指示器走 accent 色；切换动效 200ms（`motion::PANEL`），
/// 因为 Tab 是"层级跳转"而不是"悬停"，用 80ms 会显得突兀。
pub struct PanelTabs<'a> {
    labels: &'a [&'a str],
    active: &'a mut usize,
}

impl<'a> PanelTabs<'a> {
    /// 新建 Tab 条。
    pub fn new(labels: &'a [&'a str], active: &'a mut usize) -> Self {
        Self { labels, active }
    }

    /// 画出 Tab 条；返回本次**是否切换了 Tab**。
    pub fn ui(self, ui: &mut Ui) -> bool {
        if self.labels.is_empty() {
            return false;
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
                    theme::motion::HOVER,
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
            }
        });
        changed
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
            theme::motion::HOVER,
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
        theme::motion::HOVER,
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
}
