//! 画布输入(S1-a 自 app.rs 机械搬移;06-1 按**工具族 + 阶段管线**拆分,
//! 零行为变化)。本文件是唯一入口 [`VellumApp::handle_canvas_input`]:
//! 按原函数的顺序依次调用各阶段处理器(返回 true = 已消费,停止后续;
//! 与原早退等价)。工具族实现:选择 `select`、形状 `shape`、变换
//! `transform`、视图/参考线 `view`、钢笔 `pen`、渐变 `gradient`、
//! 取色/剪切 `sense`、吸附 `snap`。
//!
//! 手势状态模型(`Drag` / `PenPt`)也在此 —— 它们是画布输入的领域类型。

use egui::{pos2, Rect, Vec2};
use vb_doc::model::Geom;

use crate::app::{Tool, VellumApp};

#[derive(Debug, Clone, Copy)]
pub(crate) struct PenPt {
    pub(crate) anchor: (f64, f64),
    pub(crate) h_out: Option<(f64, f64)>,
}

impl PenPt {
    pub(crate) fn corner(x: f64, y: f64) -> Self {
        PenPt {
            anchor: (x, y),
            h_out: None,
        }
    }
    /// 入手柄 = anchor 关于 anchor 的镜像(out 的反向延长)。
    pub(crate) fn h_in(&self) -> Option<(f64, f64)> {
        self.h_out
            .map(|(hx, hy)| (2.0 * self.anchor.0 - hx, 2.0 * self.anchor.1 - hy))
    }
}

pub(crate) enum Drag {
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
    // ── 阶段 5(05-2):X-4 变换工具族(单击设中心 → 拖拽变换)──
    /// 旋转工具:sids 各自的起始 rotate(θ) 与公共中心/起始角。
    ToolRotate {
        sids: Vec<String>,
        center: (f64, f64),
        start_angle: f64,
        start_degs: Vec<f64>,
        moved: bool,
    },
    /// 镜像工具:松手按拖动方向定轴,位置反射 + scale(±1) 内容翻转。
    ToolMirror {
        center: (f64, f64),
        start: (f64, f64),
        cur: (f64, f64),
        moved: bool,
    },
    /// 缩放工具:绕中心逐轴距离比缩放(Shift 等比;Alt 以对象中心为轴)。
    ToolScale {
        center: (f64, f64),
        start: (f64, f64),
        cur: (f64, f64),
        moved: bool,
    },
    /// 自由变换:拖选区包围盒某角,对角锚定缩放全部选中(透视不做)。
    FreeTransform {
        bounds: (f64, f64, f64, f64),
        corner: u8,
        cur: (f64, f64),
        moved: bool,
    },
    // ── 阶段 5(05-2):X-5 铅笔 / 09-E 度量 ──
    /// 铅笔:自由绘制点列(世界坐标);松手按保真度容差抽稀成路径。
    PencilStroke {
        pts: Vec<(f64, f64)>,
        last: (f64, f64),
    },
    /// 度量:起点 → 当前点(世界坐标);松手把 (dx,dy,距离) 存进会话态。
    Measure {
        start: (f64, f64),
        cur: (f64, f64),
    },
}

mod gradient;
mod pen;
mod select;
mod sense;
mod shape;
mod snap;
mod transform;
mod view;
// ── 阶段 5(05-2):X-4 变换工具族 / X-5 铅笔·曲率 / 09-E 度量 ──
mod curves;
mod xform;

impl VellumApp {
    pub(crate) fn handle_canvas_input(
        &mut self,
        response: &egui::Response,
        ctx: egui::Context,
        rect: Rect,
    ) {
        // ── 阶段一:单击即消费的工具(缩放/吸管/渐变/剪刀/单击创建/钢笔)──
        if self.zoom_click(response, &ctx, rect) {
            return;
        }
        if self.eyedropper_click(response, &ctx) {
            return;
        }
        if self.gradient_click(response, &ctx, rect) {
            return;
        }
        if self.scissors_click(response, &ctx, rect) {
            return;
        }
        // X-4 变换工具族:单击画布点 = 设定变换中心(06 篇 §3.10)
        if self.xform_click(response, &ctx, rect) {
            return;
        }
        // X-5 曲率:单击矢量路径 = 自动拟合平滑控制点
        if self.curvature_click(response, &ctx, rect) {
            return;
        }
        // 09-E 度量:单击对象 = 标注其尺寸(拖动量距在拖拽管线处理)
        if self.measure_click(response, &ctx, rect) {
            return;
        }
        if self.create_click(response, &ctx, rect) {
            return;
        }
        if self.pen_press_and_drag(response, &ctx, rect) {
            return;
        }
        // 直接选择:单击选顶点 / 拖拽锚点;**不消费后续事件**(原语义:无 return)
        self.direct_select_click_and_drag(response, rect);
        // ── 阶段二:悬停反馈与共享量 ──
        self.track_cursor_world(response, rect);
        self.wheel_zoom_scroll(response, &ctx, rect);

        let mods = ctx.input(|i| i.modifiers);
        let alt = mods.alt;
        let shift = mods.shift;
        let ctrl = mods.ctrl || mods.command;

        // 本帧参考线清空(绘制在 overlay)
        self.smart_guides.clear();
        self.cursor_feedback(response, &ctx, rect);
        // ── 阶段三:平移(中键 / Space+左键)──
        let pan_wanted = self.space_down || self.tool == Tool::Hand;
        if self.pan_drag(response, &ctx, pan_wanted) {
            return;
        }
        // ── 阶段四:双击(隔离 / 文本编辑)──
        if self.double_click_edit(response, rect) {
            return;
        }
        // ── 阶段五:选中对象的屏幕 bbox(用于手柄/旋转命中)──
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
        // ── 阶段六:拖拽开始(参考线 → 手柄/旋转 → 按工具族;顺序保持)──
        if response.drag_started() {
            let Some(p0) = response.interact_pointer_pos() else {
                return;
            };
            let p = p0 - rect.min; // 画布本地
            if self.drag_begin_guides(p) {
                return;
            }
            if self.drag_begin_handle(p, rect, sel_bbox_screen.clone()) {
                return;
            }
            // 按工具族置拖拽初态(臂序保持;大臂委托各工具族文件)
            let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
            match self.tool {
                Tool::Hand => {
                    self.drag_begin_pan();
                }
                Tool::GroupSelect => {
                    self.drag_begin_group_select(p, wx, wy, shift);
                }
                Tool::Select => {
                    self.drag_begin_select(p, wx, wy, alt, shift);
                }
                Tool::Rect | Tool::Ellipse | Tool::Line | Tool::Text | Tool::Artboard => {
                    self.drag_begin_create(p);
                }
                Tool::Zoom => {
                    self.drag_begin_zoom_region(p);
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
                Tool::Gradient => {
                    self.drag_begin_gradient(wx, wy);
                }
                Tool::Scissors => {
                    // 剪刀:单击由 clicked() 处理(锚点剪开)
                }
                // ── 阶段 5(05-2)──
                Tool::Rotate | Tool::Mirror | Tool::Scale => {
                    self.drag_begin_xform(wx, wy);
                }
                Tool::FreeTransform => {
                    self.drag_begin_free_transform(wx, wy);
                }
                Tool::Pencil => {
                    self.drag_begin_pencil(wx, wy);
                }
                Tool::Measure => {
                    self.drag = Drag::Measure {
                        start: (wx, wy),
                        cur: (wx, wy),
                    };
                }
                Tool::Slice | Tool::Curvature => {
                    // 切片:复用 Drag::Create(松手 end_create 按工具落地);
                    // 曲率:单击已消费,拖拽防穿透
                }
            }
        }
        // ── 阶段七:拖拽持续(参考线/渐变/缩放/旋转消费;移动不消费)──
        if response.dragged() {
            let Some(p0) = response.interact_pointer_pos() else {
                return;
            };
            let p = p0 - rect.min;
            if self.drag_move_guide(p) {
                return;
            }
            if self.drag_move_gradient(p) {
                return;
            }
            // 缩放 / 旋转(它们自成状态,不与 MoveObj 共路)
            match &self.drag {
                Drag::Resize { .. } | Drag::Rotate { .. } => {}
                _ => {}
            }
            if self.drag_move_resize(p, shift, alt) {
                return;
            }
            if self.drag_move_rotate(p, shift) {
                return;
            }
            // ── 阶段 5(05-2):X-4 工具族 / 铅笔 / 度量的拖拽持续 ──
            let (wxm, wym) = self.camera.screen_to_world(p.x as f64, p.y as f64);
            if self.drag_move_xform(wxm, wym, p, shift, alt) {
                return;
            }
            if self.drag_move_pencil(wxm, wym) {
                return;
            }
            if let Drag::Measure { cur, .. } = &mut self.drag {
                *cur = (wxm, wym);
                return;
            }
            self.drag_move_object(p, shift, ctrl);
        }
        // ── 阶段八:拖拽结束(按 Drag 变体收束;臂序保持)──
        if response.drag_stopped() {
            match std::mem::replace(&mut self.drag, Drag::None) {
                Drag::GradientAnnotate { start, end, angle } => {
                    self.end_gradient_annotate(start, end, angle);
                }
                Drag::Guide { idx } => {
                    self.end_guide(idx, response, rect);
                }
                Drag::Marquee { start, cur } => {
                    self.end_marquee(start, cur, shift);
                }
                Drag::Create { start, cur } => {
                    self.end_create(start, cur, shift, alt);
                }
                Drag::ZoomRegion { start, cur } => {
                    self.end_zoom_region(start, cur, rect);
                }
                Drag::MoveObj {
                    sid,
                    moved,
                    start_geom,
                    ..
                } => {
                    self.end_move_obj(sid, moved, start_geom);
                }
                Drag::Resize {
                    sid,
                    moved,
                    start_geom,
                    ..
                } => {
                    self.end_resize(sid, moved, start_geom);
                }
                Drag::Rotate {
                    sid,
                    moved,
                    start_deg,
                    ..
                } => {
                    self.end_rotate(sid, moved, start_deg);
                }
                // ── 阶段 5(05-2)──
                Drag::ToolRotate {
                    sids,
                    start_degs,
                    center,
                    moved,
                    ..
                } => {
                    self.end_tool_rotate(sids, start_degs, center, moved);
                }
                Drag::ToolMirror {
                    center,
                    start,
                    cur,
                    moved,
                    ..
                } => {
                    self.end_tool_mirror(center, start, cur);
                    let _ = moved;
                }
                Drag::ToolScale {
                    center,
                    start,
                    cur,
                    moved,
                    ..
                } => {
                    self.end_tool_scale(center, start, cur, moved);
                }
                Drag::FreeTransform { moved, .. } => {
                    self.end_free_transform(moved);
                }
                Drag::PencilStroke { pts, .. } => {
                    self.end_pencil(pts);
                }
                Drag::Measure { start, cur } => {
                    self.end_measure(start, cur);
                }
                _ => {}
            }
        }
    }
}
