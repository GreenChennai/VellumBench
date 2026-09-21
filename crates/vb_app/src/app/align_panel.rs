//! 对齐面板(`⇧F7`,副文档 03-5):对齐 6 键 + 分布 4 键 + **「对齐到」三选一**。
//!
//! 关键能力是「对齐到」(03-5-2/03-5-3):
//! - **选区**:对齐目标 = 选中对象的公共包围盒(AI 默认);
//! - **关键对象**:目标 = **最后选中**的那个对象(AI 的 key object),
//!   并在画布上给它的选中框**加粗**(见 `canvas.rs` 的选中框绘制);
//! - **画板**:目标 = 对象所属画板的本地框 `(0,0,w,h)`。
//!
//! 「纯函数边界」:目标盒(`align_target_box`)与间距分布(`space_targets`)
//! 都不碰 UI,便于逐条断言。二者的**几何内核在 `vb_tools::align`** ——
//! 与 Agent 的 `align` patch op 同源,避免"两处各算一份、参照系还不一样"。

use vb_common::geom::Rect;
use vb_doc::model::{Document, Geom};

use crate::app::VellumApp;

/// 「对齐到」三选一(**唯一实现在 `vb_tools::align`**,与 Agent patch 同源)。
pub use vb_tools::align::AlignTo;

/// 参与对齐的条目:`(sid, 当前 geom, 绝对 bbox)`。
pub type AlignItem = (String, Geom, Rect);

/// 收集选中对象的对齐条目(无 bbox 的对象跳过)。
pub fn collect_items(doc: &Document, selection: &[String]) -> Vec<AlignItem> {
    let mut out = Vec::new();
    for sid in selection {
        if let Some(nid) = doc.find_by_sid(sid) {
            if let Some(bb) = vb_tools::abs_bbox(doc, nid) {
                if let Some(n) = doc.nodes.get(nid) {
                    out.push((sid.clone(), n.geom, bb));
                }
            }
        }
    }
    out
}

/// 对齐目标盒(绝对系;**唯一实现在 `vb_tools::align`**,与 Agent patch 同源)。
///
/// - `Artboard`:取**最后一个**选中对象所属画板的本地框(对象 bbox 也是画板
///   本地系,两者同帧;跨画板时不混算);
/// - `KeyObject`:取最后选中对象的 bbox(关键对象不动,其余向它靠);
/// - `Selection`:多选 = 公共包围盒;**单选回退到所属画板** —— 对齐到"自己"
///   等于没做,而单选居中/贴边是高频手势,故保留本仓库既有语义。
pub fn align_target_box(
    doc: &Document,
    items: &[AlignItem],
    align_to: AlignTo,
) -> Option<vb_tools::align::AbsBox> {
    use vb_tools::align::AbsBox;
    let last = items.last()?;
    let boxes: Vec<AbsBox> = items
        .iter()
        .map(|(_, _, r)| AbsBox::from_rect(*r))
        .collect();
    match align_to {
        AlignTo::KeyObject => boxes.last().copied(),
        AlignTo::Artboard => artboard_box(doc, &last.0),
        AlignTo::Selection if items.len() >= 2 => AbsBox::union(&boxes),
        AlignTo::Selection => artboard_box(doc, &last.0),
    }
}

/// 某个成员所属画板的本地框 `(0,0,w,h)`。
fn artboard_box(doc: &Document, sid: &str) -> Option<vb_tools::align::AbsBox> {
    let nid = doc.find_by_sid(sid)?;
    let ab = vb_tools::artboard_of(doc, nid)?;
    let n = doc.nodes.get(ab)?;
    Some(vb_tools::align::AbsBox::new(0.0, 0.0, n.geom.w, n.geom.h))
}

/// 「分布间距」目标:首末不动,中间对象的**间隙**均匀(与"等距中心"不同)。
///
/// 返回与 `items` 同序的目标 x0(或 y0);不足 3 个 → `None`。
pub fn space_targets(items: &[AlignItem], horizontal: bool) -> Option<Vec<f64>> {
    let boxes: Vec<vb_tools::align::AbsBox> = items
        .iter()
        .map(|(_, _, r)| vb_tools::align::AbsBox::from_rect(*r))
        .collect();
    vb_tools::align::space_targets(&boxes, horizontal)
}

impl VellumApp {
    /// 「对齐到」切换(命令与面板下拉共用)。
    pub(crate) fn set_align_to(&mut self, a: AlignTo) {
        self.align_to = a;
        self.say(format!("对齐到:{}", a.label()));
    }

    /// 分布间距(03-5-1 的"分布间距 2 键")。
    pub(crate) fn distribute_space(&mut self, horizontal: bool) {
        let items = collect_items(&self.doc, &self.selection);
        let Some(targets) = space_targets(&items, horizontal) else {
            self.toast_warn("分布间距:需要至少选中 3 个对象");
            return;
        };
        let mut cmds: Vec<vb_doc::commands::Command> = Vec::new();
        for (i, (sid, g, bb)) in items.iter().enumerate() {
            let (cur, want) = if horizontal {
                (bb.x0, targets[i])
            } else {
                (bb.y0, targets[i])
            };
            let d = want - cur;
            if d.abs() < 0.5 {
                continue;
            }
            cmds.push(vb_doc::commands::Command::SetGeom {
                sid: sid.clone(),
                new: Geom {
                    x: if horizontal { g.x + d } else { g.x },
                    y: if horizontal { g.y } else { g.y + d },
                    w: g.w,
                    h: g.h,
                },
                old: None,
                old_declared: None,
            });
        }
        if cmds.is_empty() {
            self.say("分布间距:已均匀,无需移动");
            return;
        }
        let n = cmds.len();
        self.undo.merging_enabled = false;
        for c in cmds {
            self.exec(c);
        }
        self.undo.merging_enabled = true;
        self.say(format!(
            "分布间距:{} 个对象已等间隙({})",
            n,
            if horizontal { "水平" } else { "垂直" }
        ));
    }

    pub(crate) fn show_align_panel(&mut self, ui: &mut egui::Ui) {
        if !self.align_panel_open {
            return;
        }
        let mut open = true;
        let to = self.align_to;
        egui::Window::new("对齐")
            .open(&mut open)
            .collapsible(false)
            .default_width(232.0)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(vb_ui::components::caption(ui, "对齐到"));
                    egui::ComboBox::from_id_salt("vb-align-to")
                        .selected_text(to.label())
                        .show_ui(ui, |ui| {
                            for a in [AlignTo::Selection, AlignTo::KeyObject, AlignTo::Artboard] {
                                let hit = ui.selectable_label(to == a, a.label()).clicked();
                                if hit {
                                    self.align_to = a;
                                }
                            }
                        });
                });
                ui.separator();
                ui.label(vb_ui::components::caption(ui, "对齐(6 键)"));
                ui.horizontal(|ui| {
                    for (id, label) in [
                        ("align.left", "左"),
                        ("align.hcenter", "中"),
                        ("align.right", "右"),
                        ("align.top", "顶"),
                        ("align.vcenter", "中"),
                        ("align.bottom", "底"),
                    ] {
                        if ui.button(label).clicked() {
                            self.run_command(id, false, false);
                        }
                    }
                });
                ui.separator();
                ui.label(vb_ui::components::caption(ui, "分布"));
                ui.horizontal(|ui| {
                    if ui.button("水平等距").clicked() {
                        self.run_command("object.distribute_h", false, false);
                    }
                    if ui.button("垂直等距").clicked() {
                        self.run_command("object.distribute_v", false, false);
                    }
                });
                ui.horizontal(|ui| {
                    if ui.button("水平等间隙").clicked() {
                        self.run_command("object.distribute_hspace", false, false);
                    }
                    if ui.button("垂直等间隙").clicked() {
                        self.run_command("object.distribute_vspace", false, false);
                    }
                });
                if self.align_to == AlignTo::KeyObject {
                    ui.separator();
                    ui.label(vb_ui::components::caption(
                        ui,
                        "关键对象 = 最后选中者(选中框已加粗)。",
                    ));
                }
            });
        self.align_panel_open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::model::{Document, Geom, Node, NodeKind};

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    fn bx(x0: f64, y0: f64, x1: f64, y1: f64) -> vb_tools::align::AbsBox {
        vb_tools::align::AbsBox::new(x0, y0, x1, y1)
    }

    fn item(sid: &str, x: f64, y: f64, w: f64, h: f64) -> AlignItem {
        (
            sid.to_string(),
            Geom { x, y, w, h },
            rect(x, y, x + w, y + h),
        )
    }

    fn doc_with_ab(w: f64, h: f64) -> (Document, String) {
        let mut doc = Document::new_default();
        let ab = doc.artboards.first().copied().unwrap();
        doc.nodes.get_mut(ab).unwrap().geom = Geom {
            x: 0.0,
            y: 0.0,
            w,
            h,
        };
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, "盒", sid.clone());
        n.geom = Geom {
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 50.0,
        };
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        doc.nodes.get_mut(ab).unwrap().children.push(id);
        (doc, sid.as_str().to_string())
    }

    #[test]
    fn selection_target_is_common_bbox() {
        let items = vec![
            item("a", 0.0, 0.0, 10.0, 10.0),
            item("b", 90.0, 40.0, 10.0, 10.0),
        ];
        let t = align_target_box(&Document::new_default(), &items, AlignTo::Selection);
        assert_eq!(t, Some(bx(0.0, 0.0, 100.0, 50.0)));
    }

    /// 单选 + 「选区」→ 回退到所属画板(保留本仓库"单选居中/贴边"的既有手势)。
    #[test]
    fn single_selection_falls_back_to_artboard() {
        let (doc, sid) = doc_with_ab(800.0, 600.0);
        let items = vec![item(&sid, 5.0, 6.0, 10.0, 10.0)];
        let t = align_target_box(&doc, &items, AlignTo::Selection);
        assert_eq!(t, Some(bx(0.0, 0.0, 800.0, 600.0)));
        // 无画板归属(游离 sid)→ None(不猜)
        let orphan = vec![item("no-such-sid", 5.0, 6.0, 10.0, 10.0)];
        assert_eq!(align_target_box(&doc, &orphan, AlignTo::Selection), None);
    }

    #[test]
    fn key_object_target_is_last_selected() {
        let items = vec![
            item("a", 0.0, 0.0, 10.0, 10.0),
            item("b", 90.0, 40.0, 10.0, 10.0),
        ];
        let t = align_target_box(&Document::new_default(), &items, AlignTo::KeyObject);
        assert_eq!(
            t,
            Some(bx(90.0, 40.0, 100.0, 50.0)),
            "关键对象应取最后选中者"
        );
    }

    #[test]
    fn artboard_target_uses_owning_board() {
        let (doc, sid) = doc_with_ab(800.0, 600.0);
        let items = vec![item(&sid, 10.0, 10.0, 50.0, 50.0)];
        let t = align_target_box(&doc, &items, AlignTo::Artboard);
        assert_eq!(t, Some(bx(0.0, 0.0, 800.0, 600.0)));
    }

    #[test]
    fn space_targets_equalize_gaps() {
        // 三块 20 宽:x 0、40、100 → 首末不动,间隙各 30
        let items = vec![
            item("a", 0.0, 0.0, 20.0, 10.0),
            item("b", 40.0, 0.0, 20.0, 10.0),
            item("c", 100.0, 0.0, 20.0, 10.0),
        ];
        let t = space_targets(&items, true).expect("三块应可分布");
        assert_eq!(t, vec![0.0, 50.0, 100.0]);
        // 首末位置不变
        assert_eq!(t[0], items[0].2.x0);
        assert_eq!(t[2], items[2].2.x0);
    }

    #[test]
    fn space_targets_needs_three_items() {
        let items = vec![
            item("a", 0.0, 0.0, 20.0, 10.0),
            item("b", 40.0, 0.0, 20.0, 10.0),
        ];
        assert!(space_targets(&items, true).is_none());
    }

    #[test]
    fn space_targets_sorts_by_visual_order() {
        // 传入顺序打乱,分布仍按视觉顺序(首末不动)
        let items = vec![
            item("c", 100.0, 0.0, 20.0, 10.0),
            item("a", 0.0, 0.0, 20.0, 10.0),
            item("b", 40.0, 0.0, 20.0, 10.0),
        ];
        let t = space_targets(&items, true).unwrap();
        // 视觉序 = a(0) → b(40) → c(100),间隙均匀后 a=0、b=50、c=100;
        // 结果按**传入顺序**回填,故为 [c=100, a=0, b=50]
        assert_eq!(t, vec![100.0, 0.0, 50.0], "按视觉序首末不动");
    }
}
