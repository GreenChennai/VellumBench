//! 05-5 响应式断点(台账 09-F):断点清单、覆盖样式读写与命令构建。
//!
//! **落盘纪律(HTML 是文档格式)**:
//! - 断点**清单**存 `index.html` 的 `<meta name="vb-breakpoints" content="375,750">`
//!   (与 09-M 文档设置的 vb-grid / vb-guides meta 同款:随文件走、
//!   往返幂等、被快照与三方对比覆盖;不用旁路 JSON)。
//! - 断点**覆盖样式**存 `Document::media_rules`(sid 寻址),canonical
//!   序列化为 `@media (max-width: Npx) { .cls { … } }` 块(节点规则之后,
//!   级联覆盖语义;见 export::render_css)。
//! - **可用断点** = meta 清单 ∪ media_rules 宽度 ∪ 冻结块里的 max-width
//!   查询(外部文档自带的断点也能在切换器里预览,只是不可结构化编辑)。
//!
//! **编辑闭环(诚实最小)**:断点态下仅 宽/高/位置/显隐/字号 五类可改,
//! 走 `Command::SetMediaStyle`(可撤销);其余属性面板只读并提示去向。
//! 画布预览不套用覆盖样式(以「视图 → 浏览器校对」对拍为准),状态栏
//! 常驻提示,不假渲染。

use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::model::Document;

use super::doc_settings::{meta_get, meta_set};

/// 断点 meta 的 name(存在 `head_extra`)。
pub const META_NAME: &str = "vb-breakpoints";

/// 解析 meta 值(逗号分隔正整数 px;坏 token 跳过)。
pub fn parse_meta(content: &str) -> Vec<u32> {
    let mut out: Vec<u32> = content
        .split(',')
        .filter_map(|t| t.trim().parse::<u32>().ok())
        .filter(|&w| w > 0)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// 编码 meta 值。
pub fn encode_meta(widths: &[u32]) -> String {
    let mut ws: Vec<u32> = widths.to_vec();
    ws.sort_unstable();
    ws.dedup();
    ws.iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// 读 meta 断点清单(无 meta = 空)。
pub fn meta_breakpoints(doc: &Document) -> Vec<u32> {
    meta_get(doc, META_NAME)
        .map(|c| parse_meta(&c))
        .unwrap_or_default()
}

/// 写 meta 断点清单(空 = 删除 meta)。直改文档(文档级设置无命令变体,
/// 与 vb-grid 同口径;调用方负责 `rev += 1` 标脏)。
pub fn set_meta_breakpoints(doc: &mut Document, widths: &[u32]) {
    let v = encode_meta(widths);
    if v.is_empty() {
        meta_set(doc, META_NAME, None);
    } else {
        meta_set(doc, META_NAME, Some(&v));
    }
}

/// 扫描冻结块(raw_css)里的 `(max-width: Npx)` 查询宽度
/// (外部文档自带断点的可用性来源;不解析内容)。
fn raw_media_widths(doc: &Document) -> Vec<u32> {
    let mut out = Vec::new();
    for raw in &doc.raw_css {
        let Some(rest) = raw.strip_prefix("@media") else {
            continue;
        };
        // 查询段 = "@media" 之后到第一个 '{'
        let Some(open) = rest.find('{') else { continue };
        let query = rest[..open].trim();
        // 只认纯 max-width 单条件(与导入器 parse_max_width_query 同口径)
        let Some(inner) = query.strip_prefix('(').and_then(|s| s.strip_suffix(')')) else {
            continue;
        };
        let Some((prop, val)) = inner.split_once(':') else {
            continue;
        };
        if !prop.trim().eq_ignore_ascii_case("max-width") {
            continue;
        }
        if let Ok(w) = val
            .trim()
            .strip_suffix("px")
            .unwrap_or("")
            .trim()
            .parse::<u32>()
        {
            out.push(w);
        }
    }
    out
}

/// 可用断点(升序去重):meta ∪ media_rules ∪ 冻结块查询。
pub fn available(doc: &Document) -> Vec<u32> {
    let mut out = meta_breakpoints(doc);
    out.extend(doc.media_rules.iter().map(|r| r.max_width));
    out.extend(raw_media_widths(doc));
    out.sort_unstable();
    out.dedup();
    out
}

/// 取节点在断点下的覆盖声明(只读视图用)。
pub fn override_decls<'a>(doc: &'a Document, sid: &str, max_width: u32) -> Option<&'a [Decl]> {
    doc.media_rules
        .iter()
        .find(|r| r.sid == sid && r.max_width == max_width)
        .map(|r| r.decls.as_slice())
}

/// 取伪类覆盖声明。
pub fn pseudo_decls<'a>(doc: &'a Document, sid: &str, pseudo: &str) -> Option<&'a [Decl]> {
    doc.pseudo_rules
        .iter()
        .find(|r| r.sid == sid && r.pseudo == pseudo)
        .map(|r| r.decls.as_slice())
}

/// 合成显示样式:基样式 + 覆盖声明(同属性后者胜)。属性面板断点态/
/// hover 态的取值来源(覆盖了什么就显示什么)。
pub fn merged_style(doc: &Document, sid: &str, max_width: u32) -> Vec<Decl> {
    let Some(id) = doc.find_by_sid(sid) else {
        return Vec::new();
    };
    let mut out = doc
        .nodes
        .get(id)
        .map(|n| n.style.clone())
        .unwrap_or_default();
    if let Some(ov) = override_decls(doc, sid, max_width) {
        for d in ov {
            if let Some(e) = out.iter_mut().find(|e| e.prop == d.prop) {
                e.value = d.value.clone();
            } else {
                out.push(d.clone());
            }
        }
    }
    out
}

/// 断点覆盖写命令(单目标;`value = None` → 删除该属性,全空则删条目)。
pub fn media_style_cmd(
    doc: &Document,
    sid: &str,
    max_width: u32,
    prop: &str,
    value: Option<&str>,
) -> Command {
    let mut decls: Vec<Decl> = override_decls(doc, sid, max_width)
        .map(|s| s.to_vec())
        .unwrap_or_default();
    match value {
        Some(v) => {
            if let Some(d) = decls.iter_mut().find(|d| d.prop == prop) {
                d.value = v.to_string();
            } else {
                decls.push(Decl {
                    prop: prop.into(),
                    value: v.into(),
                    important: false,
                });
            }
        }
        None => decls.retain(|d| d.prop != prop),
    }
    Command::SetMediaStyle {
        sid: sid.into(),
        max_width,
        new: decls,
        old: None,
    }
}

/// hover 覆盖写命令(同上)。
pub fn pseudo_style_cmd(doc: &Document, sid: &str, prop: &str, value: Option<&str>) -> Command {
    let mut decls: Vec<Decl> = pseudo_decls(doc, sid, "hover")
        .map(|s| s.to_vec())
        .unwrap_or_default();
    match value {
        Some(v) => {
            if let Some(d) = decls.iter_mut().find(|d| d.prop == prop) {
                d.value = v.to_string();
            } else {
                decls.push(Decl {
                    prop: prop.into(),
                    value: v.into(),
                    important: false,
                });
            }
        }
        None => decls.retain(|d| d.prop != prop),
    }
    Command::SetPseudoStyle {
        sid: sid.into(),
        pseudo: "hover".into(),
        new: decls,
        old: None,
    }
}

impl super::VellumApp {
    /// 05-5:断点循环切换(`view.breakpoint_cycle`;状态栏切换器同语义):
    /// 默认 → 最小断点 → … → 最大断点 → 默认。
    pub(crate) fn cycle_breakpoint(&mut self) {
        let list = available(&self.doc);
        if list.is_empty() {
            self.say("文档没有断点:可在「文件 → 文档设置」添加(vb-breakpoints)");
            return;
        }
        self.active_breakpoint = match self.active_breakpoint {
            None => Some(list[0]),
            Some(cur) => list.iter().find(|&&w| w > cur).copied(),
        };
        match self.active_breakpoint {
            Some(w) => self.say(format!(
                "断点预览:画布宽度 → {w}px(覆盖样式以浏览器校对为准)"
            )),
            None => self.say("断点预览:回到默认画布"),
        }
    }

    /// 05-5:hover 编辑态切换(`style.state_toggle`);断点态优先,
    /// 两者叠加(媒体查询内伪类)不在本轮闭环,切断点时自动回到正常态。
    pub(crate) fn toggle_style_state(&mut self) {
        self.style_state = 1 - self.style_state;
        self.say(match self.style_state {
            1 => "属性面板:hover 态编辑(落 selector:hover 规则)",
            _ => "属性面板:正常态编辑",
        });
    }

    /// 属性面板「断点覆盖编辑」段(断点态下整面板替换,诚实最小闭环:
    /// 仅 宽/高/位置/显隐(/字号) 可改,其余属性给出只读去向)。
    pub(crate) fn breakpoint_edit_section(&mut self, ui: &mut egui::Ui, sid: &str, bp: u32) {
        let t = crate::i18n::t;
        ui.heading(format!("{} · {}px", t("bp.edit-banner"), bp));
        ui.separator();

        let Some(nid) = self.doc.find_by_sid(sid) else {
            return;
        };
        let geom = self.doc.nodes.get(nid).unwrap().geom;
        let is_text = matches!(
            self.doc.nodes.get(nid).unwrap().kind,
            vb_doc::model::NodeKind::Text { .. }
        );
        let st = merged_style(&self.doc, sid, bp);
        let get_f = |prop: &str, fallback: f64| -> f64 {
            st.iter()
                .find(|d| d.prop == prop)
                .and_then(|d| d.value.trim_end_matches("px").parse::<f64>().ok())
                .unwrap_or(fallback)
        };
        let get_s = |prop: &str| -> String {
            st.iter()
                .find(|d| d.prop == prop)
                .map(|d| d.value.clone())
                .unwrap_or_default()
        };

        // 宽 / 高(覆盖声明;未覆盖时显示当前几何值)
        let mut w = get_f("width", geom.w);
        let r = vb_ui::components::NumField::new("W", &mut w)
            .speed(1.0)
            .label_width(20.0)
            .width(56.0)
            .range(0.0, 100000.0)
            .ui(ui);
        if r.changed {
            let v = format!("{}px", vb_common::units::fmt_num(w));
            let cmd = media_style_cmd(&self.doc, sid, bp, "width", Some(&v));
            self.num_commit(r, Some(cmd));
        }
        let mut h = get_f("height", geom.h);
        let r = vb_ui::components::NumField::new("H", &mut h)
            .speed(1.0)
            .label_width(20.0)
            .width(56.0)
            .range(0.0, 100000.0)
            .ui(ui);
        if r.changed {
            let v = format!("{}px", vb_common::units::fmt_num(h));
            let cmd = media_style_cmd(&self.doc, sid, bp, "height", Some(&v));
            self.num_commit(r, Some(cmd));
        }
        // 位置(left / top 覆盖)
        let mut x = get_f("left", geom.x);
        let r = vb_ui::components::NumField::new("X", &mut x)
            .speed(1.0)
            .label_width(20.0)
            .width(56.0)
            .ui(ui);
        if r.changed {
            let v = format!("{}px", vb_common::units::fmt_num(x));
            let cmd = media_style_cmd(&self.doc, sid, bp, "left", Some(&v));
            self.num_commit(r, Some(cmd));
        }
        let mut y = get_f("top", geom.y);
        let r = vb_ui::components::NumField::new("Y", &mut y)
            .speed(1.0)
            .label_width(20.0)
            .width(56.0)
            .ui(ui);
        if r.changed {
            let v = format!("{}px", vb_common::units::fmt_num(y));
            let cmd = media_style_cmd(&self.doc, sid, bp, "top", Some(&v));
            self.num_commit(r, Some(cmd));
        }
        // 显隐(display: none)
        ui.horizontal(|ui| {
            ui.label("显示");
            let hidden = get_s("display") == "none";
            if ui.selectable_label(!hidden, "显示").clicked() && hidden {
                let cmd = media_style_cmd(&self.doc, sid, bp, "display", None);
                self.exec(cmd);
            }
            if ui.selectable_label(hidden, "隐藏").clicked() && !hidden {
                let cmd = media_style_cmd(&self.doc, sid, bp, "display", Some("none"));
                self.exec(cmd);
            }
        });
        // 字号(仅文本对象)
        if is_text {
            let mut fs = get_f("font-size", 24.0);
            let r = vb_ui::components::NumField::new("字号", &mut fs)
                .speed(1.0)
                .step(1.0)
                .range(1.0, 500.0)
                .unit("px")
                .label_width(44.0)
                .width(56.0)
                .ui(ui);
            if r.changed {
                let v = format!("{}px", vb_common::units::fmt_num(fs));
                let cmd = media_style_cmd(&self.doc, sid, bp, "font-size", Some(&v));
                self.num_commit(r, Some(cmd));
            }
        }
        // 已有覆盖一览(可见即可编辑;为空说明尚未覆盖任何属性)
        let existing: Vec<(String, String)> = override_decls(&self.doc, sid, bp)
            .map(|ds| {
                ds.iter()
                    .map(|d| (d.prop.clone(), d.value.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if !existing.is_empty() {
            ui.add_space(4.0);
            ui.label("已覆盖:");
            for (prop, value) in &existing {
                ui.horizontal(|ui| {
                    ui.label(format!("{prop}: {value};"));
                    if ui.small_button("清除").clicked() {
                        let cmd = media_style_cmd(&self.doc, sid, bp, prop, None);
                        self.exec(cmd);
                    }
                });
            }
        }
        ui.add_space(4.0);
        ui.separator();
        ui.label(vb_ui::components::caption(ui, &t("bp.edit-note")));
        ui.label(vb_ui::components::caption(ui, &t("bp.unsupported")));
        ui.label(vb_ui::components::caption(ui, &t("bp.status-hint")));
        ui.separator();
        // 返回默认画布(与状态栏切换器同效)
        if ui.button(format!("← {}", t("bp.default"))).clicked() {
            self.active_breakpoint = None;
        }
    }

    /// 属性面板「hover 态编辑」段(伪类最小闭环:填充/不透明/显隐,
    /// 文本对象加 字号/字色;改动落 `selector:hover` 规则,可撤销)。
    pub(crate) fn hover_edit_section(&mut self, ui: &mut egui::Ui, sid: &str) {
        let t = crate::i18n::t;
        ui.heading(format!("{} · {}", t("state.label"), t("state.hover")));
        ui.separator();
        let Some(nid) = self.doc.find_by_sid(sid) else {
            return;
        };
        let is_text = matches!(
            self.doc.nodes.get(nid).unwrap().kind,
            vb_doc::model::NodeKind::Text { .. }
        );
        let decls: Vec<vb_css::Decl> = pseudo_decls(&self.doc, sid, "hover")
            .map(|s| s.to_vec())
            .unwrap_or_default();
        let get_f = |prop: &str, fallback: f64| -> f64 {
            decls
                .iter()
                .find(|d| d.prop == prop)
                .and_then(|d| d.value.trim_end_matches("px").parse::<f64>().ok())
                .unwrap_or(fallback)
        };
        // 填充
        let cur = decls
            .iter()
            .find(|d| d.prop == "background-color")
            .and_then(|d| vb_common::color::parse_color(&d.value));
        let mut col = cur
            .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
            .unwrap_or(egui::Color32::WHITE);
        let r = vb_ui::components::ColorField::new("填充", &mut col).ui(ui);
        if r.changed {
            let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
            let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
            let cmd = pseudo_style_cmd(&self.doc, sid, "background-color", Some(&hex));
            self.exec(cmd);
        }
        // 不透明度
        let mut op = get_f("opacity", 1.0);
        let r = vb_ui::components::NumField::new("不透明", &mut op)
            .speed(0.01)
            .step(0.01)
            .range(0.0, 1.0)
            .label_width(44.0)
            .width(56.0)
            .ui(ui);
        if r.changed {
            let v = format!("{op:.2}");
            let cmd = pseudo_style_cmd(&self.doc, sid, "opacity", Some(&v));
            self.num_commit(r, Some(cmd));
        }
        // 文本对象:字号 / 字色
        if is_text {
            let mut fs = get_f("font-size", 24.0);
            let r = vb_ui::components::NumField::new("字号", &mut fs)
                .speed(1.0)
                .step(1.0)
                .range(1.0, 500.0)
                .unit("px")
                .label_width(44.0)
                .width(56.0)
                .ui(ui);
            if r.changed {
                let v = format!("{}px", vb_common::units::fmt_num(fs));
                let cmd = pseudo_style_cmd(&self.doc, sid, "font-size", Some(&v));
                self.num_commit(r, Some(cmd));
            }
            let cur_c = decls
                .iter()
                .find(|d| d.prop == "color")
                .and_then(|d| vb_common::color::parse_color(&d.value));
            let mut tc = cur_c
                .map(|c| egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                .unwrap_or(egui::Color32::BLACK);
            let r = vb_ui::components::ColorField::new("字色", &mut tc).ui(ui);
            if r.changed {
                let [cr, cg, cb, ca] = tc.to_srgba_unmultiplied();
                let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
                let cmd = pseudo_style_cmd(&self.doc, sid, "color", Some(&hex));
                self.exec(cmd);
            }
        }
        // 已有 hover 覆盖一览
        let existing: Vec<(String, String)> = decls
            .iter()
            .map(|d| (d.prop.clone(), d.value.clone()))
            .collect();
        if !existing.is_empty() {
            ui.add_space(4.0);
            ui.label("已覆盖:");
            for (prop, value) in &existing {
                ui.horizontal(|ui| {
                    ui.label(format!("{prop}: {value};"));
                    if ui.small_button("清除").clicked() {
                        let cmd = pseudo_style_cmd(&self.doc, sid, prop, None);
                        self.exec(cmd);
                    }
                });
            }
        }
        ui.add_space(4.0);
        ui.separator();
        ui.label(vb_ui::components::caption(ui, &t("state.hover-note")));
        ui.separator();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;

    /// meta 断点清单的编解码往返与容错解析。
    #[test]
    fn meta_encode_parse_roundtrip() {
        assert_eq!(encode_meta(&[750, 375, 1440]), "375,750,1440");
        assert_eq!(parse_meta("375, 750,1080"), vec![375, 750, 1080]);
        // 去重 + 升序 + 坏 token 跳过
        assert_eq!(parse_meta("750,375,750,x,0"), vec![375, 750]);
        assert_eq!(parse_meta(""), Vec::<u32>::new());
    }

    /// meta 写读往返(head_extra 通道,与 vb-grid 同款)。
    #[test]
    fn meta_set_get_roundtrip() {
        let mut doc = Document::new_default();
        assert!(meta_breakpoints(&doc).is_empty());
        set_meta_breakpoints(&mut doc, &[1440, 375, 750]);
        assert_eq!(meta_breakpoints(&doc), vec![375, 750, 1440]);
        assert_eq!(
            doc.head_extra.last().unwrap(),
            "<meta name=\"vb-breakpoints\" content=\"375,750,1440\">"
        );
        set_meta_breakpoints(&mut doc, &[]);
        assert!(meta_get(&doc, META_NAME).is_none(), "空清单删 meta");
    }

    /// 可用断点 = meta ∪ media_rules ∪ 冻结块查询(升序去重)。
    #[test]
    fn available_unions_meta_media_and_raw() {
        let mut doc = Document::new_default();
        set_meta_breakpoints(&mut doc, &[375]);
        doc.media_rules.push(vb_doc::model::MediaRule {
            max_width: 1080,
            sid: "zz0001".into(),
            decls: vec![],
        });
        doc.raw_css
            .push("@media (max-width: 750px) {\n  .x { width: 1px; }\n}".into());
        doc.raw_css
            .push("@media screen and (max-width: 640px) {\n  .x { width: 2px; }\n}".into());
        assert_eq!(
            available(&doc),
            vec![375, 750, 1080],
            "组合查询不算可用断点"
        );
    }

    /// merged_style:基样式 + 覆盖(同属性覆盖胜,新属性追加)。
    #[test]
    fn merged_style_overrides_base() {
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let sid = doc.alloc_sid();
        let mut n = vb_doc::model::Node::new(vb_doc::model::NodeKind::Box, "卡", sid.clone());
        n.style_set("width", "400px");
        n.style_set("background-color", "#3a86ff");
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(id);
        doc.media_rules.push(vb_doc::model::MediaRule {
            max_width: 375,
            sid: sid.as_str().to_string(),
            decls: vec![
                Decl {
                    prop: "width".into(),
                    value: "80%".into(),
                    important: false,
                },
                Decl {
                    prop: "display".into(),
                    value: "none".into(),
                    important: false,
                },
            ],
        });
        let st = merged_style(&doc, sid.as_str(), 375);
        let get = |p: &str| st.iter().find(|d| d.prop == p).map(|d| d.value.clone());
        assert_eq!(get("width").as_deref(), Some("80%"), "覆盖胜");
        assert_eq!(
            get("background-color").as_deref(),
            Some("#3a86ff"),
            "基样式保留"
        );
        assert_eq!(get("display").as_deref(), Some("none"), "新属性追加");
    }

    /// 命令层闭环:view.breakpoint_cycle 循环 + style.state_toggle,
    /// 以及 SetMediaStyle 落盘 → 导出 CSS 携带 canonical 断点块。
    #[test]
    fn cycle_and_state_toggle_end_to_end() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        assert!(app.active_breakpoint.is_none());
        // 无断点文档:诚实提示,不假切换
        app.run_command("view.breakpoint_cycle", false, false);
        assert!(app.active_breakpoint.is_none());
        assert!(app.status.contains("没有断点"));

        // 文档设置通道加断点(meta)
        super::set_meta_breakpoints(&mut app.doc, &[375, 1440]);
        app.run_command("view.breakpoint_cycle", false, false);
        assert_eq!(app.active_breakpoint, Some(375));
        app.run_command("view.breakpoint_cycle", false, false);
        assert_eq!(app.active_breakpoint, Some(1440));
        app.run_command("view.breakpoint_cycle", false, false);
        assert_eq!(app.active_breakpoint, None, "最大断点后回默认");

        // 断点态下写覆盖(与属性面板同一条命令路径)
        app.run_command("view.breakpoint_cycle", false, false);
        let ab = app.doc.artboards[0];
        let sid = app.doc.alloc_sid();
        let mut n = vb_doc::model::Node::new(vb_doc::model::NodeKind::Box, "卡", sid.clone());
        n.geom = vb_doc::model::Geom {
            x: 10.0,
            y: 10.0,
            w: 200.0,
            h: 100.0,
        };
        let id = app.doc.nodes.insert(n);
        app.doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        app.doc.nodes.get_mut(ab).unwrap().children.push(id);
        app.selection = vec![sid.as_str().to_string()];
        let cmd = media_style_cmd(&app.doc, sid.as_str(), 375, "display", Some("none"));
        app.exec(cmd);
        let css = vb_doc::export::render_project(&app.doc)
            .files
            .into_iter()
            .find(|(p, _)| p == "styles/main.css")
            .map(|(_, c)| c)
            .unwrap_or_default();
        assert!(
            css.contains("@media (max-width: 375px)") && css.contains("display: none;"),
            "导出 CSS 必须携带断点覆盖块"
        );

        // 状态切换
        app.run_command("style.state_toggle", false, false);
        assert_eq!(app.style_state, 1);
        app.run_command("style.state_toggle", false, false);
        assert_eq!(app.style_state, 0);
    }

    /// 断点/hover 写命令:设值、改值、删属性的声明演化(命令层可测)。
    #[test]
    fn style_cmds_evolve_decls() {
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let sid = doc.alloc_sid();
        let n = vb_doc::model::Node::new(vb_doc::model::NodeKind::Box, "卡", sid.clone());
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(id);

        let mut cmd = media_style_cmd(&doc, sid.as_str(), 375, "width", Some("80%"));
        cmd.apply(&mut doc).unwrap();
        let mut cmd = media_style_cmd(&doc, sid.as_str(), 375, "width", Some("100%"));
        cmd.apply(&mut doc).unwrap();
        assert_eq!(
            override_decls(&doc, sid.as_str(), 375).unwrap()[0].value,
            "100%",
            "同属性改值不重复建条"
        );
        let mut cmd = media_style_cmd(&doc, sid.as_str(), 375, "display", Some("none"));
        cmd.apply(&mut doc).unwrap();
        assert_eq!(override_decls(&doc, sid.as_str(), 375).unwrap().len(), 2);
        // 清掉最后一条属性 → 条目整体删除
        let mut cmd = media_style_cmd(&doc, sid.as_str(), 375, "width", None);
        cmd.apply(&mut doc).unwrap();
        let mut cmd = media_style_cmd(&doc, sid.as_str(), 375, "display", None);
        cmd.apply(&mut doc).unwrap();
        assert!(doc.media_rules.is_empty(), "空声明条目应被删除");

        let mut cmd = pseudo_style_cmd(&doc, sid.as_str(), "opacity", Some("0.8"));
        cmd.apply(&mut doc).unwrap();
        assert_eq!(pseudo_decls(&doc, sid.as_str(), "hover").unwrap().len(), 1);
    }
}
