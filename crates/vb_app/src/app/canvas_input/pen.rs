//! 钢笔工具:单击落锚(近起点闭合)、按下拖出平滑点手柄、拖动更新出柄。
//!
//! 06-1 自 `canvas_input.rs` 按工具族拆出(纯搬移,零行为变化)。

use super::PenPt;
use crate::app::{Tool, VellumApp};

impl VellumApp {
    /// 钢笔:单击 / 拖拽起手 / 拖动三段(各自消费即停)。返回 true = 已消费。
    pub(super) fn pen_press_and_drag(
        &mut self,
        response: &egui::Response,
        _ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
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
                        return true;
                    }
                }
                self.pen_points.push(PenPt::corner(wx, wy));
                // (request_repaint 由 egui 输入事件自动触发)
            }
            return true;
        }
        // 钢笔拖拽(06 篇 §5.3 平滑点):按下锚点后拖出出手柄,入柄镜像
        if response.drag_started() && self.tool == Tool::Pen {
            if let Some(p0) = response.interact_pointer_pos() {
                let pl = p0 - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.pen_points.push(PenPt::corner(wx.round(), wy.round()));
            }
            return true;
        }
        if response.dragged() && self.tool == Tool::Pen && !self.pen_points.is_empty() {
            if let Some(p0) = response.interact_pointer_pos() {
                let pl = p0 - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                let last = self.pen_points.last_mut().unwrap();
                last.h_out = Some((wx, wy));
            }
            return true;
        }
        false
    }
}
