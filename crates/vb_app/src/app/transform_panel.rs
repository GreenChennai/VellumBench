//! 变换数值面板(`⇧F8`,副文档 03-2 / 03-3)+ 再次变换的矩阵口径(03-4-2)。
//!
//! **单一真相**:面板与画布拖拽走**同一条命令路径**(`Command::SetGeom` 与
//! `Command::SetStyle(transform)`),面板只做"投影 → 构建命令",不自己算几何
//! —— 避免了副文档 03 §6 点名的"双真相"风险。
//!
//! **几何口径**(ADR 级决定,写在这里供后续引用):
//! - `X/Y/W/H` → `geom`(父相对矩形);`W/H` 的缩放以**参考点**为轴心;
//! - `∠ 旋转` / `倾斜` → CSS `transform`(`rotate(θdeg) skew(sxdeg, sydeg)`);
//!   旋转**不折进** `geom`(轴对齐矩形表达不了旋转,画布仍按未旋转 bbox 选中,
//!   与 AI 的"旋转后仍按 bbox 选中"手感一致);
//! - `transform: translate()` **会**折进画布几何(见 `vb_layout::parse_translate`)。
//!
//! **可测边界**:几何换算、参考点、transform 编解码、再次变换重放全部是纯函数。

use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom};
use vb_ui::components::{caption, NumField};

use crate::app::VellumApp;

// ─────────────────────────── 参考点九宫格 ───────────────────────────

/// 参考点(变换轴心):左中右 × 上中下。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefPoint {
    TL,
    TC,
    TR,
    ML,
    MC,
    MR,
    BL,
    BC,
    BR,
}

impl RefPoint {
    /// 九宫格渲染顺序(3×3)。
    pub const GRID: [[RefPoint; 3]; 3] = [
        [RefPoint::TL, RefPoint::TC, RefPoint::TR],
        [RefPoint::ML, RefPoint::MC, RefPoint::MR],
        [RefPoint::BL, RefPoint::BC, RefPoint::BR],
    ];

    pub fn label(self) -> &'static str {
        match self {
            RefPoint::TL => "左上",
            RefPoint::TC => "上中",
            RefPoint::TR => "右上",
            RefPoint::ML => "左中",
            RefPoint::MC => "中心",
            RefPoint::MR => "右中",
            RefPoint::BL => "左下",
            RefPoint::BC => "下中",
            RefPoint::BR => "右下",
        }
    }

    /// 轴心的归一化位置(`0..1`,`(0,0)` = 左上角)。
    pub fn anchor(self) -> (f64, f64) {
        let col = match self {
            RefPoint::TL | RefPoint::ML | RefPoint::BL => 0.0,
            RefPoint::TC | RefPoint::MC | RefPoint::BC => 0.5,
            RefPoint::TR | RefPoint::MR | RefPoint::BR => 1.0,
        };
        let row = match self {
            RefPoint::TL | RefPoint::TC | RefPoint::TR => 0.0,
            RefPoint::ML | RefPoint::MC | RefPoint::MR => 0.5,
            RefPoint::BL | RefPoint::BC | RefPoint::BR => 1.0,
        };
        (col, row)
    }
}

/// 轴心的世界/父帧坐标(包围盒内)。
pub fn anchor_point(g: Geom, rp: RefPoint) -> (f64, f64) {
    let (ax, ay) = rp.anchor();
    (g.x + g.w * ax, g.y + g.h * ay)
}

// ─────────────────────── 以参考点为轴心缩放(纯函数) ───────────────────────

/// 以 `rp` 为轴心按 `(kx, ky)` 缩放矩形(轴心不动)。
///
/// `x' = a + (x - a)·k`、`w' = w·k`(`a` 为轴心坐标)→ 轴心两侧同比例展开。
pub fn scale_about(g: Geom, kx: f64, ky: f64, rp: RefPoint) -> Geom {
    let (ax, ay) = anchor_point(g, rp);
    let w = (g.w * kx).max(0.0);
    let h = (g.h * ky).max(0.0);
    Geom {
        x: ax + (g.x - ax) * kx,
        y: ay + (g.y - ay) * ky,
        w,
        h,
    }
}

// ─────────────────────────── transform 编解码 ───────────────────────────

/// 取 CSS 函数实参(`name` 带左括号,**小写**)。
///
/// 入参先整体小写:`vb_css::canonical_value` 会把函数名规范成小写
/// (`skewX(` → `skewx(`),函数名大小写在 CSS 里不敏感;角度数值不受影响。
fn fn_args(tf: &str, name: &str) -> Option<Vec<String>> {
    let tf = tf.to_ascii_lowercase();
    let i = tf.find(name)?;
    let rest = &tf[i + name.len()..];
    let end = rest.find(')')?;
    let args: Vec<String> = rest[..end]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (!args.is_empty()).then_some(args)
}

fn deg_of(v: &str) -> Option<f64> {
    let n = v.trim().strip_suffix("deg").unwrap_or(v.trim());
    n.parse::<f64>().ok()
}

/// 解析 `transform` → `(旋转角, skewX, skewY)`;缺失项为 0。
pub fn parse_transform(tf: &str) -> (f64, f64, f64) {
    let angle = fn_args(tf, "rotate(")
        .and_then(|a| a.first().and_then(|v| deg_of(v)))
        .unwrap_or(0.0);
    let (sx, sy) = match fn_args(tf, "skew(") {
        Some(a) => (
            a.first().and_then(|v| deg_of(v)).unwrap_or(0.0),
            a.get(1).and_then(|v| deg_of(v)).unwrap_or(0.0),
        ),
        None => (
            fn_args(tf, "skewx(")
                .and_then(|a| a.first().and_then(|v| deg_of(v)))
                .unwrap_or(0.0),
            fn_args(tf, "skewy(")
                .and_then(|a| a.first().and_then(|v| deg_of(v)))
                .unwrap_or(0.0),
        ),
    };
    (angle, sx, sy)
}

/// 组装 `transform` 值。三项全 0 → `None`(调用方删除该声明)。
///
/// 写法固定为 `rotate(θdeg) skew(sxdeg, sydeg)`(缺省项省略),函数名一律
/// **小写** —— 与 `vb_css::canonical_value` 同口径,故输出即是 canonical
/// 不动点(L1 幂等;`skewX` 会被规范化成 `skewx`,这里直接写 `skewx`)。
pub fn build_transform(angle: f64, skew_x: f64, skew_y: f64) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if angle.abs() > 1e-9 {
        parts.push(format!("rotate({}deg)", vb_common::units::fmt_num(angle)));
    }
    if skew_x.abs() > 1e-9 && skew_y.abs() > 1e-9 {
        parts.push(format!(
            "skew({}deg, {}deg)",
            vb_common::units::fmt_num(skew_x),
            vb_common::units::fmt_num(skew_y)
        ));
    } else if skew_x.abs() > 1e-9 {
        parts.push(format!("skewx({}deg)", vb_common::units::fmt_num(skew_x)));
    } else if skew_y.abs() > 1e-9 {
        parts.push(format!("skewy({}deg)", vb_common::units::fmt_num(skew_y)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

// ─────────────────────── 再次变换(位移+缩放+旋转) ───────────────────────

/// 上一次变换的量(副文档 03-4-2:`Mod+D` 重放位移/缩放/旋转)。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformDelta {
    pub dx: f64,
    pub dy: f64,
    /// 缩放比例(1.0 = 不变)。
    pub kx: f64,
    pub ky: f64,
    /// 旋转增量(度)。
    pub d_angle: f64,
}

impl TransformDelta {
    /// 纯位移(画布拖动结束时的记录)。
    pub fn translate(dx: f64, dy: f64) -> Self {
        Self {
            dx,
            dy,
            kx: 1.0,
            ky: 1.0,
            d_angle: 0.0,
        }
    }

    /// 是否为空变换(三项都没变)。
    pub fn is_noop(&self) -> bool {
        self.dx.abs() < 1e-9
            && self.dy.abs() < 1e-9
            && (self.kx - 1.0).abs() < 1e-9
            && (self.ky - 1.0).abs() < 1e-9
            && self.d_angle.abs() < 1e-9
    }
}

/// 「再次变换」把 delta 重放到 `(geom, 角度)` 上(缩放以参考点为轴心)。
pub fn replay(g: Geom, angle: f64, d: &TransformDelta, rp: RefPoint) -> (Geom, f64) {
    let scaled = scale_about(g, d.kx, d.ky, rp);
    let moved = Geom {
        x: scaled.x + d.dx,
        y: scaled.y + d.dy,
        ..scaled
    };
    (moved, angle + d.d_angle)
}

// ─────────────────────── 面板投影与命令构建 ───────────────────────

/// 变换面板的投影(单选取节点几何 + transform;多选取公共包围盒)。
#[derive(Debug, Clone)]
pub struct TransformProj {
    /// 参与变换的 sid(单选取 1 个;多选取全部)。
    pub sids: Vec<String>,
    /// 显示/编辑用的矩形(单选取该节点;多选取公共包围盒,**画板本地帧**)。
    pub rect: Geom,
    /// 当前旋转/倾斜(多选取首项)。
    pub angle: f64,
    pub skew_x: f64,
    pub skew_y: f64,
    pub multi: bool,
}

/// 投影选中对象(读自文档,无私有状态)。
pub fn project(doc: &Document, selection: &[String]) -> Option<TransformProj> {
    let sids: Vec<String> = selection.to_vec();
    if sids.is_empty() {
        return None;
    }
    let first = sids.first()?;
    let nid = doc.find_by_sid(first)?;
    let n = doc.nodes.get(nid)?;
    let tf = n.style_get("transform").unwrap_or("");
    let (angle, skew_x, skew_y) = parse_transform(tf);

    if sids.len() == 1 {
        return Some(TransformProj {
            sids,
            rect: n.geom,
            angle,
            skew_x,
            skew_y,
            multi: false,
        });
    }
    // 多选:公共包围盒(世界坐标,画板本地=世界,多画板时取并集)
    let mut bbox: Option<Geom> = None;
    for sid in &sids {
        let Some(id) = doc.find_by_sid(sid) else {
            continue;
        };
        let Some(bb) = vb_tools::abs_bbox_world(doc, id) else {
            continue;
        };
        let g = Geom {
            x: bb.x0,
            y: bb.y0,
            w: bb.x1 - bb.x0,
            h: bb.y1 - bb.y0,
        };
        bbox = Some(match bbox {
            None => g,
            Some(b) => {
                let x0 = b.x.min(g.x);
                let y0 = b.y.min(g.y);
                let x1 = (b.x + b.w).max(g.x + g.w);
                let y1 = (b.y + b.h).max(g.y + g.h);
                Geom {
                    x: x0,
                    y: y0,
                    w: x1 - x0,
                    h: y1 - y0,
                }
            }
        });
    }
    Some(TransformProj {
        sids,
        rect: bbox?,
        angle,
        skew_x,
        skew_y,
        multi: true,
    })
}

/// 写几何命令(单节点)。`old` 由命令层捕获。
pub fn set_geom_cmd(sid: &str, g: Geom) -> Command {
    Command::SetGeom {
        sid: sid.to_string(),
        new: g,
        old: None,
        old_declared: None,
    }
}

/// 写 transform 命令(角度 + 倾斜;三项全 0 → 删除该声明)。
pub fn set_transform_cmd(
    doc: &Document,
    sid: &str,
    angle: f64,
    sx: f64,
    sy: f64,
) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.nodes.get(nid)?;
    let v = build_transform(angle, sx, sy);
    Some(Command::SetStyle {
        sid: sid.to_string(),
        new: super::style_set_or_remove(n.style.clone(), "transform", v.as_deref()),
        old: None,
    })
}

/// 把多选节点的绝对包围盒换算回各自父帧的 `SetGeom`(多选缩放用)。
///
/// 口径:对**公共包围盒**按 `(kx,ky)` + 参考点缩放后,每个节点的新矩形 =
/// 它在公共包围盒内的相对位置同比例缩放(与单选的"轴心不动"一致)。
pub fn scale_multi_cmds(
    doc: &Document,
    p: &TransformProj,
    kx: f64,
    ky: f64,
    rp: RefPoint,
) -> Vec<Command> {
    let (ax, ay) = anchor_point(p.rect, rp);
    let mut out = Vec::new();
    for sid in &p.sids {
        let Some(id) = doc.find_by_sid(sid) else {
            continue;
        };
        let Some(bb) = vb_tools::abs_bbox_world(doc, id) else {
            continue;
        };
        // 节点的绝对盒 → 按同一轴心与比例缩放 → 转回父相对
        let abs = Geom {
            x: bb.x0,
            y: bb.y0,
            w: bb.x1 - bb.x0,
            h: bb.y1 - bb.y0,
        };
        let nx = ax + (abs.x - ax) * kx;
        let ny = ay + (abs.y - ay) * ky;
        let nw = abs.w * kx;
        let nh = abs.h * ky;
        // 父绝对原点(沿父链累加 geom,止于画板)
        let (px, py) = parent_abs_origin(doc, id);
        if let Some(n) = doc.nodes.get(id) {
            out.push(set_geom_cmd(
                n.sid.as_str(),
                Geom {
                    x: nx - px,
                    y: ny - py,
                    w: nw,
                    h: nh,
                },
            ));
        }
    }
    out
}

/// 节点父级的绝对原点(累加祖先 `geom.x/y`,到画板停)。
fn parent_abs_origin(doc: &Document, id: vb_doc::model::NodeId) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    let mut cur = doc.nodes.get(id).and_then(|n| n.parent);
    while let Some(pid) = cur {
        let Some(pn) = doc.nodes.get(pid) else { break };
        if matches!(pn.kind, vb_doc::model::NodeKind::Artboard) {
            break;
        }
        x += pn.geom.x;
        y += pn.geom.y;
        cur = pn.parent;
    }
    (x, y)
}

// ─────────────────────────── 面板渲染 ───────────────────────────

impl VellumApp {
    pub(crate) fn show_transform_panel(&mut self, ui: &mut egui::Ui) {
        if !self.transform_panel_open {
            return;
        }
        let mut open = true;
        egui::Window::new("变换")
            .open(&mut open)
            .collapsible(false)
            .default_width(268.0)
            .show(ui.ctx(), |ui| self.transform_panel_body(ui));
        self.transform_panel_open = open;
    }

    fn transform_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(p) = project(&self.doc, &self.selection) else {
            ui.label(caption(ui, "未选中对象 —— 选中后可数值化变换。"));
            return;
        };
        let sid = p.sids[0].clone();
        if p.multi {
            ui.label(caption(ui, "多选:数值作用于公共包围盒。"));
        }

        // ── W / H(锁链等比)+ X / Y ──
        let mut x = p.rect.x;
        let mut y = p.rect.y;
        let mut w = p.rect.w;
        let mut h = p.rect.h;
        let mut changed_geom = false;

        ui.horizontal(|ui| {
            let r = NumField::new("W", &mut w)
                .speed(1.0)
                .step(1.0)
                .unit("px")
                .width(62.0)
                .ui(ui);
            changed_geom |= r.changed;
            if ui
                .selectable_label(self.transform_lock_ratio, "🔗")
                .on_hover_text("锁定等比(改 W 时 H 同比例)")
                .clicked()
            {
                self.transform_lock_ratio = !self.transform_lock_ratio;
            }
            let r2 = NumField::new("H", &mut h)
                .speed(1.0)
                .step(1.0)
                .unit("px")
                .width(62.0)
                .ui(ui);
            changed_geom |= r2.changed;
        });
        ui.horizontal(|ui| {
            let rx = NumField::new("X", &mut x)
                .speed(1.0)
                .step(1.0)
                .unit("px")
                .width(62.0)
                .ui(ui);
            let ry = NumField::new("Y", &mut y)
                .speed(1.0)
                .step(1.0)
                .unit("px")
                .width(62.0)
                .ui(ui);
            changed_geom |= rx.changed || ry.changed;
        });

        // 等比锁:W 变则 H 同比例(用上一帧投影的尺寸做基准)
        let (kw, kh) = if p.rect.w.abs() > 1e-9 && p.rect.h.abs() > 1e-9 {
            (w / p.rect.w, h / p.rect.h)
        } else {
            (1.0, 1.0)
        };
        if changed_geom && self.transform_lock_ratio {
            let k = if (kw - 1.0).abs() > (kh - 1.0).abs() {
                kw
            } else {
                kh
            };
            if k.is_finite() && k > 0.0 {
                w = p.rect.w * k;
                h = p.rect.h * k;
            }
        }

        // ── 参考点九宫格 ──
        ui.separator();
        ui.label(caption(ui, "参考点(缩放/倾斜轴心)"));
        ui.horizontal(|ui| {
            for row in RefPoint::GRID {
                ui.vertical(|ui| {
                    for rp in row {
                        if ui
                            .selectable_label(self.transform_ref == rp, "　")
                            .on_hover_text(rp.label())
                            .clicked()
                        {
                            self.transform_ref = rp;
                        }
                    }
                });
            }
            ui.vertical(|ui| {
                ui.label(caption(ui, self.transform_ref.label()));
                if ui.button("中心").on_hover_text("复位到中心").clicked() {
                    self.transform_ref = RefPoint::MC;
                }
            });
        });

        // ── 旋转 ∠ + 倾斜 ──
        ui.separator();
        let mut angle = p.angle;
        let mut sx = p.skew_x;
        let mut sy = p.skew_y;
        let mut changed_tf = false;
        ui.horizontal(|ui| {
            let r = NumField::new("∠", &mut angle)
                .speed(1.0)
                .step(1.0)
                .unit("°")
                .width(62.0)
                .ui(ui);
            changed_tf |= r.changed;
            let r2 = NumField::new("倾斜X", &mut sx)
                .speed(1.0)
                .step(1.0)
                .unit("°")
                .width(62.0)
                .ui(ui);
            changed_tf |= r2.changed;
            let r3 = NumField::new("倾斜Y", &mut sy)
                .speed(1.0)
                .step(1.0)
                .unit("°")
                .width(62.0)
                .ui(ui);
            changed_tf |= r3.changed;
        });

        // ── 复选框(策略位,当前只影响"缩放描边"的提示) ──
        ui.checkbox(&mut self.transform_scale_stroke, "缩放描边和效果")
            .on_hover_text("HTML 无「描边随框缩放」语义:统一为几何缩放(border-width 不随之变)");
        ui.checkbox(&mut self.transform_snap_pixel, "对齐像素网格")
            .on_hover_text("几何取整到整数像素(拖动/数值输入同源)");

        // ── 提交 ──
        if changed_geom {
            let target = Geom {
                x: if self.transform_snap_pixel {
                    x.round()
                } else {
                    x
                },
                y: if self.transform_snap_pixel {
                    y.round()
                } else {
                    y
                },
                w: if self.transform_snap_pixel {
                    w.round()
                } else {
                    w
                },
                h: if self.transform_snap_pixel {
                    h.round()
                } else {
                    h
                },
            };
            let cmds = if p.multi {
                let kx = if p.rect.w.abs() > 1e-9 {
                    target.w / p.rect.w
                } else {
                    1.0
                };
                let ky = if p.rect.h.abs() > 1e-9 {
                    target.h / p.rect.h
                } else {
                    1.0
                };
                let mut c = scale_multi_cmds(&self.doc, &p, kx, ky, self.transform_ref);
                // 位移(公共包围盒 X/Y)一并作用
                let (ddx, ddy) = (target.x - p.rect.x, target.y - p.rect.y);
                if ddx.abs() > 1e-9 || ddy.abs() > 1e-9 {
                    for sid in &p.sids {
                        if let Some(id) = self.doc.find_by_sid(sid) {
                            if let Some(n) = self.doc.nodes.get(id) {
                                c.push(set_geom_cmd(
                                    n.sid.as_str(),
                                    Geom {
                                        x: n.geom.x + ddx,
                                        y: n.geom.y + ddy,
                                        ..n.geom
                                    },
                                ));
                            }
                        }
                    }
                }
                c
            } else {
                vec![set_geom_cmd(&sid, target)]
            };
            self.undo.merging_enabled = false;
            for c in cmds {
                self.exec(c);
            }
            self.undo.merging_enabled = true;
            self.transform_remember_geom(&p, target);
        }
        if changed_tf {
            if let Some(c) = set_transform_cmd(&self.doc, &sid, angle, sx, sy) {
                self.undo.merging_enabled = false;
                self.exec(c);
                self.undo.merging_enabled = true;
                self.last_transform = Some(TransformDelta {
                    d_angle: angle - p.angle,
                    ..TransformDelta::translate(0.0, 0.0)
                });
            }
        }
    }

    /// 面板改几何 → 记入"上一次变换"(供 `Mod+D` 重放)。
    fn transform_remember_geom(&mut self, p: &TransformProj, target: Geom) {
        let kx = if p.rect.w.abs() > 1e-9 {
            target.w / p.rect.w
        } else {
            1.0
        };
        let ky = if p.rect.h.abs() > 1e-9 {
            target.h / p.rect.h
        } else {
            1.0
        };
        self.last_transform = Some(TransformDelta {
            dx: target.x - p.rect.x,
            dy: target.y - p.rect.y,
            kx,
            ky,
            d_angle: 0.0,
        });
    }

    /// 记录画布拖拽产生的变换(位移/缩放/旋转),供 `Mod+D` 重放。
    pub(crate) fn remember_transform(&mut self, d: TransformDelta) {
        if !d.is_noop() {
            self.last_transform = Some(d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g() -> Geom {
        Geom {
            x: 100.0,
            y: 200.0,
            w: 40.0,
            h: 80.0,
        }
    }

    #[test]
    fn ref_point_anchors_are_correct() {
        assert_eq!(anchor_point(g(), RefPoint::TL), (100.0, 200.0));
        assert_eq!(anchor_point(g(), RefPoint::MC), (120.0, 240.0));
        assert_eq!(anchor_point(g(), RefPoint::BR), (140.0, 280.0));
        assert_eq!(anchor_point(g(), RefPoint::BC), (120.0, 280.0));
    }

    #[test]
    fn scale_about_keeps_anchor_fixed() {
        for rp in RefPoint::GRID.into_iter().flatten() {
            let a0 = anchor_point(g(), rp);
            let s = scale_about(g(), 2.0, 0.5, rp);
            let a1 = anchor_point(s, rp);
            assert!(
                (a0.0 - a1.0).abs() < 1e-9 && (a0.1 - a1.1).abs() < 1e-9,
                "{rp:?} 轴心移动了:{a0:?} → {a1:?}"
            );
        }
        // 左上轴心缩放:原点不动,尺寸翻倍
        let s = scale_about(g(), 2.0, 2.0, RefPoint::TL);
        assert_eq!((s.x, s.y, s.w, s.h), (100.0, 200.0, 80.0, 160.0));
        // 中心轴心:两侧同展
        let s = scale_about(g(), 2.0, 2.0, RefPoint::MC);
        assert_eq!((s.x, s.y, s.w, s.h), (80.0, 160.0, 80.0, 160.0));
    }

    #[test]
    fn transform_roundtrips_through_canonical_form() {
        for (angle, sx, sy) in [
            (0.0, 0.0, 0.0),
            (30.0, 0.0, 0.0),
            (0.0, 12.0, 0.0),
            (0.0, 0.0, -8.0),
            (45.0, 10.0, -5.0),
            (-90.0, 3.5, 4.5),
        ] {
            let v = build_transform(angle, sx, sy);
            match v {
                None => assert!(angle.abs() < 1e-9 && sx.abs() < 1e-9 && sy.abs() < 1e-9),
                Some(s) => {
                    // 过一遍 canonical_value 后再解析:值必须等价(写回即不动点)
                    let canon = vb_css::canonical_value(&s);
                    let (a2, x2, y2) = parse_transform(&canon);
                    assert!(
                        (a2 - angle).abs() < 1e-6
                            && (x2 - sx).abs() < 1e-6
                            && (y2 - sy).abs() < 1e-6,
                        "往返不等价:{s} → {canon}"
                    );
                    assert_eq!(build_transform(a2, x2, y2).unwrap(), canon);
                }
            }
        }
    }

    #[test]
    fn replay_applies_translate_scale_rotate() {
        // 纯位移
        let (g1, a1) = replay(
            g(),
            0.0,
            &TransformDelta::translate(10.0, -5.0),
            RefPoint::TL,
        );
        assert_eq!((g1.x, g1.y, g1.w, g1.h), (110.0, 195.0, 40.0, 80.0));
        assert_eq!(a1, 0.0);
        // 缩放 + 旋转
        let d = TransformDelta {
            dx: 0.0,
            dy: 0.0,
            kx: 2.0,
            ky: 2.0,
            d_angle: 15.0,
        };
        let (g2, a2) = replay(g(), 30.0, &d, RefPoint::TL);
        assert_eq!((g2.x, g2.y, g2.w, g2.h), (100.0, 200.0, 80.0, 160.0));
        assert_eq!(a2, 45.0);
    }

    #[test]
    fn noop_delta_is_detected() {
        assert!(TransformDelta::translate(0.0, 0.0).is_noop());
        assert!(!TransformDelta::translate(0.0, 1.0).is_noop());
        assert!(!TransformDelta {
            kx: 1.5,
            ..TransformDelta::translate(0.0, 0.0)
        }
        .is_noop());
        assert!(!TransformDelta {
            d_angle: 5.0,
            ..TransformDelta::translate(0.0, 0.0)
        }
        .is_noop());
    }
}
