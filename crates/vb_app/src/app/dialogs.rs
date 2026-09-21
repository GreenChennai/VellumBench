//! 对话框与浮窗(S1-a 自 app.rs 机械搬移,零行为变化):
//! 关于 / 双击文本编辑 / 导出 / 命令面板窗口,以及导出执行逻辑。

use egui::Key;
use vb_doc::commands::Command;
use vb_doc::model::NodeKind;

use crate::shortcuts;

use super::VellumApp;

impl VellumApp {
    pub(crate) fn show_about_window(&mut self, ui: &mut egui::Ui) {
        if self.show_about {
            let mut open = self.show_about;
            egui::Window::new("关于 Vellum Bench")
                .open(&mut open)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label(format!(
                        "Vellum Bench v{} · 绘台",
                        env!("CARGO_PKG_VERSION")
                    ));
                    ui.separator();
                    ui.label("用 Illustrator 的操作心智,编辑标准 HTML/CSS 文档。");
                    ui.label("HTML 是文档格式,不是编译产物。");
                });
            self.show_about = open;
        }
    }

    pub(crate) fn show_text_edit_window(&mut self, ui: &mut egui::Ui) {
        // 双击文本编辑窗口
        if let Some(sid) = self.editing_text.clone() {
            if let Some(nid) = self.doc.find_by_sid(&sid) {
                let mut text = match self.doc.nodes.get(nid).unwrap().kind {
                    NodeKind::Text { ref text, .. } => text.clone(),
                    _ => String::new(),
                };
                let mut open = true;
                let win_title = self
                    .doc
                    .nodes
                    .get(nid)
                    .map(|n| n.name.clone())
                    .unwrap_or_default();
                let mut commit = false;
                let mut cancel = false;
                let mut esc = false;
                egui::Window::new(format!("编辑文本 — {win_title}"))
                    .open(&mut open)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut text)
                                .desired_width(420.0)
                                .desired_rows(3),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("提交 (Ctrl+Enter)").clicked() {
                                commit = true;
                            }
                            if ui.button("取消 (Esc)").clicked() {
                                cancel = true;
                            }
                        });
                        if ui.ctx().input(|i| {
                            i.key_pressed(Key::Enter) && (i.modifiers.ctrl || i.modifiers.command)
                        }) {
                            commit = true;
                        }
                        // 04-3(2):Esc = 提交;二次 Esc 放弃(canvas.cancel
                        // 在编辑态结束后接到武装标记,作废刚提交的 SetText)
                        if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
                            esc = true;
                        }
                    });
                if commit || esc {
                    self.exec(Command::SetText {
                        sid: sid.clone(),
                        new: text,
                        old: None,
                    });
                    // 仅 Esc 提交才武装「二次 Esc 放弃」(Mod+Enter 提交不武装,
                    // 与 design/06 §3.6 的 Esc-Esc 序列语义一致)
                    if esc {
                        self.text_discard_arm = Some((sid.clone(), std::time::Instant::now()));
                    }
                    self.status = "文本已提交(Ctrl+Enter 提交 · Esc 提交 · 再按 Esc 放弃)".into();
                    self.editing_text = None;
                } else if cancel || !open {
                    self.editing_text = None;
                }
            } else {
                self.editing_text = None;
            }
        }
    }

    pub(crate) fn show_export_window(&mut self, ui: &mut egui::Ui) {
        // 导出对话框(v0.5:双引擎)
        if self.show_export {
            let mut open = self.show_export;
            egui::Window::new("导出")
                .open(&mut open)
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    const FORMATS: [&str; 9] = [
                        "PNG(Kiln)",
                        "JPG(Kiln)",
                        "GIF(Kiln)",
                        "MP4(Kiln)",
                        "SVG(Kiln)",
                        "PDF(Kiln)",
                        "EPS(Kiln)",
                        "AI(Kiln)",
                        "PPTX(Kiln)",
                    ];
                    ui.horizontal(|ui| {
                        ui.label("格式");
                        let mut f = self.export_format;
                        let label = FORMATS[f].to_string();
                        ui.add(egui::Slider::new(&mut f, 0..=8).text(label));
                        self.export_format = f;
                    });
                    if self.export_format == 0 || self.export_format == 1 {
                        ui.horizontal(|ui| {
                            ui.label("倍率");
                            for s in [1u32, 2, 3, 4] {
                                if ui
                                    .selectable_label(self.export_scale == s, format!("@{s}x"))
                                    .clicked()
                                {
                                    self.export_scale = s;
                                }
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label("倍率");
                            for s in [1u32, 2, 4] {
                                if ui
                                    .selectable_label(self.export_scale == s, format!("@{s}x"))
                                    .clicked()
                                {
                                    self.export_scale = s;
                                }
                            }
                            if self.export_scale == 3 {
                                self.export_scale = 2;
                            }
                        });
                    }
                    if self.export_format == 0 {
                        ui.checkbox(&mut self.export_transparent, "透明背景");
                    }
                    ui.separator();
                    ui.label(format!("目标:当前画板({})", self.active_artboard_name()));
                    if ui.button("导出").clicked() {
                        self.run_export_dialog();
                        self.show_export = false;
                    }
                });
            self.show_export = open;
        }
    }

    pub(crate) fn show_command_palette(&mut self, ui: &mut egui::Ui) {
        // 命令面板(P3.2,Ctrl+K)
        if self.palette_open {
            let mut open = self.palette_open;
            let mut close = false;
            egui::Window::new("命令面板")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    let search = ui.add_sized(
                        [360.0, 22.0],
                        egui::TextEdit::singleline(&mut self.palette_query).hint_text("搜索命令…"),
                    );
                    // egui TextEdit 只有点击才聚焦:面板开着就持续请求焦点,
                    // 否则首帧键入落空、工具快捷键穿透(G6)
                    search.request_focus();
                    // Esc 关闭:输入框聚焦时全局 Esc 走 TextEdit 上下文,面板须自行处理
                    if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
                        close = true;
                    }
                    // Ctrl+K 再按一次关闭(面板开启期间输入上下文是
                    // TextEdit,全局 CTX_NO_TEXT 派发不到这里)
                    if ui
                        .ctx()
                        .input(|i| i.key_pressed(Key::K) && i.modifiers.ctrl)
                    {
                        close = true;
                    }
                    let query = self.palette_query.to_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            let mut executed: Option<String> = None;
                            for &(id, label) in shortcuts::CMD_LABELS {
                                if !query.is_empty()
                                    && !label.to_lowercase().contains(&query)
                                    && !id.contains(&query)
                                {
                                    continue;
                                }
                                let key_text = shortcuts::key_text_for(id).unwrap_or_default();
                                let row = ui.add(
                                    egui::Button::new(
                                        egui::RichText::new(format!("{label}    {key_text}"))
                                            .size(12.0),
                                    )
                                    .min_size(egui::vec2(340.0, 20.0)),
                                );
                                if row.clicked() {
                                    executed = Some(id.to_string());
                                }
                            }
                            if let Some(id) = executed {
                                self.run_command(&id, false, false);
                                self.palette_open = false;
                            }
                        });
                });
            if close {
                open = false;
            }
            self.palette_open = open;
        }
    }

    /// 导出对话框执行(v0.5 双引擎)。
    fn run_export_dialog(&mut self) {
        let Some(dir) = self.project_dir.clone() else {
            self.toast_warn("先保存项目(选一个目录)再导出");
            self.save_project();
            return;
        };
        let Some(ab) = self.active_artboard() else {
            return;
        };
        let name = self.doc.nodes.get(ab).unwrap().name.clone();
        let scale = self.export_scale;
        let fmt = self.export_format;
        let kiln_format = match fmt {
            0 => vb_kiln::Format::Png,
            1 => vb_kiln::Format::Jpg,
            2 => vb_kiln::Format::Gif,
            3 => vb_kiln::Format::Mp4,
            4 => vb_kiln::Format::Svg,
            5 => vb_kiln::Format::Pdf,
            6 => vb_kiln::Format::Eps,
            7 => vb_kiln::Format::Ai,
            _ => vb_kiln::Format::Pptx,
        };
        let out_name = vb_export::expand_name_template(
            vb_export::DEFAULT_TEMPLATE,
            &self.doc.meta.title,
            &name,
            scale,
            kiln_format.ext(),
            1,
            0,
            0,
        );
        let out = dir.join(out_name);

        // 引擎选择:Kiln 为默认;VB_EXPORT_ENGINE=wpi 时 PDF/GIF/MP4 回退
        // 旧浏览器路径(回滚开关,文档见 docs/kiln-rollback.md)。
        let engine_wpi = std::env::var("VB_EXPORT_ENGINE")
            .map(|v| v.eq_ignore_ascii_case("wpi"))
            .unwrap_or(false);
        let use_wpi_fallback = engine_wpi
            && matches!(
                kiln_format,
                vb_kiln::Format::Pdf | vb_kiln::Format::Gif | vb_kiln::Format::Mp4
            );

        if use_wpi_fallback {
            let Some(wpi_dir) = vb_export::wpi::resolve_wpi_dir() else {
                self.toast_error("WPI 回退不可用:请设置 VB_WPI_DIR 指向 WPI 仓库");
                return;
            };
            let wpi_fmt = match kiln_format {
                vb_kiln::Format::Pdf => vb_export::wpi::WpiFormat::Pdf,
                vb_kiln::Format::Gif => vb_export::wpi::WpiFormat::Gif,
                _ => vb_export::wpi::WpiFormat::Mp4,
            };
            let req = vb_export::wpi::WpiExportRequest {
                format: wpi_fmt,
                scale: if scale >= 4 {
                    4
                } else if scale >= 2 {
                    2
                } else {
                    1
                },
                width: 1920,
                transparent: self.export_transparent,
                out,
                max_wait: 20.0,
            };
            match vb_export::wpi::export_via_wpi(&self.doc, &dir, &req, &wpi_dir) {
                Ok(res) => {
                    self.status = format!(
                        "WPI 回退导出 {}({} KB)",
                        res.out.display(),
                        std::fs::metadata(&res.out)
                            .map(|m| m.len() / 1024)
                            .unwrap_or(0),
                    );
                }
                Err(e) => self.toast_error(format!("WPI 回退导出失败:{e}")),
            }
            return;
        }

        // Kiln 默认路径(九格式统一)
        let req = vb_kiln::ExportRequest {
            format: kiln_format,
            scale,
            transparent: self.export_transparent,
            ..Default::default()
        };
        match vb_kiln::export_artboard(&self.doc, ab, &req, Some(&dir)) {
            Ok((bytes, report)) => match std::fs::write(&out, &bytes) {
                Ok(()) => {
                    self.status = format!(
                        "Kiln 导出 {} @{}x({})",
                        out.display(),
                        scale,
                        report.summary()
                    );
                }
                Err(e) => self.toast_error(format!("写文件失败:{e}")),
            },
            Err(e) => self.toast_error(format!("Kiln 导出失败:{e}")),
        }
    }

    pub(crate) fn export_current_artboard_png(&mut self) {
        let Some(dir) = self.project_dir.clone() else {
            self.toast_warn("先保存项目(选一个目录)再导出");
            self.save_project();
            return;
        };
        // 当前画板:含选区的画板,否则第一个
        let ab = self
            .selection
            .first()
            .and_then(|sid| self.doc.find_by_sid(sid))
            .and_then(|nid| {
                let mut p = Some(nid);
                loop {
                    match p {
                        Some(id) => {
                            let n = self.doc.nodes.get(id).unwrap();
                            if matches!(n.kind, NodeKind::Artboard) {
                                break Some(id);
                            }
                            p = n.parent;
                        }
                        None => break None,
                    }
                }
            })
            .or(self.doc.artboards.first().copied());
        let Some(ab) = ab else { return };
        let name = self.doc.nodes.get(ab).unwrap().name.clone();
        match vb_export::export_artboard_png(&self.doc, ab, 2.0, false, Some(&dir)) {
            Ok((png, warnings)) => {
                let out = dir.join(vb_export::expand_name_template(
                    vb_export::DEFAULT_TEMPLATE,
                    &self.doc.meta.title,
                    &name,
                    2,
                    "png",
                    1,
                    0,
                    0,
                ));
                match std::fs::write(&out, &png) {
                    Ok(()) => {
                        self.status = format!(
                            "导出 {} @2x({} KB){}",
                            out.display(),
                            png.len() / 1024,
                            if warnings.is_empty() {
                                String::new()
                            } else {
                                format!(";{} 条近似警告", warnings.len())
                            }
                        );
                    }
                    Err(e) => self.toast_error(format!("写文件失败:{e}")),
                }
            }
            Err(e) => self.toast_error(format!("导出失败:{e}")),
        }
    }
}
