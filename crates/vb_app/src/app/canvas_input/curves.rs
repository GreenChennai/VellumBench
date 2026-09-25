//! X-5 曲线工具 + 09-E 度量(05-2,design/06 §3.5 / §3.7 / §六):
//! 铅笔自由绘制 → 保真度容差抽稀为矢量路径;曲率单击路径段自动拟合
//! 平滑控制点;度量工具拖动量距 / 单击标注对象尺寸。
//!
//! 几何核心(RDP 抽稀、Catmull-Rom 拟合、锚点提取)在 `vb_tools::xform`,
//! 单测在彼处;本文件只做工具状态机与命令落地。

use vb_doc::commands::Command;
use vb_doc::model::NodeKind;

use super::Drag;
use crate::app::{Tool, VellumApp};

impl VellumApp {
    /// 曲率单击:命中矢量路径 → 锚点曲线拟合(SetVector,可撤销)。
    /// 返回 true = 已消费(无论命中与否 —— 曲率工具下单击不再穿透选择)。
    pub(super) fn curvature_click(
        &mut self,
        response: &egui::Response,
        _ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
        if response.clicked() && self.tool == Tool::Curvature {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.curvature_fit_at(wx, wy);
            }
            return true;
        }
        false
    }

    /// 曲率拟合核心(命中判定 + 命令构造;GUI 无关部分供测试复用)。
    pub(crate) fn curvature_fit_at(&mut self, wx: f64, wy: f64) {
        let tol = 8.0 / self.camera.zoom;
        // 命中含曲线段的矢量路径(顶点命中沿用直接选择的判定)
        let Some((sid, _)) = self.find_vector_vertex(wx, wy, tol) else {
            self.status = "曲率:请在矢量路径的锚点附近单击(先经钢笔/铅笔建路径)".into();
            return;
        };
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            return;
        };
        let path_opt = match self.doc.nodes.get(nid).map(|n| &n.kind) {
            Some(NodeKind::Vector { path }) => Some(path.clone()),
            _ => None,
        };
        let Some(path) = path_opt else {
            self.status = "曲率:目标不是矢量路径".into();
            return;
        };
        let Some((anchors, closed)) = vb_tools::xform::path_anchors(&path) else {
            self.status = "曲率:路径锚点不足,无法拟合".into();
            return;
        };
        let Some(smoothed) = vb_tools::xform::smooth_polyline(&anchors, closed) else {
            self.status = "曲率:路径锚点不足,无法拟合".into();
            return;
        };
        self.exec(Command::SetVector {
            sid: sid.clone(),
            new: smoothed,
            old: None,
        });
        self.selection = vec![sid];
        self.status = format!(
            "曲率:已为 {} 个锚点拟合平滑控制点(直接选择 A 可微调手柄)",
            anchors.len()
        );
    }

    /// 铅笔拖拽起手:记录首点(世界坐标)。
    pub(super) fn drag_begin_pencil(&mut self, wx: f64, wy: f64) {
        self.drag = Drag::PencilStroke {
            pts: vec![(wx, wy)],
            last: (wx, wy),
        };
    }

    /// 铅笔拖拽持续:距上一采样 ≥1 世界像素才追加(压点距,抽稀前置)。
    /// 返回 true = 本帧已消费。
    pub(super) fn drag_move_pencil(&mut self, wx: f64, wy: f64) -> bool {
        if let Drag::PencilStroke { pts, last } = &mut self.drag {
            if vb_tools::xform::dist(*last, (wx, wy)) >= 1.0 {
                pts.push((wx, wy));
                *last = (wx, wy);
            }
            return true;
        }
        false
    }

    /// 铅笔松手:按保真度容差 RDP 抽稀 → 创建矢量节点(描边,开放路径)。
    pub(super) fn end_pencil(&mut self, pts: Vec<(f64, f64)>) {
        let simplified = vb_tools::xform::rdp_simplify(&pts, self.pencil_fidelity);
        if simplified.len() < 2 {
            self.status = "铅笔:笔画太短(按住拖动绘制)".into();
            return;
        }
        let path = vb_tools::xform::smooth_polyline(&simplified, false);
        let n = simplified.len();
        match path {
            Some(p) => {
                self.create_vector_node(p, false);
                self.status = format!(
                    "铅笔:{} 点笔迹 → {} 锚点路径(保真度 {:.0}px,编辑 → 设置可调)",
                    pts.len(),
                    n,
                    self.pencil_fidelity
                );
            }
            None => self.status = "铅笔:笔画无法成路径".into(),
        }
    }

    /// 度量单击:命中对象 → 状态栏标注其尺寸(对象尺寸标注,06 篇 §六)。
    /// 返回 true = 已消费。
    pub(super) fn measure_click(
        &mut self,
        response: &egui::Response,
        _ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
        if response.clicked() && self.tool == Tool::Measure {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                if let Some(nid) = self.pick_at_world(wx, wy) {
                    let n = self.doc.nodes.get(nid).unwrap();
                    self.status = format!(
                        "度量:「{}」 {} × {} px(原点 {:.0},{:.0};拖动可量任意两点距离)",
                        n.name, n.geom.w, n.geom.h, n.geom.x, n.geom.y
                    );
                    if let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) {
                        // 尺寸标注保留到 measure_result(画布持续显示虚线框)
                        self.measure_anchor = Some((bb.x0, bb.y0));
                        self.measure_result = Some((
                            bb.x1 - bb.x0,
                            bb.y1 - bb.y0,
                            (bb.x1 - bb.x0).hypot(bb.y1 - bb.y0),
                        ));
                    }
                } else {
                    self.status = "度量:单击对象标注尺寸,或拖动量两点距离(Esc 退出)".into();
                }
            }
            return true;
        }
        false
    }

    /// 度量松手:结果存会话态(画布持续显示;Esc/切工具/再次度量清空)。
    pub(super) fn end_measure(&mut self, start: (f64, f64), cur: (f64, f64)) {
        let (dx, dy) = (cur.0 - start.0, cur.1 - start.1);
        let d = dx.hypot(dy);
        self.measure_anchor = Some(start);
        self.measure_result = Some((dx, dy, d));
        self.status = format!(
            "度量:距离 {d:.1}px(ΔX {dx:.1},ΔY {dy:.1};Esc 退出度量)",
            d = d,
            dx = dx,
            dy = dy
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;
    use vb_doc::model::{Geom, Node, NodeKind};

    fn add_vector(app: &mut VellumApp, pts: &[(f64, f64)]) -> String {
        use vb_common::geom::Point;
        let ab = app.doc.artboards[0];
        let sid = app.doc.alloc_sid();
        let mut path = vb_common::geom::BezPath::new();
        for (i, (x, y)) in pts.iter().enumerate() {
            let p = Point::new(*x, *y);
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        let mut n = Node::new(NodeKind::Vector { path }, "路径", sid.clone());
        n.geom = Geom {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let id = app.doc.nodes.insert(n);
        app.doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        app.doc.nodes.get_mut(ab).unwrap().children.push(id);
        sid.as_str().to_string()
    }

    /// X-5:曲率单击折线 → SetVector 平滑(锚点数守恒,出现曲线元素)。
    #[test]
    fn curvature_click_smooths_polyline() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.run_command("tool.curvature", false, false);
        add_vector(
            &mut app,
            &[(0.0, 0.0), (50.0, 0.0), (50.0, 50.0), (0.0, 50.0)],
        );
        // 单击第一个锚点(命中容差 8 世界像素)
        app.curvature_fit_at(0.0, 0.0);
        let sid = app.selection.last().unwrap().clone();
        let nid = app.doc.find_by_sid(&sid).unwrap();
        let path = match &app.doc.nodes.get(nid).unwrap().kind {
            NodeKind::Vector { path } => path.clone(),
            _ => panic!("仍是矢量路径"),
        };
        let curves = path
            .elements()
            .iter()
            .filter(|e| matches!(e, vb_common::geom::PathEl::CurveTo(..)))
            .count();
        assert!(curves >= 3, "折线已被拟合成曲线段:{curves}");
    }

    /// X-5:铅笔松手 → 按保真度抽稀并创建矢量节点(可撤销)。
    #[test]
    fn pencil_stroke_creates_simplified_vector() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.run_command("tool.pencil", false, false);
        // 一条带抖动的水平笔迹(±1px,2px 容差应吸收成 2 锚点)
        let pts: Vec<(f64, f64)> = (0..=60)
            .map(|i| (i as f64, if i % 2 == 0 { 0.5 } else { -0.5 }))
            .collect();
        app.pencil_fidelity = 2.0;
        app.end_pencil(pts);
        let sid = app
            .selection
            .last()
            .expect("铅笔松手后应选中新路径")
            .clone();
        let nid = app.doc.find_by_sid(&sid).unwrap();
        let n = app.doc.nodes.get(nid).unwrap();
        let anchors = match &n.kind {
            NodeKind::Vector { path } => vb_tools::xform::path_anchors(path).unwrap().0.len(),
            _ => panic!("铅笔产物是矢量路径"),
        };
        assert!(anchors <= 3, "抖动笔迹被抽稀(实际 {anchors} 锚点)");
        // 可撤销
        app.run_command("edit.undo", false, false);
        assert!(app.doc.find_by_sid(&sid).is_none(), "铅笔创建可撤销");
    }

    /// X-5:铅笔保真度档位循环并持久化。
    #[test]
    fn pencil_fidelity_steps_and_persists() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        assert_eq!(app.pencil_fidelity, 4.0, "默认 4px");
        app.run_command("edit.pencil_fidelity", false, false);
        assert_eq!(app.pencil_fidelity, 8.0);
        assert!(
            (app.workspace_saved.pencil_fidelity - 8.0).abs() < 1e-6,
            "落 workspace.json"
        );
    }

    /// 09-E:度量松手 → 结果 + 起点保留;Esc/切工具清空。
    #[test]
    fn measure_result_persists_and_clears() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.run_command("tool.measure", false, false);
        app.end_measure((0.0, 0.0), (30.0, 40.0));
        assert_eq!(app.measure_result, Some((30.0, 40.0, 50.0)));
        assert_eq!(app.measure_anchor, Some((0.0, 0.0)));
        // Esc 回选择并清空度量态
        app.run_command("canvas.cancel", false, false);
        assert_eq!(app.tool, Tool::Select);
        assert_eq!(app.measure_result, None);
        assert_eq!(app.measure_anchor, None);
    }

    /// 09-E:像素预览开关(命令切换 + 状态提示)。
    #[test]
    fn pixel_preview_toggles() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        assert!(!app.pixel_preview);
        app.run_command("view.pixel_preview", false, false);
        assert!(app.pixel_preview);
        app.run_command("view.pixel_preview", false, false);
        assert!(!app.pixel_preview);
    }
}
