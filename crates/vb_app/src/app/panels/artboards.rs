//! 右侧面板 · 画板 Tab(S1-b 自 layers.rs 迁入;S1-d 02-5 **全面增强**)。
//!
//! 本轮新增(对照 02 篇 §4-02-5 与 `design/03 §5.1` 画板面板规格):
//! - **画板列表**(02-5-1):名称/尺寸/背景色,按文档画板序(= 导出序)
//!   排列;点击选中并**画布定位**(复用 `view.zoom_to_selection` 命令);
//!   双击改名(经 [`Command::Rename`])。
//! - **新建/复制/删除/改名**(02-5-2):全部经命令路径(Insert/Delete/
//!   Rename);复制 = 克隆整棵画板子树(新 sid,纵向落到最下方)。
//! - **重新排列**(02-5-1):网格/按行/按列,纯函数 [`arrange_layout`]
//!   计算几何,Compound(SetGeom…) 一次落盘一次撤销;
//! - **适配图稿边界**(02-5-2):按内容包围盒(纯函数 [`content_union`])
//!   调画板几何,单条 SetGeom;无内容的画板不产生命令(不做"点了没反应")。
//! - **画板预设**(02-5-3):[`AB_PRESETS`] 七档(Web 1920/1440/1080、
//!   移动 750/375、A4 横竖)+ 自定义 W×H;取向横竖(w/h 互换);
//!   本表是**常量单一来源**,控制面板「画板工具态」共享引用。
//! - **状态栏画板导航**(02-5-4):◀ 1/N ▶(在 `panels/mod.rs`),复用
//!   `view.prev_artboard` / `view.next_artboard` 命令,与画布选中双向联动。
//!
//! 全部操作经文档命令(SetGeom/Insert/Delete/Rename/SetStyle),Agent 可
//! 用 patch 的 set_box/new_artboard/delete/rename/set_attr 等价复现;
//! 无 commands.yaml 新 ID(不新增菜单级命令)。

use egui::{Color32, Pos2, Rect};
use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeId, NodeKind};
use vb_ui::components::{ColorField, NumField};
use vb_ui::icons::{self, Name};
use vb_ui::theme;

use crate::app::control_panel::{
    combine, geom_axes_cmd, geom_field_cmds, style_prop_cmds, style_prop_remove_cmds, GeomAxis,
};
use crate::app::VellumApp;

/// 画板尺寸预设(02-5-3;**常量单一来源**:画板面板与控制面板
/// 「画板工具态/画板选项」共用此表)。自定义 = 表外任意 W×H。
pub(crate) const AB_PRESETS: [(&str, f64, f64); 7] = [
    ("Web 1920×1080", 1920.0, 1080.0),
    ("Web 1440×900", 1440.0, 900.0),
    ("Web 1080×1920", 1080.0, 1920.0),
    ("移动 750×1334", 750.0, 1334.0),
    ("移动 375×667", 375.0, 667.0),
    ("A4 竖 794×1123", 794.0, 1123.0),
    ("A4 横 1123×794", 1123.0, 794.0),
];

/// 预设之外的显示名(尺寸不匹配任何行时;自定义 W/H 数值框承接)。
pub(crate) const PRESET_CUSTOM: &str = "自定义";

/// 画板重新排列布局(02-5-1)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arrange {
    /// 网格(近方形列数)
    Grid,
    /// 按行(单行横排)
    Row,
    /// 按列(单列纵排;与「+新建」纵向堆放一致)
    Column,
}

impl Arrange {
    fn label(self) -> &'static str {
        match self {
            Arrange::Grid => "网格",
            Arrange::Row => "按行",
            Arrange::Column => "按列",
        }
    }
}

/// 重新排列几何(纯函数):按当前画板序给每块画板新 (x, y);
/// 尺寸不动。网格 = 近方形列数(ceil(√n)),列宽取列内最大、行高取行内
/// 最大;行/列 = 游标沿轴推进。gap 为画板间距。
pub(crate) fn arrange_layout(sizes: &[(f64, f64)], layout: Arrange, gap: f64) -> Vec<(f64, f64)> {
    let n = sizes.len();
    if n == 0 {
        return vec![];
    }
    match layout {
        Arrange::Row => {
            let mut out = Vec::with_capacity(n);
            let mut x = 0.0;
            for (w, _) in sizes {
                out.push((x, 0.0));
                x += w + gap;
            }
            out
        }
        Arrange::Column => {
            let mut out = Vec::with_capacity(n);
            let mut y = 0.0;
            for (_, h) in sizes {
                out.push((0.0, y));
                y += h + gap;
            }
            out
        }
        Arrange::Grid => {
            let cols = (n as f64).sqrt().ceil().max(1.0) as usize;
            let rows = n.div_ceil(cols);
            let mut col_w = vec![0.0f64; cols];
            let mut row_h = vec![0.0f64; rows];
            for (i, (w, h)) in sizes.iter().enumerate() {
                col_w[i % cols] = col_w[i % cols].max(*w);
                row_h[i / cols] = row_h[i / cols].max(*h);
            }
            let mut col_x = Vec::with_capacity(cols);
            let mut x = 0.0;
            for w in &col_w {
                col_x.push(x);
                x += w + gap;
            }
            let mut row_y = Vec::with_capacity(rows);
            let mut y = 0.0;
            for h in &row_h {
                row_y.push(y);
                y += h + gap;
            }
            (0..n).map(|i| (col_x[i % cols], row_y[i / cols])).collect()
        }
    }
}

/// 画板内容包围盒(纯函数;画板本地坐标,递归含编组内后代;
/// 与 hidden 无关 —— 适配按图稿几何,不含可见性语义)。
pub(crate) fn content_union(doc: &Document, ab: NodeId) -> Option<(f64, f64, f64, f64)> {
    let mut out: Option<(f64, f64, f64, f64)> = None;
    let kids = doc.nodes.get(ab)?.children.clone();
    for k in kids {
        content_walk(doc, k, 0.0, 0.0, &mut out);
    }
    out
}

fn content_walk(
    doc: &Document,
    id: NodeId,
    ox: f64,
    oy: f64,
    out: &mut Option<(f64, f64, f64, f64)>,
) {
    let Some(n) = doc.nodes.get(id) else {
        return;
    };
    let (x0, y0) = (ox + n.geom.x, oy + n.geom.y);
    let (x1, y1) = (x0 + n.geom.w, y0 + n.geom.h);
    match out {
        None => *out = Some((x0, y0, x1, y1)),
        Some((mx0, my0, mx1, my1)) => {
            *mx0 = mx0.min(x0);
            *my0 = my0.min(y0);
            *mx1 = mx1.max(x1);
            *my1 = my1.max(y1);
        }
    }
    for &c in &n.children {
        content_walk(doc, c, x0, y0, out);
    }
}

/// 重新排列命令(02-5-1):按 [`arrange_layout`] 生成 Compound(SetGeom…),
/// 一次落盘一次撤销。
pub(crate) fn rearrange_cmds(doc: &Document, layout: Arrange, gap: f64) -> Vec<Command> {
    let sizes: Vec<(f64, f64)> = doc
        .artboards
        .iter()
        .filter_map(|&a| doc.nodes.get(a).map(|n| (n.geom.w, n.geom.h)))
        .collect();
    let positions = arrange_layout(&sizes, layout, gap);
    let mut cmds = Vec::new();
    for (&ab, (x, y)) in doc.artboards.iter().zip(positions) {
        let Some(n) = doc.nodes.get(ab) else {
            continue;
        };
        let sid = n.sid.as_str().to_string();
        let mut g = n.geom;
        if (g.x - x).abs() < 0.5 && (g.y - y).abs() < 0.5 {
            continue; // 已在位,不产生空命令
        }
        g.x = x;
        g.y = y;
        cmds.push(Command::SetGeom {
            sid,
            new: g,
            old: None,
            old_declared: None,
        });
    }
    cmds
}

/// 适配图稿边界命令(02-5-2):画板几何 = 内容包围盒(原点随之移动,
/// 保证内容四周贴合;无内容返回 None,不产生命令)。
pub(crate) fn fit_artboard_cmd(doc: &Document, sid: &str) -> Option<Command> {
    let ab = doc.find_by_sid(sid)?;
    if !matches!(doc.nodes.get(ab)?.kind, NodeKind::Artboard) {
        return None;
    }
    let (x0, y0, x1, y1) = content_union(doc, ab)?;
    let mut g = doc.nodes.get(ab)?.geom;
    g.x += x0.round();
    g.y += y0.round();
    g.w = ((x1 - x0).round() as i64).max(1) as f64;
    g.h = ((y1 - y0).round() as i64).max(1) as f64;
    Some(Command::SetGeom {
        sid: sid.to_string(),
        new: g,
        old: None,
        old_declared: None,
    })
}

/// 复制画板命令(02-5-2):克隆整棵子树(全部新 sid)+ 更名「副本」+
/// 纵向落到现有画板最下方;返回 (Insert 命令, 新 sid)。
pub(crate) fn duplicate_artboard_cmd(doc: &mut Document, sid: &str) -> Option<(Command, String)> {
    let ab = doc.find_by_sid(sid)?;
    if !matches!(doc.nodes.get(ab)?.kind, NodeKind::Artboard) {
        return None;
    }
    let mut tree = vb_doc::model::NodeTree::from_document(doc, ab)?;
    crate::app::re_sid_tree(&mut tree, doc);
    tree.node.name = format!("{} 副本", tree.node.name);
    tree.node.geom.y = doc
        .artboards
        .iter()
        .filter_map(|&a| doc.nodes.get(a).map(|n| n.geom.y + n.geom.h))
        .fold(0.0f64, f64::max)
        + 80.0;
    let new_sid = tree.node.sid.as_str().to_string();
    let root_sid = doc.nodes.get(doc.root)?.sid.as_str().to_string();
    Some((
        Command::Insert {
            parent_sid: root_sid,
            index: usize::MAX,
            tree,
        },
        new_sid,
    ))
}

// ───────────────────────────────── 渲染 ─────────────────────────────────

impl VellumApp {
    pub(crate) fn artboards_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("画板");
        ui.separator();

        let active = self.active_artboard();
        let active_sid = active
            .and_then(|a| self.doc.nodes.get(a))
            .map(|n| n.sid.as_str().to_string());

        // --- 工具行:新建 / 复制 / 删除(02-5-2) ---
        ui.horizontal(|ui| {
            if ui.button("+ 新建").clicked() {
                // 与控制面板「+画板」同一条命令路径(S1-c 抽取共享)
                self.add_default_artboard();
            }
            if ui.button("复制").clicked() {
                match active_sid.clone().and_then(|sid| {
                    let mut doc = std::mem::take(&mut self.doc);
                    let d = duplicate_artboard_cmd(&mut doc, &sid);
                    self.doc = doc;
                    d
                }) {
                    Some((cmd, new_sid)) => {
                        self.exec(cmd);
                        self.selection = vec![new_sid];
                        self.say("画板已复制(纵向落到最下方)");
                    }
                    None => self.say("复制:先选中一块画板"),
                }
            }
            if ui.button("🗑 删除").clicked() {
                match &active_sid {
                    Some(sid) if self.doc.artboards.len() > 1 => {
                        self.exec(Command::Delete {
                            target_sid: sid.clone(),
                            captured: None,
                        });
                        self.selection.clear();
                        // 隔离栈里可能压着被删画板的节点:
                        // 不清会导致拾取拿到死 id 而全面失效
                        self.isolate_stack
                            .retain(|id| self.doc.nodes.get(*id).is_some());
                        self.say("画板已删除");
                    }
                    _ => self.say("删除:至少保留一块画板"),
                }
            }
        });

        // --- 重新排列:网格 / 按行 / 按列 + 间距(02-5-1) ---
        ui.horizontal(|ui| {
            ui.label("重新排列");
            let mut gap = self.arrange_gap;
            let r = NumField::new("间距", &mut gap)
                .speed(1.0)
                .step(8.0)
                .range(0.0, 2000.0)
                .unit("px")
                .label_width(26.0)
                .width(52.0)
                .ui(ui);
            if r.changed {
                self.arrange_gap = gap;
            }
            for layout in [Arrange::Grid, Arrange::Row, Arrange::Column] {
                if ui
                    .small_button(layout.label())
                    .on_hover_text(format!(
                        "全部画板按{}重新排列(经 SetGeom 复合命令,一次撤销)",
                        layout.label()
                    ))
                    .clicked()
                {
                    let cmds = rearrange_cmds(&self.doc, layout, self.arrange_gap);
                    let n = cmds.len();
                    if n == 0 {
                        self.say("重新排列:画板已在位");
                    } else {
                        self.exec(Command::Compound { cmds });
                        self.say(format!("已按{}排列 {n} 块画板", layout.label()));
                    }
                }
            }
        });

        // --- 适配图稿边界(02-5-2;作用于选中画板) ---
        ui.horizontal(|ui| {
            if ui
                .button("⤢ 适配图稿边界")
                .on_hover_text("画板几何 = 内容包围盒(选中画板;经 SetGeom)")
                .clicked()
            {
                match &active_sid {
                    Some(sid) => match fit_artboard_cmd(&self.doc, sid) {
                        Some(cmd) => {
                            self.exec(cmd);
                            self.say("画板已适配图稿边界");
                        }
                        None => self.say("适配图稿边界:画板没有内容"),
                    },
                    None => self.say("适配图稿边界:先选中一块画板"),
                }
            }
        });
        ui.separator();

        // --- 选中画板区:预设 / 取向 / 自定义尺寸 / 背景(02-5-3) ---
        if let Some(sid) = &active_sid {
            ui.label("选中画板");
            let (w, h) = {
                let n = self
                    .doc
                    .nodes
                    .get(self.doc.find_by_sid(sid).unwrap())
                    .unwrap();
                (n.geom.w, n.geom.h)
            };
            ui.horizontal(|ui| {
                // 预设下拉(共享 AB_PRESETS;匹配当前尺寸时高亮)
                let label = AB_PRESETS
                    .iter()
                    .find(|(_, pw, ph)| (*pw - w).abs() < 0.5 && (*ph - h).abs() < 0.5)
                    .map(|(n, _, _)| *n)
                    .unwrap_or(PRESET_CUSTOM);
                egui::ComboBox::from_id_salt("ab_panel_preset")
                    .selected_text(format!("预设 {label}"))
                    .show_ui(ui, |ui| {
                        for (name, pw, ph) in AB_PRESETS {
                            let is_cur = (pw - w).abs() < 0.5 && (ph - h).abs() < 0.5;
                            if ui.selectable_label(is_cur, name).clicked() {
                                if let Some(cmd) = geom_axes_cmd(
                                    &self.doc,
                                    sid,
                                    &[(GeomAxis::W, pw), (GeomAxis::H, ph)],
                                ) {
                                    self.exec(cmd);
                                    self.say(format!("画板预设 → {name}"));
                                }
                            }
                        }
                    });
                // 取向:横/竖互换(经 SetGeom)
                if ui
                    .small_button("⇄ 取向")
                    .on_hover_text("横竖互换(w/h 互换,经 SetGeom)")
                    .clicked()
                {
                    if let Some(cmd) =
                        geom_axes_cmd(&self.doc, sid, &[(GeomAxis::W, h), (GeomAxis::H, w)])
                    {
                        self.exec(cmd);
                        self.say("画板取向已互换");
                    }
                }
            });
            // 自定义 W×H(预设之外的尺寸从这里来)
            ui.horizontal(|ui| {
                for (axis, label) in [(GeomAxis::W, "W"), (GeomAxis::H, "H")] {
                    let mut v = match axis {
                        GeomAxis::W => w,
                        _ => h,
                    };
                    let r = NumField::new(label, &mut v)
                        .speed(1.0)
                        .step(1.0)
                        .range(1.0, 100000.0)
                        .unit("px")
                        .label_width(18.0)
                        .width(56.0)
                        .ui(ui);
                    if r.changed {
                        let cmd = combine(geom_field_cmds(
                            &self.doc,
                            std::slice::from_ref(sid),
                            axis,
                            v,
                        ));
                        if let Some(c) = cmd {
                            self.exec(c);
                        }
                    }
                }
                ui.weak(PRESET_CUSTOM);
            });
            // 背景色(画板节点 background-color;SetStyle)
            let (style, ab_sid) = {
                let n = self
                    .doc
                    .nodes
                    .get(self.doc.find_by_sid(sid).unwrap())
                    .unwrap();
                (n.style.clone(), n.sid.as_str().to_string())
            };
            let cur = style
                .iter()
                .find(|d| d.prop == "background-color")
                .and_then(|d| vb_common::color::parse_color(&d.value));
            let mut col = cur
                .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                .unwrap_or(Color32::WHITE);
            let tokens = self.doc.tokens.clone();
            let r = ColorField::new("背景", &mut col).doc_tokens(&tokens).ui(ui);
            if let Some(name) = r.var_picked {
                let cmds = style_prop_cmds(
                    &self.doc,
                    &[ab_sid],
                    "background-color",
                    &format!("var(--{name})"),
                );
                if let Some(cmd) = combine(cmds) {
                    self.exec(cmd);
                    self.say(format!("画板背景 → var(--{name})"));
                }
            } else if r.cleared {
                let cmds = style_prop_remove_cmds(&self.doc, &[ab_sid], "background-color");
                if let Some(cmd) = combine(cmds) {
                    self.exec(cmd);
                    self.say("画板背景已清除");
                }
            } else if r.changed {
                let [cr, cg, cb, ca] = col.to_srgba_unmultiplied();
                let hex = vb_common::Rgba::new(cr, cg, cb, ca).to_shortest_hex();
                let cmds = style_prop_cmds(&self.doc, &[ab_sid], "background-color", &hex);
                if let Some(cmd) = combine(cmds) {
                    self.exec(cmd);
                }
            }
            ui.separator();
        }

        // --- 画板列表(02-5-1):名称/尺寸/背景色;点击选中 + 画布定位 ---
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, &ab) in self.doc.artboards.clone().iter().enumerate() {
                let Some(n) = self.doc.nodes.get(ab) else {
                    continue;
                };
                let (sid, name, w, h) = (
                    n.sid.as_str().to_string(),
                    n.name.clone(),
                    n.geom.w,
                    n.geom.h,
                );
                let bg = n
                    .style
                    .iter()
                    .find(|d| d.prop == "background-color")
                    .and_then(|d| vb_common::color::parse_color(&d.value));
                let selected = self.selection.last().is_some_and(|s| *s == sid);
                let (rect, resp) = ui.allocate_exact_size(
                    egui::Vec2::new(ui.available_width(), theme::space::ROW_HEIGHT + 4.0),
                    egui::Sense::click(),
                );
                let t = theme::tokens(ui.ctx());
                let hover_t = ui.ctx().animate_bool_with_time(
                    ui.id().with(("abrow", &sid)),
                    resp.hovered() && !selected,
                    theme::motion::HOVER,
                );
                let fill = if selected {
                    t.accent_dim
                } else {
                    blend(Color32::TRANSPARENT, t.bg_hover, hover_t)
                };
                ui.painter().rect_filled(
                    rect.shrink2(egui::vec2(theme::space::S1, 1.0)),
                    theme::radius::sm(),
                    fill,
                );
                // 行内容:图标 | 背景(文档色,非 UI 皮肤) | 名称 | 尺寸
                ui.painter().text(
                    Pos2::new(rect.left() + 10.0, rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    Name::KindArtboard.glyph().to_string(),
                    icons::font(13.0),
                    t.text_2,
                );
                let bgc = bg
                    .map(|c| Color32::from_rgb(c.r, c.g, c.b))
                    .unwrap_or(Color32::WHITE);
                ui.painter().circle_filled(
                    Pos2::new(rect.left() + 26.0, rect.center().y),
                    4.0,
                    bgc,
                );
                ui.painter().circle_stroke(
                    Pos2::new(rect.left() + 26.0, rect.center().y),
                    4.0,
                    egui::Stroke::new(1.0, t.text_3),
                );
                let editing = self.editing_layer.as_deref() == Some(sid.as_str());
                if editing {
                    let mut buf = name.clone();
                    let edit = ui
                        .new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(
                            Pos2::new(rect.left() + 38.0, rect.top()),
                            Pos2::new(rect.right() - 96.0, rect.bottom()),
                        )))
                        .add(egui::TextEdit::singleline(&mut buf));
                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if edit.lost_focus() || enter {
                        self.editing_layer = None;
                        let trimmed = buf.trim().to_string();
                        if !trimmed.is_empty() && trimmed != name {
                            self.exec(Command::Rename {
                                sid: sid.clone(),
                                new: trimmed,
                                old: None,
                            });
                            self.say("画板已改名(data-vb-name 同步)");
                        }
                    }
                } else {
                    ui.painter().text(
                        Pos2::new(rect.left() + 38.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        name,
                        icons::font(12.5),
                        if selected { t.text } else { t.text_2 },
                    );
                }
                ui.painter().text(
                    Pos2::new(rect.right() - 6.0, rect.center().y),
                    egui::Align2::RIGHT_CENTER,
                    format!("{}×{}", w as i64, h as i64),
                    icons::font(11.0),
                    t.text_3,
                );
                if resp.clicked() {
                    self.selection = vec![sid.clone()];
                    // 点击 = 选中并画布定位(复用既有命令)
                    self.run_command("view.zoom_to_selection", false, false);
                }
                if resp.double_clicked() {
                    self.editing_layer = Some(sid.clone());
                }
                if i + 1 < self.doc.artboards.len() {
                    ui.add_space(2.0);
                }
            }
            if self.doc.artboards.len() == 1 {
                // 空态引导(02-6-6)
                ui.add_space(4.0);
                ui.label(vb_ui::components::caption(
                    ui,
                    "只有一个默认画板 —— 点「+ 新建」,或 Shift+O 用画板工具拖框。",
                ));
            }
        });
    }
}

/// 本地面板用两色插值(悬停过渡;与 vb_ui::components 内部实现同式)。
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

// ═══════════════════════════ 测试(几何 / 预设表 / 文档状态) ═══════════════════════════
//
// 门 2:重新排列(网格/行/列)与适配图稿边界的几何单测(3 画板断言 X/Y);
// 门 3:预设表常量单测(7 预设尺寸);复制/删除/改名走既有命令,
// 由 vb_doc 命令级测试与语料库覆盖,此处补复制命令的文档级断言。

#[cfg(test)]
mod tests {
    use vb_doc::model::Geom;
    use vb_doc::undo::UndoStack;

    use super::*;

    /// 夹具:三块画板(1920×1080、800×600、375×667),其中第一块有内容。
    fn three_artboards() -> Document {
        let mut doc = Document::new_empty("t", "zh-CN");
        let ab1 = doc.new_artboard("A", 1920.0, 1080.0);
        doc.new_artboard("B", 800.0, 600.0);
        doc.new_artboard("C", 375.0, 667.0);
        // A 内放两个盒子(编组内嵌套一个),包围盒应为 (10,20)-(240,340)
        let mk = |doc: &mut Document, parent: NodeId, x: f64, y: f64, w: f64, h: f64| {
            let sid = doc.alloc_sid();
            let mut n = vb_doc::model::Node::new(NodeKind::Box, "盒", sid);
            n.geom = Geom { x, y, w, h };
            let id = doc.nodes.insert(n);
            doc.nodes.get_mut(id).unwrap().parent = Some(parent);
            doc.nodes.get_mut(parent).unwrap().children.push(id);
        };
        mk(&mut doc, ab1, 10.0, 20.0, 100.0, 50.0);
        mk(&mut doc, ab1, 200.0, 300.0, 40.0, 40.0);
        doc
    }

    fn ab_geom(doc: &Document, i: usize) -> Geom {
        doc.nodes.get(doc.artboards[i]).unwrap().geom
    }

    // ── 门 2:重新排列几何 ──

    /// 按行:三画板横排,x 依次累加 w+gap。
    #[test]
    fn arrange_row_positions() {
        let doc = three_artboards();
        let cmds = rearrange_cmds(&doc, Arrange::Row, 80.0);
        // 画板 A 已在 (0,0),不产生空命令
        assert_eq!(cmds.len(), 2);
        let mut d = doc;
        let mut stack = UndoStack::new();
        stack.push(&mut d, Command::Compound { cmds }).unwrap();
        assert_eq!((ab_geom(&d, 0).x, ab_geom(&d, 0).y), (0.0, 0.0));
        assert_eq!((ab_geom(&d, 1).x, ab_geom(&d, 1).y), (2000.0, 0.0));
        assert_eq!((ab_geom(&d, 2).x, ab_geom(&d, 2).y), (2880.0, 0.0));
        stack.undo(&mut d).unwrap();
        assert_eq!(ab_geom(&d, 1).x, 0.0, "undo 逆回位置");
    }

    /// 按列:三画板纵排,y 依次累加 h+gap。
    #[test]
    fn arrange_column_positions() {
        let doc = three_artboards();
        let cmds = rearrange_cmds(&doc, Arrange::Column, 80.0);
        let mut d = doc;
        let mut stack = UndoStack::new();
        stack.push(&mut d, Command::Compound { cmds }).unwrap();
        assert_eq!((ab_geom(&d, 0).x, ab_geom(&d, 0).y), (0.0, 0.0));
        assert_eq!((ab_geom(&d, 1).x, ab_geom(&d, 1).y), (0.0, 1160.0));
        // C 顶 = B 底(1160)+ B 高(600)+ 间距(80)= 1840
        assert_eq!((ab_geom(&d, 2).x, ab_geom(&d, 2).y), (0.0, 1840.0));
    }

    /// 网格:ceil(√3)=2 列 → [A B / C],列宽取列内最大,行高取行内最大。
    #[test]
    fn arrange_grid_positions() {
        let doc = three_artboards();
        let cmds = rearrange_cmds(&doc, Arrange::Grid, 80.0);
        let mut d = doc;
        let mut stack = UndoStack::new();
        stack.push(&mut d, Command::Compound { cmds }).unwrap();
        // 列宽:第 1 列 = max(1920, 375) = 1920;第 2 列 = 800
        // 行高:第 1 行 = max(1080, 600) = 1080
        assert_eq!((ab_geom(&d, 0).x, ab_geom(&d, 0).y), (0.0, 0.0));
        assert_eq!((ab_geom(&d, 1).x, ab_geom(&d, 1).y), (2000.0, 0.0));
        assert_eq!((ab_geom(&d, 2).x, ab_geom(&d, 2).y), (0.0, 1160.0));
    }

    // ── 门 2:适配图稿边界 ──

    /// 适配 = 画板几何收缩到内容包围盒;undo 逆回;空画板无命令。
    #[test]
    fn fit_artboard_to_content_bounds() {
        let mut doc = three_artboards();
        let sid = doc
            .nodes
            .get(doc.artboards[0])
            .unwrap()
            .sid
            .as_str()
            .to_string();
        let cmd = fit_artboard_cmd(&doc, &sid).unwrap();
        let mut stack = UndoStack::new();
        stack.push(&mut doc, cmd).unwrap();
        let g = ab_geom(&doc, 0);
        // 内容包围盒 (10,20)-(240,340) → 原点随之移动、尺寸收缩
        assert_eq!((g.x, g.y), (10.0, 20.0));
        assert_eq!((g.w, g.h), (230.0, 320.0));
        stack.undo(&mut doc).unwrap();
        let g = ab_geom(&doc, 0);
        assert_eq!(
            (g.x, g.y, g.w, g.h),
            (0.0, 0.0, 1920.0, 1080.0),
            "undo 逆回"
        );
        // 空画板 → None(不产生命令,面板给提示)
        let empty = doc
            .nodes
            .get(doc.artboards[1])
            .unwrap()
            .sid
            .as_str()
            .to_string();
        assert!(fit_artboard_cmd(&doc, &empty).is_none());
    }

    // ── 门 3:预设表常量 ──

    /// 预设表 = 7 档固定尺寸(Web 1920/1440/1080、移动 750/375、A4 横竖),
    /// 尺寸逐项断言;自定义由表外 W×H 承接。
    #[test]
    fn preset_table_has_seven_sizes() {
        assert_eq!(AB_PRESETS.len(), 7);
        let expect: [(&str, f64, f64); 7] = [
            ("Web 1920×1080", 1920.0, 1080.0),
            ("Web 1440×900", 1440.0, 900.0),
            ("Web 1080×1920", 1080.0, 1920.0),
            ("移动 750×1334", 750.0, 1334.0),
            ("移动 375×667", 375.0, 667.0),
            ("A4 竖 794×1123", 794.0, 1123.0),
            ("A4 横 1123×794", 1123.0, 794.0),
        ];
        assert_eq!(AB_PRESETS, expect);
        assert_eq!(PRESET_CUSTOM, "自定义");
    }

    // ── 02-5-2 复制画板(文档级) ──

    /// 复制:画板数 +1(排最后)、子树完整克隆(新 sid)、更名「副本」;
    /// undo 逆回。
    #[test]
    fn duplicate_artboard_clones_subtree_and_reverts() {
        let mut doc = three_artboards();
        let src = doc
            .nodes
            .get(doc.artboards[0])
            .unwrap()
            .sid
            .as_str()
            .to_string();
        let n_before = doc.nodes.len();
        let (cmd, new_sid) = duplicate_artboard_cmd(&mut doc, &src).unwrap();
        let mut stack = UndoStack::new();
        stack.push(&mut doc, cmd).unwrap();
        assert_eq!(doc.artboards.len(), 4);
        let copy = doc.nodes.get(doc.find_by_sid(&new_sid).unwrap()).unwrap();
        assert_eq!(copy.name, "A 副本");
        assert_eq!(copy.children.len(), 2, "内容子树整棵克隆");
        assert_eq!(doc.nodes.len(), n_before + 3, "画板 + 2 个内容节点");
        assert_eq!(
            doc.artboards.last().copied(),
            doc.find_by_sid(&new_sid),
            "副本排在最后"
        );
        stack.undo(&mut doc).unwrap();
        assert_eq!(doc.artboards.len(), 3);
        assert!(doc.find_by_sid(&new_sid).is_none());
    }
}
