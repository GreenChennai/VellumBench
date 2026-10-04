//! 描边面板(`^F10`,05-3)的渲染层。
//!
//! 06-1 自 `appearance/ui.rs` 按「外观面板 / 描边面板」拆出
//! (纯搬移,零行为变化);投影与提交辅助在父模块 `ui`。

use egui;
use vb_ui::components::{caption, icon_button, ColorField, NumField};
use vb_ui::icons;

use super::{
    add_stroke_cmd, set_stroke_spec_cmd, AppearanceItem, AppearanceTarget, Arrowhead, StrokeAlign,
    StrokeCap, StrokeJoin, StrokeSpec,
};
use crate::app::VellumApp;

impl VellumApp {
    // ─────────────── 描边面板(^F10;05-3) ───────────────

    pub(crate) fn stroke_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.appearance_projection() else {
            ui.label(caption(
                ui,
                &vb_session::i18n::t("ui-app-appearance-stroke-001"),
            ));
            return;
        };
        let t = super::target_of(&p.kind);
        if t == AppearanceTarget::Frozen {
            ui.label(caption(
                ui,
                &vb_session::i18n::t("ui-app-appearance-stroke-002"),
            ));
            return;
        }
        let target_name = match t {
            AppearanceTarget::Box => vb_session::i18n::t("ui-app-appearance-stroke-003"),
            AppearanceTarget::Text => vb_session::i18n::t("ui-app-appearance-stroke-004"),
            AppearanceTarget::Vector => vb_session::i18n::t("ui-app-appearance-stroke-005"),
            AppearanceTarget::Frozen => vb_session::i18n::t("ui-common-freeze-block"),
        };
        ui.label(caption(
            ui,
            &vb_session::i18n::t_args(
                "ui-app-appearance-stroke-006",
                &[(
                    "target_name",
                    vb_session::i18n::FluentValue::from((target_name).to_string()),
                )],
            ),
        ));
        ui.separator();
        let Some(idx) = p
            .model
            .items
            .iter()
            .position(|i| matches!(i, AppearanceItem::Stroke(_)))
        else {
            ui.label(caption(
                ui,
                &vb_session::i18n::t("ui-app-appearance-stroke-007"),
            ));
            if ui
                .button(vb_session::i18n::t("ui-app-appearance-stroke-008"))
                .clicked()
            {
                let sid = p.sid.clone();
                self.exec_appearance(add_stroke_cmd(&self.doc, &sid));
            }
            return;
        };
        let sid = p.sid.clone();
        let mut spec = match &p.model.items[idx] {
            AppearanceItem::Stroke(s) => s.spec.clone(),
            _ => return,
        };
        let is_vector = t == AppearanceTarget::Vector;
        let is_box = t == AppearanceTarget::Box;

        // 颜色 / 粗细(全目标可用)
        let mut col = spec
            .color
            .as_deref()
            .and_then(vb_common::color::parse_color)
            .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
            .unwrap_or(egui::Color32::BLACK);
        let tokens = self.doc.tokens.clone();
        let r = ColorField::new(&vb_session::i18n::t("ui-common-color"), &mut col)
            .doc_tokens(&tokens)
            .ui(ui);
        if let Some(nm) = r.var_picked {
            spec.color = Some(format!("var(--{nm})"));
            self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone()));
        } else if r.changed && !r.cleared {
            let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
            spec.color = Some(vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex());
            self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone()));
        }
        let r = NumField::new(&vb_session::i18n::t("ui-common-weight"), &mut spec.width)
            .speed(0.5)
            .step(1.0)
            .range(0.0, 200.0)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        if r.changed {
            let res = set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone());
            self.appearance_num_apply(r, res);
        }

        // 端点(05-3-1;仅矢量有 CSS/SVG 落点,其余置灰 + 悬停说明)
        let mut cap = spec.cap;
        egui::ComboBox::from_id_salt("vb_stroke_cap")
            .selected_text(vb_session::i18n::t_args(
                "ui-app-appearance-stroke-009",
                &[(
                    "a1",
                    vb_session::i18n::FluentValue::from((cap.label()).to_string()),
                )],
            ))
            .show_ui(ui, |ui| {
                for c in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
                    let resp = if is_vector {
                        ui.selectable_value(&mut cap, c, egui::RichText::new(c.label()).weak())
                    } else {
                        ui.add_enabled(
                            false,
                            egui::Button::new(egui::RichText::new(c.label()).weak()),
                        )
                    };
                    if !is_vector {
                        resp.on_hover_text(vb_session::i18n::t("ui-app-appearance-stroke-010"));
                    }
                }
            });
        if cap != spec.cap {
            spec.cap = cap;
            self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone()));
        }

        // 边角(仅矢量;盒对象的边角由圆角决定)
        let mut join = spec.join;
        egui::ComboBox::from_id_salt("vb_stroke_join")
            .selected_text(vb_session::i18n::t_args(
                "ui-app-appearance-stroke-011",
                &[(
                    "a1",
                    vb_session::i18n::FluentValue::from((join.label()).to_string()),
                )],
            ))
            .show_ui(ui, |ui| {
                for j in [StrokeJoin::Miter, StrokeJoin::Round, StrokeJoin::Bevel] {
                    let resp = if is_vector {
                        ui.selectable_value(&mut join, j, egui::RichText::new(j.label()).weak())
                    } else {
                        ui.add_enabled(
                            false,
                            egui::Button::new(egui::RichText::new(j.label()).weak()),
                        )
                    };
                    if !is_vector {
                        resp.on_hover_text(vb_session::i18n::t("ui-app-appearance-stroke-012"));
                    }
                }
            });
        if join != spec.join {
            spec.join = join;
            self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone()));
        }

        // 斜接限制(仅矢量 + 斜接;05-3-1)
        if is_vector {
            let r = NumField::new(
                &vb_session::i18n::t("ui-app-appearance-stroke-013"),
                &mut spec.miter_limit,
            )
            .speed(0.1)
            .step(1.0)
            .range(1.0, 100.0)
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
            if r.changed {
                let res = set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone());
                self.appearance_num_apply(r, res);
            }
        }

        // 虚线(05-3-2;值/间隙最多 3 对 = 6 组;仅矢量可自定义,
        // 盒对象按「有虚线 → dashed」近似落盘)
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-app-appearance-stroke-014"));
            let pairs = spec.dash.len().div_ceil(2);
            for pi in 0..pairs {
                let mut on = spec.dash[pi * 2];
                let mut off = spec.dash.get(pi * 2 + 1).copied().unwrap_or(0.0);
                let r1 = NumField::new(&vb_session::i18n::t("ui-common-value"), &mut on)
                    .speed(0.5)
                    .range(0.0, 200.0)
                    .width(44.0)
                    .label_width(16.0)
                    .ui(ui);
                let r2 = NumField::new(
                    &vb_session::i18n::t("ui-app-appearance-stroke-015"),
                    &mut off,
                )
                .speed(0.5)
                .range(0.0, 200.0)
                .width(44.0)
                .label_width(16.0)
                .ui(ui);
                if r1.changed || r2.changed {
                    spec.dash[pi * 2] = on;
                    if spec.dash.len() > pi * 2 + 1 {
                        spec.dash[pi * 2 + 1] = off;
                    } else {
                        spec.dash.push(off);
                    }
                    let res = set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone());
                    self.appearance_num_apply(if r2.changed { r2 } else { r1 }, res);
                }
            }
            if pairs < 3
                && icon_button(
                    ui,
                    icons::Name::AddChild,
                    &vb_session::i18n::t("ui-app-appearance-stroke-016"),
                )
                .clicked()
            {
                spec.dash.push(6.0);
                spec.dash.push(3.0);
                self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone()));
            }
            if pairs > 0
                && icon_button(
                    ui,
                    icons::Name::Delete,
                    &vb_session::i18n::t("ui-app-appearance-stroke-017"),
                )
                .clicked()
            {
                spec.dash.truncate((pairs - 1) * 2);
                self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone()));
            }
        });
        if !is_vector {
            ui.label(caption(
                ui,
                &vb_session::i18n::t("ui-app-appearance-stroke-018"),
            ));
        }

        // 对齐(05-3-2):盒 = border(内侧)/outline(外侧)/居中降级;矢量恒居中
        let mut align = spec.align;
        egui::ComboBox::from_id_salt("vb_stroke_align")
            .selected_text(vb_session::i18n::t_args(
                "ui-app-appearance-stroke-019",
                &[(
                    "a1",
                    vb_session::i18n::FluentValue::from((align.label()).to_string()),
                )],
            ))
            .show_ui(ui, |ui| {
                for a in [
                    StrokeAlign::Center,
                    StrokeAlign::Inside,
                    StrokeAlign::Outside,
                ] {
                    let resp = if is_box {
                        ui.selectable_value(&mut align, a, egui::RichText::new(a.label()).weak())
                    } else {
                        ui.add_enabled(
                            false,
                            egui::Button::new(egui::RichText::new(a.label()).weak()),
                        )
                    };
                    if !is_box {
                        resp.on_hover_text(vb_session::i18n::t("ui-app-appearance-stroke-020"));
                    }
                }
            });
        if align != spec.align {
            spec.align = align;
            self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone()));
        }
        if is_box && spec.align == StrokeAlign::Center {
            ui.label(caption(
                ui,
                &vb_session::i18n::t("ui-app-appearance-stroke-021"),
            ));
        }

        // 箭头起止(矢量可选,选择 = 冻结登记提示;CSS/SVG marker 未建模)
        ui.horizontal(|ui| {
            let mut a0 = spec.arrow_start;
            let mut a1 = spec.arrow_end;
            egui::ComboBox::from_id_salt("vb_arrow_start")
                .selected_text(vb_session::i18n::t_args(
                    "ui-app-appearance-stroke-022",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from((a0.label()).to_string()),
                    )],
                ))
                .show_ui(ui, |ui| {
                    for a in [
                        Arrowhead::None,
                        Arrowhead::Arrow,
                        Arrowhead::Circle,
                        Arrowhead::Tick,
                    ] {
                        ui.selectable_value(&mut a0, a, egui::RichText::new(a.label()).weak());
                    }
                });
            egui::ComboBox::from_id_salt("vb_arrow_end")
                .selected_text(vb_session::i18n::t_args(
                    "ui-app-appearance-stroke-023",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from((a1.label()).to_string()),
                    )],
                ))
                .show_ui(ui, |ui| {
                    for a in [
                        Arrowhead::None,
                        Arrowhead::Arrow,
                        Arrowhead::Circle,
                        Arrowhead::Tick,
                    ] {
                        ui.selectable_value(&mut a1, a, egui::RichText::new(a.label()).weak());
                    }
                });
            let changed = a0 != spec.arrow_start || a1 != spec.arrow_end;
            if changed && !is_vector {
                self.toast_warn(vb_session::i18n::t("ui-app-appearance-stroke-024"));
            } else if changed {
                self.toast_warn(vb_session::i18n::t("ui-app-appearance-stroke-025"));
                spec.arrow_start = a0;
                spec.arrow_end = a1;
                self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, idx, spec.clone()));
            }
        });
    }
}

/// 读第 `index` 条描边的当前参数(粗细数值框取值用)。
pub(super) fn load_stroke(
    doc: &vb_doc::model::Document,
    sid: &str,
    index: usize,
) -> Option<StrokeSpec> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.node(nid)?;
    let attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    match super::decode_model(&n.kind, &n.style, &attrs)
        .items
        .get(index)
    {
        Some(AppearanceItem::Stroke(s)) => Some(s.spec.clone()),
        _ => None,
    }
}
