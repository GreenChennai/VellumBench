//! 右侧面板 · 属性 Tab(S1-c 02-3:**七分组**重组)。
//!
//! 组序按 `design/03 §四`:变换 / 外观 / 布局 / 文本 / 交互 / 无障碍 /
//! 导出;每组 [`SectionHeader`] 可折叠(状态存 `VellumApp::prop_groups_open`)。
//! 顶部保留语义标签行(ASCII 规格里分组外的「标签」行);组后保留
//! 对齐 / 分布 / 路径查找器动作区(既有行为)。
//!
//! 写回纪律:**只放有命令支撑的控件** —— 全部经
//! [`control_panel`](crate::app::control_panel) 的共享构建器
//! (`geom_field_cmds` / `style_prop_cmds` / `attr_cmds` / `rotation_cmds` /
//! `rename_cmds`)构造文档命令,属性面板与控制面板同一条写回路径,
//! 门 2 文档状态级测试对同一构建器断言 Document/CSS 变化。
//! 暂无命令支撑的字段(倾斜/九宫格/描边/混合模式、字体族等)只以
//! caption 登记去向,**不放假控件**。
//!
//! 多选:控件取值显示主选中;提交经 `combine` 变 Compound 作用
//! **全部**选中对象(一条 undo)。

use egui::Color32;
use vb_doc::commands::Command;
use vb_doc::model::NodeKind;
use vb_ui::components::{caption, icon_button, ColorField, NumField, SectionHeader};
use vb_ui::icons::Name;

use crate::app::control_panel::{
    attr_cmds, combine, geom_field_cmds, rename_cmds, rotation_cmds, rotation_deg_of,
    style_prop_cmds, style_prop_remove_cmds, GeomAxis,
};
use crate::app::{set_style_prop, VellumApp};
use crate::shortcuts;

/// 属性面板七分组(组序 = design/03 §四;pub(crate) 供存在性测试)。
pub(crate) const PROP_GROUPS: [&str; 7] =
    ["变换", "外观", "布局", "文本", "交互", "无障碍", "导出"];

// 组下标(与 PROP_GROUPS 对齐)
const G_TRANSFORM: usize = 0;
const G_APPEARANCE: usize = 1;
const G_LAYOUT: usize = 2;
const G_TEXT: usize = 3;
const G_INTERACT: usize = 4;
const G_A11Y: usize = 5;
const G_EXPORT: usize = 6;

impl VellumApp {
    pub(crate) fn properties_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("属性");
        ui.separator();

        // --- 选中对象的属性(先取全量快照,避免借用冲突) ---
        let sid = self.selection.last().cloned();
        if let Some(sid) = sid {
            if let Some(nid) = self.doc.find_by_sid(&sid) {
                // ── 05-5:样式编辑状态(正常 / hover)与断点覆盖模式 ──
                // 断点态优先:整面板切换为「断点覆盖编辑」最小闭环
                // (诚实闭环:其余属性只读并给出去向,不放假控件)。
                if let Some(bp) = self.active_breakpoint {
                    self.breakpoint_edit_section(ui, &sid, bp);
                    return;
                }
                // 状态下拉(正常 / hover):hover 态整面板切换为伪类最小闭环
                {
                    let mut st_sel = self.style_state;
                    ui.horizontal(|ui| {
                        ui.label(crate::i18n::t("state.label"));
                        if ui
                            .selectable_label(st_sel == 0, crate::i18n::t("state.normal"))
                            .clicked()
                        {
                            st_sel = 0;
                        }
                        if ui
                            .selectable_label(st_sel == 1, crate::i18n::t("state.hover"))
                            .clicked()
                        {
                            st_sel = 1;
                        }
                    });
                    if st_sel != self.style_state {
                        self.toggle_style_state();
                    }
                    if self.style_state == 1 {
                        self.hover_edit_section(ui, &sid);
                        return;
                    }
                }
                let sids = self.selection.clone();
                let (mut hidden, mut locked) = {
                    let n = self.doc.nodes.get(nid).unwrap();
                    (n.hidden, n.locked)
                };
                let style = self.doc.nodes.get(nid).unwrap().style.clone();
                let attrs = self
                    .doc
                    .nodes
                    .get(nid)
                    .unwrap()
                    .attrs
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>();
                let name = self.doc.nodes.get(nid).unwrap().name.clone();
                let cur_tag = self.doc.nodes.get(nid).unwrap().tag.clone();
                let is_text =
                    matches!(self.doc.nodes.get(nid).unwrap().kind, NodeKind::Text { .. });
                let (ab_w, ab_h) = self.active_artboard_size();

                // ── 05-8 符号 / 组件(09-H):实例身份横幅 ──
                // 选中实例(或实例内部节点)时显示「组件实例:<名>」;
                // 「跳到主件」把选区切到主件原型根(定义区不在画布上,
                // 属性面板即主件编辑台),并提示同步语义。
                if let Some(root) = vb_doc::symbol::instance_root_of(&self.doc, nid) {
                    let inst = self.doc.nodes.get(root).unwrap();
                    let sym_name = inst
                        .attrs
                        .get(vb_doc::symbol::ATTR_SYMBOL)
                        .cloned()
                        .unwrap_or_default();
                    let overrides = inst
                        .attrs
                        .get(vb_doc::symbol::ATTR_OVERRIDES)
                        .cloned()
                        .unwrap_or_default();
                    let ref_sid = inst
                        .attrs
                        .get(vb_doc::symbol::ATTR_SYMBOL_REF)
                        .cloned()
                        .unwrap_or_default();
                    let is_root = root == nid;
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("组件实例:{sym_name}"));
                        if !overrides.is_empty() {
                            ui.small(format!("(覆盖 {})", overrides));
                        }
                        if ui.small_button("跳到主件").clicked() {
                            if let Some(cid) = self.doc.find_by_sid(&ref_sid) {
                                if let Some(pid) =
                                    self.doc.nodes.get(cid).unwrap().children.first().copied()
                                {
                                    self.selection = vec![self
                                        .doc
                                        .nodes
                                        .get(pid)
                                        .unwrap()
                                        .sid
                                        .as_str()
                                        .to_string()];
                                    self.say(
                                        "已选中主件原型(定义区不在画布;编辑主件将同步全部实例)",
                                    );
                                }
                            }
                        }
                    });
                    if !is_root {
                        ui.small("(内部编辑将登记为覆盖,主件同步时保留)");
                    }
                    ui.separator();
                } else if let Some(c) = vb_doc::symbol::def_container_of(&self.doc, nid) {
                    // 选中主件定义区节点:显示主件身份(编辑此处将同步全部实例)
                    let main_name = self.doc.nodes.get(c).unwrap().name.clone();
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(format!("主件:{main_name}"));
                        ui.small("(编辑主件将同步全部实例;定义区不随页面导出为可见内容)");
                    });
                    ui.separator();
                }

                // 顶部「标签」行(design/03 §四 ASCII:分组外)
                const TAGS: [&str; 16] = [
                    "div", "section", "header", "nav", "main", "footer", "article", "aside", "h1",
                    "h2", "h3", "p", "span", "a", "button", "li",
                ];
                let mut tag_sel = cur_tag.clone();
                egui::ComboBox::from_id_salt("tag_sel")
                    .selected_text(format!("标签 {tag_sel}"))
                    .show_ui(ui, |ui| {
                        for t in TAGS {
                            ui.selectable_value(&mut tag_sel, t.to_string(), t);
                        }
                    });
                if tag_sel != cur_tag && tag_sel != "#text" {
                    self.exec(Command::SetTag {
                        sid: sid.clone(),
                        new: tag_sel,
                        old: None,
                    });
                }
                // 多选说明:显示主选中,写回作用全部
                if sids.len() > 1 {
                    ui.label(caption(
                        ui,
                        &format!("已选 {} 个对象 · 编辑作用于全部(一条撤销)", sids.len()),
                    ));
                }
                ui.separator();

                // ── 1. 变换:X/Y/W/H + ∠(倾斜/九宫格/缩放描边 → 阶段 2) ──
                self.prop_section(ui, G_TRANSFORM, |s, ui| {
                    let pair = |s: &mut VellumApp, ui: &mut egui::Ui, a: GeomAxis, b: GeomAxis| {
                        for axis in [a, b] {
                            let Some(nid) = s.doc.find_by_sid(&sid) else {
                                return;
                            };
                            let g = s.doc.nodes.get(nid).unwrap().geom;
                            let mut v = match axis {
                                GeomAxis::X => g.x,
                                GeomAxis::Y => g.y,
                                GeomAxis::W => g.w,
                                GeomAxis::H => g.h,
                            };
                            let label = match axis {
                                GeomAxis::X => "X",
                                GeomAxis::Y => "Y",
                                GeomAxis::W => "W",
                                GeomAxis::H => "H",
                            };
                            let r = NumField::new(label, &mut v)
                                .speed(1.0)
                                .label_width(20.0)
                                .width(56.0)
                                .range(0.0, 100000.0)
                                .percent_base(match axis {
                                    GeomAxis::X | GeomAxis::W => ab_w,
                                    _ => ab_h,
                                })
                                .ui(ui);
                            let cmd = r
                                .changed
                                .then(|| combine(geom_field_cmds(&s.doc, &sids, axis, v)));
                            s.num_commit(r, cmd.flatten());
                        }
                    };
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        pair(s, ui, GeomAxis::X, GeomAxis::Y);
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        pair(s, ui, GeomAxis::W, GeomAxis::H);
                    });
                    // ∠ 旋转数值化(与画布旋转拖拽同为 transform: rotate)
                    let mut rot = rotation_deg_of(&style);
                    let r = NumField::new("∠", &mut rot)
                        .speed(1.0)
                        .step(15.0)
                        .label_width(20.0)
                        .width(56.0)
                        .unit("°")
                        .ui(ui);
                    let cmd = r
                        .changed
                        .then(|| combine(rotation_cmds(&s.doc, &sids, rot)));
                    s.num_commit(r, cmd.flatten());
                    ui.label(caption(
                        ui,
                        "倾斜 / 参考点九宫格 / 缩放描边和效果 → 阶段 2(03)",
                    ));
                });

                // ── 2. 外观:填充/圆角/不透明度 + 外观面板入口(S4 05-1) ──
                // 圆角仅盒对象(05-6-1):非盒对象改值给提示不落盘
                let radius_ok = self
                    .doc
                    .nodes
                    .get(nid)
                    .map(|n| crate::app::appearance::accepts_round_corners(&n.kind))
                    .unwrap_or(false);
                self.prop_section(ui, G_APPEARANCE, |s, ui| {
                    // 填充(取色器浮窗 + var(--x);清除 = 移除声明)
                    let cur = style
                        .iter()
                        .find(|d| d.prop == "background-color")
                        .and_then(|d| vb_common::color::parse_color(&d.value));
                    let mut col = cur
                        .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                        .unwrap_or(Color32::WHITE);
                    let tokens = s.doc.tokens.clone();
                    let r = ColorField::new("填充", &mut col).doc_tokens(&tokens).ui(ui);
                    if let Some(nm) = r.var_picked {
                        let cmds = style_prop_cmds(
                            &s.doc,
                            &sids,
                            "background-color",
                            &format!("var(--{nm})"),
                        );
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                            s.say(format!("填充 → var(--{nm})"));
                        }
                    } else if r.cleared {
                        let cmds = style_prop_remove_cmds(&s.doc, &sids, "background-color");
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                            s.say("填充已清除");
                        }
                    } else if r.changed {
                        let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
                        let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
                        let cmds = style_prop_cmds(&s.doc, &sids, "background-color", &hex);
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                        }
                    }
                    // 圆角 / 不透明度(圆角仅盒对象;非盒对象点击给提示,05-6-1)
                    let mut radius = style
                        .iter()
                        .find(|d| d.prop == "border-radius")
                        .and_then(|d| d.value.trim_end_matches("px").parse::<f64>().ok())
                        .unwrap_or(0.0);
                    let r = NumField::new("圆角", &mut radius)
                        .speed(1.0)
                        .step(1.0)
                        .range(0.0, 2000.0)
                        .unit("px")
                        .label_width(44.0)
                        .width(56.0)
                        .ui(ui);
                    let cmd = if r.changed && !radius_ok {
                        None
                    } else {
                        r.changed.then(|| {
                            let cmds = style_prop_cmds(
                                &s.doc,
                                &sids,
                                "border-radius",
                                &format!("{}px", radius as i64),
                            );
                            combine(cmds)
                        })
                    };
                    let radius_changed = r.changed;
                    s.num_commit(r, cmd.flatten());
                    if radius_changed && !radius_ok {
                        s.toast_warn("圆角仅对盒对象有效(路径/文字对象不支持)");
                    }
                    let mut op = style
                        .iter()
                        .find(|d| d.prop == "opacity")
                        .and_then(|d| d.value.parse::<f64>().ok())
                        .unwrap_or(1.0);
                    let r = NumField::new("不透明", &mut op)
                        .speed(0.01)
                        .step(0.01)
                        .range(0.0, 1.0)
                        .label_width(44.0)
                        .width(56.0)
                        .ui(ui);
                    let cmd = r.changed.then(|| {
                        let cmds = style_prop_cmds(&s.doc, &sids, "opacity", &format!("{op:.2}"));
                        combine(cmds)
                    });
                    s.num_commit(r, cmd.flatten());
                    // 隐藏 / 锁定(对象旗标;SetFlags)
                    if ui.checkbox(&mut hidden, "隐藏").changed() {
                        let cmds: Vec<Command> = sids
                            .iter()
                            .map(|sid| Command::SetFlags {
                                sid: sid.clone(),
                                hidden: Some(hidden),
                                locked: None,
                                old: None,
                            })
                            .collect();
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                        }
                    }
                    if ui.checkbox(&mut locked, "锁定").changed() {
                        let cmds: Vec<Command> = sids
                            .iter()
                            .map(|sid| Command::SetFlags {
                                sid: sid.clone(),
                                hidden: None,
                                locked: Some(locked),
                                old: None,
                            })
                            .collect();
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                        }
                    }
                    // 外观面板 / 描边面板入口(S4 05-1/05-3:多填充、
                    // 条目排序/禁用/混合模式、描边全字段)
                    ui.horizontal(|ui| {
                        if ui.button("外观面板 ⇧F6").clicked() {
                            s.run_command("view.toggle_appearance_panel", false, false);
                        }
                        if ui.button("描边面板 ^F10").clicked() {
                            s.run_command("view.toggle_stroke_panel", false, false);
                        }
                    });
                    ui.label(caption(
                        ui,
                        "多填充 / 描边 / 效果条目(排序·禁用·混合模式)→ 外观面板;描边全字段 → 描边面板",
                    ));
                });

                // ── 3. 布局:display/方向/间距/主轴/交叉轴/内边距/外边距 ──
                self.prop_section(ui, G_LAYOUT, |s, ui| {
                    let get = |p: &str| {
                        style
                            .iter()
                            .find(|d| d.prop == p)
                            .map(|d| d.value.clone())
                            .unwrap_or_default()
                    };
                    let mut display = {
                        let d = get("display");
                        if d.is_empty() {
                            "block".to_string()
                        } else {
                            d
                        }
                    };
                    let mut direction = {
                        let d = get("flex-direction");
                        if d.is_empty() {
                            "row".to_string()
                        } else {
                            d
                        }
                    };
                    let mut gap = get("gap")
                        .trim_end_matches("px")
                        .parse::<f64>()
                        .unwrap_or(0.0);
                    let mut justify = {
                        let j = get("justify-content");
                        if j.is_empty() {
                            "flex-start".to_string()
                        } else {
                            j
                        }
                    };
                    let mut align = {
                        let a = get("align-items");
                        if a.is_empty() {
                            "stretch".to_string()
                        } else {
                            a
                        }
                    };
                    egui::ComboBox::from_id_salt("disp")
                        .selected_text(format!("显示 {display}"))
                        .show_ui(ui, |ui| {
                            for v in ["block", "flex", "inline-flex", "none"] {
                                ui.selectable_value(&mut display, v.to_string(), v);
                            }
                        });
                    egui::ComboBox::from_id_salt("dir")
                        .selected_text(format!("方向 {direction}"))
                        .show_ui(ui, |ui| {
                            for v in ["row", "column", "row-reverse", "column-reverse"] {
                                ui.selectable_value(&mut direction, v.to_string(), v);
                            }
                        });
                    let r = NumField::new("间距", &mut gap)
                        .speed(1.0)
                        .step(1.0)
                        .range(0.0, 400.0)
                        .unit("px")
                        .label_width(44.0)
                        .width(56.0)
                        .ui(ui);
                    let cmd = r.changed.then(|| {
                        let cmds =
                            style_prop_cmds(&s.doc, &sids, "gap", &format!("{}px", gap as i64));
                        combine(cmds)
                    });
                    s.num_commit(r, cmd.flatten());
                    egui::ComboBox::from_id_salt("jc")
                        .selected_text(format!("主轴 {justify}"))
                        .show_ui(ui, |ui| {
                            for v in [
                                "flex-start",
                                "center",
                                "flex-end",
                                "space-between",
                                "space-around",
                            ] {
                                ui.selectable_value(&mut justify, v.to_string(), v);
                            }
                        });
                    egui::ComboBox::from_id_salt("ai")
                        .selected_text(format!("交叉轴 {align}"))
                        .show_ui(ui, |ui| {
                            for v in ["stretch", "center", "flex-start", "flex-end"] {
                                ui.selectable_value(&mut align, v.to_string(), v);
                            }
                        });
                    // display/方向/主轴/交叉轴 改动即写(display=flex 时
                    // 自动补 justify/align,承袭既有行为)
                    if display != get("display") {
                        let mut st = set_style_prop(style.clone(), "display", &display);
                        if display == "flex" {
                            st = set_style_prop(st, "justify-content", &justify);
                            st = set_style_prop(st, "align-items", &align);
                        }
                        let cmds: Vec<Command> = sids
                            .iter()
                            .map(|sid| Command::SetStyle {
                                sid: sid.clone(),
                                new: st.clone(),
                                old: None,
                            })
                            .collect();
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                        }
                    }
                    if direction != get("flex-direction") {
                        let cmds = style_prop_cmds(&s.doc, &sids, "flex-direction", &direction);
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                        }
                    }
                    if justify != get("justify-content") {
                        let cmds = style_prop_cmds(&s.doc, &sids, "justify-content", &justify);
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                        }
                    }
                    if align != get("align-items") {
                        let cmds = style_prop_cmds(&s.doc, &sids, "align-items", &align);
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                        }
                    }
                    // 内边距 / 外边距(px 单值;多值简写待 05 细化)
                    let mut pad = get("padding")
                        .split_whitespace()
                        .next()
                        .unwrap_or("0")
                        .trim_end_matches("px")
                        .parse::<f64>()
                        .unwrap_or(0.0);
                    let r = NumField::new("内边距", &mut pad)
                        .speed(1.0)
                        .step(1.0)
                        .range(0.0, 1000.0)
                        .unit("px")
                        .label_width(44.0)
                        .width(56.0)
                        .ui(ui);
                    let cmd = r.changed.then(|| {
                        let cmds =
                            style_prop_cmds(&s.doc, &sids, "padding", &format!("{}px", pad as i64));
                        combine(cmds)
                    });
                    s.num_commit(r, cmd.flatten());
                    let mut mar = get("margin")
                        .split_whitespace()
                        .next()
                        .unwrap_or("0")
                        .trim_end_matches("px")
                        .parse::<f64>()
                        .unwrap_or(0.0);
                    let r = NumField::new("外边距", &mut mar)
                        .speed(1.0)
                        .step(1.0)
                        .range(0.0, 1000.0)
                        .unit("px")
                        .label_width(44.0)
                        .width(56.0)
                        .ui(ui);
                    let cmd = r.changed.then(|| {
                        let cmds =
                            style_prop_cmds(&s.doc, &sids, "margin", &format!("{}px", mar as i64));
                        combine(cmds)
                    });
                    s.num_commit(r, cmd.flatten());
                });

                // ── 4. 文本(仅文本对象;字体族/字距/行距 → 阶段 3 04) ──
                if is_text {
                    self.prop_section(ui, G_TEXT, |s, ui| {
                        let mut fs = style
                            .iter()
                            .find(|d| d.prop == "font-size")
                            .and_then(|d| d.value.trim_end_matches("px").parse::<f64>().ok())
                            .unwrap_or(24.0);
                        let r = NumField::new("字号", &mut fs)
                            .speed(1.0)
                            .step(1.0)
                            .range(1.0, 500.0)
                            .unit("px")
                            .label_width(44.0)
                            .width(56.0)
                            .ui(ui);
                        let cmd = r.changed.then(|| {
                            let cmds = style_prop_cmds(
                                &s.doc,
                                &sids,
                                "font-size",
                                &format!("{}px", fs as i64),
                            );
                            combine(cmds)
                        });
                        s.num_commit(r, cmd.flatten());
                        let cur_align = style
                            .iter()
                            .find(|d| d.prop == "text-align")
                            .map(|d| d.value.clone())
                            .unwrap_or_else(|| "left".into());
                        let mut sel = cur_align.clone();
                        egui::ComboBox::from_id_salt("prop_ta")
                            .selected_text(format!("对齐 {sel}"))
                            .show_ui(ui, |ui| {
                                for v in ["left", "center", "right", "justify"] {
                                    ui.selectable_value(&mut sel, v.to_string(), v);
                                }
                            });
                        if sel != cur_align {
                            let cmds = style_prop_cmds(&s.doc, &sids, "text-align", &sel);
                            if let Some(cmd) = combine(cmds) {
                                s.exec(cmd);
                            }
                        }
                        let cur_col = style
                            .iter()
                            .find(|d| d.prop == "color")
                            .and_then(|d| vb_common::color::parse_color(&d.value));
                        let mut tc = cur_col
                            .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                            .unwrap_or(Color32::BLACK);
                        let tokens = s.doc.tokens.clone();
                        let r = ColorField::new("字色", &mut tc).doc_tokens(&tokens).ui(ui);
                        if let Some(nm) = r.var_picked {
                            let cmds =
                                style_prop_cmds(&s.doc, &sids, "color", &format!("var(--{nm})"));
                            if let Some(cmd) = combine(cmds) {
                                s.exec(cmd);
                                s.say(format!("字色 → var(--{nm})"));
                            }
                        } else if r.changed && !r.cleared {
                            let [cr, cg, cb, ca] = tc.to_srgba_unmultiplied();
                            let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
                            let cmds = style_prop_cmds(&s.doc, &sids, "color", &hex);
                            if let Some(cmd) = combine(cmds) {
                                s.exec(cmd);
                            }
                        }
                        ui.label(caption(ui, "字体族 / 字距 / 行距 → 字符面板(Ctrl+T);对齐/缩进/段距 → 段落面板(Ctrl+Alt+T)"));
                    });
                }

                // ── 5. 交互:链接 / 目标 ──
                self.prop_section(ui, G_INTERACT, |s, ui| {
                    let mut href = attrs
                        .iter()
                        .find(|(k, _)| k == "href")
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label("链接");
                        let h = vb_ui::theme::row_height(ui.ctx());
                        if ui
                            .add_sized([160.0, h], egui::TextEdit::singleline(&mut href))
                            .lost_focus()
                            && href.trim() != href_value(&attrs, "href")
                        {
                            // 内容没变不推 undo 条目(此前点一下输入框
                            // 就多出一条「修改 HTML 属性」)
                            let cmds = attr_cmds(&s.doc, &sids, "href", href.trim());
                            if let Some(cmd) = combine(cmds) {
                                s.exec(cmd);
                            }
                        }
                    });
                    let mut target = href_value(&attrs, "target");
                    egui::ComboBox::from_id_salt("prop_target")
                        .selected_text(format!(
                            "目标 {}",
                            if target.is_empty() {
                                "_self"
                            } else {
                                target.as_str()
                            }
                        ))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut target, String::new(), "_self(默认)");
                            for v in ["_blank", "_parent", "_top"] {
                                ui.selectable_value(&mut target, v.to_string(), v);
                            }
                        });
                    if target != href_value(&attrs, "target") {
                        let cmds = attr_cmds(&s.doc, &sids, "target", &target);
                        if let Some(cmd) = combine(cmds) {
                            s.exec(cmd);
                        }
                    }
                });

                // ── 6. 无障碍:alt / aria-label ──
                self.prop_section(ui, G_A11Y, |s, ui| {
                    for (key, label) in [("alt", "alt"), ("aria-label", "aria")] {
                        let mut val = href_value(&attrs, key);
                        ui.horizontal(|ui| {
                            ui.label(label);
                            let h = vb_ui::theme::row_height(ui.ctx());
                            if ui
                                .add_sized([160.0, h], egui::TextEdit::singleline(&mut val))
                                .lost_focus()
                                && val != href_value(&attrs, key)
                            {
                                let cmds = attr_cmds(&s.doc, &sids, key, &val);
                                if let Some(cmd) = combine(cmds) {
                                    s.exec(cmd);
                                }
                            }
                        });
                    }
                });

                // ── 7. 导出:节点可读名(data-vb-name;倍率预设 → 遗留) ──
                self.prop_section(ui, G_EXPORT, |s, ui| {
                    let mut nm = name.clone();
                    ui.horizontal(|ui| {
                        ui.label("名称");
                        let h = vb_ui::theme::row_height(ui.ctx());
                        if ui
                            .add_sized([160.0, h], egui::TextEdit::singleline(&mut nm))
                            .lost_focus()
                            && !nm.trim().is_empty()
                            && nm.trim() != name
                        {
                            // Rename = 图层名 = data-vb-name(导出层同步写)
                            let cmds = rename_cmds(&s.doc, &sids, nm.trim());
                            if let Some(cmd) = combine(cmds) {
                                s.exec(cmd);
                                s.say(format!("名称 → {}", nm.trim()));
                            }
                        }
                    });
                    ui.label(caption(
                        ui,
                        "导出倍率 @1x/@2x/@3x:暂无节点级存储位,现用导出对话框统一倍率(遗留项见 02c 报告)",
                    ));
                });

                // --- 对齐(P3.8:复用命令派发,快捷键同源) ---
                ui.separator();
                ui.label("对齐");
                ui.horizontal(|ui| {
                    let btns: [(&str, &str); 6] = [
                        ("align.left", "⇤"),
                        ("align.hcenter", "↔"),
                        ("align.right", "⇥"),
                        ("align.top", "⤒"),
                        ("align.vcenter", "↕"),
                        ("align.bottom", "⤓"),
                    ];
                    for (id, icon) in btns {
                        if ui
                            .button(icon)
                            .on_hover_text(shortcuts::command_label(id).unwrap_or(id))
                            .clicked()
                        {
                            self.run_command(id, false, false);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("分布");
                    if ui.button("↔ 等距").clicked() {
                        self.run_command("object.distribute_h", false, false);
                    }
                    if ui.button("↕ 等距").clicked() {
                        self.run_command("object.distribute_v", false, false);
                    }
                });

                // --- 路径查找器(C1:四基本运算,两两矢量路径) ---
                ui.separator();
                ui.label("路径查找器");
                ui.horizontal(|ui| {
                    let btns: [(&str, &str); 4] = [
                        ("path.union", "联集"),
                        ("path.subtract", "减去顶层"),
                        ("path.intersect", "交集"),
                        ("path.xor", "差集"),
                    ];
                    for (id, label) in btns {
                        if ui.button(label).clicked() {
                            self.run_command(id, false, false);
                        }
                    }
                });

                ui.separator();
            }
        } else {
            // --- 空态(02-6-6;04-3-3 重制)---
            // 实测 crop-right.png(P1-⑦):三行长灰文案占满 280px 窄坞,信息密度
            // 极低。改为「一句短话 + 常用动作入口」:教学细节移进悬停提示,
            // 垂直空间留给真正能点的动作(全部走既有命令,无假控件)。
            ui.strong("未选中对象");
            ui.label(caption(
                ui,
                "点选画布对象,或从下面开始。",
            ))
            .on_hover_text(
                "选中后按 变换/外观/布局/文本/交互/无障碍/导出 分组编辑;数值框支持拖标签改值与表达式(如 320/2、50%)。",
            );
            ui.add_space(vb_ui::theme::space::S3);
            ui.label(caption(ui, "常用"));
            ui.horizontal_wrapped(|ui| {
                for (id, icon, tip) in [
                    ("tool.select", Name::ToolSelect, "选择工具(V):点选 / 拖框选"),
                    ("tool.rect", Name::ToolRect, "矩形工具(M):拖框新建"),
                    ("tool.ellipse", Name::ToolEllipse, "椭圆工具(L):拖框新建"),
                    (
                        "tool.text",
                        Name::ToolText,
                        "文字工具(T):单击点文本 / 拖框区域文本",
                    ),
                ] {
                    if icon_button(ui, icon, tip).clicked() {
                        self.run_command(id, false, false);
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (id, icon, tip) in [
                    ("view.fit", Name::Expanded, "缩放到全部画板可见(Ctrl+0)"),
                    ("window.tab_layers", Name::KindLayer, "切换到图层面板"),
                    (
                        "window.tab_artboards",
                        Name::ToolArtboard,
                        "切换到画板面板(可新建画板)",
                    ),
                ] {
                    if icon_button(ui, icon, tip).clicked() {
                        self.run_command(id, false, false);
                    }
                }
            });
            ui.add_space(vb_ui::theme::space::S3);
            ui.label(caption(
                ui,
                "提示:V 点选 · M 矩形 · L 椭圆 · Ctrl+0 适合窗口。",
            ));
            ui.separator();
        }
    }

    /// 七分组通用外壳:可折叠 SectionHeader + 展开时渲染组体。
    fn prop_section(
        &mut self,
        ui: &mut egui::Ui,
        group: usize,
        body: impl FnOnce(&mut VellumApp, &mut egui::Ui),
    ) {
        let mut open = self.prop_groups_open[group];
        let shown = SectionHeader::new(PROP_GROUPS[group])
            .collapsible(&mut open)
            .ui(ui)
            == Some(true);
        self.prop_groups_open[group] = open;
        if shown {
            body(self, ui);
            ui.add_space(2.0);
        }
    }
}

/// 属性读取:BTreeMap 序 → 值(缺省空串)。
fn href_value(attrs: &[(String, String)], key: &str) -> String {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

/// 当前活动画板的 (宽, 高)(表达式 `%` 基准;无画板时给默认尺寸)。
impl VellumApp {
    pub(crate) fn active_artboard_size(&self) -> (f64, f64) {
        self.active_artboard()
            .and_then(|a| self.doc.nodes.get(a))
            .map(|n| (n.geom.w.max(1.0), n.geom.h.max(1.0)))
            .unwrap_or((1440.0, 900.0))
    }
}

/// 七分组存在性测试(门 2 前半):组清单与 design/03 §四 完全一致,
/// 组下标常量与清单对齐。
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seven_groups_match_design04_order() {
        assert_eq!(
            PROP_GROUPS,
            ["变换", "外观", "布局", "文本", "交互", "无障碍", "导出"],
            "七分组清单/组序必须与 design/03 §四 ASCII 规格一致"
        );
        assert_eq!(G_TRANSFORM, 0);
        assert_eq!(G_APPEARANCE, 1);
        assert_eq!(G_LAYOUT, 2);
        assert_eq!(G_TEXT, 3);
        assert_eq!(G_INTERACT, 4);
        assert_eq!(G_A11Y, 5);
        assert_eq!(G_EXPORT, 6);
    }

    /// 折叠状态数组与组数对齐(渲染外壳按下标读写,越界即 panic —— 编译期
    /// 数组长度锁死)。
    #[test]
    fn fold_state_array_matches_group_count() {
        let opens: [bool; 7] = [true; 7];
        assert_eq!(opens.len(), PROP_GROUPS.len());
    }
}
