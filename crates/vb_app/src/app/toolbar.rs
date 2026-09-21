//! 工具箱(阶段 6 / 副文档 07):**四向停靠** + 单/双列 + 拖动吸附。
//!
//! 与旧「底部浮动条」的差别(S1-a 起的历史行为在这里改写):
//! - 由 `egui::Area`(浮动、锚 `CENTER_BOTTOM`)改为 **`egui::Panel`**,
//!   按 [`DockSide`] 走 `top/left/right/bottom` 分支;
//! - 顶部加**拖动把手**:拖动中实时算吸附边并画预览带,松手生效
//!   (落在中间 → 回弹,不做浮动 —— 与 `design/14` F8 精神一致);
//! - 左/右停靠可切**单列 / 双列**;
//! - 停靠位与列数**写回 `workspace.json`**(07-2),重启还原。

use egui::{Sense, Stroke};
use vb_ui::components::ToolButton;
use vb_ui::icons::Name;
use vb_ui::theme::{self, Tokens};

use super::dock_layout::DockSide;
use super::{Tool, VellumApp};

/// 吸附判定带宽(像素):指针进入边缘这么宽的带 → 预览吸附。
const SNAP_BAND: f32 = 56.0;

/// 工具箱条目 `(工具, 图标, 名称, 键位)`。
///
/// **顺序 = `design/03 §一` 的分组**(同族相邻,`family_of` 据此切静态子切片):
/// 选择 / 路径 / 形状 / 文字·画板 / 上色 / 视图。
/// 其中「选择」与「直接选择」是**两个独立工具**(07-3-1),紧邻在最前。
const TOOLS: &[(Tool, Name, &str, &str)] = &[
    // 选择族
    (Tool::Select, Name::ToolSelect, "选择", "V"),
    (Tool::DirectSelect, Name::ToolDirectSelect, "直接选择", "A"),
    (Tool::GroupSelect, Name::ToolGroupSelect, "编组选择", "Y"),
    // 路径族
    (Tool::Pen, Name::KindVector, "钢笔", "P"),
    (Tool::Scissors, Name::ToolScissors, "剪刀", "C"),
    // 形状族
    (Tool::Rect, Name::ToolRect, "矩形", "M"),
    (Tool::Ellipse, Name::ToolEllipse, "椭圆", "L"),
    (Tool::Line, Name::Crosshair, "直线", "\\"),
    // 文字 / 画板(各自独立,无同族展开)
    (Tool::Text, Name::ToolText, "文字", "T"),
    (Tool::Artboard, Name::ToolArtboard, "画板", "Shift+O"),
    // 上色族
    (Tool::Gradient, Name::ToolGradient, "渐变", "G"),
    (Tool::Eyedropper, Name::ToolEyedropper, "吸管", "I"),
    // 视图族
    (Tool::Zoom, Name::ZoomIn, "缩放", "Z"),
    (Tool::Hand, Name::ToolHand, "抓手", "H"),
];

/// 全部工具箱条目(门禁测试与渲染共用)。
pub const TOOLBOX: &[(Tool, Name, &str, &str)] = TOOLS;

/// 同族工具(07-1-7「工具长按展开同组」的数据源)。
///
/// 只有**多于一个成员**的族才值得展开(文字 / 画板返回自身,长度 1 → 不出菜单)。
pub fn family_of(tool: Tool) -> &'static [(Tool, Name, &'static str, &'static str)] {
    let i = TOOLS
        .iter()
        .position(|(t, ..)| *t == tool)
        .unwrap_or(usize::MAX);
    match i {
        0..=2 => &TOOLS[0..3],
        3..=4 => &TOOLS[3..5],
        5..=7 => &TOOLS[5..8],
        8 => &TOOLS[8..9],
        9 => &TOOLS[9..10],
        10..=11 => &TOOLS[10..12],
        12..=13 => &TOOLS[12..14],
        _ => &TOOLS[0..0],
    }
}

impl VellumApp {
    /// 按当前停靠位装配工具箱 Panel(`ui()` 在画布之前调用)。
    ///
    /// **装配位置由调用方决定**(底向必须在状态栏之前,保证状态栏永远最底),
    /// 本函数只负责"往该边放一个 Panel"。
    pub(crate) fn docked_toolbar(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(self.theme_dark);
        let (w, h) = super::dock_layout::toolbar_size(self.toolbar_dock, self.toolbar_columns);
        let side = self.toolbar_dock;
        let frame = egui::Frame::new()
            .fill(t.bg_raised)
            .stroke(Stroke::new(1.0, t.border))
            .inner_margin(theme::space::S2);

        let size = if side.is_vertical() { w } else { h };
        let panel = match side {
            DockSide::Top => egui::Panel::top("vb-toolbar"),
            DockSide::Bottom => egui::Panel::bottom("vb-toolbar"),
            DockSide::Left => egui::Panel::left("vb-toolbar"),
            DockSide::Right => egui::Panel::right("vb-toolbar"),
        };
        panel
            .exact_size(size)
            .resizable(false)
            .frame(frame)
            .show(ui, |ui| {
                self.toolbar_body(ui, side);
            });

        // 拖动中的吸附预览带(画在最上层,不参与布局)
        if let Some(preview) = self.toolbar_dock_preview {
            let screen = ui.ctx().viewport_rect();
            let band = 6.0;
            let rect = match preview {
                DockSide::Top => egui::Rect::from_min_max(
                    screen.min,
                    egui::pos2(screen.max.x, screen.min.y + band),
                ),
                DockSide::Bottom => egui::Rect::from_min_max(
                    egui::pos2(screen.min.x, screen.max.y - band),
                    screen.max,
                ),
                DockSide::Left => egui::Rect::from_min_max(
                    screen.min,
                    egui::pos2(screen.min.x + band, screen.max.y),
                ),
                DockSide::Right => egui::Rect::from_min_max(
                    egui::pos2(screen.max.x - band, screen.min.y),
                    screen.max,
                ),
            };
            let painter = ui.ctx().layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("vb-dock-preview"),
            ));
            painter.rect_filled(rect, 0.0, theme::semantic::SELECT_BOX);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("吸附到{}", preview.label()),
                egui::FontId::proportional(12.0),
                t.text,
            );
        }
    }

    fn toolbar_body(&mut self, ui: &mut egui::Ui, side: DockSide) {
        let vertical = side.is_vertical();
        if vertical {
            ui.vertical_centered(|ui| {
                self.toolbar_grip(ui, true);
                ui.add_space(theme::space::S2);
                self.toolbar_buttons(ui, true);
            });
        } else {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::space::S2;
                self.toolbar_grip(ui, false);
                ui.add_space(theme::space::S2);
                self.toolbar_buttons(ui, false);
            });
        }
    }

    /// 拖动把手(⠿):拖动中算吸附边,松手生效。
    fn toolbar_grip(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let t = Tokens::get(self.theme_dark);
        let text = if vertical { "⠿\n⠿" } else { "⠿" };
        let grip = ui.add(
            egui::Label::new(egui::RichText::new(text).color(t.text_3))
                .sense(Sense::drag())
                .selectable(false),
        );
        let grip = grip.on_hover_text(format!(
            "拖动到窗口边缘吸附停靠(当前:{}{})",
            self.toolbar_dock.label(),
            match self.toolbar_dock {
                s if s.is_vertical() => format!(" · {} 列", self.toolbar_columns),
                _ => String::new(),
            }
        ));
        if grip.drag_started() {
            self.toolbar_dragging = true;
            self.toolbar_dock_preview = None;
        }
        if grip.dragged() {
            if let Some(p) = grip.interact_pointer_pos() {
                let s = ui.ctx().viewport_rect().size();
                self.toolbar_dock_preview = DockSide::nearest(p.x, p.y, s.x, s.y, SNAP_BAND);
            }
        }
        if grip.drag_stopped() {
            self.toolbar_dragging = false;
            if let Some(side) = self.toolbar_dock_preview.take() {
                self.set_toolbar_dock(side);
            } else {
                self.say("工具栏:未吸附到边缘,已回弹(不做浮动)");
            }
        }
    }

    /// 工具按钮(左/右停靠时按列数手工分行)。
    ///
    /// **同族展开(07-1-7)**:右键任一工具 → 列出同族工具并可直切
    /// (桌面端以右键触发,等价于「长按」且不与拖拽冲突)。
    fn toolbar_buttons(&mut self, ui: &mut egui::Ui, vertical: bool) {
        let columns = if vertical {
            self.toolbar_columns.clamp(1, 2) as usize
        } else {
            TOOLS.len()
        };
        let current = self.tool;
        let mut fired: Option<&'static str> = None;
        let mut place = |ui: &mut egui::Ui, row: &[(Tool, Name, &'static str, &'static str)]| {
            for (tool, icon, label, key) in row {
                let resp = tool_button(ui, *tool, *icon, label, key, current);
                if resp.clicked() {
                    fired = Some(command_of(*tool));
                }
                let fam = family_of(*tool);
                if fam.len() > 1 {
                    resp.context_menu(|ui| {
                        ui.label(vb_ui::components::caption(ui, "同族工具"));
                        for (t2, _, lb2, k2) in fam {
                            let text = if k2.is_empty() {
                                (*lb2).to_string()
                            } else {
                                format!("{lb2}\t{k2}")
                            };
                            if ui.selectable_label(current == *t2, text).clicked() {
                                fired = Some(command_of(*t2));
                                ui.close();
                            }
                        }
                    });
                }
            }
        };
        if vertical {
            ui.vertical(|ui| {
                for row in TOOLS.chunks(columns) {
                    ui.horizontal(|ui| place(ui, row));
                }
            });
        } else {
            ui.horizontal(|ui| place(ui, TOOLS));
        }
        if let Some(id) = fired {
            self.run_command(id, false, false);
        }
    }

    /// 切换停靠位(命令与拖动共用):记住 → 持久化 → 状态提示。
    pub(crate) fn set_toolbar_dock(&mut self, side: DockSide) {
        self.toolbar_dock = side;
        self.toolbar_columns = super::dock_layout::clamp_columns(side, self.toolbar_columns);
        self.say(format!("工具栏:停靠到{}", side.label()));
        self.save_workspace();
    }

    /// 切换单/双列(仅左/右停靠有效)。
    pub(crate) fn set_toolbar_columns(&mut self, columns: u8) {
        if !self.toolbar_dock.is_vertical() {
            self.toast_warn("单/双列仅在左/右停靠时可用(顶/底停靠是单行)");
            return;
        }
        self.toolbar_columns = columns.clamp(1, 2);
        self.say(format!("工具栏:{} 列", self.toolbar_columns));
        self.save_workspace();
    }

    /// 组装当前工作区配置(持久化用)。
    pub(crate) fn workspace_config(&self) -> super::dock_layout::WorkspaceConfig {
        super::dock_layout::WorkspaceConfig {
            schema_version: super::dock_layout::SCHEMA_VERSION,
            toolbar_dock: self.toolbar_dock,
            toolbar_columns: self.toolbar_columns,
            dock_collapsed: self.dock_collapsed,
            panel_order: self.panel_order.to_vec(),
            panel_tab: self.panel_tab,
            panels_hidden: self.panels_hidden,
        }
    }

    /// 写回 `workspace.json`(失败 → toast,不静默)。成功则刷新脏检查快照。
    pub(crate) fn save_workspace(&mut self) {
        let cfg = self.workspace_config();
        match super::dock_layout::save(&cfg) {
            Ok(()) => self.workspace_saved = cfg,
            Err(e) => self.toast_warn(format!("布局未持久化:{e}")),
        }
    }

    /// 当前布局是否与**已落盘快照**不同(逐字段比较,避免每帧构造 Vec)。
    pub(crate) fn workspace_dirty(&self) -> bool {
        let a = &self.workspace_saved;
        a.toolbar_dock != self.toolbar_dock
            || a.toolbar_columns != self.toolbar_columns
            || a.dock_collapsed != self.dock_collapsed
            || a.panel_tab != self.panel_tab
            || a.panels_hidden != self.panels_hidden
            || a.panel_order.as_slice() != self.panel_order.as_slice()
    }
}

/// 一个工具按钮;返回其 `Response`(调用方据此判点击 / 挂同族展开菜单)。
fn tool_button(
    ui: &mut egui::Ui,
    tool: Tool,
    icon: Name,
    label: &str,
    key: &str,
    current: Tool,
) -> egui::Response {
    ToolButton::new(icon, label)
        .shortcut(key)
        .with_label()
        .active(current == tool)
        .ui(ui)
}

/// 工具 → 命令 id(与快捷键同一条命令路径,统一清理工具状态)。
fn command_of(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "tool.select",
        Tool::Rect => "tool.rect",
        Tool::Ellipse => "tool.ellipse",
        Tool::Line => "tool.line",
        Tool::Pen => "tool.pen",
        Tool::DirectSelect => "tool.direct_select",
        Tool::Zoom => "tool.zoom",
        Tool::Hand => "tool.hand",
        Tool::Text => "tool.text",
        Tool::Eyedropper => "tool.eyedropper",
        Tool::Artboard => "tool.artboard",
        Tool::Gradient => "tool.gradient",
        Tool::Scissors => "tool.scissors",
        Tool::GroupSelect => "tool.group_select",
    }
}
