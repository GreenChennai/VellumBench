//! 可逆命令(设计文档 09 篇 §四,ADR-0008)。
//!
//! 寻址纪律:**所有命令以稳定 sid 寻址**(不存 NodeId)—— Undo/Redo 中节点
//! 在 arena 里销毁重建,NodeId 会变,`data-vb-id` 不会(CONTEXT.md)。
//! 每条命令首次 apply 时自动捕获撤销所需状态;revert 精确逆回;再次 apply
//! (Redo)必须与首次 apply 等效。

use vb_css::Decl;

use crate::model::{Document, Geom, Node, NodeKind, NodeTree};
use crate::Result;
use crate::VbError;

/// 变更摘要(渲染器/面板据此置脏)。
#[derive(Debug, Clone, Copy, Default)]
pub struct ChangeSet {
    pub structure: bool,
    pub style: bool,
    pub geometry: bool,
    pub text: bool,
}

impl ChangeSet {
    pub fn any(&self) -> bool {
        self.structure || self.style || self.geometry || self.text
    }
    fn full() -> Self {
        ChangeSet {
            structure: true,
            style: true,
            geometry: true,
            text: true,
        }
    }
    fn style_geom() -> Self {
        ChangeSet {
            structure: false,
            style: true,
            geometry: true,
            text: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CmdKind {
    Insert,
    Delete,
    Move,
    SetGeom,
    SetStyle,
    SetText,
    SetAttrs,
    Rename,
    SetTag,
    SetVector,
    Flags,
    Group,
    Ungroup,
    Compound,
    SetToken,
}

/// 结构变更的落点(父 sid + 位置)。
#[derive(Debug, Clone)]
pub struct Slot {
    parent_sid: String,
    index: usize,
}

/// 可逆命令(sid 寻址)。
#[derive(Debug, Clone)]
pub enum Command {
    /// 插入子树。tree 携带的 sid 必须在 apply 时未被占用(调用方保证)。
    Insert {
        parent_sid: String,
        index: usize,
        tree: NodeTree,
    },
    Delete {
        target_sid: String,
        captured: Option<(Slot, NodeTree)>,
    },
    Move {
        sid: String,
        new_parent_sid: String,
        new_index: usize,
        old: Option<Slot>,
    },
    SetGeom {
        sid: String,
        new: Geom,
        old: Option<Geom>,
    },
    SetStyle {
        sid: String,
        new: Vec<Decl>,
        old: Option<Vec<Decl>>,
    },
    SetText {
        sid: String,
        new: String,
        old: Option<String>,
    },
    SetAttrs {
        sid: String,
        new: Vec<(String, String)>,
        old: Option<Vec<(String, String)>>,
    },
    Rename {
        sid: String,
        new: String,
        old: Option<String>,
    },
    /// 语义标签切换(v0.7:div ↔ section/header/h1/a …)
    SetTag {
        sid: String,
        new: String,
        old: Option<String>,
    },
    /// 矢量路径编辑(P4 钢笔/直接选择):整路径替换(锚点移动/增删都表现为新路径)
    SetVector {
        sid: String,
        new: kurbo::BezPath,
        old: Option<kurbo::BezPath>,
    },
    SetFlags {
        sid: String,
        hidden: Option<bool>,
        locked: Option<bool>,
        old: Option<(bool, bool)>,
    },
    /// 编组:成员必须同父(v0.1 约束)。group_sid 由调用方预分配,重做时复用。
    Group {
        member_sids: Vec<String>,
        name: String,
        group_sid: String,
        old_slots: Option<Vec<Slot>>,
    },
    Ungroup {
        group_sid: String,
        captured: Option<(Slot, NodeTree)>,
    },
    Compound {
        cmds: Vec<Command>,
    },
    SetToken {
        name: String,
        new: String,
        /// Some(None) = 原先不存在;Some(Some((原索引, 原值))) = 原先存在。
        old: Option<Option<(usize, String)>>,
    },
}

fn no_such(sid: &str) -> VbError {
    VbError::NoSuchNode(sid.to_string())
}

impl Command {
    pub fn kind(&self) -> CmdKind {
        match self {
            Command::Insert { .. } => CmdKind::Insert,
            Command::Delete { .. } => CmdKind::Delete,
            Command::Move { .. } => CmdKind::Move,
            Command::SetGeom { .. } => CmdKind::SetGeom,
            Command::SetStyle { .. } => CmdKind::SetStyle,
            Command::SetText { .. } => CmdKind::SetText,
            Command::SetAttrs { .. } => CmdKind::SetAttrs,
            Command::Rename { .. } => CmdKind::Rename,
            Command::SetTag { .. } => CmdKind::SetTag,
            Command::SetVector { .. } => CmdKind::SetVector,
            Command::SetFlags { .. } => CmdKind::Flags,
            Command::Group { .. } => CmdKind::Group,
            Command::Ungroup { .. } => CmdKind::Ungroup,
            Command::Compound { .. } => CmdKind::Compound,
            Command::SetToken { .. } => CmdKind::SetToken,
        }
    }

    /// Undo 合并键:同 kind + 同 target 且在时间窗内 → 合并为一条
    /// (数值框连击/连续拖动只产生一条 undo,设计文档 09 篇 §四)。
    pub fn merge_target(&self) -> Option<(CmdKind, String)> {
        match self {
            Command::SetGeom { sid, .. }
            | Command::SetStyle { sid, .. }
            | Command::SetText { sid, .. }
            | Command::Rename { sid, .. }
            | Command::SetTag { sid, .. }
            | Command::SetVector { sid, .. } => Some((self.kind(), sid.clone())),
            // 渐变拖拽每帧一条 Compound(逐目标 SetStyle):不可合并会把
            // 一次拖拽稀释成几百步 undo。仅当全部子命令为 SetStyle 时
            // 视为「多目标 SetStyle」可合并,key = 目标 sid 集合。
            Command::Compound { cmds } if !cmds.is_empty() => {
                let mut key = String::new();
                for c in cmds {
                    match c {
                        Command::SetStyle { sid, .. } => {
                            key.push_str(sid);
                            key.push(',');
                        }
                        _ => return None,
                    }
                }
                Some((self.kind(), key))
            }
            _ => None,
        }
    }

    /// 用户可见名(编辑菜单「撤销 X」,与 AI 一致)。
    pub fn label(&self) -> &'static str {
        match self {
            Command::Insert { .. } => "新建对象",
            Command::Delete { .. } => "删除对象",
            Command::Move { .. } => "移动对象",
            Command::SetGeom { .. } => "变换",
            Command::SetStyle { .. } => "修改样式",
            Command::SetText { .. } => "编辑文本",
            Command::SetAttrs { .. } => "修改 HTML 属性",
            Command::Rename { .. } => "重命名",
            Command::SetTag { .. } => "切换语义标签",
            Command::SetVector { .. } => "编辑矢量路径",
            Command::SetFlags { .. } => "切换可见/锁定",
            Command::Group { .. } => "编组",
            Command::Ungroup { .. } => "取消编组",
            Command::Compound { .. } => "复合操作",
            Command::SetToken { .. } => "修改设计令牌",
        }
    }

    fn slot_of(doc: &Document, sid: &str) -> Result<Slot> {
        let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
        let n = doc.nodes.get(id).unwrap();
        let parent = n.parent.ok_or_else(|| no_such(sid))?;
        let index = doc
            .nodes
            .get(parent)
            .unwrap()
            .children
            .iter()
            .position(|&c| c == id)
            .unwrap_or(0);
        let parent_sid = doc.nodes.get(parent).unwrap().sid.as_str().to_string();
        Ok(Slot { parent_sid, index })
    }

    pub fn apply(&mut self, doc: &mut Document) -> Result<ChangeSet> {
        match self {
            Command::Insert {
                parent_sid,
                index,
                tree,
            } => {
                if doc.find_by_sid(tree.root_sid()).is_some() {
                    return Err(VbError::Conflict(format!(
                        "sid 已存在: {}",
                        tree.root_sid()
                    )));
                }
                doc.insert_tree_at(tree, parent_sid, *index)
                    .ok_or_else(|| no_such(parent_sid))?;
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::Delete {
                target_sid,
                captured,
            } => {
                // 不变量:文档至少保留一块画板。Delete 是画板的唯一删除
                // 通道(面板按钮/画板工具/Delete 键),守卫放在命令层才能
                // 同时约束 GUI 与 Agent 两条路径;redo 分支同守,防
                // 「删 A→撤销→删 B→重做删 A」绕过。
                if let Some(id) = doc.find_by_sid(target_sid) {
                    if matches!(doc.nodes.get(id),
                                Some(n) if matches!(n.kind, NodeKind::Artboard))
                        && doc.artboards.len() <= 1
                    {
                        return Err(VbError::Conflict("至少保留一块画板".into()));
                    }
                }
                if captured.is_none() {
                    // 首次:必须先捕获槽位再取出(extract 会销毁父级信息)
                    let slot = Self::slot_of(doc, target_sid)?;
                    let (_, tree) = doc
                        .extract_subtree(target_sid)
                        .ok_or_else(|| no_such(target_sid))?;
                    *captured = Some((slot, tree));
                } else {
                    // Redo:节点已被 revert 放回,再次取出(快照保持不变)
                    doc.extract_subtree(target_sid)
                        .ok_or_else(|| no_such(target_sid))?;
                }
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::Move {
                sid,
                new_parent_sid,
                new_index,
                old,
            } => {
                if old.is_none() {
                    *old = Some(Self::slot_of(doc, sid)?);
                }
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let np = doc
                    .find_by_sid(new_parent_sid)
                    .ok_or_else(|| no_such(new_parent_sid))?;
                // 环防护:新父级不得是自身或自身后代(否则场景图成环,遍历栈溢出)
                if doc.is_descendant_or_self(id, np) {
                    return Err(VbError::Conflict(format!(
                        "不能把节点移入自身或其后代: {new_parent_sid}"
                    )));
                }
                doc.detach(id);
                let idx = (*new_index).min(doc.nodes.get(np).unwrap().children.len());
                doc.nodes.get_mut(np).unwrap().children.insert(idx, id);
                doc.nodes.get_mut(id).unwrap().parent = Some(np);
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::SetGeom { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(n.geom);
                }
                n.geom = *new;
                Ok(ChangeSet::style_geom())
            }
            Command::SetStyle { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(n.style.clone());
                }
                n.style = new.clone();
                Ok(ChangeSet::style_geom())
            }
            Command::SetText { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(match &n.kind {
                        NodeKind::Text { text, .. } => text.clone(),
                        _ => return Err(VbError::Unsupported("该对象不是文本".into())),
                    });
                }
                if let NodeKind::Text { text, .. } = &mut n.kind {
                    *text = new.clone();
                }
                Ok(ChangeSet {
                    structure: false,
                    style: false,
                    geometry: false,
                    text: true,
                })
            }
            Command::SetAttrs { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(
                        n.attrs
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    );
                }
                n.attrs = new.iter().cloned().collect();
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::Rename { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(n.name.clone());
                }
                n.name = new.clone();
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::SetTag { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(n.tag.clone());
                }
                n.tag = new.clone();
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::SetVector { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(match &n.kind {
                        NodeKind::Vector { path } => path.clone(),
                        _ => {
                            return Err(VbError::Unsupported("该对象不是矢量路径".into()));
                        }
                    });
                }
                if let NodeKind::Vector { path } = &mut n.kind {
                    *path = new.clone();
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::SetFlags {
                sid,
                hidden,
                locked,
                old,
            } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some((n.hidden, n.locked));
                }
                if let Some(h) = *hidden {
                    n.hidden = h;
                }
                if let Some(l) = *locked {
                    n.locked = l;
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::Group {
                member_sids,
                name,
                group_sid,
                old_slots,
            } => {
                if member_sids.is_empty() {
                    return Err(VbError::Parse("编组需要至少一个成员".into()));
                }
                let mut seen = std::collections::HashSet::new();
                for m in member_sids.iter() {
                    if !seen.insert(m.as_str()) {
                        return Err(VbError::Conflict(format!("编组成员重复: {m}")));
                    }
                }
                if doc.find_by_sid(group_sid).is_some() {
                    // Redo:编组已被 revert 拆掉,group_sid 应空闲;占用即状态错误
                    return Err(VbError::Conflict(format!("group sid 已存在: {group_sid}")));
                }
                if old_slots.is_none() {
                    let mut slots = Vec::new();
                    for m in member_sids.iter() {
                        slots.push(Self::slot_of(doc, m)?);
                    }
                    *old_slots = Some(slots);
                }
                let slots = old_slots.as_ref().unwrap();
                // 前置校验:成员必须同父、互不为祖先后代、不含画板。
                // 此前注释声称「成员必须同父(v0.1 约束)」但代码未校验 ——
                // 跨画板成员会被统一重定基到错误的坐标系(视觉瞬移);
                // 祖先+后代编组会产生错乱几何;画板入组会绕过「至少一块
                // 画板」的删除守卫。
                {
                    // 校验按「最具体错误优先」排列:画板 → 祖先后代 → 同父
                    for m in member_sids.iter() {
                        let id = doc.find_by_sid(m).ok_or_else(|| no_such(m))?;
                        if matches!(doc.nodes.get(id),
                                    Some(n) if matches!(n.kind, NodeKind::Artboard))
                        {
                            return Err(VbError::Conflict("画板不能编入组".into()));
                        }
                    }
                    for m in member_sids.iter() {
                        let id = doc.find_by_sid(m).ok_or_else(|| no_such(m))?;
                        for other in member_sids.iter() {
                            if other == m {
                                continue;
                            }
                            let oid = doc.find_by_sid(other).ok_or_else(|| no_such(other))?;
                            if doc.is_descendant_or_self(id, oid)
                                || doc.is_descendant_or_self(oid, id)
                            {
                                return Err(VbError::Conflict(format!(
                                    "编组成员不能互为祖先或后代: {m} / {other}"
                                )));
                            }
                        }
                    }
                    let first_parent = &slots[0].parent_sid;
                    if !slots.iter().all(|s| &s.parent_sid == first_parent) {
                        return Err(VbError::Conflict(
                            "编组成员必须同属一个父级".into(),
                        ));
                    }
                }
                // 编组落在最上层成员的原位置
                let top = slots
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, s)| s.index)
                    .map(|(_, s)| s.clone());
                let members: Vec<_> = member_sids
                    .iter()
                    .map(|s| {
                        doc.find_by_sid(s)
                            .ok_or_else(|| no_such(s))
                            .map(|id| (id, doc.nodes.get(id).unwrap().clone()))
                    })
                    .collect::<Result<_>>()?;
                let group_id = vb_common::StableId::parse(group_sid)
                    .ok_or_else(|| VbError::Parse(format!("非法 group sid: {group_sid}")))?;
                let mut group = Node::new(NodeKind::Group, name.clone(), group_id);
                let mut minx = f64::INFINITY;
                let mut miny = f64::INFINITY;
                let mut maxr = f64::NEG_INFINITY;
                let mut maxb = f64::NEG_INFINITY;
                for (_, n) in &members {
                    minx = minx.min(n.geom.x);
                    miny = miny.min(n.geom.y);
                    maxr = maxr.max(n.geom.x + n.geom.w);
                    maxb = maxb.max(n.geom.y + n.geom.h);
                }
                group.geom = Geom {
                    x: minx,
                    y: miny,
                    w: (maxr - minx).max(0.0),
                    h: (maxb - miny).max(0.0),
                };
                let gid = doc.nodes.insert(group);
                // 摘除成员并收进编组;成员坐标从原父级系重定基到组系
                // (渲染时组偏移会再累加一次,不重定基则内容整体位移)
                for (id, _) in &members {
                    doc.detach(*id);
                }
                {
                    let g = doc.nodes.get_mut(gid).unwrap();
                    for (id, _) in &members {
                        g.children.push(*id);
                    }
                }
                for (id, _) in &members {
                    let m = doc.nodes.get_mut(*id).unwrap();
                    m.geom.x -= minx;
                    m.geom.y -= miny;
                    m.parent = Some(gid);
                }
                if let Some(top_slot) = top {
                    let parent = doc
                        .find_by_sid(&top_slot.parent_sid)
                        .ok_or_else(|| no_such(&top_slot.parent_sid))?;
                    // 成员已全部摘除:top 的原索引没有补偿「排在它之下、
                    // 已被移走的成员」,直接用会让编组越过它们(如
                    // [A,B,C] 选 A、B 编组 → 错成 [C,G],应为 [G,C])。
                    let removed_below = slots
                        .iter()
                        .filter(|s| s.index < top_slot.index)
                        .count();
                    let idx = top_slot
                        .index
                        .saturating_sub(removed_below)
                        .min(doc.nodes.get(parent).unwrap().children.len());
                    doc.nodes.get_mut(parent).unwrap().children.insert(idx, gid);
                    doc.nodes.get_mut(gid).unwrap().parent = Some(parent);
                }
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::Ungroup {
                group_sid,
                captured,
            } => {
                if captured.is_none() {
                    let slot = Self::slot_of(doc, group_sid)?;
                    let (_, tree) = doc
                        .extract_subtree(group_sid)
                        .ok_or_else(|| no_such(group_sid))?;
                    *captured = Some((slot, tree));
                } else {
                    // Redo:编组已放回,再次取出(保留 captured)
                    let (slot, _) = captured.as_ref().unwrap();
                    let _ = slot;
                    doc.extract_subtree(group_sid)
                        .ok_or_else(|| no_such(group_sid))?;
                }
                // 成员平移进编组原位置;坐标从组系重定基回原父级系
                let (slot, tree) = captured.as_ref().unwrap();
                let parent = doc
                    .find_by_sid(&slot.parent_sid)
                    .ok_or_else(|| no_such(&slot.parent_sid))?;
                let (gx, gy) = (tree.node.geom.x, tree.node.geom.y);
                let mut created = Vec::new();
                for (i, child) in tree.children.iter().enumerate() {
                    let mut child = child.clone();
                    child.node.geom.x += gx;
                    child.node.geom.y += gy;
                    child.insert_into(doc, parent, slot.index + i, &mut created);
                }
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::Compound { cmds } => {
                // 原子性:任一子命令失败,逆序回滚已应用的部分再报错
                // (否则半条事务固化在文档上且不入 undo 栈,08 篇 §五)
                let mut done = 0usize;
                for c in cmds.iter_mut() {
                    match c.apply(doc) {
                        Ok(_) => done += 1,
                        Err(e) => {
                            for prev in cmds[..done].iter_mut().rev() {
                                let _ = prev.revert(doc);
                            }
                            return Err(e);
                        }
                    }
                }
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::SetToken { name, new, old } => {
                if old.is_none() {
                    *old = Some(
                        doc.tokens
                            .iter()
                            .enumerate()
                            .find_map(|(i, (n, v))| (n == name).then_some((i, v.clone()))),
                    );
                }
                // 空值 = 删除令牌(界面「删除」按钮同语义);否则原地 upsert
                if new.is_empty() {
                    doc.tokens.retain(|(n, _)| n != name);
                } else if let Some(t) = doc.tokens.iter_mut().find(|(n, _)| n == name) {
                    t.1 = new.clone();
                } else {
                    doc.tokens.push((name.clone(), new.clone()));
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
        }
    }

    pub fn revert(&mut self, doc: &mut Document) -> Result<ChangeSet> {
        match self {
            Command::Insert { tree, .. } => {
                doc.extract_subtree(tree.root_sid())
                    .ok_or_else(|| no_such(tree.root_sid()))?;
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::Delete {
                target_sid,
                captured,
            } => {
                let (slot, tree) = captured
                    .as_ref()
                    .ok_or_else(|| VbError::Parse("Delete 未捕获快照".into()))?;
                doc.insert_tree_at(tree, &slot.parent_sid, slot.index)
                    .ok_or_else(|| no_such(target_sid))?;
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::Move { sid, old, .. } => {
                let (old_parent, old_index) = match old {
                    Some(s) => (s.parent_sid.clone(), s.index),
                    None => return Ok(ChangeSet::full()),
                };
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                doc.detach(id);
                let p = doc
                    .find_by_sid(&old_parent)
                    .ok_or_else(|| no_such(&old_parent))?;
                let idx = old_index.min(doc.nodes.get(p).unwrap().children.len());
                doc.nodes.get_mut(p).unwrap().children.insert(idx, id);
                doc.nodes.get_mut(id).unwrap().parent = Some(p);
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::SetGeom { sid, old, .. } => {
                if let Some(g) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    doc.nodes.get_mut(id).unwrap().geom = *g;
                }
                Ok(ChangeSet::style_geom())
            }
            Command::SetStyle { sid, old, .. } => {
                if let Some(s) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    doc.nodes.get_mut(id).unwrap().style = s.clone();
                }
                Ok(ChangeSet::style_geom())
            }
            Command::SetText { sid, old, .. } => {
                if let Some(t) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    let n = doc.nodes.get_mut(id).unwrap();
                    if let NodeKind::Text { text, .. } = &mut n.kind {
                        *text = t.clone();
                    }
                }
                Ok(ChangeSet {
                    structure: false,
                    style: false,
                    geometry: false,
                    text: true,
                })
            }
            Command::SetAttrs { sid, old, .. } => {
                if let Some(a) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    doc.nodes.get_mut(id).unwrap().attrs = a.iter().cloned().collect();
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::Rename { sid, old, .. } => {
                if let Some(s) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    doc.nodes.get_mut(id).unwrap().name = s.clone();
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::SetTag { sid, old, .. } => {
                if let Some(t) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    doc.nodes.get_mut(id).unwrap().tag = t.clone();
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::SetVector { sid, old, .. } => {
                if let Some(p) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    let n = doc.nodes.get_mut(id).unwrap();
                    if let NodeKind::Vector { path } = &mut n.kind {
                        *path = p.clone();
                    }
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::SetFlags { sid, old, .. } => {
                if let Some((h, l)) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    let n = doc.nodes.get_mut(id).unwrap();
                    n.hidden = *h;
                    n.locked = *l;
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
            Command::Group {
                member_sids,
                group_sid,
                old_slots,
                ..
            } => {
                // 取出编组整棵树;成员按原槽位放回
                let (_, gtree) = doc
                    .extract_subtree(group_sid)
                    .ok_or_else(|| no_such(group_sid))?;
                let slots = old_slots
                    .as_ref()
                    .ok_or_else(|| VbError::Parse("Group 未捕获槽位".into()))?;
                let mut gtree = gtree;
                // 组原点:revert 把成员坐标从组系加回原父级系
                let (gx, gy) = (gtree.node.geom.x, gtree.node.geom.y);
                // 同父级内按原索引升序重插(乱序会被 clamp 推挤,z 序错乱)
                let mut order: Vec<_> = member_sids.iter().zip(slots.iter()).collect();
                order.sort_by(|a, b| {
                    a.1.parent_sid
                        .cmp(&b.1.parent_sid)
                        .then(a.1.index.cmp(&b.1.index))
                });
                for (m, slot) in order {
                    let mut member_tree = gtree.take_child(m).ok_or_else(|| no_such(m))?;
                    member_tree.node.geom.x += gx;
                    member_tree.node.geom.y += gy;
                    doc.insert_tree_at(&member_tree, &slot.parent_sid, slot.index)
                        .ok_or_else(|| no_such(m))?;
                }
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::Ungroup {
                group_sid,
                captured,
            } => {
                let (slot, tree) = captured
                    .as_ref()
                    .ok_or_else(|| VbError::Parse("Ungroup 未捕获快照".into()))?;
                // 从父级取出散落的成员(sid 未变),再把编组整棵树放回
                let member_sids: Vec<String> = tree
                    .children
                    .iter()
                    .map(|c| c.node.sid.as_str().to_string())
                    .collect();
                for m in &member_sids {
                    doc.extract_subtree(m).ok_or_else(|| no_such(m))?;
                }
                doc.insert_tree_at(tree, &slot.parent_sid, slot.index)
                    .ok_or_else(|| no_such(group_sid))?;
                doc.sync_artboards();
                Ok(ChangeSet::full())
            }
            Command::Compound { cmds } => {
                for c in cmds.iter_mut().rev() {
                    c.revert(doc)?;
                }
                Ok(ChangeSet::full())
            }
            Command::SetToken { name, old, .. } => {
                if let Some(prev) = old {
                    doc.tokens.retain(|(n, _)| n != name);
                    if let Some((idx, val)) = prev {
                        let i = (*idx).min(doc.tokens.len());
                        doc.tokens.insert(i, (name.clone(), val.clone()));
                    }
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: false,
                })
            }
        }
    }
}
