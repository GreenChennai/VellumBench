//! 颜色面板(`F6`,副文档 05-5-1)+ 色板区(05-5-2)。
//!
//! - **颜色面板**:HSB / RGB / CMYK / CSS 变量 四个页签,四种输入都能转成
//!   合法 CSS;`D` 恢复默认、`X` 切换填充/描边、`Shift+X` 交换两色(05-5-1)。
//! - **色板区**:全局色 = CSS 变量(`--vb-color-N`,存于文档令牌 → `:root`),
//!   白三角标记;**改一处全站生效**(与「设计令牌」同一机制,05-5-2)。
//! - **从页面提取颜色**:依赖 WPI `color_profiler`,当前**代码里不存在该能力**,
//!   故**不放按钮**(副文档 05 §12.4 口径:无支撑则砍掉,不做点了没反应的假控件),
//!   只在面板底部以文字如实说明。

use vb_common::color::Rgba;
use vb_common::units::fmt_num;
use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeKind};
use vb_ui::components::{caption, ColorField, NumField};
use vb_ui::icons;

use crate::app::appearance::{self, AppearanceItem, AppearanceTarget, FillBody};
use crate::app::{style_set_or_remove, VellumApp};

/// 全局色令牌名前缀(`--vb-color-N`)。
pub const GLOBAL_PREFIX: &str = "vb-color-";

/// 默认填充 / 描边色(`D` 键恢复)。
// vb-token-ok: 文档内容色(AI 默认填充白 / 描边黑),非 UI 皮肤
pub const DEFAULT_FILL: &str = "#ffffff";
// vb-token-ok: 文档内容色
pub const DEFAULT_STROKE: &str = "#000000";

// ─────────────────────── 1. 目标色读写(纯函数) ───────────────────────

/// 颜色作用目标的 CSS 落点属性。
fn fill_prop(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Text { .. } => "color",
        NodeKind::Vector { .. } => "fill",
        _ => "background-color",
    }
}

fn stroke_prop(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Text { .. } => "-webkit-text-stroke-color",
        NodeKind::Vector { .. } => "stroke",
        _ => "border-color",
    }
}

/// 读取当前目标色(CSS 串;可能是 `var(--x)`)。外观模型接管时以模型为真相。
pub fn read_target_color(doc: &Document, sid: &str, stroke: bool) -> Option<String> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.nodes.get(nid)?;
    let attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let model = appearance::decode_model(&n.kind, &n.style, &attrs);
    if stroke {
        if let Some(c) = model.items.iter().find_map(|it| match it {
            AppearanceItem::Stroke(s) => s.spec.color.clone(),
            _ => None,
        }) {
            return Some(c);
        }
    } else if let Some(c) = model.items.iter().find_map(|it| match it {
        AppearanceItem::Fill(f) => match &f.body {
            FillBody::Solid { value } => Some(value.clone()),
            FillBody::Gradient { value } => {
                vb_ui::gradient::parse(value).map(|g| g.stops[0].color.clone())
            }
            FillBody::Raw { .. } => None,
        },
        _ => None,
    }) {
        return Some(c);
    }
    let prop = if stroke {
        stroke_prop(&n.kind)
    } else {
        fill_prop(&n.kind)
    };
    n.style
        .iter()
        .find(|d| d.prop == prop)
        .map(|d| d.value.clone())
}

/// 生成写回命令。外观模型接管时改条目(与外观面板同源),否则写 CSS 属性。
pub fn write_color_cmd(
    doc: &Document,
    sid: &str,
    stroke: bool,
    value: &str,
) -> Result<Option<Command>, String> {
    let nid = doc
        .find_by_sid(sid)
        .ok_or_else(|| "对象不存在".to_string())?;
    let n = doc.nodes.get(nid).ok_or_else(|| "对象不存在".to_string())?;
    if appearance::target_of(&n.kind) == AppearanceTarget::Frozen {
        return Err("冻结对象(原样片段)不可改色".into());
    }
    let attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let model = appearance::decode_model(&n.kind, &n.style, &attrs);

    if stroke {
        if let Some((i, s)) = model.items.iter().enumerate().find_map(|(i, it)| match it {
            AppearanceItem::Stroke(s) => Some((i, s)),
            _ => None,
        }) {
            let mut spec = s.spec.clone();
            spec.color = Some(value.to_string());
            return appearance::set_stroke_spec_cmd(doc, sid, i, spec);
        }
        let prop = stroke_prop(&n.kind);
        return Ok(Some(Command::SetStyle {
            sid: sid.to_string(),
            new: style_set_or_remove(n.style.clone(), prop, Some(value)),
            old: None,
        }));
    }
    if let Some(i) = model
        .items
        .iter()
        .position(|it| matches!(it, AppearanceItem::Fill(_)))
    {
        return appearance::set_fill_body_cmd(
            doc,
            sid,
            i,
            FillBody::Solid {
                value: value.to_string(),
            },
        );
    }
    let prop = fill_prop(&n.kind);
    Ok(Some(Command::SetStyle {
        sid: sid.to_string(),
        new: style_set_or_remove(n.style.clone(), prop, Some(value)),
        old: None,
    }))
}

/// 交换填充与描边色(Shift+X):任一缺失 → `Err` 说明,不静默。
pub fn swap_cmd(doc: &Document, sid: &str) -> Result<Option<Command>, String> {
    let f =
        read_target_color(doc, sid, false).ok_or_else(|| "该对象没有填充色可交换".to_string())?;
    let s =
        read_target_color(doc, sid, true).ok_or_else(|| "该对象没有描边色可交换".to_string())?;
    let c1 = write_color_cmd(doc, sid, false, &s)?;
    // 第二条命令基于「填充已改」之后的文档状态会不同 —— 用 Compound 同批执行
    let c2 = write_color_cmd(doc, sid, true, &f)?;
    let mut cmds = Vec::new();
    cmds.extend(c1);
    cmds.extend(c2);
    Ok(Some(Command::Compound { cmds }))
}

// ─────────────────────── 2. 色板(全局色 = CSS 变量) ───────────────────────

/// 文档里的全局色令牌 `(名称, 值)`(名称不带 `--`)。
pub fn global_colors(doc: &Document) -> Vec<(String, String)> {
    doc.tokens
        .iter()
        .filter(|(n, _)| n.starts_with(GLOBAL_PREFIX))
        .cloned()
        .collect()
}

/// 新建全局色:取第一个空闲的 `vb-color-N`。
pub fn add_global_cmd(doc: &Document, value: &str) -> Command {
    let mut i = 1usize;
    let name = loop {
        let candidate = format!("{GLOBAL_PREFIX}{i}");
        if !doc.tokens.iter().any(|(n, _)| *n == candidate) {
            break candidate;
        }
        i += 1;
    };
    Command::SetToken {
        name,
        new: value.to_string(),
        old: None,
    }
}

/// 改全局色的值(**全站生效** —— 令牌写 `:root`)。
pub fn set_global_cmd(name: &str, value: &str) -> Command {
    Command::SetToken {
        name: name.to_string(),
        new: value.to_string(),
        old: None,
    }
}

/// 删除全局色(`SetToken` 空值语义 = 删令牌)。
pub fn delete_global_cmd(name: &str) -> Command {
    Command::SetToken {
        name: name.to_string(),
        new: String::new(),
        old: None,
    }
}

// ─────────────────────── 3. 颜色空间换算(纯函数) ───────────────────────

/// RGB(0..255)→ HSB(`h 0..360`, `s/v 0..1`)。
pub fn rgb_to_hsb(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let d = max - min;
    let h = if d.abs() < f64::EPSILON {
        0.0
    } else if (max - rf).abs() < f64::EPSILON {
        60.0 * (((gf - bf) / d) % 6.0)
    } else if (max - gf).abs() < f64::EPSILON {
        60.0 * ((bf - rf) / d + 2.0)
    } else {
        60.0 * ((rf - gf) / d + 4.0)
    };
    let s = if max.abs() < f64::EPSILON {
        0.0
    } else {
        d / max
    };
    (h.rem_euclid(360.0), s, max)
}

/// HSB → RGB(0..255)。
pub fn hsb_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let q = |x: f64| ((x + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (q(r1), q(g1), q(b1))
}

/// RGB → CMYK(0..1)。
pub fn rgb_to_cmyk(r: u8, g: u8, b: u8) -> (f64, f64, f64, f64) {
    let (rf, gf, bf) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let k = 1.0 - rf.max(gf).max(bf);
    if k >= 1.0 - f64::EPSILON {
        return (0.0, 0.0, 0.0, 1.0);
    }
    let f = |x: f64| (1.0 - x - k) / (1.0 - k);
    (f(rf), f(gf), f(bf), k)
}

/// CMYK → RGB。CMYK 是**印刷**空间,网页只有 sRGB —— 面板按该口径如实标注。
pub fn cmyk_to_rgb(c: f64, m: f64, y: f64, k: f64) -> (u8, u8, u8) {
    let c = c.clamp(0.0, 1.0);
    let m = m.clamp(0.0, 1.0);
    let y = y.clamp(0.0, 1.0);
    let k = k.clamp(0.0, 1.0);
    let q = |v: f64| ((1.0 - v) * (1.0 - k) * 255.0).round().clamp(0.0, 255.0) as u8;
    (q(c), q(m), q(y))
}

/// 解析 CSS 颜色(含 `var(--x)` 一层展开)→ RGBA。
pub fn resolve(v: &str, tokens: &[(String, String)]) -> Option<Rgba> {
    vb_ui::gradient::resolve_color(v, tokens)
}

fn hex_of(c: Rgba) -> String {
    c.to_shortest_hex()
}

// ─────────────────────── 4. 面板 ───────────────────────

impl VellumApp {
    pub(crate) fn show_color_panel(&mut self, ui: &mut egui::Ui) {
        if !self.color_panel_open {
            return;
        }
        let mut open = true;
        egui::Window::new("颜色")
            .open(&mut open)
            .collapsible(false)
            .default_width(300.0)
            .show(ui.ctx(), |ui| self.color_panel_body(ui));
        self.color_panel_open = open;
    }

    /// 颜色面板/快捷键共用的目标色写回(离散操作)。
    pub(crate) fn color_apply(&mut self, res: Result<Option<Command>, String>) {
        match res {
            Ok(Some(c)) => {
                self.undo.merging_enabled = false;
                self.exec(c);
                self.undo.merging_enabled = true;
            }
            Ok(None) => {}
            Err(msg) => self.toast_warn(msg),
        }
    }

    /// `X`:切换填充/描边为当前作用目标。
    pub(crate) fn color_toggle_target(&mut self) {
        self.color_target_stroke = !self.color_target_stroke;
        self.say(if self.color_target_stroke {
            "颜色面板:作用于描边(X 切回填充)"
        } else {
            "颜色面板:作用于填充(X 切到描边)"
        });
        self.color_panel_open = true;
    }

    /// `Shift+X`:交换填充与描边色。
    pub(crate) fn color_swap(&mut self) {
        let Some(sid) = self.selection.last().cloned() else {
            self.toast_warn("未选中对象");
            return;
        };
        let r = swap_cmd(&self.doc, &sid);
        self.color_apply(r);
    }

    /// `D`:恢复默认填充白 / 描边黑。
    pub(crate) fn color_default(&mut self) {
        let Some(sid) = self.selection.last().cloned() else {
            self.toast_warn("未选中对象");
            return;
        };
        let stroke = self.color_target_stroke;
        let v = if stroke { DEFAULT_STROKE } else { DEFAULT_FILL };
        let r = write_color_cmd(&self.doc, &sid, stroke, v);
        self.color_apply(r);
        self.color_panel_open = true;
    }

    fn color_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(sid) = self.selection.last().cloned() else {
            ui.label(caption(
                ui,
                "未选中对象 —— 选中后调整填充/描边色与全局色板。",
            ));
            self.color_swatch_section(ui);
            return;
        };
        let tokens = self.doc.tokens.clone();
        let stroke = self.color_target_stroke;
        let cur = read_target_color(&self.doc, &sid, stroke);

        // ── 目标切换(填充/描边)──
        ui.horizontal(|ui| {
            if ui
                .selectable_label(!stroke, "填充")
                .on_hover_text("X 切换目标")
                .clicked()
            {
                self.color_target_stroke = false;
            }
            if ui
                .selectable_label(stroke, "描边")
                .on_hover_text("X 切换目标")
                .clicked()
            {
                self.color_target_stroke = true;
            }
            if ui.button("⇄ 交换").on_hover_text("Shift+X").clicked() {
                self.color_swap();
                return;
            }
            if ui
                .button("默认")
                .on_hover_text("D:填充白 / 描边黑")
                .clicked()
            {
                self.color_default();
            }
        });

        let Some(text) = cur else {
            ui.label(caption(
                ui,
                if stroke {
                    "该对象没有描边色 —— 可在描边面板(^F10)添加描边。"
                } else {
                    "该对象没有填充色 —— 可在外观面板(⇧F6)添加填充。"
                },
            ));
            self.color_swatch_section(ui);
            return;
        };
        let rgba = resolve(&text, &tokens);

        if let Some(c) = rgba {
            let mut col = egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a);
            let cf = ColorField::new(if stroke { "描边色" } else { "填充色" }, &mut col)
                .doc_tokens(&tokens)
                .ui(ui);
            if cf.changed || cf.var_picked.is_some() {
                let v = match cf.var_picked {
                    Some(n) => format!("var(--{n})"),
                    None => hex_of(Rgba::new(col.r(), col.g(), col.b(), col.a())),
                };
                let r = write_color_cmd(&self.doc, &sid, stroke, &v);
                self.color_apply(r);
            }

            // ── 页签:HSB / RGB / CMYK / CSS 变量 ──
            ui.horizontal(|ui| {
                for (i, name) in ["HSB", "RGB", "CMYK", "CSS 变量"].iter().enumerate() {
                    if ui.selectable_label(self.color_tab == i, *name).clicked() {
                        self.color_tab = i;
                    }
                }
            });
            let mut edited: Option<String> = None;
            match self.color_tab {
                0 => {
                    let (mut h, mut s, mut b) = rgb_to_hsb(col.r(), col.g(), col.b());
                    let mut ch = false;
                    ch |= NumField::new("H", &mut h)
                        .unit("°")
                        .speed(1.0)
                        .step(1.0)
                        .range(0.0, 360.0)
                        .ui(ui)
                        .changed;
                    ch |= NumField::new("S", &mut s)
                        .unit("%")
                        .speed(0.01)
                        .step(0.01)
                        .range(0.0, 1.0)
                        .ui(ui)
                        .changed;
                    ch |= NumField::new("B", &mut b)
                        .unit("%")
                        .speed(0.01)
                        .step(0.01)
                        .range(0.0, 1.0)
                        .ui(ui)
                        .changed;
                    if ch {
                        let (r, g, bb) = hsb_to_rgb(h, s, b);
                        edited = Some(hex_of(Rgba::new(r, g, bb, col.a())));
                    }
                }
                1 => {
                    let (mut r, mut g, mut b) = (col.r() as f64, col.g() as f64, col.b() as f64);
                    let mut ch = false;
                    for (lbl, v) in [("R", &mut r), ("G", &mut g), ("B", &mut b)] {
                        ch |= NumField::new(lbl, v)
                            .speed(1.0)
                            .step(1.0)
                            .range(0.0, 255.0)
                            .ui(ui)
                            .changed;
                    }
                    if ch {
                        edited = Some(hex_of(Rgba::new(r as u8, g as u8, b as u8, col.a())));
                    }
                }
                2 => {
                    let (mut c_, mut m_, mut y_, mut k_) = rgb_to_cmyk(col.r(), col.g(), col.b());
                    let mut ch = false;
                    for (lbl, v) in [
                        ("C", &mut c_),
                        ("M", &mut m_),
                        ("Y", &mut y_),
                        ("K", &mut k_),
                    ] {
                        ch |= NumField::new(lbl, v)
                            .unit("%")
                            .speed(0.01)
                            .step(0.01)
                            .range(0.0, 1.0)
                            .ui(ui)
                            .changed;
                    }
                    if ch {
                        let (r, g, b) = cmyk_to_rgb(c_, m_, y_, k_);
                        edited = Some(hex_of(Rgba::new(r, g, b, col.a())));
                    }
                    ui.label(caption(
                        ui,
                        "CMYK 仅作输入换算(CSS 只有 sRGB);显示为近似值。",
                    ));
                }
                _ => {
                    let g = global_colors(&self.doc);
                    if g.is_empty() {
                        ui.label(caption(ui, "还没有全局色 —— 在下方色板里新建。"));
                    }
                    ui.horizontal_wrapped(|ui| {
                        for (name, value) in &g {
                            let c = resolve(value, &tokens);
                            let col32 = c
                                .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                                .unwrap_or_default();
                            if swatch(ui, col32, true).clicked() {
                                edited = Some(format!("var(--{name})"));
                            }
                        }
                    });
                    ui.label(caption(
                        ui,
                        "点全局色即写入 var(--name):改令牌值全站生效(令牌 Tab F4)。",
                    ));
                }
            }
            if let Some(v) = edited {
                let r = write_color_cmd(&self.doc, &sid, stroke, &v);
                self.color_apply(r);
            }
        } else {
            ui.label(caption(
                ui,
                &format!("当前颜色 `{text}` 不可解析(可能是变量或复杂函数),已在色板区展示。"),
            ));
        }

        self.color_swatch_section(ui);
    }

    /// 色板区(05-5-2):新建 / 应用 / 改值 / 删除全局色。
    fn color_swatch_section(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        let tokens = self.doc.tokens.clone();
        ui.horizontal(|ui| {
            ui.label("色板(全局色)");
            if ui
                .button("＋ 新建")
                .on_hover_text("把当前填充色登记为全局色(--vb-color-N)")
                .clicked()
            {
                let v = self
                    .selection
                    .last()
                    .cloned()
                    .and_then(|sid| read_target_color(&self.doc, &sid, self.color_target_stroke))
                    .unwrap_or_else(|| DEFAULT_FILL.to_string());
                let cmd = add_global_cmd(&self.doc, &v);
                self.color_apply(Ok(Some(cmd)));
            }
        });
        let globals = global_colors(&self.doc);
        if globals.is_empty() {
            ui.label(caption(
                ui,
                "全局色 = CSS 变量(--vb-color-N),存于文档令牌,改动全站生效。",
            ));
        }
        let mut apply: Option<String> = None;
        let mut del: Option<String> = None;
        let mut edit: Option<(String, String)> = None;
        ui.horizontal_wrapped(|ui| {
            for (name, value) in &globals {
                let c = resolve(value, &tokens)
                    .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a));
                let resp = swatch(ui, c.unwrap_or_default(), true);
                if resp.clicked() {
                    apply = Some(format!("var(--{name})"));
                }
                resp.context_menu(|ui| {
                    ui.label(caption(ui, &format!("--{name}: {value}")));
                    if ui.button("删除全局色").clicked() {
                        del = Some(name.clone());
                        ui.close();
                    }
                });
            }
        });
        // 编辑选中全局色的值(全站生效)
        if let Some((name, value)) = globals
            .iter()
            .find(|(n, _)| {
                self.selection
                    .last()
                    .and_then(|sid| read_target_color(&self.doc, sid, self.color_target_stroke))
                    .map(|v| v == format!("var(--{n})"))
                    .unwrap_or(false)
            })
            .cloned()
        {
            let mut col = resolve(&value, &tokens)
                .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                .unwrap_or_default();
            let cf = ColorField::new(format!("--{name}").as_str(), &mut col)
                .doc_tokens(&tokens)
                .ui(ui);
            if cf.changed {
                edit = Some((
                    name.clone(),
                    hex_of(Rgba::new(col.r(), col.g(), col.b(), col.a())),
                ));
            }
        }
        let _ = icons::Name::PanelTokens;
        if let Some(v) = apply {
            if let Some(sid) = self.selection.last().cloned() {
                let stroke = self.color_target_stroke;
                let r = write_color_cmd(&self.doc, &sid, stroke, &v);
                self.color_apply(r);
            } else {
                self.toast_warn("未选中对象 —— 先选中对象再应用全局色");
            }
        }
        if let Some((name, v)) = edit {
            let cmd = set_global_cmd(&name, &v);
            self.color_apply(Ok(Some(cmd)));
        }
        if let Some(name) = del {
            let cmd = delete_global_cmd(&name);
            self.color_apply(Ok(Some(cmd)));
        }
        ui.label(caption(
            ui,
            "从页面提取颜色:依赖 WPI color_profiler,当前未落地 —— 暂不提供按钮(计划 v2)。",
        ));
        let _ = fmt_num(0.0);
        let _ = NodeKind::Artboard;
    }
}

/// 一个色板格子(左下白三角 = 全局色,05-5-2)。
fn swatch(ui: &mut egui::Ui, color: egui::Color32, global: bool) -> egui::Response {
    let t = vb_ui::theme::tokens(ui.ctx());
    let size = egui::Vec2::splat(18.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    ui.painter()
        .rect_filled(rect, vb_ui::theme::radius::sm(), color);
    ui.painter().rect_stroke(
        rect,
        vb_ui::theme::radius::sm(),
        egui::Stroke::new(vb_ui::theme::stroke::HAIRLINE, t.border_strong),
        egui::StrokeKind::Inside,
    );
    if global {
        // 左下白三角(按 design/03 §5.5 的全局色标记)
        let p = rect.left_bottom();
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                egui::pos2(p.x + 1.0, p.y - 1.0),
                egui::pos2(p.x + 8.0, p.y - 1.0),
                egui::pos2(p.x + 1.0, p.y - 8.0),
            ],
            t.text,
            egui::Stroke::new(0.0, egui::Color32::TRANSPARENT),
        ));
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsb_roundtrip_is_stable_for_primaries() {
        for (r, g, b) in [
            (255u8, 0u8, 0u8),
            (0, 255, 0),
            (0, 0, 255),
            (128, 128, 128),
            (12, 34, 56),
            (255, 255, 255),
            (0, 0, 0),
        ] {
            let (h, s, v) = rgb_to_hsb(r, g, b);
            let (r2, g2, b2) = hsb_to_rgb(h, s, v);
            assert!(
                (r as i32 - r2 as i32).abs() <= 1
                    && (g as i32 - g2 as i32).abs() <= 1
                    && (b as i32 - b2 as i32).abs() <= 1,
                "({r},{g},{b}) → ({r2},{g2},{b2})"
            );
        }
    }

    #[test]
    fn cmyk_roundtrip_is_stable() {
        for (r, g, b) in [
            (255u8, 0u8, 0u8),
            (0, 128, 255),
            (200, 30, 90),
            (255, 255, 255),
        ] {
            let (c, m, y, k) = rgb_to_cmyk(r, g, b);
            let (r2, g2, b2) = cmyk_to_rgb(c, m, y, k);
            assert!(
                (r as i32 - r2 as i32).abs() <= 1
                    && (g as i32 - g2 as i32).abs() <= 1
                    && (b as i32 - b2 as i32).abs() <= 1,
                "({r},{g},{b}) → ({r2},{g2},{b2})"
            );
        }
    }

    #[test]
    fn black_and_white_are_exact() {
        assert_eq!(hsb_to_rgb(0.0, 0.0, 0.0), (0, 0, 0));
        assert_eq!(hsb_to_rgb(0.0, 0.0, 1.0), (255, 255, 255));
    }

    #[test]
    fn add_global_picks_first_free_name() {
        let mut doc = Document::new_default();
        // vb-token-ok: 测试夹具
        doc.tokens.push(("vb-color-1".into(), "#111".into()));
        // vb-token-ok: 测试夹具
        let cmd = add_global_cmd(&doc, "#222");
        match cmd {
            Command::SetToken { name, new, .. } => {
                assert_eq!(name, "vb-color-2");
                // vb-token-ok: 测试夹具
                assert_eq!(new, "#222");
            }
            _ => panic!("应为 SetToken"),
        }
    }
}
