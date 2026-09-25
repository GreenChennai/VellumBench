//! 透明度面板(`⇧^F10`,副文档 05-4)。
//!
//! 三块能力:
//! - **不透明度** → `opacity`;
//! - **混合模式**(16 项)→ `mix-blend-mode`;
//! - **挖空组** → `isolation: isolate`(CSS 唯一可无损对应"隔离组"语义的属性;
//!   AI 的挖空还涉及"组内相互挖空",v1 只落隔离层,UI 如实标注为近似);
//! - **蒙版区**:制作 / 释放 / 反转不透明度蒙版 → `mask-image` 渐变。
//!
//! 纪律:所有不支持项**给提示不静默**(`design/06 §七`);`mask-*` 族除
//! `mask-image` 外均不在 `vb_css` 白名单,故只写这一条,其余以 caption 说明。

use vb_common::units::fmt_num;
use vb_doc::commands::Command;
use vb_doc::model::{Document, Node};
use vb_ui::components::{caption, NumField};
use vb_ui::gradient;

use crate::app::appearance::{self, AppearanceItem, AppearanceTarget, BLEND_MODES};
use crate::app::{style_set_or_remove, VellumApp};

/// 默认不透明度蒙版:黑(不透明)→ 透明(黑 alpha 0)。
/// CSS `mask-image` 用 alpha 通道,故"不透明"即 `#000`,透明即 `#0000`。
/// **值写成 canonical 形式**(`0` 不带单位)—— 与 `vb_css::canonical_value`
/// 同口径,保证首帧保存即为不动点(L1)。
const MASK_DEFAULT: &str = "linear-gradient(#000 0, #00000000 100%)";

/// 该对象是否"盒类"(非文字/矢量/冻结)—— 决定 `opacity` 之外的能力可用性。
fn is_box(doc: &Document, sid: &str) -> bool {
    doc.find_by_sid(sid)
        .and_then(|id| doc.nodes.get(id))
        .map(|n| appearance::target_of(&n.kind) == AppearanceTarget::Box)
        .unwrap_or(false)
}

fn prop_of<'a>(n: &'a Node, prop: &str) -> Option<&'a str> {
    n.style
        .iter()
        .find(|d| d.prop == prop)
        .map(|d| d.value.as_str())
}

/// 只有一条声明的 `SetStyle`(其余声明原样保留)。
pub fn set_prop_cmd(doc: &Document, sid: &str, prop: &str, value: Option<&str>) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.nodes.get(nid)?;
    Some(Command::SetStyle {
        sid: sid.to_string(),
        new: style_set_or_remove(n.style.clone(), prop, value),
        old: None,
    })
}

/// 反转蒙版渐变(仅当蒙版是我们写的线性渐变时;否则给提示)。
pub fn invert_mask_cmd(doc: &Document, sid: &str) -> Result<Option<Command>, String> {
    let nid = doc
        .find_by_sid(sid)
        .ok_or_else(|| "对象不存在".to_string())?;
    let n = doc.nodes.get(nid).ok_or_else(|| "对象不存在".to_string())?;
    let raw = prop_of(n, "mask-image").ok_or_else(|| "该对象还没有蒙版".to_string())?;
    let mut g =
        gradient::parse(raw).ok_or_else(|| "该蒙版不是可反转的渐变蒙版(原样保留)".to_string())?;
    g.reverse();
    Ok(Some(Command::SetStyle {
        sid: sid.to_string(),
        new: style_set_or_remove(n.style.clone(), "mask-image", Some(&g.to_css())),
        old: None,
    }))
}

/// 挖空组:`isolation: isolate`(近似;UI 标注)。
pub fn knockout_cmd(doc: &Document, sid: &str, on: bool) -> Option<Command> {
    set_prop_cmd(doc, sid, "isolation", on.then_some("isolate"))
}

impl VellumApp {
    pub(crate) fn opacity_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(sid) = self.selection.last().cloned() else {
            ui.label(caption(
                ui,
                "未选中对象 —— 选中后调整不透明度 / 混合 / 蒙版。",
            ));
            return;
        };
        let Some(nid) = self.doc.find_by_sid(&sid) else {
            ui.label(caption(ui, "对象已不存在。"));
            return;
        };
        let (style, kind) = {
            let n = self.doc.nodes.get(nid).unwrap();
            (n.style.clone(), n.kind.clone())
        };
        let boxy = is_box(&self.doc, &sid);
        let tokens = self.doc.tokens.clone();

        // ── 不透明度 ──
        let mut op = style
            .iter()
            .find(|d| d.prop == "opacity")
            .and_then(|d| d.value.trim().parse::<f64>().ok())
            .unwrap_or(1.0)
            * 100.0;
        let r = NumField::new("不透明度", &mut op)
            .unit("%")
            .speed(1.0)
            .step(1.0)
            .range(0.0, 100.0)
            .width(64.0)
            .ui(ui);
        if r.changed {
            let v = fmt_num(op / 100.0);
            let cmd = if (op - 100.0).abs() < 0.01 {
                set_prop_cmd(&self.doc, &sid, "opacity", None)
            } else {
                set_prop_cmd(&self.doc, &sid, "opacity", Some(&v))
            };
            self.num_commit(r, cmd);
        }

        // ── 混合模式(16 项)──
        let cur = prop_of(self.doc.nodes.get(nid).unwrap(), "mix-blend-mode")
            .unwrap_or("normal")
            .to_string();
        ui.horizontal(|ui| {
            ui.label("混合模式");
            let mut sel = cur.clone();
            egui::ComboBox::from_id_salt("vb-mix-blend")
                .selected_text(sel.clone())
                .width(140.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut sel, "normal".to_string(), "normal");
                    for m in BLEND_MODES {
                        ui.selectable_value(&mut sel, m.to_string(), m);
                    }
                });
            if sel != cur {
                let v = (sel != "normal").then_some(sel.as_str());
                let cmd = set_prop_cmd(&self.doc, &sid, "mix-blend-mode", v);
                self.undo.merging_enabled = false;
                if let Some(c) = cmd {
                    self.exec(c);
                }
                self.undo.merging_enabled = true;
            }
        });
        if !boxy {
            ui.label(caption(
                ui,
                "混合模式对文字/矢量对象落 CSS 无独立语义(随父层生效)。",
            ));
        }

        ui.separator();

        // ── 挖空组 ──
        let mut knock = prop_of(self.doc.nodes.get(nid).unwrap(), "isolation")
            .map(|v| v.trim() == "isolate")
            .unwrap_or(false);
        let before = knock;
        ui.checkbox(&mut knock, "挖空组(隔离组)")
            .on_hover_text("CSS 落点:isolation: isolate(近似;组内相互挖空的渲染级还原计划 v2)");
        if knock != before {
            let cmd = knockout_cmd(&self.doc, &sid, knock);
            self.undo.merging_enabled = false;
            if let Some(c) = cmd {
                self.exec(c);
            }
            self.undo.merging_enabled = true;
        }

        ui.separator();

        // ── 蒙版区 ──
        ui.label("不透明度蒙版");
        let has_mask = prop_of(self.doc.nodes.get(nid).unwrap(), "mask-image").is_some();
        ui.horizontal(|ui| {
            if !has_mask {
                if ui
                    .button("制作蒙版")
                    .on_hover_text("写入 mask-image 渐变(黑→透明);顶部对象作蒙版的黑白稿未建模")
                    .clicked()
                {
                    let cmd = set_prop_cmd(&self.doc, &sid, "mask-image", Some(MASK_DEFAULT));
                    self.undo.merging_enabled = false;
                    if let Some(c) = cmd {
                        self.exec(c);
                    }
                    self.undo.merging_enabled = true;
                }
            } else {
                if ui.button("反转蒙版").clicked() {
                    match invert_mask_cmd(&self.doc, &sid) {
                        Ok(Some(c)) => {
                            self.undo.merging_enabled = false;
                            self.exec(c);
                            self.undo.merging_enabled = true;
                        }
                        Ok(None) => {}
                        Err(msg) => self.toast_warn(msg),
                    }
                }
                if ui.button("释放蒙版").clicked() {
                    let cmd = set_prop_cmd(&self.doc, &sid, "mask-image", None);
                    self.undo.merging_enabled = false;
                    if let Some(c) = cmd {
                        self.exec(c);
                    }
                    self.undo.merging_enabled = true;
                }
            }
        });
        ui.label(caption(
            ui,
            "蒙版以 mask-image 渐变表示;mask-repeat/position 等不在白名单(计划 v2)。",
        ));

        // ── 蒙版色标(与渐变面板同源的结构化编辑)──
        if let Some(raw) = prop_of(self.doc.nodes.get(nid).unwrap(), "mask-image") {
            if let Some(mut g) = gradient::parse(raw) {
                let mut sel = None;
                let bar = vb_ui::gradient::gradient_bar(ui, &mut g, &mut sel, &tokens);
                if bar.changed {
                    let cmd = set_prop_cmd(&self.doc, &sid, "mask-image", Some(&g.to_css()));
                    self.num_commit(
                        vb_ui::components::NumFieldResponse {
                            changed: true,
                            scrub_started: bar.drag_started,
                            scrub_ended: bar.drag_ended,
                            ..Default::default()
                        },
                        cmd,
                    );
                }
            }
        }
        let _ = kind;

        // ── 着色层不透明度(填充/描边条目,外观模型接管时)──
        let attrs: Vec<(String, String)> = self
            .doc
            .nodes
            .get(nid)
            .map(|n| {
                n.attrs
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let model = {
            let n = self.doc.nodes.get(nid).unwrap();
            appearance::decode_model(&n.kind, &n.style, &attrs)
        };
        let fill_blend = model.items.iter().find_map(|it| match it {
            AppearanceItem::Fill(f) => f.blend.clone(),
            _ => None,
        });
        if let Some(b) = fill_blend {
            ui.separator();
            ui.label(caption(
                ui,
                &format!("填充条目级混合:{b}(逐层 background-blend-mode,外观面板可改)。"),
            ));
        }
    }
}
