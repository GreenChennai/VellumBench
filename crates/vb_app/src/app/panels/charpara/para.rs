//! 段落面板(`⇧^F8`)渲染:对齐九式 / 缩进与间距 px 字段 / 避头尾组合框。
//!
//! 06-1 自 `panels/charpara.rs` 渲染段按面板拆出(纯搬移,零行为变化);
//! 字符面板在 `char`,字段规格在 `fields`。

use vb_css::Decl;

use crate::app::control_panel::{combine, style_prop_cmds, style_prop_remove_cmds};
use vb_doc::model::TextMode;
use vb_ui::components::{caption, NumField};
use vb_ui::theme;

use super::fields::*;
use super::model::*;
use crate::app::VellumApp;

impl VellumApp {
    // ─────────────── 段落面板 ───────────────

    pub(crate) fn para_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.text_projection() else {
            ui.label(caption(
                ui,
                "未选中文本对象 —— 段落属性作用于整个文本对象,请先选中。",
            ));
            return;
        };
        let sid = p.sid.clone();
        let style = p.style.clone();

        // ── 9 式对齐(当前式高亮 = 投影) ──
        ui.label("对齐");
        let cur = Align9::from_style(&style);
        for row in Align9::ALL.chunks(5) {
            ui.horizontal(|ui| {
                for a in row {
                    let active = cur == Some(*a);
                    if ui
                        .selectable_label(active, a.short())
                        .on_hover_text(a.label())
                        .clicked()
                    {
                        let sids = vec![sid.clone()];
                        let cmds = para_align_cmds(&self.doc, &sids, *a);
                        if let Some(cmd) = combine(cmds) {
                            self.exec(cmd);
                            self.say(format!("对齐 → {}", a.label()));
                        }
                    }
                }
            });
        }
        ui.separator();

        // ── 缩进 / 段距(px NumField,undo 会话) ──
        self.para_px_field(ui, &INDENT_L_SPEC, &sid, &style);
        self.para_px_field(ui, &INDENT_R_SPEC, &sid, &style);
        self.para_px_field(ui, &INDENT_FIRST_SPEC, &sid, &style);
        self.para_px_field(ui, &SPACE_BEFORE_SPEC, &sid, &style);
        self.para_px_field(ui, &SPACE_AFTER_SPEC, &sid, &style);
        ui.separator();

        // ── 避头尾(CJK line-break)/ 连字 / 标点挤压(白名单处置:入 L1) ──
        self.para_combo(ui, &KINSOKU_SPEC, &style, &sid);
        self.para_combo(ui, &HYPHENS_SPEC, &style, &sid);
        self.para_combo(ui, &PUNCT_SPEC, &style, &sid);
        ui.separator();

        // ── 区域文本:溢出提示 + 自动扩高(几何命令) ──
        if p.mode == TextMode::Area {
            let nid = self.doc.find_by_sid(&sid);
            let overflow = nid
                .map(|nid| area_overflow_px(&self.doc, nid))
                .unwrap_or(0.0);
            if overflow > 0.5 {
                ui.colored_label(
                    theme::semantic::overflow_dot(self.theme_dark),
                    format!("文本溢出约 {}px", overflow.ceil() as i64),
                );
                if ui.button("自动扩高(补足内容高度)").clicked() {
                    if let Some(cmd) = area_fit_height_cmd(&self.doc, &sid) {
                        self.exec(cmd);
                        self.say("区域文本已自动扩高(SetGeom,可撤销)");
                    }
                }
            } else {
                ui.label(caption(ui, "区域文本:内容未溢出"));
            }
            ui.label(caption(ui, "双击画布区域文本右下角溢出红点也可自动扩高。"));
        } else {
            ui.label(caption(
                ui,
                "区域文本属性仅作用于区域文本(点文本宽度自适应)",
            ));
        }
        ui.label(caption(
            ui,
            "画布文字为近似渲染(ADR-0017);导出为真字形,以浏览器校对为准。",
        ));
    }

    /// 段落 px 数值字段(缩进/段距):声明存在 → NumField;无声明 → caption
    /// (不显示 0 伪装成已设置)。
    fn para_px_field(&mut self, ui: &mut egui::Ui, spec: &PxFieldSpec, sid: &str, style: &[Decl]) {
        match style_font_px(style, spec.prop) {
            Some(v0) => {
                let mut v = v0;
                let r = NumField::new(spec.label, &mut v)
                    .speed(0.5)
                    .step(1.0)
                    .range(spec.lo, spec.hi)
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
            }
            None => {
                ui.label(caption(ui, &format!("{}:未声明(0 / 继承)", spec.label)));
            }
        }
    }

    /// 段落下拉字段(避头尾/连字/标点挤压):default 值 = 移除声明。
    fn para_combo(&mut self, ui: &mut egui::Ui, spec: &ComboSpec, style: &[Decl], sid: &str) {
        let cur = style
            .iter()
            .find(|d| d.prop == spec.prop)
            .map(|d| d.value.clone());
        let shown = cur.clone().unwrap_or_else(|| spec.options[0].0.to_string());
        let shown_label = spec
            .options
            .iter()
            .find(|(v, _)| *v == shown)
            .map(|(_, l)| *l)
            .unwrap_or(shown.as_str());
        let mut sel = shown.clone();
        egui::ComboBox::from_id_salt(spec.salt)
            .selected_text(format!("{} {shown_label}", spec.label))
            .show_ui(ui, |ui| {
                for (v, l) in spec.options {
                    ui.selectable_value(&mut sel, v.to_string(), *l);
                }
            });
        if sel != cur.unwrap_or_else(|| spec.options[0].0.to_string()) {
            let sids = vec![sid.to_string()];
            let cmds = if sel == spec.options[0].0 {
                // 首项 = 默认:移除声明(选项表首项必须语义为「默认/无」)
                style_prop_remove_cmds(&self.doc, &sids, spec.prop)
            } else {
                style_prop_cmds(&self.doc, &sids, spec.prop, &sel)
            };
            if let Some(cmd) = combine(cmds) {
                self.exec(cmd);
            }
        }
    }
}
