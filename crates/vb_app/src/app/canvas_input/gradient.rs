//! 渐变工具:Alt+单击移除渐变、拖拽批注方向(实时应用)、松手留存批注。
//!
//! 06-1 自 `canvas_input.rs` 按工具族拆出(纯搬移,零行为变化)。

use vb_doc::commands::Command;

use super::Drag;
use crate::app::{Tool, VellumApp};

impl VellumApp {
    /// 渐变工具单击:双击批注色标优先;Alt = 移除渐变恢复纯色。返回 true = 已消费。
    pub(super) fn gradient_click(
        &mut self,
        response: &egui::Response,
        ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
        // 渐变单击:Alt = 移除 background-image 恢复纯色(06 篇 §5.5)
        if response.clicked() && self.tool == Tool::Gradient {
            // 双击批注上的色标 → 选中该色标并打开渐变面板(05-2-3)
            if response.double_clicked() {
                if let Some(pp) = response.interact_pointer_pos() {
                    let pl = pp - rect.min;
                    if self.grad_annot_double_click((pl.x, pl.y)) {
                        return true;
                    }
                }
            }
            let alt = ctx.input(|i| i.modifiers.alt);
            if alt {
                let targets = self.selection.clone();
                if targets.is_empty() {
                    self.status = "渐变:先选中对象".into();
                } else {
                    for sid in targets {
                        let Some(t) = self.doc.find_by_sid(&sid) else {
                            continue;
                        };
                        let mut style = self.doc.nodes.get(t).unwrap().style.clone();
                        let before = style.len();
                        style.retain(|d| d.prop != "background-image");
                        if style.len() != before {
                            self.exec(Command::SetStyle {
                                sid,
                                new: style,
                                old: None,
                            });
                        }
                    }
                    self.status = "已移除渐变(恢复纯色)".into();
                }
            } else if self.selection.is_empty() {
                self.status = "渐变:先选中对象,再拖动设定方向".into();
            }
            return true;
        }
        false
    }

    /// 渐变拖拽起手:置批注态(需选区)。
    pub(super) fn drag_begin_gradient(&mut self, wx: f64, wy: f64) {
        // 渐变批注者:需要选区;拖动方向 = 渐变方向
        if self.selection.is_empty() {
            self.status = "渐变:先选中对象".into();
        } else {
            self.drag = Drag::GradientAnnotate {
                start: (wx, wy),
                end: (wx, wy),
                angle: 0.0,
            };
        }
    }

    /// 渐变拖动:方向 = 起点→光标,实时应用。返回 true = 本帧已消费。
    pub(super) fn drag_move_gradient(&mut self, p: egui::Vec2) -> bool {
        if let Drag::GradientAnnotate { start, .. } = &self.drag {
            let (wx, wy) = self.camera.screen_to_world(p.x as f64, p.y as f64);
            let dx = wx - start.0;
            let dy = wy - start.1;
            if dx.hypot(dy) >= 2.0 {
                // CSS 角度:0° = 向上,90° = 向右(Y 轴向下,故取 -dy)
                let angle = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
                if let Drag::GradientAnnotate {
                    angle: slot, end, ..
                } = &mut self.drag
                {
                    *slot = angle;
                    *end = (wx, wy);
                }
                self.apply_gradient_to_selection(angle);
            }
            return true;
        }
        false
    }

    /// 渐变松手:批注留存(双击色标可改色)。
    pub(super) fn end_gradient_annotate(&mut self, start: (f64, f64), end: (f64, f64), angle: f64) {
        // 批注保留(05-2-3):双击其上的色标可改色
        self.gradient_annot = Some((start.0, start.1, end.0, end.1));
        self.status =
            format!("线性渐变已应用 {angle:.0}°(起=原填充 → 止=#ffffff;双击色标改色;Alt+单击移除)");
    }
}
