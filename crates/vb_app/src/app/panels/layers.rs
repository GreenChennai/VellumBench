//! 右侧面板 · 图层 Tab(S1-a 自 app.rs 机械搬移;S1-d 02-4 **全面增强**)。
//!
//! 本轮新增(对照 `design/03 §5.1` 与 02 篇 §4-02-4):
//! - **搜索框**(02-4-7):按名过滤图层树,纯前端(不动文档);
//!   命中子节点的祖先行保持可见。
//! - **拖拽重排**(02-4-1):行拖拽换序,**含拖进编组/拖进其他画板**
//!   (目标行高亮:中带=进入,上/下缘=插到其前/后);落下经
//!   [`Command::Move`],跨父时补一条 [`Command::SetGeom`] 重定基,
//!   保持世界位置不变(层序操作不动画面);拖拽语义是纯函数
//!   [`drop_slot`] / [`move_cmds`],文档状态级测试覆盖。
//! - **Alt+拖拽 = 复制**(02-4-2):经 [`Command::Insert`](复制体重新
//!   分配 sid,与画布 Alt+拖动同一命令路径)。
//! - **颜色标记**(02-4-3):行首色块小圆点,点击循环取色;颜色作为
//!   `data-vb-mark` HTML 属性入文档(走 [`Command::SetAttrs`],导入/
//!   导出原样往返,不改模型 —— 语料库不受影响)。
//! - **右键菜单**(02-4-4):隔离(容器)/锁定其他/隐藏其他/选择同类/
//!   转换为编组;全部接真实命令路径。**不做"点了没反应"**:单节点
//!   导出(无单节点导出命令)、剪切蒙版(文档模型无节点间遮罩关系)
//!   不进菜单/底栏,登记遗留项(见 02d 报告)。
//! - **冻结块行**(02-4-6):❄ 图标 + 灰色斜体(`NodeKind::Frozen`,
//!   导入已有该概念)。
//! - 改名:双击行进入行内编辑(失焦/回车经 [`Command::Rename`] 落
//!   `data-vb-name`);底部操作行:定位对象(视口跳到选中节点,
//!   复用 `view.zoom_to_selection` 命令 ID)。

use egui::{Color32, Pos2, Rect, RichText, Sense};
use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeId, NodeKind};
use vb_ui::components::caption;
use vb_ui::icons::{self, Name};
use vb_ui::theme;

use crate::app::VellumApp;

/// 图层颜色标记调色板(AI 的图层着色八色;**文档内容色**,落盘为
/// `data-vb-mark` 属性值,不属 UI 皮肤 —— 硬编码棘轮按内容色语义登记)。
pub(crate) const MARK_COLORS: [&str; 8] = [
    "#e5484d", // vb-token-ok: 文档内容色(图层标记随文档落盘,非 UI 皮肤)
    "#f76b15", // vb-token-ok: 文档内容色(图层标记随文档落盘,非 UI 皮肤)
    "#ffb224", // vb-token-ok: 文档内容色(图层标记随文档落盘,非 UI 皮肤)
    "#46a758", // vb-token-ok: 文档内容色(图层标记随文档落盘,非 UI 皮肤)
    "#0090ff", // vb-token-ok: 文档内容色(图层标记随文档落盘,非 UI 皮肤)
    "#8e4ec6", // vb-token-ok: 文档内容色(图层标记随文档落盘,非 UI 皮肤)
    "#e93d82", // vb-token-ok: 文档内容色(图层标记随文档落盘,非 UI 皮肤)
    "#8a8a8a", // vb-token-ok: 文档内容色(图层标记随文档落盘,非 UI 皮肤)
];

/// 标记落盘用的 HTML 属性名(导入/导出原样往返;Agent 可用
/// `set_attr` 复现)。
pub(crate) const MARK_ATTR: &str = "data-vb-mark";

/// 行拖拽进行中的状态(S1-d 02-4-1/2)。
#[derive(Debug, Clone)]
pub(crate) struct LayerDrag {
    pub sid: String,
    /// Alt+拖拽 = 复制(落下走 Insert 命令)。
    pub dup: bool,
}

/// 拖拽落点语义(高亮与命令构造共用)。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum DropTarget {
    /// 放进目标容器(中带):追加为其子级末尾。
    Into { parent_sid: String, rect: Rect },
    /// 插到目标行之前/之后(上/下缘):目标行的父级 + 行索引。
    Edge {
        parent_sid: String,
        index: usize,
        /// 插入指示线(y 坐标与横向范围)。
        line_y: f32,
        x0: f32,
        x1: f32,
    },
}

/// 一行的命中区(逐帧收集,拖拽落点判定用;pub(crate) 供测试构造)。
pub(crate) struct DropZone {
    pub rect: Rect,
    pub sid: String,
    pub parent_sid: String,
    pub container: bool,
    pub index: usize,
}

/// 一行的渲染描述(先收集后渲染,避免渲染循环里同时借文档与面板)。
struct RowDesc {
    sid: String,
    name: String,
    icon: Name,
    frozen: bool,
    container: bool,
    is_artboard: bool,
    hidden: bool,
    locked: bool,
    has_children: bool,
    expanded: bool,
    depth: u8,
    parent_sid: Option<String>,
    index: usize,
    mark: Option<String>,
}

// ───────────────────────── 纯函数(文档状态级测试覆盖) ─────────────────────────

/// 颜色标记循环:无标记 → 第一色;末色 → 清除。
pub(crate) fn cycle_mark(cur: Option<&str>) -> Option<&'static str> {
    match cur.and_then(|c| MARK_COLORS.iter().position(|&m| m == c)) {
        None => Some(MARK_COLORS[0]),
        Some(i) if i + 1 < MARK_COLORS.len() => Some(MARK_COLORS[i + 1]),
        Some(_) => None,
    }
}

/// 节点(或其任一后代)名称是否命中搜索词(大小写不敏感;空词全过)。
pub(crate) fn matches_query(doc: &Document, id: NodeId, q: &str) -> bool {
    let ql = q.trim().to_lowercase();
    if ql.is_empty() {
        return true;
    }
    let Some(n) = doc.nodes.get(id) else {
        return false;
    };
    n.name.to_lowercase().contains(&ql) || subtree_name_hit(doc, id, &ql)
}

/// 后代子树里是否有名称命中(祖先行保持可见的依据)。
fn subtree_name_hit(doc: &Document, id: NodeId, ql: &str) -> bool {
    let Some(n) = doc.nodes.get(id) else {
        return false;
    };
    n.children.iter().any(|&c| {
        doc.nodes
            .get(c)
            .is_some_and(|cn| cn.name.to_lowercase().contains(ql))
            || subtree_name_hit(doc, c, ql)
    })
}

/// 「其他」节点集:全部画板子树中,除目标自身/祖先/后代之外的所有节点
/// (锁定其他 / 隐藏其他的作用域;root 不在画板子树内,天然排除)。
pub(crate) fn others_of(doc: &Document, sid: &str) -> Vec<String> {
    let Some(tid) = doc.find_by_sid(sid) else {
        return vec![];
    };
    let mut all = Vec::new();
    for &ab in &doc.artboards {
        doc.subtree(ab, &mut all);
    }
    // 画板节点是画布不是对象:锁定/隐藏其他不波及画板本身
    all.retain(|&id| {
        id != tid
            && !doc.is_descendant_or_self(tid, id)
            && !doc.is_descendant_or_self(id, tid)
            && !doc
                .nodes
                .get(id)
                .is_some_and(|n| matches!(n.kind, NodeKind::Artboard))
    });
    all.into_iter()
        .filter_map(|id| doc.nodes.get(id).map(|n| n.sid.as_str().to_string()))
        .collect()
}

/// 「选择同类」:与目标同 kind 的全部节点 sid(跨画板;root 除外)。
pub(crate) fn same_kind_sids(doc: &Document, sid: &str) -> Vec<String> {
    let Some(tid) = doc.find_by_sid(sid) else {
        return vec![];
    };
    let kind = doc.nodes.get(tid).map(|n| n.kind.kind_name());
    let mut all = Vec::new();
    for &ab in &doc.artboards {
        doc.subtree(ab, &mut all);
    }
    all.into_iter()
        .filter(|&id| {
            doc.nodes
                .get(id)
                .is_some_and(|n| Some(n.kind.kind_name()) == kind)
        })
        .map(|id| doc.nodes.get(id).unwrap().sid.as_str().to_string())
        .collect()
}

/// 转换为编组(02-4-4):把单个节点包进新编组(单成员 [`Command::Group`],
/// 现有命令路径;画板/无父节点不支持,返回 None)。
pub(crate) fn wrap_in_group_cmd(doc: &mut Document, sid: &str) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    if matches!(doc.nodes.get(nid)?.kind, NodeKind::Artboard) {
        return None;
    }
    doc.nodes.get(nid)?.parent?;
    let group_sid = doc.alloc_sid();
    Some(Command::Group {
        member_sids: vec![sid.to_string()],
        name: format!("编组 {}", group_sid.as_str()),
        group_sid: group_sid.as_str().to_string(),
        old_slots: None,
    })
}

/// Alt 复制落点命令(02-4-2):克隆子树 → 重新分配 sid → 插入目标槽位;
/// 返回 (命令, 新 sid)。与画布 Alt+拖动同一条 Insert 命令路径。
pub(crate) fn dup_insert_cmd(
    doc: &mut Document,
    sid: &str,
    parent_sid: &str,
    index: usize,
) -> Option<(Command, String)> {
    let nid = doc.find_by_sid(sid)?;
    let fallback = vb_doc::model::NodeTree {
        node: doc.nodes.get(nid)?.clone(),
        children: vec![],
    };
    let mut tree = vb_doc::model::NodeTree::from_document(doc, nid).unwrap_or(fallback);
    crate::app::re_sid_tree(&mut tree, doc);
    let new_sid = tree.node.sid.as_str().to_string();
    Some((
        Command::Insert {
            parent_sid: parent_sid.to_string(),
            index,
            tree,
        },
        new_sid,
    ))
}

/// 节点相对所属画板的累计偏移(严格祖先链 geom 之和,不含自身与画板)。
fn chain_offset(doc: &Document, id: NodeId) -> (f64, f64) {
    let mut dx = 0.0;
    let mut dy = 0.0;
    let mut cur = id;
    while let Some(p) = doc.nodes.get(cur).and_then(|n| n.parent) {
        let Some(pn) = doc.nodes.get(p) else {
            break;
        };
        if matches!(pn.kind, NodeKind::Artboard) {
            break;
        }
        dx += pn.geom.x;
        dy += pn.geom.y;
        cur = p;
    }
    (dx, dy)
}

/// 父级锚点:子级本地 (0,0) 映射到的世界坐标(画板父级 = 画板原点;
/// 其余 = 画板原点 + 祖先链 + 父级自身 geom)。
fn parent_anchor(doc: &Document, p: NodeId) -> (f64, f64) {
    let Some(pn) = doc.nodes.get(p) else {
        return (0.0, 0.0);
    };
    if matches!(pn.kind, NodeKind::Artboard) {
        return (pn.geom.x, pn.geom.y);
    }
    let (cx, cy) = chain_offset(doc, p);
    let ab = vb_tools::artboard_of(doc, p)
        .and_then(|a| doc.nodes.get(a))
        .map(|a| (a.geom.x, a.geom.y))
        .unwrap_or((0.0, 0.0));
    (ab.0 + cx + pn.geom.x, ab.1 + cy + pn.geom.y)
}

/// 拖拽落下的命令序列(02-4-1):同父 = 纯 Move 重排;跨父 = Move +
/// SetGeom 重定基(保持世界位置,层序操作不动画面)。
pub(crate) fn move_cmds(
    doc: &Document,
    sid: &str,
    new_parent_sid: &str,
    new_index: usize,
) -> Vec<Command> {
    let Some(nid) = doc.find_by_sid(sid) else {
        return vec![];
    };
    let Some(np) = doc.find_by_sid(new_parent_sid) else {
        return vec![];
    };
    let move_cmd = Command::Move {
        sid: sid.to_string(),
        new_parent_sid: new_parent_sid.to_string(),
        new_index,
        old: None,
    };
    let Some(old_np) = doc.nodes.get(nid).and_then(|n| n.parent) else {
        return vec![move_cmd];
    };
    if old_np == np {
        return vec![move_cmd];
    }
    // 跨父:世界位置保持(旧锚点 + 原本地 → 新锚点系)
    let g = doc.nodes.get(nid).unwrap().geom;
    let old_anchor = parent_anchor(doc, old_np);
    let new_anchor = parent_anchor(doc, np);
    vec![
        move_cmd,
        Command::SetGeom {
            sid: sid.to_string(),
            new: vb_doc::model::Geom {
                x: world_of(old_anchor, g).0 - new_anchor.0,
                y: world_of(old_anchor, g).1 - new_anchor.1,
                w: g.w,
                h: g.h,
            },
            old: None,
            old_declared: None,
        },
    ]
}

fn world_of(anchor: (f64, f64), g: vb_doc::model::Geom) -> (f64, f64) {
    (anchor.0 + g.x, anchor.1 + g.y)
}

/// 由指针位置与命中区计算拖拽落点(纯函数;含自环/入自身子树守卫)。
pub(crate) fn drop_slot(
    doc: &Document,
    zones: &[DropZone],
    pos: Option<Pos2>,
    drag_sid: &str,
) -> Option<DropTarget> {
    let pos = pos?;
    let drag_id = doc.find_by_sid(drag_sid)?;
    for z in zones {
        if !z.rect.contains(pos) {
            continue;
        }
        // 放在自己行上 = 无操作;放进自身子树 = 拒绝(场景图成环)
        let zone_id = doc.find_by_sid(&z.sid)?;
        if z.sid == drag_sid || doc.is_descendant_or_self(drag_id, zone_id) {
            return None;
        }
        let band = z.rect.height() * 0.25;
        // 画板只能在其父级(root)内重排,不能「进入」其他容器
        // (画板嵌进画板会脱离 artboards 注册表与导出序)
        let drag_is_artboard = doc
            .nodes
            .get(drag_id)
            .is_some_and(|n| matches!(n.kind, NodeKind::Artboard));
        if z.container
            && !drag_is_artboard
            && pos.y > z.rect.top() + band
            && pos.y < z.rect.bottom() - band
        {
            return Some(DropTarget::Into {
                parent_sid: z.sid.clone(),
                rect: z.rect,
            });
        }
        let before = pos.y < z.rect.center().y;
        return Some(DropTarget::Edge {
            parent_sid: z.parent_sid.clone(),
            index: if before { z.index } else { z.index + 1 },
            line_y: if before {
                z.rect.top()
            } else {
                z.rect.bottom()
            },
            x0: z.rect.left(),
            x1: z.rect.right(),
        });
    }
    None
}

/// 颜色标记命令(data-vb-mark 属性 upsert/删除;走 SetAttrs 可逆路径)。
fn mark_cmd(doc: &Document, sid: &str, mark: Option<String>) -> Command {
    let merged: Vec<(String, String)> = doc
        .find_by_sid(sid)
        .and_then(|id| doc.nodes.get(id))
        .map(|n| {
            n.attrs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut merged = merged;
    match mark {
        Some(c) => match merged.iter_mut().find(|(k, _)| k == MARK_ATTR) {
            Some(slot) => slot.1 = c,
            None => merged.push((MARK_ATTR.to_string(), c)),
        },
        None => merged.retain(|(k, _)| k != MARK_ATTR),
    }
    Command::SetAttrs {
        sid: sid.to_string(),
        new: merged,
        old: None,
    }
}

// ───────────────────────────────── 渲染 ─────────────────────────────────

impl VellumApp {
    pub(crate) fn layers_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("图层");
        ui.separator();

        // --- 搜索框(02-4-7;纯前端过滤) ---
        ui.horizontal(|ui| {
            ui.monospace("🔍");
            ui.add_sized(
                [ui.available_width(), 18.0],
                egui::TextEdit::singleline(&mut self.layer_search).hint_text("搜索图层名"),
            );
        });
        let query = self.layer_search.clone();

        // --- 收集行描述(先收集后渲染,免借用冲突) ---
        let rows = self.collect_rows(&query);
        let mut zones: Vec<DropZone> = Vec::new();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for r in &rows {
                self.layer_row_ui(ui, r, &mut zones);
            }
            if rows.is_empty() {
                ui.label(caption(ui, "没有匹配的图层(清空搜索框恢复)"));
            }
            self.layer_drag_overlay(ui, &zones);
        });

        // --- 底部操作行(02-4-5) ---
        // 剪切蒙版 / 单节点导出:文档模型暂无命令支撑,不放假按钮
        // (登记遗留项,见 02d 报告)。
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("⌖ 定位对象").clicked() {
                if self.selection.is_empty() {
                    self.say("定位对象:先在图层树选中一个节点");
                } else {
                    self.run_command("view.zoom_to_selection", false, false);
                }
            }
        });
    }

    /// 拖拽进行中:落点高亮 + 松手落下(02-4-1/2)。
    fn layer_drag_overlay(&mut self, ui: &mut egui::Ui, zones: &[DropZone]) {
        if self.layer_drag.is_none() {
            return;
        }
        let pos = ui.input(|i| i.pointer.interact_pos());
        let target = self
            .layer_drag
            .as_ref()
            .and_then(|d| drop_slot(&self.doc, zones, pos, &d.sid));
        let t = theme::tokens(ui.ctx());
        match &target {
            Some(DropTarget::Into { rect, .. }) => {
                ui.painter()
                    .rect_filled(*rect, theme::radius::sm(), t.accent.gamma_multiply(0.22));
                ui.painter().rect_stroke(
                    *rect,
                    theme::radius::sm(),
                    egui::Stroke::new(1.5, t.accent),
                    egui::StrokeKind::Inside,
                );
            }
            Some(DropTarget::Edge { line_y, x0, x1, .. }) => {
                ui.painter().line_segment(
                    [Pos2::new(*x0, *line_y), Pos2::new(*x1, *line_y)],
                    egui::Stroke::new(2.0, t.accent),
                );
            }
            None => {}
        }
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(drag) = self.layer_drag.take() {
                self.finish_layer_drop(&drag, target);
            }
        }
    }

    /// 收集全部画板子树的行描述(搜索过滤 + 展开/收起)。
    fn collect_rows(&self, query: &str) -> Vec<RowDesc> {
        let filtering = !query.trim().is_empty();
        let mut out = Vec::new();
        for &ab in &self.doc.artboards {
            self.push_rows(ab, 0, query, filtering, &mut out);
        }
        out
    }

    fn push_rows(
        &self,
        id: NodeId,
        depth: u8,
        query: &str,
        filtering: bool,
        out: &mut Vec<RowDesc>,
    ) {
        let Some(n) = self.doc.nodes.get(id) else {
            return;
        };
        let parent_sid = n
            .parent
            .and_then(|p| self.doc.nodes.get(p))
            .map(|p| p.sid.as_str().to_string());
        let index = n
            .parent
            .and_then(|p| self.doc.nodes.get(p))
            .map(|p| p.children.iter().position(|&c| c == id).unwrap_or(0))
            .unwrap_or(0);
        if !filtering || matches_query(&self.doc, id, query) {
            let kind = &n.kind;
            out.push(RowDesc {
                sid: n.sid.as_str().to_string(),
                name: n.name.clone(),
                icon: kind_icon(kind),
                frozen: matches!(kind, NodeKind::Frozen { .. }),
                container: kind.is_container(),
                is_artboard: matches!(kind, NodeKind::Artboard),
                hidden: n.hidden,
                locked: n.locked,
                has_children: !n.children.is_empty(),
                expanded: filtering || !self.layer_expanded.contains(n.sid.as_str()),
                depth,
                parent_sid,
                index,
                mark: n.attrs.get(MARK_ATTR).cloned(),
            });
        }
        // 收起态不递归(搜索态强制展开命中路径)
        let collapsed = !filtering && self.layer_expanded.contains(n.sid.as_str());
        if !n.children.is_empty() && !collapsed {
            for &c in &n.children {
                self.push_rows(c, depth + 1, query, filtering, out);
            }
        }
    }

    /// 渲染一行 + 收集命中区 + 处理交互(选中/改名/显隐/锁定/层序/
    /// 拖拽起手/颜色标记/右键菜单)。
    fn layer_row_ui(&mut self, ui: &mut egui::Ui, r: &RowDesc, zones: &mut Vec<DropZone>) {
        let t = theme::tokens(ui.ctx());
        let selected = self.selection.last().is_some_and(|s| *s == r.sid);
        let editing = self.editing_layer.as_deref() == Some(r.sid.as_str());
        let indent = theme::space::S2 + r.depth as f32 * theme::space::S6;
        let (rect, resp) = ui.allocate_exact_size(
            egui::Vec2::new(ui.available_width(), theme::space::ROW_HEIGHT),
            Sense::click_and_drag(),
        );
        zones.push(DropZone {
            rect,
            sid: r.sid.clone(),
            parent_sid: r.parent_sid.clone().unwrap_or_default(),
            container: r.container,
            index: r.index,
        });

        // 行底:选中 > 冻结淡底 > 悬停
        let hover_t = ui.ctx().animate_bool_with_time(
            ui.id().with(("vblayer", &r.sid)),
            resp.hovered() && !selected,
            theme::motion::HOVER,
        );
        let fill = if selected {
            t.accent_dim
        } else if r.frozen {
            theme::semantic::frozen_fill(self.theme_dark)
        } else {
            blend(Color32::TRANSPARENT, t.bg_hover, hover_t)
        };
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(theme::space::S1, 1.0)),
            theme::radius::sm(),
            fill,
        );

        // 布局:缩进 | 展开箭头 | 类型图标 | 标记点 | 名称 | ↑ ↓ 👁 🔒
        let center = rect.center().y;
        let right_edge = rect.right() - theme::space::S2;
        let btn_w = 18.0;
        let lock_x = right_edge - btn_w * 0.5;
        let eye_x = lock_x - btn_w;
        let down_x = eye_x - btn_w;
        let up_x = down_x - btn_w;
        let mut x = rect.left() + indent;

        // 展开箭头(有子级才画;点击切换)
        if r.has_children {
            let chev = if r.expanded {
                Name::Expanded
            } else {
                Name::Collapsed
            };
            let crect = Rect::from_min_size(Pos2::new(x, rect.top()), egui::Vec2::splat(14.0));
            ui.painter().text(
                crect.center(),
                egui::Align2::CENTER_CENTER,
                chev.glyph().to_string(),
                icons::font(11.0),
                t.text_3,
            );
            let crep = ui.interact(crect, ui.id().with(("vbchev", &r.sid)), Sense::click());
            if crep.clicked() {
                if r.expanded {
                    self.layer_expanded.insert(r.sid.clone());
                } else {
                    self.layer_expanded.remove(&r.sid);
                }
            }
            crep.on_hover_text(if r.expanded { "收起" } else { "展开" });
        }
        x += 15.0;

        // 类型图标(冻结块 = ❄)
        ui.painter().text(
            Pos2::new(x + 7.0, center),
            egui::Align2::CENTER_CENTER,
            r.icon.glyph().to_string(),
            icons::font(12.0),
            if r.frozen { t.text_3 } else { t.text_2 },
        );
        x += 17.0;

        // 颜色标记点(02-4-3):点击循环取色,入文档(data-vb-mark)
        let drect = Rect::from_center_size(Pos2::new(x + 4.0, center), egui::Vec2::splat(10.0));
        let mark_color = r
            .mark
            .as_deref()
            .and_then(vb_common::color::parse_color)
            .map(|c| Color32::from_rgb(c.r, c.g, c.b));
        let _ = ui.painter().circle_filled(
            drect.center(),
            3.5,
            mark_color.unwrap_or(Color32::TRANSPARENT),
        );
        if mark_color.is_none() {
            ui.painter()
                .circle_stroke(drect.center(), 3.5, egui::Stroke::new(1.0, t.text_3));
        }
        let drep = ui.interact(drect, ui.id().with(("vbmark", &r.sid)), Sense::click());
        if drep.clicked() {
            let next = cycle_mark(r.mark.as_deref()).map(str::to_string);
            let said = match &next {
                Some(c) => format!("颜色标记 → {c}(data-vb-mark,已入文档)"),
                None => "颜色标记已清除".to_string(),
            };
            let cmd = mark_cmd(&self.doc, &r.sid, next);
            self.exec(cmd);
            self.say(said);
        }
        drep.on_hover_text("图层颜色标记(点击循环;颜色随文档保存)");
        x += 12.0;

        // 名称(冻结块 = 灰色斜体;双击行内改名)
        let name_rect = Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new(up_x - btn_w * 0.6, rect.bottom()),
        );
        if editing {
            let mut buf = r.name.clone();
            let edit = ui
                .new_child(egui::UiBuilder::new().max_rect(name_rect))
                .add(egui::TextEdit::singleline(&mut buf).desired_width(name_rect.width()));
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if edit.lost_focus() || enter {
                self.editing_layer = None;
                let trimmed = buf.trim().to_string();
                if !trimmed.is_empty() && trimmed != r.name {
                    self.exec(Command::Rename {
                        sid: r.sid.clone(),
                        new: trimmed,
                        old: None,
                    });
                    self.say("已重命名(data-vb-name 同步)");
                }
            }
        } else {
            let mut text = RichText::new(&r.name).font(egui::FontId::proportional(12.5));
            text = if r.frozen {
                text.italics().color(t.text_3)
            } else if r.hidden {
                text.color(t.text_3)
            } else {
                text.color(t.text)
            };
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_rect));
            child.add(egui::Label::new(text).truncate().selectable(false));
        }

        // 右侧开关:↑ ↓ 👁 🔒(既有行为保持)
        if self.icon_hit(
            ui,
            up_x,
            center,
            btn_w,
            &r.sid,
            "vbup",
            Name::MoveUp,
            t.text_3,
            "前移一层",
        ) {
            self.reorder(&r.sid, 1);
        }
        if self.icon_hit(
            ui,
            down_x,
            center,
            btn_w,
            &r.sid,
            "vbdown",
            Name::MoveDown,
            t.text_3,
            "后移一层",
        ) {
            self.reorder(&r.sid, -1);
        }
        let eye_icon = if r.hidden {
            Name::Hidden
        } else {
            Name::Visible
        };
        let eye_color = if r.hidden { t.warn } else { t.text_3 };
        let eye_tip = if r.hidden {
            "显示(取消隐藏)"
        } else {
            "隐藏"
        };
        if self.icon_hit(
            ui, eye_x, center, btn_w, &r.sid, "vbeye", eye_icon, eye_color, eye_tip,
        ) {
            self.exec(Command::SetFlags {
                sid: r.sid.clone(),
                hidden: Some(!r.hidden),
                locked: None,
                old: None,
            });
        }
        let lock_icon = if r.locked {
            Name::Locked
        } else {
            Name::Unlocked
        };
        let lock_color = if r.locked { t.warn } else { t.text_3 };
        let lock_tip = if r.locked { "解锁" } else { "锁定" };
        if self.icon_hit(
            ui, lock_x, center, btn_w, &r.sid, "vblock", lock_icon, lock_color, lock_tip,
        ) {
            self.exec(Command::SetFlags {
                sid: r.sid.clone(),
                hidden: None,
                locked: Some(!r.locked),
                old: None,
            });
        }

        // 行交互:点击选中 / 双击改名 / 拖拽起手 / 右键菜单
        if resp.clicked() {
            self.selection = vec![r.sid.clone()];
        }
        if resp.double_clicked() {
            self.editing_layer = Some(r.sid.clone());
        }
        if resp.drag_started() {
            let dup = ui.input(|i| i.modifiers.alt);
            self.layer_drag = Some(LayerDrag {
                sid: r.sid.clone(),
                dup,
            });
            if dup {
                self.say("Alt+拖拽:复制到目标位置");
            }
        }
        if r.frozen {
            resp.clone()
                .on_hover_text("冻结块:含不支持的 CSS,原样保留(❄)");
        }
        self.layer_context_menu(ui, &resp, r);
    }

    /// 行内右侧小图标命中区(手绘 + 点击;与行主体点击互不干扰)。
    #[allow(clippy::too_many_arguments)]
    fn icon_hit(
        &mut self,
        ui: &mut egui::Ui,
        cx: f32,
        cy: f32,
        size: f32,
        sid: &str,
        key: &'static str,
        icon: Name,
        color: Color32,
        tip: &str,
    ) -> bool {
        let r = Rect::from_center_size(Pos2::new(cx, cy), egui::Vec2::splat(size));
        let resp = ui.interact(r, ui.id().with((key, sid)), Sense::click());
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            icon.glyph().to_string(),
            icons::font(13.0),
            color,
        );
        let clicked = resp.clicked();
        resp.on_hover_text(tip);
        clicked
    }

    /// 右键菜单(02-4-4):全部接真实命令路径;无支撑的项不出现。
    fn layer_context_menu(&mut self, _ui: &mut egui::Ui, resp: &egui::Response, r: &RowDesc) {
        resp.context_menu(|ui| {
            // 隔离(仅容器;复用既有隔离模式栈)
            if r.container && ui.button("隔离(进入隔离模式)").clicked() {
                if let Some(nid) = self.doc.find_by_sid(&r.sid) {
                    self.isolate_stack.push(nid);
                    self.selection = vec![r.sid.clone()];
                    self.say(format!("已进入隔离模式:{}(Esc 退出)", r.name));
                }
                ui.close();
            }
            if ui.button("锁定其他").clicked() {
                let others = others_of(&self.doc, &r.sid);
                if others.is_empty() {
                    self.say("锁定其他:没有其他对象");
                } else {
                    let n = others.len();
                    let cmds = others
                        .into_iter()
                        .map(|sid| Command::SetFlags {
                            sid,
                            hidden: None,
                            locked: Some(true),
                            old: None,
                        })
                        .collect();
                    self.exec(Command::Compound { cmds });
                    self.say(format!("已锁定其他 {n} 个对象"));
                }
                ui.close();
            }
            if ui.button("隐藏其他").clicked() {
                let others = others_of(&self.doc, &r.sid);
                if others.is_empty() {
                    self.say("隐藏其他:没有其他对象");
                } else {
                    let n = others.len();
                    let cmds = others
                        .into_iter()
                        .map(|sid| Command::SetFlags {
                            sid,
                            hidden: Some(true),
                            locked: None,
                            old: None,
                        })
                        .collect();
                    self.exec(Command::Compound { cmds });
                    self.selection = vec![r.sid.clone()];
                    self.say(format!("已隐藏其他 {n} 个对象"));
                }
                ui.close();
            }
            if ui.button("选择同类").clicked() {
                let sids = same_kind_sids(&self.doc, &r.sid);
                let n = sids.len();
                self.selection = sids;
                self.say(format!("已选择同类 {n} 个对象"));
                ui.close();
            }
            if !r.is_artboard && ui.button("转换为编组").clicked() {
                let mut doc = std::mem::take(&mut self.doc);
                let cmd = wrap_in_group_cmd(&mut doc, &r.sid);
                let gsid = cmd.as_ref().and_then(|c| match c {
                    Command::Group { group_sid, .. } => Some(group_sid.clone()),
                    _ => None,
                });
                self.doc = doc;
                match cmd {
                    Some(c) => {
                        self.exec(c);
                        if let Some(g) = gsid {
                            self.selection = vec![g];
                        }
                        self.say("已转换为编组");
                    }
                    None => self.say("转换为编组:该对象不支持"),
                }
                ui.close();
            }
        });
    }

    /// 拖拽落下(02-4-1/2):按落点语义构造命令并执行。
    fn finish_layer_drop(&mut self, drag: &LayerDrag, target: Option<DropTarget>) {
        let Some(target) = target else {
            return;
        };
        let (parent_sid, index) = match &target {
            DropTarget::Into { parent_sid, .. } => {
                let len = self
                    .doc
                    .find_by_sid(parent_sid)
                    .map(|pid| self.doc.nodes.get(pid).unwrap().children.len())
                    .unwrap_or(0);
                (parent_sid.clone(), len)
            }
            DropTarget::Edge {
                parent_sid, index, ..
            } => (parent_sid.clone(), *index),
        };
        if drag.dup {
            // Alt = 复制:克隆子树插入目标槽位(+ 跨父重定基)
            let mut doc = std::mem::take(&mut self.doc);
            let dup = dup_insert_cmd(&mut doc, &drag.sid, &parent_sid, index);
            self.doc = doc;
            if let Some((insert, new_sid)) = dup {
                let mut cmds = vec![insert];
                cmds.extend(rebase_for_new_parent(
                    &self.doc,
                    &drag.sid,
                    &new_sid,
                    &parent_sid,
                ));
                self.exec(Command::Compound { cmds });
                self.selection = vec![new_sid];
                self.say("已复制到目标位置(Alt+拖拽)");
            }
        } else {
            let cmds = move_cmds(&self.doc, &drag.sid, &parent_sid, index);
            let moved = match cmds.len() {
                0 => false,
                1 => {
                    self.exec(cmds.into_iter().next().unwrap());
                    true
                }
                _ => {
                    self.exec(Command::Compound { cmds });
                    true
                }
            };
            if moved {
                self.say(if matches!(target, DropTarget::Into { .. }) {
                    "已移入目标位置(世界位置保持)"
                } else {
                    "已调整层序"
                });
            }
            self.selection = vec![drag.sid.clone()];
        }
    }
}

/// 复制落点后的重定基命令(Alt 复制跨父时保持世界位置)。
fn rebase_for_new_parent(
    doc: &Document,
    src_sid: &str,
    new_sid: &str,
    parent_sid: &str,
) -> Vec<Command> {
    let (Some(src), Some(np)) = (doc.find_by_sid(src_sid), doc.find_by_sid(parent_sid)) else {
        return vec![];
    };
    let Some(src_parent) = doc.nodes.get(src).and_then(|n| n.parent) else {
        return vec![];
    };
    if src_parent == np {
        return vec![];
    }
    let g = doc.nodes.get(src).unwrap().geom;
    let old_anchor = parent_anchor(doc, src_parent);
    let new_anchor = parent_anchor(doc, np);
    vec![Command::SetGeom {
        sid: new_sid.to_string(),
        new: vb_doc::model::Geom {
            x: old_anchor.0 + g.x - new_anchor.0,
            y: old_anchor.1 + g.y - new_anchor.1,
            w: g.w,
            h: g.h,
        },
        old: None,
        old_declared: None,
    }]
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

fn kind_icon(kind: &NodeKind) -> Name {
    match kind {
        NodeKind::Artboard => Name::KindArtboard,
        NodeKind::Layer => Name::KindLayer,
        NodeKind::Group => Name::KindGroup,
        NodeKind::Box => Name::KindBox,
        NodeKind::Text { .. } => Name::KindText,
        NodeKind::Image { .. } => Name::KindImage,
        NodeKind::Vector { .. } => Name::KindVector,
        NodeKind::Slice => Name::KindSlice,
        NodeKind::Frozen { .. } => Name::KindFrozen,
    }
}

// ═══════════════════════════ 测试(文档状态级) ═══════════════════════════
//
// 门 1(02d 验收):重排/跨编组拖拽、Alt 复制、颜色标记、右键菜单各项
// 均有**文档状态级**测试 —— 经命令路径(UndoStack::push)断言 Document
// 结构/层序/标记落盘;UI 手势层的落点判定用纯函数 drop_slot 断言。

#[cfg(test)]
mod tests {
    use vb_doc::model::{Geom, Node, NodeKind};
    use vb_doc::undo::UndoStack;

    use super::*;

    fn add_box(doc: &mut Document, parent: &str, name: &str, x: f64, y: f64) -> String {
        let sid = doc.alloc_sid();
        let mut n = Node::new(NodeKind::Box, name, sid.clone());
        n.geom = Geom {
            x,
            y,
            w: 100.0,
            h: 50.0,
        };
        let pid = doc.find_by_sid(parent).unwrap();
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(pid);
        doc.nodes.get_mut(pid).unwrap().children.push(id);
        sid.as_str().to_string()
    }

    fn add_group(doc: &mut Document, parent: &str, name: &str) -> String {
        let sid = doc.alloc_sid();
        let n = Node::new(NodeKind::Group, name, sid.clone());
        let pid = doc.find_by_sid(parent).unwrap();
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(pid);
        doc.nodes.get_mut(pid).unwrap().children.push(id);
        sid.as_str().to_string()
    }

    /// 夹具:画板 AB1(组 G 内含 g1;同级 a、b)+ 画板 AB2(原点 y=1000,盒 c)。
    struct Fx {
        doc: Document,
        ab1: String,
        ab2: String,
        g: String,
        g1: String,
        a: String,
        b: String,
        c: String,
    }

    fn fx() -> Fx {
        let mut doc = Document::new_default();
        let ab1 = doc
            .nodes
            .get(doc.artboards[0])
            .unwrap()
            .sid
            .as_str()
            .to_string();
        let ab2 = doc.new_artboard("画板 2", 800.0, 600.0);
        doc.nodes.get_mut(ab2).unwrap().geom.y = 1000.0;
        let ab2 = doc.nodes.get(ab2).unwrap().sid.as_str().to_string();
        let g = add_group(&mut doc, &ab1, "组");
        let g1 = add_box(&mut doc, &g, "g1", 10.0, 10.0);
        let a = add_box(&mut doc, &ab1, "盒A", 5.0, 200.0);
        let b = add_box(&mut doc, &ab1, "盒B", 300.0, 200.0);
        let c = add_box(&mut doc, &ab2, "盒C", 50.0, 60.0);
        Fx {
            doc,
            ab1,
            ab2,
            g,
            g1,
            a,
            b,
            c,
        }
    }

    fn children_of(doc: &Document, sid: &str) -> Vec<String> {
        doc.find_by_sid(sid)
            .map(|id| {
                doc.nodes
                    .get(id)
                    .unwrap()
                    .children
                    .iter()
                    .map(|&c| doc.nodes.get(c).unwrap().sid.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn parent_of(doc: &Document, sid: &str) -> Option<String> {
        doc.find_by_sid(sid)
            .and_then(|id| doc.nodes.get(id))
            .and_then(|n| n.parent)
            .map(|p| doc.nodes.get(p).unwrap().sid.as_str().to_string())
    }

    fn geom_of(doc: &Document, sid: &str) -> Geom {
        doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().geom
    }

    // ── 02-4-1 拖拽重排(命令层等价路径) ──

    /// 同父重排:落点 = 末行下缘(index=末位)→ 单条 Move;
    /// 层序变化,undo 精确逆回。
    #[test]
    fn drop_edge_reorders_within_parent_and_reverts() {
        let mut f = fx();
        // ab1 子序 [g, a, b];把 a 落到 b 下缘 → index=3 → [g, b, a]
        let cmds = move_cmds(&f.doc, &f.a, &f.ab1, 3);
        assert_eq!(cmds.len(), 1, "同父重排只需一条 Move");
        assert!(matches!(cmds[0], Command::Move { .. }));
        let mut stack = UndoStack::new();
        for c in cmds {
            stack.push(&mut f.doc, c).unwrap();
        }
        assert_eq!(
            children_of(&f.doc, &f.ab1),
            vec![f.g.clone(), f.b.clone(), f.a.clone()]
        );
        stack.undo(&mut f.doc).unwrap();
        assert_eq!(
            children_of(&f.doc, &f.ab1),
            vec![f.g.clone(), f.a.clone(), f.b.clone()],
            "undo 逆回层序"
        );
    }

    /// 跨父拖进编组:落点 = 组中带 → Move 进组 + SetGeom 重定基;
    /// 世界位置保持;undo 精确逆回(结构与几何)。
    #[test]
    fn drop_into_group_moves_and_keeps_world_pos() {
        let f = fx();
        let mut doc = f.doc;
        // 组有偏移 (100, 80):组内本地系 = 画板系 - (100, 80)
        let gid = doc.find_by_sid(&f.g).unwrap();
        doc.nodes.get_mut(gid).unwrap().geom = Geom {
            x: 100.0,
            y: 80.0,
            w: 400.0,
            h: 300.0,
        };
        // 落点 = 组中带(Into)→ 索引 = 组子级末尾
        let rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(200.0, 100.0));
        let zones = vec![DropZone {
            rect,
            sid: f.g.clone(),
            parent_sid: f.ab1.clone(),
            container: true,
            index: 0,
        }];
        let target = drop_slot(&doc, &zones, Some(egui::Pos2::new(100.0, 50.0)), &f.a).unwrap();
        let (parent, index) = match &target {
            DropTarget::Into { parent_sid, .. } => {
                let len = doc
                    .find_by_sid(parent_sid)
                    .map(|p| doc.nodes.get(p).unwrap().children.len())
                    .unwrap_or(0);
                (parent_sid.clone(), len)
            }
            _ => panic!("中带应为 Into"),
        };
        let cmds = move_cmds(&doc, &f.a, &parent, index);
        assert_eq!(cmds.len(), 2, "跨父 = Move + SetGeom 重定基");
        // 与面板落下路径一致:整包 Compound(一次 undo 全量逆回)
        let mut stack = UndoStack::new();
        stack.push(&mut doc, Command::Compound { cmds }).unwrap();
        assert_eq!(parent_of(&doc, &f.a).as_deref(), Some(f.g.as_str()));
        let a = geom_of(&doc, &f.a);
        assert_eq!(a.x, 5.0 - 100.0, "世界 X 保持(本地重定基)");
        assert_eq!(a.y, 200.0 - 80.0, "世界 Y 保持(本地重定基)");
        stack.undo(&mut doc).unwrap();
        assert_eq!(parent_of(&doc, &f.a).as_deref(), Some(f.ab1.as_str()));
        let a = geom_of(&doc, &f.a);
        assert_eq!((a.x, a.y), (5.0, 200.0), "undo 逆回几何");
    }

    /// 跨画板拖拽:盒 C(画板2 原点 y=1000,本地 50,60)拖进画板 1
    /// → 世界位置 (50, 1060) 保持;undo 逆回父级。
    #[test]
    fn drop_across_artboards_keeps_world_pos() {
        let mut f = fx();
        let cmds = move_cmds(&f.doc, &f.c, &f.ab1, usize::MAX);
        assert_eq!(cmds.len(), 2);
        // 与面板落下路径一致:整包 Compound(一次 undo 全量逆回)
        let mut stack = UndoStack::new();
        stack.push(&mut f.doc, Command::Compound { cmds }).unwrap();
        let c = geom_of(&f.doc, &f.c);
        assert_eq!((c.x, c.y), (50.0, 1060.0), "世界位置保持");
        stack.undo(&mut f.doc).unwrap();
        assert_eq!(parent_of(&f.doc, &f.c).as_deref(), Some(f.ab2.as_str()));
    }

    /// 落点守卫:放进自身行 / 放进自身子树 → None(不下命令)。
    #[test]
    fn drop_slot_rejects_self_and_own_subtree() {
        let f = fx();
        let r = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(100.0));
        let zones = vec![
            DropZone {
                rect: r,
                sid: f.g.clone(),
                parent_sid: f.ab1.clone(),
                container: true,
                index: 0,
            },
            DropZone {
                rect: r,
                sid: f.g1.clone(),
                parent_sid: f.g.clone(),
                container: false,
                index: 0,
            },
        ];
        let pos = egui::Pos2::new(50.0, 50.0);
        // 拖组放组自身行上 = None
        assert!(drop_slot(&f.doc, &zones, Some(pos), &f.g).is_none());
        // 拖组放到自身子树成员(g1)的行上 = None
        assert!(
            drop_slot(&f.doc, &zones, Some(pos), &f.a).is_some(),
            "无关对象可正常落点"
        );
        // 拖 a 到 g1(不是 a 的子树)→ 命中 g1 行,守卫不拦
        assert!(
            drop_slot(&f.doc, &zones, Some(pos), &f.g).is_none(),
            "组进入自身子树被守卫"
        );
        // 指针在区外 / 无指针 → None
        assert!(drop_slot(&f.doc, &zones, Some(egui::Pos2::new(999.0, 999.0)), &f.a).is_none());
        assert!(drop_slot(&f.doc, &zones, None, &f.a).is_none());
    }

    /// 落点语义:容器中带 = Into;上/下缘 = Edge(索引与指示线随缘)。
    #[test]
    fn drop_slot_band_semantics() {
        let f = fx();
        let r = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(200.0, 100.0));
        let zones = vec![DropZone {
            rect: r,
            sid: f.g.clone(),
            parent_sid: f.ab1.clone(),
            container: true,
            index: 0,
        }];
        // 中带(25%~75%)→ Into 组
        let t = drop_slot(&f.doc, &zones, Some(egui::Pos2::new(100.0, 50.0)), &f.a).unwrap();
        assert!(matches!(t, DropTarget::Into { ref parent_sid, .. } if parent_sid == &f.g));
        // 上缘 → Edge 插到组之前(index = 组当前索引 0;指示线在行顶)
        let t = drop_slot(&f.doc, &zones, Some(egui::Pos2::new(100.0, 5.0)), &f.a).unwrap();
        match t {
            DropTarget::Edge { index, line_y, .. } => {
                assert_eq!(index, 0);
                assert_eq!(line_y, 0.0);
            }
            _ => panic!("上缘应为 Edge"),
        }
        // 下缘 → Edge 插到组之后(index = 1;指示线在行底)
        let t = drop_slot(&f.doc, &zones, Some(egui::Pos2::new(100.0, 95.0)), &f.a).unwrap();
        match t {
            DropTarget::Edge { index, line_y, .. } => {
                assert_eq!(index, 1);
                assert_eq!(line_y, 100.0);
            }
            _ => panic!("下缘应为 Edge"),
        }
    }

    /// 画板拖拽守卫:画板落到其他画板行中带**不产生 Into**
    /// (画板嵌画板会脱离导出序;只能 Edge 重排)。
    #[test]
    fn drop_slot_never_puts_artboard_inside_container() {
        let f = fx();
        let rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(200.0, 100.0));
        let zones = vec![DropZone {
            rect,
            sid: f.ab1.clone(),
            parent_sid: String::new(),
            container: true,
            index: 0,
        }];
        let mid = egui::Pos2::new(100.0, 50.0);
        // 普通对象 → Into 画板(跨画板移动合法)
        let t = drop_slot(&f.doc, &zones, Some(mid), &f.a).unwrap();
        assert!(matches!(t, DropTarget::Into { .. }));
        // 画板 → 中带退化为 Edge(root 内重排),绝不入组
        let t = drop_slot(&f.doc, &zones, Some(mid), &f.ab2).unwrap();
        assert!(matches!(t, DropTarget::Edge { .. }), "画板不得进入容器");
    }

    // ── 02-4-2 Alt 复制(命令层等价路径) ──

    /// Alt 复制:克隆 A 进组 → 新 sid、世界位置保持;undo 后消失。
    #[test]
    fn alt_dup_inserts_copy_into_group_and_reverts() {
        let mut f = fx();
        let gid = f.doc.find_by_sid(&f.g).unwrap();
        f.doc.nodes.get_mut(gid).unwrap().geom = Geom {
            x: 100.0,
            y: 80.0,
            w: 400.0,
            h: 300.0,
        };
        let (insert, new_sid) = dup_insert_cmd(&mut f.doc, &f.a, &f.g, usize::MAX).unwrap();
        assert_ne!(new_sid, f.a, "复制体必须有自己的稳定 id");
        let mut cmds = vec![insert];
        cmds.extend(rebase_for_new_parent(&f.doc, &f.a, &new_sid, &f.g));
        assert_eq!(cmds.len(), 2, "跨父复制 = Insert + SetGeom 重定基");
        // 与面板落下路径一致:整包 Compound(一次 undo 全量逆回)
        let mut stack = UndoStack::new();
        stack.push(&mut f.doc, Command::Compound { cmds }).unwrap();
        assert!(f.doc.find_by_sid(&new_sid).is_some(), "复制体落盘");
        assert_eq!(
            children_of(&f.doc, &f.g),
            vec![f.g1.clone(), new_sid.clone()]
        );
        let copy = geom_of(&f.doc, &new_sid);
        assert_eq!((copy.x, copy.y), (-95.0, 120.0), "复制体世界位置与源一致");
        stack.undo(&mut f.doc).unwrap();
        assert!(f.doc.find_by_sid(&new_sid).is_none(), "undo 后复制体消失");
    }

    // ── 02-4-3 颜色标记(入文档 + 往返) ──

    /// 标记循环:None → 首色 → … → 末色 → None;写回 SetAttrs 后
    /// 落盘为 data-vb-mark,undo 清除,导出产物含该属性(合法属性往返)。
    #[test]
    fn mark_cycles_persists_and_roundtrips() {
        let mut f = fx();
        // 循环语义
        assert_eq!(cycle_mark(None), Some(MARK_COLORS[0]));
        let last = *MARK_COLORS.last().unwrap();
        assert_eq!(cycle_mark(Some(last)), None, "末色再点 = 清除");

        let cmd = mark_cmd(&f.doc, &f.a, Some(MARK_COLORS[2].to_string()));
        let mut stack = UndoStack::new();
        stack.push(&mut f.doc, cmd).unwrap();
        let a = f.doc.nodes.get(f.doc.find_by_sid(&f.a).unwrap()).unwrap();
        assert_eq!(
            a.attrs.get(MARK_ATTR).map(String::as_str),
            Some(MARK_COLORS[2]),
            "标记落盘为 data-vb-mark 属性"
        );
        // 导出落盘:HTML 产物含 data-vb-mark(split_attrs 导入器原样保留)
        let files = vb_doc::export::render_project(&f.doc).files;
        assert!(
            files.iter().any(|f| f.1.contains("data-vb-mark")),
            "导出 HTML 应携带 data-vb-mark"
        );
        // undo 清除
        stack.undo(&mut f.doc).unwrap();
        let a = f.doc.nodes.get(f.doc.find_by_sid(&f.a).unwrap()).unwrap();
        assert!(!a.attrs.contains_key(MARK_ATTR), "undo 后标记消失");
    }

    // ── 02-4-4 右键菜单的命令路径 ──

    /// 隐藏其他:除自身/祖先/后代外全部 hidden;undo 全量逆回。
    #[test]
    fn hide_others_compound_and_revert() {
        let mut f = fx();
        let others = others_of(&f.doc, &f.a);
        // 组 g 与其子 g1 是 a 的兄弟子树(不是祖先);画板节点已排除
        assert_eq!(others.len(), 4, "g、g1、b、c");
        assert!(!others.contains(&f.a), "自身除外");
        for s in [&f.g, &f.g1, &f.b, &f.c] {
            assert!(others.contains(s), "其他应含 {s}");
        }
        let cmds: Vec<Command> = others
            .iter()
            .map(|sid| Command::SetFlags {
                sid: sid.clone(),
                hidden: Some(true),
                locked: None,
                old: None,
            })
            .collect();
        let mut stack = UndoStack::new();
        stack.push(&mut f.doc, Command::Compound { cmds }).unwrap();
        for sid in [&f.g, &f.g1, &f.b, &f.c] {
            let n = f.doc.nodes.get(f.doc.find_by_sid(sid).unwrap()).unwrap();
            assert!(n.hidden, "{sid} 应被隐藏");
        }
        assert!(
            !f.doc
                .nodes
                .get(f.doc.find_by_sid(&f.a).unwrap())
                .unwrap()
                .hidden,
            "目标自身保持可见"
        );
        stack.undo(&mut f.doc).unwrap();
        for sid in [&f.g, &f.g1, &f.b, &f.c] {
            let n = f.doc.nodes.get(f.doc.find_by_sid(sid).unwrap()).unwrap();
            assert!(!n.hidden, "undo 后 {sid} 应恢复可见");
        }
    }

    /// 锁定其他:作用域同隐藏其他(g1 的祖先链除外 → a、b、c);undo 逆回。
    #[test]
    fn lock_others_compound_and_revert() {
        let mut f = fx();
        let others = others_of(&f.doc, &f.g1);
        assert_eq!(others.len(), 3);
        assert!(!others.contains(&f.g) && !others.contains(&f.g1));
        let cmds: Vec<Command> = others
            .iter()
            .map(|sid| Command::SetFlags {
                sid: sid.clone(),
                hidden: None,
                locked: Some(true),
                old: None,
            })
            .collect();
        let mut stack = UndoStack::new();
        stack.push(&mut f.doc, Command::Compound { cmds }).unwrap();
        assert!(
            f.doc
                .nodes
                .get(f.doc.find_by_sid(&f.a).unwrap())
                .unwrap()
                .locked
        );
        assert!(
            f.doc
                .nodes
                .get(f.doc.find_by_sid(&f.c).unwrap())
                .unwrap()
                .locked
        );
        stack.undo(&mut f.doc).unwrap();
        assert!(
            !f.doc
                .nodes
                .get(f.doc.find_by_sid(&f.a).unwrap())
                .unwrap()
                .locked
        );
    }

    /// 选择同类:所有 Box 节点(a、b、c、g1;g 是 Group 不算)。
    #[test]
    fn select_same_kind_scope() {
        let f = fx();
        let sids = same_kind_sids(&f.doc, &f.a);
        assert_eq!(sids.len(), 4);
        for s in [&f.a, &f.b, &f.c, &f.g1] {
            assert!(sids.contains(s), "同类应含 {s}");
        }
        let groups = same_kind_sids(&f.doc, &f.g);
        assert_eq!(groups, vec![f.g.clone()]);
    }

    /// 转换为编组:单成员 Group 包裹;成员进组、组顶替原槽位;
    /// undo 完整逆回。画板被拒(菜单也不出现该项)。
    #[test]
    fn wrap_in_group_uses_existing_group_command() {
        let mut f = fx();
        let cmd = wrap_in_group_cmd(&mut f.doc, &f.a).unwrap();
        let gsid = match &cmd {
            Command::Group { group_sid, .. } => group_sid.clone(),
            _ => panic!("应为 Group 命令"),
        };
        let mut stack = UndoStack::new();
        stack.push(&mut f.doc, cmd).unwrap();
        assert_eq!(parent_of(&f.doc, &f.a).as_deref(), Some(gsid.as_str()));
        assert_eq!(parent_of(&f.doc, &gsid).as_deref(), Some(f.ab1.as_str()));
        assert_eq!(children_of(&f.doc, &gsid), vec![f.a.clone()]);
        stack.undo(&mut f.doc).unwrap();
        assert_eq!(parent_of(&f.doc, &f.a).as_deref(), Some(f.ab1.as_str()));
        assert!(f.doc.find_by_sid(&gsid).is_none());
        // 画板不支持
        assert!(wrap_in_group_cmd(&mut f.doc, &f.ab1).is_none());
    }

    // ── 02-4-7 搜索过滤 ──

    /// 命中子节点的祖先行保持可见;无关节点被过滤;大小写不敏感。
    #[test]
    fn search_keeps_ancestors_of_matches() {
        let f = fx();
        let gid = f.doc.find_by_sid(&f.g).unwrap();
        let aid = f.doc.find_by_sid(&f.a).unwrap();
        // "g1" 命中:组(祖先)+ g1;盒 A 不命中
        assert!(matches_query(&f.doc, gid, "g1"));
        assert!(!matches_query(&f.doc, aid, "g1"));
        // 名字直接命中 + 大小写不敏感
        assert!(matches_query(&f.doc, gid, "组"));
        assert!(matches_query(&f.doc, aid, "盒a"));
        // 空词全过
        assert!(matches_query(&f.doc, aid, "  "));
    }
}
