//! 变换工具族:手柄 / 旋转命中拖拽起手、Resize / Rotate 拖动与松手收束,
//! 以及手柄命中、缩放几何两个纯辅助函数。
//!
//! 06-1 自 `canvas_input.rs` 按工具族拆出(纯搬移,零行为变化)。

use egui::Rect;
use vb_doc::commands::Command;
use vb_doc::model::Geom;

use super::{Drag, Tool};
use crate::app::{fmt_deg, parse_rotate_deg, set_style_prop, VellumApp};

impl VellumApp {
    /// 手柄 / 旋转命中(单选优先):命中即置对应拖拽态。返回 true = 已消费。
    pub(super) fn drag_begin_handle(
        &mut self,
        p: egui::Vec2,
        rect: egui::Rect,
        sel_bbox_screen: Option<(egui::Rect, String)>,
    ) -> bool {
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
                        return true;
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
                return true;
            }
        }
        false
    }

    /// Resize 拖动:按手柄改几何。返回 true = 本帧已消费。
    pub(super) fn drag_move_resize(&mut self, p: egui::Vec2, shift: bool, alt: bool) -> bool {
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
                old_declared: None,
            });
            if let Drag::Resize { moved, .. } = &mut self.drag {
                *moved = true;
            }
            return true;
        }
        false
    }

    /// Rotate 拖动:绕中心旋转(Shift 15° 吸附)。返回 true = 本帧已消费。
    pub(super) fn drag_move_rotate(&mut self, p: egui::Vec2, shift: bool) -> bool {
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
                style = set_style_prop(style, "transform", &format!("rotate({}deg)", fmt_deg(deg)));
                self.exec(Command::SetStyle {
                    sid,
                    new: style,
                    old: None,
                });
                if let Drag::Rotate { moved, .. } = &mut self.drag {
                    *moved = true;
                }
            }
            return true;
        }
        false
    }

    /// 缩放松手:把缩放记进「上一次变换」(Mod+D 重放用)。
    pub(super) fn end_resize(&mut self, sid: String, moved: bool, start_geom: Geom) {
        if !moved {
            return;
        }
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            return;
        };
        let g = self.doc.nodes.get(nid).unwrap().geom;
        let kx = if start_geom.w.abs() > 1e-9 {
            g.w / start_geom.w
        } else {
            1.0
        };
        let ky = if start_geom.h.abs() > 1e-9 {
            g.h / start_geom.h
        } else {
            1.0
        };
        // 缩放可能同时改变位置(拖边/角):`replay` 先按比例缩放再位移
        self.remember_transform(crate::app::transform_panel::TransformDelta {
            dx: g.x - start_geom.x,
            dy: g.y - start_geom.y,
            kx,
            ky,
            d_angle: 0.0,
        });
    }

    /// 旋转松手:把角度记进「上一次变换」。
    pub(super) fn end_rotate(&mut self, sid: String, moved: bool, start_deg: f64) {
        if !moved {
            return;
        }
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            return;
        };
        let cur = self
            .doc
            .nodes
            .get(nid)
            .and_then(|n| n.style_get("transform"))
            .map(|t| crate::app::transform_panel::parse_transform(t).0)
            .unwrap_or(0.0);
        self.remember_transform(crate::app::transform_panel::TransformDelta {
            d_angle: cur - start_deg,
            ..crate::app::transform_panel::TransformDelta::translate(0.0, 0.0)
        });
    }
}

/// 8 手柄命中:返回 0=NW 1=N 2=NE 3=E 4=SE 5=S 6=SW 7=W;未命中 None。
pub(super) fn hit_handle(p: egui::Pos2, bbox: Rect) -> Option<u8> {
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
