//! 控制面板 · 字段编辑器(按 `CtlKind` 分派的各字段编辑 UI)。
//!
//! 06-1 自 `control_panel.rs` 渲染段按职责拆出(纯搬移,零行为变化):
//! 通栏装配与固定区在 `render`。

use super::render::num_compact;
use crate::app::panels::artboards::{AB_PRESETS, PRESET_CUSTOM};
use egui::Color32;
use vb_ui::components::{ColorField, NumField};

use super::spec::*;
use super::writes::*;
use crate::app::VellumApp;

impl VellumApp {
    /// 变换 X/Y/W/H(主选中取值;多选逐对象替换该分量)。
    pub(super) fn geom_field_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        let Some(sid) = self.selection.last().cloned() else {
            return;
        };
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            return;
        };
        let g = self.doc.nodes.get(nid).unwrap().geom;
        let (ab_w, ab_h) = self.active_artboard_size();
        let axis = match field.id {
            "x" => GeomAxis::X,
            "y" => GeomAxis::Y,
            "w" => GeomAxis::W,
            _ => GeomAxis::H,
        };
        let mut v = match axis {
            GeomAxis::X => g.x,
            GeomAxis::Y => g.y,
            GeomAxis::W => g.w,
            GeomAxis::H => g.h,
        };
        let r = num_compact(ui, field.label, &mut v)
            .percent_base(if matches!(axis, GeomAxis::X | GeomAxis::W) {
                ab_w
            } else {
                ab_h
            })
            .range(0.0, 100000.0)
            .ui(ui);
        let cmd = r.changed.then(|| {
            let sids = self.selection.clone();
            combine(geom_field_cmds(&self.doc, &sids, axis, v))
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 通用「样式数值字段」(描边粗细 / 圆角 / 字号)。
    pub(super) fn style_num_field_ui(
        &mut self,
        ui: &mut egui::Ui,
        label: &str,
        prop: &str,
        lo: f64,
        hi: f64,
    ) {
        let Some(v0) = self.primary_style_num(prop) else {
            return;
        };
        let mut v = v0;
        let r = NumField::new(label, &mut v)
            .speed(1.0)
            .step(1.0)
            .range(lo, hi)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        let cmd = r.changed.then(|| {
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(&self.doc, &sids, prop, &format!("{}px", v as i64));
            combine(cmds)
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 颜色字段(填充 / 描边 / 字色):取色器浮窗 + var(--x) +
    /// 清除 = 整条声明移除(写回一律经 style_prop_cmds)。
    pub(super) fn color_field_ui(&mut self, ui: &mut egui::Ui, label: &str, prop: &str) {
        let Some(style) = self.primary_style() else {
            return;
        };
        let cur = style
            .iter()
            .find(|d| d.prop == prop)
            .and_then(|d| vb_common::color::parse_color(&d.value));
        let mut col = cur
            .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
            .unwrap_or(Color32::WHITE);
        let tokens = self.doc.tokens.clone();
        let r = ColorField::new(label, &mut col).doc_tokens(&tokens).ui(ui);
        if let Some(name) = r.var_picked {
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(&self.doc, &sids, prop, &format!("var(--{name})"));
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say(format!("{label} → var(--{name})"));
            }
        } else if r.cleared {
            let sids = self.selection.clone();
            let cmds = style_prop_remove_cmds(&self.doc, &sids, prop);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say(format!("{label} 已清除"));
            }
        } else if r.changed {
            let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
            let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(&self.doc, &sids, prop, &hex);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    /// 描边色(border 简写 `{w}px solid {color}`;无边框以 1px 起步;
    /// 简写不支持 var()/清除 —— 未变即不写)。
    pub(super) fn border_color_ui(&mut self, ui: &mut egui::Ui, label: &str) {
        let Some((w, col)) = self.primary_border() else {
            return;
        };
        let mut c = col
            .map(|x| Color32::from_rgba_unmultiplied(x.r, x.g, x.b, x.a))
            .unwrap_or(Color32::BLACK);
        let tokens = self.doc.tokens.clone();
        let r = ColorField::new(label, &mut c).doc_tokens(&tokens).ui(ui);
        if r.var_picked.is_some() || r.cleared || !r.changed {
            return;
        }
        let [cr, cg, cb, ca] = c.to_srgba_unmultiplied();
        let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
        let sids = self.selection.clone();
        let cmds = style_prop_cmds(
            &self.doc,
            &sids,
            "border",
            &format!("{}px solid {hex}", w as i64),
        );
        if let Some(cmd) = combine(cmds) {
            self.exec(cmd);
        }
    }

    /// 描边粗细(border 简写的宽度分量;沿用当前描边色)。
    pub(super) fn border_width_ui(&mut self, ui: &mut egui::Ui, label: &str) {
        let Some((w, col)) = self.primary_border() else {
            return;
        };
        let mut v = w;
        let r = NumField::new(label, &mut v)
            .speed(0.5)
            .step(1.0)
            .range(0.0, 100.0)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        let cmd = r.changed.then(|| {
            let hex = col
                .map(|c| c.to_shortest_hex())
                .unwrap_or_else(|| "#1a1a1a".into()); // vb-token-ok: 文档内容色
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(
                &self.doc,
                &sids,
                "border",
                &format!("{}px solid {hex}", v as i64),
            );
            combine(cmds)
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 文字对齐(text-align;真实 CSS,浏览器校对可见;画布为近似渲染)。
    pub(super) fn text_align_ui(&mut self, ui: &mut egui::Ui) {
        let cur = self
            .primary_style()
            .and_then(|s| {
                s.iter()
                    .find(|d| d.prop == "text-align")
                    .map(|d| d.value.clone())
            })
            .unwrap_or_else(|| "left".into());
        let mut sel = cur.clone();
        egui::ComboBox::from_id_salt("ctl_text_align")
            .selected_text(format!("对齐 {sel}"))
            .show_ui(ui, |ui| {
                for v in ["left", "center", "right", "justify"] {
                    ui.selectable_value(&mut sel, v.to_string(), v);
                }
            });
        if sel != cur {
            let sids = self.selection.clone();
            let cmds = style_prop_cmds(&self.doc, &sids, "text-align", &sel);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    /// 渐变类型(线性 ↔ 径向,保留色标)。
    pub(super) fn gradient_kind_ui(&mut self, ui: &mut egui::Ui) {
        let cur_kind = self.primary_gradient().map(|(k, _, _)| k);
        let mut sel = cur_kind.unwrap_or(GradKind::Linear);
        egui::ComboBox::from_id_salt("ctl_g_kind")
            .selected_text(match sel {
                GradKind::Linear => "线性",
                GradKind::Radial => "径向",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut sel, GradKind::Linear, "线性");
                ui.selectable_value(&mut sel, GradKind::Radial, "径向");
            });
        if cur_kind == Some(sel) {
            return;
        }
        let sids = self.selection.clone();
        let cmds = gradient_kind_cmds(&self.doc, &sids, sel);
        if let Some(cmd) = combine(cmds) {
            self.exec(cmd);
        }
    }

    /// 渐变角度数值化(在现有拖方向能力之上;无渐变回退生成双色)。
    pub(super) fn gradient_angle_ui(&mut self, ui: &mut egui::Ui) {
        let Some((_, a0, _)) = self.primary_gradient() else {
            return;
        };
        let mut v = a0;
        let r = NumField::new("角度", &mut v)
            .speed(1.0)
            .step(15.0)
            .range(0.0, 360.0)
            .unit("°")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        let cmd = r.changed.then(|| {
            let sids = self.selection.clone();
            let cmds = gradient_angle_cmds(&self.doc, &sids, v);
            combine(cmds)
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 画板预设下拉(改活动画板 W/H;匹配当前尺寸时高亮显示名称)。
    pub(super) fn artboard_preset_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (w, h, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.geom.w, n.geom.h, n.sid.as_str().to_string())
        };
        let label = AB_PRESETS
            .iter()
            .find(|(_, pw, ph)| (*pw - w).abs() < 0.5 && (*ph - h).abs() < 0.5)
            .map(|(n, _, _)| *n)
            .unwrap_or(PRESET_CUSTOM);
        egui::ComboBox::from_id_salt("ctl_ab_preset")
            .selected_text(format!("预设 {label}"))
            .show_ui(ui, |ui| {
                for (name, pw, ph) in AB_PRESETS {
                    let active = (pw - w).abs() < 0.5 && (ph - h).abs() < 0.5;
                    if ui.selectable_label(active, name).clicked() {
                        if let Some(cmd) =
                            geom_axes_cmd(&self.doc, &sid, &[(GeomAxis::W, pw), (GeomAxis::H, ph)])
                        {
                            self.exec(cmd);
                            self.say(format!("画板预设 → {name}"));
                        }
                    }
                }
            });
    }

    /// 画板取向(横/竖:w、h 互换,经 SetGeom)。
    pub(super) fn artboard_orient_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (w, h, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.geom.w, n.geom.h, n.sid.as_str().to_string())
        };
        let mut sel = if w >= h { "横" } else { "竖" };
        let before = sel;
        egui::ComboBox::from_id_salt("ctl_ab_orient")
            .selected_text(format!("取向 {sel}"))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut sel, "横", "横向");
                ui.selectable_value(&mut sel, "竖", "纵向");
            });
        if sel != before {
            if let Some(cmd) = geom_axes_cmd(
                &self.doc,
                &sid,
                &[(GeomAxis::W, h.max(1.0)), (GeomAxis::H, w.max(1.0))],
            ) {
                self.exec(cmd);
                self.say(format!(
                    "画板取向 → {}",
                    if sel == "横" { "横向" } else { "纵向" }
                ));
            }
        }
    }

    /// 画板尺寸 W/H(活动画板;SetGeom)。
    pub(super) fn artboard_size_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (g, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.geom, n.sid.as_str().to_string())
        };
        let is_w = field.id == "ab.w";
        let mut v = if is_w { g.w } else { g.h };
        let r = NumField::new(field.label, &mut v)
            .speed(1.0)
            .step(1.0)
            .range(1.0, 100000.0)
            .label_width(20.0)
            .width(56.0)
            .percent_base(if is_w { g.w.max(1.0) } else { g.h.max(1.0) })
            .ui(ui);
        let cmd = r.changed.then(|| {
            let axis = if is_w { GeomAxis::W } else { GeomAxis::H };
            combine(geom_field_cmds(&self.doc, &[sid], axis, v))
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 画板位置 X/Y(画板 geom 即世界坐标)。
    pub(super) fn artboard_pos_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (g, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.geom, n.sid.as_str().to_string())
        };
        let is_x = field.id == "ab.x";
        let mut v = if is_x { g.x } else { g.y };
        let r = num_compact(ui, field.label, &mut v).ui(ui);
        let cmd = r.changed.then(|| {
            let axis = if is_x { GeomAxis::X } else { GeomAxis::Y };
            combine(geom_field_cmds(&self.doc, &[sid], axis, v))
        });
        self.num_commit(r, cmd.flatten());
    }

    /// 画板名称(Rename;画板工具态)。
    pub(super) fn artboard_name_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (sid, name) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.sid.as_str().to_string(), n.name.clone())
        };
        let mut buf = name.clone();
        if ui
            .add_sized(
                [120.0, vb_ui::theme::row_height(ui.ctx())],
                egui::TextEdit::singleline(&mut buf),
            )
            .lost_focus()
            && buf != name
            && !buf.trim().is_empty()
        {
            let cmds = rename_cmds(&self.doc, &[sid], buf.trim());
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say(format!("画板已改名 → {}", buf.trim()));
            }
        }
    }

    /// 画板背景色(活动画板节点的 background-color;SetStyle)。
    pub(super) fn artboard_bg_ui(&mut self, ui: &mut egui::Ui) {
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let (style, sid) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (n.style.clone(), n.sid.as_str().to_string())
        };
        let cur = style
            .iter()
            .find(|d| d.prop == "background-color")
            .and_then(|d| vb_common::color::parse_color(&d.value));
        let mut col = cur
            .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
            .unwrap_or(Color32::WHITE);
        let tokens = self.doc.tokens.clone();
        let r = ColorField::new("画板底", &mut col)
            .doc_tokens(&tokens)
            .ui(ui);
        if let Some(name) = r.var_picked {
            let cmds = style_prop_cmds(
                &self.doc,
                &[sid],
                "background-color",
                &format!("var(--{name})"),
            );
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say(format!("画板底 → var(--{name})"));
            }
        } else if r.cleared {
            let cmds = style_prop_remove_cmds(&self.doc, &[sid], "background-color");
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
                self.say("画板底已清除");
            }
        } else if r.changed {
            let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
            let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
            let cmds = style_prop_cmds(&self.doc, &[sid], "background-color", &hex);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    /// 锚点坐标(直接选择:当前锚点 = 拖拽/点选中的,否则选中矢量第 0 个;
    /// 写回 SetVector,与画布锚点拖拽同一路径)。
    pub(super) fn anchor_field_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        let Some((sid, vi)) = self.anchor_target() else {
            ui.weak("选中矢量路径后可改锚点坐标");
            return;
        };
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            return;
        };
        let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
            return;
        };
        let Some((_, wx, wy)) = self
            .vector_vertices(&sid)
            .into_iter()
            .find(|(i, _, _)| *i == vi)
        else {
            return;
        };
        let is_x = field.id == "ax";
        let mut v = if is_x { wx } else { wy };
        let r = num_compact(ui, field.label, &mut v).speed(1.0).ui(ui);
        let cmd = r.changed.then(|| {
            let local = if is_x {
                (v - bb.x0, wy - bb.y0)
            } else {
                (wx - bb.x0, v - bb.y0)
            };
            self.build_anchor_cmd(&sid, vi, local)
        });
        self.num_commit(r, cmd.flatten());
    }
}
