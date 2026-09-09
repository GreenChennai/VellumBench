//! 应用主体:画布(Vello 纹理合成)+ AI 式交互 + 属性/图层/状态面板。
//!
//! v0.1 范围(路线图 11 篇):选择/矩形/椭圆工具、Alt 复制、Shift 约束、
//! 框选(相交即选中)、Undo/Redo、属性面板、图层列表、状态栏、保存/导出。
//! 文本在画布上以 egui 近似绘制(ADR-0017)。

use std::path::PathBuf;

use egui::{
    pos2, vec2, Align2, Color32, CursorIcon, FontData, FontDefinitions, FontId, Key, Margin,
    PointerButton, Rect, Sense, Stroke, Vec2,
};
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, NodeKind};
use vb_doc::undo::UndoStack;
use vb_render::encode::encode_artboard;
use vb_tools::Camera;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rect,
    Ellipse,
    Hand,
}

enum Drag {
    None,
    /// Space/中键/抓手:平移视图
    Pan {
        start_pan: Vec2,
    },
    /// 移动对象(sid;alt 首动复制出的新 sid)
    MoveObj {
        sid: String,
        start_geom: Geom,
        grab_dx: f64,
        grab_dy: f64,
        moved: bool,
    },
    /// 8 手柄缩放(handle: 0=NW 1=N 2=NE 3=E 4=SE 5=S 6=SW 7=W)
    Resize {
        sid: String,
        start_geom: Geom,
        handle: u8,
        start: Vec2,
        moved: bool,
    },
    /// 旋转(角点外圈)
    Rotate {
        sid: String,
        center: (f64, f64),
        start_angle: f64,
        start_deg: f64,
        moved: bool,
    },
    /// 框选(相交即选中)
    Marquee {
        start: Vec2,
        cur: Vec2,
    },
    /// 矩形/椭圆创建预览
    Create {
        start: Vec2,
        cur: Vec2,
    },
}

pub struct VellumApp {
    pub doc: Document,
    pub undo: UndoStack,
    pub camera: Camera,
    pub tool: Tool,
    /// 稳定 sid(跨 Undo 存活)
    pub selection: Vec<String>,
    pub project_dir: Option<PathBuf>,
    drag: Drag,
    space_down: bool,
    cursor_world: (f64, f64),
    grid_on: bool,
    smart_guides_on: bool,
    /// 本帧吸附参考线(画板本地坐标的线段 [x0,y0,x1,y1]),每帧清空
    smart_guides: Vec<[f64; 4]>,
    /// 双击文本编辑中的 sid
    editing_text: Option<String>,
    status: String,
    last_move_delta: Option<(f64, f64)>,
    canvas_rect: Option<Rect>,
    gpu: Option<GpuCanvas>,
    frame_times: std::collections::VecDeque<f32>,
    show_about: bool,
}

struct GpuCanvas {
    renderer: vello::Renderer,
    tex: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2], egui::TextureId)>,
}

impl VellumApp {
    pub fn new(cc: &eframe::CreationContext<'_>, project: Option<PathBuf>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_dark_theme(&cc.egui_ctx);

        let (doc, project_dir) = match &project {
            Some(p) => match vb_doc::import::import_project(p) {
                Ok(r) => {
                    println!(
                        "已打开:{}(画板 {})",
                        r.project_dir.display(),
                        r.doc.artboards.len()
                    );
                    (r.doc, Some(r.project_dir))
                }
                Err(e) => {
                    eprintln!("打开失败:{e};使用新建文档");
                    (Document::new_default(), None)
                }
            },
            None => (Document::new_default(), None),
        };

        Self {
            doc,
            undo: UndoStack::new(),
            camera: Camera::default(),
            tool: Tool::Select,
            selection: Vec::new(),
            project_dir,
            drag: Drag::None,
            space_down: false,
            cursor_world: (0.0, 0.0),
            grid_on: true,
            smart_guides_on: true,
            smart_guides: Vec::new(),
            editing_text: None,
            status:
                "就绪 — V 选择 · M 矩形 · Alt+拖动 复制 · Shift 约束 · Space 平移 · Ctrl+0 适合"
                    .into(),
            last_move_delta: None,
            canvas_rect: None,
            gpu: None,
            frame_times: std::collections::VecDeque::new(),
            show_about: false,
        }
    }

    // ---------- 命令执行 ----------

    fn exec(&mut self, cmd: Command) {
        if let Err(e) = self.undo.push(&mut self.doc, cmd) {
            self.status = format!("命令失败:{e}");
        }
    }

    fn fit_view(&mut self) {
        let Some(rect) = self.canvas_rect else { return };
        let Some(&ab) = self.doc.artboards.first() else {
            return;
        };
        let Some(_n) = self.doc.nodes.get(ab) else {
            return;
        };
        // 全部画板的联合 bbox(纵向排布)
        let mut min_y = f64::INFINITY;
        let mut max_r = f64::NEG_INFINITY;
        let mut max_b = f64::NEG_INFINITY;
        let mut min_x = f64::INFINITY;
        for &a in &self.doc.artboards {
            if let Some(an) = self.doc.nodes.get(a) {
                min_x = min_x.min(an.geom.x);
                min_y = min_y.min(an.geom.y);
                max_r = max_r.max(an.geom.x + an.geom.w);
                max_b = max_b.max(an.geom.y + an.geom.h);
            }
        }
        let w = (max_r - min_x).max(1.0);
        let h = (max_b - min_y).max(1.0);
        let margin = 60.0f64;
        let zoom = (((rect.width() as f64) - margin * 2.0) / w)
            .min(((rect.height() as f64) - margin * 2.0) / h)
            .min(4.0);
        self.camera.zoom = zoom;
        self.camera.pan_x = rect.left() as f64 + margin - min_x * zoom;
        self.camera.pan_y = rect.top() as f64 + margin - min_y * zoom;
    }

    fn save_project(&mut self) {
        if self.project_dir.is_none() {
            let picked = rfd::FileDialog::new()
                .set_title("选择项目保存目录")
                .pick_folder();
            self.project_dir = picked;
        }
        let Some(dir) = self.project_dir.clone() else {
            self.status = "已取消保存".into();
            return;
        };
        match vb_doc::export::write_project(&self.doc, &dir) {
            Ok(files) => {
                self.doc.rev += 1;
                self.status = format!(
                    "已保存 {} → {}",
                    files
                        .iter()
                        .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(", "),
                    dir.display()
                );
            }
            Err(e) => self.status = format!("保存失败:{e}"),
        }
    }

    fn open_project(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("打开项目目录(含 index.html)")
            .pick_folder()
        {
            match vb_doc::import::import_project(&dir) {
                Ok(r) => {
                    let n = r.doc.artboards.len();
                    self.doc = r.doc;
                    self.undo = UndoStack::new();
                    self.selection.clear();
                    self.project_dir = Some(r.project_dir);
                    self.fit_view();
                    self.status = format!("已打开 {}(画板 {n})", dir.display());
                }
                Err(e) => self.status = format!("打开失败:{e}"),
            }
        }
    }

    fn export_current_artboard_png(&mut self) {
        let Some(dir) = self.project_dir.clone() else {
            self.status = "先保存项目(选一个目录)再导出".into();
            self.save_project();
            return;
        };
        // 当前画板:含选区的画板,否则第一个
        let ab = self
            .selection
            .first()
            .and_then(|sid| self.doc.find_by_sid(sid))
            .and_then(|nid| {
                let mut p = Some(nid);
                loop {
                    match p {
                        Some(id) => {
                            let n = self.doc.nodes.get(id).unwrap();
                            if matches!(n.kind, NodeKind::Artboard) {
                                break Some(id);
                            }
                            p = n.parent;
                        }
                        None => break None,
                    }
                }
            })
            .or(self.doc.artboards.first().copied());
        let Some(ab) = ab else { return };
        let name = self.doc.nodes.get(ab).unwrap().name.clone();
        match vb_export::export_artboard_png(&self.doc, ab, 2.0, false, Some(&dir)) {
            Ok((png, warnings)) => {
                let out = dir.join(vb_export::expand_name_template(
                    vb_export::DEFAULT_TEMPLATE,
                    &self.doc.meta.title,
                    &name,
                    2,
                    "png",
                    1,
                    0,
                    0,
                ));
                match std::fs::write(&out, &png) {
                    Ok(()) => {
                        self.status = format!(
                            "导出 {} @2x({} KB){}",
                            out.display(),
                            png.len() / 1024,
                            if warnings.is_empty() {
                                String::new()
                            } else {
                                format!(";{} 条近似警告", warnings.len())
                            }
                        );
                    }
                    Err(e) => self.status = format!("写文件失败:{e}"),
                }
            }
            Err(e) => self.status = format!("导出失败:{e}"),
        }
    }

    fn artboard_at_world(&self, wx: f64, wy: f64) -> Option<vb_doc::model::NodeId> {
        self.doc.artboards.iter().copied().find(|&a| {
            self.doc
                .nodes
                .get(a)
                .map(|n| {
                    wx >= n.geom.x
                        && wx <= n.geom.x + n.geom.w
                        && wy >= n.geom.y
                        && wy <= n.geom.y + n.geom.h
                })
                .unwrap_or(false)
        })
    }
}

impl eframe::App for VellumApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // FPS 统计
        let dt = ui.ctx().input(|i| i.stable_dt);
        if dt > 0.0 {
            self.frame_times.push_back(dt);
            if self.frame_times.len() > 60 {
                self.frame_times.pop_front();
            }
        }

        self.handle_shortcuts(ui.ctx());
        self.top_menu(ui);
        self.right_panel(ui);
        self.status_bar(ui, frame);
        self.canvas(ui, frame);

        if self.show_about {
            let mut open = self.show_about;
            egui::Window::new("关于 Vellum Bench")
                .open(&mut open)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label(format!(
                        "Vellum Bench v{} · 绘台",
                        env!("CARGO_PKG_VERSION")
                    ));
                    ui.separator();
                    ui.label("用 Illustrator 的操作心智,编辑标准 HTML/CSS 文档。");
                    ui.label("HTML 是文档格式,不是编译产物。");
                });
            self.show_about = open;
        }

        // 双击文本编辑窗口
        if let Some(sid) = self.editing_text.clone() {
            if let Some(nid) = self.doc.find_by_sid(&sid) {
                let mut text = match self.doc.nodes.get(nid).unwrap().kind {
                    NodeKind::Text { ref text, .. } => text.clone(),
                    _ => String::new(),
                };
                let mut open = true;
                let win_title = self
                    .doc
                    .nodes
                    .get(nid)
                    .map(|n| n.name.clone())
                    .unwrap_or_default();
                let mut commit = false;
                let mut cancel = false;
                egui::Window::new(format!("编辑文本 — {win_title}"))
                .open(&mut open)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(420.0)
                            .desired_rows(3),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("提交 (Ctrl+Enter)").clicked() {
                            commit = true;
                        }
                        if ui.button("取消 (Esc)").clicked() {
                            cancel = true;
                        }
                    });
                    if ui.ctx().input(|i| {
                        i.key_pressed(Key::Enter) && (i.modifiers.ctrl || i.modifiers.command)
                    }) {
                        commit = true;
                    }
                    if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
                        cancel = true;
                    }
                });
                if commit {
                    self.exec(Command::SetText {
                        sid: sid.clone(),
                        new: text,
                        old: None,
                    });
                    self.status = "文本已更新".into();
                    self.editing_text = None;
                } else if cancel || !open {
                    self.editing_text = None;
                }
            } else {
                self.editing_text = None;
            }
        }
    }
}

// ---------- 输入 ----------

impl VellumApp {
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        self.space_down = ctx.input(|i| i.key_down(Key::Space));
        if ctx.input(|i| i.key_pressed(Key::Space)) {
            ctx.set_cursor_icon(CursorIcon::Grab);
        }

        let (mods, events): (egui::Modifiers, Vec<egui::Event>) =
            ctx.input(|i| (i.modifiers, i.events.clone()));
        let ctrl = mods.ctrl || mods.command;
        let alt = mods.alt;
        let shift = mods.shift;

        for ev in &events {
            if let egui::Event::Key {
                key, pressed: true, ..
            } = ev
            {
                match (*key, ctrl) {
                    (Key::V, false) => self.tool = Tool::Select,
                    (Key::M, false) => self.tool = Tool::Rect,
                    (Key::L, false) => self.tool = Tool::Ellipse,
                    (Key::H, false) => self.tool = Tool::Hand,
                    (Key::Z, true) => {
                        let label = if shift {
                            self.undo.redo(&mut self.doc).ok().flatten()
                        } else {
                            self.undo.undo(&mut self.doc).ok().flatten()
                        };
                        self.status = match label {
                            Some(l) => format!("{}:{l}", if shift { "重做" } else { "撤销" }),
                            None => "没有可撤销/重做的操作".into(),
                        };
                    }
                    (Key::Y, true) => {
                        let _ = self.undo.redo(&mut self.doc);
                    }
                    (Key::S, true) => self.save_project(),
                    (Key::E, true) => self.export_current_artboard_png(),
                    (Key::O, true) => self.open_project(),
                    (Key::N, true) => {
                        self.doc = Document::new_default();
                        self.undo = UndoStack::new();
                        self.selection.clear();
                        self.project_dir = None;
                        self.fit_view();
                        self.status = "新建文档(1440×900)".into();
                    }
                    (Key::G, true) if shift => self.ungroup_selection(),
                    (Key::G, true) => self.group_selection(),
                    (Key::D, true) => self.transform_again(),
                    (Key::A, true) if !alt => {
                        // 全选(当前画板)
                        if let Some(&ab) = self.doc.artboards.first() {
                            let kids = self.doc.nodes.get(ab).unwrap().children.clone();
                            self.selection = kids
                                .into_iter()
                                .filter(|id| {
                                    self.doc.nodes.get(*id).map(|n| !n.locked).unwrap_or(false)
                                })
                                .map(|id| self.doc.nodes.get(id).unwrap().sid.as_str().to_string())
                                .collect();
                        }
                    }
                    (Key::CloseBracket, true) => {
                        let d = if shift { 10001 } else { 1 };
                        let sids = self.selection.clone();
                        for s in sids { self.reorder(&s, d); }
                    }
                    (Key::OpenBracket, true) => {
                        let d = if shift { -10001 } else { -1 };
                        let sids = self.selection.clone();
                        for s in sids { self.reorder(&s, d); }
                    }
                    (Key::Escape, _) => {
                        self.selection.clear();
                        if !matches!(self.drag, Drag::None) {
                            self.drag = Drag::None;
                            self.status = "已取消".into();
                        }
                    }
                    (Key::Delete, _) | (Key::Backspace, _) => self.delete_selection(),
                    _ => {}
                }
            }
        }
        if let Some((k, _)) = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        }) {
            self.arrow_nudge(k, ctrl, shift);
        }
    }

    fn arrow_nudge(&mut self, key: Key, _ctrl: bool, shift: bool) {
        let step = if shift { 10.0 } else { 1.0 };
        let (dx, dy) = match key {
            Key::ArrowLeft => (-step, 0.0),
            Key::ArrowRight => (step, 0.0),
            Key::ArrowUp => (0.0, -step),
            Key::ArrowDown => (0.0, step),
            _ => return,
        };
        if self.selection.is_empty() {
            self.camera.pan_x += dx;
            self.camera.pan_y += dy;
            return;
        }
        let sids = self.selection.clone();
        for sid in sids {
            if let Some(nid) = self.doc.find_by_sid(&sid) {
                let mut g = self.doc.nodes.get(nid).unwrap().geom;
                g.x += dx;
                g.y += dy;
                self.exec(Command::SetGeom {
                    sid: sid.clone(),
                    new: g,
                    old: None,
                });
            }
        }
        self.last_move_delta = Some((dx, dy));
    }

    fn delete_selection(&mut self) {
        let sids = std::mem::take(&mut self.selection);
        for sid in sids {
            self.exec(Command::Delete {
                target_sid: sid,
                captured: None,
            });
        }
        self.status = "已删除(Ctrl+Z 撤销)".into();
    }

    fn group_selection(&mut self) {
        if self.selection.len() < 2 {
            self.status = "编组需要至少 2 个对象".into();
            return;
        }
        let group_sid = self.doc.alloc_sid();
        let members = self.selection.clone();
        self.exec(Command::Group {
            member_sids: members,
            name: format!("编组 {}", group_sid.as_str()),
            group_sid: group_sid.as_str().to_string(),
            old_slots: None,
        });
        self.selection = vec![group_sid.as_str().to_string()];
        self.status = "已编组(Ctrl+G)".into();
    }

    fn ungroup_selection(&mut self) {
        let Some(sid) = self.selection.first().cloned() else {
            return;
        };
        self.exec(Command::Ungroup {
            group_sid: sid,
            captured: None,
        });
        self.selection.clear();
        self.status = "已取消编组(Ctrl+Shift+G)".into();
    }

    /// 5c425e8f8c036574:delta=+1 524d79fb4e005c42(z 5e8f5347),-1 540e79fb;front/back 7528 00b110000
    fn reorder(&mut self, sid: &str, delta: i32) {
        let Some(nid) = self.doc.find_by_sid(sid) else { return };
        let Some(parent) = self.doc.nodes.get(nid).and_then(|n| n.parent) else { return };
        let len = self.doc.nodes.get(parent).unwrap().children.len();
        let cur = self.doc.nodes.get(parent).unwrap().children.iter().position(|&c| c == nid).unwrap_or(0);
        let new_index = if delta.abs() >= 10000 {
            if delta > 0 { len - 1 } else { 0 }
        } else {
            (cur as i32 + delta).clamp(0, len as i32 - 1) as usize
        };
        if new_index == cur {
            return;
        }
        let parent_sid = self.doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        self.exec(Command::Move {
            sid: sid.to_string(),
            new_parent_sid: parent_sid,
            new_index,
            old: None,
        });
    }

    fn transform_again(&mut self) {        let Some((dx, dy)) = self.last_move_delta else {
            self.status = "没有可再次的变换(先移动一次)".into();
            return;
        };
        let sids = self.selection.clone();
        if sids.is_empty() {
            self.status = "先选中对象".into();
            return;
        }
        for sid in sids {
            if let Some(nid) = self.doc.find_by_sid(&sid) {
                let mut g = self.doc.nodes.get(nid).unwrap().geom;
                g.x += dx;
                g.y += dy;
                self.exec(Command::SetGeom {
                    sid: sid.clone(),
                    new: g,
                    old: None,
                });
            }
        }
        self.status = "再次变换(Ctrl+D)".into();
    }
}

// ---------- 面板 ----------

impl VellumApp {
    fn top_menu(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("文件", |ui| {
                    if ui.button("新建 Ctrl+N").clicked() {
                        self.doc = Document::new_default();
                        self.undo = UndoStack::new();
                        self.selection.clear();
                        self.project_dir = None;
                        self.fit_view();
                    }
                    if ui.button("打开项目… Ctrl+O").clicked() {
                        self.open_project();
                    }
                    ui.separator();
                    if ui.button("保存 Ctrl+S").clicked() {
                        self.save_project();
                    }
                    if ui.button("导出当前画板 PNG @2x Ctrl+E").clicked() {
                        self.export_current_artboard_png();
                    }
                    ui.separator();
                    if ui.button("退出").clicked() {
                        std::process::exit(0);
                    }
                });
                ui.menu_button("编辑", |ui| {
                    let undo_label =
                        format!("撤销 {} Ctrl+Z", self.undo.undo_label().unwrap_or(""));
                    if ui
                        .add_enabled(self.undo.can_undo(), egui::Button::new(undo_label))
                        .clicked()
                    {
                        let _ = self.undo.undo(&mut self.doc);
                    }
                    let redo_label =
                        format!("重做 {} Ctrl+Shift+Z", self.undo.redo_label().unwrap_or(""));
                    if ui
                        .add_enabled(self.undo.can_redo(), egui::Button::new(redo_label))
                        .clicked()
                    {
                        let _ = self.undo.redo(&mut self.doc);
                    }
                    ui.separator();
                    if ui.button("编组 Ctrl+G").clicked() {
                        self.group_selection();
                    }
                    if ui.button("取消编组 Ctrl+Shift+G").clicked() {
                        self.ungroup_selection();
                    }
                });
                ui.menu_button("视图", |ui| {
                    if ui.button("放大 Ctrl++").clicked() {
                        if let Some(r) = self.canvas_rect {
                            self.camera
                                .zoom_at(r.center().x as f64, r.center().y as f64, 1.25);
                        }
                    }
                    if ui.button("缩小 Ctrl+-").clicked() {
                        if let Some(r) = self.canvas_rect {
                            self.camera
                                .zoom_at(r.center().x as f64, r.center().y as f64, 0.8);
                        }
                    }
                    if ui.button("适合窗口 Ctrl+0").clicked() {
                        self.fit_view();
                    }
                    if ui.button("实际大小 Ctrl+1").clicked() {
                        self.camera.zoom = 1.0;
                    }
                    ui.separator();
                    ui.checkbox(&mut self.grid_on, "显示网格");
                });
                ui.menu_button("帮助", |ui| {
                    if ui.button("关于").clicked() {
                        self.show_about = true;
                    }
                });
                ui.separator();
                // 工具条
                for (t, icon) in [
                    (Tool::Select, "➤ 选择"),
                    (Tool::Rect, "▭ 矩形"),
                    (Tool::Ellipse, "◯ 椭圆"),
                    (Tool::Hand, "✋ 抓手"),
                ] {
                    if ui.selectable_label(self.tool == t, icon).clicked() {
                        self.tool = t;
                    }
                }
            });
        });
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("props")
            .default_size(280.0)
            .resizable(true)
            .show(ui, |ui| {
                // --- 画板管理(v0.5) ---
                ui.horizontal(|ui| {
                    ui.heading("画板");
                    if ui.small_button("+ 新建").clicked() {
                        let name = format!("画板 {}", self.doc.artboards.len() + 1);
                        let sid = self.doc.alloc_sid();
                        let mut n = vb_doc::model::Node::new(
                            NodeKind::Artboard,
                            name.clone(),
                            sid.clone(),
                        );
                        n.geom = Geom {
                            x: 0.0,
                            y: 0.0,
                            w: 1440.0,
                            h: 900.0,
                        };
                        // 纵向堆到最下方
                        n.geom.y = self
                            .doc
                            .artboards
                            .iter()
                            .filter_map(|&a| self.doc.nodes.get(a).map(|n| n.geom.y + n.geom.h))
                            .fold(0.0f64, f64::max)
                            + 80.0;
                        let root_sid = self
                            .doc
                            .nodes
                            .get(self.doc.root)
                            .unwrap()
                            .sid
                            .as_str()
                            .to_string();
                        let tree = vb_doc::model::NodeTree {
                            node: n,
                            children: vec![],
                        };
                        self.exec(Command::Insert {
                            parent_sid: root_sid,
                            index: usize::MAX,
                            tree,
                        });
                        self.status = format!("已新建 {}(Shift+O 画板工具 v0.5)", name);
                    }
                });
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let mut delete: Option<String> = None;
                        let mut select: Option<String> = None;
                        for (i, &ab) in self.doc.artboards.clone().iter().enumerate() {
                            let Some(n) = self.doc.nodes.get(ab) else {
                                continue;
                            };
                            let (sid, name) = (n.sid.as_str().to_string(), n.name.clone());
                            let selected = self.selection.last().map(|s| s == &sid).unwrap_or(false);
                            if i > 0 {
                                ui.separator();
                            }
                            if ui
                                .selectable_label(selected, &name)
                                .on_hover_text("点击选中画板(可在图层树重命名)")
                                .clicked()
                            {
                                select = Some(sid.clone());
                            }
                            if self.doc.artboards.len() > 1 && ui.small_button("🗑").clicked() {
                                delete = Some(sid.clone());
                            }
                        }
                        if let Some(sid) = delete {
                            self.exec(Command::Delete {
                                target_sid: sid,
                                captured: None,
                            });
                            self.selection.clear();
                            self.status = "画板已删除".into();
                        }
                        if let Some(sid) = select {
                            self.selection = vec![sid];
                        }
                    });
                });
                ui.separator();

                ui.heading("属性");
                ui.separator();

                // --- 选中对象的属性(先取快照,避免借用冲突) ---
                let sid = self.selection.last().cloned();
                if let Some(sid) = sid {
                    if let Some(nid) = self.doc.find_by_sid(&sid) {
                        let (mut g, mut hidden, mut locked) = {
                            let n = self.doc.nodes.get(nid).unwrap();
                            (n.geom, n.hidden, n.locked)
                        };
                        let style_snapshot = self.doc.nodes.get(nid).unwrap().style.clone();
                        let cur_fill = vb_css_resolve_fill(&style_snapshot);
                        let mut col = cur_fill
                            .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                            .unwrap_or(Color32::WHITE);
                        let mut radius = style_snapshot
                            .iter()
                            .find(|d| d.prop == "border-radius")
                            .and_then(|d| d.value.trim_end_matches("px").parse::<f64>().ok())
                            .unwrap_or(0.0);
                        let mut op = style_snapshot
                            .iter()
                            .find(|d| d.prop == "opacity")
                            .and_then(|d| d.value.parse::<f64>().ok())
                            .unwrap_or(1.0);

                        egui::Grid::new("props_grid")
                            .num_columns(4)
                            .spacing([6.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("X");
                                if ui.add(egui::DragValue::new(&mut g.x).speed(1.0)).changed() {
                                    self.apply_geom(&sid, g);
                                }
                                ui.label("W");
                                if ui.add(egui::DragValue::new(&mut g.w).speed(1.0)).changed() {
                                    self.apply_geom(&sid, g);
                                }
                                ui.label("Y");
                                if ui.add(egui::DragValue::new(&mut g.y).speed(1.0)).changed() {
                                    self.apply_geom(&sid, g);
                                }
                                ui.label("H");
                                if ui.add(egui::DragValue::new(&mut g.h).speed(1.0)).changed() {
                                    self.apply_geom(&sid, g);
                                }
                                ui.end_row();
                            });

                        ui.horizontal(|ui| {
                            ui.label("填充");
                            if ui.color_edit_button_srgba(&mut col).changed() {
                                let [r, gg, b, a] = col.to_array();
                                self.exec(Command::SetStyle {
                                    sid: sid.clone(),
                                    new: set_style_prop(
                                        style_snapshot.clone(),
                                        "background-color",
                                        &vb_common::Rgba::new(r, gg, b, a).to_shortest_hex(),
                                    ),
                                    old: None,
                                });
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("圆角");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut radius)
                                        .speed(1.0)
                                        .range(0.0..=200.0),
                                )
                                .changed()
                            {
                                self.exec(Command::SetStyle {
                                    sid: sid.clone(),
                                    new: set_style_prop(
                                        style_snapshot.clone(),
                                        "border-radius",
                                        &format!("{}px", radius as i64),
                                    ),
                                    old: None,
                                });
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("不透明度");
                            if ui
                                .add(egui::DragValue::new(&mut op).speed(0.01).range(0.0..=1.0))
                                .changed()
                            {
                                let v = format!("{}", (op * 100.0).round() / 100.0);
                                self.exec(Command::SetStyle {
                                    sid: sid.clone(),
                                    new: set_style_prop(style_snapshot.clone(), "opacity", &v),
                                    old: None,
                                });
                            }
                        });
                        if ui.checkbox(&mut hidden, "隐藏").changed() {
                            self.exec(Command::SetFlags {
                                sid: sid.clone(),
                                hidden: Some(hidden),
                                locked: None,
                                old: None,
                            });
                        }
                        if ui.checkbox(&mut locked, "锁定").changed() {
                            self.exec(Command::SetFlags {
                                sid: sid.clone(),
                                hidden: None,
                                locked: Some(locked),
                                old: None,
                            });
                        }
                        ui.separator();
                    }
                } else {
                    ui.colored_label(egui::Color32::GRAY, "未选中对象");
                    ui.label("V 点选 / 拖框选 · M 画矩形 · L 画椭圆");
                    ui.separator();
                }

                // --- 图层列表(全部画板;含层序调整) ---
                ui.heading("图层");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for &ab in self.doc.artboards.clone().iter() {
                        let Some(abn) = self.doc.nodes.get(ab) else {
                            continue;
                        };
                        let ab_sid = abn.sid.as_str().to_string();
                        let ab_name = abn.name.clone();
                        let ab_sel = self.selection.last().map(|s| s.as_str() == ab_sid).unwrap_or(false);
                        let kids = abn.children.clone();

                        // 画板行(可重命名)
                        ui.horizontal(|ui| {
                            let selected = ab_sel;
                            let mut name = ab_name.clone();
                            if ui
                                .selectable_label(selected, format!("📁 {name}"))
                                .clicked()
                            {
                                self.selection = vec![ab_sid.clone()];
                            }
                            let resp = ui.add_sized(
                                [100.0, 18.0],
                                egui::TextEdit::singleline(&mut name).interactive(true),
                            );
                            if resp.lost_focus() && name != ab_name {
                                self.exec(Command::Rename {
                                    sid: ab_sid.clone(),
                                    new: name,
                                    old: None,
                                });
                            }
                        });
                        // 子行(自顶向下)
                        for &c in kids.iter().rev() {
                            let Some(n) = self.doc.nodes.get(c) else {
                                continue;
                            };
                            let row_sid = n.sid.as_str().to_string();
                            let row_kind = kind_icon(&n.kind);
                            let orig_name = n.name.clone();
                            let selected = self
                                .selection
                                .last()
                                .map(|s| s.as_str() == row_sid)
                                .unwrap_or(false);
                            let mut name = orig_name.clone();
                            let mut hidden = n.hidden;
                            let mut locked = n.locked;
                            ui.horizontal(|ui| {
                                ui.label("    ");
                                let eye = if hidden { "≠" } else { "👁" };
                                let lock = if locked { "🔒" } else { "" };
                                if ui
                                    .selectable_label(selected, format!("{eye} {lock} {row_kind}"))
                                    .clicked()
                                {
                                    self.selection = vec![row_sid.clone()];
                                }
                                let resp = ui.add_sized(
                                    [110.0, 18.0],
                                    egui::TextEdit::singleline(&mut name).interactive(true),
                                );
                                if resp.lost_focus() && name != orig_name {
                                    self.exec(Command::Rename {
                                        sid: row_sid.clone(),
                                        new: name,
                                        old: None,
                                    });
                                }
                                if ui.small_button("👁").clicked() {
                                    hidden = !hidden;
                                    self.exec(Command::SetFlags {
                                        sid: row_sid.clone(),
                                        hidden: Some(hidden),
                                        locked: None,
                                        old: None,
                                    });
                                }
                                if ui.small_button("🔒").clicked() {
                                    locked = !locked;
                                    self.exec(Command::SetFlags {
                                        sid: row_sid.clone(),
                                        hidden: None,
                                        locked: Some(locked),
                                        old: None,
                                    });
                                }
                                // 层序:↑ = 前移一层(列表自顶向下 = z 序从高到低)
                                if ui.small_button("↑").clicked() {
                                    self.reorder(&row_sid, 1);
                                }
                                if ui.small_button("↓").clicked() {
                                    self.reorder(&row_sid, -1);
                                }
                            });
                        }
                        ui.separator();
                    }
                });

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

    fn apply_geom(&mut self, sid: &str, g: Geom) {
        self.exec(Command::SetGeom {
            sid: sid.to_string(),
            new: g,
            old: None,
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let backend = frame
            .wgpu_render_state()
            .map(|rs| format!("{:?}", rs.adapter.get_info().backend))
            .unwrap_or_else(|| "无 GPU".into());
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{}%", (self.camera.zoom * 100.0) as i64));
                ui.separator();
                ui.label(format!(
                    "⌖ {:.0}, {:.0}",
                    self.cursor_world.0, self.cursor_world.1
                ));
                ui.separator();
                ui.label(format!("选中 {}", self.selection.len()));
                ui.separator();
                ui.label(format!("rev {} · {}", self.doc.rev, backend));
                ui.separator();
                ui.strong(&self.status);
            });
        });
    }
}

// ---------- 画布 ----------

impl VellumApp {
    fn canvas(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::canvas(ui.style()).fill(Color32::from_rgb(0x14, 0x14, 0x14)))
            .show(ui, |ui| {
                let Some(rect) = self
                    .canvas_rect
                    .or(Some(ui.max_rect()))
                    .map(|r| r.intersect(ui.max_rect()))
                else {
                    return;
                };
                let rect = rect.expand(0.0);
                self.canvas_rect = Some(rect);
                let response = ui.allocate_rect(rect, Sense::click_and_drag());
                let painter = ui.painter().with_clip_rect(rect);

                // 网格画在最底层
                if self.grid_on {
                    draw_grid(&painter, rect, &self.camera);
                }

                // 渲染画布内容(GPU,画板背景会盖住网格)
                self.render_canvas_gpu(frame, rect);

                if let Some(tex_id) = self.tex_id() {
                    painter.image(
                        tex_id,
                        rect,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                self.draw_artboards(&painter, rect.min.to_vec2());
                self.draw_overlays(&painter, rect);

                self.handle_canvas_input(&response, ui.ctx().clone(), rect);
            });
    }

    fn render_canvas_gpu(&mut self, frame: &eframe::Frame, rect: Rect) {
        let Some(rs) = frame.wgpu_render_state() else {
            return;
        };
        let size = [
            (rect.width().max(1.0) as u32).min(4096),
            (rect.height().max(1.0) as u32).min(4096),
        ];

        // 初始化 vello renderer(一次)
        if self.gpu.is_none() {
            match vello::Renderer::new(
                &rs.device,
                vello::RendererOptions {
                    use_cpu: false,
                    antialiasing_support: vello::AaSupport::all(),
                    num_init_threads: std::num::NonZeroUsize::new(1),
                    pipeline_cache: None,
                },
            ) {
                Ok(r) => {
                    self.gpu = Some(GpuCanvas {
                        renderer: r,
                        tex: None,
                    })
                }
                Err(e) => {
                    eprintln!("Vello 初始化失败(画布将无内容):{e}");
                    return;
                }
            }
        }
        let gpu = self.gpu.as_mut().unwrap();

        // 纹理尺寸管理
        let need_recreate = match &gpu.tex {
            Some((_, _, s, _)) => s != &size,
            None => true,
        };
        if need_recreate {
            if let Some((_, _, _, old_id)) = gpu.tex.take() {
                rs.renderer.write().free_texture(&old_id);
            }
            let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vb-canvas"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let id = rs.renderer.write().register_native_texture(
                &rs.device,
                &view,
                wgpu::FilterMode::Linear,
            );
            gpu.tex = Some((tex, view, size, id));
        }

        // 编码 + 渲染(当前画板;多画板逐个编码)
        let mut scene = vello::Scene::new();
        for &ab in &self.doc.artboards {
            if let Ok(list) = encode_artboard(&self.doc, ab) {
                let n = self.doc.nodes.get(ab).unwrap();
                let z = self.camera.zoom;
                let tx = self.camera.pan_x + n.geom.x * z;
                let ty = self.camera.pan_y + n.geom.y * z;
                let tf = vello::kurbo::Affine::translate(vello::kurbo::Vec2::new(tx, ty))
                    * vello::kurbo::Affine::scale(z);
                let mut sub = vello::Scene::new();
                vb_render::gpu::encode_scene(&mut sub, &list);
                scene.append(&sub, Some(tf));
            }
        }
        let gpu = self.gpu.as_mut().unwrap();
        if let Some((_, view, s, _)) = &gpu.tex {
            let params = vello::RenderParams {
                base_color: vello::peniko::Color::from_rgb8(0x14, 0x14, 0x14),
                width: s[0],
                height: s[1],
                antialiasing_method: vello::AaConfig::Area,
            };
            if let Err(e) = gpu
                .renderer
                .render_to_texture(&rs.device, &rs.queue, &scene, view, &params)
            {
                eprintln!("vello 渲染失败:{e}");
            }
        }
    }

    fn tex_id(&self) -> Option<egui::TextureId> {
        self.gpu.as_ref().and_then(|g| g.tex.as_ref().map(|t| t.3))
    }

    fn handle_canvas_input(&mut self, response: &egui::Response, ctx: egui::Context, rect: Rect) {
        // 光标世界坐标(指针先转画布本地)
        if let Some(p) = response.hover_pos() {
            let pl = p - rect.min;
            let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
            self.cursor_world = (wx, wy);
        }
        // Alt+滚轮 / 滚轮缩放与滚动
        let (scroll, alt_down) = ctx.input(|i| (i.smooth_scroll_delta, i.modifiers.alt));
        if response.hovered() && scroll.y != 0.0 {
            if alt_down {
                if let Some(p) = response.hover_pos() {
                    let pl = p - rect.min;
                    self.camera
                        .zoom_at(pl.x as f64, pl.y as f64, (-(scroll.y as f64) / 400.0).exp());
                }
            } else {
                self.camera.pan_y += scroll.y as f64;
                self.camera.pan_x += scroll.x as f64;
            }
        }

        let mods = ctx.input(|i| i.modifiers);
        let alt = mods.alt;
        let shift = mods.shift;
        let _ = alt_down;

        // 本帧参考线清空(绘制在 overlay)
        self.smart_guides.clear();

        // 平移:中键 或 Space+左键 或 抓手工具
        let pan_wanted = self.space_down || self.tool == Tool::Hand;
        if pan_wanted {
            ctx.set_cursor_icon(CursorIcon::Grab);
        }

        if response.dragged_by(PointerButton::Middle)
            || (pan_wanted && response.dragged_by(PointerButton::Primary))
        {
            if let Drag::Pan { start_pan } = self.drag {
                self.camera.pan_x = start_pan.x as f64 + (response.drag_delta().x) as f64;
                self.camera.pan_y = start_pan.y as f64 + (response.drag_delta().y) as f64;
            } else if matches!(self.drag, Drag::None) {
                let d = response.drag_delta();
                self.camera.pan_x += d.x as f64;
                self.camera.pan_y += d.y as f64;
                self.drag = Drag::None;
            }
            return;
        }

        // 双击:进入文本编辑(AI:双击文本进入编辑)
        if response.double_clicked() && self.tool == Tool::Select {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                if let Some(ab) = self.artboard_at_world(wx, wy) {
                    if let Some(nid) = vb_tools::hit_test(&self.doc, ab, wx, wy) {
                        let n = self.doc.nodes.get(nid).unwrap();
                        if matches!(n.kind, NodeKind::Text { .. }) {
                            self.editing_text = Some(n.sid.as_str().to_string());
                            self.status = format!("编辑文本:{}(Ctrl+Enter 提交,Esc 取消)", n.name);
                        }
                    }
                }
            }
        }

        // 选中对象的屏幕 bbox(用于手柄/旋转命中)
        let sel_bbox_screen = self.selection.last().and_then(|sid| {
            let nid = self.doc.find_by_sid(sid)?;
            let bb = vb_tools::abs_bbox(&self.doc, nid)?;
            let (x0, y0) = self.camera.world_to_screen(bb.x0, bb.y0);
            let (x1, y1) = self.camera.world_to_screen(bb.x1, bb.y1);
            Some((
                Rect::from_min_max(
                    pos2(x0 as f32 + rect.min.x, y0 as f32 + rect.min.y),
                    pos2(x1 as f32 + rect.min.x, y1 as f32 + rect.min.y),
                ),
                sid.clone(),
            ))
        });

        if response.drag_started() {
            let Some(p0) = response.interact_pointer_pos() else {
                return;
            };
            let p = p0 - rect.min; // 画布本地

            // --- 1) 手柄/旋转命中(单选优先) ---
            if let (Some((bbox, sid)), true) = (sel_bbox_screen.clone(), self.tool == Tool::Select) {
                // 角外圈 → 旋转
                const RING: f32 = 14.0;
                let corners = [
                    bbox.left_top(),
                    bbox.right_top(),
                    bbox.right_bottom(),
                    bbox.left_bottom(),
                ];
                if let Some(nid) = self.doc.find_by_sid(&sid) {
                    let n = self.doc.nodes.get(nid).unwrap();
                    let cur_deg = n
                        .style_get("transform")
                        .and_then(parse_rotate_deg)
                        .unwrap_or(0.0);
                    for c in corners {
                        let d = (p - (c - rect.min)).length();
                        if d > 6.0 && d < RING + 6.0 {
                            let cx = bbox.center().x as f64;
                            let cy = bbox.center().y as f64;
                            let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                            let a0 = (wy - cy).atan2(wx - cx);
                            self.drag = Drag::Rotate {
                                sid,
                                center: (cx, cy),
                                start_angle: a0,
                                start_deg: cur_deg,
                                moved: false,
                            };
                            return;
                        }
                    }
                }
                // 8 手柄
                if let Some(h) = hit_handle(rect.min + p, bbox) {
                    let nid = self.doc.find_by_sid(&sid).unwrap();
                    let g = self.doc.nodes.get(nid).unwrap().geom;
                    self.drag = Drag::Resize {
                        sid,
                        start_geom: g,
                        handle: h,
                        start: p,
                        moved: false,
                    };
                    return;
                }
            }

            // --- 2) 常规:点选 / 框选 / 创建 ---
            let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
            match self.tool {
                Tool::Hand => {
                    self.drag = Drag::Pan {
                        start_pan: vec2(
                            self.camera.pan_x as f32,
                            self.camera.pan_y as f32,
                        ),
                    };
                }
                Tool::Select => {
                    let hit = self
                        .artboard_at_world(wx, wy)
                        .and_then(|ab| vb_tools::hit_test(&self.doc, ab, wx, wy))
                        .map(|nid| self.doc.nodes.get(nid).unwrap().sid.as_str().to_string());
                    if let Some(sid) = hit {
                        if !self.selection.contains(&sid) {
                            if shift {
                                self.selection.push(sid.clone());
                            } else {
                                self.selection = vec![sid.clone()];
                            }
                        }
                        // Alt = 复制并拖动(AI 招牌)
                        let drag_sid = if alt {
                            let nid = self.doc.find_by_sid(&sid).unwrap();
                            let parent = self.doc.nodes.get(nid).unwrap().parent.unwrap();
                            let new_id = self.doc.clone_subtree(nid, parent);
                            let new_sid =
                                self.doc.nodes.get(new_id).unwrap().sid.as_str().to_string();
                            self.selection = vec![new_sid.clone()];
                            new_sid
                        } else {
                            sid.clone()
                        };
                        let nid = self.doc.find_by_sid(&drag_sid).unwrap();
                        let g = self.doc.nodes.get(nid).unwrap().geom;
                        self.drag = Drag::MoveObj {
                            sid: drag_sid,
                            start_geom: g,
                            grab_dx: wx - g.x,
                            grab_dy: wy - g.y,
                            moved: false,
                        };
                    } else {
                        self.drag = Drag::Marquee { start: p, cur: p };
                        if !shift {
                            self.selection.clear();
                        }
                    }
                }
                Tool::Rect | Tool::Ellipse => {
                    self.drag = Drag::Create { start: p, cur: p };
                }
            }
        }

        if response.dragged() {
            let Some(p0) = response.interact_pointer_pos() else {
                return;
            };
            let p = p0 - rect.min;

            // 缩放 / 旋转(它们自成状态,不与 MoveObj 共路)
            match &self.drag {
                Drag::Resize { .. } | Drag::Rotate { .. } => {}
                _ => {}
            }
            let resize_update: Option<(String, Geom)> = match &mut self.drag {
                Drag::Resize {
                    sid,
                    start_geom,
                    handle,
                    start,
                    ..
                } => {
                    let dx = (p.x - start.x) as f64 / self.camera.zoom;
                    let dy = (p.y - start.y) as f64 / self.camera.zoom;
                    Some((sid.clone(), resize_geom(*start_geom, *handle, dx, dy, shift, alt)))
                }
                _ => None,
            };
            if let Some((sid, g)) = resize_update {
                self.exec(Command::SetGeom { sid, new: g, old: None });
                if let Drag::Resize { moved, .. } = &mut self.drag {
                    *moved = true;
                }
                return;
            }
            if let Drag::Rotate {
                sid,
                center,
                start_angle,
                start_deg,
                ..
            } = &mut self.drag
            {
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                let a = (wy - center.1).atan2(wx - center.0);
                let mut deg = *start_deg + (a - *start_angle).to_degrees();
                if shift {
                    deg = (deg / 15.0).round() * 15.0;
                }
                deg = (deg * 10.0).round() / 10.0;
                let sid = sid.clone();
                if let Some(nid) = self.doc.find_by_sid(&sid) {
                    let mut style = self.doc.nodes.get(nid).unwrap().style.clone();
                    style = set_style_prop(
                        style,
                        "transform",
                        &format!("rotate({}deg)", fmt_deg(deg)),
                    );
                    self.exec(Command::SetStyle { sid, new: style, old: None });
                    if let Drag::Rotate { moved, .. } = &mut self.drag {
                        *moved = true;
                    }
                }
                return;
            }

            // 移动 + 智能参考线
            let drag_update: Option<(String, Geom)> = match &mut self.drag {
                Drag::Marquee { cur, .. } | Drag::Create { cur, .. } => {
                    *cur = p;
                    None
                }
                Drag::MoveObj {
                    sid,
                    start_geom,
                    grab_dx,
                    grab_dy,
                    ..
                } => {
                    let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                    let mut nx = wx - *grab_dx;
                    let mut ny = wy - *grab_dy;
                    let (dx, dy) =
                        vb_tools::constrain_axis(nx - start_geom.x, ny - start_geom.y, shift);
                    nx = (start_geom.x + dx).round();
                    ny = (start_geom.y + dy).round();
                    Some((sid.clone(), Geom { x: nx, y: ny, w: start_geom.w, h: start_geom.h }))
                }
                _ => None,
            };
            if let Some((sid, g)) = drag_update {
                // 智能参考线:对齐兄弟/画板(屏幕空间 6px 阈值)
                let mut g = g;
                if self.smart_guides_on {
                    if let Some(nid) = self.doc.find_by_sid(&sid) {
                        let parent = self.doc.nodes.get(nid).unwrap().parent;
                        let ab = self.artboard_at_world(g.x + 1.0, g.y + 1.0);
                        let (sx, sy, lines) =
                            self.smart_snap(nid, parent, ab, &g, 6.0 / self.camera.zoom);
                        g.x = sx;
                        g.y = sy;
                        self.smart_guides = lines;
                    }
                }
                self.exec(Command::SetGeom { sid: sid.clone(), new: g, old: None });
                if let Drag::MoveObj { moved, start_geom, .. } = &mut self.drag {
                    *moved = true;
                    self.last_move_delta = Some((g.x - start_geom.x, g.y - start_geom.y));
                }
            }
        }

        if response.drag_stopped() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Marquee { start, cur } => {
                    // 相交即选中(AI)
                    let (x0, y0) = self
                        .camera
                        .screen_to_world(start.x.min(cur.x) as f64, start.y.min(cur.y) as f64);
                    let (x1, y1) = self
                        .camera
                        .screen_to_world(start.x.max(cur.x) as f64, start.y.max(cur.y) as f64);
                    let r = vb_common::geom::rect_xywh(x0, y0, x1 - x0, y1 - y0);
                    if let Some(ab) = self.doc.artboards.first().copied() {
                        let hits = vb_tools::marquee_select(&self.doc, ab, r);
                        let mut sids: Vec<String> = hits
                            .into_iter()
                            .map(|id| self.doc.nodes.get(id).unwrap().sid.as_str().to_string())
                            .collect();
                        if shift {
                            sids.extend(self.selection.drain(..));
                        }
                        self.selection = sids;
                        if !self.selection.is_empty() {
                            self.status = format!("框选 {} 个对象", self.selection.len());
                        }
                    }
                }
                Drag::Create { start, cur } => {
                    let (sx, sy) = self.camera.screen_to_world(start.x as f64, start.y as f64);
                    let (cx, cy) = self.camera.screen_to_world(cur.x as f64, cur.y as f64);
                    let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                    if g.w >= 2.0 && g.h >= 2.0 {
                        let sid = self.doc.alloc_sid();
                        let kind = NodeKind::Box;
                        let mut n =
                            vb_doc::model::Node::new(kind, format!("矩形 {}", sid.as_str()), sid.clone());
                        n.geom = g;
                        if self.tool == Tool::Ellipse {
                            n.style.push(vb_css::Decl {
                                prop: "border-radius".into(),
                                value: "50%".into(),
                                important: false,
                            });
                        }
                        n.style.push(vb_css::Decl {
                            prop: "background-color".into(),
                            value: "#d4d4d4".into(),
                            important: false,
                        });
                        n.style.push(vb_css::Decl {
                            prop: "border".into(),
                            value: "1px solid #1a1a1a".into(),
                            important: false,
                        });
                        let (wx, wy) = (g.x, g.y);
                        let ab = self
                            .artboard_at_world(wx, wy)
                            .or(self.doc.artboards.first().copied())
                            .unwrap();
                        let ab_sid = self.doc.nodes.get(ab).unwrap().sid.as_str().to_string();
                        let ab_len = self.doc.nodes.get(ab).unwrap().children.len();
                        self.drag = Drag::None;
                        let tree = vb_doc::model::NodeTree { node: n, children: vec![] };
                        self.exec(Command::Insert { parent_sid: ab_sid, index: ab_len, tree });
                        self.selection = vec![sid.as_str().to_string()];
                        self.status = "已创建对象".into();
                    }
                }
                Drag::MoveObj {
                    sid,
                    start_geom,
                    moved,
                    ..
                } => {
                    if moved {
                        if let Some(nid) = self.doc.find_by_sid(&sid) {
                            let g = self.doc.nodes.get(nid).unwrap().geom;
                            self.last_move_delta = Some((g.x - start_geom.x, g.y - start_geom.y));
                        }
                    }
                }
                Drag::Resize { moved, .. } | Drag::Rotate { moved, .. } => {
                    let _ = moved;
                }
                _ => {}
            }
        }
    }

    /// 智能参考线:移动中的对象边/中心 对齐 兄弟边/中心 或 画板边/中心。
    /// 返回 (吸附后 x, 吸附后 y, 参考线段[画板本地坐标])。
    fn smart_snap(
        &self,
        moving: vb_doc::model::NodeId,
        parent: Option<vb_doc::model::NodeId>,
        artboard: Option<vb_doc::model::NodeId>,
        g: &Geom,
        tol: f64,
    ) -> (f64, f64, Vec<[f64; 4]>) {
        let mut xs: Vec<(f64, f64, f64)> = Vec::new(); // (候选 x, 线 y0, 线 y1)
        let mut ys: Vec<(f64, f64, f64)> = Vec::new(); // (候选 y, 线 x0, 线 x1)

        // 画板边/中心
        if let Some(ab) = artboard {
            if let Some(n) = self.doc.nodes.get(ab) {
                let (ax, ay, aw, ah) = (0.0, 0.0, n.geom.w, n.geom.h);
                xs.push((ax, ay, ay + ah));
                xs.push((ax + aw / 2.0, ay, ay + ah));
                xs.push((ax + aw, ay, ay + ah));
                ys.push((ay, ax, ax + aw));
                ys.push((ay + ah / 2.0, ax, ax + aw));
                ys.push((ay + ah, ax, ax + aw));
            }
        }
        // 兄弟节点
        if let Some(pid) = parent {
            if let Some(pn) = self.doc.nodes.get(pid) {
                for &c in &pn.children {
                    if c == moving {
                        continue;
                    }
                    if let Some(bb) = vb_tools::abs_bbox(&self.doc, c) {
                        let (bx0, by0, bx1, by1) = (bb.x0, bb.y0, bb.x1, bb.y1);
                        xs.push((bx0, by0, by1));
                        xs.push(((bx0 + bx1) / 2.0, by0, by1));
                        xs.push((bx1, by0, by1));
                        ys.push((by0, bx0, bx1));
                        ys.push(((by0 + by1) / 2.0, bx0, bx1));
                        ys.push((by1, bx0, bx1));
                    }
                }
            }
        }

        let mut lines: Vec<[f64; 4]> = Vec::new();
        let mx = [g.x, g.x + g.w / 2.0, g.x + g.w];
        let my = [g.y, g.y + g.h / 2.0, g.y + g.h];

        let mut best_x: Option<(f64, f64)> = None; // (delta, 候选)
        for e in mx {
            for (cand, ly0, ly1) in &xs {
                let d = (e - cand).abs();
                if d <= tol && best_x.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best_x = Some((d, *cand));
                    lines.push([*cand, *ly0 - 12.0, *cand, *ly1 + 12.0]);
                }
            }
        }
        let mut best_y: Option<(f64, f64)> = None;
        for e in my {
            for (cand, lx0, lx1) in &ys {
                let d = (e - cand).abs();
                if d <= tol && best_y.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best_y = Some((d, *cand));
                    lines.push([*lx0 - 12.0, *cand, *lx1 + 12.0, *cand]);
                }
            }
        }
        let nx = best_x.map(|(_, c)| {
            // 对齐的是哪条边?吸附到候选后保持原相对关系:取移动后最接近候选的那条边
            let cur = [g.x, g.x + g.w / 2.0, g.x + g.w]
                .iter()
                .copied()
                .min_by(|a, b| {
                    (*a - c).abs()
                        .partial_cmp(&(*b - c).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(c);
            g.x + (c - cur)
        });
        let ny = best_y.map(|(_, c)| {
            let cur = [g.y, g.y + g.h / 2.0, g.y + g.h]
                .iter()
                .copied()
                .min_by(|a, b| {
                    (*a - c).abs()
                        .partial_cmp(&(*b - c).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(c);
            g.y + (c - cur)
        });
        (nx.unwrap_or(g.x), ny.unwrap_or(g.y), lines)
    }

    fn draw_artboards(&self, painter: &egui::Painter, origin: egui::Vec2) {
        for &ab in &self.doc.artboards {
            let Some(n) = self.doc.nodes.get(ab) else {
                continue;
            };
            let (x0, y0) = self.camera.world_to_screen(n.geom.x, n.geom.y);
            let (x1, y1) = self
                .camera
                .world_to_screen(n.geom.x + n.geom.w, n.geom.y + n.geom.h);
            let r = Rect::from_min_max(
                pos2(x0 as f32 + origin.x, y0 as f32 + origin.y),
                pos2(x1 as f32 + origin.x, y1 as f32 + origin.y),
            );
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(0x3d, 0x3d, 0x3d)),
                egui::StrokeKind::Outside,
            );
            painter.text(
                pos2(r.left(), r.top() - 16.0),
                Align2::LEFT_BOTTOM,
                &n.name,
                FontId::proportional(11.0),
                Color32::from_rgb(0x8a, 0x8a, 0x8a),
            );
        }
    }

    fn draw_overlays(&self, painter: &egui::Painter, viewport: Rect) {
        let origin = viewport.min.to_vec2();
        // 智能参考线(品红,与 AI 同色)
        for l in &self.smart_guides {
            let (x0, y0) = self.camera.world_to_screen(l[0], l[1]);
            let (x1, y1) = self.camera.world_to_screen(l[2], l[3]);
            painter.line_segment(
                [
                    pos2(x0 as f32 + origin.x, y0 as f32 + origin.y),
                    pos2(x1 as f32 + origin.x, y1 as f32 + origin.y),
                ],
                Stroke::new(1.0, Color32::from_rgb(0xff, 0x00, 0xff)),
            );
        }
        if std::env::var("VB_NO_OVERLAY").is_ok() {
            return;
        }
        // 文本近似绘制 + 冻结块占位(ADR-0017)
        let mut ids = Vec::new();
        for &ab in &self.doc.artboards {
            self.doc.subtree(ab, &mut ids);
        }
        for id in ids {
            let Some(n) = self.doc.nodes.get(id) else {
                continue;
            };
            if n.hidden || n.tag == "#text" {
                continue;
            }
            let abs = vb_tools::abs_bbox(&self.doc, id);
            let Some(bb) = abs else { continue };
            let (sx, sy) = self.camera.world_to_screen(bb.x0, bb.y0);
            let (ex, ey) = self.camera.world_to_screen(bb.x1, bb.y1);
            let r = Rect::from_min_max(
                pos2(sx as f32 + origin.x, sy as f32 + origin.y),
                pos2(ex as f32 + origin.x, ey as f32 + origin.y),
            );
            if !r.intersects(viewport) {
                continue;
            }
            match &n.kind {
                NodeKind::Text { text, .. } => {
                    let fs = n
                        .style_get("font-size")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(16.0);
                    let color = n
                        .style_get("color")
                        .and_then(vb_common::color::parse_color)
                        .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                        .unwrap_or(Color32::from_rgb(0x20, 0x20, 0x20));
                    let px_size = (fs * self.camera.zoom) as f32;
                    painter.text(
                        pos2(r.left() + 2.0, r.top() + 2.0),
                        Align2::LEFT_TOP,
                        text,
                        FontId::proportional(px_size.max(6.0)),
                        color,
                    );
                }
                NodeKind::Frozen { .. } => {
                    painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(200, 195, 185, 60));
                    painter.text(
                        pos2(r.left() + 6.0, r.top() + 4.0),
                        Align2::LEFT_TOP,
                        "❄ 冻结块",
                        FontId::proportional(11.0),
                        Color32::from_rgb(0x8a, 0x8a, 0x8a),
                    );
                }
                _ => {}
            }
        }

        // 选中框 + 手柄
        for sid in &self.selection {
            let Some(nid) = self.doc.find_by_sid(sid) else {
                continue;
            };
            let Some(bb) = vb_tools::abs_bbox(&self.doc, nid) else {
                continue;
            };
            let (sx, sy) = self.camera.world_to_screen(bb.x0, bb.y0);
            let (ex, ey) = self.camera.world_to_screen(bb.x1, bb.y1);
            let r = Rect::from_min_max(
                pos2(sx as f32 + origin.x - 0.5, sy as f32 + origin.y - 0.5),
                pos2(ex as f32 + origin.x + 0.5, ey as f32 + origin.y + 0.5),
            );
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0, Color32::from_rgb(0x33, 0x99, 0xff)),
                egui::StrokeKind::Outside,
            );
            for hx in [r.left(), r.center().x, r.right()] {
                for hy in [r.top(), r.center().y, r.bottom()] {
                    if (hx == r.center().x) == (hy == r.center().y) {
                        painter.rect_filled(
                            Rect::from_center_size(pos2(hx, hy), vec2(6.0, 6.0)),
                            1.0,
                            Color32::from_rgb(0x33, 0x99, 0xff),
                        );
                    }
                }
            }
        }

        // 框选/创建预览
        match &self.drag {
            Drag::Marquee { start, cur } => {
                let r = Rect::from_two_pos(pos2(start.x, start.y), pos2(cur.x, cur.y));
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(51, 153, 255, 24));
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, Color32::from_rgb(0x33, 0x99, 0xff)),
                    egui::StrokeKind::Middle,
                );
            }
            Drag::Create { start, cur } => {
                let r = Rect::from_two_pos(pos2(start.x, start.y), pos2(cur.x, cur.y));
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, Color32::from_rgb(0x8a, 0x8a, 0x8a)),
                    egui::StrokeKind::Middle,
                );
            }
            _ => {}
        }
    }
}

// ---------- 辅助 ----------

fn kind_icon(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Artboard => "▣",
        NodeKind::Layer => "📁",
        NodeKind::Group => "📁",
        NodeKind::Box => "▢",
        NodeKind::Text { .. } => "T",
        NodeKind::Image { .. } => "🖼",
        NodeKind::Vector => "✎",
        NodeKind::Slice => "✂",
        NodeKind::Frozen { .. } => "❄",
    }
}

fn set_style_prop(style: Vec<vb_css::Decl>, prop: &str, value: &str) -> Vec<vb_css::Decl> {
    let mut s = style;
    if let Some(d) = s.iter_mut().find(|d| d.prop == prop) {
        d.value = value.to_string();
    } else {
        s.push(vb_css::Decl {
            prop: prop.into(),
            value: value.into(),
            important: false,
        });
    }
    s
}

fn draw_grid(painter: &egui::Painter, rect: Rect, cam: &Camera) {
    let step = 64.0 * cam.zoom as f32;
    if step < 8.0 {
        return;
    }
    let color = Color32::from_rgb(0x3a, 0x3a, 0x3a);
    let start_x = (rect.left() / step).floor() * step;
    let start_y = (rect.top() / step).floor() * step;
    let mut x = start_x;
    while x <= rect.right() {
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(0.5, color),
        );
        x += step;
    }
    let mut y = start_y;
    while y <= rect.bottom() {
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(0.5, color),
        );
        y += step;
    }
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for candidate in [
        "C:\\Windows\\Fonts\\simhei.ttf",
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
    ] {
        if let Ok(bytes) = std::fs::read(candidate) {
            fonts
                .font_data
                .insert("vb-cjk".into(), FontData::from_owned(bytes).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts
                    .families
                    .get_mut(&family)
                    .unwrap()
                    .push("vb-cjk".into());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn setup_dark_theme(ctx: &egui::Context) {
    // 设计文档 03 篇 §七 深色令牌(应用到全部主题,本应用只用深色)
    ctx.all_styles_mut(|style| {
        let vs = &mut style.visuals;
        vs.panel_fill = Color32::from_rgb(0x2a, 0x2a, 0x2a);
        vs.window_fill = Color32::from_rgb(0x2a, 0x2a, 0x2a);
        vs.extreme_bg_color = Color32::from_rgb(0x1e, 0x1e, 0x1e);
        vs.faint_bg_color = Color32::from_rgb(0x24, 0x24, 0x24);
        vs.selection.bg_fill = Color32::from_rgb(0x2e, 0x86, 0xff);
        vs.selection.stroke = Stroke::new(1.0, Color32::from_rgb(0x2e, 0x86, 0xff));
        vs.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3d, 0x3d, 0x3d));
        vs.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2e, 0x86, 0xff));
    });
}

const _: Margin = Margin::ZERO;

/// 从样式快照解析填充色(供属性面板;var() 引用按令牌解析由 encode 层负责)。
fn vb_css_resolve_fill(style: &[vb_css::Decl]) -> Option<vb_common::Rgba> {
    style
        .iter()
        .find(|d| d.prop == "background-color")
        .and_then(|d| vb_common::color::parse_color(&d.value))
}

/// 8 手柄命中:返回 0=NW 1=N 2=NE 3=E 4=SE 5=S 6=SW 7=W;未命中 None。
fn hit_handle(p: egui::Pos2, bbox: Rect) -> Option<u8> {
    const R: f32 = 6.0;
    let pts = [
        bbox.left_top(),
        (bbox.center().x, bbox.top()).into(),
        bbox.right_top(),
        (bbox.right(), bbox.center().y).into(),
        bbox.right_bottom(),
        (bbox.center().x, bbox.bottom()).into(),
        bbox.left_bottom(),
        (bbox.left(), bbox.center().y).into(),
    ];
    pts.iter()
        .position(|c| {
            let c: egui::Pos2 = *c;
            (p - c).length() <= R + 2.0
        })
        .map(|i| i as u8)
}

/// 缩放几何:handle 决定动哪条边;Shift 等比(角手柄);Alt 从中心。
fn resize_geom(
    g0: Geom,
    handle: u8,
    dx: f64,
    dy: f64,
    shift: bool,
    alt: bool,
) -> Geom {
    let (mut x0, mut y0) = (g0.x, g0.y);
    let (mut x1, mut y1) = (g0.x + g0.w, g0.y + g0.h);
    let west = matches!(handle, 0 | 6 | 7);
    let east = matches!(handle, 2 | 3 | 4);
    let north = matches!(handle, 0 | 1 | 2);
    let south = matches!(handle, 4 | 5 | 6);
    let corner = matches!(handle, 0 | 2 | 4 | 6);

    if west {
        x0 += dx;
        if alt {
            x1 -= dx;
        }
    }
    if east {
        x1 += dx;
        if alt {
            x0 -= dx;
        }
    }
    if north {
        y0 += dy;
        if alt {
            y1 -= dy;
        }
    }
    if south {
        y1 += dy;
        if alt {
            y0 -= dy;
        }
    }
    if x1 < x0 {
        std::mem::swap(&mut x0, &mut x1);
    }
    if y1 < y0 {
        std::mem::swap(&mut y0, &mut y1);
    }
    let (mut w, mut h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));

    // Shift 等比(仅角手柄):以较大的轴比率回推另一轴
    if shift && corner && g0.w > 0.0 && g0.h > 0.0 {
        let ratio = g0.w / g0.h;
        let cx = (x0 + x1) / 2.0;
        let cy = (y0 + y1) / 2.0;
        if w / h > ratio {
            h = w / ratio;
        } else {
            w = h * ratio;
        }
        // 保持锚点:Alt=中心,否则固定对侧
        if alt {
            x0 = cx - w / 2.0;
            y0 = cy - h / 2.0;
        } else {
            // 固定对侧角
            let (ax, ay) = match handle {
                0 => (g0.x + g0.w, g0.y + g0.h),
                2 => (g0.x, g0.y + g0.h),
                4 => (g0.x, g0.y),
                _ => (g0.x + g0.w, g0.y),
            };
            x0 = if matches!(handle, 0 | 6 | 7) { ax - w } else { ax };
            y0 = if matches!(handle, 0 | 1 | 2) { ay - h } else { ay };
        }
        x1 = x0 + w;
        y1 = y0 + h;
    }

    // 取整到像素
    Geom {
        x: x0.round(),
        y: y0.round(),
        w: (x1 - x0).round().max(1.0),
        h: (y1 - y0).round().max(1.0),
    }
}

/// 解析 transform 中的 rotate(θdeg) → 度(CSS 顺时针)。
fn parse_rotate_deg(v: &str) -> Option<f64> {
    let i = v.find("rotate(")? + "rotate(".len();
    let rest = &v[i..];
    let end = rest.find(')')?;
    rest[..end].trim().trim_end_matches("deg").trim().parse().ok()
}

/// 角度输出格式化(去尾 0)。
fn fmt_deg(deg: f64) -> String {
    vb_common::units::fmt_num((deg * 10.0).round() / 10.0)
}
