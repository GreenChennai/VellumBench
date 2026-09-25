//! 外观面板(`⇧F6`,05-1)与描边面板(`^F10`,05-3)的渲染层。
//!
//! 只做「投影 → 构建命令 → [`VellumApp::exec`]/[`num_commit`]」;
//! 模型/编解码/构建器在 [`super`](super)(门禁测试直接打那一层);
//! 描边面板的渲染在兄弟模块 [`super::stroke`](stroke)。

use vb_doc::model::NodeKind;
use vb_ui::components::{caption, icon_button, ColorField, NumField, NumFieldResponse};
use vb_ui::icons;

use super::stroke::load_stroke;
use super::{
    add_effect_cmd, add_fill_cmd, add_stroke_cmd, decode_model, duplicate_item_cmd, move_item_cmd,
    remove_item_cmd, set_effect_cmd, set_fill_body_cmd, set_item_blend_cmd, set_stroke_spec_cmd,
    toggle_item_cmd, update_stroke_width, AppearanceItem, AppearanceModel, AppearanceResult,
    AppearanceTarget, Effect, FillBody, BLEND_MODES, UNSUPPORTED,
};
use crate::app::VellumApp;

/// 外观投影(每帧从文档回读,不在面板私存)。
pub(super) struct AppearanceProj {
    pub(super) sid: String,
    pub(super) kind: NodeKind,
    pub(super) model: AppearanceModel,
}

/// 新效果默认参数(文档内容色,非 UI 皮肤)。
fn default_drop_shadow() -> Effect {
    Effect::DropShadow {
        x: 4.0,
        y: 4.0,
        blur: 8.0,
        spread: 0.0,
        color: "#00000066".into(), // vb-token-ok: 投影默认色(文档内容,非 UI 皮肤)
    }
}

fn default_inner_shadow() -> Effect {
    Effect::InnerShadow {
        x: 0.0,
        y: 2.0,
        blur: 6.0,
        spread: 0.0,
        color: "#00000066".into(), // vb-token-ok: 内阴影默认色(文档内容,非 UI 皮肤)
    }
}

fn default_glow() -> Effect {
    Effect::OuterGlow {
        blur: 12.0,
        color: "#2e86ff80".into(), // vb-token-ok: 发光默认色(文档内容,非 UI 皮肤)
    }
}

impl VellumApp {
    pub(super) fn appearance_projection(&self) -> Option<AppearanceProj> {
        let sid = self.selection.last()?.clone();
        let nid = self.doc.find_by_sid(&sid)?;
        let n = self.doc.nodes.get(nid)?;
        let attrs: Vec<(String, String)> = n
            .attrs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Some(AppearanceProj {
            kind: n.kind.clone(),
            model: decode_model(&n.kind, &n.style, &attrs),
            sid,
        })
    }

    /// 构建器统一执行:成功入 undo;失败 toast(05-6 绝不沉默)。
    /// 离散操作纪律:**禁用合并**(添加/删除/移动/禁用各成一条 undo);
    /// 参数编辑走 [`Self::appearance_num_apply`] 的会话合并。
    pub(super) fn exec_appearance(&mut self, r: AppearanceResult) {
        match r {
            Ok(Some(cmd)) => {
                self.undo.merging_enabled = false;
                self.exec(cmd);
                self.undo.merging_enabled = true;
            }
            Ok(None) => {}
            Err(msg) => self.toast_warn(msg),
        }
    }

    /// NumField 会话 + 构建器结果统一收口(成功入 undo 会话;失败 toast)。
    pub(super) fn appearance_num_apply(&mut self, r: NumFieldResponse, res: AppearanceResult) {
        let cmd = match &res {
            Ok(Some(c)) => Some(c.clone()),
            _ => None,
        };
        self.num_commit(r, cmd);
        if let Err(msg) = res {
            self.toast_warn(msg);
        }
    }

    pub(crate) fn appearance_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.appearance_projection() else {
            ui.label(caption(
                ui,
                "未选中对象 —— 选中后在此管理填充/描边/效果条目。",
            ));
            return;
        };
        let t = super::target_of(&p.kind);
        if t == AppearanceTarget::Frozen {
            ui.label(caption(
                ui,
                "冻结块:内部不可编辑(原样保留的 HTML 片段);可移动/缩放/删除。",
            ));
            return;
        }
        let target_name = match t {
            AppearanceTarget::Box => "盒对象",
            AppearanceTarget::Text => "文字",
            AppearanceTarget::Vector => "矢量路径",
            AppearanceTarget::Frozen => "冻结块",
        };
        ui.label(caption(
            ui,
            &format!("{target_name} · 条目顺序 = CSS 叠加顺序(首条最上)"),
        ));
        ui.separator();

        // ── 条目列表(眼睛 / 摘要 / 混合 / 上移 / 下移 / 复制 / 删除) ──
        let count = p.model.items.len();
        for i in 0..count {
            let item = p.model.items[i].clone();
            let enabled = item.enabled();
            let eye = if enabled {
                icons::Name::Visible
            } else {
                icons::Name::Hidden
            };
            ui.horizontal(|ui| {
                if icon_button(ui, eye, if enabled { "临时禁用" } else { "启用" }).clicked() {
                    let sid = p.sid.clone();
                    self.exec_appearance(toggle_item_cmd(&self.doc, &sid, i, !enabled));
                }
                if ui
                    .selectable_label(self.appearance_sel == Some(i), item.summary())
                    .clicked()
                {
                    self.appearance_sel = Some(i);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, icons::Name::Delete, "删除条目").clicked() {
                        let sid = p.sid.clone();
                        self.exec_appearance(remove_item_cmd(&self.doc, &sid, i));
                    }
                    if icon_button(ui, icons::Name::Copy, "复制条目").clicked() {
                        let sid = p.sid.clone();
                        self.exec_appearance(duplicate_item_cmd(&self.doc, &sid, i));
                    }
                    if icon_button(ui, icons::Name::MoveDown, "下移(CSS 中更靠底)").clicked()
                    {
                        let sid = p.sid.clone();
                        self.exec_appearance(move_item_cmd(&self.doc, &sid, i, 1));
                    }
                    if icon_button(ui, icons::Name::MoveUp, "上移(CSS 中更靠顶)").clicked() {
                        let sid = p.sid.clone();
                        self.exec_appearance(move_item_cmd(&self.doc, &sid, i, -1));
                    }
                    self.blend_combo(ui, &p, i, &item);
                });
            });
        }
        if count == 0 {
            ui.label(caption(ui, "暂无条目 —— 从下方添加填充/描边/效果。"));
        }

        // ── 添加按钮(05-1-2) ──
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("+ 填充").clicked() {
                let sid = p.sid.clone();
                self.exec_appearance(add_fill_cmd(
                    &self.doc,
                    &sid,
                    FillBody::Solid {
                        // vb-token-ok: 新条目默认填充(文档内容,非 UI 皮肤)
                        value: "#d4d4d4".into(),
                    },
                ));
                self.appearance_sel = Some(0);
            }
            if ui.button("+ 描边").clicked() {
                let sid = p.sid.clone();
                self.exec_appearance(add_stroke_cmd(&self.doc, &sid));
                self.appearance_sel = Some(0);
            }
            self.effect_add_menu(ui, &p);
        });

        // ── 选中条目的参数编辑 ──
        if let Some(i) = self.appearance_sel {
            if i < count {
                ui.separator();
                self.appearance_item_editor(ui, &p, i);
            } else {
                self.appearance_sel = None;
            }
        }

        // ── 不支持的能力(弱化可点,点击给提示;05-6-2) ──
        ui.separator();
        ui.label(caption(ui, "不支持的能力(点击查看说明):"));
        ui.horizontal_wrapped(|ui| {
            for f in UNSUPPORTED {
                if ui
                    .button(egui::RichText::new(f.label).weak())
                    .on_hover_text(f.message)
                    .clicked()
                {
                    self.toast_warn(f.message);
                }
            }
        });
    }

    /// 条目混合模式下拉:填充落 `background-blend-mode` 逐层(无损);
    /// 描边/效果条目无 CSS 落点 → 选择仍记入模型,◇ 标注「未落盘」。
    fn blend_combo(
        &mut self,
        ui: &mut egui::Ui,
        p: &AppearanceProj,
        i: usize,
        item: &AppearanceItem,
    ) {
        let cur = item.blend().unwrap_or("normal").to_string();
        let frozen_blend = !matches!(item, AppearanceItem::Fill(_));
        let mut sel = cur.clone();
        egui::ComboBox::from_id_salt(("vb_blend", i))
            .width(76.0)
            .selected_text(if frozen_blend {
                format!("{cur} ◇")
            } else {
                cur.clone()
            })
            .show_ui(ui, |ui| {
                for b in BLEND_MODES {
                    ui.selectable_value(&mut sel, b.to_string(), b);
                }
            });
        if sel != cur {
            let sid = p.sid.clone();
            let blend = (sel != "normal").then(|| sel.clone());
            self.exec_appearance(set_item_blend_cmd(&self.doc, &sid, i, blend));
        }
    }

    /// 「+ 效果」菜单:六种映射 + 羽化 + SVG 滤镜冻结说明 + 不支持项弱化可点。
    fn effect_add_menu(&mut self, ui: &mut egui::Ui, p: &AppearanceProj) {
        egui::ComboBox::from_id_salt("vb_fx_add")
            .selected_text("+ 效果")
            .width(84.0)
            .show_ui(ui, |ui| {
                let mut pick: Option<Effect> = None;
                if ui.selectable_label(false, "投影").clicked() {
                    pick = Some(default_drop_shadow());
                }
                if ui.selectable_label(false, "内阴影").clicked() {
                    pick = Some(default_inner_shadow());
                }
                if ui.selectable_label(false, "外发光").clicked() {
                    pick = Some(default_glow());
                }
                if ui.selectable_label(false, "内发光").clicked() {
                    pick = Some(Effect::InnerGlow {
                        blur: 12.0,
                        color: "#2e86ff80".into(), // vb-token-ok: 发光默认色(文档内容,非 UI 皮肤)
                    });
                }
                if ui.selectable_label(false, "高斯模糊").clicked() {
                    pick = Some(Effect::GaussianBlur { radius: 4.0 });
                }
                if ui.selectable_label(false, "圆角").clicked() {
                    pick = Some(Effect::RoundCorners { radius: 8.0 });
                }
                if ui.selectable_label(false, "羽化").clicked() {
                    pick = Some(Effect::Feather { radius: 12.0 });
                }
                ui.separator();
                if ui
                    .selectable_label(false, egui::RichText::new("SVG 滤镜(冻结)").weak())
                    .on_hover_text("SVG 滤镜原样保留(冻结);v1 不提供编辑器")
                    .clicked()
                {
                    self.toast_warn("SVG 滤镜效果原样保留(冻结);v1 不提供编辑器");
                }
                for f in UNSUPPORTED {
                    if ui
                        .selectable_label(false, egui::RichText::new(f.label).weak())
                        .on_hover_text(f.message)
                        .clicked()
                    {
                        self.toast_warn(f.message);
                    }
                }
                if let Some(e) = pick {
                    let sid = p.sid.clone();
                    // 记住"上一个效果",供「效果 → 应用上一个效果」复用(阶段 5)
                    self.last_effect = Some(e.clone());
                    self.exec_appearance(add_effect_cmd(&self.doc, &sid, e));
                    self.appearance_sel = Some(0);
                }
            });
    }

    /// 选中条目的参数编辑(填充色 / 描边快调 / 效果参数)。
    fn appearance_item_editor(&mut self, ui: &mut egui::Ui, p: &AppearanceProj, i: usize) {
        let sid = p.sid.clone();
        match p.model.items[i].clone() {
            AppearanceItem::Fill(f) => match f.body {
                FillBody::Solid { value } => {
                    let mut col = vb_common::color::parse_color(&value)
                        .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                        .unwrap_or(egui::Color32::WHITE);
                    let tokens = self.doc.tokens.clone();
                    let r = ColorField::new("颜色", &mut col).doc_tokens(&tokens).ui(ui);
                    if let Some(nm) = r.var_picked {
                        self.exec_appearance(set_fill_body_cmd(
                            &self.doc,
                            &sid,
                            i,
                            FillBody::Solid {
                                value: format!("var(--{nm})"),
                            },
                        ));
                    } else if r.cleared {
                        self.exec_appearance(remove_item_cmd(&self.doc, &sid, i));
                    } else if r.changed {
                        let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
                        let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
                        self.exec_appearance(set_fill_body_cmd(
                            &self.doc,
                            &sid,
                            i,
                            FillBody::Solid { value: hex },
                        ));
                    }
                }
                FillBody::Gradient { value } => {
                    ui.label(caption(ui, &format!("渐变:{value}")));
                    ui.label(caption(ui, "色标编辑 → 渐变面板(Ctrl+F9);此处保真往返。"));
                }
                FillBody::Raw { value } => {
                    ui.label(caption(ui, &format!("原样保真:{value}")));
                }
            },
            AppearanceItem::Stroke(s) => {
                ui.label(caption(ui, "快调颜色/粗细;全字段 → 描边面板(Ctrl+F10)"));
                let mut spec = s.spec;
                let mut col = spec
                    .color
                    .as_deref()
                    .and_then(vb_common::color::parse_color)
                    .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                    .unwrap_or(egui::Color32::BLACK);
                let tokens = self.doc.tokens.clone();
                let r = ColorField::new("颜色", &mut col).doc_tokens(&tokens).ui(ui);
                if let Some(nm) = r.var_picked {
                    spec.color = Some(format!("var(--{nm})"));
                    self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, i, spec));
                } else if r.changed && !r.cleared {
                    let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
                    spec.color = Some(vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex());
                    self.exec_appearance(set_stroke_spec_cmd(&self.doc, &sid, i, spec));
                }
                let mut w = match load_stroke(&self.doc, &sid, i) {
                    Some(s) => s.width,
                    None => return,
                };
                let r = NumField::new("粗细", &mut w)
                    .speed(0.5)
                    .step(1.0)
                    .range(0.0, 200.0)
                    .unit("px")
                    .label_width(44.0)
                    .width(56.0)
                    .ui(ui);
                if r.changed {
                    let res = update_stroke_width(&self.doc, &sid, i, w);
                    self.appearance_num_apply(r, res);
                }
            }
            AppearanceItem::Effect(e) => self.effect_param_editor(ui, &sid, i, &e.effect),
        }
    }

    /// 效果参数编辑(数值框会话合并同 NumField,05-6-3)。
    fn effect_param_editor(&mut self, ui: &mut egui::Ui, sid: &str, i: usize, e: &Effect) {
        let mut eff = e.clone();
        let mut resp = NumFieldResponse::default();
        {
            let num = |ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64| {
                NumField::new(label, v)
                    .speed(speed)
                    .step(1.0)
                    .range(-500.0, 500.0)
                    .unit("px")
                    .label_width(44.0)
                    .width(56.0)
                    .ui(ui)
            };
            match &mut eff {
                Effect::DropShadow { .. } | Effect::InnerShadow { .. } => {
                    let (x, y, blur, spread, color) = match &mut eff {
                        Effect::DropShadow {
                            x,
                            y,
                            blur,
                            spread,
                            color,
                        }
                        | Effect::InnerShadow {
                            x,
                            y,
                            blur,
                            spread,
                            color,
                        } => (x, y, blur, spread, color),
                        _ => unreachable!(),
                    };
                    let r1 = num(ui, "X", x, 1.0);
                    let r2 = num(ui, "Y", y, 1.0);
                    let r3 = num(ui, "模糊", blur, 1.0);
                    let r4 = num(ui, "扩展", spread, 1.0);
                    for r in [r1, r2, r3, r4] {
                        resp.changed |= r.changed;
                        resp.scrub_started |= r.scrub_started;
                        resp.scrub_ended |= r.scrub_ended;
                        resp.focus_lost |= r.focus_lost;
                        if resp.expr_error.is_none() {
                            resp.expr_error = r.expr_error;
                        }
                    }
                    self.effect_color_field(ui, &mut resp, color);
                }
                Effect::OuterGlow { blur, color } | Effect::InnerGlow { blur, color } => {
                    let r1 = num(ui, "模糊", blur, 1.0);
                    resp.changed |= r1.changed;
                    resp.scrub_started |= r1.scrub_started;
                    resp.scrub_ended |= r1.scrub_ended;
                    resp.focus_lost |= r1.focus_lost;
                    if resp.expr_error.is_none() {
                        resp.expr_error = r1.expr_error;
                    }
                    self.effect_color_field(ui, &mut resp, color);
                }
                Effect::GaussianBlur { radius } | Effect::RoundCorners { radius } => {
                    resp = num(ui, "半径", radius, 0.5);
                }
                Effect::Feather { radius } => {
                    resp = num(ui, "羽化", radius, 1.0);
                }
                Effect::Other { prop, value } => {
                    ui.label(caption(ui, &format!("原样保真:{prop}: {value}")));
                }
            }
        }
        if resp.changed {
            let res = set_effect_cmd(&self.doc, sid, i, eff);
            self.appearance_num_apply(resp, res);
        }
    }

    /// 效果颜色字段(变化并入同帧会话)。
    fn effect_color_field(
        &mut self,
        ui: &mut egui::Ui,
        resp: &mut NumFieldResponse,
        color: &mut String,
    ) {
        let mut col = vb_common::color::parse_color(color)
            .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
            .unwrap_or(egui::Color32::BLACK);
        let tokens = self.doc.tokens.clone();
        let cr = ColorField::new("颜色", &mut col).doc_tokens(&tokens).ui(ui);
        if let Some(nm) = cr.var_picked {
            *color = format!("var(--{nm})");
            resp.changed = true;
        } else if cr.changed && !cr.cleared {
            let [a, b, c, d] = col.to_srgba_unmultiplied();
            *color = vb_common::Rgba::new(a, b, c, d).to_shortest_hex();
            resp.changed = true;
        }
    }
}
