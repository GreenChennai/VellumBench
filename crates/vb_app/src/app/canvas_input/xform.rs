//! X-4 变换工具族(05-2,design/06 §3.10):旋转 R / 镜像 O / 缩放 S /
//! 自由变换 E 的「工具化 + 单击设中心」语义。
//!
//! 状态机:选中工具 → **单击**画布点设定变换中心(`xf_center`)→
//! **拖拽**围绕中心按工具语义变换 → 松手收束(记「上一次变换」);
//! Esc 回选择(通用 `canvas.cancel` 管线,含撤销作废)。
//! 几何语义的纯函数在 `vb_tools::xform`(单测锁定)。
//!
//! 借用纪律:拖拽态字段一律**先快照、后落命令、再回写标志**
//! (`self.drag` 与 `self.exec`/`self.doc` 不能同时借用)。

use vb_doc::commands::Command;
use vb_doc::model::Geom;

use super::Drag;
use crate::app::{fmt_deg, parse_rotate_deg, set_style_prop, Tool, VellumApp};

impl VellumApp {
    /// X-4 工具单击:单击画布点 = 设定变换中心。返回 true = 已消费。
    pub(super) fn xform_click(
        &mut self,
        response: &egui::Response,
        _ctx: &egui::Context,
        rect: egui::Rect,
    ) -> bool {
        if response.clicked() && self.tool.is_transform_family() {
            if let Some(p) = response.interact_pointer_pos() {
                let pl = p - rect.min;
                let (wx, wy) = self.camera.screen_to_world(pl.x as f64, pl.y as f64);
                self.xf_center = Some((wx, wy));
                let name = match self.tool {
                    Tool::Rotate => "旋转",
                    Tool::Mirror => "镜像",
                    _ => "缩放",
                };
                self.status = format!("{name}中心已设定({:.0},{:.0}),拖拽对象即围绕它变换", wx, wy);
            }
            return true;
        }
        false
    }

    /// 当前选区的 (sid, geom) 列表(工具拖拽的变换目标)。
    fn xform_targets(&self) -> Vec<(String, Geom)> {
        self.selection
            .iter()
            .filter_map(|sid| {
                self.doc
                    .find_by_sid(sid)
                    .and_then(|id| self.doc.nodes.get(id))
                    .map(|n| (sid.clone(), n.geom))
            })
            .collect()
    }

    /// 当前选区的世界包围盒(无选区 None)。
    fn selection_world_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        let mut acc: Option<vb_common::geom::Rect> = None;
        for sid in &self.selection {
            let Some(nid) = self.doc.find_by_sid(sid) else {
                continue;
            };
            let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
                continue;
            };
            acc = Some(match acc {
                Some(a) => a.union(bb),
                None => bb,
            });
        }
        acc.map(|r| (r.x0, r.y0, r.x1 - r.x0, r.y1 - r.y0))
    }

    /// 变换中心:单击设定优先;未单击时回退选区包围盒中心(06 篇:
    /// 「单击设中心」是显式语义,直接拖拽也得有个确定中心)。
    fn xform_center_or_default(&self) -> (f64, f64) {
        if let Some(c) = self.xf_center {
            return c;
        }
        match self.selection_world_bounds() {
            Some((x, y, w, h)) => (x + w / 2.0, y + h / 2.0),
            None => self.cursor_world,
        }
    }

    /// X-4 工具拖拽起手(旋转/镜像/缩放;自由变换另有角命中入口)。
    pub(super) fn drag_begin_xform(&mut self, wx: f64, wy: f64) {
        if self.selection.is_empty() {
            self.status = "先选中对象,再单击设中心 / 拖拽变换".into();
            return;
        }
        let sids = self.selection.clone();
        match self.tool {
            Tool::Rotate => {
                let center = self.xform_center_or_default();
                let start_angle = vb_tools::xform::angle_of(center, (wx, wy));
                let start_degs = sids
                    .iter()
                    .filter_map(|sid| {
                        self.doc.find_by_sid(sid).and_then(|id| {
                            self.doc
                                .nodes
                                .get(id)
                                .and_then(|n| n.style_get("transform"))
                                .and_then(parse_rotate_deg)
                        })
                    })
                    .collect();
                self.drag = Drag::ToolRotate {
                    sids,
                    center,
                    start_angle,
                    start_degs,
                    moved: false,
                };
            }
            Tool::Mirror => {
                let center = self.xform_center_or_default();
                self.drag = Drag::ToolMirror {
                    center,
                    start: (wx, wy),
                    cur: (wx, wy),
                    moved: false,
                };
            }
            _ => {
                let center = self.xform_center_or_default();
                self.drag = Drag::ToolScale {
                    center,
                    start: (wx, wy),
                    cur: (wx, wy),
                    moved: false,
                };
            }
        }
    }

    /// 自由变换拖拽起手:命中选区包围盒四角之一才开始(06 篇「四角独立拖动」)。
    pub(super) fn drag_begin_free_transform(&mut self, wx: f64, wy: f64) {
        let Some((bx, by, bw, bh)) = self.selection_world_bounds() else {
            self.status = "自由变换:先选中对象".into();
            return;
        };
        // 命中四角(世界容差 = 8 屏幕像素)
        let tol = 8.0 / self.camera.zoom;
        let corners = [(bx, by), (bx + bw, by), (bx + bw, by + bh), (bx, by + bh)];
        let Some(corner) = corners
            .iter()
            .position(|c| vb_tools::xform::dist(*c, (wx, wy)) <= tol)
            .map(|i| i as u8)
        else {
            self.status = "自由变换:拖选区四角之一(对角锚定缩放)".into();
            return;
        };
        let sids = self.selection.clone();
        let _ = sids; // 变换目标由 xform_targets() 实时取(选区即目标)
        self.drag = Drag::FreeTransform {
            bounds: (bx, by, bw, bh),
            corner,
            cur: (wx, wy),
            moved: false,
        };
    }

    /// X-4 拖拽持续(旋转/镜像/缩放/自由变换)。返回 true = 本帧已消费。
    ///
    /// 借用纪律:先**整块拷贝**拖拽态快照,再 `self.exec` 落命令,
    /// 最后回写 `moved`/`cur` —— `self.drag` 与 `self.exec` 不重叠借用。
    pub(super) fn drag_move_xform(
        &mut self,
        wx: f64,
        wy: f64,
        _p: egui::Vec2,
        shift: bool,
        alt: bool,
    ) -> bool {
        // ── 旋转:绕中心角差 → 每对象 rotate(θ0+Δ) ──
        let rot = if let Drag::ToolRotate {
            sids,
            center,
            start_angle,
            start_degs,
            ..
        } = &self.drag
        {
            Some((sids.clone(), *center, *start_angle, start_degs.clone()))
        } else {
            None
        };
        if let Some((sids, center, start_angle, base)) = rot {
            let a = vb_tools::xform::angle_of(center, (wx, wy));
            let mut delta_deg = (a - start_angle).to_degrees();
            if shift {
                // 06 篇 §3.10:Shift 约束 15°
                delta_deg = (delta_deg / 15.0).round() * 15.0;
            }
            let mut cmds = Vec::new();
            for (i, sid) in sids.iter().enumerate() {
                let start_deg = base.get(i).copied().unwrap_or(0.0);
                let deg = ((start_deg + delta_deg) * 10.0).round() / 10.0;
                if let Some(nid) = self.doc.find_by_sid(sid) {
                    let mut style = self.doc.nodes.get(nid).unwrap().style.clone();
                    style =
                        set_style_prop(style, "transform", &format!("rotate({}deg)", fmt_deg(deg)));
                    cmds.push(Command::SetStyle {
                        sid: sid.clone(),
                        new: style,
                        old: None,
                    });
                }
            }
            if !cmds.is_empty() {
                self.exec(Command::Compound { cmds });
                self.status = format!("旋转 {}°(Shift 约束 15°)", fmt_deg(delta_deg));
            }
            if let Drag::ToolRotate { moved, .. } = &mut self.drag {
                *moved = true;
            }
            return true;
        }
        // ── 镜像:轴随拖动方向变化,位置反射 + scale(±1) 翻转 ──
        let mirror = if let Drag::ToolMirror { center, .. } = &self.drag {
            Some(*center)
        } else {
            None
        };
        if let Some(center) = mirror {
            let axis = vb_tools::xform::MirrorAxis::from_drag(wx - center.0, wy - center.1);
            let mut cmds = Vec::new();
            for (sid, g0) in self.xform_targets() {
                cmds.push(Command::SetGeom {
                    sid: sid.clone(),
                    new: vb_tools::xform::mirror_geom(g0, axis, center),
                    old: None,
                    old_declared: None,
                });
                if let Some(style) = mirrored_style(&self.doc, &sid, axis) {
                    cmds.push(Command::SetStyle {
                        sid: sid.clone(),
                        new: style,
                        old: None,
                    });
                }
            }
            if !cmds.is_empty() {
                self.exec(Command::Compound { cmds });
            }
            if let Drag::ToolMirror { moved: m, cur, .. } = &mut self.drag {
                *m = true;
                *cur = (wx, wy);
            }
            return true;
        }
        // ── 缩放:绕中心逐轴距离比(Shift 等比;Alt 从对象中心)──
        let scale = if let Drag::ToolScale { center, start, .. } = &self.drag {
            Some((*center, *start))
        } else {
            None
        };
        if let Some((center, start)) = scale {
            // 系数 = 当前/起点到**中心**的逐轴距离比(中心 = 单击设定点)
            let (mut kx, mut ky) = vb_tools::xform::scale_factors(center, start, (wx, wy));
            if shift {
                // 06 篇 §3.10:Shift 等比(取较大轴比率,保留方向)
                let neg = kx < 0.0 || ky < 0.0;
                let k = kx.abs().max(ky.abs()) * if neg { -1.0 } else { 1.0 };
                kx = k;
                ky = k;
            }
            // Alt = 从对象中心缩放(轴心改为选区包围盒中心)
            let c = if alt {
                self.selection_world_bounds()
                    .map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
                    .unwrap_or(center)
            } else {
                center
            };
            let mut cmds = Vec::new();
            for (sid, g0) in self.xform_targets() {
                cmds.push(Command::SetGeom {
                    sid,
                    new: vb_tools::xform::scale_geom_about(g0, c, kx, ky),
                    old: None,
                    old_declared: None,
                });
            }
            if !cmds.is_empty() {
                self.exec(Command::Compound { cmds });
                self.status = format!("缩放 ×{kx:.2}/×{ky:.2}");
            }
            if let Drag::ToolScale { moved: m, cur, .. } = &mut self.drag {
                *m = true;
                *cur = (wx, wy);
            }
            return true;
        }
        // ── 自由变换:拖角 → 对角锚定缩放(透视不做,网页无对应)──
        let free = if let Drag::FreeTransform { bounds, corner, .. } = &self.drag {
            Some((*bounds, *corner))
        } else {
            None
        };
        if let Some((bounds, corner)) = free {
            let (bx, by, bw, bh) = bounds;
            let anchor = match corner {
                0 => (bx + bw, by + bh),
                1 => (bx, by + bh),
                2 => (bx, by),
                _ => (bx + bw, by),
            };
            // 拖拽起点 = 被抓的角(锚定对角,比例 = 当前/起点到对角的距离比)
            let corner_start = match corner {
                0 => (bx, by),
                1 => (bx + bw, by),
                2 => (bx + bw, by + bh),
                _ => (bx, by + bh),
            };
            let (kx, ky) = vb_tools::xform::scale_factors(anchor, corner_start, (wx, wy));
            let mut cmds = Vec::new();
            for (sid, g0) in self.xform_targets() {
                cmds.push(Command::SetGeom {
                    sid,
                    new: vb_tools::xform::scale_geom_about(g0, anchor, kx, ky),
                    old: None,
                    old_declared: None,
                });
            }
            if !cmds.is_empty() {
                self.exec(Command::Compound { cmds });
                self.status = format!("自由变换 ×{kx:.2}/×{ky:.2}");
            }
            if let Drag::FreeTransform { moved: m, cur, .. } = &mut self.drag {
                *m = true;
                *cur = (wx, wy);
            }
            return true;
        }
        false
    }

    /// 旋转松手:状态收束(角度已逐帧写入各对象 transform;旋转手柄
    /// 同款由 `end_rotate` 记忆,工具版按增量统一记入「上一次变换」)。
    pub(super) fn end_tool_rotate(
        &mut self,
        sids: Vec<String>,
        start_degs: Vec<f64>,
        _center: (f64, f64),
        moved: bool,
    ) {
        if !moved {
            return;
        }
        // 以主对象(最后选中)的角度增量作为「上一次变换」的旋转分量
        if let (Some(sid), Some(start_deg)) = (sids.last(), start_degs.last()) {
            let cur = self
                .doc
                .find_by_sid(sid)
                .and_then(|id| self.doc.nodes.get(id))
                .and_then(|n| n.style_get("transform"))
                .and_then(parse_rotate_deg)
                .unwrap_or(0.0);
            self.remember_transform(crate::app::transform_panel::TransformDelta {
                d_angle: cur - start_deg,
                ..crate::app::transform_panel::TransformDelta::translate(0.0, 0.0)
            });
        }
        self.status = "旋转完成(Esc 回选择工具)".into();
    }

    /// 镜像松手:状态提示(命令已在拖拽中逐帧落地)。
    pub(super) fn end_tool_mirror(
        &mut self,
        center: (f64, f64),
        start: (f64, f64),
        cur: (f64, f64),
    ) {
        let axis = vb_tools::xform::MirrorAxis::from_drag(cur.0 - start.0, cur.1 - start.1);
        let _ = center;
        let name = match axis {
            vb_tools::xform::MirrorAxis::Vertical => "左右镜像(竖直轴)",
            vb_tools::xform::MirrorAxis::Horizontal => "上下镜像(水平轴)",
        };
        self.status = format!("镜像完成:{name}(Esc 回选择工具)");
    }

    /// 缩放松手:把缩放记进「上一次变换」(比例按中心距离比)。
    pub(super) fn end_tool_scale(
        &mut self,
        center: (f64, f64),
        start: (f64, f64),
        cur: (f64, f64),
        moved: bool,
    ) {
        if !moved {
            return;
        }
        let (kx, ky) = vb_tools::xform::scale_factors(center, start, cur);
        self.remember_transform(crate::app::transform_panel::TransformDelta {
            kx,
            ky,
            ..crate::app::transform_panel::TransformDelta::translate(0.0, 0.0)
        });
        self.status = format!("缩放完成 ×{kx:.2}/×{ky:.2}(Esc 回选择工具)");
    }

    /// 自由变换松手:状态收束。
    pub(super) fn end_free_transform(&mut self, moved: bool) {
        if moved {
            self.status = "自由变换完成(Esc 回选择工具)".into();
        }
    }
}

/// 镜像后的 transform 声明:保留 rotate,叠加/翻转 scaleX/scaleY(-1)。
/// 再次同轴镜像 = 翻回(±1 自消),不会无限叠加。
fn mirrored_style(
    doc: &vb_doc::model::Document,
    sid: &str,
    axis: vb_tools::xform::MirrorAxis,
) -> Option<Vec<vb_css::Decl>> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.nodes.get(nid)?;
    let tf = n.style_get("transform").unwrap_or("").to_string();
    let deg = parse_rotate_deg(&tf).unwrap_or(0.0);
    // 当前翻转态(同轴再拖 = 复原)
    let (key, cur_flip) = match axis {
        vb_tools::xform::MirrorAxis::Vertical => ("scaleX", tf.contains("scaleX(-1)")),
        vb_tools::xform::MirrorAxis::Horizontal => ("scaleY", tf.contains("scaleY(-1)")),
    };
    let mut parts: Vec<String> = Vec::new();
    if !cur_flip {
        parts.push(format!("{key}(-1)"));
    }
    if deg.abs() > 1e-9 {
        parts.push(format!("rotate({}deg)", fmt_deg(deg)));
    }
    let value = parts.join(" ");
    let mut style = n.style.clone();
    if value.is_empty() {
        style.retain(|d| d.prop != "transform");
    } else {
        style = set_style_prop(style, "transform", &value);
    }
    Some(style)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;
    use vb_doc::model::{Node, NodeKind};

    fn add_box(app: &mut VellumApp, g: Geom) -> String {
        let ab = app.doc.artboards[0];
        let sid = app.doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, "盒", sid.clone());
        n.geom = g;
        let id = app.doc.nodes.insert(n);
        app.doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        app.doc.nodes.get_mut(ab).unwrap().children.push(id);
        app.selection = vec![sid.as_str().to_string()];
        sid.as_str().to_string()
    }

    /// X-4:新工具命令派发 + Esc 回选择(06 篇 §3.10 键位语义)。
    #[test]
    fn xform_tools_dispatch_and_esc_returns_to_select() {
        let _env = crate::ENV_LOCK.lock();
        for (cmd, tool) in [
            ("tool.rotate", Tool::Rotate),
            ("tool.mirror", Tool::Mirror),
            ("tool.scale", Tool::Scale),
            ("tool.free_transform", Tool::FreeTransform),
        ] {
            let mut app = app_fresh(None);
            app.run_command(cmd, false, false);
            assert_eq!(app.tool, tool, "{cmd} 必须切入 {tool:?}");
            app.run_command("canvas.cancel", false, false);
            assert_eq!(app.tool, Tool::Select, "Esc 必须从 {tool:?} 回选择");
        }
    }

    /// X-4:变换中心是**工具会话态** —— 切走变换族工具即清空。
    #[test]
    fn xf_center_is_cleared_when_leaving_transform_family() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.run_command("tool.rotate", false, false);
        app.xf_center = Some((50.0, 60.0));
        add_box(
            &mut app,
            Geom {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
        );
        app.run_command("tool.rect", false, false);
        assert_eq!(app.xf_center, None, "切到非变换族工具必须清中心");
        app.run_command("tool.rotate", false, false);
        app.xf_center = Some((50.0, 60.0));
        app.run_command("tool.mirror", false, false);
        assert_eq!(
            app.xf_center,
            Some((50.0, 60.0)),
            "同族切换保留显式设定的中心(中心随工具会话,不随族内切换丢失)"
        );
    }

    /// X-4 镜像语义(纯函数级):位置反射 + scaleX(-1) 翻转,再拖同向复原。
    #[test]
    fn mirror_semantics_reflect_position_and_flip_content() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.run_command("tool.mirror", false, false);
        let sid = add_box(
            &mut app,
            Geom {
                x: 120.0,
                y: 50.0,
                w: 80.0,
                h: 40.0,
            },
        );
        app.xf_center = Some((100.0, 70.0));
        // 模拟拖拽起手 + 水平拖(→ 竖直轴,左右镜像)
        app.drag_begin_xform(120.0, 60.0);
        app.drag_move_xform(190.0, 70.0, egui::vec2(0.0, 0.0), false, false);
        let nid = app.doc.find_by_sid(&sid).unwrap();
        let n = app.doc.nodes.get(nid).unwrap();
        // 盒 (120..200) 关于 x=100 反射 → (0..80)
        assert_eq!((n.geom.x, n.geom.w), (0.0, 80.0), "位置关于中心反射");
        assert!(
            n.style_get("transform")
                .unwrap_or("")
                .contains("scaleX(-1)"),
            "内容翻转写入 transform"
        );
        // 松手(保留命令)→ 再次同向镜像 = 复原
        app.end_tool_mirror((100.0, 70.0), (120.0, 60.0), (190.0, 70.0));
        app.drag_begin_xform(120.0, 60.0);
        app.drag_move_xform(190.0, 70.0, egui::vec2(0.0, 0.0), false, false);
        let n = app.doc.nodes.get(nid).unwrap();
        assert_eq!((n.geom.x, n.geom.w), (120.0, 80.0), "再次镜像回到原位");
        assert!(
            !n.style_get("transform")
                .unwrap_or("")
                .contains("scaleX(-1)"),
            "翻转分量自消,不叠加"
        );
    }

    /// X-4 缩放语义(纯函数级):绕单击中心按距离比缩放,Shift 等比。
    #[test]
    fn scale_semantics_scales_about_set_center() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.run_command("tool.scale", false, false);
        let sid = add_box(
            &mut app,
            Geom {
                x: 100.0,
                y: 100.0,
                w: 100.0,
                h: 50.0,
            },
        );
        app.xf_center = Some((0.0, 0.0));
        app.drag_begin_xform(150.0, 125.0);
        // 拖到一半距离 → kx = ky = 0.5
        app.drag_move_xform(75.0, 62.5, egui::vec2(0.0, 0.0), true, false);
        let nid = app.doc.find_by_sid(&sid).unwrap();
        let n = app.doc.nodes.get(nid).unwrap();
        assert_eq!((n.geom.x, n.geom.w), (50.0, 50.0), "x 轴按 0.5 收缩");
        assert_eq!((n.geom.y, n.geom.h), (50.0, 25.0), "y 轴按 0.5 收缩");
        app.end_tool_scale((0.0, 0.0), (150.0, 125.0), (75.0, 62.5), true);
    }

    /// X-4 自由变换:角命中判定 + 对角锚定缩放。
    #[test]
    fn free_transform_drag_corner_scales_about_opposite() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.run_command("tool.free_transform", false, false);
        let sid = add_box(
            &mut app,
            Geom {
                x: 100.0,
                y: 100.0,
                w: 100.0,
                h: 100.0,
            },
        );
        // 拖 SE 角 (200,200) → 向内拖到 (150,150):对角 NW (100,100) 锚定,k=0.5
        app.drag_begin_free_transform(200.0, 200.0);
        if let Drag::FreeTransform { corner, .. } = &app.drag {
            assert_eq!(*corner, 2, "命中 SE 角");
        } else {
            panic!("四角命中应置 FreeTransform 拖拽态");
        }
        app.drag_move_xform(150.0, 150.0, egui::vec2(0.0, 0.0), false, false);
        let nid = app.doc.find_by_sid(&sid).unwrap();
        let n = app.doc.nodes.get(nid).unwrap();
        assert_eq!(
            (n.geom.x, n.geom.y, n.geom.w, n.geom.h),
            (100.0, 100.0, 50.0, 50.0),
            "对角锚定,盒子按 0.5 缩"
        );
        app.end_free_transform(true);
    }
}
