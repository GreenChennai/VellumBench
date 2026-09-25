//! 取色 / 剪切工具:吸管单击取样式、剪刀在矢量锚点处剪开。
//!
//! 06-1 自 `canvas_input.rs` 按工具族拆出(纯搬移,零行为变化)。

use crate::app::{Tool, VellumApp};

impl VellumApp {
    /// 吸管单击:取色/取样式应用到选区(Alt = 全部样式)。返回 true = 已消费。
    pub(super) fn eyedropper_click(
        &mut self,
        response: &egui::Response,
        ctx: &egui::Context,
    ) -> bool {
        // 吸管单击:取色/取样式应用到选区(06 篇 §5.4;Alt = 全部样式)
        if response.clicked() && self.tool == Tool::Eyedropper {
            let alt = ctx.input(|i| i.modifiers.alt);
            self.eyedropper_pick(alt);
            return true;
        }
        false
    }

    /// 剪刀单击:在矢量锚点处剪开。返回 true = 已消费。
    pub(super) fn scissors_click(
        &mut self,
        response: &egui::Response,
        _ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
        // 剪刀单击:在矢量锚点处剪开(闭路开口 / 开路分段;06 篇 §5.3)
        if response.clicked() && self.tool == Tool::Scissors {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.scissors_cut(wx, wy, 8.0 / self.camera.zoom);
            }
            return true;
        }
        false
    }
}
