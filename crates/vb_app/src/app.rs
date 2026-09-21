//! 应用主体:画布(Vello 纹理合成)+ AI 式交互 + 属性/图层/状态面板。
//!
//! v0.1 范围(路线图 11 篇):选择/矩形/椭圆工具、Alt 复制、Shift 约束、
//! 框选(相交即选中)、Undo/Redo、属性面板、图层列表、状态栏、保存/导出。
//! 文本在画布上以 egui 近似绘制(ADR-0017)。
//!
//! S1-a 拆分说明(纯机械搬移,零行为变化):面板/画布/画布输入/菜单/
//! 工具条/对话框/编辑命令已按职责拆至本模块的子模块(`src/app/*.rs`、
//! `src/app/panels/*.rs`);本文件保留共享状态与协调逻辑(结构体、
//! 命令派发 run_command、主循环装配、项目开存与拾取)。

/// 对齐面板(⇧F7,阶段 2 / 副文档 03-5);`pub` 供纯函数门禁测试。
pub mod align_panel;
/// 外观面板/描边面板(S4 05-1/05-3/05-6):条目模型、CSS 编解码、
/// 命令构建器与不支持登记(`pub` 供门禁测试);渲染在子模块 `ui`。
pub mod appearance;
mod canvas;
mod canvas_input;
/// 「帮助 → 能力台账」窗口的渲染层(数据在 `crate::capabilities`)。
mod capabilities_ui;
/// 颜色面板(F6,05-5)+ 色板区;`pub` 供文档状态级门禁测试打纯函数层。
pub mod color_panel;
mod commands;
/// 控制面板(S1-c 02-2)与面板元数据。
///
/// `pub`:spec 类型、纯函数构建器与 `PROP_GROUPS` 供集成测试
/// (`vb_app::app::control_panel`)与文档引用;渲染入口本身 `pub(crate)`。
pub mod control_panel;
mod dialogs;
/// 工具栏停靠几何与 `workspace.json` 持久化(阶段 6 / 副文档 07);
/// `pub` 供停靠/持久化门禁测试打纯函数层。
pub mod dock_layout;
/// 渐变面板(^F9,05-2)与结构化渐变写回;`pub` 供文档状态级门禁测试打纯函数层。
pub mod gradient_panel;
/// 阶段 5(AI 规范 9 项菜单)新增命令的实现(副文档 06)。
pub mod menu_commands;
mod menus;
/// 透明度面板(⇧^F10,05-4);`pub` 供文档状态级门禁测试打纯函数层。
pub mod opacity_panel;
mod panels;
/// 工具箱四向停靠与同族分组(阶段 6 / 副文档 07);
/// `pub` 供工具箱分组门禁测试打纯函数层。
pub mod toolbar;
/// 变换数值面板(⇧F8,阶段 2 / 副文档 03-2/03-3);`pub` 供纯函数门禁测试。
pub mod transform_panel;

use std::path::PathBuf;

use egui::{Key, Margin, Rect, Vec2};
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, NodeKind};
use vb_doc::undo::UndoStack;
use vb_tools::Camera;
use vb_ui::cursor as vbcursor;
use vb_ui::fonts as vb_fonts;
use vb_ui::theme;

use crate::shortcuts::{self, InputContext};

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
    /// 渐变工具:拖动设定线性渐变方向(06 篇 §5.5)
    Gradient,
    /// 剪刀:在矢量锚点处剪开(闭路开口/开路分段;06 篇 §5.3)
    Scissors,
    /// 编组选择:单击选中命中对象所在的整个编组(06 篇 P0)
    GroupSelect,
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
    /// 渐变批注者:从 start 拖向光标 = 渐变方向与长度(06 篇 §3.11)。
    /// 阶段 4(05-2-3)补齐 `end`:松手后批注保留(见
    /// [`VellumApp::gradient_annot`]),可双击批注上的色标改色。
    GradientAnnotate {
        start: (f64, f64),
        end: (f64, f64),
        angle: f64,
    },
    /// Space/中键/抓手:平移视图
    Pan {
        start_pan: Vec2,
    },
    /// 移动对象(sid;alt 首动复制出的新 sid)。多选时 `others`
    /// 携带其余选中对象的起始几何,整体随主对象位移(B4)。
    MoveObj {
        sid: String,
        start_geom: Geom,
        grab_dx: f64,
        grab_dy: f64,
        moved: bool,
        others: Vec<(String, Geom)>,
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
    /// 位图缓存(B3):GUI 逐帧编码,不缓存则每帧解码一次文件;
    /// 热重载/打开新项目时清空。
    image_cache: std::collections::HashMap<String, vb_render::encode::BitmapData>,
    /// 拖拽期间是否有命令落盘(Esc 取消时据此判断有无可作废条目)。
    drag_edited: bool,
    /// 已同步到 egui 的主题(None=尚未同步;B4 主题单一真相)。
    theme_synced: Option<bool>,
    /// 当前主题(true=深色)。P2.7 支持浅色。
    theme_dark: bool,
    /// 右侧面板当前 Tab(S1-b:存 **Tab 语义 id**(0=属性 1=图层 2=画板
    /// 3=令牌);槽位顺序见 [`Self::panel_order`])。
    panel_tab: usize,
    /// 面板坞用户折叠偏好(S1-b 02-1-1)。
    /// 实际折叠 = `vb_ui::dock::should_collapse(视口宽, 本值)` —— 窗口
    /// <1200 时强制折叠且**不回写**本值(拉宽后自动恢复展开)。
    /// 持久化到 workspace.json 为阶段 7 项(接口已按"单字段可序列化"预留)。
    dock_collapsed: bool,
    /// Tab 顺序(槽位 → Tab 语义 id;S1-b 02-1-2,右键 Tab 可换,
    /// **内存可换**,持久化到 workspace.json 为阶段 7 项,接口预留)。
    panel_order: [usize; panels::TAB_COUNT],
    /// 隐藏/恢复所有面板(S1-b 02-6-5,`Tab`;隐藏右侧坞+状态栏+浮动工具条)。
    panels_hidden: bool,
    /// NumField 提交会话进行中(02-6-2;true = 连续编辑并入同一条 undo)。
    num_commit_open: bool,
    /// 属性面板七分组折叠状态(S1-c 02-3;下标 = `panels::properties` 组序)。
    prop_groups_open: [bool; 7],
    /// 图层面板搜索框(S1-d 02-4-7;按名过滤,纯前端,不动文档)。
    layer_search: String,
    /// 图层树展开的编组/图层 sid 集(02-4-1;缺省全展开,收起后记录)。
    layer_expanded: std::collections::HashSet<String>,
    /// 图层面板行拖拽进行中(S1-d 02-4-1/2:sid + 是否 Alt 复制)。
    layer_drag: Option<panels::layers::LayerDrag>,
    /// 图层面板行内改名的 sid(双击进入,失焦/回车提交 Rename)。
    editing_layer: Option<String>,
    /// 画板面板「重新排列」的画板间距(S1-d 02-5;默认 80px)。
    arrange_gap: f64,
    /// toast 通知(02-6-6:错误/告警走 toast,常规状态保留 footer)。
    toasts: vb_ui::toast::ToastHost,
    /// 上一帧视口宽度(run_command 无 ctx,折叠判定用它)。
    last_viewport_width: f32,
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
    /// 字符面板显隐(04,Ctrl+T;design/03 §5.10 独立浮窗)
    char_panel_open: bool,
    /// 段落面板显隐(04,Ctrl+Alt+T)
    para_panel_open: bool,
    /// 外观面板显隐(S4,⇧F6;design/03 §5.9 独立浮窗)
    appearance_panel_open: bool,
    /// 描边面板显隐(S4,^F10;design/03 §5.7 独立浮窗)
    stroke_panel_open: bool,
    /// 外观面板当前选中条目下标(参数编辑区;越界自动清)
    appearance_sel: Option<usize>,
    /// 渐变面板显隐(S4-b,^F9;design/03 §5.6 独立浮窗)
    gradient_panel_open: bool,
    /// 渐变面板当前选中色标下标(越界自动夹回)
    gradient_sel: Option<usize>,
    /// 画布渐变批注(05-2-3):`(起点x, 起点y, 终点x, 终点y)` 世界坐标。
    /// 松手后保留,供「双击批注上的色标改色」命中判定与重绘。
    gradient_annot: Option<(f64, f64, f64, f64)>,
    /// 透明度面板显隐(S4-b,⇧^F10;design/03 §5.8 独立浮窗)
    opacity_panel_open: bool,
    /// 颜色面板显隐(S4-b,F6;design/03 §5.4 独立浮窗;含色板区 §5.5)
    color_panel_open: bool,
    /// 颜色面板当前页签(HSB / RGB / CMYK / CSS 变量)
    color_tab: usize,
    /// 颜色面板作用目标:`true` = 描边,`false` = 填充(AI 的 X 切换)
    color_target_stroke: bool,
    /// 最近一次使用的效果(效果菜单「应用上一个效果」的回忆项);
    /// 外观面板与效果菜单都往里写,是**会话状态**。
    last_effect: Option<crate::app::appearance::Effect>,
    /// 工具箱停靠边(阶段 6 / 07-1;持久化到 `workspace.json`)。
    toolbar_dock: dock_layout::DockSide,
    /// 左/右停靠时的列数(1 或 2)。
    toolbar_columns: u8,
    /// 拖动把手进行中(会话态,不持久化)。
    toolbar_dragging: bool,
    /// 拖动中的吸附预览边(会话态,不持久化)。
    toolbar_dock_preview: Option<dock_layout::DockSide>,
    /// 上次**已持久化**的工作区快照(脏检查;阶段 6 / 07-2 写通)。
    workspace_saved: dock_layout::WorkspaceConfig,
    /// 变换数值面板显隐(阶段 2 / 03-2,`⇧F8`)
    transform_panel_open: bool,
    /// 变换参考点(九宫格;缩放/倾斜轴心)
    transform_ref: transform_panel::RefPoint,
    /// W/H 锁链等比
    transform_lock_ratio: bool,
    /// 「缩放描边和效果」(HTML 无该语义,UI 如实标注)
    transform_scale_stroke: bool,
    /// 「对齐像素网格」(几何取整)
    transform_snap_pixel: bool,
    /// 上一次变换(位移 + 缩放 + 旋转):`Mod+D` 再次变换重放它(03-4-2)
    last_transform: Option<transform_panel::TransformDelta>,
    /// 对齐面板显隐(阶段 2 / 03-5,`⇧F7`)
    align_panel_open: bool,
    /// 「对齐到」三选一(选区 / 关键对象 / 画板)
    align_to: align_panel::AlignTo,
    /// 能力台账窗口(副文档 09-3;`帮助 → 能力台账`)
    capabilities_ui: crate::capabilities::CapabilityUi,
    /// 文字工具待用模式(04-3 Shift+T 循环;新建文本用)
    text_mode_pending: vb_doc::model::TextMode,
    /// 新建文本默认样式(04-3-3:字符面板「默认样式」区投影;替换写死 24px/黑)。
    /// 会话状态,持久化到 workspace.json 为阶段 7 项。
    text_default: vb_doc::model::SegStyle,
    /// 文本「二次 Esc 放弃」武装:(sid, 提交时刻);Esc 提交后短时武装,
    /// 再次 Esc 作废刚提交的 SetText(不进 redo 栈,「放弃」语义)。
    text_discard_arm: Option<(String, std::time::Instant)>,
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

        // 工作区布局(阶段 6 / 07-2):启动还原;损坏/版本不符 → 回退默认 + 告警
        let (ws, ws_warn) = dock_layout::load();
        let ws_panel_order: [usize; 4] = {
            let mut a = [0, 1, 2, 3];
            for (i, v) in ws.panel_order.iter().take(4).enumerate() {
                a[i] = *v;
            }
            a
        };
        if let Some(w) = &ws_warn {
            log::warn!("{w}");
        }

        let (doc, project_dir) = match &project {
            Some(p) => match import_with_layout(p) {
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
            image_cache: std::collections::HashMap::new(),
            drag_edited: false,
            theme_synced: None,
            theme_dark: true,
            // 面板坞状态由工作区配置还原(阶段 6 / 07-2-1)
            panel_tab: ws.panel_tab.min(panels::TAB_COUNT - 1),
            dock_collapsed: ws.dock_collapsed,
            panel_order: ws_panel_order,
            panels_hidden: ws.panels_hidden,
            num_commit_open: false,
            prop_groups_open: [true; 7],
            layer_search: String::new(),
            layer_expanded: std::collections::HashSet::new(),
            layer_drag: None,
            editing_layer: None,
            arrange_gap: 80.0,
            toasts: vb_ui::toast::ToastHost::default(),
            last_viewport_width: 1280.0,
            smart_guides: Vec::new(),
            editing_text: None,
            char_panel_open: false,
            para_panel_open: false,
            appearance_panel_open: false,
            stroke_panel_open: false,
            appearance_sel: None,
            gradient_panel_open: false,
            gradient_sel: None,
            gradient_annot: None,
            opacity_panel_open: false,
            color_panel_open: false,
            color_tab: 0,
            color_target_stroke: false,
            last_effect: None,
            toolbar_dock: ws.toolbar_dock,
            toolbar_columns: ws.toolbar_columns,
            toolbar_dragging: false,
            toolbar_dock_preview: None,
            workspace_saved: ws.clone(),
            transform_panel_open: false,
            transform_ref: transform_panel::RefPoint::MC,
            transform_lock_ratio: false,
            transform_scale_stroke: false,
            transform_snap_pixel: false,
            last_transform: None,
            align_panel_open: false,
            align_to: align_panel::AlignTo::default(),
            capabilities_ui: crate::capabilities::CapabilityUi::default(),
            text_mode_pending: vb_doc::model::TextMode::Point,
            text_default: vb_doc::model::SegStyle {
                font_size: Some(24.0),
                color: Some("#1a1a1a".into()), // vb-token-ok: 新建文本默认字色(文档内容,非 UI 皮肤)
                ..vb_doc::model::SegStyle::default()
            },
            text_discard_arm: None,
            status: match &ws_warn {
                Some(w) => format!("{w} — 已使用默认布局"),
                None => "就绪 — V 选择 · A 直接选择 · M 矩形 · Alt+拖动 复制 · Shift 约束 · Space 平移 · Ctrl+0 适合".into(),
            },
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
            self.toast_error(format!("命令失败:{e}"));
            return;
        }
        // 拖拽进行中的每次落盘都标记(Esc 取消时据此作废合并条目)
        if !matches!(self.drag, Drag::None) {
            self.drag_edited = true;
        }
    }

    /// 常规状态提示(保留 footer 单行;02-6-6)。
    pub(crate) fn say(&mut self, text: impl Into<String>) {
        self.status = text.into();
    }

    /// 告警级反馈:footer 同步短句 + toast(可复制、更长存活)。
    pub(crate) fn toast_warn(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.status = t.clone();
        self.toasts.push(vb_ui::toast::ToastKind::Warn, t);
    }

    /// 错误级反馈:footer 同步短句 + toast(可复制、10s 存活)。
    pub(crate) fn toast_error(&mut self, text: impl Into<String>) {
        let t = text.into();
        self.status = t.clone();
        self.toasts.push(vb_ui::toast::ToastKind::Error, t);
    }

    /// NumField 提交统一路径(02-6-2 ⭐):把组件的会话信号折算成
    /// `UndoStack::begin_session / end_session`,使连续 scrubby/键盘
    /// 步进/连续表达式提交**合并为一条 undo**;顺带执行命令与错误 toast。
    ///
    /// 规则:
    /// - `scrub_started` 或首帧 `changed` → 开会话(幂等);
    /// - `scrub_ended` / `focus_lost` → 关会话(松手或点走 = 一次编辑结束);
    /// - `run_command` 顶层兜底关会话(见上)。
    pub(crate) fn num_commit(&mut self, r: vb_ui::NumFieldResponse, cmd: Option<Command>) {
        if !self.num_commit_open && (r.scrub_started || r.changed) {
            self.num_commit_open = true;
            self.undo.begin_session();
        }
        if let Some(c) = cmd {
            self.exec(c);
        }
        if let Some(e) = r.expr_error {
            self.toast_error(e);
        }
        if r.scrub_ended || (r.focus_lost && self.num_commit_open) {
            self.num_commit_open = false;
            self.undo.end_session();
        }
    }

    /// 缩放到选区(C3):选区联合 bbox 充满视口;无选区回退 fit_view。
    fn zoom_to_selection(&mut self) {
        let Some(rect) = self.canvas_rect else {
            return;
        };
        let mut x0 = f64::INFINITY;
        let mut y0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for sid in &self.selection {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                if let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) {
                    x0 = x0.min(bb.x0);
                    y0 = y0.min(bb.y0);
                    x1 = x1.max(bb.x1);
                    y1 = y1.max(bb.y1);
                }
            }
        }
        if !x0.is_finite() {
            self.fit_view();
            return;
        }
        let margin = 60.0f64;
        let w = (x1 - x0).max(20.0);
        let h = (y1 - y0).max(20.0);
        let zoom = ((rect.width() as f64 - margin * 2.0) / w)
            .min((rect.height() as f64 - margin * 2.0) / h)
            .clamp(0.01, 64.0);
        self.camera.zoom = zoom;
        self.camera.pan_x = rect.width() as f64 / 2.0 - (x0 + w / 2.0) * zoom;
        self.camera.pan_y = rect.height() as f64 / 2.0 - (y0 + h / 2.0) * zoom;
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
            Err(e) => self.toast_error(format!("保存失败:{e}")),
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
        // 文档变更:位图缓存整体失效(B3;文件内容可能已被外部替换)
        self.image_cache.clear();
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
            match import_with_layout(&dir) {
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
                    self.layer_drag = None;
                    self.editing_layer = None;
                    self.saved_rev = self.doc.rev;
                    self.status = format!("检测到外部修改,已自动采用(Agent 热重载,{n} 画板)");
                }
                Err(e) => self.toast_error(format!("热重载失败:{e}")),
            }
        } else {
            self.toast_warn("检测到磁盘修改,但本地有未保存编辑(未自动采用;先 Ctrl+S 或撤销)");
        }
    }

    fn open_project(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("打开项目目录(含 index.html)")
            .pick_folder()
        {
            match import_with_layout(&dir) {
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
                    self.layer_drag = None;
                    self.editing_layer = None;
                    self.project_dir = Some(r.project_dir);
                    self.fit_view();
                    self.status = format!("已打开 {}(画板 {n})", dir.display());
                }
                Err(e) => self.toast_error(format!("打开失败:{e}")),
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
    fn insert_target(&mut self, wx: f64, wy: f64) -> vb_doc::model::NodeId {
        if let Some(iso) = self.isolate_top() {
            return iso;
        }
        match self
            .artboard_at_world(wx, wy)
            .or(self.doc.artboards.first().copied())
        {
            Some(ab) => ab,
            // 兜底:命令层「至少一块画板」守卫之外的第二道保险(导入 0 画板
            // 文档后直接开画等)。恢复路径直接落一块默认画板,不走 undo。
            None => {
                let id = self.doc.new_artboard("画板 1", 1440.0, 900.0);
                self.status = "画布为空,已重建默认画板".into();
                id
            }
        }
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
        // 主题单一真相(B4):theme_dark 是唯一来源,变化时同步进
        // egui 偏好 —— 否则 tokens(ctx)(读 ctx.theme())在系统浅色
        // 模式下取到浅色令牌,深色界面对比度塌掉
        if self.theme_synced != Some(self.theme_dark) {
            ui.ctx().set_theme(if self.theme_dark {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            });
            self.theme_synced = Some(self.theme_dark);
        }
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
        // 工作区布局写通(阶段 6 / 07-2):任何停靠/Tab/折叠变化在下一次存活帧落盘
        if self.workspace_dirty() {
            self.save_workspace();
        }
        // 缓存视口宽度(run_command 里做折叠判定用)
        self.last_viewport_width = ui.ctx().viewport_rect().width();
        self.top_menu(ui);
        // 控制面板(S1-c 02-2):菜单栏下的随工具上下文条;随 Tab 一起隐藏
        if !self.panels_hidden {
            self.control_bar(ui);
        }
        // Tab(02-6-5):隐藏所有面板 —— 右侧坞/状态栏/浮动工具条都不画,
        // 画布吃满窗口;再按 Tab 恢复。顶部菜单保留(可发现性)。
        // 工具箱停靠(阶段 6 / 07-1)。**装配次序 = 层级规矩**(07 §6 风险):
        // egui 的底/顶面板"先装者更靠外",故 底向工具栏必须装在**状态栏之后**,
        // 状态栏才能永远贴底;顶/左/右三向则在状态栏之前装配。
        if !self.panels_hidden && self.toolbar_dock != dock_layout::DockSide::Bottom {
            self.docked_toolbar(ui);
        }
        if !self.panels_hidden {
            self.right_panel(ui);
            self.status_bar(ui, frame);
            if self.toolbar_dock == dock_layout::DockSide::Bottom {
                self.docked_toolbar(ui);
            }
        }
        self.canvas(ui, frame);

        // 对话框与浮窗(拆分至 dialogs.rs,窗口内容逐字未动)
        self.show_about_window(ui);
        // 04 字符/段落浮窗(Ctrl+T / Ctrl+Alt+T;design/03 §5.10)
        self.show_char_panel(ui);
        self.show_para_panel(ui);
        // S4 外观/描边浮窗(⇧F6 / ^F10;design/03 §5.9 / §5.7)
        self.show_appearance_panel(ui);
        self.show_stroke_panel(ui);
        self.show_gradient_panel(ui);
        self.show_opacity_panel(ui);
        self.show_color_panel(ui);
        // 变换数值面板(阶段 2 / 03-2,⇧F8)
        self.show_transform_panel(ui);
        // 对齐面板(阶段 2 / 03-5,⇧F7)
        self.show_align_panel(ui);
        // 能力台账(副文档 09-3;帮助 → 能力台账)
        self.show_capabilities_window(ui);
        self.show_text_edit_window(ui);
        self.show_export_window(ui);
        self.show_command_palette(ui);
        // toast 通知(02-6-6:错误/告警,右下角可堆叠;面板隐藏时也在)
        self.toasts.show(ui.ctx());
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
            let Some(sc) = shortcuts::lookup(key, ctrl, shift, alt) else {
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
    /// 判定单一真相在 `shortcuts::top_input_context` 纯函数
    /// (B1 派发层回归测试直接打它;此处只做 GUI 状态投影)。
    fn input_context(&self, ctx: &egui::Context) -> InputContext {
        shortcuts::top_input_context(
            self.editing_text.is_some(),
            self.palette_open,
            ctx.egui_wants_keyboard_input(),
            !matches!(self.drag, Drag::None),
        )
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
        // NumField 提交会话兜底收口:任何快捷键/菜单命令都意味着
        // 用户离开了数值框编辑(键盘输入在 TextEdit 上下文不会派发到这),
        // 会话不该跨过一次显式命令继续合并。
        if self.num_commit_open {
            self.num_commit_open = false;
            self.undo.end_session();
        }
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
            self.toast_warn("拖拽进行中:先松手或 Esc 取消");
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
                self.layer_drag = None;
                self.editing_layer = None;
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
            "view.next_artboard" | "view.prev_artboard" => {
                // 画板循环导航:选中并视图居中(C3)
                if self.doc.artboards.is_empty() {
                    return;
                }
                let cur = self
                    .selection
                    .first()
                    .and_then(|s| self.doc.find_by_sid(s))
                    .and_then(|id| self.doc.artboards.iter().position(|&a| a == id));
                let n = self.doc.artboards.len();
                let next = match cur {
                    Some(i) => {
                        if id == "view.next_artboard" {
                            (i + 1) % n
                        } else {
                            (i + n - 1) % n
                        }
                    }
                    None => 0,
                };
                let ab = self.doc.artboards[next];
                let sid = self.doc.nodes.get(ab).unwrap().sid.as_str().to_string();
                self.selection = vec![sid];
                self.run_command("view.zoom_to_selection", false, false);
            }
            "view.next_panel_tab" => {
                self.panel_tab = (self.panel_tab + 1) % panels::TAB_COUNT;
            }
            // ── S1-b 面板显隐(F7 / Tab;design/02 §四-面板显隐) ──
            "view.toggle_layers_panel" => {
                let forced = self.last_viewport_width < vb_ui::theme::space::COLLAPSE_BELOW;
                // 「图层可见」= 面板未被 Tab 隐藏、未折叠(窄窗强制折叠不算)且正处图层 Tab
                let layers_visible = !self.panels_hidden
                    && (forced || !self.dock_collapsed)
                    && self.panel_tab == panels::TAB_LAYERS;
                if layers_visible {
                    if !forced {
                        self.dock_collapsed = true;
                    }
                    self.say("图层面板:已折叠(F7 恢复)");
                } else {
                    self.panels_hidden = false;
                    if !forced {
                        self.dock_collapsed = false;
                    }
                    self.panel_tab = panels::TAB_LAYERS;
                    self.say("图层面板:显示(F7 折叠)");
                }
            }
            "view.toggle_all_panels" => {
                self.panels_hidden = !self.panels_hidden;
                self.say(if self.panels_hidden {
                    "已隐藏所有面板(Tab 恢复)"
                } else {
                    "已恢复所有面板(Tab 再隐藏)"
                });
            }
            "view.zoom_to_selection" => {
                self.zoom_to_selection();
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
                let key = shortcuts::key_text_for("view.guides_from_selection")
                    .unwrap_or_else(|| "未绑定".into());
                self.status = format!("从选区生成 {added} 条参考线({key})");
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
            // ── 04 字符/段落面板与文字工具模式 ──
            "view.toggle_char_panel" => {
                self.char_panel_open = !self.char_panel_open;
                self.say(if self.char_panel_open {
                    "字符面板:显示(Ctrl+T 关闭)"
                } else {
                    "字符面板:隐藏(Ctrl+T 显示)"
                });
            }
            "view.toggle_para_panel" => {
                self.para_panel_open = !self.para_panel_open;
                self.say(if self.para_panel_open {
                    "段落面板:显示(Ctrl+Alt+T 关闭)"
                } else {
                    "段落面板:隐藏(Ctrl+Alt+T 显示)"
                });
            }
            // ── S4 外观/描边面板显隐(⇧F6 / ^F10;design/03 §5.9 / §5.7) ──
            "view.toggle_appearance_panel" => {
                self.appearance_panel_open = !self.appearance_panel_open;
                self.say(if self.appearance_panel_open {
                    "外观面板:显示(⇧F6 关闭)"
                } else {
                    "外观面板:隐藏(⇧F6 显示)"
                });
            }
            "view.toggle_stroke_panel" => {
                self.stroke_panel_open = !self.stroke_panel_open;
                self.say(if self.stroke_panel_open {
                    "描边面板:显示(^F10 关闭)"
                } else {
                    "描边面板:隐藏(^F10 显示)"
                });
            }
            // ── S4-b 渐变/透明度/颜色面板显隐(^F9 / ⇧^F10 / F6) ──
            "view.toggle_gradient_panel" => {
                self.gradient_panel_open = !self.gradient_panel_open;
                self.say(if self.gradient_panel_open {
                    "渐变面板:显示(^F9 关闭)"
                } else {
                    "渐变面板:隐藏(^F9 显示)"
                });
            }
            "view.toggle_opacity_panel" => {
                self.opacity_panel_open = !self.opacity_panel_open;
                self.say(if self.opacity_panel_open {
                    "透明度面板:显示(⇧^F10 关闭)"
                } else {
                    "透明度面板:隐藏(⇧^F10 显示)"
                });
            }
            "view.toggle_color_panel" => {
                self.color_panel_open = !self.color_panel_open;
                self.say(if self.color_panel_open {
                    "颜色面板:显示(F6 关闭)"
                } else {
                    "颜色面板:隐藏(F6 显示)"
                });
            }
            // ── S4-b 颜色动作(D / X / Shift+X;design/03 §5.4)──
            "color.toggle_target" => self.color_toggle_target(),
            "color.swap_fill_stroke" => self.color_swap(),
            "color.default_fill_stroke" => self.color_default(),
            // ── 副文档 09-3:能力台账(帮助 → 能力台账)──
            "help.capabilities" => {
                self.capabilities_ui.toggle();
                self.say(if self.capabilities_ui.open {
                    "能力台账:显示(再点关闭)"
                } else {
                    "能力台账:隐藏"
                });
            }
            // ── 阶段 2:变换数值面板(副文档 03-2,⇧F8)──
            "view.toggle_transform_panel" => {
                self.transform_panel_open = !self.transform_panel_open;
                self.say(if self.transform_panel_open {
                    "变换面板:显示(⇧F8 关闭)"
                } else {
                    "变换面板:隐藏(⇧F8 显示)"
                });
            }
            "view.toggle_align_panel" => {
                self.align_panel_open = !self.align_panel_open;
                self.say(if self.align_panel_open {
                    "对齐面板:显示(⇧F7 关闭)"
                } else {
                    "对齐面板:隐藏(⇧F7 显示)"
                });
            }
            // ── 阶段 2:路径查找器扩展三运算(副文档 03-1-4)──
            "path.merge" => self.path_boolean(vb_tools::boolean::BooleanOp::Merge),
            "path.subtract_back" => self.path_boolean(vb_tools::boolean::BooleanOp::SubtractBack),
            "path.crop" => self.path_boolean(vb_tools::boolean::BooleanOp::Crop),
            // ── 阶段 2:对齐工具族(副文档 03-5)──
            "align.to_selection" => self.set_align_to(align_panel::AlignTo::Selection),
            "align.to_key_object" => self.set_align_to(align_panel::AlignTo::KeyObject),
            "align.to_artboard" => self.set_align_to(align_panel::AlignTo::Artboard),
            "object.distribute_hspace" => self.distribute_space(true),
            "object.distribute_vspace" => self.distribute_space(false),
            // ── 阶段 6:工具箱停靠(副文档 07-4-1)──
            "view.dock_toolbar_top" => {
                self.set_toolbar_dock(dock_layout::DockSide::Top);
            }
            "view.dock_toolbar_left" => {
                self.set_toolbar_dock(dock_layout::DockSide::Left);
            }
            "view.dock_toolbar_right" => {
                self.set_toolbar_dock(dock_layout::DockSide::Right);
            }
            "view.dock_toolbar_bottom" => {
                self.set_toolbar_dock(dock_layout::DockSide::Bottom);
            }
            "view.toolbar_columns_1" => self.set_toolbar_columns(1),
            "view.toolbar_columns_2" => self.set_toolbar_columns(2),
            "tool.text_cycle_mode" => {
                // Shift+T:点 ↔ 区域循环(路径文本 v1.5 冻结登记);选中
                // 文本对象时经 SetTextMode 命令一并转换(可撤销)
                self.text_mode_pending = match self.text_mode_pending {
                    vb_doc::model::TextMode::Point => vb_doc::model::TextMode::Area,
                    vb_doc::model::TextMode::Area => vb_doc::model::TextMode::Point,
                };
                let pending = format!("{:?}", self.text_mode_pending);
                let targets: Vec<String> = self
                    .selection
                    .iter()
                    .filter(|sid| {
                        self.doc
                            .find_by_sid(sid)
                            .and_then(|nid| self.doc.nodes.get(nid))
                            .is_some_and(|n| matches!(n.kind, vb_doc::model::NodeKind::Text { .. }))
                    })
                    .cloned()
                    .collect();
                if !targets.is_empty() {
                    let cmds: Vec<Command> = targets
                        .iter()
                        .map(|sid| Command::SetTextMode {
                            sid: sid.clone(),
                            new: self.text_mode_pending,
                            old: None,
                        })
                        .collect();
                    let n = cmds.len();
                    self.exec(Command::Compound { cmds });
                    self.say(format!(
                        "文字模式 → {pending}(待用 + {n} 个选中文本对象已转换)"
                    ));
                } else {
                    self.say(format!("文字模式 → {pending}(下次新建生效)"));
                }
            }
            "tool.eyedropper" => self.set_tool(Tool::Eyedropper),
            "tool.artboard" => self.set_tool(Tool::Artboard),
            "tool.gradient" => self.set_tool(Tool::Gradient),
            "tool.scissors" => self.set_tool(Tool::Scissors),
            "tool.group_select" => self.set_tool(Tool::GroupSelect),
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
            // ── C1 路径查找器(四基本运算;两两矢量路径) ──
            "path.union" => self.path_boolean(vb_tools::boolean::BooleanOp::Union),
            "path.subtract" => self.path_boolean(vb_tools::boolean::BooleanOp::Subtract),
            "path.intersect" => self.path_boolean(vb_tools::boolean::BooleanOp::Intersect),
            "path.xor" => self.path_boolean(vb_tools::boolean::BooleanOp::Xor),
            "align.left" => self.align_selection("left"),
            "align.hcenter" => self.align_selection("hcenter"),
            "align.right" => self.align_selection("right"),
            "align.top" => self.align_selection("top"),
            "align.vcenter" => self.align_selection("vcenter"),
            "align.bottom" => self.align_selection("bottom"),
            // ── P3.9 锁定 / 隐藏 ──
            "object.lock" => {
                // 多选合成一条 Compound(N 条独立 undo → 一条,B4)
                let cmds: Vec<Command> = self
                    .selection
                    .clone()
                    .into_iter()
                    .map(|sid| Command::SetFlags {
                        sid,
                        hidden: None,
                        locked: Some(true),
                        old: None,
                    })
                    .collect();
                if !cmds.is_empty() {
                    self.exec(Command::Compound { cmds });
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
                let cmds: Vec<Command> = self
                    .selection
                    .clone()
                    .into_iter()
                    .map(|sid| Command::SetFlags {
                        sid,
                        hidden: Some(true),
                        locked: None,
                        old: None,
                    })
                    .collect();
                if !cmds.is_empty() {
                    self.exec(Command::Compound { cmds });
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
                // 04-3(2):文本「二次 Esc = 放弃」—— Esc 提交后短时武装,
                // 再次 Esc 作废刚提交的 SetText(不进 redo 栈;放弃不可重做)
                if let Some((sid, at)) = self.text_discard_arm.clone() {
                    let armed = at.elapsed() < std::time::Duration::from_secs(5)
                        && self.undo.top().is_some_and(
                            |c| matches!(c, Command::SetText { sid: s, .. } if *s == sid),
                        );
                    self.text_discard_arm = None;
                    if armed {
                        self.undo.cancel_top(&mut self.doc);
                        self.say("已放弃文本修改(未入撤销栈)");
                        return;
                    }
                }
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
                    // Esc 取消语义:拖拽产生的合并条目从 undo 栈整体作废
                    // (不进 redo 栈 —— 取消的动作不可重做),文档直接回到
                    // 拖拽前。此前走 exec(还原)会留下一条
                    // 「Ctrl+Z 跳回被取消位置」的 undo 步(B4)
                    match std::mem::replace(&mut self.drag, Drag::None) {
                        Drag::MoveObj { .. }
                        | Drag::Resize { .. }
                        | Drag::Rotate { .. }
                        | Drag::GradientAnnotate { .. } => {
                            if self.drag_edited {
                                self.undo.cancel_top(&mut self.doc);
                                self.status = "已取消(未入撤销栈)".into();
                            } else {
                                self.status = "已取消".into();
                            }
                            self.drag_edited = false;
                            self.last_move_delta = None;
                        }
                        _ => {
                            self.status = "已取消".into();
                        }
                    }
                    self.smart_guides.clear();
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
            // ── 阶段 5:AI 规范 9 项菜单新增命令(实现见 `menu_commands.rs`)──
            other => {
                if !self.run_menu_command(other) {
                    debug_assert!(false, "命令 {other} 未在 run_command 中实现");
                    log::warn!("未实现的命令:{other}");
                    self.toast_error(format!("命令未实现:{other}"));
                }
            }
        }
    }
}

// ---------- 辅助 ----------

/// 菜单项按钮:标签 + 右侧键位文本(键位一律查 `shortcuts` 注册表)。
/// 导入 + 内存布局求值(P0-1):打开/热重载/打开目录三条路共用。
/// 声明几何(百分比锚/inset/right|bottom/流式)经 taffy 解析为具体几何供画布
/// 使用;声明本身保留在 style,保存不烤入。缺标记/布局降级告警走 log(不静默)。
fn import_with_layout(
    path: &std::path::Path,
) -> Result<vb_doc::import::ImportResult, vb_doc::VbError> {
    let mut r = vb_doc::import::import_project(path)?;
    let dir = r.project_dir.clone();
    for w in &r.warnings {
        log::warn!("{w}");
    }
    let synthetic = r.synthetic_artboard;
    for w in vb_layout::apply_import_layout(&mut r.doc, Some(&dir), synthetic) {
        log::warn!("{w}");
    }
    Ok(r)
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

/// 设置或**删除**一条声明(`value = None` → 移除)。透明度/颜色面板共用:
/// 清空要真的把声明删掉,不能留 `opacity: ;` 这种非法值。
pub(crate) fn style_set_or_remove(
    style: Vec<vb_css::Decl>,
    prop: &str,
    value: Option<&str>,
) -> Vec<vb_css::Decl> {
    let mut s = style;
    match value {
        Some(v) => {
            if let Some(d) = s.iter_mut().find(|d| d.prop == prop) {
                d.value = v.to_string();
            } else {
                s.push(vb_css::Decl {
                    prop: prop.into(),
                    value: v.into(),
                    important: false,
                });
            }
        }
        None => s.retain(|d| d.prop != prop),
    }
    s
}

const _: Margin = Margin::ZERO;

/// 解析 transform 中的 rotate(θdeg) → 度(CSS 顺时针)。
/// (S1-c 自 canvas_input.rs 上移共享:画布旋转拖拽 / 属性面板 ∠ / 控制面板 ∠)
pub(crate) fn parse_rotate_deg(v: &str) -> Option<f64> {
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
pub(crate) fn fmt_deg(deg: f64) -> String {
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

/// 递归给子树分配全新 sid(粘贴用:副本是新元素,必须有自己的稳定 id)。
fn re_sid_tree(tree: &mut vb_doc::model::NodeTree, doc: &mut Document) {
    tree.node.sid = doc.alloc_sid();
    for c in &mut tree.children {
        re_sid_tree(c, doc);
    }
}
