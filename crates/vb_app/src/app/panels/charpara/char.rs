//! 字符面板(`⇧^F7`)渲染:投影 / 字段编辑 / run 与整段写回提交。
//!
//! 06-1 自 `panels/charpara.rs` 渲染段按面板拆出(纯搬移,零行为变化);
//! 段落面板在 `para`,字段规格在 `fields`。

use vb_css::Decl;
use vb_doc::model::{NodeKind, SegStyle, TextMode};

use crate::app::control_panel::{combine, style_prop_cmds, style_prop_remove_cmds};
use egui::Color32;
use vb_ui::components::{caption, ColorField, NumField};
use vb_ui::theme;

use super::fields::*;
use super::model::*;
use crate::app::VellumApp;

// ═══════════════════ 5. 渲染 ═══════════════════

/// 主选中文本的投影(面板每帧从文档回读,**不在面板私存**)。
pub(super) struct TextProj {
    pub(super) sid: String,
    pub(super) has_runs: bool,
    pub(super) mode: TextMode,
    pub(super) style: Vec<Decl>,
    /// 首 run 样式(run 作用域的取值投影)。
    pub(super) seg: Option<SegStyle>,
}

impl VellumApp {
    pub(super) fn text_projection(&self) -> Option<TextProj> {
        let sid = self.selection.last()?.clone();
        let nid = self.doc.find_by_sid(&sid)?;
        let n = self.doc.nodes.get(nid)?;
        let NodeKind::Text { mode, segments, .. } = &n.kind else {
            return None;
        };
        Some(TextProj {
            sid,
            has_runs: !segments.is_empty(),
            mode: *mode,
            style: n.style.clone(),
            seg: segments.first().map(|s| s.style.clone()),
        })
    }

    // ─────────────── 字符面板 ───────────────

    pub(crate) fn char_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.text_projection() else {
            // 无文本选中:置灰 + 引导(design/14 空态);默认样式区仍可用
            ui.label(caption(
                ui,
                &vb_session::i18n::t("ui-app-panels-charpara-char-001"),
            ));
            ui.separator();
            self.default_style_body(ui);
            return;
        };
        let runs = p.has_runs;
        ui.horizontal(|ui| {
            ui.label(if runs {
                vb_session::i18n::t("ui-app-panels-charpara-char-002")
            } else {
                vb_session::i18n::t("ui-app-panels-charpara-char-003")
            });
            if runs {
                if ui
                    .button(vb_session::i18n::t("ui-app-panels-charpara-char-004"))
                    .clicked()
                {
                    if let Some(cmd) = clear_runs_cmd(&self.doc, &p.sid) {
                        self.exec(cmd);
                        self.say(vb_session::i18n::t("ui-app-panels-charpara-char-005"));
                    }
                }
            } else {
                let can = make_full_run_cmd(&self.doc, &p.sid).is_some();
                if ui
                    .add_enabled(
                        can,
                        egui::Button::new(vb_session::i18n::t("ui-app-panels-charpara-char-006")),
                    )
                    .clicked()
                {
                    if let Some(cmd) = make_full_run_cmd(&self.doc, &p.sid) {
                        self.exec(cmd);
                        self.say(vb_session::i18n::t("ui-app-panels-charpara-char-007"));
                    }
                }
            }
        });
        ui.separator();

        // ── 字段区(run 作用域禁用仅节点级字段;整段禁用仅 run 级字段) ──
        let sid = p.sid.clone();
        let style = p.style.clone();
        let seg = p.seg.unwrap_or_default();

        // 字体族(TextEdit 失焦提交;空 = 清除回继承)
        let node_family = style
            .iter()
            .find(|d| d.prop == "font-family")
            .map(|d| d.value.clone());
        let mut family = if runs {
            seg.font_family.clone().unwrap_or_default()
        } else {
            node_family.clone().unwrap_or_default()
        };
        ui.horizontal(|ui| {
            ui.label(vb_session::i18n::t("ui-common-font"));
            // 04-3-1:高度从字号派生(P1-② 压叠根因:写死 18pt 装不下 CJK 字形)
            let h = theme::row_height(ui.ctx());
            if ui
                .add_sized([150.0, h], egui::TextEdit::singleline(&mut family))
                .lost_focus()
            {
                let t = family.trim().to_string();
                self.commit_char_family(&sid, runs, node_family.as_deref(), &t);
            }
        });
        let families = doc_font_families(&self.doc);
        if !families.is_empty() {
            ui.horizontal(|ui| {
                ui.label(caption(
                    ui,
                    &vb_session::i18n::t("ui-app-panels-charpara-char-008"),
                ));
                for f in families.iter().take(3) {
                    if ui.selectable_label(false, f).clicked() {
                        self.commit_char_family(&sid, runs, node_family.as_deref(), f);
                    }
                }
            });
        }

        // 粗体 / 斜体
        ui.horizontal(|ui| {
            let node_bold = style
                .iter()
                .find(|d| d.prop == "font-weight")
                .map(|d| matches!(d.value.as_str(), "bold" | "600" | "700" | "800" | "900"))
                .unwrap_or(false);
            let mut bold = if runs {
                seg.bold == Some(true)
            } else {
                node_bold
            };
            if ui
                .checkbox(&mut bold, vb_session::i18n::t("ui-common-bold"))
                .changed()
            {
                self.commit_char_bold(&sid, runs, bold);
            }
            let node_italic = style
                .iter()
                .find(|d| d.prop == "font-style")
                .map(|d| d.value == "italic" || d.value == "oblique")
                .unwrap_or(false);
            let mut italic = if runs {
                seg.italic == Some(true)
            } else {
                node_italic
            };
            if ui
                .checkbox(&mut italic, vb_session::i18n::t("ui-common-italic"))
                .changed()
            {
                self.commit_char_italic(&sid, runs, italic);
            }
        });

        // 大小 / 行距 / 字距(scrubby NumField;undo 会话同控制面板)
        self.char_px_field(
            ui,
            &SIZE_SPEC,
            runs,
            &sid,
            seg.font_size,
            style_font_px(&style, SIZE_SPEC.prop),
        );
        self.char_px_field(
            ui,
            &LH_SPEC,
            runs,
            &sid,
            seg.line_height,
            style_font_px(&style, LH_SPEC.prop),
        );
        self.char_px_field(
            ui,
            &TRACK_SPEC,
            runs,
            &sid,
            seg.letter_spacing,
            style_font_px(&style, TRACK_SPEC.prop),
        );

        // 基线偏移(仅 run 落点)
        ui.add_enabled_ui(runs, |ui| {
            let mut bs = seg.baseline_shift.unwrap_or(0.0);
            let r = NumField::new(
                &vb_session::i18n::t("ui-app-panels-charpara-char-009"),
                &mut bs,
            )
            .speed(0.5)
            .step(1.0)
            .range(-200.0, 200.0)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
            let cmd = r.changed.then(|| {
                seg_field_cmd(&self.doc, &sid, move |s| {
                    s.baseline_shift = if bs == 0.0 { None } else { Some(bs) };
                })
            });
            self.num_commit(r, cmd.flatten());
        });
        if !runs {
            ui.label(caption(
                ui,
                &vb_session::i18n::t("ui-app-panels-charpara-char-010"),
            ));
        }

        // 下划线 / 删除线
        ui.horizontal(|ui| {
            let node_deco = style
                .iter()
                .find(|d| d.prop == "text-decoration")
                .map(|d| d.value.clone())
                .unwrap_or_default();
            let mut ul = if runs {
                seg.underline == Some(true)
            } else {
                node_deco.contains("underline")
            };
            let mut st = if runs {
                seg.strikethrough == Some(true)
            } else {
                node_deco.contains("line-through")
            };
            if ui
                .checkbox(
                    &mut ul,
                    vb_session::i18n::t("ui-app-panels-charpara-char-011"),
                )
                .changed()
            {
                self.commit_char_deco(&sid, runs, ul, st);
            }
            if ui
                .checkbox(
                    &mut st,
                    vb_session::i18n::t("ui-app-panels-charpara-char-012"),
                )
                .changed()
            {
                self.commit_char_deco(&sid, runs, ul, st);
            }
        });

        // 语言(仅整段:lang 属性)
        ui.add_enabled_ui(!runs, |ui| {
            let mut lang = self
                .doc
                .find_by_sid(&sid)
                .and_then(|nid| self.doc.nodes.get(nid))
                .and_then(|n| {
                    n.attrs
                        .iter()
                        .find(|(k, _)| k.as_str() == "lang")
                        .map(|(_, v)| v.clone())
                })
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(vb_session::i18n::t("ui-app-panels-charpara-char-013"));
                let h = theme::row_height(ui.ctx());
                if ui
                    .add_sized([120.0, h], egui::TextEdit::singleline(&mut lang))
                    .lost_focus()
                {
                    let sids = vec![sid.clone()];
                    let cmds =
                        crate::app::control_panel::attr_cmds(&self.doc, &sids, "lang", lang.trim());
                    if let Some(cmd) = combine(cmds) {
                        self.exec(cmd);
                    }
                }
            });
        });
        if runs {
            ui.label(caption(
                ui,
                &vb_session::i18n::t("ui-app-panels-charpara-char-014"),
            ));
        }

        // 抗锯齿(仅整段;-webkit-font-smoothing)
        ui.add_enabled_ui(!runs, |ui| {
            let cur = style
                .iter()
                .find(|d| d.prop == "-webkit-font-smoothing")
                .map(|d| d.value.clone());
            let mut sel = cur.clone().unwrap_or_else(|| "auto".into());
            egui::ComboBox::from_id_salt("char_smoothing")
                .selected_text(vb_session::i18n::t_args(
                    "ui-app-panels-charpara-char-015",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            (if sel == "auto" {
                                vb_session::i18n::t("ui-app-panels-charpara-char-016")
                            } else {
                                sel.as_str().to_string()
                            })
                            .to_string(),
                        ),
                    )],
                ))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut sel,
                        "auto".into(),
                        vb_session::i18n::t("ui-app-panels-charpara-char-016"),
                    );
                    for v in [
                        "antialiased",
                        "subpixel-antialiased",
                        "none",
                        "optimizeLegibility",
                    ] {
                        ui.selectable_value(&mut sel, v.to_string(), v);
                    }
                });
            if sel != cur.clone().unwrap_or_else(|| "auto".into()) {
                let sids = vec![sid.clone()];
                let cmds = if sel == "auto" {
                    style_prop_remove_cmds(&self.doc, &sids, "-webkit-font-smoothing")
                } else {
                    style_prop_cmds(&self.doc, &sids, "-webkit-font-smoothing", &sel)
                };
                if let Some(cmd) = combine(cmds) {
                    self.exec(cmd);
                }
            }
        });

        // 冻结登记(design/03 §5.10 有字段名、CSS 无对称往返落点)
        ui.separator();
        ui.label(caption(
            ui,
            &vb_session::i18n::t("ui-app-panels-charpara-char-017"),
        ));
        ui.label(caption(
            ui,
            &vb_session::i18n::t("ui-app-panels-charpara-char-018"),
        ));

        ui.separator();
        self.default_style_body(ui);
    }

    /// 新建文本默认样式(会话级;04-3-3:新建文本继承此处,替换写死 24px/黑)。
    fn default_style_body(&mut self, ui: &mut egui::Ui) {
        ui.strong(vb_session::i18n::t("ui-app-panels-charpara-char-019"));
        let mut fs = self.text_default.font_size.unwrap_or(24.0);
        let r = NumField::new(&vb_session::i18n::t("ui-common-font-size"), &mut fs)
            .speed(1.0)
            .step(1.0)
            .range(1.0, 500.0)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        if r.changed {
            self.text_default.font_size = Some(fs);
        }
        let mut lh = self.text_default.line_height.unwrap_or(0.0);
        let r = NumField::new(&vb_session::i18n::t("ui-common-line-height"), &mut lh)
            .speed(1.0)
            .step(1.0)
            .range(0.0, 2000.0)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        if r.changed {
            self.text_default.line_height = if lh > 0.0 { Some(lh) } else { None };
        }
        let mut ls = self.text_default.letter_spacing.unwrap_or(0.0);
        let r = NumField::new(&vb_session::i18n::t("ui-common-letter-spacing"), &mut ls)
            .speed(0.5)
            .step(1.0)
            .range(-50.0, 500.0)
            .unit("px")
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        if r.changed {
            self.text_default.letter_spacing = if ls != 0.0 { Some(ls) } else { None };
        }
        ui.horizontal(|ui| {
            let mut bold = self.text_default.bold == Some(true);
            if ui
                .checkbox(&mut bold, vb_session::i18n::t("ui-common-bold"))
                .changed()
            {
                self.text_default.bold = Some(bold);
            }
            let mut italic = self.text_default.italic == Some(true);
            if ui
                .checkbox(&mut italic, vb_session::i18n::t("ui-common-italic"))
                .changed()
            {
                self.text_default.italic = Some(italic);
            }
        });
        // 默认字色
        let cur = self
            .text_default
            .color
            .clone()
            .unwrap_or_else(|| "#1a1a1a".into()); // vb-token-ok: 新建文本默认字色(文档内容)
        let mut col = vb_common::color::parse_color(&cur)
            .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
            .unwrap_or(Color32::BLACK);
        let r = ColorField::new(&vb_session::i18n::t("ui-common-text-color"), &mut col).ui(ui);
        if r.changed && !r.cleared {
            let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
            self.text_default.color = Some(vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex());
        }
        ui.label(caption(
            ui,
            &vb_session::i18n::t("ui-app-panels-charpara-char-020"),
        ));
    }

    // ── 字段提交辅助(整段 / run 双路) ──

    fn commit_char_family(&mut self, sid: &str, runs: bool, node_family: Option<&str>, t: &str) {
        if runs {
            let next = (!t.is_empty()).then(|| t.to_string());
            if let Some(cmd) = seg_field_cmd(&self.doc, sid, move |s| s.font_family = next.clone())
            {
                self.exec(cmd);
            }
        } else if Some(t) != node_family.map(str::trim) {
            let sids = vec![sid.to_string()];
            let cmds = if t.is_empty() {
                style_prop_remove_cmds(&self.doc, &sids, "font-family")
            } else {
                style_prop_cmds(&self.doc, &sids, "font-family", t)
            };
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    fn commit_char_bold(&mut self, sid: &str, runs: bool, bold: bool) {
        if runs {
            if let Some(cmd) = seg_field_cmd(&self.doc, sid, move |s| {
                s.bold = if bold { Some(true) } else { None }
            }) {
                self.exec(cmd);
            }
        } else {
            let sids = vec![sid.to_string()];
            let cmds = char_bold_cmds(&self.doc, &sids, bold);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    fn commit_char_italic(&mut self, sid: &str, runs: bool, italic: bool) {
        if runs {
            if let Some(cmd) = seg_field_cmd(&self.doc, sid, move |s| {
                s.italic = if italic { Some(true) } else { None }
            }) {
                self.exec(cmd);
            }
        } else {
            let sids = vec![sid.to_string()];
            let cmds = char_italic_cmds(&self.doc, &sids, italic);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    fn commit_char_deco(&mut self, sid: &str, runs: bool, ul: bool, strike: bool) {
        if runs {
            if let Some(cmd) = seg_field_cmd(&self.doc, sid, move |s| {
                s.underline = ul.then_some(true);
                s.strikethrough = strike.then_some(true);
            }) {
                self.exec(cmd);
            }
        } else {
            let sids = vec![sid.to_string()];
            let cmds = char_deco_cmds(&self.doc, &sids, ul, strike);
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }

    /// 字符面板 px 数值字段(大小/行距/字距):整段 → SetStyle;
    /// run → SetSegs;投影 None 时整段不画死控件(无声明 = 继承,给出 caption)。
    fn char_px_field(
        &mut self,
        ui: &mut egui::Ui,
        spec: &PxFieldSpec,
        runs: bool,
        sid: &str,
        seg_val: Option<f64>,
        node_val: Option<f64>,
    ) {
        let label = spec.label;
        let (lo, hi) = (spec.lo, spec.hi);
        if runs {
            let mut v = seg_val.unwrap_or(0.0);
            let has = seg_val.is_some();
            let r = NumField::new(label, &mut v)
                .speed(0.5)
                .step(1.0)
                .range(lo, hi)
                .unit("px")
                .label_width(44.0)
                .width(56.0)
                .ui(ui);
            let cmd = r.changed.then(|| {
                let prop = spec.prop;
                let next = v; // 克隆进闭包
                seg_field_cmd(&self.doc, sid, move |s| match prop {
                    "font-size" => s.font_size = Some(next),
                    "line-height" => s.line_height = Some(next),
                    _ => s.letter_spacing = Some(next),
                })
            });
            self.num_commit(r, cmd.flatten());
            if !has {
                ui.label(caption(
                    ui,
                    &vb_session::i18n::t("ui-app-panels-charpara-char-021"),
                ));
            }
        } else if let Some(v0) = node_val {
            let mut v = v0;
            let r = NumField::new(label, &mut v)
                .speed(0.5)
                .step(1.0)
                .range(lo, hi)
                .unit("px")
                .label_width(44.0)
                .width(56.0)
                .ui(ui);
            let cmd = r.changed.then(|| {
                let sids = vec![sid.to_string()];
                combine(style_prop_cmds(
                    &self.doc,
                    &sids,
                    spec.prop,
                    &format!("{}px", vb_common::units::fmt_num(v)),
                ))
            });
            self.num_commit(r, cmd.flatten());
        } else {
            ui.label(caption(
                ui,
                &vb_session::i18n::t_args(
                    "ui-app-panels-charpara-char-022",
                    &[(
                        "label",
                        vb_session::i18n::FluentValue::from((label).to_string()),
                    )],
                ),
            ));
        }
    }
}
