//! 工具箱(阶段 6 / 副文档 07):**四向停靠** + 单/双列 + 拖动吸附。
//!
//! 与旧「底部浮动条」的差别(S1-a 起的历史行为在这里改写):
//! - 由 `egui::Area`(浮动、锚 `CENTER_BOTTOM`)改为 **`egui::Panel`**,
//!   按 [`DockSide`] 走 `top/left/right/bottom` 分支;
//! - 顶部加**拖动把手**:拖动中实时算吸附边并画预览带,松手生效
//!   (落在中间 → 回弹,不做浮动 —— 与 `design/14` F8 精神一致);
//! - 左/右停靠可切**单列 / 双列**;
//! - 停靠位与列数**写回 `workspace.json`**(07-2),重启还原。
//!
//! **04-6(副文档 04,P1-⑥)**:
//! - 按**族分组 + 组间分隔线**(选择族 | 形状族 | 路径族 | 文字·上色 |
//!   视图族),消灭「一行 16+ 小图标无分组」的密集感;
//! - 图标尺寸统一从行高派生(`theme::row_height`),不再写死;
//! - 顶/底停靠时按可用宽**换行**(面板高度随之抬升),任何窗宽不溢出;
//! - 「显示未支持工具」开关(编辑 → 设置 → 显示未支持工具,design/06 §二):
//!   默认隐藏;开启后未支持工具**置灰**排在对应族尾,点击弹「计划于 vX」
//!   提示 —— 绝不「点了没反应」。

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
/// **顺序 = 04-6-1 的族分组 + 05-2 扩充**(同族相邻,分隔线据
/// [`GROUP_ENDS`] 切):选择族 | 变换族(X-4)| 形状族 |
/// 路径族(钢笔·铅笔·曲率·剪刀)| 文字·画板·上色·切片 | 视图族。
const TOOLS: &[(Tool, Name, &str, &str)] = &[
    // 选择族
    (Tool::Select, Name::ToolSelect, "选择", "V"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::DirectSelect, Name::ToolDirectSelect, "直接选择", "A"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::GroupSelect, Name::ToolGroupSelect, "编组选择", "Y"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    // 变换族(05-2 X-4:单击设中心 → 拖拽变换;06 篇 §3.10)
    (Tool::Rotate, Name::ToolRotate, "旋转", "R"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Mirror, Name::ToolMirror, "镜像", "O"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Scale, Name::ToolScale, "缩放", "S"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (
        Tool::FreeTransform,
        Name::ToolFreeTransform,
        "自由变换", // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
        "E",
    ),
    // 形状族
    (Tool::Rect, Name::ToolRect, "矩形", "M"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Ellipse, Name::ToolEllipse, "椭圆", "L"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Line, Name::Crosshair, "直线", "\\"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    // 路径族(05-2 X-5:铅笔 N 自由绘制、曲率 Shift+` 自动拟合)
    (Tool::Pen, Name::KindVector, "钢笔", "P"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Pencil, Name::ToolPencil, "铅笔", "N"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Curvature, Name::ToolCurvature, "曲率", "Shift+`"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Scissors, Name::ToolScissors, "剪刀", "C"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    // 文字 / 画板 / 上色(吸管·渐变)/ 切片(09-C)
    (Tool::Text, Name::ToolText, "文字", "T"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Artboard, Name::ToolArtboard, "画板", "Shift+O"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Eyedropper, Name::ToolEyedropper, "吸管", "I"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Gradient, Name::ToolGradient, "渐变", "G"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Slice, Name::KindSlice, "切片", "Shift+K"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    // 视图族(05-2 09-E:度量工具入视图族)
    (Tool::Zoom, Name::ZoomIn, "缩放", "Z"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Hand, Name::ToolHand, "抓手", "H"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
    (Tool::Measure, Name::ToolMeasure, "度量", ""), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
];

/// 全部工具箱条目(门禁测试与渲染共用)。
pub const TOOLBOX: &[(Tool, Name, &str, &str)] = TOOLS;

/// 族边界(TOOLS 下标;`GROUP_ENDS[i]` = 第 i 组的**末下标 + 1**)。
///
/// 组数 = `GROUP_ENDS.len()`;组间分隔线数量 = 组数 − 1。
/// 「同族展开」([`family_of`],07-1-7 右键菜单)沿用**同一张表**,
/// 只有文字/画板/切片是刻意的单成员族(设计如此,无同族可展开)。
const GROUP_ENDS: [usize; 6] = [3, 7, 10, 14, 19, 22];

/// 第 `g` 组在 TOOLS 中的下标区间(纯函数;分组几何门禁打这里)。
pub fn group_range(g: usize) -> std::ops::Range<usize> {
    let start = if g == 0 { 0 } else { GROUP_ENDS[g - 1] };
    let end = GROUP_ENDS.get(g).copied().unwrap_or(GROUP_ENDS[0]);
    start..end.max(start)
}

/// 工具所属组下标(纯函数;渲染据此插组间分隔线)。
pub fn group_of(tool: Tool) -> usize {
    let i = TOOLS
        .iter()
        .position(|(t, ..)| *t == tool)
        .unwrap_or(usize::MAX);
    GROUP_ENDS
        .iter()
        .position(|&end| i < end)
        .unwrap_or(GROUP_ENDS.len() - 1)
}

/// 同族工具(07-1-7「工具长按/右键展开同组」的数据源)。
///
/// **族 ≠ 视觉分组**:`GROUP_ENDS` 是 04-6-1 的分隔线分组;族是「可互相
/// 展开切换」的语义,沿用阶段 6 的裁定 —— 文字 / 画板 / 切片是刻意的
/// 单成员族(无同族可展开,不出菜单),吸管+渐变构成上色族。
pub fn family_of(tool: Tool) -> &'static [(Tool, Name, &'static str, &'static str)] {
    let i = TOOLS
        .iter()
        .position(|(t, ..)| *t == tool)
        .unwrap_or(usize::MAX);
    match i {
        0..=2 => &TOOLS[0..3],     // 选择族
        3..=6 => &TOOLS[3..7],     // 变换族(X-4)
        7..=9 => &TOOLS[7..10],    // 形状族
        10..=13 => &TOOLS[10..14], // 路径族(钢笔·铅笔·曲率·剪刀)
        14 => &TOOLS[14..15],      // 文字(单成员,不出菜单)
        15 => &TOOLS[15..16],      // 画板(单成员,不出菜单)
        16..=17 => &TOOLS[16..18], // 上色族(吸管 + 渐变)
        18 => &TOOLS[18..19],      // 切片(单成员,不出菜单)
        19..=21 => &TOOLS[19..22], // 视图族
        _ => &TOOLS[0..0],
    }
}

/// 工具箱一行的**需求宽**(pt;04-6-2「不许溢出」的判定输入)。
///
/// 组成:把手(一行高)+ Σ 工具按钮(一行高)+ 组间分隔(每个 ≈ 行高 × 0.6)+ 项间距。
///
/// `row` = 按钮边长(派生行高,见 `theme::row_height`)。
/// 顶/底停靠时调用方按「需求宽 ÷ 可用宽」向上取整决定行数。
pub fn toolbar_row_width_with(row: f32) -> f32 {
    let tools = TOOLBOX.len() as f32;
    let seps = (GROUP_ENDS.len() - 1) as f32;
    let spacing = theme::space::S2;
    // 把手 + 按钮 + 分隔 + 每项一个间距 + 余量
    row + tools * (row + spacing) + seps * row * 0.6 + spacing * 4.0
}

/// 顶/底停靠在 `avail_width` 宽、按钮边长 `row` 下需要的行数(纯函数;≥1)。
pub fn rows_for_width(avail_width: f32, row: f32) -> usize {
    let need = toolbar_row_width_with(row);
    ((need / avail_width.max(1.0)).ceil() as usize).clamp(1, 3)
}

/// 05-2(09-C):「切片」已从占位转正为真工具(design/06 §3.14);
/// 占位表现已为空 —— Dropped 工具(实时上色等)按 05-1 裁定**不出现在
/// 工具箱**(不留灰按钮,UI 无入口)。
const PLANNED_TOOLS: &[(Name, &str, &str)] = &[];

impl VellumApp {
    /// 按当前停靠位装配工具箱 Panel(`ui()` 在画布之前调用)。
    ///
    /// **装配位置由调用方决定**(底向必须在状态栏之前,保证状态栏永远最底),
    /// 本函数只负责vb_session::i18n::t("ui-app-toolbar-008")。
    pub(crate) fn docked_toolbar(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(self.theme_dark);
        let side = self.toolbar_dock;
        // 04-6-2:顶/底停靠横向过窄 → 换行(面板高度 = 行数 × 控行高),
        // 内容用 horizontal_wrapped 流式铺排,任何窗宽都不溢出。
        let row_h = theme::row_height(ui.ctx());
        let rows = if side.is_vertical() {
            1
        } else {
            rows_for_width(ui.ctx().viewport_rect().width(), row_h)
        };
        let (w, h) = if side.is_vertical() {
            let (w, _) = super::dock_layout::toolbar_size(self.toolbar_dock, self.toolbar_columns);
            (w, 0.0)
        } else {
            (
                0.0,
                (theme::space::CONTROL_BAR_HEIGHT).max(rows as f32 * (row_h + theme::space::S4)),
            )
        };
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
                vb_session::i18n::t_args(
                    "ui-app-toolbar-009",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from((preview.label()).to_string()),
                    )],
                ),
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
        let grip = grip.on_hover_text(vb_session::i18n::t_args(
            "ui-app-toolbar-010",
            &[
                (
                    "a1",
                    vb_session::i18n::FluentValue::from((self.toolbar_dock.label()).to_string()),
                ),
                (
                    "a2",
                    vb_session::i18n::FluentValue::from(
                        (match self.toolbar_dock {
                            s if s.is_vertical() => vb_session::i18n::t_args(
                                "ui-app-toolbar-011",
                                &[(
                                    "a1",
                                    vb_session::i18n::FluentValue::from(
                                        (self.toolbar_columns).to_string(),
                                    ),
                                )],
                            ),
                            _ => String::new(),
                        })
                        .to_string(),
                    ),
                ),
            ],
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
                self.say(vb_session::i18n::t("ui-app-toolbar-012"));
            }
        }
    }

    /// 工具按钮(左/右停靠时按列数手工分行;组间插分隔线)。
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
        // U-4/§8.6 #1:长按 200ms 展开同族 —— 状态在闭包里收集,循环外
        // 统一落到 `family_popup`(借用分离:闭包不碰 self)。
        let mut longpress: Option<(Tool, egui::Pos2)> = None;
        let mut place = |ui: &mut egui::Ui, row: &[(Tool, Name, &'static str, &'static str)]| {
            for (tool, icon, label, key) in row {
                let resp = tool_button(ui, *tool, *icon, label, key, current);
                let fam_len = family_of(*tool).len();
                // ── 长按计时(egui 会话内存;按住逐帧续写,松手清零)──
                // 状态 = (起始时刻, 本次按住是否已触发)。触发过的按住
                // 在松手帧仍要抑制 click,故先读上一帧的 fired 再清。
                let lp_key = egui::Id::new("vb-toolbar-lp").with(*label);
                let (was_fired, now_fired) = ui.ctx().data_mut(|d| {
                    let prev: Option<(std::time::Instant, bool)> = d.get_temp(lp_key);
                    let was_fired = prev.map(|(_, f)| f).unwrap_or(false);
                    let down = resp.is_pointer_button_down_on();
                    let mut next = match (down, prev) {
                        (true, Some(st)) => Some(st),
                        (true, None) => Some((std::time::Instant::now(), false)),
                        (false, _) => None,
                    };
                    let mut fired_now = false;
                    if let (true, Some((t0, fired))) = (down, next.as_mut()) {
                        if !*fired
                            && fam_len > 1
                            && t0.elapsed() >= std::time::Duration::from_millis(200)
                        {
                            *fired = true;
                            fired_now = true;
                        }
                    }
                    d.insert_temp(lp_key, next);
                    (was_fired, fired_now)
                });
                if now_fired {
                    longpress = Some((*tool, resp.rect.left_bottom()));
                }
                if resp.clicked() && !was_fired {
                    fired = Some(command_of(*tool));
                }
                let fam = family_of(*tool);
                if fam.len() > 1 {
                    resp.context_menu(|ui| {
                        ui.label(vb_ui::components::caption(
                            ui,
                            &vb_session::i18n::t("ui-app-toolbar-013"),
                        ));
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

        // 04-6-1:按组分块渲染,组间插分隔线(竖排 = 横线,横排 = 竖线)。
        // 每组内部再按列数切行(左/右停靠单双列)。
        if vertical {
            ui.vertical(|ui| {
                for g in 0..GROUP_ENDS.len() {
                    if g > 0 {
                        ui.add_space(theme::space::S2);
                        ui.separator();
                        ui.add_space(theme::space::S2);
                    }
                    for row in TOOLS[group_range(g)].chunks(columns) {
                        ui.horizontal(|ui| place(ui, row));
                    }
                }
                // 04-6:未支持工具(置灰,组间同样分隔)
                if self.show_all_tools {
                    ui.add_space(theme::space::S2);
                    ui.separator();
                    ui.add_space(theme::space::S2);
                    for row in PLANNED_TOOLS.chunks(columns) {
                        ui.horizontal(|ui| self.place_planned(ui, row));
                    }
                }
            });
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = theme::space::S2;
                for g in 0..GROUP_ENDS.len() {
                    if g > 0 {
                        ui.separator();
                    }
                    place(ui, &TOOLS[group_range(g)]);
                }
                if self.show_all_tools {
                    ui.separator();
                    self.place_planned(ui, PLANNED_TOOLS);
                }
            });
        }
        if let Some(id) = fired {
            self.run_command(id, false, false);
        }
        // U-4:本次按住触发了长按 → 打开同族弹层(松手 click 已按
        // `was_fired` 抑制,不会同时切工具)。
        if let Some((tool, pos)) = longpress {
            self.family_popup = Some((tool, pos));
            self.family_popup_arm_release = true;
        }
    }

    /// U-4:长按展开的同族工具弹层(帧级渲染;frame.rs 调用)。
    ///
    /// 交互:点条目 = 切换工具并收起;点弹层外 = 收起;长按松手的同一次
    /// click 由 `family_popup_arm_release` 吞掉(否则弹层刚开就被松手
    /// 关闭,长按永远白按)。
    pub(crate) fn family_popup_ui(&mut self, ui: &mut egui::Ui) {
        let Some((tool, pos)) = self.family_popup else {
            return;
        };
        let fam: Vec<(Tool, Name, &'static str, &'static str)> = family_of(tool).to_vec();
        let t = Tokens::get(self.theme_dark);
        let current = self.tool;
        let mut chosen: Option<&'static str> = None;
        let mut picked = false;
        let mut popup_rect = egui::Rect::NOTHING;
        egui::Area::new(egui::Id::new("vb-family-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(t.bg_raised)
                    .stroke(Stroke::new(theme::stroke::HAIRLINE, t.border))
                    .corner_radius(theme::radius::lg())
                    .inner_margin(theme::space::S3)
                    .show(ui, |ui| {
                        ui.set_min_width(128.0);
                        ui.label(vb_ui::components::caption(
                            ui,
                            &vb_session::i18n::t("ui-app-toolbar-014"),
                        ));
                        ui.separator();
                        for (t2, icon, lb2, k2) in &fam {
                            let text = vb_ui::components::key_badge_text(lb2, k2);
                            let btn = ui.add(
                                egui::Button::new(
                                    egui::RichText::new(format!("{}  {text}", icon.glyph())).font(
                                        egui::FontId::new(12.0, vb_ui::fonts::family_regular()),
                                    ),
                                )
                                .min_size(egui::vec2(ui.available_width(), 20.0))
                                .fill(if current == *t2 {
                                    t.accent_dim
                                } else {
                                    egui::Color32::TRANSPARENT
                                }),
                            );
                            if btn.clicked() {
                                chosen = Some(command_of(*t2));
                                picked = true;
                            }
                        }
                    });
                popup_rect = ui.min_rect();
            });
        // 关闭判定:长按松手的那次 click 先被 arm 标记吞掉;此后
        // 「点弹层外」或「选中条目」才真正收起。
        let released_now = ui.input(|i| i.pointer.any_released());
        if self.family_popup_arm_release {
            if released_now {
                self.family_popup_arm_release = false;
            }
        } else {
            let outside_click = ui.input(|i| i.pointer.any_click())
                && !popup_rect.contains(ui.input(|i| i.pointer.interact_pos()).unwrap_or_default());
            if outside_click {
                self.family_popup = None;
            }
        }
        if let Some(id) = chosen {
            self.run_command(id, false, false);
        }
        if picked {
            self.family_popup = None;
        }
    }

    /// 未支持工具占位:置灰图标,悬停见「计划于 vX」,点击给状态/toast 提示
    /// (design/06 §二:绝不「点了没反应」)。
    fn place_planned(&mut self, ui: &mut egui::Ui, row: &[(Name, &'static str, &'static str)]) {
        for (icon, label, tip) in row {
            let side = theme::row_height(ui.ctx());
            let resp = ToolButton::new(*icon, label)
                .with_label()
                .size(side)
                .enabled(false)
                .ui(ui);
            let resp = resp.on_hover_text(*tip);
            if resp.clicked() {
                self.toast_warn((*tip).to_string());
            }
        }
    }

    /// 底部浮动工具条(§8.7 ⭐ vb_session::i18n::t("ui-app-toolbar-015")):画布底部居中的
    /// 浮层 Area —— 高 40、圆角 12、**L4 材质**(中性阶按下标 7 档 +
    /// 表面 96%/92% 不透明 + L4 阴影)、图标 20,当前工具 accent。
    /// 工具箱停靠在底部时自动隐藏(同一排工具不画两遍);
    /// `Tab` 隐藏面板时一起隐藏(frame.rs 装配)。
    pub(crate) fn floating_toolbar(&mut self, ui: &mut egui::Ui) {
        let t = Tokens::get(self.theme_dark);
        // 常用八件:选择族 + 形状 + 文字 + 钢笔 + 抓手(§8.7 示意的
        // "➤ ▭ ◯ T P ╲ ✋");完整 22 工具仍在工具箱。
        const FLOAT_TOOLS: &[(Tool, Name, &str, &str)] = &[
            (Tool::Select, Name::ToolSelect, "选择", "V"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
            (Tool::Rect, Name::ToolRect, "矩形", "M"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
            (Tool::Ellipse, Name::ToolEllipse, "椭圆", "L"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
            (Tool::Line, Name::Crosshair, "直线", "\\"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
            (Tool::Text, Name::ToolText, "文字", "T"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
            (Tool::Pen, Name::KindVector, "钢笔", "P"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
            (Tool::Hand, Name::ToolHand, "抓手", "H"), // vb-literal-ok: const/static 表的 &str 值,fn 化留后续(en 缺失记录台账)
        ];
        let current = self.tool;
        let zoom_pct = (self.camera.zoom * 100.0).round() as i64;
        let mut fired: Option<&'static str> = None;
        let mut fit_clicked = false;
        egui::Area::new(egui::Id::new("vb-floating-toolbar"))
            .order(egui::Order::Foreground)
            .anchor(
                egui::Align2::CENTER_BOTTOM,
                egui::vec2(0.0, -(theme::space::STATUS_BAR_HEIGHT + theme::space::S6)),
            )
            .show(ui.ctx(), |ui| {
                // L4 材质:底色取中性阶(深 N7/浅 N1)+ 表面不透明度
                let base = theme::elevation::base(theme::elevation::L4, &t);
                let [r, g, b, _] = base.to_srgba_unmultiplied();
                let alpha = (theme::elevation::surface_alpha_l4(t.dark) * 255.0).round() as u8;
                egui::Frame::new()
                    .fill(egui::Color32::from_rgba_unmultiplied(r, g, b, alpha))
                    .stroke(Stroke::new(theme::stroke::HAIRLINE, t.border))
                    .corner_radius(theme::radius::xl())
                    .shadow(theme::elevation::shadow_l4(t.dark))
                    .inner_margin(egui::Margin::symmetric(
                        theme::space::S2 as i8,
                        theme::space::S1 as i8,
                    ))
                    .show(ui, |ui| {
                        ui.set_min_height(theme::space::FLOATING_TOOLBAR_HEIGHT - theme::space::S4);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = theme::space::S1;
                            for (tool, icon, label, key) in FLOAT_TOOLS {
                                let resp = tool_button(ui, *tool, *icon, label, key, current);
                                if resp.clicked() {
                                    fired = Some(command_of(*tool));
                                }
                            }
                            ui.separator();
                            // 缩放 chip:显示当前 %,点击 = 适合窗口(与
                            // 状态栏缩放项同路)
                            let chip = ui
                                .add(egui::Button::new(
                                    egui::RichText::new(format!("{zoom_pct}%")).font(
                                        vb_ui::fonts::font(12.0, vb_ui::fonts::Weight::Medium),
                                    ),
                                ))
                                .on_hover_text(vb_session::i18n::t("ui-app-toolbar-016"));
                            if chip.clicked() {
                                fit_clicked = true;
                            }
                        });
                    });
            });
        if let Some(id) = fired {
            self.run_command(id, false, false);
        }
        if fit_clicked {
            self.run_command("view.fit", false, false);
        }
    }

    /// 切换停靠位(命令与拖动共用):记住 → 持久化 → 状态提示。
    pub(crate) fn set_toolbar_dock(&mut self, side: DockSide) {
        self.toolbar_dock = side;
        self.toolbar_columns = super::dock_layout::clamp_columns(side, self.toolbar_columns);
        self.say(vb_session::i18n::t_args(
            "ui-app-toolbar-017",
            &[(
                "a1",
                vb_session::i18n::FluentValue::from((side.label()).to_string()),
            )],
        ));
        self.save_workspace();
    }

    /// 切换单/双列(仅左/右停靠有效)。
    pub(crate) fn set_toolbar_columns(&mut self, columns: u8) {
        if !self.toolbar_dock.is_vertical() {
            self.toast_warn(vb_session::i18n::t("ui-app-toolbar-018"));
            return;
        }
        self.toolbar_columns = columns.clamp(1, 2);
        self.say(vb_session::i18n::t_args(
            "ui-app-toolbar-019",
            &[(
                "a1",
                vb_session::i18n::FluentValue::from((self.toolbar_columns).to_string()),
            )],
        ));
        self.save_workspace();
    }

    /// 组装当前工作区配置(持久化用)。
    pub(crate) fn workspace_config(&self) -> super::dock_layout::WorkspaceConfig {
        let t = &self.text_default;
        super::dock_layout::WorkspaceConfig {
            schema_version: super::dock_layout::SCHEMA_VERSION,
            toolbar_dock: self.toolbar_dock,
            toolbar_columns: self.toolbar_columns,
            dock_collapsed: self.dock_collapsed,
            panel_order: self.panel_order.to_vec(),
            panel_tab: self.panel_tab,
            panels_hidden: self.panels_hidden,
            // 04-2:次级面板摆放记忆(浮窗态/位置/组顺序/当前组)
            sec_floating: self.sec.floating.to_vec(),
            sec_pos: self.sec.pos.to_vec(),
            sec_group_order: self.sec.group_order.to_vec(),
            sec_active_group: self.sec.active_group,
            // 04-4:开发者统计 / 提示条开关
            dev_stats: self.dev_stats,
            hints: self.hints,
            // 04-3 / 04-6:UI 缩放因子 / 显示未支持工具
            ui_scale: self.ui_scale,
            show_all_tools: self.show_all_tools,
            // ── 第四轮 U-2/H-1:坞宽记忆 / 次级坞折叠 / 动效开关 ──
            dock_width: self.dock_width,
            sec_dock_width: self.sec_dock_width,
            sec_dock_collapsed: self.sec_dock_collapsed,
            motion_enabled: self.motion_enabled,
            density_compact: self.density_compact,
            // 07-A:自动保存间隔档位
            autosave_interval_secs: self.autosave_interval_secs,
            // 05-2(X-5):铅笔保真度容差
            pencil_fidelity: self.pencil_fidelity,
            // ── 05-4-A2:首选项九分类(收编既有散落项 + 新增)──
            theme_dark: self.theme_dark,
            rulers_default: self.rulers_on,
            grid_default: self.grid_on,
            guides_default: self.guides_visible,
            smart_guides_default: self.smart_guides_on,
            grid_size: self.grid_size,
            text_default_size: t.font_size.unwrap_or(24.0),
            text_default_color: t.color.clone().unwrap_or_else(|| "#1a1a1a".into()),
            text_default_family: t.font_family.clone().unwrap_or_default(),
            artboard_preset: self.artboard_preset,
            autosave_keep: self.autosave_keep,
            // 05-7:界面语言(进程级单例读当前值)
            ui_lang: crate::i18n::lang().code().to_string(),
            workspace_presets: self.workspace_saved.workspace_presets.clone(),
        }
    }

    /// 本窗口是否为根视口(独立模式 / 根项目窗;UI-12:根视口不产生
    /// 窗口层文件,布局随全局 workspace.json,与旧口径一致)。
    pub(crate) fn is_root_window(&self) -> bool {
        self.viewport_id == egui::ViewportId::ROOT
    }

    /// 当前窗口布局层快照(UI-12:布局字段 = 工具栏停靠/面板坞/次级坞/坞宽)。
    fn window_layer(&self) -> super::dock_layout::LayoutSnapshot {
        super::dock_layout::LayoutSnapshot::from_config(&self.workspace_config())
    }

    /// UI-12:外壳开窗后调用 —— 聚合读取(全局偏好 + 本窗口布局层)并
    /// 应用。层文件缺失 = 首次开窗(保持全局/默认);**损坏 = 告警 +
    /// 整层回退**(RB-08)。根视口不加载层(无层文件语义)。
    pub(crate) fn attach_window_layer(&mut self) {
        if self.is_root_window() {
            return;
        }
        let id = self.viewport_id.0.value();
        let (layer, warn) = super::dock_layout::load_window_layer(id);
        if let Some(w) = warn {
            log::warn!("{w}");
            self.toast_warn(w);
        }
        if let Some(l) = layer {
            // 布局字段逐项应用;次级坞经 from_config 复用既有越界兜底
            // (组序去重/下标夹回),不写第二份解析
            let mut synth = self.workspace_saved.clone();
            synth.toolbar_dock = l.toolbar_dock;
            synth.toolbar_columns = l.toolbar_columns;
            synth.dock_collapsed = l.dock_collapsed;
            synth.panel_order = l.panel_order;
            synth.panel_tab = l.panel_tab;
            synth.panels_hidden = l.panels_hidden;
            synth.sec_floating = l.sec_floating;
            synth.sec_pos = l.sec_pos;
            synth.sec_group_order = l.sec_group_order;
            synth.sec_active_group = l.sec_active_group;
            synth.dock_width = l.dock_width;
            synth.sec_dock_width = l.sec_dock_width;
            synth.sec_dock_collapsed = l.sec_dock_collapsed;
            self.toolbar_dock = synth.toolbar_dock;
            self.toolbar_columns = synth.toolbar_columns;
            self.dock_collapsed = synth.dock_collapsed;
            if synth.panel_order.len() == super::panels::TAB_COUNT {
                let mut order = [0usize; super::panels::TAB_COUNT];
                order.copy_from_slice(&synth.panel_order);
                self.panel_order = order;
            }
            self.panel_tab = synth.panel_tab.min(super::panels::TAB_COUNT - 1);
            self.panels_hidden = synth.panels_hidden;
            self.sec = super::panel_dock::SecDockState::from_config(&synth);
            self.dock_width = synth.dock_width;
            self.sec_dock_width = synth.sec_dock_width;
            self.sec_dock_collapsed = synth.sec_dock_collapsed;
        }
        // 基线对齐当前态(无论层是否存在,避免首帧误报脏)
        self.window_layer_saved = self.window_layer();
    }

    /// 写回 `workspace.json`(失败 → toast,不静默)。成功则刷新脏检查快照。
    ///
    /// 多窗口写策略(UI-12 改版):**窗口布局字段**写每窗口独立层文件
    /// `workspace.win-<id>.json`(互不覆盖);**进程级偏好**仍写全局
    /// workspace.json(根视口连布局一起写,行为与旧口径一致;子窗口写
    /// 全局时布局字段回填基线值,防止子窗布局覆盖根窗/独立模式的全局布局)。
    /// 预设合流保持:写前从磁盘回读 `workspace_presets`,他窗刚改过则以
    /// 磁盘为准合入。
    pub(crate) fn save_workspace(&mut self) {
        let mut cfg = self.workspace_config();
        if let Some(p) = super::dock_layout::config_path() {
            let (disk, _) = super::dock_layout::load_from(&p);
            if disk.workspace_presets != self.workspace_saved.workspace_presets {
                cfg.workspace_presets = disk.workspace_presets.clone();
                self.workspace_saved.workspace_presets = disk.workspace_presets;
            }
        }
        // 子窗口:全局文件只承载偏好,布局字段回填基线(不覆盖他窗)
        let layer = self.window_layer();
        if !self.is_root_window() {
            let saved = self.workspace_saved.clone();
            cfg.toolbar_dock = saved.toolbar_dock;
            cfg.toolbar_columns = saved.toolbar_columns;
            cfg.dock_collapsed = saved.dock_collapsed;
            cfg.panel_order = saved.panel_order.clone();
            cfg.panel_tab = saved.panel_tab;
            cfg.panels_hidden = saved.panels_hidden;
            cfg.sec_floating = saved.sec_floating.clone();
            cfg.sec_pos = saved.sec_pos.clone();
            cfg.sec_group_order = saved.sec_group_order.clone();
            cfg.sec_active_group = saved.sec_active_group;
            cfg.dock_width = saved.dock_width;
            cfg.sec_dock_width = saved.sec_dock_width;
            cfg.sec_dock_collapsed = saved.sec_dock_collapsed;
        }
        match super::dock_layout::save(&cfg) {
            Ok(()) => self.workspace_saved = cfg,
            Err(e) => self.toast_warn(vb_session::i18n::t_args(
                "ui-app-toolbar-020",
                &[("e", vb_session::i18n::FluentValue::from((e).to_string()))],
            )),
        }
        // 窗口层(UI-12):子窗口布局独立落盘;根视口同步基线不写文件
        if self.is_root_window() {
            self.window_layer_saved = layer;
        } else if layer != self.window_layer_saved {
            let id = self.viewport_id.0.value();
            match super::dock_layout::save_window_layer(id, &layer) {
                Ok(()) => self.window_layer_saved = layer,
                Err(e) => self.toast_warn(vb_session::i18n::t_args(
                    "ui-app-toolbar-021",
                    &[("e", vb_session::i18n::FluentValue::from((e).to_string()))],
                )),
            }
        }
    }

    /// 只落预设列表(布局字段取当前内存值;调用方已改好
    /// `workspace_saved.workspace_presets`)。
    fn save_workspace_presets_only(&mut self) {
        let mut cfg = self.workspace_config();
        cfg.workspace_presets = self.workspace_saved.workspace_presets.clone();
        match super::dock_layout::save(&cfg) {
            Ok(()) => {
                self.workspace_saved = cfg;
            }
            Err(e) => {
                self.toast_warn(vb_session::i18n::t_args(
                    "ui-app-toolbar-022",
                    &[("e", vb_session::i18n::FluentValue::from((e).to_string()))],
                ));
                // 回滚内存列表,保持与磁盘一致(磁盘读不到就清空本次改动)
                match super::dock_layout::config_path().map(|p| super::dock_layout::load_from(&p)) {
                    Some((disk, _)) => {
                        self.workspace_saved.workspace_presets = disk.workspace_presets
                    }
                    None => self.toast_warn(vb_session::i18n::t("ui-app-toolbar-023")),
                }
            }
        }
    }

    /// 当前布局是否与**已落盘快照**不同(逐字段比较,避免每帧构造 Vec)。
    ///
    /// UI-12:根视口比较全局快照(布局+偏好);子窗口比较「窗口布局层
    /// 快照」+「全局偏好字段」(布局字段不再参与全局比较)。
    pub(crate) fn workspace_dirty(&self) -> bool {
        if !self.is_root_window() {
            let layer = self.window_layer();
            if layer != self.window_layer_saved {
                return true;
            }
        }
        let a = &self.workspace_saved;
        a.toolbar_dock != self.toolbar_dock
            || a.toolbar_columns != self.toolbar_columns
            || a.dock_collapsed != self.dock_collapsed
            || a.panel_tab != self.panel_tab
            || a.panels_hidden != self.panels_hidden
            || a.panel_order.as_slice() != self.panel_order.as_slice()
            // 04-2/04-4 新增记忆字段
            || a.sec_floating.as_slice() != self.sec.floating.as_slice()
            || a.sec_pos.as_slice() != self.sec.pos.as_slice()
            || a.sec_group_order.as_slice() != self.sec.group_order.as_slice()
            || a.sec_active_group != self.sec.active_group
            || a.dev_stats != self.dev_stats
            || a.hints != self.hints
            // 04-3 / 04-6:UI 缩放 / 未支持工具开关
            || (a.ui_scale - self.ui_scale).abs() > f32::EPSILON
            || a.show_all_tools != self.show_all_tools
            // ── 第四轮 U-2/H-1:坞宽 / 次级坞折叠 / 动效开关 ──
            || (a.dock_width - self.dock_width).abs() > 0.5
            || (a.sec_dock_width - self.sec_dock_width).abs() > 0.5
            || a.sec_dock_collapsed != self.sec_dock_collapsed
            || a.motion_enabled != self.motion_enabled
            || a.density_compact != self.density_compact
            // 07-A:自动保存间隔
            || a.autosave_interval_secs != self.autosave_interval_secs
            // 05-2(X-5):铅笔保真度
            || (a.pencil_fidelity - self.pencil_fidelity).abs() > f64::EPSILON
            // ── 05-4-A2:首选项九分类 ──
            || a.theme_dark != self.theme_dark
            || a.rulers_default != self.rulers_on
            || a.grid_default != self.grid_on
            || a.guides_default != self.guides_visible
            || a.smart_guides_default != self.smart_guides_on
            || (a.grid_size - self.grid_size).abs() > f64::EPSILON
            || a.text_default_size != self.text_default.font_size.unwrap_or(24.0)
            || a.text_default_color != self.text_default.color.clone().unwrap_or_default()
            || a.text_default_family != self.text_default.font_family.clone().unwrap_or_default()
            || a.artboard_preset != self.artboard_preset
            || a.autosave_keep != self.autosave_keep
            // 05-7:界面语言
            || a.ui_lang != crate::i18n::lang().code()
    }

    /// 工作区预设(内存视图;保存于 workspace.json `workspace_presets`)。
    pub(crate) fn workspace_presets(&self) -> &[super::dock_layout::WorkspacePreset] {
        &self.workspace_saved.workspace_presets
    }

    /// 预设列表的操作基线:磁盘上的**最新**列表(他窗可能刚保存/删除;
    /// 最后写入胜策略下的防覆盖合流),磁盘不可得 → 内存快照。
    fn presets_base_list(&self) -> Vec<super::dock_layout::WorkspacePreset> {
        match super::dock_layout::config_path().map(|p| super::dock_layout::load_from(&p)) {
            Some((disk, _)) => disk.workspace_presets,
            None => self.workspace_saved.workspace_presets.clone(),
        }
    }

    /// 保存当前布局为命名工作区预设(X-7):重名覆盖,立即落盘。
    pub(crate) fn workspace_preset_save(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            self.toast_warn(vb_session::i18n::t("ui-app-toolbar-024"));
            return;
        }
        let cur = self.workspace_config();
        let preset = super::dock_layout::WorkspacePreset {
            name: name.to_string(),
            layout: super::dock_layout::LayoutSnapshot {
                toolbar_dock: cur.toolbar_dock,
                toolbar_columns: cur.toolbar_columns,
                dock_collapsed: cur.dock_collapsed,
                panel_order: cur.panel_order.clone(),
                panel_tab: cur.panel_tab,
                panels_hidden: cur.panels_hidden,
                sec_floating: cur.sec_floating.clone(),
                sec_pos: cur.sec_pos.clone(),
                sec_group_order: cur.sec_group_order.clone(),
                sec_active_group: cur.sec_active_group,
                dock_width: cur.dock_width,
                sec_dock_width: cur.sec_dock_width,
                sec_dock_collapsed: cur.sec_dock_collapsed,
            },
        };
        let mut list = self.presets_base_list();
        match list.iter().position(|p| p.name == name) {
            Some(i) => list[i] = preset,
            None => list.push(preset),
        }
        self.workspace_saved.workspace_presets = list;
        self.save_workspace_presets_only();
        self.say(vb_session::i18n::t_args(
            "ui-app-toolbar-025",
            &[(
                "name",
                vb_session::i18n::FluentValue::from((name).to_string()),
            )],
        ));
    }

    /// 删除命名工作区预设(立即落盘)。
    pub(crate) fn workspace_preset_delete(&mut self, name: &str) {
        let mut list = self.presets_base_list();
        let before = list.len();
        list.retain(|p| p.name != name);
        if list.len() == before {
            self.say(vb_session::i18n::t_args(
                "ui-app-toolbar-026",
                &[(
                    "name",
                    vb_session::i18n::FluentValue::from((name).to_string()),
                )],
            ));
            return;
        }
        self.workspace_saved.workspace_presets = list;
        self.save_workspace_presets_only();
        self.say(vb_session::i18n::t_args(
            "ui-app-toolbar-027",
            &[(
                "name",
                vb_session::i18n::FluentValue::from((name).to_string()),
            )],
        ));
    }

    /// 应用命名工作区预设(布局字段整组换血 + 立即落盘)。
    pub(crate) fn workspace_preset_apply(&mut self, name: &str) {
        let Some(p) = self
            .workspace_saved
            .workspace_presets
            .iter()
            .find(|p| p.name == name)
            .cloned()
        else {
            self.toast_warn(vb_session::i18n::t_args(
                "ui-app-toolbar-028",
                &[(
                    "name",
                    vb_session::i18n::FluentValue::from((name).to_string()),
                )],
            ));
            return;
        };
        let l = p.layout;
        self.toolbar_dock = l.toolbar_dock;
        self.toolbar_columns = super::dock_layout::clamp_columns(l.toolbar_dock, l.toolbar_columns);
        self.dock_collapsed = l.dock_collapsed;
        self.panel_order = {
            let mut a = [0usize, 1, 2, 3];
            for (i, v) in l.panel_order.iter().take(4).enumerate() {
                a[i] = *v;
            }
            // 越界值归一(与 LayoutSnapshot::normalized 同口径兜底)
            for v in &mut a {
                if *v >= super::panels::TAB_COUNT {
                    *v = 0;
                }
            }
            a
        };
        self.panel_tab = l.panel_tab.min(super::panels::TAB_COUNT - 1);
        self.panels_hidden = l.panels_hidden;
        // SecDockState 用定长数组(Vec 快照 → 数组截断补齐)
        self.sec.floating = {
            // 05-10:面板扩到 13(短数组尾部补默认,与 normalize 同口径)
            let mut a = [false; 13];
            for (i, x) in l.sec_floating.iter().take(13).enumerate() {
                a[i] = *x;
            }
            a
        };
        self.sec.pos = {
            let mut a = [[0.0f32, 0.0]; 13];
            for (i, x) in l.sec_pos.iter().take(13).enumerate() {
                a[i] = *x;
            }
            a
        };
        self.sec.group_order = {
            let mut a = [0usize, 1, 2, 3, 4, 5, 6];
            for (i, x) in l.sec_group_order.iter().take(7).enumerate() {
                a[i] = *x;
            }
            // 越界值归一
            for v in &mut a {
                if *v >= 7 {
                    *v = 0;
                }
            }
            a
        };
        self.sec.active_group = l.sec_active_group.min(6);
        self.say(vb_session::i18n::t_args(
            "ui-app-toolbar-029",
            &[(
                "name",
                vb_session::i18n::FluentValue::from((name).to_string()),
            )],
        ));
        self.save_workspace();
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
    // §8.7 工具箱重制:图标钮 = 32 方钮(S9 刻度,ToolButton 24/32 两档
    // 的大档)+ 浮动档 20px 图标(§8.5 图标三档上限);「图标+文字」堆叠
    // 退场 —— 名称与键位全在 tooltip(零成本学习模式不变),单列 44 里
    // 呼吸感回来了。
    ToolButton::new(icon, label)
        .shortcut(key)
        .size(theme::space::S9)
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
        // ── 阶段 5(05-2)──
        Tool::Rotate => "tool.rotate",
        Tool::Mirror => "tool.mirror",
        Tool::Scale => "tool.scale",
        Tool::FreeTransform => "tool.free_transform",
        Tool::Pencil => "tool.pencil",
        Tool::Curvature => "tool.curvature",
        Tool::Slice => "tool.slice",
        Tool::Measure => "tool.measure",
    }
}

// ─────────────────────── 04-6 分组几何门禁(单测) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 门 1:族表恰好划分工具箱 —— 每个工具属于且仅属于一个组,组并集
    /// 覆盖全部工具(分组清晰的结构前提)。
    #[test]
    fn groups_partition_the_toolbox_exactly_once() {
        let mut seen = Vec::new();
        for g in 0..GROUP_ENDS.len() {
            for (t, ..) in &TOOLS[group_range(g)] {
                let name = format!("{t:?}");
                assert!(!seen.contains(&name), "{name} 被两组同时收录");
                seen.push(name);
                assert_eq!(group_of(*t), g, "{t:?} 的组归属与组表不一致");
            }
        }
        assert_eq!(seen.len(), TOOLS.len(), "组并集必须覆盖全部工具");
    }

    /// 门 2:组序 = 04-6-1 的验收口径 + 05-2 扩充 —— 选择族 |
    /// 变换族(X-4)| 形状族 | 路径族(钢笔·铅笔·曲率·剪刀)|
    /// 文字·画板·吸管·渐变·切片 | 视图族(含度量)。
    #[test]
    fn group_order_matches_spec() {
        let expect: [&[Tool]; 6] = [
            &[Tool::Select, Tool::DirectSelect, Tool::GroupSelect],
            &[Tool::Rotate, Tool::Mirror, Tool::Scale, Tool::FreeTransform],
            &[Tool::Rect, Tool::Ellipse, Tool::Line],
            &[Tool::Pen, Tool::Pencil, Tool::Curvature, Tool::Scissors],
            &[
                Tool::Text,
                Tool::Artboard,
                Tool::Eyedropper,
                Tool::Gradient,
                Tool::Slice,
            ],
            &[Tool::Zoom, Tool::Hand, Tool::Measure],
        ];
        for (g, want) in expect.iter().enumerate() {
            let got: Vec<Tool> = TOOLS[group_range(g)].iter().map(|(t, ..)| *t).collect();
            assert_eq!(got.as_slice(), *want, "第 {g} 组的成员/顺序漂移");
        }
    }

    /// 门 3:组间分隔线数量 = 组数 − 1(渲染据此插线;几何单测钉住常数)。
    #[test]
    fn separator_count_is_groups_minus_one() {
        let groups = GROUP_ENDS.len();
        let expected_seps = groups - 1;
        // 每 4 字节一组边界,只验证推导:22 个工具、6 组 → 5 条分隔线
        assert_eq!(TOOLBOX.len(), 22);
        assert_eq!(expected_seps, 5);
        // 组区间无缝衔接(分隔线插在边界上,不丢不重)
        for g in 1..groups {
            assert_eq!(group_range(g - 1).end, group_range(g).start);
        }
        assert_eq!(group_range(groups - 1).end, TOOLS.len());
    }

    /// 门 4:顶/底停靠宽度预算 —— 最小窗口(1024pt)下单行放得下;
    /// 需求行数随宽度收窄单调不减(换行兜底),且上限 3 行。
    #[test]
    fn horizontal_toolbar_fits_min_window_and_never_overflows() {
        let row = theme::space::ROW_HEIGHT;
        assert!(
            toolbar_row_width_with(row) <= 1024.0,
            "单行需求宽 {} 超过最小窗口 1024",
            toolbar_row_width_with(row)
        );
        assert_eq!(rows_for_width(1024.0, row), 1);
        assert!(rows_for_width(400.0, row) >= 2, "窄窗口必须换行(不许溢出)");
        let mut prev = 0usize;
        for w in [2000.0f32, 1200.0, 800.0, 400.0, 200.0] {
            let rows = rows_for_width(w, row);
            assert!(rows >= prev, "行数必须随宽度收窄单调不减");
            assert!((1..=3).contains(&rows));
            prev = rows;
        }
    }

    /// 门 5:未支持工具占位 —— 有图标、有名称、有「计划于 vX」提示,
    /// 且不与任何已实现工具重名(渲染面仅在 show_all_tools 开启时出现)。
    #[test]
    fn planned_tools_declare_version_and_stay_out_by_default() {
        for (_, _, tip) in PLANNED_TOOLS {
            assert!(tip.contains("计划于 v"), "未支持工具必须写明计划版本:{tip}");
        }
        for (_, _, label, _) in TOOLBOX {
            for (_, pname, _) in PLANNED_TOOLS {
                assert_ne!(label, pname, "已实现工具与未支持占位重名:{label}");
            }
        }
    }
}
