//! 对话框与浮窗(S1-a 自 app.rs 机械搬移,零行为变化):
//! 关于 / 双击文本编辑 / 导出 / 命令面板窗口,以及导出执行逻辑。

use egui::Key;
use vb_doc::commands::Command;
use vb_doc::model::NodeKind;

use crate::shortcuts;

use super::VellumApp;

impl VellumApp {
    // ───────────────── H-6:Esc 回退链的第 1 层 —— 对话框 ─────────────────
    //
    // 统一链:对话框 > 浮层(命令面板/同族弹层) > 面板聚焦(文本编辑)
    // > 工具回选择。「逐层消费不穿透」的实现口径:
    // - 命令面板与文本编辑开启时输入上下文已是 TextEdit
    //   (`shortcuts::top_input_context`),canvas.cancel 根本不派发 ——
    //   它们各自在窗口内处理 Esc,天然不穿透;
    // - 其余对话框(工具/信息/确认)不改变输入上下文,此前按 Esc 会
    //   直接落到「工具回选择」,这是本次修的缺陷。现在 canvas.cancel
    //   先查 [`Self::esc_dialog_top`]:有开着的对话框 → 关掉最顶一层并
    //   消费本次 Esc;全关才落到画布语义。
    //
    // 优先级从上到下 = 从最模态到最不模态(确认 > 编辑 > 工具 > 信息)。
    pub(crate) fn esc_dialog_top(&self) -> Option<&'static str> {
        if self.jump_confirm.is_some() {
            return Some("跳转确认");
        }
        if self.font_dialog.is_some() {
            return Some("字体替换");
        }
        if self.conflict_open {
            return Some("冲突对比");
        }
        if self.recover.is_some() {
            return Some("崩溃恢复");
        }
        if self.plugin_auth.is_some() {
            return Some("插件授权");
        }
        if self.new_dialog.is_some() {
            return Some("新建项目");
        }
        if self.prefs_open {
            return Some("首选项");
        }
        if self.keymap_open {
            return Some("键位方案");
        }
        if self.doc_settings_open {
            return Some("文档设置");
        }
        if self.workspace_dialog_open {
            return Some("工作区");
        }
        if self.health_open {
            return Some("项目体检");
        }
        if self.show_export {
            return Some("导出");
        }
        if self.show_about {
            return Some("关于");
        }
        if self.proofread_open {
            return Some("浏览器校对");
        }
        if self.external_info_open {
            return Some("外部改动信息");
        }
        if self.family_popup.is_some() {
            return Some("同族工具弹层");
        }
        None
    }

    /// 关掉 [`Self::esc_dialog_top`] 指向的那一层(只关一个;其余保留)。
    /// 各对话框自己的窗口渲染层会在同帧读这些开关并收尾,无需额外清理。
    pub(crate) fn close_esc_dialog_top(&mut self) {
        let Some(which) = self.esc_dialog_top() else {
            return;
        };
        match which {
            "跳转确认" => self.jump_confirm = None,
            "字体替换" => self.font_dialog = None,
            "冲突对比" => self.conflict_open = false,
            "崩溃恢复" => self.recover = None,
            "插件授权" => self.plugin_auth = None,
            "新建项目" => self.new_dialog = None,
            "首选项" => self.prefs_open = false,
            "键位方案" => self.keymap_open = false,
            "文档设置" => self.doc_settings_open = false,
            "工作区" => self.workspace_dialog_open = false,
            "项目体检" => self.health_open = false,
            "导出" => self.show_export = false,
            "关于" => self.show_about = false,
            "浏览器校对" => self.proofread_open = false,
            "外部改动信息" => self.external_info_open = false,
            "同族工具弹层" => self.family_popup = None,
            _ => {}
        }
        self.say(format!("已关闭{which}(Esc)"));
    }

    pub(crate) fn show_about_window(&mut self, ui: &mut egui::Ui) {
        if self.show_about {
            let mut open = self.show_about;
            let mut close_clicked = false;
            egui::Window::new("关于 Vellum Bench")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    // U-7/H-1:统一入场淡入(120ms + 4px);Esc = 关闭
                    vb_ui::motion::fade_slide(
                        ui,
                        egui::Id::new("vb-dlg-about"),
                        vb_ui::theme::motion::STATE,
                        4.0,
                        |ui| {
                            ui.label(format!(
                                "Vellum Bench v{} · 绘台",
                                env!("CARGO_PKG_VERSION")
                            ));
                            ui.separator();
                            ui.label("用 Illustrator 的操作心智,编辑标准 HTML/CSS 文档。");
                            ui.label("HTML 是文档格式,不是编译产物。");
                            let (close, _) = vb_ui::components::dialog_footer(ui, "关闭", None);
                            close_clicked = close;
                        },
                    );
                    if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
                        close_clicked = true;
                    }
                });
            self.show_about = open && !close_clicked;
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
                    .resizable(false)
                    .show(ui.ctx(), |ui| {
                        // H-1:入场淡入(120ms + 4px)
                        vb_ui::motion::fade_slide(
                            ui,
                            egui::Id::new("vb-dlg-textedit"),
                            vb_ui::theme::motion::STATE,
                            4.0,
                            |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut text)
                                        .desired_width(420.0)
                                        .desired_rows(3),
                                );
                                // U-7:主按钮右下(Ctrl+Enter 同效;本窗的
                                // Esc=提交、二次 Esc 放弃是 design/06 §3.6
                                // 钉死的序列语义,不随 U-7 改为取消)
                                let (commit_btn, cancel_btn) = vb_ui::components::dialog_footer(
                                    ui,
                                    "提交 (Ctrl+Enter)",
                                    Some("取消 (Esc)"),
                                );
                                if commit_btn {
                                    commit = true;
                                }
                                if cancel_btn {
                                    cancel = true;
                                }
                            },
                        );
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

    /// 「新建项目 / 从模板新建」对话框(阶段 2 / 02-4-1)。
    /// 确认后经外壳 `CreateProject / CreateFromTemplate` 生成最小合法项目并**开新窗口**。
    pub(crate) fn show_new_project_window(&mut self, ui: &mut egui::Ui) {
        if self.new_dialog.is_none() {
            return;
        }
        let template_mode = self.new_dialog.as_ref().is_some_and(|d| d.template_mode);
        let mut open = true;
        let mut done = false;
        egui::Window::new(if template_mode {
            "从模板新建"
        } else {
            "新建项目"
        })
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            // H-1:入场淡入(120ms + 4px)
            vb_ui::motion::fade_slide(
                ui,
                egui::Id::new("vb-dlg-newproj"),
                vb_ui::theme::motion::STATE,
                4.0,
                |ui| {
                    let Some(dlg) = self.new_dialog.as_mut() else {
                        return;
                    };
                    if let Some(action) = crate::new_project::dialog_ui(ui, dlg) {
                        match action {
                            crate::new_project::DialogAction::Create(spec) => {
                                if let Some(tx) = &self.shell_tx {
                                    let _ =
                                        tx.send(crate::shell::ShellRequest::CreateProject(spec));
                                }
                            }
                            crate::new_project::DialogAction::CreateTemplate {
                                template,
                                location,
                                name,
                            } => {
                                if let Some(tx) = &self.shell_tx {
                                    let _ =
                                        tx.send(crate::shell::ShellRequest::CreateFromTemplate {
                                            template,
                                            location,
                                            name,
                                        });
                                }
                            }
                            crate::new_project::DialogAction::Cancel => {}
                        }
                        done = true;
                    }
                },
            );
        });
        if done || !open {
            self.new_dialog = None;
        }
    }

    pub(crate) fn show_export_window(&mut self, ui: &mut egui::Ui) {
        // 导出对话框(v0.5:双引擎;U-7:主按钮右下 + Enter 主操作 + Esc 取消)
        if self.show_export {
            let mut open = self.show_export;
            let mut export_clicked = false;
            let mut cancel_clicked = false;
            egui::Window::new("导出")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    // H-1:入场淡入(120ms + 4px)
                    vb_ui::motion::fade_slide(
                        ui,
                        egui::Id::new("vb-dlg-export"),
                        vb_ui::theme::motion::STATE,
                        4.0,
                        |ui| {
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
                                            .selectable_label(
                                                self.export_scale == s,
                                                format!("@{s}x"),
                                            )
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
                                            .selectable_label(
                                                self.export_scale == s,
                                                format!("@{s}x"),
                                            )
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
                            ui.label(format!("目标:当前画板({})", self.active_artboard_name()));
                            // U-7:主按钮右下,取消在其左
                            let (primary, cancel) =
                                vb_ui::components::dialog_footer(ui, "导出", Some("取消"));
                            if primary {
                                export_clicked = true;
                            }
                            if cancel {
                                cancel_clicked = true;
                            }
                            // Enter = 主操作(本对话框无文本框,不会与输入冲突);
                            // Esc = 取消(canvas.cancel 被 Esc 回退链优先消费,
                            // 见 dispatch_canvas 的 esc_dialog_top)
                            if ui.ctx().input(|i| i.key_pressed(Key::Enter)) {
                                export_clicked = true;
                            }
                            if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
                                cancel_clicked = true;
                            }
                        },
                    );
                });
            if export_clicked {
                self.run_export_dialog();
                self.show_export = false;
            } else if cancel_clicked || !open {
                self.show_export = false;
            } else {
                self.show_export = open;
            }
        }
    }

    pub(crate) fn show_command_palette(&mut self, ui: &mut egui::Ui) {
        // 命令面板(P3.2,Ctrl+K;04-1 重制:开→聚焦→过滤→↑↓→Enter→Esc)
        if !self.palette_open {
            return;
        }
        let mut open = true;
        let mut close = false;
        let mut executed: Option<String> = None;
        let vp_center = ui.ctx().viewport_rect().center();
        egui::Window::new("命令面板")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(vp_center - egui::vec2(0.0, 60.0))
            .show(ui.ctx(), |ui| {
                // H-1:入场淡入(80ms —— 面板是"高频轻弹"的浮层,比对话框更快)
                vb_ui::motion::fade_slide(
                    ui,
                    egui::Id::new("vb-dlg-palette"),
                    vb_ui::theme::motion::HOVER,
                    2.0,
                    |ui| {
                        let search = ui.add_sized(
                            [360.0, vb_ui::theme::row_height(ui.ctx())],
                            egui::TextEdit::singleline(&mut self.palette_query)
                                .hint_text("搜索命令…"),
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
                        // 过滤后的候选(标签/ID 的关键字子串;04-1-3 输入即过滤)
                        let hits = palette_filter(&self.palette_query);
                        if hits.is_empty() {
                            ui.label(
                                egui::RichText::new("没有匹配的命令")
                                    .size(12.0)
                                    .color(egui::Color32::GRAY),
                            );
                            return;
                        }
                        // ↑↓ 移动选择游标(对过滤后列表取模;输入变化时游标已归零)
                        self.palette_sel = palette_move_sel(self.palette_sel, hits.len(), ui.ctx());
                        // Enter 执行选中项
                        if ui.ctx().input(|i| i.key_pressed(Key::Enter)) {
                            executed =
                                Some(hits[self.palette_sel.min(hits.len() - 1)].0.to_string());
                        }
                        let sel = self.palette_sel.min(hits.len() - 1);
                        let query = self.palette_query.trim().to_lowercase();
                        let accent = vb_ui::theme::tokens(ui.ctx()).accent;
                        egui::ScrollArea::vertical()
                            .max_height(320.0)
                            .show(ui, |ui| {
                                for (i, &(id, label)) in hits.iter().enumerate() {
                                    // 键位文本查有效键位集(用户方案覆盖优先,05-4-A2)
                                    let key_text = self.menu_key_text(id).unwrap_or_default();
                                    // H-5:行 = ▶ 标记 + 标签(匹配段 accent+粗体高亮)
                                    // + 右对齐等宽键位徽章;整行可点,选中行高亮底色。
                                    let selected_row = i == sel;
                                    let row = palette_row_ui(
                                        ui,
                                        mark_prefix(selected_row),
                                        label,
                                        &query,
                                        &key_text,
                                        accent,
                                        selected_row,
                                    );
                                    if row.clicked() {
                                        executed = Some(id.to_string());
                                    }
                                }
                            });
                    },
                );
            });
        // 执行 / 关闭统一在末尾收口(注意:执行后也要经 `open` 落回
        // palette_open,不能直写 —— 否则被下面的 `= open` 覆盖回真,
        // 面板就永远关不掉;这正是 04-1 实测「Enter 后面板不消失」的根因)
        if let Some(id) = executed {
            self.run_command(&id, false, false);
            open = false;
        }
        if close {
            open = false;
        }
        self.palette_open = open;
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
}

// ─────────────────────── 命令面板纯函数(04-1 状态机;门禁测试打这里) ───────────────────────

/// 关键字过滤:标签 / 命令 id 的大小写不敏感子串匹配,保持注册表顺序。
/// `pub`:04-1 门禁测试(过滤语义)直接调用。
pub fn palette_filter(query: &str) -> Vec<(&'static str, &'static str)> {
    let q = query.trim().to_lowercase();
    shortcuts::CMD_LABELS
        .iter()
        .copied()
        .filter(|&(id, label)| q.is_empty() || label.to_lowercase().contains(&q) || id.contains(&q))
        .collect()
}

/// 键盘选择游标的前缀标记(04-1-3:选中行 ▶)。
fn mark_prefix(selected: bool) -> &'static str {
    if selected {
        "▶ "
    } else {
        "   "
    }
}

/// 命令面板行的**分段**模型(H-5;纯函数,单测直打):把标签按查询词
/// 切成 `(文本, 是否命中)` 序列,渲染层对命中段用 accent+粗体。
/// 大小写不敏感定位;切片不在字符边界(Unicode 特例)时整体不高亮。
fn palette_row_segments(label: &str, query: &str) -> Vec<(String, bool)> {
    if query.is_empty() {
        return vec![(label.to_string(), false)];
    }
    let Some(at) = label.to_lowercase().find(query) else {
        return vec![(label.to_string(), false)];
    };
    let end = at + query.len();
    match label.get(at..end) {
        Some(hit) => {
            let mut out = Vec::with_capacity(3);
            if at > 0 {
                out.push((label[..at].to_string(), false));
            }
            out.push((hit.to_string(), true));
            if end < label.len() {
                out.push((label[end..].to_string(), false));
            }
            out
        }
        None => vec![(label.to_string(), false)],
    }
}

/// 命令面板一行(H-5):底色 + 分段标签(匹配段 accent+粗体)+
/// 右对齐等宽键位徽章。整行 `Sense::click()`,返回其 Response。
fn palette_row_ui(
    ui: &mut egui::Ui,
    mark: &str,
    label: &str,
    query: &str,
    key_text: &str,
    accent: egui::Color32,
    selected: bool,
) -> egui::Response {
    let t = vb_ui::theme::tokens(ui.ctx());
    let h = 20.0;
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    let fill = if selected {
        ui.style().visuals.selection.bg_fill
    } else {
        ui.style().visuals.widgets.inactive.bg_fill
    };
    ui.painter()
        .rect_filled(rect, vb_ui::theme::radius::sm(), fill);

    let base_font = egui::FontId::proportional(12.0);
    let bold_font = egui::FontId::new(12.0, vb_ui::fonts::family_semibold());
    let cy = rect.center().y;
    let mut x = rect.left() + vb_ui::theme::space::S4;
    let mut first = true;
    for (seg_text, is_hit) in palette_row_segments(label, query) {
        let text = if first {
            first = false;
            format!("{mark}{seg_text}")
        } else {
            seg_text
        };
        let (font, color) = if is_hit {
            (bold_font.clone(), accent)
        } else {
            (base_font.clone(), t.text)
        };
        let galley = ui.painter().layout(text, font, color, 600.0);
        let w = galley.size().x;
        ui.painter()
            .galley(egui::pos2(x, cy - galley.size().y * 0.5), galley, color);
        x += w;
    }
    // 右对齐等宽键位徽章(空 = 不画)
    if !key_text.is_empty() {
        let mono = egui::FontId::new(12.0, vb_ui::fonts::family_mono());
        let g = ui
            .painter()
            .layout(key_text.to_owned(), mono, t.text_3, 200.0);
        ui.painter().galley(
            egui::pos2(
                rect.right() - g.size().x - vb_ui::theme::space::S4,
                cy - g.size().y * 0.5,
            ),
            g,
            t.text_3,
        );
    }
    resp.on_hover_text("执行此命令")
}

/// ↑↓ 移动选择游标(对候选数取模;左右键与滚轮不动游标)。
/// `pub`:04-1 门禁测试(状态机)直接调用。
pub fn palette_move_sel(cur: usize, len: usize, ctx: &egui::Context) -> usize {
    if len == 0 {
        return 0;
    }
    let (up, down) = ctx.input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
    match (up, down) {
        (true, false) => (cur + len - 1) % len,
        (false, true) => (cur + 1) % len,
        _ => cur.min(len - 1),
    }
}

impl VellumApp {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 04-1 门禁:过滤语义(空 = 全量;标签 / id 子串;大小写不敏感)。
    #[test]
    fn palette_filter_matches_label_or_id() {
        assert_eq!(
            palette_filter("").len(),
            shortcuts::CMD_LABELS.len(),
            "空查询必须列出全部命令"
        );
        let hits = palette_filter("命令面板");
        assert!(
            hits.iter().any(|&(id, _)| id == "app.command_palette"),
            "中文标签必须可搜:{hits:?}"
        );
        let by_id = palette_filter("COMMAND_PALETTE");
        assert!(
            by_id.iter().any(|&(id, _)| id == "app.command_palette"),
            "id 大小写不敏感:{by_id:?}"
        );
        assert!(palette_filter("不存在的命令xyzzy").is_empty(), "无命中为空");
    }

    /// 04-1 门禁:↑↓ 状态机(循环移动 + 空列表安全 + 越界夹回)。
    #[test]
    fn palette_selection_wraps_and_clamps() {
        let ctx = egui::Context::default();
        assert_eq!(palette_move_sel(0, 0, &ctx), 0, "空列表不动游标");
        assert_eq!(palette_move_sel(7, 3, &ctx), 2, "越界游标夹回末项");
        // 无按键 = 保持(且夹回合法域)
        assert_eq!(palette_move_sel(1, 5, &ctx), 1);
    }

    /// H-5 门禁:命令面板高亮分段 —— 命中段被单独标出(渲染层据此
    /// 用 accent+粗体);空查询与按 id 命中的行整体不高亮;文本内容
    /// 拼回后与原标签一致(只分段,不改字)。
    #[test]
    fn palette_row_highlights_matched_substring() {
        // 命中:标签里的查询词(大小写不敏感定位,保留原行大小写)
        let segs = palette_row_segments("命令面板", "面板");
        assert_eq!(
            segs,
            vec![("命令".to_string(), false), ("面板".to_string(), true)]
        );
        // 空查询:单段不高亮
        assert_eq!(
            palette_row_segments("命令面板", ""),
            vec![("命令面板".to_string(), false)]
        );
        // 按 id 命中但标签不含查询词:整体不高亮
        assert_eq!(
            palette_row_segments("命令面板", "palette"),
            vec![("命令面板".to_string(), false)]
        );
        // 分段拼回 = 原标签(不改字,只标样式)
        let text: String = palette_row_segments("界面缩放", "缩放")
            .into_iter()
            .map(|(s, _)| s)
            .collect();
        assert_eq!(text, "界面缩放");
    }

    /// 04-1 根因门禁:`Ctrl+K` / `Ctrl+0` 的派发链 —— 绑定存在、
    /// 画布态触发、文本编辑态不触发(工具键被吞是正确的)。
    #[test]
    fn ctrl_k_and_ctrl_0_dispatch_chain() {
        use crate::shortcuts::{fires_in, lookup, top_input_context, InputContext};
        let k = lookup(egui::Key::K, true, false, false).expect("Ctrl+K 必须有绑定");
        assert_eq!(k.id, "app.command_palette");
        let zero = lookup(egui::Key::Num0, true, false, false).expect("Ctrl+0 必须有绑定");
        assert_eq!(zero.id, "view.fit");
        // 画布态:两条都触发
        let canvas = top_input_context(false, false, false, false);
        assert!(fires_in(k, canvas) && fires_in(zero, canvas));
        // 文本编辑态(真文本框聚焦):不触发(交还给输入框)
        let text = top_input_context(false, false, true, false);
        assert!(!fires_in(k, text) && !fires_in(zero, text));
        // 面板开着:进 TextEdit 态,全局派发不重复触发(面板自行处理 Esc/Ctrl+K)
        let palette = top_input_context(false, true, false, false);
        assert!(!fires_in(k, palette));
        assert_eq!(InputContext::TextEdit.name(), "text.edit");
    }

    /// 04-1 根因说明:egui 0.35 的 `egui_wants_keyboard_input` =
    /// 「任何控件聚焦」,数值框回车提交后会保留焦点(`vb_ui::components`
    /// 02-6-2)—— 若按旧实现喂它,快捷键层会被顶进 TextEdit 态,
    /// Ctrl+K / Ctrl+0 全部吞键。此处锁定新语义:`top_input_context`
    /// 的 `wants_keyboard` 入参只由 `text_edit_focused()` 提供
    /// (见 `VellumApp::input_context` 注释)。
    #[test]
    fn wants_keyboard_flag_means_text_edit_only() {
        use crate::shortcuts::top_input_context;
        // 任意非文本控件聚焦(如 NumField 回车后)不再是 TextEdit 态
        let canvas = top_input_context(false, false, false, false);
        assert_eq!(canvas.name(), "canvas");
        // 真文本框聚焦仍是 TextEdit 态
        let text = top_input_context(false, false, true, false);
        assert_eq!(text.name(), "text.edit");
    }
}
