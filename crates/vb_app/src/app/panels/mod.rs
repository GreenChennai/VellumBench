//! 右侧面板坞(S1-b 02-1 重构;04-4 状态栏三层分离):
//! - **可折叠**:展开 = 可拖宽 240–420(默认 280)的完整坞;
//!   折叠 = 40px 图标条(每 Tab 一个图标,点击展开并切到该 Tab;
//!   窗口 <1200 强制折叠,此时点击只切 Tab,见 `vb_ui::dock`);
//! - **Tab 4 页**:属性 / 图层 / 画板 / 令牌;顺序右键可换(内存,
//!   持久化到 workspace.json 为阶段 7 项);
//! - 底部状态栏(**只留状态**)+ 提示条(可关)+ 开发者统计(默认隐藏,
//!   04-4);次级面板坞见 `super::panel_dock`。
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
pub(crate) mod proofread;
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

    /// 展开态:完整面板坞(默认 280;可拖 240–420,**宽度记忆入
    /// workspace.json** —— U-2,拖动结束经 `workspace_dirty` 写通)。
    fn dock_expanded(&mut self, ui: &mut egui::Ui) {
        let r = egui::Panel::right("props")
            .default_size(vb_ui::dock::clamp_width(self.dock_width))
            .size_range(vb_ui::dock::DOCK_MIN..=vb_ui::dock::DOCK_MAX)
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
                // 04-4:右坞底部的常驻 FPS/帧时间/节点行已移除 —— 性能数字
                // 只在「开发者统计」浮层可见(默认隐藏,`视图 → 开发者统计`)
            });
        // U-2:egui 会话内维护的实际宽度读回记忆位;差异经 workspace_dirty
        // 在存活帧落盘(与次级坞、工具栏停靠同一套写通路径)。
        let w = r.response.rect.width();
        if (w - self.dock_width).abs() > 0.5 {
            self.dock_width = dock::clamp_width(w);
        }
    }

    /// 底部状态栏(04-4 三层分离:**只留状态**)。
    ///
    /// 画板切换 / 缩放 / 坐标 / 选中数 / rev / 诚实标注(降级渲染、
    /// 近似渲染);教学提示 → 提示条([`Self::hint_bar`]),调试数据 →
    /// 开发者统计([`Self::show_dev_stats`])。最窄窗口(1024)不换行:
    /// 全部条目为等宽小标签,超长坐标截断由 egui 单行裁剪兜底。
    pub(crate) fn status_bar(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
                // U-9:缩放百分比是可点项(点击 = 适合窗口)—— hover 手型
                // 光标 + 下划线 + tooltip,与状态栏其余可点项同一套手势。
                let zoom_text = format!("{}%", (self.camera.zoom * 100.0) as i64);
                let zoom_resp = ui
                    .add(egui::Button::new(
                        egui::RichText::new(&zoom_text).size(12.0),
                    ))
                    .on_hover_text("适合窗口(Ctrl+0):全部画板可见;滚轮 / Ctrl+滚轮缩放");
                if zoom_resp.hovered() {
                    ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                }
                paint_hover_underline(ui, &zoom_resp);
                if zoom_resp.clicked() {
                    self.run_command("view.fit", false, false);
                }
                ui.separator();
                // ── 05-5:断点切换器(响应式预览宽度)──
                // 文档存在可用断点(meta/媒体规则/冻结块查询)时出现;
                // 切到断点 → 画布视宽即断点宽,属性面板进入覆盖编辑。
                {
                    let bps = crate::app::breakpoints::available(&self.doc);
                    if !bps.is_empty() {
                        let cur = self.active_breakpoint;
                        let t = crate::i18n::t;
                        egui::ComboBox::from_id_salt("bp_switch")
                            .selected_text(match cur {
                                Some(w) => format!("{} {}px", t("bp.switcher"), w),
                                None => format!("{}: {}", t("bp.switcher"), t("bp.default")),
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(cur.is_none(), t("bp.default").to_string())
                                    .clicked()
                                {
                                    self.active_breakpoint = None;
                                }
                                for w in bps {
                                    if ui
                                        .selectable_label(cur == Some(w), format!("{w}px"))
                                        .clicked()
                                    {
                                        self.active_breakpoint = Some(w);
                                    }
                                }
                            })
                            .response
                            .on_hover_text(t("bp.status-hint"));
                        ui.separator();
                    }
                }
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
                // rev 保留在状态栏(状态语义);渲染后端属调试数据 → 开发者统计
                ui.label(format!("rev {}", self.doc.rev));
                // 07-A:自动保存印记(轻量常驻;悬停说明落盘位置)
                if let Some(stamp) = self.autosave_stamp_text() {
                    ui.separator();
                    ui.label(stamp).on_hover_text(
                        "自动保存快照写入项目 .vb-autosave/(滚动保留 3 份;不覆盖 index.html)",
                    );
                }
                // 07-R:外部改动印记(Agent/其他进程改盘 → 热重载;点击看详情)
                if let Some(ext) = &self.external_change {
                    ui.separator();
                    let (text, tip) = if ext.adopted {
                        ("外部已改动(已重载)", "点击查看最近外部改动的时间与触发文件")
                    } else {
                        (
                            "外部已改动(未采用)",
                            "本地有未保存编辑,未自动采用 —— 点击查看触发文件",
                        )
                    };
                    let color = if ext.adopted {
                        theme::tokens(ui.ctx()).warn
                    } else {
                        // 07-I:未采用是告警级,走主题 danger 令牌(两主题可读)
                        theme::tokens(ui.ctx()).danger
                    };
                    let ext_resp = ui.colored_label(color, text);
                    // U-9:可点项 hover 手势 —— 手型光标 + 下划线,tooltip 提示可点
                    if ext_resp.hovered() {
                        ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                    }
                    paint_hover_underline(ui, &ext_resp);
                    if ext_resp.on_hover_text(tip).clicked() {
                        // 09-N 打通(05-4-A2):未采用 = 磁盘与内存有分叉 →
                        // 点印记直接打开三方对比对话框;已重载仍走信息窗。
                        if ext.adopted {
                            self.external_info_open = !self.external_info_open;
                        } else {
                            self.conflict_open = true;
                        }
                    }
                }
                // 03-5-3 诚实标注:降级渲染必须有可见标记(不静默)。
                // GPU/vello 不可用 = 画布内容无法渲染的降级态;
                // 提示级走主题 warn 令牌(07-I 口径,两主题可读)。
                if self.gpu.is_none() {
                    ui.separator();
                    ui.colored_label(theme::tokens(ui.ctx()).warn, "⚠ 降级渲染")
                        .on_hover_text("GPU/渲染器不可用,画布内容未按完整管线渲染");
                }
                // X-2 口径:画布文字为 egui 近似(常驻低对比标注,与画布
                // 角落提示一致;真实字形见导出,校对见「视图 → 浏览器校对」)
                ui.separator();
                ui.label("近似渲染").on_hover_text(
                    "画布文字为近似渲染(ADR-0017);导出为真字形,可用「视图 → 浏览器校对」对拍",
                );
            });
        });
    }

    /// 提示条(04-4 三层分离的第②层):操作反馈与入门提示的独立一层。
    ///
    /// 内容 = `self.status`(初始为教学提示,随操作被替换)。默认开,
    /// `视图 → 提示` 可关(状态入 `workspace.json`);关闭后错误/告警
    /// 仍经 toast 可见(02-6-6)。
    pub(crate) fn hint_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("vb_hints")
            .exact_size(theme::space::STATUS_BAR_HEIGHT)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::tokens(ui.ctx()).bg_panel))
            .show(ui, |ui| {
                // 长提示截断显示,完整内容悬停可见(不换行、不挤状态栏)
                let text = self.status.clone();
                let t = theme::tokens(ui.ctx());
                let resp = ui
                    .add(
                        egui::Label::new(egui::RichText::new(&text).size(12.0).color(t.text_2))
                            .truncate(),
                    )
                    .on_hover_text(&text);
                let _ = resp;
            });
    }

    /// 「最近外部改动」信息窗(07-R):点状态栏印记开合。
    ///
    /// 展示触发时间(多久前 + Unix 秒)与触发文件清单(相对项目根;
    /// 最多记 8 个,超出以"…"收口)。`未采用` 态给处置指引(先保存或撤销)。
    pub(crate) fn show_external_change_window(&mut self, ui: &mut egui::Ui) {
        if !self.external_info_open {
            return;
        }
        let mut open = true;
        egui::Window::new("最近外部改动")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                let Some(ext) = self.external_change.clone() else {
                    ui.label("本会话还没有检测到外部改动。");
                    return;
                };
                let secs = ext.at.elapsed().as_secs();
                let ago = if secs < 60 {
                    format!("{secs} 秒前")
                } else {
                    format!("{} 分钟前", secs / 60)
                };
                ui.horizontal(|ui| {
                    if ext.adopted {
                        ui.colored_label(theme::tokens(ui.ctx()).warn, "已重载");
                    } else {
                        ui.colored_label(theme::tokens(ui.ctx()).danger, "未采用");
                    }
                    ui.label(format!("{ago}(Unix {} 秒)", ext.at_unix));
                });
                if !ext.adopted {
                    ui.weak("本地有未保存编辑:未自动采用。可对比磁盘版本后取舍。");
                    // 09-N 打通(05-4-A2):信息窗内的对比入口(与状态栏
                    // 印记点击同路,直接打开三方对比对话框)
                    if ui.button("对比并合并…").clicked() {
                        self.conflict_open = true;
                    }
                }
                ui.separator();
                ui.label("触发文件:");
                for f in &ext.files {
                    ui.monospace(f);
                }
                if ext.files.len() >= crate::app::EXTERNAL_FILES_MAX {
                    ui.weak("…(仅记录最近的触发文件)");
                }
            });
        self.external_info_open = open;
    }

    /// 开发者统计浮层(04-4-1 / W6 决策:调试数据默认隐藏)。
    ///
    /// FPS / 帧时间 / 节点数 / 视口 / 渲染后端与**显卡型号**只在这里出现;
    /// 开关 = `视图 → 开发者统计`,状态入 `workspace.json`(dev_stats)。
    /// FPS 采样亦只在开关打开时进行(见 `app.rs` `ui()`,给阶段 6 idle 节流留路)。
    pub(crate) fn show_dev_stats(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if !self.dev_stats {
            return;
        }
        let adapter = frame.wgpu_render_state().map(|rs| {
            let info = rs.adapter.get_info();
            (info.backend, info.name)
        });
        let fps = self.frame_stats();
        let mut open = true;
        egui::Window::new("开发者统计")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_pos(default_stats_anchor(ui))
            .show(ui.ctx(), |ui| {
                let vp = ui.ctx().viewport_rect();
                match (fps, adapter) {
                    (Some((fps, frame_ms)), Some((backend, name))) => {
                        ui.label(format!("FPS {fps:.0} · 帧时间 {frame_ms:.1} ms"));
                        ui.label(format!("节点 {}", self.doc.nodes.len()));
                        ui.label(format!("渲染 {backend:?} · {name}"));
                        ui.label(format!(
                            "视口 {:.0}×{:.0} · 缩放 {}%",
                            vp.width(),
                            vp.height(),
                            (self.camera.zoom * 100.0) as i64
                        ));
                    }
                    (Some((fps, frame_ms)), None) => {
                        ui.label(format!("FPS {fps:.0} · 帧时间 {frame_ms:.1} ms"));
                        ui.label(format!("节点 {}", self.doc.nodes.len()));
                        ui.label("渲染:无 GPU(降级)");
                    }
                    (None, Some((backend, name))) => {
                        ui.label(format!("FPS 采样中… · 渲染 {backend:?} · {name}"));
                    }
                    (None, None) => {
                        ui.label("FPS 采样中… · 渲染:无 GPU(降级)");
                    }
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(
                        "调试数据仅开发者可见;用户界面默认不显示任何性能/硬件信息(04-4)。",
                    )
                    .size(12.0),
                );
            });
        self.dev_stats = open;
    }

    /// FPS/帧时间统计(采样窗口均值;`None` = 样本不足,显示"采样中")。
    fn frame_stats(&self) -> Option<(f32, f32)> {
        if self.frame_times.is_empty() {
            return None;
        }
        let avg: f32 = self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32;
        if avg <= 0.0 {
            return None;
        }
        Some((1.0 / avg, avg * 1000.0))
    }
}

/// 开发者统计浮层的默认停靠角(视口右上、避开标题栏与控制条)。
fn default_stats_anchor(ui: &egui::Ui) -> egui::Pos2 {
    let vp = ui.ctx().viewport_rect();
    egui::pos2((vp.right() - 280.0).max(vp.left() + 8.0), vp.top() + 96.0)
}

/// U-9:可点文本的悬停下划线(状态栏「缩放 / 外部改动」等可点项的
/// 统一手势暗示;光标由调用方置 PointingHand)。
fn paint_hover_underline(ui: &egui::Ui, resp: &egui::Response) {
    if resp.hovered() {
        let rect = resp.rect;
        let y = rect.bottom() - 1.0;
        ui.painter().line_segment(
            [
                egui::pos2(rect.left() + 1.0, y),
                egui::pos2(rect.right() - 1.0, y),
            ],
            egui::Stroke::new(1.0, theme::tokens(ui.ctx()).text_2),
        );
    }
}
