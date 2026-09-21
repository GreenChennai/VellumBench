//! 右侧面板 · 令牌 Tab(S1-a 自 app.rs 机械搬移,零行为变化)。

use egui::Color32;
use vb_doc::commands::Command;

use crate::app::VellumApp;

impl VellumApp {
    pub(crate) fn tokens_tab(&mut self, ui: &mut egui::Ui) {
        // --- 设计令牌(v0.7:CSS 变量,改一处全站生效) ---
        ui.heading("设计令牌");
        ui.horizontal(|ui| {
            let mut add: Option<(String, String)> = None;
            if ui.small_button("+ 令牌").clicked() {
                add = Some((
                    format!("brand-{}", self.doc.tokens.len() + 1),
                    "#888888".into(), // vb-token-ok: 新令牌默认值(文档内容,非 UI 皮肤)
                ));
            }
            if let Some((n, v)) = add {
                self.exec(Command::SetToken {
                    name: n,
                    new: v,
                    old: None,
                });
            }
        });
        let tokens = self.doc.tokens.clone();
        for (i, (name, value)) in tokens.iter().enumerate() {
            ui.horizontal(|ui| {
                let mut v = value.clone();
                if vb_common::color::parse_color(value).is_some() {
                    if let Some(c) = vb_common::color::parse_color(value) {
                        let mut col = Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
                        if ui.color_edit_button_srgba(&mut col).changed() {
                            let [r, g, b, a] = col.to_array();
                            self.exec(Command::SetToken {
                                name: name.clone(),
                                new: vb_common::Rgba::new(r, g, b, a).to_shortest_hex(),
                                old: None,
                            });
                        }
                    }
                }
                let resp = ui.add_sized([70.0, 18.0], egui::Label::new(format!("--{name}")));
                let _ = resp;
                if ui
                    .add_sized([110.0, 18.0], egui::TextEdit::singleline(&mut v))
                    .lost_focus()
                    && v != *value
                {
                    self.exec(Command::SetToken {
                        name: name.clone(),
                        new: v,
                        old: None,
                    });
                }
                if ui.small_button("🗑").clicked() {
                    // 删除令牌 = SetToken 到空再移除(v0.1:直接移除,可撤销)
                    self.exec(Command::SetToken {
                        name: name.clone(),
                        new: String::new(),
                        old: None,
                    });
                }
                let _ = i;
            });
        }
    }
}
