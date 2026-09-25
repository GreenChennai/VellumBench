//! 控制面板 · 渲染器:菜单栏下 40px 通栏(`control_bar`)+ 右侧固定区。
//!
//! 06-1 自 `control_panel.rs` 按职责拆出(纯搬移,零行为变化):
//! 字段级编辑器方法在 `editors`(同属 `impl VellumApp`,按职责分文件)。

use vb_css::Decl;
use vb_doc::commands::Command;
use vb_ui::components::NumField;
use vb_ui::theme;

use super::spec::*;
use super::writes::*;
use crate::app::VellumApp;

// ═══════════════════════════ 3. 渲染器 ═══════════════════════════

// 画板尺寸预设(02-5-3):**常量单一来源 = `panels::artboards::AB_PRESETS`**
// (S1-d 起画板面板与本面板共用;七档 Web 1920/1440/1080、移动 750/375、
// A4 横竖 + 自定义)。PRESET_CUSTOM 用于尺寸不匹配任何行时的显示名。

impl VellumApp {
    /// 当前工具态 spec(把 VellumApp 状态投影成纯输入再查表)。
    pub(crate) fn current_spec(&self) -> ControlPanelSpec {
        spec_for(&CtlCtx {
            tool: self.tool,
            has_selection: !self.selection.is_empty(),
        })
    }

    /// 控制面板:菜单栏下 40px 通栏(egui 顶栏按 show 顺序堆叠,
    /// 本面板在 menu 之后 show,自然落在其下方)。
    pub(crate) fn control_bar(&mut self, ui: &mut egui::Ui) {
        let t = theme::tokens(ui.ctx());
        egui::Panel::top("control_bar")
            .exact_size(theme::space::CONTROL_BAR_HEIGHT)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(t.bg_panel)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ui, |ui| {
                // 固定区预留宽:标题(130)+ 画板切换(170)+ 缩放(90)+ 分隔与间距
                const FIXED_RESERVE: f32 = 430.0;
                let avail = ui.available_width();
                let spec = self.current_spec();
                ui.horizontal(|ui| {
                    // ── 变量区(随工具态;过窄窗口横向滚动,不挤压固定区) ──
                    egui::ScrollArea::horizontal()
                        .auto_shrink([false, true])
                        .max_width((avail - FIXED_RESERVE).max(120.0))
                        .show(ui, |ui| {
                            ui.set_min_height(theme::space::ROW_HEIGHT);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = theme::space::S3;
                                for field in &spec.fields {
                                    self.control_field_ui(ui, field);
                                }
                                // 需要选区的态没有对象时给引导,不画死控件
                                // (不做"点了没反应")
                                if needs_selection(spec.state) && self.selection.is_empty() {
                                    ui.weak("先选中对象");
                                }
                            });
                        });
                    ui.separator();
                    // ── 固定右侧区(02-2-3) ──
                    ui.spacing_mut().item_spacing.x = theme::space::S3;
                    self.fixed_zone_ui(ui);
                });
            });
    }

    /// 单个字段的取值 + 控件 + 提交(按 id 分发;取值/写回都走 §2 构建器)。
    pub(super) fn control_field_ui(&mut self, ui: &mut egui::Ui, field: &CtlField) {
        // 04-5-3:画板数(只读计数;design/03 §三「画板选项:… 画板数」)。
        // 增删画板走「+画板」按钮与画板面板,此处不做假的可编辑控件。
        if field.id == "ab.count" {
            ui.weak(format!("画板数 {}", self.doc.artboards.len()));
            return;
        }
        match field.kind {
            CtlKind::Hint => {
                ui.weak(field.label);
                return;
            }
            CtlKind::Button => {
                let clicked = ui.button(field.label).on_hover_text(field.id).clicked();
                if clicked {
                    match field.write {
                        CtlWrite::App(id) => self.run_command(id, false, false),
                        CtlWrite::Doc("Insert") => self.add_default_artboard(),
                        CtlWrite::Doc("SetStyle(background-image)") => {
                            // 渐变反向(线性 +180° / 径向色标逆序)
                            let sids = self.selection.clone();
                            let cmds = gradient_reverse_cmds(&self.doc, &sids);
                            if let Some(cmd) = combine(cmds) {
                                self.exec(cmd);
                                self.say("渐变已反向");
                            }
                        }
                        _ => {}
                    }
                }
                return;
            }
            _ => {}
        }
        match field.id {
            // ── 变换(design/03:选择·有选区) ──
            "x" | "y" | "w" | "h" => self.geom_field_ui(ui, field),
            "rot" => {
                let Some(v0) = self.primary_style().map(|s| rotation_deg_of(&s)) else {
                    return;
                };
                let mut v = v0;
                let r = NumField::new(field.label, &mut v)
                    .speed(1.0)
                    .step(15.0)
                    .label_width(20.0)
                    .width(56.0)
                    .unit("°")
                    .ui(ui);
                let cmd = r.changed.then(|| {
                    let sids = self.selection.clone();
                    combine(rotation_cmds(&self.doc, &sids, v))
                });
                self.num_commit(r, cmd.flatten());
            }
            "fill" | "pen.fill" | "sh.fill" | "t.color" => {
                let prop = match field.id {
                    "pen.fill" => "fill",
                    "t.color" => "color",
                    _ => "background-color",
                };
                self.color_field_ui(ui, field.label, prop);
            }
            "opacity" => {
                let v0 = self.primary_style_num("opacity").unwrap_or(1.0);
                let mut v = v0;
                let r = NumField::new(field.label, &mut v)
                    .speed(0.01)
                    .step(0.01)
                    .range(0.0, 1.0)
                    .label_width(44.0)
                    .width(56.0)
                    .ui(ui);
                let cmd = r.changed.then(|| {
                    let sids = self.selection.clone();
                    let cmds = style_prop_cmds(&self.doc, &sids, "opacity", &format!("{v:.2}"));
                    combine(cmds)
                });
                self.num_commit(r, cmd.flatten());
            }
            // ── 直接选择:锚点坐标 ──
            "ax" | "ay" => self.anchor_field_ui(ui, field),
            // ── 钢笔 / 形状 ──
            "pen.stroke" => self.color_field_ui(ui, field.label, "stroke"),
            "pen.sw" => self.style_num_field_ui(ui, field.label, "stroke-width", 0.1, 200.0),
            "sh.stroke" => self.border_color_ui(ui, field.label),
            "sh.sw" => self.border_width_ui(ui, field.label),
            "sh.radius" => self.style_num_field_ui(ui, field.label, "border-radius", 0.0, 4000.0),
            // ── 文字 ──
            "t.size" => self.style_num_field_ui(ui, field.label, "font-size", 1.0, 1000.0),
            "t.align" => self.text_align_ui(ui),
            // ── 渐变 ──
            "g.kind" => self.gradient_kind_ui(ui),
            "g.angle" => self.gradient_angle_ui(ui),
            // ── 画板选项 / 画板工具 ──
            "ab.preset" => self.artboard_preset_ui(ui),
            "ab.orient" => self.artboard_orient_ui(ui),
            "ab.w" | "ab.h" => self.artboard_size_ui(ui, field),
            "ab.x" | "ab.y" => self.artboard_pos_ui(ui, field),
            "ab.name" => self.artboard_name_ui(ui),
            "ab.bg" => self.artboard_bg_ui(ui),
            // Button / Hint 已在上面的分支处理;兜底不给交互控件
            "align.h" | "align.v" | "group" | "fwd" | "bwd" | "g.reverse" | "ab.add"
            | "anchor.hint" | "pen.hint" | "text.hint" | "g.hint" | "ab.hint" | "view.hint"
            | "eyedropper.hint" | "scissors.hint" => {}
            other => {
                ui.weak(other);
            }
        }
    }
}

impl VellumApp {
    /// 固定右侧区:文档标题(改名经 SetMetaTitle,导出写 `<title>`)+
    /// 画板切换下拉(联动画布:选中 + 缩放到选区)+ 缩放下拉
    /// (全部走既有 view.* 命令 ID;浏览器校对按钮 → 阶段 8 遗留)。
    pub(super) fn fixed_zone_ui(&mut self, ui: &mut egui::Ui) {
        // 文档标题(真实写文档:SetMetaTitle)
        let mut title = self.doc.meta.title.clone();
        ui.label("标题");
        if ui
            .add_sized(
                [110.0, vb_ui::theme::row_height(ui.ctx())],
                egui::TextEdit::singleline(&mut title),
            )
            .lost_focus()
            && !title.trim().is_empty()
            && title.trim() != self.doc.meta.title
        {
            let new = title.trim().to_string();
            self.exec(Command::SetMetaTitle { new, old: None });
            self.say(format!("文档标题 → {}", title.trim()));
        }
        // 画板切换下拉(联动画布)
        let idx = self
            .selection
            .first()
            .and_then(|s| self.doc.find_by_sid(s))
            .and_then(|id| self.doc.artboards.iter().position(|&a| a == id));
        let cur_name = idx
            .map(|i| {
                self.doc
                    .nodes
                    .get(self.doc.artboards[i])
                    .unwrap()
                    .name
                    .clone()
            })
            .unwrap_or_else(|| format!("共 {} 块", self.doc.artboards.len()));
        egui::ComboBox::from_id_salt("ctl_ab_switch")
            .selected_text(format!(
                "{}/{} {}",
                idx.map(|i| i + 1).unwrap_or(0),
                self.doc.artboards.len(),
                short_name(&cur_name, 8)
            ))
            .show_ui(ui, |ui| {
                for (i, &ab) in self.doc.artboards.clone().iter().enumerate() {
                    let n = self.doc.nodes.get(ab).unwrap();
                    let selected = idx == Some(i);
                    if ui
                        .selectable_label(
                            selected,
                            format!("{} {}×{}", n.name, n.geom.w as i64, n.geom.h as i64),
                        )
                        .clicked()
                    {
                        self.selection = vec![n.sid.as_str().to_string()];
                        // 联动画布:选中后视图居中(既有命令路径)
                        self.run_command("view.zoom_to_selection", false, false);
                    }
                }
            });
        // 缩放下拉(全部既有命令 ID)
        let pct = (self.camera.zoom * 100.0).round() as i64;
        egui::ComboBox::from_id_salt("ctl_zoom")
            .selected_text(format!("{pct}%"))
            .show_ui(ui, |ui| {
                if ui.selectable_label(false, "适合窗口").clicked() {
                    self.run_command("view.fit", false, false);
                    ui.close();
                }
                if ui.selectable_label(false, "100%").clicked() {
                    self.run_command("view.actual_size", false, false);
                    ui.close();
                }
                if ui.selectable_label(false, "放大一档").clicked() {
                    self.run_command("view.zoom_in", false, false);
                    ui.close();
                }
                if ui.selectable_label(false, "缩小一档").clicked() {
                    self.run_command("view.zoom_out", false, false);
                    ui.close();
                }
            });
        // 浏览器校对按钮:design/03 §三 称"本产品独有",但现有命令层
        // 无可复用实现(vb_browser 仅 CLI 侧)—— 不放假按钮,登记阶段 8。
    }

    // ── 取值辅助(主选中快照) ──

    pub(super) fn primary_style(&self) -> Option<Vec<Decl>> {
        let sid = self.selection.last()?;
        let nid = self.doc.find_by_sid(sid)?;
        Some(self.doc.nodes.get(nid)?.style.clone())
    }

    pub(super) fn primary_style_num(&self, prop: &str) -> Option<f64> {
        let style = self.primary_style()?;
        let d = style.iter().find(|d| d.prop == prop)?;
        let v: f64 = d.value.trim_end_matches("px").trim().parse().ok()?;
        Some(v)
    }

    /// 主选中 border 简写 → (宽, 颜色);无边框 = (1, None)。
    pub(super) fn primary_border(&self) -> Option<(f64, Option<vb_common::Rgba>)> {
        let style = self.primary_style()?;
        match style.iter().find(|d| d.prop == "border") {
            Some(d) => {
                let mut it = d.value.split_whitespace();
                let w: f64 = it
                    .next()
                    .and_then(|s| s.trim_end_matches("px").parse().ok())
                    .unwrap_or(1.0);
                let col = it.find_map(vb_common::color::parse_color);
                Some((w, col))
            }
            None => Some((1.0, None)),
        }
    }

    /// 主选中渐变 → (类型, 角度, 色标)。
    pub(super) fn primary_gradient(&self) -> Option<(GradKind, f64, Vec<String>)> {
        let style = self.primary_style()?;
        let d = style.iter().find(|d| d.prop == "background-image")?;
        parse_gradient(&d.value)
    }

    /// 直接选择的锚点目标:(sid, 顶点序号)。
    /// 拖拽/点选中的锚点优先;否则取首个选中矢量的第 0 个锚点。
    pub(super) fn anchor_target(&self) -> Option<(String, usize)> {
        if let Some((sid, vi)) = &self.ds_vertex {
            return Some((sid.clone(), *vi));
        }
        let sid = self.selection.last()?;
        self.vector_vertices(sid)
            .first()
            .map(|(i, _, _)| (sid.clone(), *i))
    }
}

/// 需要选区才有意义的工具态(无选区时渲染器给引导文案而非死控件)。
fn needs_selection(state: &str) -> bool {
    matches!(
        state,
        "select.transform" | "gradient" | "text" | "pen" | "rect" | "ellipse"
    )
}

/// 控制面板紧凑数值框(单行 40px 条:窄标签 + 56px 输入)。
pub(super) fn num_compact<'a>(_: &mut egui::Ui, label: &'a str, v: &'a mut f64) -> NumField<'a> {
    let w = if label.chars().count() <= 2 {
        20.0
    } else {
        44.0
    };
    NumField::new(label, v)
        .speed(1.0)
        .label_width(w)
        .width(56.0)
}

/// 名称截断(下拉选中态避免撑爆 40px 条)。
fn short_name(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars).collect();
        format!("{t}…")
    }
}
