//! 图层 Tab · 数据模型与纯函数(标记 / 搜索 / 拖拽落点语义 / 命令构建)。
//!
//! 06-1 自 `panels/layers.rs` 按「模型 / 渲染 / 拖放」拆出(纯搬移,零行为变化):
//! 渲染在 `render`,拖拽高亮与落下在 `dragdrop`,门禁测试在 `tests`。

use egui::{Pos2, Rect};
use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeId, NodeKind};
use vb_ui::icons::Name;

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
pub(super) struct RowDesc {
    pub(super) sid: String,
    pub(super) name: String,
    pub(super) icon: Name,
    pub(super) frozen: bool,
    pub(super) container: bool,
    pub(super) is_artboard: bool,
    pub(super) hidden: bool,
    pub(super) locked: bool,
    pub(super) has_children: bool,
    pub(super) expanded: bool,
    pub(super) depth: u8,
    pub(super) parent_sid: Option<String>,
    pub(super) index: usize,
    pub(super) mark: Option<String>,
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
pub(super) fn mark_cmd(doc: &Document, sid: &str, mark: Option<String>) -> Command {
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

/// 复制落点后的重定基命令(Alt 复制跨父时保持世界位置)。
pub(super) fn rebase_for_new_parent(
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
