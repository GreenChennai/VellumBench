//! 应用主体:画布(Vello 纹理合成)+ AI 式交互 + 属性/图层/状态面板。
//!
//! v0.1 范围(路线图 11 篇):选择/矩形/椭圆工具、Alt 复制、Shift 约束、
//! 框选(相交即选中)、Undo/Redo、属性面板、图层列表、状态栏、保存/导出。
//! 文本在画布上以 egui 近似绘制(ADR-0017)。
//!
//! S1-a / 06-1 拆分说明(纯机械搬移,零行为变化):面板/画布/画布输入/
//! 菜单/工具条/对话框/编辑命令已按职责拆至子模块(`src/app/*.rs`、
//! `src/app/panels/*.rs`);06-1 再把命令派发(`dispatch*`)、项目开存
//! 与外部监听(`external`)、视图导航与拾取(`nav`)迁出 —— 本文件只留
//! **应用生命周期 + 装配**:结构体、构造、主循环与共享纯函数。

/// 对齐面板(⇧F7,阶段 2 / 副文档 03-5);`pub` 供纯函数门禁测试。
pub mod align_panel;
/// 外观面板/描边面板(S4 05-1/05-3/05-6):条目模型、CSS 编解码、
/// 命令构建器与不支持登记(`pub` 供门禁测试);渲染在子模块 `ui`。
pub mod appearance;
mod assemble;
/// 资产面板(阶段 7 / 07-K):assets/ 清单 + 引用关系 + 定位/替换。
/// `pub`:数据模型(`AssetRow` / `ReplacePick`)与纯函数供门禁测试。
pub mod assets_panel;
/// 05-5 响应式断点(09-F):断点清单 meta、可用断点、覆盖样式命令构建
/// 与门禁测试(`pub` 供文档状态级门禁测试打纯函数层)。
pub mod breakpoints;
mod canvas;
mod canvas_input;
/// 「帮助 → 能力台账」窗口的渲染层(数据在 `crate::capabilities`)。
mod capabilities_ui;
/// 09-B 剪切蒙版(05-2):overflow 容器建模的命令序纯函数 + 单测。
mod clip_mask;
/// 颜色面板(F6,05-5)+ 色板区;`pub` 供文档状态级门禁测试打纯函数层。
pub mod color_panel;
mod commands;
/// 控制面板(S1-c 02-2)与面板元数据。
///
/// `pub`:spec 类型、纯函数构建器与 `PROP_GROUPS` 供集成测试
/// (`vb_app::app::control_panel`)与文档引用;渲染入口本身 `pub(crate)`。
pub mod control_panel;
mod dialogs;
mod dispatch;
mod dispatch_canvas;
mod dispatch_view;
/// 工具栏停靠几何与 `workspace.json` 持久化(阶段 6 / 副文档 07);
/// `pub` 供停靠/持久化门禁测试打纯函数层。
/// 项目开存与外部监听(06-1 自本文件迁出;`ExternalChange` 路径不变)。
pub use external::{ExternalChange, EXTERNAL_FILES_MAX};

/// 05-4-A2:外部冲突三方对比对话框(09-N;差异视图复用 recover 的 LCS 行 diff)。
mod conflict_dialog;
/// 05-4-A2:文档设置对话框(09-M;项目级网格/参考线落 index.html 的 vb-* meta)。
mod doc_settings;
pub mod dock_layout;
mod external;
/// 05-4-A2:字体缺失专项对话框(09-O;替换经命令层可撤销)。
mod font_dialog;
mod frame;
/// 渐变面板(^F9,05-2)与结构化渐变写回;`pub` 供文档状态级门禁测试打纯函数层。
pub mod gradient_panel;
/// 项目健康检查(阶段 7 / 07-E):缺失资源 / 失效链接 / 冻结块 /
/// 未使用资产 / 超长文件的纯函数盘点 + 报告窗口(子模块内单测)。
/// `pub(crate)`:07-K 资产面板复用其引用收集(`asset_reference_map`),
/// 不写两套。
pub(crate) mod health;
/// 撤销历史面板(阶段 7 / 07-D):最近 N 步命令名列表 + 点击跳转状态机。
mod history;
/// 09-D 图像置入/替换 + 蒙版/保真度命令实现(05-2;纯函数可测)。
mod image_ops;
/// 05-4-A2:键位方案编辑器 GUI(09-L;存储与叠加解析在 `crate::keymap`)。
pub(crate) mod keymap_dialog;
/// 阶段 5(AI 规范 9 项菜单)新增命令的实现(副文档 06)。
pub mod menu_commands;
mod menus;
mod nav;
/// 透明度面板(⇧^F10,05-4);`pub` 供文档状态级门禁测试打纯函数层。
pub mod opacity_panel;
/// 次级面板坞(04-2 / 副文档 04 P0-⑥):九面板停靠 Tab 化 +
/// 受控浮窗(防级联)+ workspace 记忆;`pub` 供度量门禁测试打纯函数层。
pub mod panel_dock;
mod panels;
/// 05-10 插件宿主 UI(09-J,ADR-VB-L12):HostServices 端口、管理窗口、
/// 插件坞面板、授权弹窗与逐帧 poll(注册表/权限/子进程在 `vb_plugin`)。
pub(crate) mod plugins;
/// 05-4-A2:首选项九分类对话框(09-L;收编既有散落设置项)。
mod prefs_dialog;
/// 自动保存节拍 + 崩溃恢复 GUI(阶段 7 / 07-A·07-B;文件层在 `crate::autosave`)。
mod recover;
/// 05-8 符号 / 组件命令的选区语义与派发(09-H;事务构建在 vb_doc::symbol)。
pub mod symbol_cmds;
/// 05-9 动效时间轴(09-I,ADR-VB-L11):模型 ⇄ CSS 投影、时间轴面板、
/// 播放会话与预览节拍;`pub` 供序列化/投影门禁测试打纯函数层。
pub mod timeline;
/// 工具箱四向停靠与同族分组(阶段 6 / 副文档 07);
/// `pub` 供工具箱分组门禁测试打纯函数层。
pub mod toolbar;
/// 变换数值面板(⇧F8,阶段 2 / 副文档 03-2/03-3);`pub` 供纯函数门禁测试。
pub mod transform_panel;
/// 05-4-A2:X-7 打印(复用 Kiln PDF 链)与新建工作区(命名预设)。
mod workspace_dialog;

use std::path::PathBuf;

use egui::{Margin, Rect};
use vb_doc::commands::Command;
use vb_doc::model::Document;
use vb_doc::undo::UndoStack;
use vb_tools::Camera;

// 06-1 拆分后的私有转发:自由函数在 `external`,本文件与兄弟子模块
// (commands / canvas_input)仍经 `super::` 路径调用,调用点不动。
use canvas_input::{Drag, PenPt};
use external::re_sid_tree;

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
    // ── 阶段 5(05-2):X-4 变换工具族(06 篇 §3.10:单击设中心 → 拖拽变换)──
    /// 旋转 R:单击设中心,拖动旋转;Shift 约束 15°
    Rotate,
    /// 镜像 O:单击设中心,拖动决定镜像轴
    Mirror,
    /// 缩放 S:单击设中心,拖动缩放;Shift 等比;Alt 从对象中心
    Scale,
    /// 自由变换 E:拖选区四角之一,对角锚定缩放(透视变形不做,网页无对应)
    FreeTransform,
    // ── 阶段 5(05-2):X-5 曲线工具 ──
    /// 铅笔 N:自由绘制 → 按保真度容差抽稀为矢量路径(06 篇 §3.7)
    Pencil,
    /// 曲率:点击矢量路径段自动拟合平滑控制点(06 篇 §3.5)
    Curvature,
    // ── 阶段 5(05-2):09-C 切片 / 09-E 度量 ──
    /// 切片 Shift+K:拖框建立 data-vb-slice 切片(06 篇 §3.14)
    Slice,
    /// 度量:拖动量两点击点间距离 / 单击标注对象尺寸(06 篇 §六)
    Measure,
}

impl Tool {
    /// X-4 变换工具族判定(单击设中心 → 拖拽变换的共享入口)。
    pub fn is_transform_family(self) -> bool {
        matches!(
            self,
            Tool::Rotate | Tool::Mirror | Tool::Scale | Tool::FreeTransform
        )
    }
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
    pub(crate) theme_synced: Option<bool>,
    /// 当前主题(true=深色)。P2.7 支持浅色。
    pub(crate) theme_dark: bool,
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
    /// U-4:工具长按 400ms 展开的同族弹层(工具 + 锚点屏幕坐标;会话态)。
    family_popup: Option<(Tool, egui::Pos2)>,
    /// U-4:长按松手的那次 click 抑制标记(同帧消费,会话态)。
    family_popup_arm_release: bool,
    /// U-2:主右坞宽度记忆(px;workspace.json `dock_width`)。
    dock_width: f32,
    /// U-2:次级坞宽度记忆(px;workspace.json `sec_dock_width`)。
    sec_dock_width: f32,
    /// U-2:次级坞用户折叠偏好(<1600 强制折叠不回写;workspace.json)。
    sec_dock_collapsed: bool,
    /// H-1:动效总开关(默认开;workspace.json `motion_enabled`)。
    pub(crate) motion_enabled: bool,
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
    /// pub(crate):03-3 canvas_shot 读回阶段要按它换算 points→物理像素
    pub(crate) canvas_rect: Option<Rect>,
    /// pub(crate):03-3 canvas_shot 读回画布 GPU 纹理用
    pub(crate) gpu: Option<GpuCanvas>,
    frame_times: std::collections::VecDeque<f32>,
    /// 06-3:idle 帧率实测钩子的窗口起点(VB_FPS_LOG=1 启用;None = 关)。
    fps_log_at: Option<std::time::Instant>,
    /// 06-3:当前窗口内已发生的帧数(与 `fps_log_at` 配对)。
    fps_log_frames: u32,
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
    /// 命令面板键盘选择游标(04-1-3:↑↓ 移动,Enter 执行;下标对过滤后列表)
    palette_sel: usize,
    /// 上次保存/导入时的 rev(外部修改判定:磁盘变了但 rev 未动 → 自动采用)
    saved_rev: u64,
    /// 文件监听事件通道(Agent/外部编辑器改 HTML → 热重载,v0.6;携带事件路径)
    watcher_rx: Option<std::sync::mpsc::Receiver<Vec<std::path::PathBuf>>>,
    /// 抑制自身保存触发的重载
    last_self_write: Option<std::time::Instant>,
    /// 阶段 2:外壳协作通道(打开/新建/关闭/主页/主题/最近列表经外壳单点写;
    /// None = 无外壳的旧式独立构造,走就地打开/新建的兜底路径)。
    pub(crate) shell_tx: Option<std::sync::mpsc::Sender<crate::shell::ShellRequest>>,
    /// 阶段 2:本窗口的视口 id(外壳据此定位"哪个窗口要关闭/聚焦",02-5-2)。
    pub(crate) viewport_id: egui::ViewportId,
    /// 阶段 2:「新建项目 / 从模板新建」对话框(02-4-1;确认后发外壳开新窗口)。
    new_dialog: Option<crate::new_project::NewProjectDialog>,
    /// 03-3:画布出图(隐藏 `--canvas-shot` 参数;门禁采样用,None = 普通启动)。
    pub(crate) canvas_shot: Option<crate::canvas_shot::CanvasShotCfg>,
    /// 03-4:浏览器校对面板(开 + 任务 + UI 状态)。
    pub(crate) proofread_open: bool,
    pub(crate) proofread_target: usize,
    pub(crate) proofread_mode: u8,
    pub(crate) proofread_slider: f32,
    pub(crate) proofread_show_heat: bool,
    pub(crate) proofread: Option<panels::proofread::ProofreadJob>,
    /// 03-4 冒烟钩子:VB_PROOFREAD_AUTOSTART 已消费(只跑一次)。
    proofread_autostart_done: bool,
    /// 04-2:次级面板坞会话态(九面板摆放/组 Tab;持久化投影 `sec_*`)。
    pub(crate) sec: panel_dock::SecDockState,
    /// 04-4:开发者统计浮层(默认关;状态入 `workspace.json`)。
    dev_stats: bool,
    /// 04-4:提示条开关(默认开;教学/操作提示只在这条提示条出现)。
    hints: bool,
    /// 04-3:UI 缩放因子(乘在系统 DPI 之上;workspace.json `ui_scale`)。
    ui_scale: f32,
    /// 04-6:工具箱是否显示「未支持工具」(workspace.json `show_all_tools`)。
    pub(crate) show_all_tools: bool,
    /// 04-5:打开/新建项目后待执行的「适合窗口」(画布矩形就绪后的第一帧落)。
    fit_pending: bool,
    /// 冒烟钩子:VB_SMOKE_COMMAND 已消费(只跑一次;与
    /// VB_PROOFREAD_AUTOSTART 同款,不进 UI 面,供脚本截图驱动)。
    smoke_cmd_done: bool,
    // ── 阶段 7(数据安全批次:07-A ~ 07-E)──
    /// 07-A:自动保存间隔(秒;0 = 关;workspace.json `autosave_interval_secs`,
    /// 档位见 `crate::autosave::INTERVAL_STEPS`,默认 60s)。
    autosave_interval_secs: u32,
    /// 07-A:节拍计时(首次见脏起表;干净态清空)。
    autosave_last: Option<std::time::Instant>,
    /// 07-A:最近一次自动保存((Unix 秒, 时刻)—— 状态栏"已自动保存"印记)。
    autosave_at: Option<(i64, std::time::Instant)>,
    /// 07-B:待处理的崩溃恢复提示(打开项目时检出 `.vb-autosave/` 残留)。
    recover: Option<crate::autosave::RecoverPrompt>,
    /// 07-B:「查看差异」只读对比视图开关。
    recover_diff_open: bool,
    /// 07-B:VB_RECOVER_AUTO 钩子已消费(只跑一次;强杀 e2e 用)。
    recover_auto_done: bool,
    /// 07-D:历史面板显隐(次级坞「变换」组;开关真值)。
    history_open: bool,
    /// 07-D:待确认的历史回退(Some((目标深度, 将丢弃的重做步数)))。
    jump_confirm: Option<(usize, usize)>,
    /// 07-E:项目健康检查窗口开关。
    health_open: bool,
    /// 07-E:最近一次体检报告(打开窗口时若空则现算)。
    health_report: Option<Vec<health::HealthIssue>>,
    // ── 阶段 7b(编辑体验 / 外部协同批次:07-K / 07-R)──
    /// 07-K:资产面板显隐(次级坞「资产」组;开关真值)。
    assets_open: bool,
    /// 07-K:资产缩略图纹理缓存(键 = 资产相对路径;None = 解码失败,
    /// 不逐帧重试读盘)。
    asset_thumbs: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    /// 07-K:「替换引用」进行中的选择态(目标节点 + 候选资产)。
    asset_replace: Option<assets_panel::ReplacePick>,
    /// 07-K:资产行折算缓存(键 = doc.rev;文档没变不重读盘,「刷新」清空)。
    assets_cache: Option<(u64, Vec<assets_panel::AssetRow>)>,
    /// 07-R:最近一次外部改动印记(Agent/其他进程改盘触发热重载时记录;
    /// 未采用 = 本地有未保存编辑)。
    external_change: Option<ExternalChange>,
    /// 07-R:「最近外部改动」信息窗显隐(点状态栏印记开合)。
    external_info_open: bool,
    // ── 阶段 5(05-2:交互完整性六件)──
    /// X-4 变换工具族的变换中心(世界坐标):单击画布点设定;拖拽围绕它
    /// 施加旋转/镜像/缩放。切工具 / Esc / 选区变化时清空。
    pub(crate) xf_center: Option<(f64, f64)>,
    /// X-5 铅笔保真度容差(px;0–20,design/06 §3.7):RDP 抽稀容差,
    /// 「编辑 → 设置 → 铅笔保真度」循环档位;workspace.json `pencil_fidelity`。
    pub(crate) pencil_fidelity: f64,
    /// 09-E 像素预览(视图菜单):缩放 ≥ 阈值时画布对齐物理像素网格渲染提示。
    pub(crate) pixel_preview: bool,
    /// 09-E 度量结果(度量工具松手后保留,画布持续显示;Esc/再次度量清空):
    /// (dx, dy, 距离) 世界坐标 px。
    pub(crate) measure_result: Option<(f64, f64, f64)>,
    /// 09-E 度量标注的起点(与 `measure_result` 配对,画布持续显示)。
    pub(crate) measure_anchor: Option<(f64, f64)>,
    // ── 阶段 5(05-4-A2:对话框族)──
    /// 09-L:首选项九分类对话框(显隐 + 当前分类页)。
    pub(crate) prefs_open: bool,
    pub(crate) prefs_tab: usize,
    /// 09-L:键位方案编辑器显隐与编辑态(数据在 `crate::keymap`)。
    pub(crate) keymap_open: bool,
    pub(crate) keymap_editor: keymap_dialog::KeymapEditor,
    /// 09-L:用户键位方案(`keymap.json` 覆盖层)与有效键位集。
    pub(crate) keymap: crate::keymap::KeymapStore,
    pub(crate) keymap_live: Vec<crate::keymap::LiveBinding>,
    /// 09-M:文档设置对话框显隐(编辑态在 `doc_settings` 模块)。
    pub(crate) doc_settings_open: bool,
    /// 09-M:文档设置编辑态(None = 未打开;打开时从文档快照)。
    pub(crate) doc_settings_state: Option<doc_settings::DocSettingsState>,
    /// 09-N:外部冲突三方对比对话框显隐。
    pub(crate) conflict_open: bool,
    /// 09-N:对比窗口的差分对选择(默认 磁盘 ↔ 内存)。
    pub(crate) conflict_diff_pair: conflict_dialog::DiffPair,
    /// 09-O:字体缺失专项对话框(None = 无待处理缺失)。
    pub(crate) font_dialog: Option<font_dialog::FontDialogState>,
    /// X-7:新建工作区对话框显隐(保存/应用/删除预设)。
    pub(crate) workspace_dialog_open: bool,
    /// X-7:工作区对话框的名称输入(保存当前布局为预设)。
    pub(crate) workspace_dialog_name: String,
    /// 首选项「参考线与网格」:网格基础间距 px(画布网格分级基数;
    /// workspace.json `grid_size`)。
    pub(crate) grid_size: f64,
    /// 首选项「数据」:自动保存快照保留份数(1–3;workspace.json `autosave_keep`)。
    pub(crate) autosave_keep: u32,
    /// 首选项「画板」:新画板默认预设(`panels::artboards::AB_PRESETS` 下标;
    /// workspace.json `artboard_preset`)。
    pub(crate) artboard_preset: usize,
    // ── 阶段 5(05-5:断点与响应式 / 伪类)──
    /// 当前预览断点(None = 默认画布);状态栏切换器与
    /// `view.breakpoint_cycle` 写入。会话态(不持久化)。
    pub(crate) active_breakpoint: Option<u32>,
    /// 属性面板样式编辑目标状态:0 = 正常,1 = hover(05-5 伪类最小闭环)。
    /// 会话态(不持久化)。
    pub(crate) style_state: u8,
    // ── 阶段 5(05-9:动效时间轴,09-I)──
    /// 时间轴面板显隐(次级坞「时间轴」组;开关真值)。
    pub(crate) timeline_open: bool,
    /// 动画预览:播放中(会话态,不持久化)。
    pub(crate) anim_playing: bool,
    /// 动画预览:播放头(秒;0 = 静态呈现)。
    pub(crate) anim_time: f64,
    /// 动画预览:循环(到尾回绕 / 停在末帧)。
    pub(crate) anim_loop: bool,
    /// 上帧时钟(egui time;dt 推进用,暂停即清)。
    pub(crate) anim_last_clock: Option<f64>,
    /// 动画实例缓存(键 = doc.rev;预览逐帧免重复解析 @keyframes)。
    pub(crate) anim_cache: Option<(
        u64,
        std::collections::HashMap<String, vb_kiln::anim::NodeAnim>,
    )>,
    /// 时间轴选中关键帧(轨道下标 = `TrackProp::ALL` 序,帧下标)。
    pub(crate) anim_sel: Option<(usize, usize)>,
    /// 时间轴关键帧拖拽进行中(undo 会话开合标记)。
    pub(crate) anim_drag_open: bool,
    // ── 阶段 5(05-10:插件系统,09-J;ADR-VB-L12)──
    /// 插件宿主(注册表 / 权限闸门 / 状态机 / 子进程;逻辑在
    /// `vb_plugin` crate,本侧经 `app/plugins.rs` 的 HostServices 端口
    /// 提供 runCommand / 文档只读投影 / 导出)。
    pub(crate) plugin_host: vb_plugin::host::PluginHost,
    /// 插件管理窗口显隐(「编辑 → 插件管理…」)。
    pub(crate) plugins_mgr_open: bool,
    /// 插件坞面板显隐(次级坞「插件」组;Running 插件的注册面板)。
    pub(crate) plugins_panel_open: bool,
    /// 待授权插件(首次启用授权弹窗编辑态;None = 弹窗关闭)。
    pub(crate) plugin_auth: Option<plugins::PluginAuthState>,
    /// 管理窗口里展开日志环的插件 id 集(会话态)。
    pub(crate) plugin_logs_open: std::collections::HashSet<String>,
    /// 插件坞面板当前选中的插件下标(多 Running 插件时)。
    pub(crate) plugin_panel_sel: usize,
    /// 插件面板输入框草稿(键 = "插件/面板/输入id";会话态)。
    pub(crate) plugin_input_buf: std::collections::HashMap<String, String>,
}

pub(crate) struct GpuCanvas {
    renderer: vello::Renderer,
    /// tex pub(crate):03-3 canvas_shot 读回画布纹理用
    pub(crate) tex: Option<(wgpu::Texture, wgpu::TextureView, [u32; 2], egui::TextureId)>,
}

impl VellumApp {
    /// 09-E 像素预览的最低缩放阈值(倍):低于它画布远小于物理像素,
    /// 对齐无意义(design/06 §六「按 1:1 设备像素光栅显示」的适用下限)。
    pub(crate) const PIXEL_PREVIEW_MIN_ZOOM: f64 = 8.0;

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

    /// 是否有未保存修改(窗口标题 `*` 与关闭确认的判据,02-5-4/02-5-6)。
    pub fn is_dirty(&self) -> bool {
        self.doc.rev != self.saved_rev
    }

    /// 命令 id 的**当前**键位文本(菜单/命令面板展示):
    /// 用户键位方案(`keymap.json`)覆盖优先,其次静态注册表。
    /// 菜单渲染统一走这里,禁止直接查静态表(05-4-A2 键位编辑器配套)。
    pub(crate) fn menu_key_text(&self, id: &str) -> Option<String> {
        crate::keymap::key_text_for(&self.keymap, id)
    }

    /// 显示名(窗口标题用):文档标题,空则退目录名,再退「未命名」。
    pub fn display_name(&self) -> String {
        if !self.doc.meta.title.trim().is_empty() {
            return self.doc.meta.title.clone();
        }
        if let Some(dir) = &self.project_dir {
            if let Some(name) = dir.file_name() {
                return name.to_string_lossy().to_string();
            }
        }
        "未命名".into()
    }

    /// 外壳主题广播(02-3-5:主页与所有窗口跟随同一主题)。
    pub fn set_theme_dark(&mut self, dark: bool) {
        if self.theme_dark != dark {
            self.theme_dark = dark;
            self.theme_synced = None; // 强制下一帧向 egui 重新同步
        }
    }

    /// 本窗口所属视口 id(外壳定位用)。
    pub fn viewport_id(&self) -> egui::ViewportId {
        self.viewport_id
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
        // 05-2:X-4 变换中心与度量结果是**工具会话态**,切走即清
        // (回到同族另一工具也重设,避免拖出与当前工具无关的中心)。
        if !self.tool.is_transform_family() {
            self.xf_center = None;
        }
        self.measure_result = None;
        self.measure_anchor = None;
    }

    /// 04-3:UI 缩放档位表(±档位即可;完整首选项九分类属阶段 5)。
    /// 1.0 = 跟随系统 DPI,不额外缩放。
    const UI_SCALE_STEPS: [f32; 7] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0];

    /// 步进 UI 缩放档位(`dir`:+1 增大 / -1 减小),持久化并给出状态提示。
    fn step_ui_scale(&mut self, dir: i32) {
        let steps = Self::UI_SCALE_STEPS;
        let cur = self.ui_scale;
        let next = match steps.iter().position(|&s| (s - cur).abs() < 1e-3) {
            Some(i) => {
                let j = (i as i32 + dir).clamp(0, steps.len() as i32 - 1) as usize;
                steps[j]
            }
            // 当前值不在档位表里(手改 workspace.json):按方向取最近档
            None => {
                if dir > 0 {
                    steps
                        .iter()
                        .copied()
                        .find(|&s| s > cur)
                        .unwrap_or_else(|| *steps.last().unwrap_or(&1.0))
                } else {
                    steps
                        .iter()
                        .rev()
                        .copied()
                        .find(|&s| s < cur)
                        .unwrap_or(steps[0])
                }
            }
        };
        self.ui_scale = next;
        self.save_workspace();
        self.say(format!(
            "界面缩放 {}%(叠加在系统 DPI 之上;视图 → 界面缩放可调)",
            (next * 100.0) as i64
        ));
    }
}

// ---------- 输入(派发在 dispatch*)----------

// ---------- 辅助 ----------

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

// ─────────────────────── 04-5 / 04-3 门禁(单测) ───────────────────────
