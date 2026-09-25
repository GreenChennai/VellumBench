//! 形状工具族:单击默认尺寸创建(Rect/Ellipse/Line/Text/Artboard)、
//! 拖拽创建起手与松手落地(拖框区域文本 / 画板 / 形状)。
//!
//! 06-1 自 `canvas_input.rs` 按工具族拆出(纯搬移,零行为变化)。

use vb_doc::model::Geom;

use super::Drag;
use crate::app::{Tool, VellumApp};

impl VellumApp {
    /// 单击创建(默认尺寸;与拖框创建互斥)。返回 true = 已消费。
    pub(super) fn create_click(
        &mut self,
        response: &egui::Response,
        _ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
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
                return true;
            }
        }
        // 切片工具单击(06 篇 §3.14「从选区创建」):有选区 → 以选区包围盒
        // 建切片;无选区 → 提示拖框(绝不点了没反应)
        if response.clicked() && self.tool == Tool::Slice {
            if !self.selection.is_empty() {
                self.slice_from_selection();
            } else {
                self.status = "切片:拖框建立切片,或先选中对象再单击(从选区建立)".into();
            }
            return true;
        }
        false
    }

    /// 形状工具拖拽起手:置创建态。
    pub(super) fn drag_begin_create(&mut self, p: egui::Vec2) {
        self.drag = Drag::Create { start: p, cur: p };
    }

    /// 创建松手:按工具落地(拖框区域文本 / 画板 / 形状)。
    pub(super) fn end_create(
        &mut self,
        start: egui::Vec2,
        cur: egui::Vec2,
        shift: bool,
        alt: bool,
    ) {
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
            // 切片(09-C):拖框建立 data-vb-slice 切片节点
            Tool::Slice => {
                let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                self.create_slice(Geom {
                    x: g.x.round(),
                    y: g.y.round(),
                    w: g.w.round().max(8.0),
                    h: g.h.round().max(8.0),
                });
            }
            _ => {
                let g = vb_tools::drag_rect_geom(sx, sy, cx, cy, shift, alt);
                self.create_shape(g);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;
    use vb_doc::model::{Node, NodeKind};

    /// 09-C:切片命令派发(Shift+K 切工具)+ 拖框松手建立带
    /// `data-vb-slice` 属性的切片节点,可撤销。
    #[test]
    fn slice_tool_creates_marked_node_and_undoes() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.run_command("tool.slice", false, false);
        assert_eq!(app.tool, Tool::Slice, "Shift+K 切入切片工具");
        // 模拟拖框 (100,100) → (400,300)(世界坐标松手)
        app.drag = Drag::Create {
            start: egui::vec2(100.0, 100.0),
            cur: egui::vec2(400.0, 300.0),
        };
        app.end_create(
            egui::vec2(100.0, 100.0),
            egui::vec2(400.0, 300.0),
            false,
            false,
        );
        // 真实管线里 drag_stopped 会先清拖拽态;直接调用需手动还原
        app.drag = Drag::None;
        let sid = app.selection.last().expect("切片建立后应选中").clone();
        let nid = app.doc.find_by_sid(&sid).unwrap();
        let n = app.doc.nodes.get(nid).unwrap();
        assert!(matches!(n.kind, NodeKind::Slice), "产物是切片节点");
        assert_eq!(
            n.attrs.get("data-vb-slice").map(String::as_str),
            Some(n.name.as_str()),
            "切片属性 = 节点名(vellum-cli 按它取区域)"
        );
        assert_eq!(
            vb_tools::abs_bbox(&app.doc, nid).unwrap(),
            vb_common::geom::Rect::new(100.0, 100.0, 400.0, 300.0),
            "切片几何 = 拖框区域"
        );
        // 可撤销
        app.run_command("edit.undo", false, false);
        assert!(app.doc.find_by_sid(&sid).is_none(), "切片建立可撤销");
    }

    /// 09-C:从选区建立(对象 → 切片 → 建立)= 选中对象的世界包围盒。
    #[test]
    fn slice_from_selection_covers_selection_bounds() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        let ab = app.doc.artboards[0];
        for g in [
            Geom {
                x: 20.0,
                y: 30.0,
                w: 100.0,
                h: 60.0,
            },
            Geom {
                x: 200.0,
                y: 90.0,
                w: 80.0,
                h: 60.0,
            },
        ] {
            let sid = app.doc.alloc_sid();
            let mut n = Node::new(NodeKind::Box, "盒", sid.clone());
            n.geom = g;
            let id = app.doc.nodes.insert(n);
            app.doc.nodes.get_mut(id).unwrap().parent = Some(ab);
            app.doc.nodes.get_mut(ab).unwrap().children.push(id);
            app.selection.push(sid.as_str().to_string());
        }
        app.slice_from_selection();
        let sid = app.selection.last().unwrap().clone();
        let nid = app.doc.find_by_sid(&sid).unwrap();
        let n = app.doc.nodes.get(nid).unwrap();
        assert!(matches!(n.kind, NodeKind::Slice));
        assert_eq!(
            vb_tools::abs_bbox(&app.doc, nid).unwrap(),
            vb_common::geom::Rect::new(20.0, 30.0, 280.0, 150.0),
            "切片 = 选区公共包围盒"
        );
    }
}
