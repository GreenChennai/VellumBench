//! 视图工具族:缩放工具单击、滚轮缩放/滚动、光标反馈、平移(中键 /
//! Space+左键 / 抓手)、区域缩放,以及标尺参考线的抓取 / 拖动 / 删除。
//!
//! 06-1 自 `canvas_input.rs` 按工具族拆出(纯搬移,零行为变化)。

use egui::{pos2, vec2, PointerButton, Rect};
use vb_ui::cursor as vbcursor;

use super::transform::hit_handle;
use super::{Drag, Tool};
use crate::app::VellumApp;

impl VellumApp {
    /// 缩放工具单击:放大 / Alt+单击缩小。返回 true = 已消费。
    pub(super) fn zoom_click(
        &mut self,
        response: &egui::Response,
        ctx: &egui::Context,
        rect: Rect,
    ) -> bool {
        // 缩放工具单击:放大 / Alt+单击缩小(02 篇 §5.6)
        if response.clicked() && self.tool == Tool::Zoom {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let alt_click = ctx.input(|i| i.modifiers.alt);
                let f = if alt_click { 1.0 / 1.25 } else { 1.25 };
                self.camera.zoom_at(pl.x as f64, pl.y as f64, f);
                self.status = format!("缩放 {}%", (self.camera.zoom * 100.0) as i64);
            }
            return true;
        }
        false
    }

    /// 光标世界坐标(指针先转画布本地)。
    pub(super) fn track_cursor_world(&mut self, response: &egui::Response, rect: Rect) {
        // 光标世界坐标(指针先转画布本地)
        if let Some(p) = response.hover_pos() {
            let pl = p - rect.min;
            let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
            self.cursor_world = (wx, wy);
        }
    }

    /// 滚轮缩放与滚动(悬停时;H-2 修饰键矩阵):
    ///
    /// | 输入 | 行为 |
    /// |---|---|
    /// | Ctrl(⌘)+滚轮 | 以**光标为锚**缩放(设计/主流图像软件通用) |
    /// | Alt+滚轮 | 以光标为锚缩放(既有行为,保留) |
    /// | Shift+滚轮 | 水平平移(纵向滚距折算横向) |
    /// | 滚轮 / 触摸板横扫 | 纵向 + 横向平移 |
    ///
    /// 锚点正确性由 `Camera::zoom_at` 的单测锁(`camera_zoom_at_keeps_cursor_point`,
    /// vb_tools)—— 本层只负责把修饰键路由到 zoom_at / pan。
    pub(super) fn wheel_zoom_scroll(
        &mut self,
        response: &egui::Response,
        ctx: &egui::Context,
        rect: Rect,
    ) {
        let (scroll, mods) = ctx.input(|i| (i.smooth_scroll_delta, i.modifiers));
        if response.hovered() && (scroll.y != 0.0 || scroll.x != 0.0) {
            let zoom_wanted = mods.alt || mods.ctrl || mods.command;
            if zoom_wanted {
                // 光标锚点缩放:指针位置先转画布本地坐标
                if let Some(p) = response.hover_pos() {
                    let pl = p - rect.min;
                    let steps = scroll.y + scroll.x; // 触摸板横扫也计缩放步进
                    self.camera
                        .zoom_at(pl.x as f64, pl.y as f64, (-(steps as f64) / 400.0).exp());
                }
            } else if mods.shift {
                // Shift+滚轮 = 水平平移:纵向滚距折算成横向(触控板的
                // 横向分量一并叠加,方向与内容跟随惯例一致)
                self.camera.pan_x += (scroll.y + scroll.x) as f64;
            } else {
                self.camera.pan_y += scroll.y as f64;
                self.camera.pan_x += scroll.x as f64;
            }
        }
    }

    /// 光标反馈:平移手掌 / 工具光标映射 / 手柄悬停方向光标。
    pub(super) fn cursor_feedback(
        &mut self,
        response: &egui::Response,
        ctx: &egui::Context,
        rect: Rect,
    ) {
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
                // 直接选择:与「选择」必须**看得出不同**(07-3-5)。egui 没有
                // Illustrator 的空心箭头/锚点光标,`Cell`(方框十字)是最接近
                // 「锚点」语义的内置图标 —— 与 `cursor.rs` 的旋转光标同属
                // **已知妥协**,在 `cursor` 模块的测试里钉住。
                Tool::DirectSelect => ctx.set_cursor_icon(vbcursor::DIRECT_SELECT),
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
    }

    /// 平移拖拽(中键 或 Space+左键)。返回 true = 本帧已消费。
    pub(super) fn pan_drag(
        &mut self,
        response: &egui::Response,
        ctx: &egui::Context,
        pan_wanted: bool,
    ) -> bool {
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
            return true;
        }
        false
    }

    /// 抓手工具:拖拽起手置平移态。
    pub(super) fn drag_begin_pan(&mut self) {
        self.drag = Drag::Pan {
            start_pan: vec2(self.camera.pan_x as f32, self.camera.pan_y as f32),
        };
    }

    /// 缩放工具:拖拽起手置区域缩放态。
    pub(super) fn drag_begin_zoom_region(&mut self, p: egui::Vec2) {
        self.drag = Drag::ZoomRegion { start: p, cur: p };
    }

    /// 标尺参考线拖拽起手:从标尺拖出新建 / ±3px 内抓取既有线。返回 true = 已消费。
    pub(super) fn drag_begin_guides(&mut self, p: egui::Vec2) -> bool {
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
                // 命中已有参考线(±3px)一律抓取,与指针在不在标尺条内
                // 无关;否则水平参考线拖到画布中部后,方向 guard 不匹配
                // 会凭空新建一条垂直线(G7)
                let idx = match near_line {
                    Some(i) => i,
                    None if in_top => {
                        self.guides.push((true, wy));
                        self.guides.len() - 1
                    }
                    None => {
                        self.guides.push((false, wx));
                        self.guides.len() - 1
                    }
                };
                self.drag = Drag::Guide { idx };
                return true;
            }
        }
        false
    }

    /// 参考线拖动:实时跟随光标(世界坐标)。返回 true = 本帧已消费。
    pub(super) fn drag_move_guide(&mut self, p: egui::Vec2) -> bool {
        if let Drag::Guide { idx } = &self.drag {
            let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
            if let Some(g) = self.guides.get_mut(*idx) {
                g.1 = if g.0 { wy } else { wx };
            }
            return true;
        }
        false
    }

    /// 参考线松手:拖回标尺条内 / 画布外 = 删除。
    pub(super) fn end_guide(&mut self, idx: usize, response: &egui::Response, rect: Rect) {
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

    /// 区域缩放松手:区域充满视口。
    pub(super) fn end_zoom_region(&mut self, start: egui::Vec2, cur: egui::Vec2, rect: Rect) {
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
}
