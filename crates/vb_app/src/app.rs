//! 应用主体:画布(Vello 纹理合成)+ AI 式交互 + 属性/图层/状态面板。
//!
//! v0.1 范围(路线图 11 篇):选择/矩形/椭圆工具、Alt 复制、Shift 约束、
//! 框选(相交即选中)、Undo/Redo、属性面板、图层列表、状态栏、保存/导出。
//! 文本在画布上以 egui 近似绘制(ADR-0017)。

use std::path::PathBuf;

use egui::{
    pos2, vec2, Align2, Color32, FontId, Key, Margin, PointerButton, Rect, Sense, Stroke, Vec2,
};
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, NodeKind};
use vb_doc::undo::UndoStack;
use vb_render::encode::encode_artboard;
use vb_tools::Camera;

use crate::shortcuts::{self, InputContext};
use vb_ui::components::{icon_button, PanelTabs, ToolButton};
use vb_ui::cursor as vbcursor;
use vb_ui::fonts as vb_fonts;
use vb_ui::icons::{self, Name};
use vb_ui::theme::{self, semantic, Tokens};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    DirectSelect,
    Rect,
    Ellipse,
    /// 直线段:创建细长 Box(HTML 中即一个 2px 高的色条,02 篇 \)
    Line,
    /// 钢笔:逐点落锚点,直线段连接;点击起点或 Enter 闭合/结束
    Pen,
    Hand,
    /// 缩放工具:单击放大 / Alt+单击缩小 / 拖框缩放到区域
    Zoom,
    /// 文字工具:单击点文本 / 拖框区域文本(06 篇 §5.2)
    Text,
    /// 吸管:点击取色应用到选区;Alt 取全部样式(06 篇 §5.4)
    Eyedropper,
    /// 画板工具:拖框新建画板(06 篇 §3.1)
    Artboard,
}

/// 钢笔锚点:anchor + 出手柄(画板本地坐标;平滑点 h_out=Some,
/// 入手柄 = 镜像;角点 h_out=None)。拖拽落点产生平滑点(06 篇 §5.3)。
#[derive(Debug, Clone, Copy)]
struct PenPt {
    anchor: (f64, f64),
    h_out: Option<(f64, f64)>,
}

impl PenPt {
    fn corner(x: f64, y: f64) -> Self {
        PenPt {
            anchor: (x, y),
            h_out: None,
        }
    }
    /// 入手柄 = anchor 关于 anchor 的镜像(out 的反向延长)。
    fn h_in(&self) -> Option<(f64, f64)> {
        self.h_out
            .map(|(hx, hy)| (2.0 * self.anchor.0 - hx, 2.0 * self.anchor.1 - hy))
    }
}

enum Drag {
    None,
    /// 拖动标尺参考线(idx;松手在标尺内/画布外 = 删除)
    Guide {
        idx: usize,
    },
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
    /// 矩形/椭圆创建预览(直线工具复用同一状态,落点为线段端点)
    Create {
        start: Vec2,
        cur: Vec2,
    },
    /// 缩放工具拖框:松开后把框内区域放大到画布
    ZoomRegion {
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
    smart_guides: Vec<[f64; 4]>,
    /// P4.2 标尺开关与参考线(画板本地坐标;true=水平线)
    /// 轮廓模式(线框):`Mod+Y`(02 篇 §四-视图)
    outline_mode: bool,
    /// 当前主题(true=深色)。P2.7 支持浅色。
    theme_dark: bool,
    /// 右侧面板当前 Tab(P2.6)。
    panel_tab: usize,
    rulers_on: bool,
    guides_visible: bool,
    guides_locked: bool,
    guides: Vec<(bool, f64)>,
    /// P4.3 隔离模式:进入栈(双击编组 push,Esc pop;面包屑 = 栈内容)
    isolate_stack: Vec<vb_doc::model::NodeId>,
    /// P4 钢笔进行中的锚点(世界坐标;平滑点带出手柄)
    pen_points: Vec<PenPt>,
    /// 直接选择:正在拖拽的 (sid, 顶点序号)
    ds_vertex: Option<(String, usize)>,
    /// 双击文本编辑中的 sid
    editing_text: Option<String>,
    status: String,
    last_move_delta: Option<(f64, f64)>,
    canvas_rect: Option<Rect>,
    gpu: Option<GpuCanvas>,
    frame_times: std::collections::VecDeque<f32>,
    show_about: bool,
    show_export: bool,
    export_format: usize,
    export_scale: u32,
    export_transparent: bool,
    /// 内部剪贴板(P3.3):(来源父级 sid, 子树快照)
    clipboard: Vec<(String, vb_doc::model::NodeTree)>,
    /// 连续粘贴的递增偏移(×16px);复制/剪切时归零
    paste_offset: u32,
    /// 命令面板(P3.2,Ctrl+K)
    palette_open: bool,
    palette_query: String,
    /// 上次保存/导入时的 rev(外部修改判定:磁盘变了但 rev 未动 → 自动采用)
    saved_rev: u64,
    /// 文件监听事件通道(Agent/外部编辑器改 HTML → 热重载,v0.6)
    watcher_rx: Option<std::sync::mpsc::Receiver<()>>,
    /// 抑制自身保存触发的重载
    last_self_write: Option<std::time::Instant>,
}

struct GpuCanvas {
    renderer: vello::Renderer,
    tex: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2], egui::TextureId)>,
}

impl VellumApp {
    pub fn new(cc: &eframe::CreationContext<'_>, project: Option<PathBuf>) -> Self {
        let fonts_report = vb_fonts::install(&cc.egui_ctx);
        log::info!("字体安装: {}", fonts_report.summary());

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

        let mut app = Self {
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
            outline_mode: false,
            theme_dark: true,
            panel_tab: 0,
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
            show_export: false,
            export_format: 0,
            export_scale: 2,
            export_transparent: false,
            saved_rev: 0,
            watcher_rx: None,
            last_self_write: None,
            clipboard: Vec::new(),
            paste_offset: 0,
            palette_open: false,
            palette_query: String::new(),
            rulers_on: true,
            guides_visible: true,
            guides_locked: false,
            guides: Vec::new(),
            isolate_stack: Vec::new(),
            pen_points: Vec::new(),
            ds_vertex: None,
        };
        app.watcher_rx = start_watcher(project.as_deref());
        app
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
        // clamp 下限:画布极窄时分子为负会产生负缩放(视图翻转)
        let zoom = (((rect.width() as f64) - margin * 2.0) / w)
            .min(((rect.height() as f64) - margin * 2.0) / h)
            .clamp(0.01, 4.0);
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
                self.saved_rev = self.doc.rev;
                self.last_self_write = Some(std::time::Instant::now());
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

    /// 轮询文件监听(去抖 300ms;Agent 场景默认自动采用外部修改)。
    fn poll_watcher(&mut self) {
        let Some(rx) = &self.watcher_rx else {
            return;
        };
        if rx.try_recv().is_err() {
            return;
        }
        while rx.try_recv().is_ok() {}
        if self
            .last_self_write
            .map(|t| t.elapsed() < std::time::Duration::from_millis(800))
            .unwrap_or(false)
        {
            return; // 自己刚写盘,不算外部修改
        }
        let Some(dir) = self.project_dir.clone() else {
            return;
        };
        if self.doc.rev == self.saved_rev {
            match vb_doc::import::import_project(&dir) {
                Ok(r) => {
                    let n = r.doc.artboards.len();
                    self.doc = r.doc;
                    self.undo = UndoStack::new();
                    self.selection.clear();
                    // 新 arena 的 NodeId 与旧文档无对应关系,全部悬空引用作废
                    self.isolate_stack.clear();
                    self.pen_points.clear();
                    self.ds_vertex = None;
                    self.editing_text = None;
                    self.drag = Drag::None;
                    self.saved_rev = self.doc.rev;
                    self.status = format!("检测到外部修改,已自动采用(Agent 热重载,{n} 画板)");
                }
                Err(e) => self.status = format!("热重载失败:{e}"),
            }
        } else {
            self.status = "检测到磁盘修改,但本地有未保存编辑(未自动采用;先 Ctrl+S 或撤销)".into();
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
                    self.isolate_stack.clear();
                    self.pen_points.clear();
                    self.ds_vertex = None;
                    self.editing_text = None;
                    self.drag = Drag::None;
                    self.project_dir = Some(r.project_dir);
                    self.fit_view();
                    self.status = format!("已打开 {}(画板 {n})", dir.display());
                }
                Err(e) => self.status = format!("打开失败:{e}"),
            }
        }
    }

    /// 当前活动画板(含选区的画板,否则第一个)。
    fn active_artboard(&self) -> Option<vb_doc::model::NodeId> {
        self.selection
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
            .or(self.doc.artboards.first().copied())
    }

    fn active_artboard_name(&self) -> String {
        self.active_artboard()
            .and_then(|a| self.doc.nodes.get(a).map(|n| n.name.clone()))
            .unwrap_or_else(|| "无".into())
    }

    /// 导出对话框执行(v0.5 双引擎)。
    fn run_export_dialog(&mut self) {
        let Some(dir) = self.project_dir.clone() else {
            self.status = "先保存项目(选一个目录)再导出".into();
            self.save_project();
            return;
        };
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let name = self.doc.nodes.get(ab).unwrap().name.clone();
        let scale = self.export_scale;
        let fmt = self.export_format;
        let out_name = vb_export::expand_name_template(
            vb_export::DEFAULT_TEMPLATE,
            &self.doc.meta.title,
            &name,
            scale,
            match fmt {
                0 => "png",
                1 => "svg",
                2 => "pdf",
                3 => "gif",
                _ => "mp4",
            },
            1,
            0,
            0,
        );
        let out = dir.join(out_name);
        match fmt {
            0 => match vb_export::export_artboard_png(
                &self.doc,
                ab,
                scale as f32,
                self.export_transparent,
                Some(&dir),
            ) {
                Ok((png, _warnings)) => match std::fs::write(&out, &png) {
                    Ok(()) => {
                        self.status = format!(
                            "导出 {} @{}x({} KB)",
                            out.display(),
                            scale,
                            png.len() / 1024
                        );
                    }
                    Err(e) => self.status = format!("写文件失败:{e}"),
                },
                Err(e) => self.status = format!("导出失败:{e}"),
            },
            1 => match vb_export::export_artboard_svg(&self.doc, ab, scale) {
                Ok(svg) => match std::fs::write(&out, &svg) {
                    Ok(()) => {
                        self.status = format!(
                            "导出 SVG {} @{}x({} KB)",
                            out.display(),
                            scale,
                            svg.len() / 1024
                        );
                    }
                    Err(e) => self.status = format!("写文件失败:{e}"),
                },
                Err(e) => self.status = format!("导出失败:{e}"),
            },
            browser_fmt => {
                let wpi_dir = std::path::PathBuf::from(vb_export::wpi::DEFAULT_WPI_DIR);
                let wpi_fmt = match browser_fmt {
                    2 => vb_export::wpi::WpiFormat::Pdf,
                    3 => vb_export::wpi::WpiFormat::Gif,
                    _ => vb_export::wpi::WpiFormat::Mp4,
                };
                let req = vb_export::wpi::WpiExportRequest {
                    format: wpi_fmt,
                    scale: if scale >= 4 {
                        4
                    } else if scale >= 2 {
                        2
                    } else {
                        1
                    },
                    width: 1920,
                    transparent: self.export_transparent,
                    out,
                    max_wait: 20.0,
                };
                match vb_export::wpi::export_via_wpi(&self.doc, &dir, &req, &wpi_dir) {
                    Ok(res) => {
                        self.status = format!(
                            "浏览器引擎导出 {}({} KB){}",
                            res.out.display(),
                            std::fs::metadata(&res.out)
                                .map(|m| m.len() / 1024)
                                .unwrap_or(0),
                            if res.warnings.is_empty() {
                                String::new()
                            } else {
                                format!(";{} 条警告", res.warnings.len())
                            }
                        );
                    }
                    Err(e) => self.status = format!("WPI 导出失败:{e}"),
                }
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

    /// 切换工具的唯一入口:清进行中的钢笔锚点与直接选择顶点。
    /// 直接赋值 `self.tool` 会残留上一次未完成的路径(切走再切回钢笔,
    /// 单击会接着旧路径画/误触旧起点闭合)。
    fn set_tool(&mut self, tool: Tool) {
        if self.tool == tool {
            return;
        }
        self.pen_points.clear();
        self.ds_vertex = None;
        self.tool = tool;
    }

    /// 隔离栈顶(当前隔离的编组;空 = 未隔离)。
    fn isolate_top(&self) -> Option<vb_doc::model::NodeId> {
        self.isolate_stack.last().copied()
    }

    /// 选中/命中统一入口:隔离模式下只在隔离子树内拾取。
    fn pick_at_world(&self, wx: f64, wy: f64) -> Option<vb_doc::model::NodeId> {
        if let Some(iso) = self.isolate_top() {
            let ab = vb_tools::artboard_of(&self.doc, iso)?;
            let (ox, oy) = self.doc.artboard_origin(ab);
            return vb_tools::hit_test_root(&self.doc, iso, wx - ox, wy - oy);
        }
        let ab = self.artboard_at_world(wx, wy)?;
        vb_tools::hit_test(&self.doc, ab, wx, wy)
    }

    /// 新对象的插入目标:隔离模式下落进隔离组(06 篇 §4.3),否则所属画板。
    fn insert_target(&self, wx: f64, wy: f64) -> vb_doc::model::NodeId {
        if let Some(iso) = self.isolate_top() {
            return iso;
        }
        self.artboard_at_world(wx, wy)
            .or(self.doc.artboards.first().copied())
            .expect("文档至少有一块画板")
    }

    /// 世界坐标 → 指定父级的本地坐标(累计父级 geom 偏移,止于画板)。
    /// 直接用世界坐标建对象会在第 2+ 画板/编组内产生双倍偏移。
    fn world_to_parent_local(
        &self,
        parent: vb_doc::model::NodeId,
        mut x: f64,
        mut y: f64,
    ) -> (f64, f64) {
        let mut p = self.doc.nodes.get(parent).and_then(|n| n.parent);
        while let Some(pid) = p {
            let pn = self.doc.nodes.get(pid).expect("parent 存活");
            if matches!(pn.kind, NodeKind::Artboard) {
                break;
            }
            x -= pn.geom.x;
            y -= pn.geom.y;
            p = pn.parent;
        }
        (x, y)
    }
}

impl eframe::App for VellumApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // 主题逐帧应用(幂等;P2.7 支持 深/浅 切换)
        theme::apply(ui.ctx(), self.theme_dark);
        // FPS 统计
        let dt = ui.ctx().input(|i| i.stable_dt);
        if dt > 0.0 {
            self.frame_times.push_back(dt);
            if self.frame_times.len() > 60 {
                self.frame_times.pop_front();
            }
        }

        self.poll_watcher();
        self.handle_shortcuts(ui.ctx());
        self.top_menu(ui);
        // <1200px 自动折叠右侧面板(P2.6 验收项)
        if ui.ctx().viewport_rect().width() >= vb_ui::theme::space::COLLAPSE_BELOW {
            self.right_panel(ui);
        }
        self.status_bar(ui, frame);
        self.canvas(ui, frame);
        self.floating_toolbar(ui.ctx());

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

        // 导出对话框(v0.5:双引擎)
        if self.show_export {
            let mut open = self.show_export;
            egui::Window::new("导出")
                .open(&mut open)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    const FORMATS: [&str; 5] = [
                        "PNG @N(原生)",
                        "SVG 矢量(原生)",
                        "PDF(浏览器/WPI)",
                        "GIF(浏览器/WPI)",
                        "MP4(浏览器/WPI)",
                    ];
                    ui.horizontal(|ui| {
                        ui.label("格式");
                        let mut f = self.export_format;
                        let label = FORMATS[f].to_string();
                        ui.add(egui::Slider::new(&mut f, 0..=4).text(label));
                        self.export_format = f;
                    });
                    if self.export_format == 0 || self.export_format == 1 {
                        ui.horizontal(|ui| {
                            ui.label("倍率");
                            for s in [1u32, 2, 3, 4] {
                                if ui
                                    .selectable_label(self.export_scale == s, format!("@{s}x"))
                                    .clicked()
                                {
                                    self.export_scale = s;
                                }
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("倍率");
                            for s in [1u32, 2, 4] {
                                if ui
                                    .selectable_label(self.export_scale == s, format!("@{s}x"))
                                    .clicked()
                                {
                                    self.export_scale = s;
                                }
                            }
                            if self.export_scale == 3 {
                                self.export_scale = 2;
                            }
                        });
                    }
                    if self.export_format == 0 {
                        ui.checkbox(&mut self.export_transparent, "透明背景");
                    }
                    ui.separator();
                    ui.label(format!("目标:当前画板({})", self.active_artboard_name()));
                    if ui.button("导出").clicked() {
                        self.run_export_dialog();
                        self.show_export = false;
                    }
                });
            self.show_export = open;
        }

        // 命令面板(P3.2,Ctrl+K)
        if self.palette_open {
            let mut open = self.palette_open;
            let mut close = false;
            egui::Window::new("命令面板")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.add_sized(
                        [360.0, 22.0],
                        egui::TextEdit::singleline(&mut self.palette_query).hint_text("搜索命令…"),
                    );
                    // Esc 关闭:输入框聚焦时全局 Esc 走 TextEdit 上下文,面板须自行处理
                    if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
                        close = true;
                    }
                    let query = self.palette_query.to_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            let mut executed: Option<String> = None;
                            for &(id, label) in shortcuts::CMD_LABELS {
                                if !query.is_empty()
                                    && !label.to_lowercase().contains(&query)
                                    && !id.contains(&query)
                                {
                                    continue;
                                }
                                let key_text = shortcuts::key_text_for(id).unwrap_or_default();
                                let row = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(format!("{label}    {key_text}"))
                                            .size(12.0),
                                    )
                                    .min_size(egui::vec2(340.0, 20.0)),
                                );
                                if row.clicked() {
                                    executed = Some(id.to_string());
                                }
                            }
                            if let Some(id) = executed {
                                self.run_command(&id, false, false);
                                self.palette_open = false;
                            }
                        });
                });
            if close {
                open = false;
            }
            self.palette_open = open;
        }
    }
}

// ---------- 输入 ----------

impl VellumApp {
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // 输入上下文栈(02 篇 §一):只有栈顶上下文消费按键。
        // 文本编辑 / 输入框聚焦 → TextEdit,工具键与 Delete 一律不生效(B1 修复)。
        let top = self.input_context(ctx);
        self.space_down = top != InputContext::TextEdit && ctx.input(|i| i.key_down(Key::Space));
        if self.space_down && ctx.input(|i| i.key_pressed(Key::Space)) {
            ctx.set_cursor_icon(vbcursor::PAN);
        }

        // 本帧按下的键(含修饰键)。遍历**全部**事件(旧实现只看第一个,方向键连按会丢)。
        let pressed: Vec<(Key, bool, bool, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((
                        *key,
                        modifiers.ctrl || modifiers.command,
                        modifiers.shift,
                        modifiers.alt,
                    )),
                    _ => None,
                })
                .collect()
        });

        for (key, ctrl, shift, alt) in pressed {
            let Some(sc) = shortcuts::lookup(key, ctrl, shift) else {
                continue;
            };
            // 上下文守卫:栈顶不允许 → 不消费(交给输入框 / 面板自行处理)
            if !shortcuts::fires_in(sc, top) {
                continue;
            }
            self.run_command(sc.id, shift, alt);
        }
    }

    /// 当前输入上下文(栈顶)。02 篇 §一 / 14 篇 §4.1。
    fn input_context(&self, ctx: &egui::Context) -> InputContext {
        if self.editing_text.is_some() || ctx.egui_wants_keyboard_input() {
            return InputContext::TextEdit;
        }
        if !matches!(self.drag, Drag::None) {
            return InputContext::Tool;
        }
        InputContext::Canvas
    }

    /// 命令派发单一入口(快捷键 / 菜单 / 未来的命令面板共用)。
    ///
    /// `id` 必须出现在 `shortcuts::IMPLEMENTED_IDS` 中(有测试把关)。
    /// `_alt` 为修饰键上下文预留(当前无命令依赖 Alt 分支:Alt 语义由
    /// 画布拖动层直接处理,见 02 篇 §二 四大灵魂手势)。
    fn run_command(&mut self, id: &str, shift: bool, _alt: bool) {
        debug_assert!(
            shortcuts::is_implemented(id),
            "命令 {id} 未在 shortcuts::IMPLEMENTED_IDS 中声明"
        );
        // 拖拽进行中收敛可派发集合:删除正在拖的对象会让后续每帧 SetGeom
        // 打到死 sid 上刷屏报错;切工具会让 drag 状态与新工具错位
        if matches!(
            self.drag,
            Drag::MoveObj { .. }
                | Drag::Resize { .. }
                | Drag::Rotate { .. }
                | Drag::Marquee { .. }
                | Drag::Create { .. }
                | Drag::ZoomRegion { .. }
        ) && !(id == "canvas.cancel"
            || id.starts_with("app.")
            || id.starts_with("file.")
            || id.starts_with("view."))
        {
            self.status = "拖拽进行中:先松手或 Esc 取消".into();
            return;
        }
        match id {
            // ── 文件 ──
            "file.new" => {
                self.doc = Document::new_default();
                self.undo = UndoStack::new();
                self.selection.clear();
                self.project_dir = None;
                // 旧 doc 的 NodeId 全部失效,进行中的状态一并作废
                self.isolate_stack.clear();
                self.pen_points.clear();
                self.ds_vertex = None;
                self.editing_text = None;
                self.drag = Drag::None;
                self.fit_view();
                self.status = "新建文档(1440×900)".into();
            }
            "file.open" => self.open_project(),
            "file.save" => self.save_project(),
            "file.export_dialog" => self.show_export = true,
            "file.export_repeat" => self.export_current_artboard_png(),
            // ── 编辑 ──
            "edit.undo" | "edit.redo" => {
                let redo = id == "edit.redo";
                // 恢复选区策略(AI 行为):撤销删除→重选被删对象;撤销编组→重选成员;
                // 撤销解组→重选编组;重做反向。其余情形清掉悬空 sid。
                let top = if redo {
                    self.undo.top_redo().cloned()
                } else {
                    self.undo.top().cloned()
                };
                let label = if redo {
                    self.undo.redo(&mut self.doc).ok().flatten()
                } else {
                    self.undo.undo(&mut self.doc).ok().flatten()
                };
                if label.is_some() {
                    self.isolate_stack
                        .retain(|id| self.doc.nodes.get(*id).is_some());
                    self.selection = match (&top, redo) {
                        (Some(Command::Delete { target_sid, .. }), false) => {
                            vec![target_sid.clone()]
                        }
                        (Some(Command::Group { member_sids, .. }), false) => member_sids.clone(),
                        (Some(Command::Ungroup { group_sid, .. }), false) => {
                            vec![group_sid.clone()]
                        }
                        (Some(Command::Insert { tree, .. }), true) => {
                            vec![tree.root_sid().to_string()]
                        }
                        (Some(Command::Delete { target_sid, .. }), true) => self
                            .selection
                            .iter()
                            .filter(|s| *s != target_sid)
                            .cloned()
                            .collect(),
                        (Some(Command::Ungroup { captured, .. }), true) => captured
                            .as_ref()
                            .map(|(_, tree)| {
                                tree.children
                                    .iter()
                                    .map(|c| c.node.sid.as_str().to_string())
                                    .collect()
                            })
                            .unwrap_or_default(),
                        _ => std::mem::take(&mut self.selection),
                    };
                    self.selection.retain(|s| self.doc.find_by_sid(s).is_some());
                }
                self.status = match label {
                    Some(l) => format!("{}:{l}", if redo { "重做" } else { "撤销" }),
                    None => "没有可撤销/重做的操作".into(),
                };
            }
            "edit.select_all" => {
                // 当前画板(含选区推断),不是硬编码第一块
                if let Some(ab) = self.active_artboard() {
                    let kids = self.doc.nodes.get(ab).unwrap().children.clone();
                    self.selection = kids
                        .into_iter()
                        .filter(|id| {
                            self.doc
                                .nodes
                                .get(*id)
                                .map(|n| !n.locked && !n.hidden)
                                .unwrap_or(false)
                        })
                        .map(|id| self.doc.nodes.get(id).unwrap().sid.as_str().to_string())
                        .collect();
                    self.status = format!("已全选 {} 个对象", self.selection.len());
                }
            }
            // ── 对象 ──
            "object.group" => self.group_selection(),
            "object.ungroup" => self.ungroup_selection(),
            "object.transform_again" => self.transform_again(),
            "object.bring_forward" => self.reorder_selection(1),
            "object.bring_to_front" => self.reorder_selection(10001),
            "object.send_backward" => self.reorder_selection(-1),
            "object.send_to_back" => self.reorder_selection(-10001),
            "object.delete" => self.delete_selection(),
            // ── 视图(B2:菜单显示的加速键在此真正落地) ──
            "view.zoom_in" | "view.zoom_out" => {
                let f = if id == "view.zoom_in" { 1.25 } else { 0.8 };
                if let Some(r) = self.canvas_rect {
                    self.camera
                        .zoom_at(r.center().x as f64, r.center().y as f64, f);
                }
                self.status = format!("缩放 {}%", (self.camera.zoom * 100.0) as i64);
            }
            "view.fit" => {
                self.fit_view();
                self.status = format!("适合窗口 {}%", (self.camera.zoom * 100.0) as i64);
            }
            "view.actual_size" => {
                self.camera.zoom = 1.0;
                self.status = "实际大小 100%".into();
            }
            "view.outline" => {
                self.outline_mode = !self.outline_mode;
                self.status = if self.outline_mode {
                    "轮廓模式:开(Mod+Y)".into()
                } else {
                    "轮廓模式:关(Mod+Y)".into()
                };
            }
            "view.toggle_grid" => {
                self.grid_on = !self.grid_on;
                self.status = format!("网格:{}", if self.grid_on { "显示" } else { "隐藏" });
            }
            "view.toggle_theme" => {
                self.theme_dark = !self.theme_dark;
                self.status = if self.theme_dark {
                    "主题:深色"
                } else {
                    "主题:浅色"
                }
                .into();
            }
            "view.toggle_smart_guides" => {
                self.smart_guides_on = !self.smart_guides_on;
                self.smart_guides.clear();
                self.status = format!(
                    "智能参考线:{}",
                    if self.smart_guides_on { "开" } else { "关" }
                );
            }
            "view.toggle_rulers" => {
                self.rulers_on = !self.rulers_on;
                self.status = format!("标尺:{}", if self.rulers_on { "显示" } else { "隐藏" });
            }
            "view.toggle_guides" => {
                self.guides_visible = !self.guides_visible;
                self.status = format!(
                    "参考线:{}",
                    if self.guides_visible {
                        "显示"
                    } else {
                        "隐藏"
                    }
                );
            }
            "view.lock_guides" => {
                self.guides_locked = !self.guides_locked;
                self.status = format!(
                    "参考线:{}",
                    if self.guides_locked {
                        "已锁定"
                    } else {
                        "未锁定"
                    }
                );
            }
            "view.guides_from_selection" => {
                let mut added = 0;
                for sid in &self.selection {
                    if let Some(nid) = self.doc.find_by_sid(sid) {
                        if let Some(_n) = self.doc.nodes.get(nid) {
                            let bb = vb_tools::abs_bbox_world(&self.doc, nid).unwrap_or_default();
                            for pos in [bb.x0, (bb.x0 + bb.x1) / 2.0, bb.x1] {
                                self.guides.push((false, pos));
                                added += 1;
                            }
                            for pos in [bb.y0, (bb.y0 + bb.y1) / 2.0, bb.y1] {
                                self.guides.push((true, pos));
                                added += 1;
                            }
                        }
                    }
                }
                self.status = format!("从选区生成 {added} 条参考线(Ctrl+5)");
            }
            // ── 工具箱(统一经 set_tool:清进行中的钢笔锚点/直接选择顶点) ──
            "tool.select" => self.set_tool(Tool::Select),
            "tool.rect" => self.set_tool(Tool::Rect),
            "tool.ellipse" => self.set_tool(Tool::Ellipse),
            "tool.line" => self.set_tool(Tool::Line),
            "tool.pen" => self.set_tool(Tool::Pen),
            "tool.direct_select" => self.set_tool(Tool::DirectSelect),
            "tool.zoom" => self.set_tool(Tool::Zoom),
            "tool.hand" => self.set_tool(Tool::Hand),
            "tool.text" => self.set_tool(Tool::Text),
            "tool.eyedropper" => self.set_tool(Tool::Eyedropper),
            "tool.artboard" => self.set_tool(Tool::Artboard),
            // ── P3.8 分布(≥3 选中) ──
            "object.distribute_h" => self.distribute_selection(true),
            "object.distribute_v" => self.distribute_selection(false),
            // ── P3.3 剪贴板 ──
            "edit.copy" => self.clipboard_copy(),
            "edit.cut" => {
                self.clipboard_copy();
                self.delete_selection();
            }
            "edit.paste" => self.clipboard_paste(false),
            "edit.paste_in_place" => self.clipboard_paste(true),
            // ── P3.8 对齐 ──
            "align.left" => self.align_selection("left"),
            "align.hcenter" => self.align_selection("hcenter"),
            "align.right" => self.align_selection("right"),
            "align.top" => self.align_selection("top"),
            "align.vcenter" => self.align_selection("vcenter"),
            "align.bottom" => self.align_selection("bottom"),
            // ── P3.9 锁定 / 隐藏 ──
            "object.lock" => {
                let sids = self.selection.clone();
                for sid in sids {
                    self.exec(Command::SetFlags {
                        sid,
                        hidden: None,
                        locked: Some(true),
                        old: None,
                    });
                }
                self.status = "已锁定所选".into();
            }
            "object.unlock_all" => {
                // 走 SetFlags 复合命令入 undo 栈(此前裸改 arena 不可撤销)
                let mut ids = Vec::new();
                for &ab in &self.doc.artboards {
                    self.doc.subtree(ab, &mut ids);
                }
                let cmds: Vec<Command> = ids
                    .into_iter()
                    .filter_map(|id| {
                        let n = self.doc.nodes.get(id)?;
                        if !n.locked {
                            return None;
                        }
                        Some(Command::SetFlags {
                            sid: n.sid.as_str().to_string(),
                            hidden: None,
                            locked: Some(false),
                            old: None,
                        })
                    })
                    .collect();
                if cmds.is_empty() {
                    self.status = "没有已锁定的对象".into();
                } else {
                    self.exec(Command::Compound { cmds });
                    self.status = "已解锁全部".into();
                }
            }
            "object.hide" => {
                let sids = self.selection.clone();
                for sid in sids {
                    self.exec(Command::SetFlags {
                        sid,
                        hidden: Some(true),
                        locked: None,
                        old: None,
                    });
                }
                self.status = "已隐藏所选".into();
            }
            "object.show_all" => {
                let mut ids = Vec::new();
                for &ab in &self.doc.artboards {
                    self.doc.subtree(ab, &mut ids);
                }
                let cmds: Vec<Command> = ids
                    .into_iter()
                    .filter_map(|id| {
                        let n = self.doc.nodes.get(id)?;
                        if !n.hidden {
                            return None;
                        }
                        Some(Command::SetFlags {
                            sid: n.sid.as_str().to_string(),
                            hidden: Some(false),
                            locked: None,
                            old: None,
                        })
                    })
                    .collect();
                if cmds.is_empty() {
                    self.status = "没有已隐藏的对象".into();
                } else {
                    self.exec(Command::Compound { cmds });
                    self.status = "已显示全部".into();
                }
            }
            // ── P3.2 命令面板 ──
            "app.command_palette" => {
                // 切换语义:面板开着再按 Ctrl+K 关闭(此前只能点 X)
                self.palette_open = !self.palette_open;
                self.palette_query.clear();
            }
            // ── 画布 ──
            "canvas.nudge_left" | "canvas.nudge_right" | "canvas.nudge_up"
            | "canvas.nudge_down" => {
                let key = match id {
                    "canvas.nudge_left" => Key::ArrowLeft,
                    "canvas.nudge_right" => Key::ArrowRight,
                    "canvas.nudge_up" => Key::ArrowUp,
                    _ => Key::ArrowDown,
                };
                self.arrow_nudge(key, false, shift);
            }
            "canvas.cancel" => {
                // 钢笔进行中:Esc = 结束开放路径(02 篇 §5.3)
                if self.tool == Tool::Pen && !self.pen_points.is_empty() {
                    self.finish_pen(false);
                    self.status = "钢笔:路径已结束(开放)".into();
                    return;
                }
                // P4.3 隔离模式:Esc 逐层弹出进入栈(面包屑回退)
                if let Some(popped) = self.isolate_stack.pop() {
                    self.selection.clear();
                    let name = self
                        .doc
                        .nodes
                        .get(popped)
                        .map(|n| n.name.clone())
                        .unwrap_or_default();
                    self.status = match self.isolate_top() {
                        Some(up) => format!(
                            "退出 {name} → {}",
                            self.doc
                                .nodes
                                .get(up)
                                .map(|n| n.name.clone())
                                .unwrap_or_default()
                        ),
                        None => format!("退出隔离模式({name})"),
                    };
                    return;
                }
                self.selection.clear();
                if !matches!(self.drag, Drag::None) {
                    // Esc 取消拖拽必须还原几何:移动/缩放中每帧 SetGeom 已落盘,
                    // 只丢状态会让"取消"留下脏数据
                    match std::mem::replace(&mut self.drag, Drag::None) {
                        Drag::MoveObj {
                            sid, start_geom, ..
                        }
                        | Drag::Resize {
                            sid, start_geom, ..
                        } => {
                            if self.doc.find_by_sid(&sid).is_some() {
                                self.exec(Command::SetGeom {
                                    sid,
                                    new: start_geom,
                                    old: None,
                                });
                            }
                        }
                        Drag::Rotate { sid, start_deg, .. } => {
                            if let Some(nid) = self.doc.find_by_sid(&sid) {
                                let deg = if start_deg.abs() < f64::EPSILON {
                                    "none".to_string()
                                } else {
                                    format!("rotate({start_deg}deg)")
                                };
                                let mut style = self.doc.nodes.get(nid).unwrap().style.clone();
                                if let Some(d) = style.iter_mut().find(|d| d.prop == "transform") {
                                    d.value = deg;
                                }
                                self.exec(Command::SetStyle {
                                    sid,
                                    new: style,
                                    old: None,
                                });
                            }
                        }
                        _ => {}
                    }
                    self.smart_guides.clear();
                    self.status = "已取消".into();
                }
            }
            // ── P4 钢笔:Enter 结束路径 ──
            "canvas.pen_finish" => {
                if self.tool == Tool::Pen && !self.pen_points.is_empty() {
                    self.finish_pen(false);
                    self.status = "钢笔:路径已结束".into();
                }
            }
            // ── 应用级(无键位,仅菜单) ──
            "app.about" => self.show_about = true,
            "app.quit" => std::process::exit(0),
            other => {
                debug_assert!(false, "命令 {other} 未在 run_command 中实现");
                log::warn!("未实现的命令:{other}");
                self.status = format!("命令未实现:{other}");
            }
        }
    }

    /// 按层序调整当前选区(`Mod+[`/`]` 与菜单共用)。
    fn reorder_selection(&mut self, delta: i32) {
        let sids = self.selection.clone();
        for s in sids {
            self.reorder(&s, delta);
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
        // 多选解散全部组(AI 行为);散开后重选原成员
        let groups: Vec<String> = self
            .selection
            .iter()
            .filter(|sid| {
                self.doc
                    .find_by_sid(sid)
                    .and_then(|nid| self.doc.nodes.get(nid))
                    .is_some_and(|n| matches!(n.kind, NodeKind::Group))
            })
            .cloned()
            .collect();
        if groups.is_empty() {
            self.status = "取消编组:选中对象里没有编组".into();
            return;
        }
        let mut members: Vec<String> = Vec::new();
        for g in &groups {
            if let Some(gid) = self.doc.find_by_sid(g) {
                for c in &self.doc.nodes.get(gid).unwrap().children {
                    members.push(self.doc.nodes.get(*c).unwrap().sid.as_str().to_string());
                }
            }
        }
        for g in groups {
            self.exec(Command::Ungroup {
                group_sid: g,
                captured: None,
            });
        }
        self.selection = members;
        self.status = "已取消编组(Ctrl+Shift+G)".into();
    }

    /// 剪贴板:复制所选子树(非破坏;NodeTree::from_document 只克隆)。
    fn clipboard_copy(&mut self) {
        if self.selection.is_empty() {
            self.status = "剪贴板:未选中对象".into();
            return;
        }
        let mut buf = Vec::new();
        for sid in &self.selection {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                if let Some(n) = self.doc.nodes.get(nid) {
                    let parent_sid = n
                        .parent
                        .and_then(|p| self.doc.nodes.get(p))
                        .map(|p| p.sid.as_str().to_string())
                        .unwrap_or_default();
                    if let Some(tree) = vb_doc::model::NodeTree::from_document(&self.doc, nid) {
                        buf.push((parent_sid, tree));
                    }
                }
            }
        }
        self.clipboard = buf;
        self.paste_offset = 0;
        self.status = format!("已复制 {} 个对象", self.clipboard.len());
    }

    /// 剪贴板:粘贴。`in_place` = 原坐标(AI 的贴在前面);否则按 16px 递增偏移。
    fn clipboard_paste(&mut self, in_place: bool) {
        if self.clipboard.is_empty() {
            self.status = "剪贴板为空".into();
            return;
        }
        let offset = if in_place { 0 } else { self.paste_offset };
        if !in_place {
            // 就地粘贴不消耗偏移预算(否则后续普通粘贴多跳 16px)
            self.paste_offset += 1;
        }
        let dx = (offset * 16) as f64;
        let dy = (offset * 16) as f64;

        // 深拷贝出命令序列(整批一个 undo 条目)
        let entries = self.clipboard.clone();
        let mut cmds: Vec<Command> = Vec::new();
        let mut pasted_sids: Vec<String> = Vec::new();
        for (parent_sid, tree) in &entries {
            let mut tree = tree.clone();
            re_sid_tree(&mut tree, &mut self.doc);
            if !in_place {
                tree.node.geom.x += dx;
                tree.node.geom.y += dy;
            }
            pasted_sids.push(tree.node.sid.as_str().to_string());
            cmds.push(Command::Insert {
                parent_sid: parent_sid.clone(),
                index: usize::MAX,
                tree,
            });
        }
        self.exec(Command::Compound { cmds });
        self.selection = pasted_sids;
        self.status = format!(
            "已粘贴 {} 个对象{}",
            self.clipboard.len(),
            if in_place { "(就地)" } else { "" }
        );
    }

    /// 分布(P3.8 尾巴):≥3 个选中时,让相邻对象间距相等。
    /// `horizontal` = 水平分布;否则垂直。
    fn distribute_selection(&mut self, horizontal: bool) {
        let mut items: Vec<(String, Geom)> = Vec::new();
        for sid in &self.selection {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                if let Some(n) = self.doc.nodes.get(nid) {
                    items.push((sid.clone(), n.geom));
                }
            }
        }
        if items.len() < 3 {
            self.status = "分布需要至少 3 个对象".into();
            return;
        }
        // 按前缘排序(左→右或上→下):游标按前缘推进,排序也必须按前缘,
        // 否则尺寸悬殊时右缘序 ≠ 前缘序,分布结果互相穿越
        if horizontal {
            items.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        } else {
            items.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
        }
        let first = items.first().unwrap().1;
        let last = items.last().unwrap().1;
        // 首尾不动,中间等间距
        let (total_span, _size_sum): (f64, f64) = if horizontal {
            let span = (last.x + last.w) - first.x - items.iter().map(|(_, g)| g.w).sum::<f64>();
            (span, items.iter().map(|(_, g)| g.w).sum())
        } else {
            let span = (last.y + last.h) - first.y - items.iter().map(|(_, g)| g.h).sum::<f64>();
            (span, items.iter().map(|(_, g)| g.h).sum())
        };
        let n = items.len();
        if n < 2 {
            return;
        }
        let gap = total_span / (n - 1) as f64;
        let mut cmds: Vec<Command> = Vec::new();
        let mut cursor = if horizontal { first.x } else { first.y };
        for (i, (sid, g)) in items.iter().enumerate() {
            if i == 0 || i == n - 1 {
                // 首尾不动,但仍推进游标
                cursor += if horizontal { g.w + gap } else { g.h + gap };
                continue;
            }
            let mut ng = *g;
            if horizontal {
                ng.x = cursor.round();
                cursor += ng.w + gap;
            } else {
                ng.y = cursor.round();
                cursor += ng.h + gap;
            }
            cmds.push(Command::SetGeom {
                sid: sid.clone(),
                new: ng,
                old: None,
            });
        }
        let count = cmds.len();
        let axis = if horizontal { "水平" } else { "垂直" };
        if count > 0 {
            self.exec(Command::Compound { cmds });
            self.status = format!("已{}分布 {count} 个对象(间距 {gap:.0}px)", axis);
        }
    }

    /// 钢笔结束:closed = 闭合路径;否则开放路径(仅描边)。
    /// 锚点 → BezPath(直线段;平滑手柄属 P4 后半)。
    fn finish_pen(&mut self, closed: bool) {
        let pts = std::mem::take(&mut self.pen_points);
        if pts.len() < 2 {
            return;
        }
        // 平滑点(拖出出手柄)→ 三次贝塞尔;单柄 → 二次;无柄 → 直线(06 篇 §5.3)
        let mut path = vb_common::geom::BezPath::new();
        path.move_to(vb_common::geom::Point::new(
            pts[0].anchor.0,
            pts[0].anchor.1,
        ));
        let n = pts.len();
        let segs = if closed { n } else { n - 1 };
        for i in 1..=segs {
            let prev = &pts[i - 1];
            let cur = &pts[i % n];
            let end = vb_common::geom::Point::new(cur.anchor.0, cur.anchor.1);
            let c1 = prev.h_out.map(|(x, y)| vb_common::geom::Point::new(x, y));
            let c2 = cur.h_in().map(|(x, y)| vb_common::geom::Point::new(x, y));
            match (c1, c2) {
                (Some(a), Some(b)) => path.curve_to(a, b, end),
                (Some(a), None) | (None, Some(a)) => path.quad_to(a, end),
                (None, None) => path.line_to(end),
            }
        }
        if closed {
            path.close_path();
        }
        self.create_vector_node(path, closed);
    }

    /// 由路径创建矢量节点(P4.5)。
    fn create_vector_node(&mut self, path: vb_common::geom::BezPath, closed: bool) {
        use kurbo::Shape;
        let (minx, miny, maxx, maxy) = {
            let bb = path.bounding_box();
            (bb.x0, bb.y0, bb.x1, bb.y1)
        };
        let sid = self.doc.alloc_sid();
        let mut n = vb_doc::model::Node::new(
            NodeKind::Vector { path: path.clone() },
            format!("路径 {}", sid.as_str()),
            sid.clone(),
        );
        n.tag = "svg".into();
        n.geom = Geom {
            x: minx.round(),
            y: miny.round(),
            w: (maxx - minx).ceil().max(1.0),
            h: (maxy - miny).ceil().max(1.0),
        };
        // 路径以节点原点为基准:平移到 geom.x/y
        let mut shifted = vb_common::geom::BezPath::new();
        for el in &path.elements().to_vec() {
            use vb_common::geom::PathEl;
            match &el {
                PathEl::MoveTo(p) => {
                    shifted.move_to(*p - vb_common::geom::Vec2::new(n.geom.x, n.geom.y))
                }
                PathEl::LineTo(p) => {
                    shifted.line_to(*p - vb_common::geom::Vec2::new(n.geom.x, n.geom.y))
                }
                PathEl::QuadTo(c, p) => shifted.quad_to(
                    *c - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                    *p - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                ),
                PathEl::CurveTo(c1, c2, p) => shifted.curve_to(
                    *c1 - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                    *c2 - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                    *p - vb_common::geom::Vec2::new(n.geom.x, n.geom.y),
                ),
                PathEl::ClosePath => shifted.close_path(),
            }
        }
        n.kind = NodeKind::Vector { path: shifted };
        if closed {
            n.style.push(vb_css::Decl {
                prop: "fill".into(),
                // vb-token-ok: 钢笔闭合形状的默认填充(文档内容,非 UI 皮肤,同下方直线描边)
                value: "#d4d4d4".into(),
                important: false,
            });
        }
        n.style.push(vb_css::Decl {
            prop: "stroke".into(),
            // vb-token-ok: 钢笔路径的默认描边(文档内容,非 UI 皮肤)
            value: "#1a1a1a".into(),
            important: false,
        });
        n.style.push(vb_css::Decl {
            prop: "stroke-width".into(),
            value: "1.5px".into(),
            important: false,
        });
        let parent = self.insert_target(n.geom.x, n.geom.y);
        let (dx, dy) = self.world_to_parent_local(parent, 0.0, 0.0);
        n.geom.x += dx;
        n.geom.y += dy;
        let parent_sid = self.doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        let ab_len = self.doc.nodes.get(parent).unwrap().children.len();
        let tree = vb_doc::model::NodeTree {
            node: n,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid,
            index: ab_len,
            tree,
        });
        self.selection = vec![sid.as_str().to_string()];
    }

    /// 直接选择:命中检测 — 找光标附近矢量节点的顶点。返回 (sid, 顶点序号)。
    fn find_vector_vertex(&self, wx: f64, wy: f64, tol: f64) -> Option<(String, usize)> {
        for &ab in &self.doc.artboards {
            let mut ids = Vec::new();
            self.doc.subtree(ab, &mut ids);
            for id in ids {
                let Some(n) = self.doc.nodes.get(id) else {
                    continue;
                };
                if n.hidden || n.locked {
                    continue;
                }
                if let NodeKind::Vector { path } = &n.kind {
                    for (i, el) in path.elements().iter().enumerate() {
                        use vb_common::geom::PathEl;
                        let p = match el {
                            PathEl::MoveTo(p) | PathEl::LineTo(p) => *p,
                            _ => continue,
                        };
                        // 路径以节点原点存储 → 世界 = 世界 bbox 原点 + 点;
                        // 单个节点取不到 bbox 只跳过该节点(此前 `?` 会放弃整棵树)
                        let Some(bb) = vb_tools::abs_bbox_world(&self.doc, id) else {
                            continue;
                        };
                        let ax = bb.x0 + p.x;
                        let ay = bb.y0 + p.y;
                        if (ax - wx).hypot(ay - wy) <= tol {
                            return Some((n.sid.as_str().to_string(), i));
                        }
                    }
                }
            }
        }
        None
    }

    /// 取矢量节点的顶点绝对坐标(直接选择渲染/拖拽用)。
    fn vector_vertices(&self, sid: &str) -> Vec<(usize, f64, f64)> {
        let Some(nid) = self.doc.find_by_sid(sid) else {
            return vec![];
        };
        let Some(n) = self.doc.nodes.get(nid) else {
            return vec![];
        };
        let NodeKind::Vector { path } = &n.kind else {
            return vec![];
        };
        let bb = vb_tools::abs_bbox_world(&self.doc, nid).unwrap_or(vb_common::geom::Rect::ZERO);
        path.elements()
            .iter()
            .enumerate()
            .filter_map(|(i, el)| {
                use vb_common::geom::PathEl;
                match el {
                    PathEl::MoveTo(p) | PathEl::LineTo(p) => Some((i, bb.x0 + p.x, bb.y0 + p.y)),
                    _ => None,
                }
            })
            .collect()
    }

    /// 对齐(P3.8):多选 → 在选择包围盒内对齐;单选 → 对齐所属画板。
    fn align_selection(&mut self, mode: &str) {
        if self.selection.is_empty() {
            self.status = "对齐:未选中对象".into();
            return;
        }
        // (sid, 当前几何, 绝对 bbox)
        let mut items: Vec<(String, Geom, vb_common::geom::Rect)> = Vec::new();
        for sid in &self.selection {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                if let Some(bb) = vb_tools::abs_bbox(&self.doc, nid) {
                    if let Some(n) = self.doc.nodes.get(nid) {
                        items.push((sid.clone(), n.geom, bb));
                    }
                }
            }
        }
        if items.is_empty() {
            return;
        }
        // 目标包围盒:单选 = 画板;多选 = 选择集合的包围盒
        let (bx0, by0, bx1, by1) = if items.len() == 1 {
            // 目标画板 = 该对象所属画板(沿父链上溯;坐标探测会找错画板)。
            // 对象 bbox 是画板本地坐标,目标框同样取本地系 (0,0,w,h)
            let ab_sid = items[0].0.clone();
            let ab = self
                .doc
                .find_by_sid(&ab_sid)
                .and_then(|nid| vb_tools::artboard_of(&self.doc, nid));
            match ab.and_then(|a| self.doc.nodes.get(a).map(|n| (n.geom.w, n.geom.h))) {
                Some((aw, ah)) => (0.0, 0.0, aw, ah),
                None => (0.0, 0.0, 1440.0, 900.0),
            }
        } else {
            let mut bx0 = f64::INFINITY;
            let mut by0 = f64::INFINITY;
            let mut bx1 = f64::NEG_INFINITY;
            let mut by1 = f64::NEG_INFINITY;
            for (_, _, bb) in &items {
                bx0 = bx0.min(bb.x0);
                by0 = by0.min(bb.y0);
                bx1 = bx1.max(bb.x1);
                by1 = by1.max(bb.y1);
            }
            (bx0, by0, bx1, by1)
        };

        let mut cmds: Vec<Command> = Vec::new();
        for (sid, g, bb) in &items {
            let (nx0, ny0) = match mode {
                "left" => (bx0, bb.y0),
                "hcenter" => ((bx0 + bx1) / 2.0 - bb.width() / 2.0, bb.y0),
                "right" => (bx1 - bb.width(), bb.y0),
                "top" => (bb.x0, by0),
                "vcenter" => (bb.x0, (by0 + by1) / 2.0 - bb.height() / 2.0),
                "bottom" => (bb.x0, by1 - bb.height()),
                _ => continue,
            };
            // 绝对位移转相对位移(节点 geom 相对画板/父级)
            let dx = nx0 - bb.x0;
            let dy = ny0 - bb.y0;
            if dx.abs() < 0.5 && dy.abs() < 0.5 {
                continue;
            }
            cmds.push(Command::SetGeom {
                sid: sid.clone(),
                new: Geom {
                    x: g.x + dx,
                    y: g.y + dy,
                    w: g.w,
                    h: g.h,
                },
                old: None,
            });
        }
        if cmds.is_empty() {
            self.status = "对齐:无需移动".into();
            return;
        }
        let n = cmds.len();
        self.exec(Command::Compound { cmds });
        self.status = format!("已对齐 {n} 个对象({mode})");
    }

    /// 层序调整:delta=+1 前移一层(z 序升),-1 后移;front/back 用 ±10000
    fn reorder(&mut self, sid: &str, delta: i32) {
        let Some(nid) = self.doc.find_by_sid(sid) else {
            return;
        };
        let Some(parent) = self.doc.nodes.get(nid).and_then(|n| n.parent) else {
            return;
        };
        let len = self.doc.nodes.get(parent).unwrap().children.len();
        let cur = self
            .doc
            .nodes
            .get(parent)
            .unwrap()
            .children
            .iter()
            .position(|&c| c == nid)
            .unwrap_or(0);
        let new_index = if delta.abs() >= 10000 {
            if delta > 0 {
                len - 1
            } else {
                0
            }
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

    fn transform_again(&mut self) {
        let Some((dx, dy)) = self.last_move_delta else {
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
                // 点击的菜单项先收集,菜单全部渲染完再派发(避免借用冲突)。
                // 键位文本一律查 `shortcuts` 注册表,禁止在 label 里手写(B2 根治)。
                let mut fired: Option<&'static str> = None;

                ui.menu_button("文件", |ui| {
                    for item in shortcuts::MENU_FILE {
                        if menu_item_button(ui, item, true).clicked() {
                            fired = Some(item.id);
                            ui.close();
                        }
                    }
                });

                ui.menu_button("编辑", |ui| {
                    for item in shortcuts::MENU_EDIT {
                        // 撤销/重做显示"会撤销什么",与 AI 一致
                        let extra = match item.id {
                            "edit.undo" => self.undo.undo_label().unwrap_or("").to_string(),
                            "edit.redo" => self.undo.redo_label().unwrap_or("").to_string(),
                            _ => String::new(),
                        };
                        let enabled = match item.id {
                            "edit.undo" => self.undo.can_undo(),
                            "edit.redo" => self.undo.can_redo(),
                            _ => true,
                        };
                        if menu_item_button_with(ui, item, extra.as_str(), enabled).clicked() {
                            fired = Some(item.id);
                            ui.close();
                        }
                    }
                });

                ui.menu_button("对象", |ui| {
                    for item in shortcuts::MENU_OBJECT {
                        if menu_item_button(ui, item, true).clicked() {
                            fired = Some(item.id);
                            ui.close();
                        }
                    }
                });

                ui.menu_button("视图", |ui| {
                    for item in shortcuts::MENU_VIEW {
                        match item.id {
                            // 三个开关:复选呈现,但状态由命令派发统一改写
                            "view.toggle_grid"
                            | "view.toggle_smart_guides"
                            | "view.outline"
                            | "view.toggle_theme" => {
                                let mut cur = match item.id {
                                    "view.toggle_grid" => self.grid_on,
                                    "view.toggle_smart_guides" => self.smart_guides_on,
                                    "view.toggle_theme" => !self.theme_dark,
                                    _ => self.outline_mode,
                                };
                                ui.horizontal(|ui| {
                                    if ui.checkbox(&mut cur, item.label).changed() {
                                        fired = Some(item.id);
                                    }
                                    if let Some(k) = shortcuts::key_text_for(item.id) {
                                        ui.weak(k);
                                    }
                                });
                            }
                            _ => {
                                if menu_item_button(ui, item, true).clicked() {
                                    fired = Some(item.id);
                                    ui.close();
                                }
                            }
                        }
                    }
                });

                ui.menu_button("帮助", |ui| {
                    for item in shortcuts::MENU_HELP {
                        if menu_item_button(ui, item, true).clicked() {
                            fired = Some(item.id);
                            ui.close();
                        }
                    }
                });

                if let Some(id) = fired {
                    self.run_command(id, false, false);
                }
            });
        });
    }

    /// 底部浮动工具条(P2.6):圆角 12、不透明度 0.96、底部居中锚定。
    fn floating_toolbar(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("vb-floating-toolbar"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -vb_ui::theme::space::S3])
            .order(egui::Order::Middle)
            .show(ctx, |ui| {
                let t = Tokens::get(self.theme_dark);
                egui::Frame::canvas(ui.style())
                    .fill(t.bg_raised.gamma_multiply(0.96))
                    .stroke(Stroke::new(1.0, t.border))
                    .corner_radius(vb_ui::theme::radius::xl())
                    .inner_margin(vb_ui::theme::space::S3)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = vb_ui::theme::space::S2;
                            for (tool, icon, label, key) in [
                                (Tool::Select, Name::ToolSelect, "选择", "V"),
                                (Tool::Rect, Name::ToolRect, "矩形", "M"),
                                (Tool::Ellipse, Name::ToolEllipse, "椭圆", "L"),
                                (Tool::Line, Name::Crosshair, "直线", "\\"),
                                (Tool::Pen, Name::KindVector, "钢笔", "P"),
                                (Tool::DirectSelect, Name::ToolSelect, "直接选择", "A"),
                                (Tool::Zoom, Name::ZoomIn, "缩放", "Z"),
                                (Tool::Hand, Name::ToolHand, "抓手", "H"),
                                (Tool::Text, Name::ToolText, "文字", "T"),
                                (Tool::Eyedropper, Name::ToolEyedropper, "吸管", "I"),
                                (Tool::Artboard, Name::ToolArtboard, "画板", "Shift+O"),
                            ] {
                                if ToolButton::new(icon, label)
                                    .shortcut(key)
                                    .with_label()
                                    .active(self.tool == tool)
                                    .ui(ui)
                                    .clicked()
                                {
                                    // 与快捷键同一条命令路径(统一清理工具状态)
                                    let id = match tool {
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
                                    };
                                    self.run_command(id, false, false);
                                }
                            }
                        });
                    });
            });
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("props")
            .default_size(280.0)
            .resizable(true)
            .show(ui, |ui| {
                // Tab 条(P2.6):属性 / 图层 / 令牌
                {
                    let at = &mut self.panel_tab;
                    PanelTabs::new(&["属性", "图层", "令牌"], at).ui(ui);
                }
                match self.panel_tab {
                    0 => {
                        // 属性(含选中对象的网页能力)
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
                                    .and_then(|d| {
                                        d.value.trim_end_matches("px").parse::<f64>().ok()
                                    })
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
                                        if ui
                                            .add(egui::DragValue::new(&mut g.x).speed(1.0))
                                            .changed()
                                        {
                                            self.apply_geom(&sid, g);
                                        }
                                        ui.label("W");
                                        if ui
                                            .add(egui::DragValue::new(&mut g.w).speed(1.0))
                                            .changed()
                                        {
                                            self.apply_geom(&sid, g);
                                        }
                                        ui.label("Y");
                                        if ui
                                            .add(egui::DragValue::new(&mut g.y).speed(1.0))
                                            .changed()
                                        {
                                            self.apply_geom(&sid, g);
                                        }
                                        ui.label("H");
                                        if ui
                                            .add(egui::DragValue::new(&mut g.h).speed(1.0))
                                            .changed()
                                        {
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
                                                &vb_common::Rgba::new(r, gg, b, a)
                                                    .to_shortest_hex(),
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
                                        .add(
                                            egui::DragValue::new(&mut op)
                                                .speed(0.01)
                                                .range(0.0..=1.0),
                                        )
                                        .changed()
                                    {
                                        let v = format!("{}", (op * 100.0).round() / 100.0);
                                        self.exec(Command::SetStyle {
                                            sid: sid.clone(),
                                            new: set_style_prop(
                                                style_snapshot.clone(),
                                                "opacity",
                                                &v,
                                            ),
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

                                // --- 对齐(P3.8:复用命令派发,快捷键同源) ---
                                ui.separator();
                                ui.label("对齐");
                                ui.horizontal(|ui| {
                                    let btns: [(&str, &str); 6] = [
                                        ("align.left", "⇤"),
                                        ("align.hcenter", "↔"),
                                        ("align.right", "⇥"),
                                        ("align.top", "⤒"),
                                        ("align.vcenter", "↕"),
                                        ("align.bottom", "⤓"),
                                    ];
                                    for (id, icon) in btns {
                                        if ui
                                            .button(icon)
                                            .on_hover_text(
                                                shortcuts::command_label(id).unwrap_or(id),
                                            )
                                            .clicked()
                                        {
                                            self.run_command(id, false, false);
                                        }
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label("分布");
                                    if ui.button("↔ 等距").clicked() {
                                        self.run_command("object.distribute_h", false, false);
                                    }
                                    if ui.button("↕ 等距").clicked() {
                                        self.run_command("object.distribute_v", false, false);
                                    }
                                });

                                // --- v0.7 网页能力 ---
                                ui.separator();
                                ui.heading("网页");
                                let (cur_tag, attrs, style2) = {
                                    let n = self.doc.nodes.get(nid).unwrap();
                                    (n.tag.clone(), n.attrs.clone(), n.style.clone())
                                };
                                // 语义标签
                                const TAGS: [&str; 16] = [
                                    "div", "section", "header", "nav", "main", "footer", "article",
                                    "aside", "h1", "h2", "h3", "p", "span", "a", "button", "li",
                                ];
                                let mut tag_sel = cur_tag.clone();
                                egui::ComboBox::from_id_salt("tag_sel")
                                    .selected_text(format!("标签: {tag_sel}"))
                                    .show_ui(ui, |ui| {
                                        for t in TAGS {
                                            ui.selectable_value(&mut tag_sel, t.to_string(), t);
                                        }
                                    });
                                if tag_sel != cur_tag && tag_sel != "#text" {
                                    self.exec(Command::SetTag {
                                        sid: sid.clone(),
                                        new: tag_sel,
                                        old: None,
                                    });
                                }
                                // 链接与无障碍
                                let mut href = attrs.get("href").cloned().unwrap_or_default();
                                let mut aria = attrs.get("aria-label").cloned().unwrap_or_default();
                                ui.horizontal(|ui| {
                                    ui.label("链接");
                                    if ui
                                        .add_sized(
                                            [160.0, 18.0],
                                            egui::TextEdit::singleline(&mut href),
                                        )
                                        .lost_focus()
                                    {
                                        let mut merged = attrs.clone();
                                        if href.is_empty() {
                                            merged.remove("href");
                                        } else {
                                            merged.insert("href".into(), href.clone());
                                        }
                                        self.exec(Command::SetAttrs {
                                            sid: sid.clone(),
                                            new: merged.into_iter().collect(),
                                            old: None,
                                        });
                                    }
                                });
                                ui.horizontal(|ui| {
                                    ui.label("aria ");
                                    if ui
                                        .add_sized(
                                            [160.0, 18.0],
                                            egui::TextEdit::singleline(&mut aria),
                                        )
                                        .lost_focus()
                                    {
                                        let mut merged = attrs.clone();
                                        if aria.is_empty() {
                                            merged.remove("aria-label");
                                        } else {
                                            merged.insert("aria-label".into(), aria.clone());
                                        }
                                        self.exec(Command::SetAttrs {
                                            sid: sid.clone(),
                                            new: merged.into_iter().collect(),
                                            old: None,
                                        });
                                    }
                                });
                                // 自动布局(flex)
                                ui.collapsing("自动布局", |ui| {
                                    let get = |p: &str| {
                                        style2
                                            .iter()
                                            .find(|d| d.prop == p)
                                            .map(|d| d.value.clone())
                                            .unwrap_or_default()
                                    };
                                    let mut display = {
                                        let d = get("display");
                                        if d.is_empty() {
                                            "block".to_string()
                                        } else {
                                            d
                                        }
                                    };
                                    let mut gap = get("gap")
                                        .trim_end_matches("px")
                                        .parse::<f64>()
                                        .unwrap_or(0.0);
                                    let mut justify = {
                                        let j = get("justify-content");
                                        if j.is_empty() {
                                            "flex-start".to_string()
                                        } else {
                                            j
                                        }
                                    };
                                    let mut align = {
                                        let a = get("align-items");
                                        if a.is_empty() {
                                            "stretch".to_string()
                                        } else {
                                            a
                                        }
                                    };
                                    egui::ComboBox::from_id_salt("disp")
                                        .selected_text(format!("display: {display}"))
                                        .show_ui(ui, |ui| {
                                            for v in ["block", "flex", "inline-flex", "none"] {
                                                ui.selectable_value(&mut display, v.to_string(), v);
                                            }
                                        });
                                    ui.horizontal(|ui| {
                                        ui.label("间距");
                                        if ui
                                            .add(
                                                egui::DragValue::new(&mut gap)
                                                    .speed(1.0)
                                                    .range(0.0..=200.0),
                                            )
                                            .changed()
                                        {
                                            self.exec(Command::SetStyle {
                                                sid: sid.clone(),
                                                new: set_style_prop(
                                                    style2.clone(),
                                                    "gap",
                                                    &format!("{}px", gap as i64),
                                                ),
                                                old: None,
                                            });
                                        }
                                    });
                                    egui::ComboBox::from_id_salt("jc")
                                        .selected_text(format!("主轴: {justify}"))
                                        .show_ui(ui, |ui| {
                                            for v in [
                                                "flex-start",
                                                "center",
                                                "flex-end",
                                                "space-between",
                                                "space-around",
                                            ] {
                                                ui.selectable_value(&mut justify, v.to_string(), v);
                                            }
                                        });
                                    egui::ComboBox::from_id_salt("ai")
                                        .selected_text(format!("交叉轴: {align}"))
                                        .show_ui(ui, |ui| {
                                            for v in ["stretch", "center", "flex-start", "flex-end"]
                                            {
                                                ui.selectable_value(&mut align, v.to_string(), v);
                                            }
                                        });
                                    // display/gap 改动即写(display=flex 时自动补 justify/align)
                                    if display != get("display") {
                                        let mut st =
                                            set_style_prop(style2.clone(), "display", &display);
                                        if display == "flex" {
                                            st = set_style_prop(st, "justify-content", &justify);
                                            st = set_style_prop(st, "align-items", &align);
                                        }
                                        self.exec(Command::SetStyle {
                                            sid: sid.clone(),
                                            new: st,
                                            old: None,
                                        });
                                    }
                                });

                                ui.separator();
                            }
                        } else {
                            ui.colored_label(egui::Color32::GRAY, "未选中对象");
                            ui.label("V 点选 / 拖框选 · M 画矩形 · L 画椭圆");
                            ui.separator();
                        }
                    }
                    1 => {
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
                                    .filter_map(|&a| {
                                        self.doc.nodes.get(a).map(|n| n.geom.y + n.geom.h)
                                    })
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
                                    let selected =
                                        self.selection.last().map(|s| s == &sid).unwrap_or(false);
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
                                    if self.doc.artboards.len() > 1
                                        && ui.small_button("🗑").clicked()
                                    {
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

                        // --- 图层列表(全部画板;含层序调整) ---
                        ui.heading("图层");
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for &ab in self.doc.artboards.clone().iter() {
                                let Some(abn) = self.doc.nodes.get(ab) else {
                                    continue;
                                };
                                let ab_sid = abn.sid.as_str().to_string();
                                let ab_name = abn.name.clone();
                                let ab_sel = self
                                    .selection
                                    .last()
                                    .map(|s| s.as_str() == ab_sid)
                                    .unwrap_or(false);
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
                                        ui.label(icons::rich(row_kind, 12.0));
                                        if hidden {
                                            ui.label(icons::rich(Name::Hidden, 12.0));
                                        }
                                        if locked {
                                            ui.label(icons::rich(Name::Locked, 12.0));
                                        }
                                        if ui.selectable_label(selected, &name).clicked() {
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
                                        let eye_tip = if hidden {
                                            "显示(取消隐藏)"
                                        } else {
                                            "隐藏"
                                        };
                                        if icon_button(
                                            ui,
                                            if hidden { Name::Hidden } else { Name::Visible },
                                            eye_tip,
                                        )
                                        .clicked()
                                        {
                                            hidden = !hidden;
                                            self.exec(Command::SetFlags {
                                                sid: row_sid.clone(),
                                                hidden: Some(hidden),
                                                locked: None,
                                                old: None,
                                            });
                                        }
                                        let lock_tip = if locked { "解锁" } else { "锁定" };
                                        if icon_button(
                                            ui,
                                            if locked { Name::Locked } else { Name::Unlocked },
                                            lock_tip,
                                        )
                                        .clicked()
                                        {
                                            locked = !locked;
                                            self.exec(Command::SetFlags {
                                                sid: row_sid.clone(),
                                                hidden: None,
                                                locked: Some(locked),
                                                old: None,
                                            });
                                        }
                                        // 层序:↑ = 前移一层(列表自顶向下 = z 序从高到低)
                                        if icon_button(ui, Name::MoveUp, "前移一层").clicked() {
                                            self.reorder(&row_sid, 1);
                                        }
                                        if icon_button(ui, Name::MoveDown, "后移一层").clicked()
                                        {
                                            self.reorder(&row_sid, -1);
                                        }
                                    });
                                }
                                ui.separator();
                            }
                        });
                    }
                    _ => {
                        // --- 设计令牌(v0.7:CSS 变量,改一处全站生效) ---
                        ui.heading("设计令牌");
                        ui.horizontal(|ui| {
                            let mut add: Option<(String, String)> = None;
                            if ui.small_button("+ 令牌").clicked() {
                                add = Some((
                                    format!("brand-{}", self.doc.tokens.len() + 1),
                                    "#888888".into(), // vb-token-ok: 新令牌默认值(文档内容,非 UI 皮肤)
                                ));
                            }
                            if let Some((n, v)) = add {
                                self.exec(Command::SetToken {
                                    name: n,
                                    new: v,
                                    old: None,
                                });
                            }
                        });
                        let tokens = self.doc.tokens.clone();
                        for (i, (name, value)) in tokens.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let mut v = value.clone();
                                if vb_common::color::parse_color(value).is_some() {
                                    if let Some(c) = vb_common::color::parse_color(value) {
                                        let mut col =
                                            Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                                        if ui.color_edit_button_srgba(&mut col).changed() {
                                            let [r, g, b, a] = col.to_array();
                                            self.exec(Command::SetToken {
                                                name: name.clone(),
                                                new: vb_common::Rgba::new(r, g, b, a)
                                                    .to_shortest_hex(),
                                                old: None,
                                            });
                                        }
                                    }
                                }
                                let resp = ui
                                    .add_sized([70.0, 18.0], egui::Label::new(format!("--{name}")));
                                let _ = resp;
                                if ui
                                    .add_sized([110.0, 18.0], egui::TextEdit::singleline(&mut v))
                                    .lost_focus()
                                    && v != *value
                                {
                                    self.exec(Command::SetToken {
                                        name: name.clone(),
                                        new: v,
                                        old: None,
                                    });
                                }
                                if ui.small_button("🗑").clicked() {
                                    // 删除令牌 = SetToken 到空再移除(v0.1:直接移除,可撤销)
                                    self.exec(Command::SetToken {
                                        name: name.clone(),
                                        new: String::new(),
                                        old: None,
                                    });
                                }
                                let _ = i;
                            });
                        }
                    }
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

// ---------- 画布 ----------

impl VellumApp {
    fn canvas(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::canvas(ui.style()).fill(Tokens::get(self.theme_dark).bg_canvas))
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
                    draw_grid(&painter, rect, &self.camera, self.theme_dark);
                }

                // 渲染画布内容(GPU,画板背景会盖住网格)
                if self.outline_mode {
                    // 轮廓模式(Mod+Y):不画实体,只勾勒每个对象的绝对边界
                    self.draw_outline_mode(&painter, rect);
                } else {
                    self.render_canvas_gpu(frame, rect);
                    if let Some(tex_id) = self.tex_id() {
                        painter.image(
                            tex_id,
                            rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
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
        // P4.3 隔离模式:遮罩压暗非隔离内容,隔离子树顶层重绘保持全亮
        // (06 篇 §4.3「淡化 25%」;重绘与所属画板同一变换)
        if let Some(iso) = self.isolate_top() {
            if let Some(ab) = vb_tools::artboard_of(&self.doc, iso) {
                if let Some(abn) = self.doc.nodes.get(ab) {
                    let z = self.camera.zoom;
                    let tf = vello::kurbo::Affine::translate(vello::kurbo::Vec2::new(
                        self.camera.pan_x + abn.geom.x * z,
                        self.camera.pan_y + abn.geom.y * z,
                    )) * vello::kurbo::Affine::scale(z);
                    let c = Tokens::get(self.theme_dark).bg_canvas;
                    let scrim = vello::peniko::Color::from_rgba8(c.r(), c.g(), c.b(), 185);
                    let mut sc = vello::Scene::new();
                    sc.fill(
                        vello::peniko::Fill::NonZero,
                        vello::kurbo::Affine::IDENTITY,
                        scrim,
                        None,
                        &vello::kurbo::Rect::new(
                            -1.0e5,
                            -1.0e5,
                            abn.geom.w + 1.0e5,
                            abn.geom.h + 1.0e5,
                        ),
                    );
                    scene.append(&sc, Some(tf));
                    if let Ok(list) = vb_render::encode::encode_subtree(&self.doc, iso) {
                        let mut sub = vello::Scene::new();
                        vb_render::gpu::encode_scene(&mut sub, &list);
                        scene.append(&sub, Some(tf));
                    }
                }
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
        // 缩放工具单击:放大 / Alt+单击缩小(02 篇 §5.6)
        if response.clicked() && self.tool == Tool::Zoom {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let alt_click = ctx.input(|i| i.modifiers.alt);
                let f = if alt_click { 1.0 / 1.25 } else { 1.25 };
                self.camera.zoom_at(pl.x as f64, pl.y as f64, f);
                self.status = format!("缩放 {}%", (self.camera.zoom * 100.0) as i64);
            }
            return;
        }
        // 吸管单击:取色/取样式应用到选区(06 篇 §5.4;Alt = 全部样式)
        if response.clicked() && self.tool == Tool::Eyedropper {
            let alt = ctx.input(|i| i.modifiers.alt);
            self.eyedropper_pick(alt);
            return;
        }
        // 单击创建(Rect/Ellipse/Line/Text/Artboard 工具下单击 = 默认尺寸;处理单帧合并的合成拖拽)
        if response.clicked()
            && matches!(
                self.tool,
                Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Text | Tool::Artboard
            )
        {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                match self.tool {
                    Tool::Text => {
                        self.create_text_node((wx.round(), wy.round()), None);
                    }
                    Tool::Artboard => {
                        self.create_artboard(Geom {
                            x: wx.round(),
                            y: wy.round(),
                            w: 1440.0,
                            h: 900.0,
                        });
                    }
                    _ => {
                        self.create_shape(Geom {
                            x: (wx - 60.0).round(),
                            y: (wy - 40.0).round(),
                            w: 120.0,
                            h: 80.0,
                        });
                        self.status = "已创建对象(单击默认尺寸)".into();
                    }
                }
                return;
            }
        }
        // 钢笔工具单击(P4.5):落锚点;靠近起点时闭合
        if response.clicked() && self.tool == Tool::Pen {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                let (wx, wy) = (wx.round(), wy.round());
                if let Some(first) = self.pen_points.first() {
                    let (x0, y0) = first.anchor;
                    if (wx - x0).hypot(wy - y0) <= 6.0 / self.camera.zoom
                        && self.pen_points.len() >= 3
                    {
                        self.finish_pen(true);
                        return;
                    }
                }
                self.pen_points.push(PenPt::corner(wx, wy));
                // (request_repaint 由 egui 输入事件自动触发)
            }
            return;
        }
        // 钢笔拖拽(06 篇 §5.3 平滑点):按下锚点后拖出出手柄,入柄镜像
        if response.drag_started() && self.tool == Tool::Pen {
            if let Some(p0) = response.interact_pointer_pos() {
                let pl = p0 - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.pen_points.push(PenPt::corner(wx.round(), wy.round()));
            }
            return;
        }
        if response.dragged() && self.tool == Tool::Pen && !self.pen_points.is_empty() {
            if let Some(p0) = response.interact_pointer_pos() {
                let pl = p0 - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                let last = self.pen_points.last_mut().unwrap();
                last.h_out = Some((wx, wy));
            }
            return;
        }
        // 直接选择单击(A):命中矢量顶点 → 选中节点并记录待拖(P4.6)
        if response.clicked() && self.tool == Tool::DirectSelect {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.ds_vertex = self.find_vector_vertex(wx, wy, 8.0 / self.camera.zoom);
                if let Some((sid, _)) = &self.ds_vertex {
                    self.selection = vec![sid.clone()];
                }
            }
        }

        // 直接选择(A):拖拽锚点 → SetVector 命令入 undo 栈(P4.6 接线)。
        // 连续拖拽落在 500ms 合并窗口内 = 一条 undo 条目(与数值框同策略)
        if self.tool == Tool::DirectSelect && response.dragged() {
            if let (Some(p0), Some((sid, vi))) =
                (response.interact_pointer_pos(), self.ds_vertex.as_ref())
            {
                let p = p0 - rect.min;
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                let (sid, vi) = (sid.clone(), *vi);
                if let Some(nid) = self.doc.find_by_sid(&sid) {
                    if let Some(n) = self.doc.nodes.get(nid) {
                        if let NodeKind::Vector { path } = &n.kind {
                            // 顶点存节点本地坐标:新本地 = 光标世界 - 节点世界原点
                            if let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) {
                                let mut els: Vec<vb_common::geom::PathEl> =
                                    path.elements().to_vec();
                                let new_pt = vb_common::geom::Point::new(wx - bb.x0, wy - bb.y0);
                                els[vi] = match els[vi] {
                                    vb_common::geom::PathEl::MoveTo(_) => {
                                        vb_common::geom::PathEl::MoveTo(new_pt)
                                    }
                                    vb_common::geom::PathEl::LineTo(_) => {
                                        vb_common::geom::PathEl::LineTo(new_pt)
                                    }
                                    other => other,
                                };
                                let np = vb_common::geom::BezPath::from_vec(els);
                                self.exec(Command::SetVector {
                                    sid,
                                    new: np,
                                    old: None,
                                });
                            }
                        }
                    }
                }
            }
        }
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
                    self.camera.zoom_at(
                        pl.x as f64,
                        pl.y as f64,
                        (-(scroll.y as f64) / 400.0).exp(),
                    );
                }
            } else {
                self.camera.pan_y += scroll.y as f64;
                self.camera.pan_x += scroll.x as f64;
            }
        }

        let mods = ctx.input(|i| i.modifiers);
        let alt = mods.alt;
        let shift = mods.shift;
        let ctrl = mods.ctrl || mods.command;
        let _ = alt_down;

        // 本帧参考线清空(绘制在 overlay)
        self.smart_guides.clear();

        // 平移:中键 或 Space+左键 或 抓手工具
        let pan_wanted = self.space_down || self.tool == Tool::Hand;
        if pan_wanted {
            ctx.set_cursor_icon(vbcursor::PAN);
        } else {
            // 工具光标映射(P2.8 尾巴):绘图类十字线、缩放放大镜
            match self.tool {
                Tool::Rect | Tool::Ellipse | Tool::Line => {
                    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                Tool::Zoom => ctx.set_cursor_icon(egui::CursorIcon::ZoomIn),
                _ => {}
            }
        }

        // 手柄悬停光标(P2.8):非平移态下,指针落在选中对象手柄上给方向光标,
        // 旋转圈在角外侧(见下方 Drag::Rotate 命中区)。
        if !pan_wanted && self.tool == Tool::Select {
            if let Some(p) = response.hover_pos() {
                'outer: for sid in &self.selection {
                    let Some(nid) = self.doc.find_by_sid(sid) else {
                        continue;
                    };
                    let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
                        continue;
                    };
                    let (sx, sy) = self.camera.world_to_screen(bb.x0, bb.y0);
                    let (ex, ey) = self.camera.world_to_screen(bb.x1, bb.y1);
                    let r = Rect::from_min_max(
                        pos2(sx as f32 + rect.min.x, sy as f32 + rect.min.y),
                        pos2(ex as f32 + rect.min.x, ey as f32 + rect.min.y),
                    );
                    if let Some(h) = hit_handle(p, r) {
                        ctx.set_cursor_icon(vbcursor::for_handle(h));
                        break 'outer;
                    }
                }
            }
        }

        if response.dragged_by(PointerButton::Middle)
            || (pan_wanted && response.dragged_by(PointerButton::Primary))
        {
            ctx.set_cursor_icon(vbcursor::PANNING);
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

        // 双击:进入文本编辑(AI:双击文本进入编辑)/编组进入隔离
        if response.double_clicked() && self.tool == Tool::Select {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                if let Some(nid) = self.pick_at_world(wx, wy) {
                    let n = self.doc.nodes.get(nid).unwrap();
                    // P4.3 隔离模式:双击编组进入(06 篇 §4.3;嵌套逐层进栈)
                    if matches!(n.kind, NodeKind::Group) {
                        self.isolate_stack.push(nid);
                        self.selection.clear();
                        let crumbs: Vec<String> = self
                            .isolate_stack
                            .iter()
                            .filter_map(|id| self.doc.nodes.get(*id).map(|n| n.name.clone()))
                            .collect();
                        self.status = format!("隔离模式:{}(Esc 退出)", crumbs.join(" / "));
                        return;
                    }
                    if matches!(n.kind, NodeKind::Text { .. }) {
                        self.editing_text = Some(n.sid.as_str().to_string());
                        self.status = format!("编辑文本:{}(Ctrl+Enter 提交,Esc 取消)", n.name);
                    }
                }
            }
        }

        // 选中对象的屏幕 bbox(用于手柄/旋转命中)
        let sel_bbox_screen = self.selection.last().and_then(|sid| {
            let nid = self.doc.find_by_sid(sid)?;
            let bb = vb_tools::abs_bbox_world(&self.doc, nid)?;
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

            // --- 0) 标尺参考线抓取(未锁定时优先级最高):从标尺拖出新建,
            //        在参考线 ±3px 内按下则拖动既有参考线(02 篇 §5.2) ---
            if self.tool == Tool::Select && !self.guides_locked {
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                const STRIP: f32 = 20.0;
                let in_top = p.y <= STRIP;
                let in_left = p.x <= STRIP;
                let near_line = self.guides.iter().position(|&(h, pos)| {
                    if h {
                        let (_, sy) = self.camera.world_to_screen(0.0, pos);
                        (p.y - sy as f32).abs() <= 3.0
                    } else {
                        let (sx, _) = self.camera.world_to_screen(pos, 0.0);
                        (p.x - sx as f32).abs() <= 3.0
                    }
                });
                if in_top || in_left || near_line.is_some() {
                    let idx = match near_line {
                        Some(i) if self.guides[i].0 == in_top => i,
                        _ => {
                            self.guides.push((in_top, if in_top { wy } else { wx }));
                            self.guides.len() - 1
                        }
                    };
                    self.drag = Drag::Guide { idx };
                    return;
                }
            }

            // --- 1) 手柄/旋转命中(单选优先) ---
            if let (Some((bbox, sid)), true) = (sel_bbox_screen.clone(), self.tool == Tool::Select)
            {
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
                        start_pan: vec2(self.camera.pan_x as f32, self.camera.pan_y as f32),
                    };
                }
                Tool::Select => {
                    let hit = self
                        .pick_at_world(wx, wy)
                        .map(|nid| self.doc.nodes.get(nid).unwrap().sid.as_str().to_string());
                    if let Some(sid) = hit {
                        if !self.selection.contains(&sid) {
                            if shift {
                                self.selection.push(sid.clone());
                            } else {
                                self.selection = vec![sid.clone()];
                            }
                        }
                        // Alt = 复制并拖动(AI 招牌);走 Insert 命令入 undo 栈,
                        // 裸 clone_subtree 产生的克隆体永远撤销不掉
                        let drag_sid = if alt {
                            let nid = self.doc.find_by_sid(&sid).unwrap();
                            match self.doc.nodes.get(nid).unwrap().parent {
                                Some(parent_id) => {
                                    let fallback = vb_doc::model::NodeTree {
                                        node: self.doc.nodes.get(nid).unwrap().clone(),
                                        children: vec![],
                                    };
                                    let mut tree =
                                        vb_doc::model::NodeTree::from_document(&self.doc, nid)
                                            .unwrap_or(fallback);
                                    re_sid_tree(&mut tree, &mut self.doc);
                                    let new_sid = tree.node.sid.as_str().to_string();
                                    let parent_sid = self
                                        .doc
                                        .nodes
                                        .get(parent_id)
                                        .unwrap()
                                        .sid
                                        .as_str()
                                        .to_string();
                                    self.exec(Command::Insert {
                                        parent_sid,
                                        index: usize::MAX,
                                        tree,
                                    });
                                    self.selection = vec![new_sid.clone()];
                                    new_sid
                                }
                                None => sid.clone(),
                            }
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
                Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Text | Tool::Artboard => {
                    self.drag = Drag::Create { start: p, cur: p };
                }
                Tool::Zoom => {
                    self.drag = Drag::ZoomRegion { start: p, cur: p };
                }
                Tool::Pen => {
                    // 钢笔单击由 clicked() 处理;这里兜底防穿透
                }
                Tool::DirectSelect => {
                    // 直接选择:单击由 clicked() 处理(顶点命中)
                }
                Tool::Eyedropper => {
                    // 吸管:单击由 clicked() 处理(取色/取样式)
                }
            }
        }

        if response.dragged() {
            let Some(p0) = response.interact_pointer_pos() else {
                return;
            };
            let p = p0 - rect.min;

            // 参考线拖动:实时跟随光标(世界坐标)
            if let Drag::Guide { idx } = &self.drag {
                let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
                if let Some(g) = self.guides.get_mut(*idx) {
                    g.1 = if g.0 { wy } else { wx };
                }
                return;
            }

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
                    Some((
                        sid.clone(),
                        resize_geom(*start_geom, *handle, dx, dy, shift, alt),
                    ))
                }
                _ => None,
            };
            if let Some((sid, g)) = resize_update {
                self.exec(Command::SetGeom {
                    sid,
                    new: g,
                    old: None,
                });
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
                    style =
                        set_style_prop(style, "transform", &format!("rotate({}deg)", fmt_deg(deg)));
                    self.exec(Command::SetStyle {
                        sid,
                        new: style,
                        old: None,
                    });
                    if let Drag::Rotate { moved, .. } = &mut self.drag {
                        *moved = true;
                    }
                }
                return;
            }

            // 移动 + 智能参考线
            let drag_update: Option<(String, Geom)> = match &mut self.drag {
                Drag::Marquee { cur, .. }
                | Drag::Create { cur, .. }
                | Drag::ZoomRegion { cur, .. } => {
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
                    Some((
                        sid.clone(),
                        Geom {
                            x: nx,
                            y: ny,
                            w: start_geom.w,
                            h: start_geom.h,
                        },
                    ))
                }
                _ => None,
            };
            if let Some((sid, g)) = drag_update {
                // 智能参考线:对齐兄弟/画板(屏幕空间 6px 阈值);拖动中 Mod 临时禁用(P3.10)
                let mut g = g;
                if self.smart_guides_on && !ctrl {
                    if let Some(nid) = self.doc.find_by_sid(&sid) {
                        let parent = self.doc.nodes.get(nid).unwrap().parent;
                        // 移动对象所属画板直接沿父链上溯(坐标探测在多画板
                        // 且对象位于负坐标时会找错画板)
                        let ab = vb_tools::artboard_of(&self.doc, nid);
                        let (sx, sy, lines) =
                            self.smart_snap(nid, parent, ab, &g, 6.0 / self.camera.zoom);
                        g.x = sx;
                        g.y = sy;
                        self.smart_guides = lines;
                    }
                }
                self.exec(Command::SetGeom {
                    sid: sid.clone(),
                    new: g,
                    old: None,
                });
                if let Drag::MoveObj {
                    moved, start_geom, ..
                } = &mut self.drag
                {
                    *moved = true;
                    self.last_move_delta = Some((g.x - start_geom.x, g.y - start_geom.y));
                }
            }
        }

        if response.drag_stopped() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::Guide { idx } => {
                    // 松手在标尺条内/画布外 = 删除(AI 拖回标尺删参考线)
                    let inside = response.interact_pointer_pos().map(|pp| {
                        let pl = pp - rect.min;
                        pl.x > 20.0 && pl.y > 20.0 && rect.contains(pp)
                    });
                    if inside != Some(true) && idx < self.guides.len() {
                        self.guides.remove(idx);
                        self.status = "参考线已删除".into();
                    }
                }
                Drag::Marquee { start, cur } => {
                    // 相交即选中(AI);框选落在拖拽起点所在画板(此前硬编码
                    // artboards.first(),多画板文档在其它画板框选错乱)
                    let (x0, y0) = self
                        .camera
                        .screen_to_world(start.x.min(cur.x) as f64, start.y.min(cur.y) as f64);
                    let (x1, y1) = self
                        .camera
                        .screen_to_world(start.x.max(cur.x) as f64, start.y.max(cur.y) as f64);
                    let r = vb_common::geom::rect_xywh(x0, y0, x1 - x0, y1 - y0);
                    let (swx, swy) = self.camera.screen_to_world(start.x as f64, start.y as f64);
                    let ab = self
                        .artboard_at_world(swx, swy)
                        .or_else(|| self.doc.artboards.first().copied());
                    if let Some(ab) = ab {
                        let hits = match self.isolate_top() {
                            Some(iso) => {
                                let (ox, oy) = self.doc.artboard_origin(ab);
                                let lr = vb_common::geom::Rect::new(
                                    r.x0 - ox,
                                    r.y0 - oy,
                                    r.x1 - ox,
                                    r.y1 - oy,
                                );
                                vb_tools::marquee_select_root(&self.doc, iso, lr)
                            }
                            None => vb_tools::marquee_select(&self.doc, ab, r),
                        };
                        let mut sids: Vec<String> = hits
                            .into_iter()
                            .map(|id| self.doc.nodes.get(id).unwrap().sid.as_str().to_string())
                            .collect();
                        if shift {
                            for s in &self.selection {
                                if !sids.contains(s) {
                                    sids.push(s.clone());
                                }
                            }
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
                    match self.tool {
                        Tool::Text => {
                            let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                            // 拖框 = 区域文本;几乎没拖 = 点文本(与 clicked 互斥,兜底)
                            if g.w < 24.0 && g.h < 24.0 {
                                self.create_text_node((sx.round(), sy.round()), None);
                            } else {
                                self.create_text_node(
                                    (sx.round(), sy.round()),
                                    Some(Geom {
                                        x: g.x.round(),
                                        y: g.y.round(),
                                        w: g.w.round().max(120.0),
                                        h: g.h.round().max(40.0),
                                    }),
                                );
                            }
                        }
                        Tool::Artboard => {
                            let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                            self.create_artboard(Geom {
                                x: g.x.round(),
                                y: g.y.round(),
                                w: g.w.round().max(80.0),
                                h: g.h.round().max(80.0),
                            });
                        }
                        _ => {
                            let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                            self.create_shape(g);
                        }
                    }
                }
                Drag::ZoomRegion { start, cur } => {
                    let (x0, y0) = self
                        .camera
                        .screen_to_world(start.x.min(cur.x) as f64, start.y.min(cur.y) as f64);
                    let (x1, y1) = self
                        .camera
                        .screen_to_world(start.x.max(cur.x) as f64, start.y.max(cur.y) as f64);
                    let rw = (x1 - x0).max(1.0);
                    let rh = (y1 - y0).max(1.0);
                    if let Some(r) = self.canvas_rect {
                        if rw > 1.0 && rh > 1.0 {
                            let zoom = ((r.width() as f64) / rw)
                                .min((r.height() as f64) / rh)
                                .clamp(0.01, 64.0);
                            self.camera.zoom = zoom;
                            // pan 使区域中心落在画布中心(screen 为画布本地坐标)
                            let ccx = (r.center().x - rect.min.x) as f64;
                            let ccy = (r.center().y - rect.min.y) as f64;
                            self.camera.pan_x = ccx - (x0 + rw / 2.0) * zoom;
                            self.camera.pan_y = ccy - (y0 + rh / 2.0) * zoom;
                            self.status = format!("缩放到区域 {}%", (zoom * 100.0) as i64);
                        }
                    }
                }
                Drag::MoveObj {
                    sid,
                    moved,
                    start_geom,
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
                                                       // 兄弟完整 bbox(P4.1 间距/尺寸类用)
        let mut sib_rects: Vec<(f64, f64, f64, f64)> = Vec::new(); // (x0,y0,x1,y1)

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
                    // 隐藏/锁定对象不可见不可选,也不得吸走拖动(与拾取口径一致)
                    if self
                        .doc
                        .nodes
                        .get(c)
                        .map(|n| n.hidden || n.locked)
                        .unwrap_or(true)
                    {
                        continue;
                    }
                    if let Some(bb) = vb_tools::abs_bbox(&self.doc, c) {
                        let (bx0, by0, bx1, by1) = (bb.x0, bb.y0, bb.x1, bb.y1);
                        sib_rects.push((bx0, by0, bx1, by1));
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
        let mut best_line_x: Option<[f64; 4]> = None;
        for e in mx {
            for (cand, ly0, ly1) in &xs {
                let d = (e - cand).abs();
                if d <= tol && best_x.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best_x = Some((d, *cand));
                    // 只保留当前最优候选的参考线(此前每个容差内候选都画一条)
                    best_line_x = Some([*cand, *ly0 - 12.0, *cand, *ly1 + 12.0]);
                }
            }
        }
        if let Some(l) = best_line_x {
            lines.push(l);
        }
        let mut best_y: Option<(f64, f64)> = None;
        let mut best_line_y: Option<[f64; 4]> = None;
        for e in my {
            for (cand, lx0, lx1) in &ys {
                let d = (e - cand).abs();
                if d <= tol && best_y.map(|(bd, _)| d < bd).unwrap_or(true) {
                    best_y = Some((d, *cand));
                    best_line_y = Some([*lx0 - 12.0, *cand, *lx1 + 12.0, *cand]);
                }
            }
        }
        if let Some(l) = best_line_y {
            lines.push(l);
        }
        let nx = best_x.map(|(_, c)| {
            // 对齐的是哪条边?吸附到候选后保持原相对关系:取移动后最接近候选的那条边
            let cur = [g.x, g.x + g.w / 2.0, g.x + g.w]
                .iter()
                .copied()
                .min_by(|a, b| {
                    (*a - c)
                        .abs()
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
                    (*a - c)
                        .abs()
                        .partial_cmp(&(*b - c).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(c);
            g.y + (c - cur)
        });

        let mut nx = nx.unwrap_or(g.x);
        let mut ny = ny.unwrap_or(g.y);

        // ── P4.1 间距类:移动边与某兄弟形成"与既有兄弟对间距相等"的布局时吸附。
        // 仅在坐标轴对齐类未命中时尝试(对齐优先)。
        if best_x.is_none() && sib_rects.len() >= 2 {
            let mut sibs: Vec<(f64, f64, f64, f64)> = sib_rects.clone();
            sibs.sort_by(|a, b| a.2.total_cmp(&b.2));
            let mut gaps: Vec<f64> = Vec::new();
            for w in sibs.windows(2) {
                let gp = w[1].0 - w[0].2;
                if gp > 0.0 {
                    gaps.push(gp);
                }
            }
            let mut best: Option<(f64, f64, f64, f64)> = None; // (delta, cand_x, y0, y1)
            for (ax0, ay0, ax1, ay1) in &sib_rects {
                for gp in &gaps {
                    // 放在兄弟右侧:移动盒左缘 = 兄弟右缘 + gp
                    let cand = ax1 + gp;
                    let d = (g.x - cand).abs();
                    if d <= tol && best.as_ref().map(|(bd, ..)| d < *bd).unwrap_or(true) {
                        best = Some((d, cand, *ay0, *ay1));
                    }
                    // 放在兄弟左侧:移动盒左缘 = 兄弟左缘 - gp - 移动盒宽
                    let cand2 = ax0 - gp - g.w;
                    let d2 = (g.x - cand2).abs();
                    if d2 <= tol && best.as_ref().map(|(bd, ..)| d2 < *bd).unwrap_or(true) {
                        best = Some((d2, cand2, *ay0, *ay1));
                    }
                }
            }
            if let Some((_, cand, ly0, ly1)) = best {
                nx = cand;
                // 间距参考线:横跨两盒中点的水平测量线
                let mid_y = ly0 + (ly1 - ly0) / 2.0;
                lines.push([cand, mid_y, cand + g.w, mid_y]);
            }
        }
        if best_y.is_none() && sib_rects.len() >= 2 {
            let mut vrects: Vec<(f64, f64, f64, f64)> = sib_rects.clone();
            vrects.sort_by(|a, b| a.3.total_cmp(&b.3));
            let mut gaps: Vec<f64> = Vec::new();
            for w in vrects.windows(2) {
                let gp = w[1].1 - w[0].3;
                if gp > 0.0 {
                    gaps.push(gp);
                }
            }
            let mut best: Option<(f64, f64, f64, f64)> = None;
            for (bx0, by0, bx1, by1) in &vrects {
                for gp in &gaps {
                    let cand = *by1 + gp;
                    let d = (g.y - cand).abs();
                    if d <= tol && best.as_ref().map(|(bd, ..)| d < *bd).unwrap_or(true) {
                        best = Some((d, cand, *bx0, *bx1));
                    }
                    let cand2 = by0 - gp - g.h;
                    let d2 = (g.y - cand2).abs();
                    if d2 <= tol && best.as_ref().map(|(bd, ..)| d2 < *bd).unwrap_or(true) {
                        best = Some((d2, cand2, *bx0, *bx1));
                    }
                }
            }
            if let Some((_, cand, lx0, lx1)) = best {
                ny = cand;
                let mid_x = lx0 + (lx1 - lx0) / 2.0;
                lines.push([mid_x, cand, mid_x, cand + g.h]);
            }
        }

        // ── P4.1 尺寸相等类:宽(或高)与某兄弟一致时轻微吸附(仅拖动,不改尺寸) ──
        // 移动时若宽恰等于某兄弟宽,沿该兄弟左缘对齐提示;此处以参考线表达。
        for (bx0, by0, bx1, by1) in &sib_rects {
            let dw = (*bx1 - *bx0 - g.w).abs();
            let dh = (*by1 - *by0 - g.h).abs();
            if dw <= tol * 0.5 {
                lines.push([*bx0, *by0 - 8.0, *bx0, *by1 + 8.0]);
            }
            if dh <= tol * 0.5 {
                lines.push([*bx0 - 8.0, *by0, *bx1 + 8.0, *by0]);
            }
        }

        (nx, ny, lines)
    }

    fn draw_artboards(&self, painter: &egui::Painter, origin: egui::Vec2) {
        let t = Tokens::get(self.theme_dark);
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
                Stroke::new(1.0, t.border),
                egui::StrokeKind::Outside,
            );
            painter.text(
                pos2(r.left(), r.top() - 16.0),
                Align2::LEFT_BOTTOM,
                &n.name,
                FontId::proportional(11.0),
                t.text_3,
            );
        }
    }

    /// 轮廓模式(线框,`Mod+Y`):不画填充,只勾勒每个对象的绝对边界。
    ///
    /// AI 的轮廓模式用于查看结构关系与重叠顺序;这里是等价的最小实现
    /// (路径级轮廓待 P4 钢笔/矢量落地后替换)。
    fn draw_outline_mode(&self, painter: &egui::Painter, viewport: Rect) {
        let t = Tokens::get(self.theme_dark);
        let origin = viewport.min.to_vec2();
        let ab_stroke = Stroke::new(1.0, t.border_strong);
        let node_stroke = Stroke::new(1.0, t.border);
        let mut ids = Vec::new();
        for &ab in &self.doc.artboards {
            // 画板本身
            if let Some(an) = self.doc.nodes.get(ab) {
                let (x0, y0) = self.camera.world_to_screen(an.geom.x, an.geom.y);
                let (x1, y1) = self
                    .camera
                    .world_to_screen(an.geom.x + an.geom.w, an.geom.y + an.geom.h);
                painter.rect_stroke(
                    Rect::from_min_max(
                        pos2(x0 as f32 + origin.x, y0 as f32 + origin.y),
                        pos2(x1 as f32 + origin.x, y1 as f32 + origin.y),
                    ),
                    0.0,
                    ab_stroke,
                    egui::StrokeKind::Middle,
                );
            }
            self.doc.subtree(ab, &mut ids);
        }
        for id in ids {
            let Some(n) = self.doc.nodes.get(id) else {
                continue;
            };
            if n.hidden || n.tag == "#text" || matches!(n.kind, NodeKind::Artboard) {
                continue;
            }
            let Some(bb) = vb_tools::abs_bbox_world(&self.doc, id) else {
                continue;
            };
            let (sx, sy) = self.camera.world_to_screen(bb.x0, bb.y0);
            let (ex, ey) = self.camera.world_to_screen(bb.x1, bb.y1);
            let r = Rect::from_min_max(
                pos2(sx as f32 + origin.x, sy as f32 + origin.y),
                pos2(ex as f32 + origin.x, ey as f32 + origin.y),
            );
            if !r.intersects(viewport) {
                continue;
            }
            painter.rect_stroke(r, 0.0, node_stroke, egui::StrokeKind::Middle);
        }
    }

    fn draw_overlays(&self, painter: &egui::Painter, viewport: Rect) {
        let t = Tokens::get(self.theme_dark);
        let origin = viewport.min.to_vec2();

        // P4.2 标尺参考线(青色;从选区生成/从标尺拖出,世界坐标)
        if self.guides_visible {
            for &(h, pos) in &self.guides {
                let seg = if h {
                    let (_, sy) = self.camera.world_to_screen(0.0, pos);
                    [
                        pos2(viewport.left(), sy as f32 + origin.y),
                        pos2(viewport.right(), sy as f32 + origin.y),
                    ]
                } else {
                    let (sx, _) = self.camera.world_to_screen(pos, 0.0);
                    [
                        pos2(sx as f32 + origin.x, viewport.top()),
                        pos2(sx as f32 + origin.x, viewport.bottom()),
                    ]
                };
                painter.line_segment(seg, Stroke::new(1.0, semantic::GUIDE_RULER));
            }
        }

        // P4.5 钢笔预览:锚点连线 + 橡皮筋 + 平滑手柄(出柄实线/入柄镜像虚线)
        if self.tool == Tool::Pen && !self.pen_points.is_empty() {
            let to_screen = |(x, y): (f64, f64)| {
                let (sx, sy) = self.camera.world_to_screen(x, y);
                pos2(sx as f32 + origin.x, sy as f32 + origin.y)
            };
            let stroke = Stroke::new(1.2, semantic::SELECT_BOX);
            let mut prev = to_screen(self.pen_points[0].anchor);
            for pt in self.pen_points.iter().skip(1) {
                let cur = to_screen(pt.anchor);
                painter.line_segment([prev, cur], stroke);
                prev = cur;
            }
            // 橡皮筋:最后锚点 → 光标
            let last = self.pen_points.last().unwrap();
            let cursor = to_screen(self.cursor_world);
            painter.line_segment(
                [to_screen(last.anchor), cursor],
                Stroke::new(0.8, semantic::HOVER_BOX),
            );
            // 锚点方块 + 手柄
            for pt in &self.pen_points {
                let a = to_screen(pt.anchor);
                painter.rect_filled(
                    Rect::from_center_size(a, vec2(6.0, 6.0)),
                    1.0,
                    semantic::SELECT_BOX,
                );
                if let Some((hx, hy)) = pt.h_out {
                    let h = to_screen((hx, hy));
                    painter.line_segment([a, h], Stroke::new(0.8, semantic::GUIDE_SMART_DARK));
                    painter.circle_filled(h, 2.5, semantic::GUIDE_SMART_DARK);
                    if let Some((ix, iy)) = pt.h_in() {
                        let i2 = to_screen((ix, iy));
                        painter.line_segment([a, i2], Stroke::new(0.8, semantic::GUIDE_SMART_DARK));
                        painter.circle_filled(i2, 2.5, semantic::GUIDE_SMART_DARK);
                    }
                }
            }
            // 靠近起点 ≥3 锚点:高亮提示可闭合
            if self.pen_points.len() >= 3 {
                let (x0, y0) = self.pen_points[0].anchor;
                if (self.cursor_world.0 - x0).hypot(self.cursor_world.1 - y0)
                    <= 8.0 / self.camera.zoom
                {
                    painter.circle_filled(to_screen((x0, y0)), 5.0, semantic::SELECT_BOX);
                }
            }
        }

        // 智能参考线(品红,与 AI 同色)
        for l in &self.smart_guides {
            let (x0, y0) = self.camera.world_to_screen(l[0], l[1]);
            let (x1, y1) = self.camera.world_to_screen(l[2], l[3]);
            painter.line_segment(
                [
                    pos2(x0 as f32 + origin.x, y0 as f32 + origin.y),
                    pos2(x1 as f32 + origin.x, y1 as f32 + origin.y),
                ],
                // 智能参考线品红是**语义色**(AI 品红,02/03 篇钉死),深/浅主题共用同一个值;
                // vb-token-ok:不参与令牌化,P2 只把它挪进 vb_ui::theme 的 const
                Stroke::new(1.0, Color32::from_rgb(0xff, 0x00, 0xff)),
            );
        }
        if std::env::var("VB_NO_OVERLAY").is_ok() {
            return;
        }
        // 数值浮层(P3.7,14 篇 §4.4):移动 / 缩放 / 旋转时跟随光标显示实时数值
        let drag_label: Option<String> = match &self.drag {
            Drag::MoveObj {
                sid, start_geom, ..
            } => {
                let n = self
                    .doc
                    .find_by_sid(sid)
                    .and_then(|id| self.doc.nodes.get(id));
                n.map(|n| {
                    format!(
                        "X {}\nY {}\nΔX +{}\nΔY +{}",
                        vb_common::units::fmt_num(n.geom.x),
                        vb_common::units::fmt_num(n.geom.y),
                        vb_common::units::fmt_num(n.geom.x - start_geom.x),
                        vb_common::units::fmt_num(n.geom.y - start_geom.y),
                    )
                })
            }
            Drag::Resize { sid, .. } => self.doc.find_by_sid(sid).and_then(|id| {
                self.doc.nodes.get(id).map(|n| {
                    format!(
                        "W {}\nH {}",
                        vb_common::units::fmt_num(n.geom.w),
                        vb_common::units::fmt_num(n.geom.h)
                    )
                })
            }),
            Drag::Rotate { sid, .. } => self.doc.find_by_sid(sid).and_then(|id| {
                self.doc
                    .nodes
                    .get(id)
                    .and_then(|n| n.style_get("transform"))
                    .and_then(vb_render::encode::parse_rotate_deg)
                    .map(|d| format!("旋转 {}°", vb_common::units::fmt_num(d)))
            }),
            // 创建/缩放区域:拖拽中实时显示目标尺寸(P3.7)
            Drag::Create { start, cur } | Drag::ZoomRegion { start, cur } => {
                let w = ((cur.x - start.x).abs() as f64 / self.camera.zoom).round();
                let h = ((cur.y - start.y).abs() as f64 / self.camera.zoom).round();
                Some(format!("{} × {}", w, h))
            }
            _ => None,
        };
        if let Some(label) = drag_label {
            let (cx, cy) = self.cursor_world;
            let (sx, sy) = self.camera.world_to_screen(cx, cy);
            let pos = pos2(sx as f32 + origin.x + 16.0, sy as f32 + origin.y + 16.0);
            let bg = t.bg_raised;
            let fg = t.text;
            painter.rect_filled(
                Rect::from_min_size(
                    pos,
                    egui::vec2(96.0, 16.0 * label.lines().count() as f32 + 10.0),
                ),
                4.0,
                bg,
            );
            painter.text(
                pos2(pos.x + 8.0, pos.y + 5.0),
                Align2::LEFT_TOP,
                label,
                FontId::monospace(11.0),
                fg,
            );
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
            let abs = vb_tools::abs_bbox_world(&self.doc, id);
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
                        // HTML 规范的默认文字色(文档内容,非 UI 皮肤)
                        .unwrap_or(Color32::from_rgb(0x20, 0x20, 0x20)); // vb-token-ok: 文档内容默认色,非 UI 皮肤
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
                    painter.rect_filled(r, 4.0, semantic::frozen_fill(self.theme_dark));
                    painter.text(
                        pos2(r.left() + 6.0, r.top() + 4.0),
                        Align2::LEFT_TOP,
                        "冻结块",
                        FontId::proportional(11.0),
                        t.text_3,
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
            let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
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
                Stroke::new(1.0, semantic::SELECT_BOX),
                egui::StrokeKind::Outside,
            );
            for hx in [r.left(), r.center().x, r.right()] {
                for hy in [r.top(), r.center().y, r.bottom()] {
                    if (hx == r.center().x) == (hy == r.center().y) {
                        painter.rect_filled(
                            Rect::from_center_size(pos2(hx, hy), vec2(6.0, 6.0)),
                            1.0,
                            semantic::SELECT_BOX,
                        );
                    }
                }
            }
        }

        // 直接选择(A):矢量锚点方块(拖拽中的锚点高亮)
        if self.tool == Tool::DirectSelect {
            if let Some(sid) = self.selection.last() {
                for (i, vx, vy) in self.vector_vertices(sid) {
                    let (sx, sy) = self.camera.world_to_screen(vx, vy);
                    let c = pos2(sx as f32 + origin.x, sy as f32 + origin.y);
                    let active = self
                        .ds_vertex
                        .as_ref()
                        .map(|(s, vi)| s == sid && *vi == i)
                        .unwrap_or(false);
                    let fill = if active {
                        semantic::SELECT_BOX
                    } else {
                        t.bg_panel
                    };
                    painter.rect_filled(Rect::from_center_size(c, vec2(7.0, 7.0)), 1.0, fill);
                    painter.rect_stroke(
                        Rect::from_center_size(c, vec2(7.0, 7.0)),
                        1.0,
                        Stroke::new(1.0, semantic::SELECT_BOX),
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }

        // 框选/创建预览
        match &self.drag {
            Drag::Marquee { start, cur } => {
                let r = Rect::from_two_pos(pos2(start.x, start.y), pos2(cur.x, cur.y));
                painter.rect_filled(r, 0.0, semantic::MARQUEE_FILL);
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, semantic::SELECT_BOX),
                    egui::StrokeKind::Middle,
                );
            }
            Drag::Create { start, cur } => {
                let r = Rect::from_two_pos(pos2(start.x, start.y), pos2(cur.x, cur.y));
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, t.border_strong),
                    egui::StrokeKind::Middle,
                );
            }
            _ => {}
        }

        // P4.2 标尺(上/左,20px;刻度与网格同一世界节奏,最后画盖在内容上)
        if self.rulers_on {
            const STRIP: f32 = 20.0;
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(viewport.left(), viewport.top()),
                    pos2(viewport.right(), viewport.top() + STRIP),
                ),
                0.0,
                t.bg_panel,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(viewport.left(), viewport.top()),
                    pos2(viewport.left() + STRIP, viewport.bottom()),
                ),
                0.0,
                t.bg_panel,
            );
            let mut level = 64.0f64;
            while level * self.camera.zoom < 12.0 {
                level *= 4.0;
            }
            let label_step = level * 2.0;
            let tick = Stroke::new(1.0, t.border);
            let wx_left = (0.0 - self.camera.pan_x) / self.camera.zoom;
            let wx_right = (viewport.width() as f64 - self.camera.pan_x) / self.camera.zoom;
            let k0 = (wx_left / level).floor() as i64;
            let k1 = (wx_right / level).ceil() as i64;
            for k in k0..=k1 {
                let wx = k as f64 * level;
                let sx = (wx * self.camera.zoom + self.camera.pan_x) as f32 + origin.x;
                let is_label = (wx / label_step).fract().abs() < 1e-6;
                let len = if is_label { 10.0 } else { 5.0 };
                painter.line_segment(
                    [
                        pos2(sx, viewport.top() + STRIP - len),
                        pos2(sx, viewport.top() + STRIP),
                    ],
                    tick,
                );
                if is_label {
                    painter.text(
                        pos2(sx + 2.0, viewport.top()),
                        Align2::LEFT_TOP,
                        format!("{}", wx as i64),
                        FontId::monospace(9.0),
                        t.text_2,
                    );
                }
            }
            let wy_top = (0.0 - self.camera.pan_y) / self.camera.zoom;
            let wy_bottom = (viewport.height() as f64 - self.camera.pan_y) / self.camera.zoom;
            let j0 = (wy_top / level).floor() as i64;
            let j1 = (wy_bottom / level).ceil() as i64;
            for j in j0..=j1 {
                let wy = j as f64 * level;
                let sy = (wy * self.camera.zoom + self.camera.pan_y) as f32 + origin.y;
                let is_label = (wy / label_step).fract().abs() < 1e-6;
                let len = if is_label { 10.0 } else { 5.0 };
                painter.line_segment(
                    [
                        pos2(viewport.left() + STRIP - len, sy),
                        pos2(viewport.left() + STRIP, sy),
                    ],
                    tick,
                );
                if is_label {
                    painter.text(
                        pos2(viewport.left() + 1.0, sy + 1.0),
                        Align2::LEFT_TOP,
                        format!("{}", wy as i64),
                        FontId::monospace(9.0),
                        t.text_2,
                    );
                }
            }
        }
    }
}

// ---------- 辅助 ----------

/// 菜单项按钮:标签 + 右侧键位文本(键位一律查 `shortcuts` 注册表)。
fn menu_item_button(
    ui: &mut egui::Ui,
    item: &shortcuts::MenuItem,
    enabled: bool,
) -> egui::Response {
    menu_item_button_with(ui, item, "", enabled)
}

/// 同上,`extra` 为附在标签后的补充文本(如"撤销"后面的会撤销什么)。
fn menu_item_button_with(
    ui: &mut egui::Ui,
    item: &shortcuts::MenuItem,
    extra: &str,
    enabled: bool,
) -> egui::Response {
    let label = if extra.is_empty() {
        item.label.to_string()
    } else {
        format!("{} {}", item.label, extra)
    };
    let btn = match shortcuts::key_text_for(item.id) {
        Some(k) => egui::Button::new(label).shortcut_text(k),
        None => egui::Button::new(label),
    };
    ui.add_enabled(enabled, btn)
}

fn kind_icon(kind: &NodeKind) -> Name {
    match kind {
        NodeKind::Artboard => Name::KindArtboard,
        NodeKind::Layer => Name::KindLayer,
        NodeKind::Group => Name::KindGroup,
        NodeKind::Box => Name::KindBox,
        NodeKind::Text { .. } => Name::KindText,
        NodeKind::Image { .. } => Name::KindImage,
        NodeKind::Vector { .. } => Name::KindVector,
        NodeKind::Slice => Name::KindSlice,
        NodeKind::Frozen { .. } => Name::KindFrozen,
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

fn draw_grid(painter: &egui::Painter, rect: Rect, cam: &Camera, dark: bool) {
    // 世界锚定:网格线 = 世界 k*level,屏幕位 = k*level*zoom + pan。
    // 此前按屏幕取整导致平移时网格纹丝不动、缩放时相对世界跳动。
    let mut level = 64.0f64;
    while level * cam.zoom < 16.0 {
        level *= 4.0;
    }
    let step = (level * cam.zoom) as f32;
    if step < 6.0 {
        return;
    }
    let color = semantic::guide_grid(dark);
    let k0 = ((rect.left() as f64 - cam.pan_x) / (level * cam.zoom)).floor() as i64;
    let k1 = ((rect.right() as f64 - cam.pan_x) / (level * cam.zoom)).ceil() as i64;
    for k in k0..=k1 {
        let x = (k as f64 * level * cam.zoom + cam.pan_x) as f32;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(0.5, color),
        );
    }
    let j0 = ((rect.top() as f64 - cam.pan_y) / (level * cam.zoom)).floor() as i64;
    let j1 = ((rect.bottom() as f64 - cam.pan_y) / (level * cam.zoom)).ceil() as i64;
    for j in j0..=j1 {
        let y = (j as f64 * level * cam.zoom + cam.pan_y) as f32;
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(0.5, color),
        );
    }
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
fn resize_geom(g0: Geom, handle: u8, dx: f64, dy: f64, shift: bool, alt: bool) -> Geom {
    let (mut x0, mut y0) = (g0.x, g0.y);
    let (mut x1, mut y1) = (g0.x + g0.w, g0.y + g0.h);
    let west = matches!(handle, 6 | 7 | 0);
    let east = matches!(handle, 2..=4);
    let north = matches!(handle, 0..=2);
    let south = matches!(handle, 4..=6);
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
            x0 = if matches!(handle, 6 | 7 | 0) {
                ax - w
            } else {
                ax
            };
            y0 = if matches!(handle, 0..=2) { ay - h } else { ay };
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
    rest[..end]
        .trim()
        .trim_end_matches("deg")
        .trim()
        .parse()
        .ok()
}

/// 角度输出格式化(去尾 0)。
fn fmt_deg(deg: f64) -> String {
    vb_common::units::fmt_num((deg * 10.0).round() / 10.0)
}

/// 启动项目目录文件监听(v0.6:Agent/外部编辑改 HTML → 画布热重载)。
fn start_watcher(project: Option<&std::path::Path>) -> Option<std::sync::mpsc::Receiver<()>> {
    use notify::Watcher;
    let dir = project?;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if res.is_ok() {
                // 去抖由主循环做(200ms 窗口)
                let _ = tx.send(());
            }
        })
        .ok()?;
    watcher
        .watch(dir, notify::RecursiveMode::NonRecursive)
        .ok()?;
    std::mem::forget(watcher); // v0.1:与 App 同生命周期
    Some(rx)
}

impl VellumApp {
    /// 在指定几何处创建 Box 形状(矩形/椭圆由当前工具决定),可撤销。
    fn create_shape(&mut self, mut g: Geom) {
        // 直线工具:创建 2px 高的细长色条(HTML 中即一条水平线;斜线待 P4 矢量路径)
        let is_line = self.tool == Tool::Line;
        if is_line {
            g.h = 2.0;
        }
        let sid = self.doc.alloc_sid();
        let name = if is_line { "直线" } else { "矩形" };
        let mut n = vb_doc::model::Node::new(
            NodeKind::Box,
            format!("{} {}", name, sid.as_str()),
            sid.clone(),
        );
        n.geom = g;
        if self.tool == Tool::Ellipse {
            n.style.push(vb_css::Decl {
                prop: "border-radius".into(),
                value: "50%".into(),
                important: false,
            });
        }
        if is_line {
            n.style.push(vb_css::Decl {
                prop: "background-color".into(),
                value: "#1a1a1a".into(), // vb-token-ok: 直线是文档内容
                important: false,
            });
        } else {
            n.style.push(vb_css::Decl {
                prop: "background-color".into(),
                value: "#d4d4d4".into(), // vb-token-ok: 新建形状默认填充(文档内容,非 UI 皮肤)
                important: false,
            });
            n.style.push(vb_css::Decl {
                prop: "border".into(),
                value: "1px solid #1a1a1a".into(),
                important: false,
            });
        }
        let parent = self.insert_target(g.x, g.y);
        let (lx, ly) = self.world_to_parent_local(parent, g.x, g.y);
        n.geom.x = lx;
        n.geom.y = ly;
        let parent_sid = self.doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        let plen = self.doc.nodes.get(parent).unwrap().children.len();
        let tree = vb_doc::model::NodeTree {
            node: n,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid,
            index: plen,
            tree,
        });
        self.selection = vec![sid.as_str().to_string()];
        self.status = "已创建对象".into();
    }

    /// 文字工具:点文本(area=None)/ 区域文本(拖框)。
    /// 创建后立即进入编辑(AI 行为)。
    fn create_text_node(&mut self, point: (f64, f64), area: Option<Geom>) {
        let parent = self.insert_target(point.0, point.1);
        let (lx, ly) = self.world_to_parent_local(parent, point.0, point.1);
        let sid = self.doc.alloc_sid();
        let (mode, g) = match area {
            Some(mut a) => {
                a.x = lx;
                a.y = ly;
                (vb_doc::model::TextMode::Area, a)
            }
            None => (
                vb_doc::model::TextMode::Point,
                Geom {
                    x: lx,
                    y: ly,
                    w: 200.0,
                    h: 36.0,
                },
            ),
        };
        let mut n = vb_doc::model::Node::new(
            NodeKind::Text {
                text: "双击编辑文本".into(),
                mode,
            },
            format!("文本 {}", sid.as_str()),
            sid.clone(),
        );
        n.tag = "p".into();
        n.geom = g;
        n.style.push(vb_css::Decl {
            prop: "font-size".into(),
            value: "24px".into(),
            important: false,
        });
        n.style.push(vb_css::Decl {
            prop: "color".into(),
            // vb-token-ok: 新建文本默认字色(文档内容,非 UI 皮肤)
            value: "#1a1a1a".into(),
            important: false,
        });
        let parent_sid = self.doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        let plen = self.doc.nodes.get(parent).unwrap().children.len();
        let tree = vb_doc::model::NodeTree {
            node: n,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid,
            index: plen,
            tree,
        });
        self.selection = vec![sid.as_str().to_string()];
        self.editing_text = Some(sid.as_str().to_string());
        self.status = if area.is_some() {
            "已创建区域文本(拖框宽度即换行宽度)".into()
        } else {
            "已创建点文本(输入内容,Ctrl+Enter 提交)".into()
        };
    }

    /// 画板工具:在世界坐标处新建画板(画板 geom 即世界坐标)。
    fn create_artboard(&mut self, g: Geom) {
        let count = self.doc.artboards.len();
        let sid = self.doc.alloc_sid();
        let mut n = vb_doc::model::Node::new(
            NodeKind::Artboard,
            format!("画板 {}", count + 1),
            sid.clone(),
        );
        n.geom = g;
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
        self.selection = vec![sid.as_str().to_string()];
        self.status = format!("已新建画板 {}({}×{})", count + 1, g.w as i64, g.h as i64);
    }

    /// 吸管:命中对象取填充色应用到选区;Alt = 吸取全部样式替换。
    fn eyedropper_pick(&mut self, alt: bool) {
        let (wx, wy) = self.cursor_world;
        let Some(nid) = self.pick_at_world(wx, wy) else {
            self.status = "吸管:未命中对象".into();
            return;
        };
        let src = self.doc.nodes.get(nid).unwrap();
        if alt {
            // 全部样式:整份替换(目标无声明才允许空)
            let style = src.style.clone();
            if style.is_empty() {
                self.status = "吸管:目标没有样式可吸取".into();
                return;
            }
            let targets = self.selection.clone();
            if targets.is_empty() {
                self.status = format!("已吸取 {} 条样式(先选中对象再点应用)", style.len());
                return;
            }
            for sid in targets {
                self.exec(Command::SetStyle {
                    sid,
                    new: style.clone(),
                    old: None,
                });
            }
            self.status = format!("已应用全部样式({} 条声明)", style.len());
        } else {
            let Some(color) = src.fill_color() else {
                self.status = "吸管:目标没有填充色(Alt 可吸全部样式)".into();
                return;
            };
            let hex = color.to_shortest_hex();
            let targets = self.selection.clone();
            if targets.is_empty() {
                self.status = format!("已取色 {hex}(先选中对象再点应用)");
                return;
            }
            for sid in targets {
                let Some(t) = self.doc.find_by_sid(&sid) else {
                    continue;
                };
                let mut style = self.doc.nodes.get(t).unwrap().style.clone();
                if let Some(d) = style.iter_mut().find(|d| d.prop == "background-color") {
                    d.value = hex.clone();
                } else {
                    style.push(vb_css::Decl {
                        prop: "background-color".into(),
                        value: hex.clone(),
                        important: false,
                    });
                }
                self.exec(Command::SetStyle {
                    sid,
                    new: style,
                    old: None,
                });
            }
            self.status = format!("已应用填充 {hex}");
        }
    }
}

/// 递归给子树分配全新 sid(粘贴用:副本是新元素,必须有自己的稳定 id)。
fn re_sid_tree(tree: &mut vb_doc::model::NodeTree, doc: &mut Document) {
    tree.node.sid = doc.alloc_sid();
    for c in &mut tree.children {
        re_sid_tree(c, doc);
    }
}
