//! 右侧面板坞(S1-b 02-1 重构):
//! - **可折叠**:展开 = 可拖宽 240–420(默认 280)的完整坞;
//!   折叠 = 40px 图标条(每 Tab 一个图标,点击展开并切到该 Tab;
//!   窗口 <1200 强制折叠,此时点击只切 Tab,见 `vb_ui::dock`);
//! - **Tab 4 页**:属性 / 图层 / 画板 / 令牌;顺序右键可换(内存,
//!   持久化到 workspace.json 为阶段 7 项);
//! - FPS 底栏与底部状态栏(照旧)。
//!
//! 折叠只改 egui 布局,不触碰 GPU surface(wgpu 纹理由 `canvas.rs`
//! 持有,与本模块无生命周期耦合)—— 02 篇 §6 风险项的落地方式。

/// `pub(crate)`:行拖拽状态(`layers::LayerDrag`)挂 `VellumApp`,
/// 画板预设常量表(`artboards::AB_PRESETS`)供控制面板共享
/// (02-5-5 常量单一来源)。
/// `charpara` 的构建器(`Align9` / `para_align_cmds` / 溢出估算)与
/// 字段清单常量供画布(红点/双击扩高)与门禁测试共享。
pub(crate) mod artboards;
pub(crate) mod charpara;
pub(crate) mod layers;
mod properties;
mod tokens;

use vb_ui::components::{icon_button, PanelTabs};
use vb_ui::dock;
use vb_ui::icons::{self, Name};
use vb_ui::theme;

use crate::app::VellumApp;

/// Tab 语义 id(存储值;顺序见 `VellumApp::panel_order`)。
pub(crate) const TAB_PROPERTIES: usize = 0;
pub(crate) const TAB_LAYERS: usize = 1;
pub(crate) const TAB_ARTBOARDS: usize = 2;
pub(crate) const TAB_TOKENS: usize = 3;
pub(crate) const TAB_COUNT: usize = 4;

// 语义 id 连续且落在数组下标范围内(编译期锁定;match 里以 `_` 兜底令牌页)
const _: () = assert!(TAB_TOKENS == TAB_COUNT - 1);

/// Tab 标题(下标 = 语义 id)。`pub` 供「窗口」菜单命令做状态提示(阶段 5)。
pub const TAB_LABELS: [&str; TAB_COUNT] = ["属性", "图层", "画板", "令牌"];
/// 折叠图标条上的图标(下标 = 语义 id)。
const TAB_ICONS: [Name; TAB_COUNT] = [
    Name::PanelProperties,
    Name::KindLayer,
    Name::ToolArtboard,
    Name::PanelTokens,
];

impl VellumApp {
    pub(crate) fn right_panel(&mut self, ui: &mut egui::Ui) {
        let width = ui.ctx().viewport_rect().width();
        // 折叠是「窗口宽 + 用户偏好」的纯函数(规则与单测在 vb_ui::dock)
        if dock::should_collapse(width, self.dock_collapsed) {
            self.dock_rail(ui, width);
        } else {
            self.dock_expanded(ui);
        }
    }

    /// 折叠态:40px 图标条。点击图标 → 切到该 Tab;<1200 时**不**展开
    /// (强制折叠,见 `dock::rail_click_can_expand`),≥1200 才展开。
    fn dock_rail(&mut self, ui: &mut egui::Ui, viewport_width: f32) {
        egui::Panel::right("dock_rail")
            .exact_size(dock::ICON_RAIL_WIDTH)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::tokens(ui.ctx()).bg_panel))
            .show(ui, |ui| {
                let t = theme::tokens(ui.ctx());
                ui.add_space(theme::space::S2);
                // 顶部展开按钮(强制折叠窗口下只给视觉反馈,点了不展开)
                let can_expand = dock::rail_click_can_expand(viewport_width);
                if icon_button(ui, Name::Expanded, "展开面板坞").clicked() && can_expand {
                    self.dock_collapsed = false;
                }
                ui.separator();
                for slot in 0..TAB_COUNT {
                    let tab = self.panel_order[slot];
                    let selected = self.panel_tab == tab;
                    let hint = if can_expand {
                        format!("{}(点击展开并切换)", TAB_LABELS[tab])
                    } else {
                        format!("{}(窗口过窄,仅切换)", TAB_LABELS[tab])
                    };
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::Vec2::splat(theme::space::ROW_HEIGHT),
                        egui::Sense::click(),
                    );
                    let hover_t = ui.ctx().animate_bool_with_time(
                        ui.id().with(("vbrail", tab)),
                        resp.hovered(),
                        theme::motion::HOVER,
                    );
                    let fill = if selected {
                        t.accent_dim
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let _ = hover_t;
                    ui.painter().rect_filled(rect, theme::radius::md(), fill);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        TAB_ICONS[tab].glyph().to_string(),
                        icons::font(16.0),
                        if selected { t.accent } else { t.text_2 },
                    );
                    if resp.clicked() {
                        self.panel_tab = tab;
                        if can_expand {
                            self.dock_collapsed = false;
                        }
                    }
                    let _ = resp.on_hover_text(hint);
                }
            });
    }

    /// 展开态:完整面板坞(280,可拖 240–420)。
    fn dock_expanded(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("props")
            .default_size(theme::space::DOCK_WIDTH)
            .size_range(dock::DOCK_MIN..=dock::DOCK_MAX)
            .resizable(true)
            .show(ui, |ui| {
                // Tab 条(顺序 = panel_order;右键 Tab 可重排,内存)
                let labels: Vec<&str> = self.panel_order.iter().map(|&t| TAB_LABELS[t]).collect();
                let mut slot = self
                    .panel_order
                    .iter()
                    .position(|&t| t == self.panel_tab)
                    .unwrap_or(0);
                let mut reorder: Option<(usize, i32)> = None;
                ui.horizontal(|ui| {
                    // 折叠按钮(F7 折的是图层语义;这里折整个坞)
                    if icon_button(ui, Name::Collapsed, "折叠面板坞").clicked() {
                        self.dock_collapsed = true;
                    }
                    let r = PanelTabs::new(&labels, &mut slot)
                        .reorderable(true)
                        .ui_ex(ui);
                    if r.changed {
                        self.panel_tab = self.panel_order[slot];
                    }
                    reorder = r.reorder;
                });
                if let Some((i, dir)) = reorder {
                    let j = (i as i32 + dir).clamp(0, (TAB_COUNT - 1) as i32) as usize;
                    self.panel_order.swap(i, j);
                }
                match self.panel_tab {
                    TAB_PROPERTIES => self.properties_tab(ui),
                    TAB_LAYERS => self.layers_tab(ui),
                    TAB_ARTBOARDS => self.artboards_tab(ui),
                    _ => self.tokens_tab(ui),
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.separator();
                    if !self.frame_times.is_empty() {
                        let avg: f32 =
                            self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
                        ui.label(format!(
                            "FPS {:.0} · 帧时间 {:.1}ms · 节点 {}",
                            1.0 / avg,
                            avg * 1000.0,
                            self.doc.nodes.len()
                        ));
                    }
                });
            });
    }

    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let backend = frame
            .wgpu_render_state()
            .map(|rs| format!("{:?}", rs.adapter.get_info().backend))
            .unwrap_or_else(|| "无 GPU".into());
        // 画板导航 ◀ 1/N ▶(S1-d 02-5-4):当前序号从选中画板**派生**
        // (画布改选 → 状态栏自动同步);◀▶ 复用 view.prev/next_artboard
        // 命令(选中 + 视口跳转),与画布/面板双向联动。
        let ab_idx = self
            .selection
            .first()
            .and_then(|s| self.doc.find_by_sid(s))
            .and_then(|id| self.doc.artboards.iter().position(|&a| a == id))
            .or_else(|| {
                self.active_artboard()
                    .and_then(|a| self.doc.artboards.iter().position(|&x| x == a))
            });
        let ab_n = self.doc.artboards.len();
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ab_n > 0 {
                    if icon_button(ui, Name::PrevArtboard, "上一画板(Ctrl+PageUp)").clicked() {
                        self.run_command("view.prev_artboard", false, false);
                    }
                    ui.label(format!("{}/{}", ab_idx.map(|i| i + 1).unwrap_or(1), ab_n));
                    if icon_button(ui, Name::NextArtboard, "下一画板(Ctrl+PageDown)").clicked()
                    {
                        self.run_command("view.next_artboard", false, false);
                    }
                    ui.separator();
                }
                ui.label(format!("{}%", (self.camera.zoom * 100.0) as i64));
                ui.separator();
                ui.label(format!(
                    "⌖ {:.0}, {:.0}",
                    self.cursor_world.0, self.cursor_world.1
                ));
                ui.separator();
                ui.label(format!("选中 {}", self.selection.len()));
                if self.outline_mode {
                    ui.separator();
                    ui.label("轮廓");
                }
                ui.separator();
                ui.label(format!("rev {} · {}", self.doc.rev, backend));
                ui.separator();
                ui.strong(&self.status);
            });
        });
    }
}
