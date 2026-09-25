//! 启动主页(阶段 2 / 副文档 02-3):最近项目 + 动作区 + 搜索 + 键盘导航。
//!
//! **职责单一**:主页是纯入口,不编辑文档(02 §3 决策);所有动作都有
//! 命令 ID(`home.*`,登记在 `shortcuts::IMPLEMENTED_IDS` 与 commands.yaml,
//! 验收要求"Agent 可复现"),经 [`ShellRequest`] 发给外壳执行。
//!
//! 视觉沿用 `vb_ui` 令牌(`Tokens::get`),不做第二套视觉(02 §6 风险)。

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use egui::{Align2, Color32, Key, ScrollArea};
use vb_ui::theme::Tokens;

use crate::capabilities::{CapStatus, CAPABILITIES};
use crate::new_project::{self, NewProjectDialog};
use crate::recent::{relative_time, RecentItem};
use crate::shell::ShellRequest;

/// 缩略图纹理缓存(失败记为占位,不逐帧重试读盘/解码)。
enum Thumb {
    Failed,
    Ready(egui::TextureHandle),
}

/// 主页状态(外壳持有;主题与最近项目数据由外壳传入/单点写)。
pub struct LauncherUi {
    /// 是否可见(home 启动模式恒可见;project 模式经「主页」打开)。
    pub open: bool,
    /// 搜索过滤(02-3-3:按名称/路径)。
    pub search: String,
    /// 过滤后列表的选中下标(键盘 ↑↓ / Enter)。
    pub selected: usize,
    /// 上一帧搜索框是否持有焦点(键盘派发的上下文判定)。
    search_focused: bool,
    /// 「聚焦搜索框」待办(home.search 命令的落地)。
    focus_search: bool,
    /// 能力台账窗口(02-3-6)。
    pub show_caps: bool,
    /// 「新建项目 / 从模板新建」对话框(02-4-1)。
    pub dialog: Option<NewProjectDialog>,
    /// 「移除记录」确认(02-3-4:Delete 带确认;02-2-4:不静默丢)。
    pub confirm_remove: Option<String>,
    /// 外壳广播的错误/提示行(超时自动消失)。
    pub message: Option<(String, std::time::Instant)>,
    /// H-7:正在打开的项目名(None = 空闲)。外壳把打开动作推迟一帧,
    /// 让「正在打开…」占位先画出来 —— wgpu 初始化(实测 ~9s)不再白屏。
    pub opening: Option<String>,
    /// 07-Q:artboard 来源徽标缓存(键 = 项目路径;每路径只读一次盘)。
    artboard_badges: std::collections::HashMap<String, bool>,
    thumbs: std::collections::HashMap<String, Thumb>,
}

impl LauncherUi {
    pub fn new(open: bool) -> Self {
        LauncherUi {
            open,
            search: String::new(),
            selected: 0,
            search_focused: false,
            focus_search: false,
            show_caps: false,
            dialog: None,
            confirm_remove: None,
            message: None,
            opening: None,
            artboard_badges: std::collections::HashMap::new(),
            thumbs: std::collections::HashMap::new(),
        }
    }

    /// 该项目是否带 artboard 来源徽标(07-Q;结果按路径缓存,不逐帧读盘)。
    fn artboard_badged(&mut self, item: &RecentItem) -> bool {
        *self
            .artboard_badges
            .entry(item.path.clone())
            .or_insert_with(|| is_artboard_project(Path::new(&item.path)))
    }

    /// 渲染主页(挂在根视口或主页子视口的 Ui 上)。
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        items: &[&RecentItem],
        session_len: usize,
        tx: &Sender<ShellRequest>,
    ) {
        let t = Tokens::get(ui.ctx().theme() == egui::Theme::Dark);
        let view = filtered(items, &self.search);
        self.handle_keys(ui.ctx(), &view, tx);

        // ── 顶部:标题 + 动作区 + 搜索(02-3-1 / 02-3-3)──
        egui::Panel::top("vb-home-header").show(ui, |ui| {
            ui.add_space(6.0);
            egui::MenuBar::new().ui(ui, |ui| {
                ui.label(vb_ui::components::strong("Vellum Bench"));
                ui.weak(format!("v{}", env!("CARGO_PKG_VERSION")));
                ui.separator();
                if ui.button("新建项目").clicked() {
                    self.run_home_command("home.new_project", tx, &view);
                }
                if ui.button("打开项目…").clicked() {
                    self.run_home_command("home.open_project", tx, &view);
                }
                if ui.button("从模板新建").clicked() {
                    self.run_home_command("home.new_from_template", tx, &view);
                }
                if session_len > 0 && ui.button(format!("恢复上次会话({session_len})")).clicked()
                {
                    self.run_home_command("home.restore_session", tx, &view);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let resp = ui.add_sized(
                        [240.0, vb_ui::theme::row_height(ui.ctx())],
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("搜索名称或路径…(Ctrl+F)"),
                    );
                    if self.focus_search {
                        resp.request_focus();
                        self.focus_search = false;
                    }
                    self.search_focused = resp.has_focus();
                    if resp.changed() {
                        self.selected = 0;
                    }
                });
            });
            ui.add_space(2.0);
        });

        // ── 底部:版本 + 许可 + 能力台账(02-3-6)──
        egui::Panel::bottom("vb-home-footer").show(ui, |ui| {
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.weak(format!("v{} · 许可 ACL-1.0", env!("CARGO_PKG_VERSION")));
                ui.separator();
                if ui.button("能力台账").clicked() {
                    self.run_home_command("home.capabilities", tx, &view);
                }
                ui.weak("主页是主窗口:关闭主页 = 退出 Vellum Bench");
                if let Some((msg, at)) = &self.message {
                    if at.elapsed() < std::time::Duration::from_secs(8) {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.colored_label(t.warn, msg);
                        });
                    }
                }
            });
            ui.add_space(4.0);
        });

        // ── 中央:最近项目列表 / 空态(02-3-2)──
        egui::CentralPanel::default().show(ui, |ui| {
            if items.is_empty() {
                self.empty_state(ui, tx, &view);
                return;
            }
            if view.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.weak(format!("没有匹配「{}」的项目", self.search));
                });
                return;
            }
            self.selected = self.selected.min(view.len() - 1);
            ScrollArea::vertical().show(ui, |ui| {
                for (i, item) in view.iter().enumerate() {
                    self.item_row(ui, i, item, tx, &t);
                }
            });
        });

        // ── 对话框与确认 ──
        self.show_dialog(ui.ctx(), tx);
        self.show_confirm_remove(ui.ctx(), tx);
        self.show_capabilities(ui.ctx());
        // H-7:正在打开占位(最后画,浮在最上)
        self.opening_overlay(ui.ctx());
    }

    // ---------- 键盘(02-3-4:Enter / Ctrl+N / Ctrl+O / Delete / ↑↓) ----------

    fn handle_keys(
        &mut self,
        ctx: &egui::Context,
        view: &[&RecentItem],
        tx: &Sender<ShellRequest>,
    ) {
        let editing = self.search_focused || self.dialog.is_some() || self.confirm_remove.is_some();
        // 逐事件扫描(方向键连按不丢;与 app.rs handle_shortcuts 同套路)
        let keys: Vec<(Key, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, modifiers.ctrl || modifiers.command)),
                    _ => None,
                })
                .collect()
        });
        for (key, ctrl) in keys {
            match key {
                Key::ArrowDown if !editing => {
                    self.run_home_command("home.select_next", tx, view);
                }
                Key::ArrowUp if !editing => {
                    self.run_home_command("home.select_prev", tx, view);
                }
                Key::Enter if self.dialog.is_none() => {
                    self.run_home_command("home.open_selected", tx, view);
                }
                Key::N if ctrl => self.run_home_command("home.new_project", tx, view),
                Key::O if ctrl => self.run_home_command("home.open_project", tx, view),
                Key::F if ctrl => self.run_home_command("home.search", tx, view),
                Key::Delete if !editing => {
                    self.run_home_command("home.remove_selected", tx, view);
                }
                _ => {}
            }
        }
    }

    // ---------- 命令派发(主页操作的命令 ID 单一入口;Agent 可复现) ----------

    fn run_home_command(&mut self, id: &str, tx: &Sender<ShellRequest>, view: &[&RecentItem]) {
        debug_assert!(
            crate::shortcuts::is_implemented(id),
            "主页命令 {id} 未在 shortcuts::IMPLEMENTED_IDS 中声明"
        );
        match id {
            "home.new_project" => self.dialog = Some(NewProjectDialog::new()),
            "home.new_from_template" => self.dialog = Some(NewProjectDialog::new_template()),
            "home.open_project" => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("打开项目目录(含 index.html)")
                    .pick_folder()
                {
                    let _ = tx.send(ShellRequest::OpenProject(dir));
                }
            }
            "home.open_selected" => {
                if let Some(item) = view.get(self.selected) {
                    let path = PathBuf::from(item.path.clone());
                    let _ = tx.send(ShellRequest::OpenProject(path));
                }
            }
            "home.remove_selected" => {
                if let Some(item) = view.get(self.selected) {
                    self.confirm_remove = Some(item.path.clone());
                }
            }
            "home.select_next" => {
                if !view.is_empty() {
                    self.selected = (self.selected + 1).min(view.len() - 1);
                }
            }
            "home.select_prev" => {
                self.selected = self.selected.saturating_sub(1);
            }
            "home.pin_selected" => {
                if let Some(item) = view.get(self.selected) {
                    let path = PathBuf::from(item.path.clone());
                    let _ = tx.send(ShellRequest::TogglePinRecent(path));
                }
            }
            "home.search" => self.focus_search = true,
            "home.restore_session" => {
                let _ = tx.send(ShellRequest::RestoreSession);
            }
            "home.capabilities" => self.show_caps = !self.show_caps,
            other => log::warn!("主页命令未实现:{other}"),
        }
    }

    // ---------- 列表行 ----------

    /// U-1:MRU 卡片(重制)。
    ///
    /// - 高度收紧:缩略图 16:9(96×54)与两行文字,卡片总高 ~70
    ///   (旧版 ~92,留白多、信息密度低);
    /// - **操作钮悬停/选中显现**(旧版五个常驻按钮挤占一整行);
    ///   鼠标路径之外,右键菜单提供全部操作(键盘全可达不变);
    /// - hover 提升:底色 + 描边同步增强(0→80ms 过渡);
    /// - 卡片是同一块命中区:单击选中、双击打开、右键菜单。
    fn item_row(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        item: &RecentItem,
        tx: &Sender<ShellRequest>,
        t: &Tokens,
    ) {
        let stale = item.is_stale();
        let selected = i == self.selected;
        // 07-Q:artboard 来源徽标(检测到画板标记类才显示,不逐帧读盘)
        let artboard = !stale && self.artboard_badged(item);

        // ── 卡片矩形(先分配,同帧即得 hover —— 底色/描边提升不迟一帧)──
        let card_h = 54.0 + 12.0; // 缩略图 16:9 + 上下 6px 内边距
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), card_h),
            egui::Sense::click(),
        );
        let hover_t = ui.ctx().animate_bool_with_time(
            ui.id().with(("vb-mru", &item.path)),
            resp.hovered() && !selected,
            vb_ui::theme::anim_time(ui.ctx(), vb_ui::theme::motion::HOVER),
        );
        let painter = ui.painter();
        let fill = if selected {
            t.accent_dim
        } else {
            blend(t.bg_panel, t.bg_hover, hover_t)
        };
        painter.rect_filled(rect, vb_ui::theme::radius::lg(), fill);
        // 描边:选中 > 悬停 > 静息(hover 提升走「边框+底色」双通道)
        let stroke = if selected {
            egui::Stroke::new(1.5, t.accent)
        } else {
            egui::Stroke::new(1.0, blend(t.border, t.text_3, hover_t))
        };
        painter.rect_stroke(
            rect,
            vb_ui::theme::radius::lg(),
            stroke,
            egui::StrokeKind::Inside,
        );

        // ── 卡片内容(缩略图 | 名称/路径 | 时间+操作)──
        // 右列预留宽度:操作显现时给足一排按钮,静息时只留相对时间 ——
        // 避免名称列吃满可用宽把右列挤出卡片外(实测首版踩过)。
        let actions_visible = resp.hovered() || selected;
        let action_reserve: f32 = if actions_visible { 350.0 } else { 72.0 };
        let mut card = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(ui.id().with(("vb-mru-body", &item.path)))
                .max_rect(rect.shrink2(egui::vec2(6.0, 6.0))),
        );
        card.horizontal(|ui| {
            self.thumb(ui, item, t);
            // 名称行 + 路径行(路径截断,悬停卡片 tooltip 显示全路径)
            ui.vertical(|ui| {
                ui.set_min_width((ui.available_width() - action_reserve).max(120.0));
                ui.horizontal(|ui| {
                    let name = if item.pinned {
                        format!("★ {}", item.name)
                    } else {
                        item.name.clone()
                    };
                    let name_color = if stale { t.text_3 } else { t.text };
                    ui.label(egui::RichText::new(name).strong().color(name_color));
                    if artboard {
                        ui.label(egui::RichText::new("artboard").small().color(t.accent))
                            .on_hover_text(
                                "来源:artboard 项目(index.html 带画板标记类,可直接打开编辑)",
                            );
                    }
                    if stale {
                        ui.colored_label(t.danger, "路径已失效");
                    }
                });
                ui.label(egui::RichText::new(&item.path).small().color(if stale {
                    t.text_3
                } else {
                    t.text_2
                }));
            });
            // 右列:静息 = 相对时间;悬停/选中 = 操作钮(悬停显现,U-1)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.weak(relative_time(item.last_opened, crate::recent::now_secs()));
                if !actions_visible {
                    return;
                }
                if ui.button("移除").clicked() {
                    self.confirm_remove = Some(item.path.clone());
                }
                let reveal = ui.add_enabled(
                    !stale,
                    egui::Button::new(vb_ui::icons::rich(vb_ui::icons::Name::KindImage, 14.0)),
                );
                let reveal = reveal.on_hover_text("在资源管理器中显示");
                if reveal.clicked() {
                    reveal_in_explorer(&item.path);
                }
                let copy = ui.add(egui::Button::new(vb_ui::icons::rich(
                    vb_ui::icons::Name::Copy,
                    14.0,
                )));
                let copy = copy.on_hover_text("复制路径");
                if copy.clicked() {
                    ui.ctx().copy_text(item.path.clone());
                }
                let pin_label = if item.pinned {
                    "取消固定"
                } else {
                    "固定"
                };
                if ui.button(pin_label).clicked() {
                    let _ = tx.send(ShellRequest::TogglePinRecent(PathBuf::from(
                        item.path.clone(),
                    )));
                }
                let open_btn = ui.add_enabled(!stale, egui::Button::new("打开"));
                if open_btn.clicked() {
                    let _ = tx.send(ShellRequest::OpenProject(PathBuf::from(item.path.clone())));
                }
            });
        });

        // 卡片级交互:单击选中 / 双击打开 / 右键菜单 / 全路径 tooltip
        if resp.clicked() {
            self.selected = i;
        }
        if resp.double_clicked() && !stale {
            let _ = tx.send(ShellRequest::OpenProject(PathBuf::from(item.path.clone())));
        }
        resp.context_menu(|ui| {
            if ui.add_enabled(!stale, egui::Button::new("打开")).clicked() {
                let _ = tx.send(ShellRequest::OpenProject(PathBuf::from(item.path.clone())));
                ui.close();
            }
            let pin_label = if item.pinned {
                "取消固定"
            } else {
                "固定"
            };
            if ui.button(pin_label).clicked() {
                let _ = tx.send(ShellRequest::TogglePinRecent(PathBuf::from(
                    item.path.clone(),
                )));
                ui.close();
            }
            if ui.button("复制路径").clicked() {
                ui.ctx().copy_text(item.path.clone());
                ui.close();
            }
            if ui
                .add_enabled(!stale, egui::Button::new("在资源管理器中显示"))
                .clicked()
            {
                reveal_in_explorer(&item.path);
                ui.close();
            }
            ui.separator();
            if ui.button("移除记录").clicked() {
                self.confirm_remove = Some(item.path.clone());
                ui.close();
            }
        });
        let _ = resp.on_hover_text(if stale {
            format!("{}(路径已失效)", item.path)
        } else {
            item.path.clone()
        });
        ui.add_space(4.0);
    }

    // ---------- 缩略图(02-2-5) ----------

    fn thumb(&mut self, ui: &mut egui::Ui, item: &RecentItem, t: &Tokens) {
        const TW: f32 = 96.0;
        const TH: f32 = 54.0;
        let key = item
            .thumb
            .as_deref()
            .filter(|p| Path::new(p).is_file())
            .unwrap_or("");
        let ready = match self.thumbs.entry(key.to_string()) {
            std::collections::hash_map::Entry::Occupied(o) => matches!(o.get(), Thumb::Ready(_)),
            std::collections::hash_map::Entry::Vacant(v) => {
                let loaded = if key.is_empty() {
                    None
                } else {
                    load_texture(ui.ctx(), key)
                };
                match loaded {
                    Some(tex) => {
                        v.insert(Thumb::Ready(tex));
                        true
                    }
                    None => {
                        v.insert(Thumb::Failed);
                        false
                    }
                }
            }
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(TW, TH), egui::Sense::hover());
        if ready {
            if let Some(Thumb::Ready(tex)) = self.thumbs.get(key) {
                ui.painter().image(
                    tex.id(),
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                return;
            }
        }
        // 占位:画板底色 + 项目首字符(生成失败/尚未生成,不阻塞)
        ui.painter().rect_filled(rect, 3.0, t.bg_canvas);
        ui.painter().rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.0, t.border),
            egui::StrokeKind::Inside,
        );
        let ch = item.name.chars().next().unwrap_or('?');
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            ch,
            egui::FontId::proportional(22.0),
            t.text_3,
        );
    }

    // ---------- 空态 / 对话框 / 确认 / 台账 ----------

    fn empty_state(&mut self, ui: &mut egui::Ui, tx: &Sender<ShellRequest>, view: &[&RecentItem]) {
        // U-1:空态插图式引导 —— 大图标 + 标题 + 一句引导 + 动作按钮
        ui.vertical_centered(|ui| {
            ui.add_space(56.0);
            ui.label(vb_ui::icons::rich(vb_ui::icons::Name::KindArtboard, 48.0).color(t_faint(ui)));
            ui.add_space(10.0);
            ui.label(egui::RichText::new("还没有项目").heading().strong());
            ui.add_space(6.0);
            ui.weak(
                "点「新建项目」开始,或打开一个含 index.html 的项目目录;也可以把目录拖进本窗口。",
            );
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button("新建项目").clicked() {
                    self.run_home_command("home.new_project", tx, view);
                }
                if ui.button("打开项目…").clicked() {
                    self.run_home_command("home.open_project", tx, view);
                }
                if ui.button("从模板新建").clicked() {
                    self.run_home_command("home.new_from_template", tx, view);
                }
            });
        });
    }

    /// H-7:「正在打开…」占位浮层 —— 外壳把重活(导入 + wgpu 初始化)
    /// 推迟一帧执行,这层提示先画出来,打开期间不再是白屏/无响应假死。
    fn opening_overlay(&mut self, ctx: &egui::Context) {
        let Some(name) = self.opening.clone() else {
            return;
        };
        let t = Tokens::get(ctx.theme() == egui::Theme::Dark);
        egui::Area::new(egui::Id::new("vb-home-opening"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, -20.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(t.bg_raised)
                    .stroke(egui::Stroke::new(1.0, t.accent))
                    .corner_radius(vb_ui::theme::radius::lg())
                    .inner_margin(egui::Margin::symmetric(16, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(16.0));
                            ui.label(egui::RichText::new(format!("正在打开「{name}」…")).strong());
                            ui.weak("(首次打开含渲染初始化,约需几秒)");
                        });
                    });
            });
        // 打开流程会阻塞下一帧,主动要帧保证占位立即呈现
        ctx.request_repaint();
    }

    fn show_dialog(&mut self, ctx: &egui::Context, tx: &Sender<ShellRequest>) {
        if self.dialog.is_none() {
            return;
        }
        let template_mode = self.dialog.as_ref().is_some_and(|d| d.template_mode);
        let mut open = true;
        let mut done = false;
        egui::Window::new(if template_mode {
            "从模板新建"
        } else {
            "新建项目"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let Some(dlg) = self.dialog.as_mut() else {
                return;
            };
            if let Some(action) = new_project::dialog_ui(ui, dlg) {
                match action {
                    new_project::DialogAction::Create(spec) => {
                        let _ = tx.send(ShellRequest::CreateProject(spec));
                    }
                    new_project::DialogAction::CreateTemplate {
                        template,
                        location,
                        name,
                    } => {
                        let _ = tx.send(ShellRequest::CreateFromTemplate {
                            template,
                            location,
                            name,
                        });
                    }
                    new_project::DialogAction::Cancel => {}
                }
                done = true;
            }
        });
        if done || !open {
            self.dialog = None;
        }
    }

    fn show_confirm_remove(&mut self, ctx: &egui::Context, tx: &Sender<ShellRequest>) {
        let Some(path) = self.confirm_remove.clone() else {
            return;
        };
        let mut done = false;
        egui::Window::new("移除最近项目记录?")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("将从列表移除:\n{}", path));
                ui.weak("只移除记录,不删除磁盘上的项目文件。");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("移除").clicked() {
                        let _ = tx.send(ShellRequest::RemoveRecent(PathBuf::from(path.clone())));
                        done = true;
                    }
                    if ui.button("取消").clicked() {
                        done = true;
                    }
                });
            });
        if done {
            self.confirm_remove = None;
        }
    }

    fn show_capabilities(&mut self, ctx: &egui::Context) {
        if !self.show_caps {
            return;
        }
        let mut open = self.show_caps;
        egui::Window::new("能力台账(做了什么 / 没做什么)")
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    for c in CAPABILITIES {
                        ui.horizontal(|ui| {
                            ui.weak(c.id);
                            ui.label(c.name);
                            let badge = match c.status {
                                CapStatus::Done => "已落地",
                                CapStatus::Partial(_) => "部分",
                                CapStatus::Planned(_) => "计划",
                                CapStatus::Dropped(_) => "不做",
                            };
                            ui.weak(badge);
                        });
                        if !c.status.note().is_empty() {
                            ui.weak(c.status.note());
                        }
                        ui.separator();
                    }
                });
            });
        self.show_caps = open;
    }
}

/// 搜索过滤(02-3-3):名称或路径包含(大小写不敏感);空串 = 全部。
pub fn filtered<'a>(items: &[&'a RecentItem], q: &str) -> Vec<&'a RecentItem> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .copied()
        .filter(|it| it.name.to_lowercase().contains(&q) || it.path.to_lowercase().contains(&q))
        .collect()
}

/// artboard 项目识别(07-Q):`index.html` 含画板标记类即认定来源为
/// artboard。标记类与 `vb_doc::import` 的 ARTBOARD_CLASSES 同口径
/// (`vb-artboard` / `vs-artboard` / `vsm-artboard`);**只读识别**,
/// 不改 artboard 侧的任何文件。
pub fn is_artboard_project(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("index.html")) else {
        return false;
    };
    ["vb-artboard", "vs-artboard", "vsm-artboard"]
        .iter()
        .any(|m| text.contains(m))
}

/// 在系统文件管理器中显示目录(02-2-3;跨平台最小实现)。
pub fn reveal_in_explorer(path: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// 读 PNG → egui 纹理(失败返回 None,调用方落占位图)。
fn load_texture(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width().max(1) as usize, img.height().max(1) as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("vb-thumb", color, egui::TextureOptions::LINEAR))
}

/// 主页本地两色插值(卡片 hover 提升;与 `vb_ui::components` 同式)。
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

/// 空态插图用弱色(随主题;不写死颜色值 —— 颜色字面量唯一出处是
/// `vb_ui::theme`,本文件只引用令牌)。
fn t_faint(ui: &egui::Ui) -> Color32 {
    Tokens::get(ui.ctx().theme() == egui::Theme::Dark).text_3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, path: &str) -> RecentItem {
        RecentItem {
            path: path.into(),
            name: name.into(),
            last_opened: 0,
            pinned: false,
            thumb: None,
        }
    }

    #[test]
    fn filter_matches_name_and_path_case_insensitive() {
        let items = [
            item("豆屿咖啡", r"C:\p\landing"),
            item("Poster", r"C:\p\海报\poster"),
        ];
        let view: Vec<&RecentItem> = items.iter().collect();
        assert_eq!(filtered(&view, "").len(), 2, "空串 = 全部");
        assert_eq!(filtered(&view, "豆屿").len(), 1);
        assert_eq!(filtered(&view, "LANDING").len(), 1, "路径匹配大小写不敏感");
        assert_eq!(filtered(&view, "海报").len(), 1);
        assert_eq!(filtered(&view, "不存在").len(), 0);
    }

    // ── 07-Q:artboard 项目识别 + --project 直开/编辑/存回兼容 ──

    /// 临时 artboard 风格项目(index.html 用 `vs-` 前缀标记,与
    /// artboard 仓库产物同形;夹具在 TEMP,**不触碰 artboard 仓库**)。
    fn artboard_fixture(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vb-artboard-fx-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("index.html"),
            r#"<html><body>
<section class="vs-artboard" data-vb-id="aaa001" data-vb-name="主画板" style="width:800px;height:600px;">
  <div class="vs-layer" data-vb-id="bbb002" data-vb-name="标题层" style="left:40px;top:40px;width:300px;height:60px;">
    <h1 data-vb-id="ccc003" data-vb-name="标题">Hello</h1>
  </div>
</section>
</body></html>"#,
        )
        .unwrap();
        d
    }

    #[test]
    fn artboard_project_detected_by_all_marker_dialects() {
        // vs- 前缀
        let dir = artboard_fixture("vs");
        assert!(is_artboard_project(&dir), "vs-artboard 标记必须识别");
        std::fs::remove_dir_all(&dir).unwrap();
        // vb- / vsm- 前缀
        for marker in ["vb-artboard", "vsm-artboard"] {
            let dir = artboard_fixture("mark");
            std::fs::write(
                dir.join("index.html"),
                format!(r#"<section class="{marker}">x</section>"#),
            )
            .unwrap();
            assert!(is_artboard_project(&dir), "{marker} 标记必须识别");
            std::fs::remove_dir_all(&dir).unwrap();
        }
        // 无标记 / 无 index.html → 不认
        let plain = artboard_fixture("plain");
        std::fs::write(plain.join("index.html"), "<p>plain html</p>").unwrap();
        assert!(!is_artboard_project(&plain));
        let empty = std::env::temp_dir().join("vb-artboard-fx-empty");
        let _ = std::fs::remove_dir_all(&empty);
        std::fs::create_dir_all(&empty).unwrap();
        assert!(!is_artboard_project(&empty), "缺 index.html 不误报");
        std::fs::remove_dir_all(&empty).unwrap();
    }

    /// --project 直开 artboard 项目:打开 → 命令层编辑 → 存回,
    /// 画板标记与画板结构全程保真(07-Q 兼容性核验)。
    #[test]
    fn artboard_project_open_edit_save_roundtrip() {
        use vb_doc::commands::Command;
        let _env = crate::ENV_LOCK.lock();
        let file = std::env::temp_dir().join(format!("vb-artboard-ws-{}.json", std::process::id()));
        unsafe {
            std::env::set_var("VB_WORKSPACE", &file);
        }
        let dir = artboard_fixture("round");
        // 打开(resolve_project_dir 对目录原样透传,与 --project 同路)
        let mut app = crate::app::VellumApp::try_open_project(&egui::Context::default(), &dir)
            .expect("artboard 项目必须能直接打开");
        assert!(!app.doc.artboards.is_empty(), "画板标记要被导入成画板");
        let n_before = app.doc.artboards.len();
        // 编辑:改画板名(经 UndoStack 走 Rename 命令层,可撤销)
        let ab = app.doc.artboards[0];
        let sid = app.doc.nodes.get(ab).unwrap().sid.as_str().to_string();
        app.undo
            .push(
                &mut app.doc,
                Command::Rename {
                    sid: sid.clone(),
                    new: "改名后的画板".into(),
                    old: None,
                },
            )
            .unwrap();
        assert!(app.is_dirty(), "编辑后必须标脏");
        // 存回:磁盘产物保留画板结构(标记类按 ADR-014 规范化为 vb- 前缀)
        assert!(app.save_project(), "存回必须成功");
        let saved = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(
            saved.contains("vb-artboard"),
            "存回必须保留画板标记(vs- 是导入方言,落盘规范化为 vb- 前缀)"
        );
        assert!(saved.contains("改名后的画板"), "编辑结果落盘");
        assert_eq!(app.doc.artboards.len(), n_before, "存回不得增删画板");
        std::fs::remove_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(&file);
    }
}
