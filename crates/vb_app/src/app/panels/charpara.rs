//! 字符面板 `Ctrl+T` 与段落面板 `Ctrl+Alt+T`(04 阶段目标③,design/03 §5.10)。
//!
//! 结构与控制面板同纪律(ADR-VB-U04):**纯函数构建器 + 门禁测试** ——
//! 面板控件只做「投影 → 构建命令 → 经 [`VellumApp::exec`]/[`num_commit`]
//! 入 undo」,不在面板里私存文档状态(值全部回读 `SegStyle` / `Node.style`)。
//!
//! 作用域规则(副文档 04-1-2):
//! - 节点**存在段内 run**(`segments` 非空)→ 字段作用于全部 run(`SegStyle`,
//!   经 `SetSegs` 命令,可撤销);「整段转 run」按钮把全文包成单 run 以支持
//!   创建 run;「移除全部 run」回到整段。
//! - 否则作用于**整段**(`Node.style` 白名单声明,经 `SetStyle`)。
//! - 基线偏移只有 run 落点(`vertical-align` 对块级元素无意义)→ 整段置灰;
//!   语言 / 抗锯齿只有节点级落点(`lang` 属性 / `-webkit-font-smoothing`)
//!   → run 作用域置灰。
//!
//! **不放假控件**(design/03 §5.10 字段逐条裁定见 `04a` 报告白名单处置表):
//! 字偶距 / 垂直缩放 / 水平缩放 / 字符旋转无对称的 CSS 往返落点 → 冻结登记
//! (caption 说明),绝不做「点了没反应」的假输入框。
//!
//! 画布文字仍为 egui 近似(ADR-0017,诚实标注);溢出估算与自动扩高用
//! 导出同款量测(`vb_render::text`,真字形引擎),不受近似影响。

use egui::Color32;
use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, NodeId, NodeKind, SegStyle, TextMode, TextSeg};
use vb_ui::components::{caption, ColorField, NumField};
use vb_ui::theme;

use crate::app::control_panel::{combine, style_prop_cmds, style_prop_remove_cmds};
use crate::app::{set_style_prop, VellumApp};

// ═══════════════════ 1. 段落对齐九式(design/03 §5.10) ═══════════════════

/// 段落对齐九式。每式对应一组**互不相同**的白名单声明组合
/// (`text-align` + `text-align-last`),投影 [`Align9::from_style`] 与
/// 写回 [`Align9::decls`] 对称 → 往返幂等。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align9 {
    /// 左对齐
    Left,
    /// 居中对齐
    Center,
    /// 右对齐
    Right,
    /// 两端对齐 · 末行左(`text-align:justify` 的 CSS 默认末行行为)
    Justify,
    /// 两端对齐 · 末行居中
    JustifyLastCenter,
    /// 两端对齐 · 末行右
    JustifyLastRight,
    /// 两端对齐 · 末行两端
    JustifyLastJustify,
    /// 全部两端(含末行;`text-align:justify-all`)
    JustifyAll,
    /// 强制撑满(全部两端 + 末行两端声明)
    JustifyAllLast,
}

impl Align9 {
    pub const ALL: [Align9; 9] = [
        Align9::Left,
        Align9::Center,
        Align9::Right,
        Align9::Justify,
        Align9::JustifyLastCenter,
        Align9::JustifyLastRight,
        Align9::JustifyLastJustify,
        Align9::JustifyAll,
        Align9::JustifyAllLast,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Align9::Left => "左对齐",
            Align9::Center => "居中对齐",
            Align9::Right => "右对齐",
            Align9::Justify => "两端对齐·末行左",
            Align9::JustifyLastCenter => "两端对齐·末行居中",
            Align9::JustifyLastRight => "两端对齐·末行右",
            Align9::JustifyLastJustify => "两端对齐·末行两端",
            Align9::JustifyAll => "全部两端(含末行)",
            Align9::JustifyAllLast => "强制撑满(末行两端)",
        }
    }

    /// 按钮短符号(段落面板 9 式网格)。
    pub fn short(self) -> &'static str {
        match self {
            Align9::Left => "⇤",
            Align9::Center => "↔",
            Align9::Right => "⇥",
            Align9::Justify => "≡",
            Align9::JustifyLastCenter => "≡·中",
            Align9::JustifyLastRight => "≡·右",
            Align9::JustifyLastJustify => "≡≡",
            Align9::JustifyAll => "▮",
            Align9::JustifyAllLast => "▮▮",
        }
    }

    /// 该式的声明组(全部落白名单;顺序固定)。
    pub fn decls(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Align9::Left => vec![("text-align", "left")],
            Align9::Center => vec![("text-align", "center")],
            Align9::Right => vec![("text-align", "right")],
            Align9::Justify => vec![("text-align", "justify")],
            Align9::JustifyLastCenter => {
                vec![("text-align", "justify"), ("text-align-last", "center")]
            }
            Align9::JustifyLastRight => {
                vec![("text-align", "justify"), ("text-align-last", "right")]
            }
            Align9::JustifyLastJustify => {
                vec![("text-align", "justify"), ("text-align-last", "justify")]
            }
            Align9::JustifyAll => vec![("text-align", "justify-all")],
            Align9::JustifyAllLast => vec![
                ("text-align", "justify-all"),
                ("text-align-last", "justify"),
            ],
        }
    }

    /// 从节点 style 投影当前对齐式(未声明 text-align = None,面板视为
    /// 「未设置」;左/中/右式不携带 text-align-last)。
    pub fn from_style(style: &[Decl]) -> Option<Align9> {
        let get = |p: &str| style.iter().find(|d| d.prop == p).map(|d| d.value.clone());
        let ta = get("text-align")?;
        let tal = get("text-align-last");
        Some(match (ta.as_str(), tal.as_deref()) {
            ("left", _) => Align9::Left,
            ("center", _) => Align9::Center,
            ("right", _) => Align9::Right,
            ("justify", None | Some("auto")) => Align9::Justify,
            ("justify", Some("center")) => Align9::JustifyLastCenter,
            ("justify", Some("right")) => Align9::JustifyLastRight,
            ("justify", Some("justify")) => Align9::JustifyLastJustify,
            ("justify-all", None | Some("auto")) => Align9::JustifyAll,
            ("justify-all", Some("justify")) => Align9::JustifyAllLast,
            _ => return None,
        })
    }
}

/// 段落对齐写回:按式的声明组设置;式**不携带**的对齐声明
/// (如左对齐时的 `text-align-last`)整条移除,避免残留声明把投影
/// 推到另一式(往返幂等的另一半)。
pub fn para_align_cmds(doc: &Document, sids: &[String], a: Align9) -> Vec<Command> {
    let decls = a.decls();
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let mut st = doc.nodes.get(nid)?.style.clone();
            for (p, v) in &decls {
                st = set_style_prop(st, p, v);
            }
            for p in ["text-align", "text-align-last"] {
                if !decls.iter().any(|(dp, _)| *dp == p) {
                    st.retain(|d| d.prop != p);
                }
            }
            Some(Command::SetStyle {
                sid: sid.clone(),
                new: st,
                old: None,
            })
        })
        .collect()
}

// ═══════════════════ 2. 字符面板构建器(run / 整段两条写回路径) ═══════════════════

/// 「整段转 run」:把文本全文包成单 run(`0..len` 默认样式)。
/// 已有 run / 空文本 / 非文本节点 → None(按钮置灰的理由)。
pub fn make_full_run_cmd(doc: &Document, sid: &str) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    match &doc.nodes.get(nid)?.kind {
        NodeKind::Text { text, segments, .. } => {
            if !segments.is_empty() || text.is_empty() {
                return None;
            }
            Some(Command::SetSegs {
                sid: sid.to_string(),
                new: vec![TextSeg {
                    start: 0,
                    end: text.len(),
                    style: SegStyle::default(),
                }],
                old: None,
            })
        }
        _ => None,
    }
}

/// 「移除全部 run」:回到整段样式(SetSegs 空表;无 run → None)。
pub fn clear_runs_cmd(doc: &Document, sid: &str) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    match &doc.nodes.get(nid)?.kind {
        NodeKind::Text { segments, .. } if !segments.is_empty() => Some(Command::SetSegs {
            sid: sid.to_string(),
            new: Vec::new(),
            old: None,
        }),
        _ => None,
    }
}

/// run 作用域写字段:对节点**全部段注记**应用一次 `SegStyle` 覆盖,
/// 整批一条 `SetSegs`(可撤销)。无 run / 非文本 / 覆盖无效果 → None。
pub fn seg_field_cmd(doc: &Document, sid: &str, apply: impl Fn(&mut SegStyle)) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    let orig = match &doc.nodes.get(nid)?.kind {
        NodeKind::Text { segments, .. } if !segments.is_empty() => segments.clone(),
        _ => return None,
    };
    let mut new = orig.clone();
    for s in &mut new {
        apply(&mut s.style);
    }
    if new == orig {
        return None;
    }
    Some(Command::SetSegs {
        sid: sid.to_string(),
        new,
        old: None,
    })
}

/// 整段作用域:粗体开关 → `font-weight` 700/400(多选 → Compound)。
pub fn char_bold_cmds(doc: &Document, sids: &[String], bold: bool) -> Vec<Command> {
    style_prop_cmds(doc, sids, "font-weight", if bold { "700" } else { "400" })
}

/// 整段作用域:斜体开关 → `font-style` italic/normal。
pub fn char_italic_cmds(doc: &Document, sids: &[String], italic: bool) -> Vec<Command> {
    style_prop_cmds(
        doc,
        sids,
        "font-style",
        if italic { "italic" } else { "normal" },
    )
}

/// 整段作用域:下划线/删除线 → `text-decoration` 关键字合成。
/// 两项都关 = 整条移除(继承);开 = 按值序 `underline line-through` 重建。
pub fn char_deco_cmds(
    doc: &Document,
    sids: &[String],
    underline: bool,
    strike: bool,
) -> Vec<Command> {
    if !underline && !strike {
        return style_prop_remove_cmds(doc, sids, "text-decoration");
    }
    let mut v: Vec<&str> = Vec::new();
    if underline {
        v.push("underline");
    }
    if strike {
        v.push("line-through");
    }
    style_prop_cmds(doc, sids, "text-decoration", &v.join(" "))
}

// ═══════════════════ 3. 区域文本溢出(design/03 §六 红点 / 06 §3.6 自动扩高) ═══════════════════

fn decl_px(style: &[Decl], prop: &str) -> Option<f64> {
    style
        .iter()
        .find(|d| d.prop == prop)
        .and_then(|d| vb_common::units::parse_px(&d.value))
}

/// 区域文本溢出估算:内容需要高度 − 框高(px;≤0 = 不溢出)。
///
/// 与导出同款量测(`vb_render::text::measure_text_weighted`,贪心断行 +
/// 禁则 + 字重感知);行高 = 显式 px / 无单位倍数 × 字号 / 默认 1.32×
/// (与 vb_layout 同默认)。点文本宽度自适应,不判溢出。
pub fn area_overflow_px(doc: &Document, nid: NodeId) -> f64 {
    let Some(n) = doc.nodes.get(nid) else {
        return 0.0;
    };
    let NodeKind::Text {
        text,
        mode: TextMode::Area,
        ..
    } = &n.kind
    else {
        return 0.0;
    };
    let fs = decl_px(&n.style, "font-size").unwrap_or(16.0).max(1.0);
    let lh = match n.style_get("line-height") {
        Some(v) => {
            let t = v.trim();
            if t.ends_with("px") || t.ends_with("pt") {
                vb_common::units::parse_px(t).unwrap_or(fs * 1.32)
            } else {
                t.parse::<f64>().unwrap_or(1.32) * fs
            }
        }
        None => fs * 1.32,
    };
    let weight: u16 = n
        .style_get("font-weight")
        .and_then(|w| match w.trim() {
            "bold" => Some(700),
            "normal" => Some(400),
            other => other.parse().ok(),
        })
        .unwrap_or(400);
    let ls = decl_px(&n.style, "letter-spacing").unwrap_or(0.0);
    let family = n.style_get("font-family").unwrap_or("");
    let (max_w, lines) = vb_render::text::measure_text_weighted(
        text,
        family,
        fs as f32,
        weight,
        n.geom.w.max(1.0) as f32,
        ls as f32,
    );
    let _ = max_w;
    (lines as f64 * lh) - n.geom.h
}

/// 自动扩高(几何命令):把区域文本框高补足内容需要高度(向上取整);
/// 不溢出 / 非区域文本 → None。经 `SetGeom` 入 undo。
pub fn area_fit_height_cmd(doc: &Document, sid: &str) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    let overflow = area_overflow_px(doc, nid);
    if overflow <= 0.0 {
        return None;
    }
    let n = doc.nodes.get(nid)?;
    let mut g: Geom = n.geom;
    g.h = (g.h + overflow).ceil().max(1.0);
    Some(Command::SetGeom {
        sid: sid.to_string(),
        new: g,
        old: None,
        old_declared: None,
    })
}

// ═══════════════════ 4. 字段清单(design/03 §5.10 落位表;门 1 真相表) ═══════════════════

#[cfg(test)]
mod field_tables {
    /// 字符面板字段清单:(字段 id,design/03 §5.10 字段名)。
    /// 冻结登记项以 `frozen.` 前缀标识 —— 只做 caption,不做假控件。
    pub(crate) const CHAR_FIELDS: &[(&str, &str)] = &[
        ("char.family", "字体"),
        ("char.bold", "样式(粗体)"),
        ("char.italic", "样式(斜体)"),
        ("char.size", "大小"),
        ("char.line_height", "行距"),
        ("char.tracking", "字距"),
        ("char.baseline", "基线偏移"),
        ("char.underline", "下划线"),
        ("char.strike", "删除线"),
        ("char.lang", "语言"),
        ("char.smoothing", "抗锯齿"),
        ("char.frozen.kerning", "字偶距"),
        ("char.frozen.vscale", "垂直缩放"),
        ("char.frozen.hscale", "水平缩放"),
        ("char.frozen.rotation", "字符旋转"),
    ];

    /// 段落面板字段清单。
    pub(crate) const PARA_FIELDS: &[(&str, &str)] = &[
        ("para.align", "对齐(9 式)"),
        ("para.indent_l", "左缩进"),
        ("para.indent_r", "右缩进"),
        ("para.indent_first", "首行缩进"),
        ("para.space_before", "段前"),
        ("para.space_after", "段后"),
        ("para.kinsoku", "避头尾"),
        ("para.hyphens", "连字"),
        ("para.punct_squeeze", "标点挤压"),
        ("para.area_fit", "区域溢出·自动扩高"),
    ];
}

// ═══════════════════ 5. 渲染 ═══════════════════

/// 主选中文本的投影(面板每帧从文档回读,**不在面板私存**)。
struct TextProj {
    sid: String,
    has_runs: bool,
    mode: TextMode,
    style: Vec<Decl>,
    /// 首 run 样式(run 作用域的取值投影)。
    seg: Option<SegStyle>,
}

impl VellumApp {
    fn text_projection(&self) -> Option<TextProj> {
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

    /// 主循环装配:字符 / 段落浮窗(app.rs `update` 末尾调用)。
    pub(crate) fn show_char_panel(&mut self, ui: &mut egui::Ui) {
        if !self.char_panel_open {
            return;
        }
        let mut open = true;
        egui::Window::new("字符")
            .open(&mut open)
            .collapsible(false)
            .default_width(260.0)
            .show(ui.ctx(), |ui| {
                self.char_panel_body(ui);
            });
        self.char_panel_open = open;
    }

    pub(crate) fn show_para_panel(&mut self, ui: &mut egui::Ui) {
        if !self.para_panel_open {
            return;
        }
        let mut open = true;
        egui::Window::new("段落")
            .open(&mut open)
            .collapsible(false)
            .default_width(280.0)
            .show(ui.ctx(), |ui| {
                self.para_panel_body(ui);
            });
        self.para_panel_open = open;
    }

    // ─────────────── 字符面板 ───────────────

    fn char_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.text_projection() else {
            // 无文本选中:置灰 + 引导(design/14 空态);默认样式区仍可用
            ui.label(caption(
                ui,
                "未选中文本对象 —— 选中后在此编辑字符样式;下方为新建文本默认样式。",
            ));
            ui.separator();
            self.default_style_body(ui);
            return;
        };
        let runs = p.has_runs;
        ui.horizontal(|ui| {
            ui.label(if runs {
                "作用于:段内 run"
            } else {
                "作用于:整段"
            });
            if runs {
                if ui.button("移除全部 run").clicked() {
                    if let Some(cmd) = clear_runs_cmd(&self.doc, &p.sid) {
                        self.exec(cmd);
                        self.say("已移除段内 run(回到整段样式)");
                    }
                }
            } else {
                let can = make_full_run_cmd(&self.doc, &p.sid).is_some();
                if ui
                    .add_enabled(can, egui::Button::new("整段转 run"))
                    .clicked()
                {
                    if let Some(cmd) = make_full_run_cmd(&self.doc, &p.sid) {
                        self.exec(cmd);
                        self.say("已把全文包成单 run(字符样式现作用于 run)");
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
            ui.label("字体");
            if ui
                .add_sized([150.0, 18.0], egui::TextEdit::singleline(&mut family))
                .lost_focus()
            {
                let t = family.trim().to_string();
                self.commit_char_family(&sid, runs, node_family.as_deref(), &t);
            }
        });
        let families = doc_font_families(&self.doc);
        if !families.is_empty() {
            ui.horizontal(|ui| {
                ui.label(caption(ui, "文档已有:"));
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
            if ui.checkbox(&mut bold, "粗体").changed() {
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
            if ui.checkbox(&mut italic, "斜体").changed() {
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
            let r = NumField::new("基线", &mut bs)
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
            ui.label(caption(ui, "基线偏移:仅段内 run(整段落点无有效 CSS)"));
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
            if ui.checkbox(&mut ul, "下划线").changed() {
                self.commit_char_deco(&sid, runs, ul, st);
            }
            if ui.checkbox(&mut st, "删除线").changed() {
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
                ui.label("语言");
                if ui
                    .add_sized([120.0, 18.0], egui::TextEdit::singleline(&mut lang))
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
            ui.label(caption(ui, "语言 / 抗锯齿:仅整段(继承节点)"));
        }

        // 抗锯齿(仅整段;-webkit-font-smoothing)
        ui.add_enabled_ui(!runs, |ui| {
            let cur = style
                .iter()
                .find(|d| d.prop == "-webkit-font-smoothing")
                .map(|d| d.value.clone());
            let mut sel = cur.clone().unwrap_or_else(|| "auto".into());
            egui::ComboBox::from_id_salt("char_smoothing")
                .selected_text(format!(
                    "抗锯齿 {}",
                    if sel == "auto" {
                        "自动"
                    } else {
                        sel.as_str()
                    }
                ))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut sel, "auto".into(), "自动");
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
            "字偶距 / 垂直缩放 / 水平缩放 / 字符旋转:冻结点 —— 无对称 CSS 往返落点,不做假控件(04a 报告处置表)",
        ));
        ui.label(caption(
            ui,
            "画布文字为近似渲染(ADR-0017);导出为真字形,以浏览器校对为准。",
        ));

        ui.separator();
        self.default_style_body(ui);
    }

    /// 新建文本默认样式(会话级;04-3-3:新建文本继承此处,替换写死 24px/黑)。
    fn default_style_body(&mut self, ui: &mut egui::Ui) {
        ui.strong("新建文本默认样式");
        let mut fs = self.text_default.font_size.unwrap_or(24.0);
        let r = NumField::new("字号", &mut fs)
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
        let r = NumField::new("行距", &mut lh)
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
        let r = NumField::new("字距", &mut ls)
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
            if ui.checkbox(&mut bold, "粗体").changed() {
                self.text_default.bold = Some(bold);
            }
            let mut italic = self.text_default.italic == Some(true);
            if ui.checkbox(&mut italic, "斜体").changed() {
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
        let r = ColorField::new("字色", &mut col).ui(ui);
        if r.changed && !r.cleared {
            let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
            self.text_default.color = Some(vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex());
        }
        ui.label(caption(
            ui,
            "默认样式为会话状态(持久化 → workspace.json,阶段 7 登记)。",
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
                ui.label(caption(ui, "(继承节点;输入数值即写 run 覆盖)"));
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
            ui.label(caption(ui, &format!("{label}:未声明(继承)")));
        }
    }

    // ─────────────── 段落面板 ───────────────

    fn para_panel_body(&mut self, ui: &mut egui::Ui) {
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

/// 节点 style 上的 px 数值取值(font-size / line-height / letter-spacing …)。
fn style_font_px(style: &[Decl], prop: &str) -> Option<f64> {
    decl_px(style, prop)
}

/// px 数值字段的静态规格(消参数爆炸;`prop` 同时是 run 字段分派键)。
struct PxFieldSpec {
    label: &'static str,
    prop: &'static str,
    lo: f64,
    hi: f64,
}

const SIZE_SPEC: PxFieldSpec = PxFieldSpec {
    label: "大小",
    prop: "font-size",
    lo: 1.0,
    hi: 1000.0,
};
const LH_SPEC: PxFieldSpec = PxFieldSpec {
    label: "行距",
    prop: "line-height",
    lo: 1.0,
    hi: 2000.0,
};
const TRACK_SPEC: PxFieldSpec = PxFieldSpec {
    label: "字距",
    prop: "letter-spacing",
    lo: -50.0,
    hi: 500.0,
};
const INDENT_L_SPEC: PxFieldSpec = PxFieldSpec {
    label: "左缩进",
    prop: "padding-left",
    lo: 0.0,
    hi: 1000.0,
};
const INDENT_R_SPEC: PxFieldSpec = PxFieldSpec {
    label: "右缩进",
    prop: "padding-right",
    lo: 0.0,
    hi: 1000.0,
};
const INDENT_FIRST_SPEC: PxFieldSpec = PxFieldSpec {
    label: "首行缩",
    prop: "text-indent",
    lo: -200.0,
    hi: 1000.0,
};
const SPACE_BEFORE_SPEC: PxFieldSpec = PxFieldSpec {
    label: "段前",
    prop: "margin-top",
    lo: 0.0,
    hi: 2000.0,
};
const SPACE_AFTER_SPEC: PxFieldSpec = PxFieldSpec {
    label: "段后",
    prop: "margin-bottom",
    lo: 0.0,
    hi: 2000.0,
};

/// 下拉字段的静态规格。
struct ComboSpec {
    label: &'static str,
    salt: &'static str,
    prop: &'static str,
    /// (值, 显示名);首项语义必须是「默认/无」= 移除声明。
    options: &'static [(&'static str, &'static str)],
}

const KINSOKU_SPEC: ComboSpec = ComboSpec {
    label: "避头尾",
    salt: "para_kinsoku",
    prop: "line-break",
    options: &[
        ("auto", "自动"),
        ("loose", "宽松"),
        ("normal", "一般"),
        ("strict", "严格"),
    ],
};
const HYPHENS_SPEC: ComboSpec = ComboSpec {
    label: "连字",
    salt: "para_hyphens",
    prop: "hyphens",
    options: &[("manual", "手动(默认)"), ("auto", "自动"), ("none", "关")],
};
const PUNCT_SPEC: ComboSpec = ComboSpec {
    label: "标点挤压",
    salt: "para_punct",
    prop: "hanging-punctuation",
    options: &[
        ("none", "无"),
        ("allow-end", "允许末行悬挂"),
        ("force-end", "强制末行悬挂"),
    ],
};

/// 文档中已使用的字体族(字符面板「文档已有」快选;扫描节点声明去重)。
fn doc_font_families(doc: &Document) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, n) in doc.nodes.iter() {
        if let Some(f) = n.style_get("font-family") {
            let f = f.trim().to_string();
            if !f.is_empty() && !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

// ═══════════════════ 6. 门禁测试 ═══════════════════

#[cfg(test)]
mod tests {
    use vb_doc::export::render_project;
    use vb_doc::model::{Node, TextMode};
    use vb_doc::undo::UndoStack;

    use super::*;

    /// 夹具:画板 + 文本节点(内容与几何自定)。
    fn text_doc(text: &str, mode: TextMode, w: f64, h: f64) -> (Document, String) {
        let mut doc = Document::new_default();
        let ab = doc.artboards.first().copied().unwrap();
        let sid = doc.alloc_sid();
        let mut n = Node::new(
            NodeKind::Text {
                text: text.into(),
                mode,
                segments: Vec::new(),
            },
            "文本",
            sid.clone(),
        );
        n.geom = Geom {
            x: 0.0,
            y: 0.0,
            w,
            h,
        };
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(id);
        (doc, sid.as_str().to_string())
    }

    fn style_of<'a>(doc: &'a Document, sid: &str) -> &'a Vec<Decl> {
        &doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().style
    }

    fn segs_of<'a>(doc: &'a Document, sid: &str) -> &'a [TextSeg] {
        match &doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().kind {
            NodeKind::Text { segments, .. } => segments,
            _ => panic!("不是文本"),
        }
    }

    // ───────────── 门 1:字段清单与 design/03 §5.10 一致 ─────────────

    /// 字符面板字段清单 = design/03 §5.10 字段表(顺序一致;冻结登记项
    /// 显式在列 —— 是「登记」而非「丢失」)。
    #[test]
    fn char_fields_match_design_5_10() {
        let ids: Vec<&str> = field_tables::CHAR_FIELDS
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "char.family",          // 字体
                "char.bold",            // 样式·粗体
                "char.italic",          // 样式·斜体
                "char.size",            // 大小
                "char.line_height",     // 行距
                "char.tracking",        // 字距
                "char.baseline",        // 基线偏移
                "char.underline",       // 下划线
                "char.strike",          // 删除线
                "char.lang",            // 语言
                "char.smoothing",       // 抗锯齿
                "char.frozen.kerning",  // 字偶距(冻结)
                "char.frozen.vscale",   // 垂直缩放(冻结)
                "char.frozen.hscale",   // 水平缩放(冻结)
                "char.frozen.rotation", // 字符旋转(冻结)
            ],
            "字符面板字段与 design/03 §5.10 漂移"
        );
    }

    /// 段落面板字段清单 = design/03 §5.10 / 副文档 04-2。
    #[test]
    fn para_fields_match_design() {
        let ids: Vec<&str> = field_tables::PARA_FIELDS
            .iter()
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "para.align",
                "para.indent_l",
                "para.indent_r",
                "para.indent_first",
                "para.space_before",
                "para.space_after",
                "para.kinsoku",
                "para.hyphens",
                "para.punct_squeeze",
                "para.area_fit",
            ],
            "段落面板字段与 design/03 §5.10 漂移"
        );
    }

    // ───────────── 门 2:对齐九式投影/写回对称 + 命令可逆 ─────────────

    /// 九式声明组互不相同;from_style(decls) 恒等投影(往返幂等的根基)。
    #[test]
    fn align9_decls_are_distinct_and_project_identity() {
        for a in Align9::ALL {
            let decls: Vec<Decl> = a
                .decls()
                .into_iter()
                .map(|(p, v)| Decl {
                    prop: p.into(),
                    value: v.into(),
                    important: false,
                })
                .collect();
            assert_eq!(
                Align9::from_style(&decls),
                Some(a),
                "{} 的声明组投影不是恒等",
                a.label()
            );
        }
        let distinct: std::collections::HashSet<Vec<String>> = Align9::ALL
            .iter()
            .map(|a| a.decls().iter().map(|(p, v)| format!("{p}:{v}")).collect())
            .collect();
        assert_eq!(distinct.len(), 9, "九式声明组必须互不相同");
    }

    /// 段落对齐写回 → undo 精确逆回 → redo 等效(导出快照级)。
    #[test]
    fn para_align_cmds_undo_exact() {
        let (mut doc, sid) = text_doc("段落对齐测试", TextMode::Area, 200.0, 60.0);
        let before = render_project(&doc).files;
        let mut stack = UndoStack::new();
        // 本测试验证「逐命令」精确逆回;合并行为由 vb_doc charseg/undo 测试覆盖
        stack.merging_enabled = false;
        let cmd = para_align_cmds(&doc, std::slice::from_ref(&sid), Align9::JustifyLastCenter)
            .into_iter()
            .next()
            .unwrap();
        stack.push(&mut doc, cmd).expect("apply");
        let styled = render_project(&doc).files;
        assert_ne!(styled, before);
        let css = styled.iter().find(|(p, _)| p == "styles/main.css").unwrap();
        assert!(
            css.1.contains("text-align-last: center"),
            "导出应带末行居中"
        );
        stack.undo(&mut doc).expect("undo");
        assert_eq!(render_project(&doc).files, before, "undo 精确逆回");
        stack.redo(&mut doc).expect("redo");
        assert_eq!(render_project(&doc).files, styled, "redo 等效");

        // 切回左对齐:text-align-last 整条移除(不留残留声明)
        let cmd = para_align_cmds(&doc, std::slice::from_ref(&sid), Align9::Left)
            .into_iter()
            .next()
            .unwrap();
        stack.push(&mut doc, cmd).expect("apply");
        assert!(
            !style_of(&doc, &sid)
                .iter()
                .any(|d| d.prop == "text-align-last"),
            "切回左对齐不得残留 text-align-last"
        );
        assert_eq!(Align9::from_style(style_of(&doc, &sid)), Some(Align9::Left));
    }

    // ───────────── 门 2:字符 run / 整段双路写回 ─────────────

    /// 整段转 run → run 字段覆盖 → 移除 run;全程 undo 精确逆回。
    #[test]
    fn char_run_pipeline_undo_exact() {
        let (mut doc, sid) = text_doc("富文本样式", TextMode::Point, 200.0, 40.0);
        let before = render_project(&doc).files;
        let mut stack = UndoStack::new();
        stack.merging_enabled = false; // 同上:逐命令逆回语义
        let push = |stack: &mut UndoStack, doc: &mut Document, cmd: Option<Command>| {
            stack.push(doc, cmd.expect("命令应存在")).expect("apply");
        };
        // 注意:构建器(&doc)与 push(&mut doc) 参数先行物化,借用分离

        let c = make_full_run_cmd(&doc, &sid);
        push(&mut stack, &mut doc, c);
        assert_eq!(segs_of(&doc, &sid).len(), 1);

        let c = seg_field_cmd(&doc, &sid, |s| {
            s.bold = Some(true);
            s.letter_spacing = Some(2.0);
            s.baseline_shift = Some(4.0);
            s.underline = Some(true);
        });
        push(&mut stack, &mut doc, c);
        let segs = segs_of(&doc, &sid);
        assert_eq!(segs[0].style.bold, Some(true));
        assert_eq!(segs[0].style.letter_spacing, Some(2.0));
        let styled = render_project(&doc).files;

        let c = clear_runs_cmd(&doc, &sid);
        push(&mut stack, &mut doc, c);
        assert!(segs_of(&doc, &sid).is_empty());

        // 连续三次 undo:移除 run → 改字段 → 建 run,逐步精确逆回
        stack.undo(&mut doc).expect("undo");
        assert_eq!(segs_of(&doc, &sid).len(), 1);
        assert_eq!(render_project(&doc).files, styled);
        stack.undo(&mut doc).expect("undo");
        assert_eq!(
            segs_of(&doc, &sid)[0].style,
            SegStyle::default(),
            "字段覆盖应被精确逆回"
        );
        stack.undo(&mut doc).expect("undo");
        assert!(segs_of(&doc, &sid).is_empty());
        assert_eq!(render_project(&doc).files, before);
    }

    /// 整段作用域:粗体 / 斜体 / 下划线 / 字距写 Node.style,undo 精确逆回。
    #[test]
    fn char_node_scope_writes_and_reverts() {
        let (mut doc, sid) = text_doc("整段样式", TextMode::Point, 200.0, 40.0);
        let before = render_project(&doc).files;
        let mut stack = UndoStack::new();
        stack.merging_enabled = false;
        let sids = vec![sid.clone()];
        // SetStyle 是整表替换:命令必须按**当前文档状态**顺序构建
        // (与面板交互时序一致;预建数组会用旧状态互相覆盖)
        let cmd = char_bold_cmds(&doc, &sids, true).remove(0);
        stack.push(&mut doc, cmd).expect("apply");
        let cmd = char_italic_cmds(&doc, &sids, true).remove(0);
        stack.push(&mut doc, cmd).expect("apply");
        let cmd = char_deco_cmds(&doc, &sids, true, true).remove(0);
        stack.push(&mut doc, cmd).expect("apply");
        let cmd = style_prop_cmds(&doc, &sids, "letter-spacing", "1.5px").remove(0);
        stack.push(&mut doc, cmd).expect("apply");
        let styled = render_project(&doc).files;
        let css = styled.iter().find(|(p, _)| p == "styles/main.css").unwrap();
        for expect in [
            "font-weight: 700",
            "font-style: italic",
            "text-decoration: underline line-through",
            "letter-spacing: 1.5px",
        ] {
            assert!(css.1.contains(expect), "导出应含 {expect}");
        }
        for _ in 0..4 {
            stack.undo(&mut doc).expect("undo");
        }
        assert_eq!(render_project(&doc).files, before, "逐条 undo 精确逆回");
    }

    // ───────────── 门 2:区域文本溢出估算 + 自动扩高 ─────────────

    /// 溢出估算与自动扩高:窄框溢出 → 扩高到内容需要高度 → 再估不溢出;
    /// SetGeom undo 精确逆回。量测与导出同引擎(真字形),不依赖 egui。
    #[test]
    fn area_fit_height_expands_to_content_and_reverts() {
        let long_text = "这是一段用于溢出测试的中文文案,反复重复以触发换行。".repeat(6);
        let (mut doc, sid) = text_doc(&long_text, TextMode::Area, 120.0, 30.0);
        let nid = doc.find_by_sid(&sid).unwrap();
        assert!(area_overflow_px(&doc, nid) > 0.0, "窄框长文必须判溢出");
        let before = render_project(&doc).files;
        let h0 = doc.nodes.get(nid).unwrap().geom.h;
        let mut stack = UndoStack::new();
        let fit = area_fit_height_cmd(&doc, &sid).unwrap();
        stack.push(&mut doc, fit).expect("apply");
        let h1 = doc
            .nodes
            .get(doc.find_by_sid(&sid).unwrap())
            .unwrap()
            .geom
            .h;
        assert!(h1 > h0, "扩高后必须更高:{h0} → {h1}");
        let nid = doc.find_by_sid(&sid).unwrap();
        assert!(area_overflow_px(&doc, nid) <= 0.0, "扩高后不应再溢出");
        let after = render_project(&doc).files;
        stack.undo(&mut doc).expect("undo");
        assert_eq!(render_project(&doc).files, before, "undo 精确逆回");
        stack.redo(&mut doc).expect("redo");
        assert_eq!(render_project(&doc).files, after, "redo 等效");

        // 不溢出时自动扩高 = 无命令(按钮置灰的依据)
        let (doc2, sid2) = text_doc("短", TextMode::Area, 400.0, 200.0);
        assert!(area_fit_height_cmd(&doc2, &sid2).is_none());
        // 点文本不参与
        let (doc3, sid3) = text_doc("点文本", TextMode::Point, 40.0, 8.0);
        let nid3 = doc3.find_by_sid(&sid3).unwrap();
        assert_eq!(area_overflow_px(&doc3, nid3), 0.0);
        let _ = doc2;
    }
}
