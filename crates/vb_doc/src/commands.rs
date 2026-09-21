//! 可逆命令(设计文档 09 篇 §四,ADR-0008)。
//!
//! 寻址纪律:**所有命令以稳定 sid 寻址**(不存 NodeId)—— Undo/Redo 中节点
//! 在 arena 里销毁重建,NodeId 会变,`data-vb-id` 不会(CONTEXT.md)。
//! 每条命令首次 apply 时自动捕获撤销所需状态;revert 精确逆回;再次 apply
//! (Redo)必须与首次 apply 等效。

use vb_css::Decl;

use vb_common::geom::BezPath;

use crate::model::{Document, Geom, Node, NodeKind, NodeTree, TextMode, TextSeg};
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
    SetSegs,
    SetTextMode,
    SetAttrs,
    Rename,
    SetTag,
    SetVector,
    Flags,
    Group,
    Ungroup,
    Compound,
    SetToken,
    PathBoolean,
    SetMeta,
}

/// 结构变更的落点(父 sid + 位置)。
#[derive(Debug, Clone)]
pub struct Slot {
    parent_sid: String,
    index: usize,
}

/// 文本编辑快照:SetText 的 `old`(内容 + 段注记,一起逆回)。
#[derive(Debug, Clone)]
pub struct TextSnapshot {
    pub text: String,
    pub segs: Vec<TextSeg>,
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
        /// (P0-1)首次应用前的 `geom_declared`(None = 尚未应用)。
        /// 显式移动/缩放会把声明几何兑换为显式几何(materialize),撤销时原样还原。
        old_declared: Option<bool>,
    },
    SetStyle {
        sid: String,
        new: Vec<Decl>,
        old: Option<Vec<Decl>>,
    },
    SetText {
        sid: String,
        new: String,
        /// 编辑快照(内容 + 段注记):模型不变量「编辑 text 时必须清空
        /// segments」的可逆承载 —— 撤销时二者一起还原。
        old: Option<TextSnapshot>,
    },
    /// 段内富文本注记整体替换(04 字符面板:run 级样式写入)。
    /// `new` 必须是 `text` 字节区间上的升序不重叠区间(apply 校验)。
    SetSegs {
        sid: String,
        new: Vec<TextSeg>,
        old: Option<Vec<TextSeg>>,
    },
    /// 点文本 ↔ 区域文本切换(04-3 Shift+T;模式影响断行/布局,可撤销)。
    SetTextMode {
        sid: String,
        new: TextMode,
        old: Option<TextMode>,
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
    /// 路径查找器(批次 C1,ADR-0012):lhs 替换为布尔结果,rhs 删除。
    /// 结果路径由调用方(vb_app / vb_agent)经 `vb_tools::boolean`
    /// 预计算;命令只负责可逆应用(ADR-0008)。
    PathBoolean {
        /// union / subtract / intersect / xor(仅展示用)
        op: String,
        lhs_sid: String,
        rhs_sid: String,
        /// 结果路径(lhs 节点本地,已重定基到新包围盒原点)
        new_path: BezPath,
        /// lhs 节点新几何(父级帧)
        new_geom: Geom,
        /// 应用快照:lhs 原 (path, geom) + rhs 槽位与子树
        captured: Option<(Option<BezPath>, Geom, Slot, NodeTree)>,
    },
    /// 文档标题(S1-c 控制面板固定区:文档改名 → `<title>`;可撤销)。
    /// 标题是文档级状态(非节点),故不走 Rename;导出写 `<title>`。
    SetMetaTitle {
        new: String,
        old: Option<String>,
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
            Command::SetSegs { .. } => CmdKind::SetSegs,
            Command::SetTextMode { .. } => CmdKind::SetTextMode,
            Command::SetAttrs { .. } => CmdKind::SetAttrs,
            Command::Rename { .. } => CmdKind::Rename,
            Command::SetTag { .. } => CmdKind::SetTag,
            Command::SetVector { .. } => CmdKind::SetVector,
            Command::SetFlags { .. } => CmdKind::Flags,
            Command::Group { .. } => CmdKind::Group,
            Command::Ungroup { .. } => CmdKind::Ungroup,
            Command::Compound { .. } => CmdKind::Compound,
            Command::SetToken { .. } => CmdKind::SetToken,
            Command::PathBoolean { .. } => CmdKind::PathBoolean,
            Command::SetMetaTitle { .. } => CmdKind::SetMeta,
        }
    }

    /// Undo 合并键:同 kind + 同 target 且在时间窗内 → 合并为一条
    /// (数值框连击/连续拖动只产生一条 undo,设计文档 09 篇 §四)。
    pub fn merge_target(&self) -> Option<(CmdKind, String)> {
        match self {
            Command::SetGeom { sid, .. }
            | Command::SetStyle { sid, .. }
            | Command::SetText { sid, .. }
            | Command::SetSegs { sid, .. }
            | Command::Rename { sid, .. }
            | Command::SetTag { sid, .. }
            | Command::SetVector { sid, .. } => Some((self.kind(), sid.clone())),
            // SetTextMode 是离散动作(两次切换 = 两步 undo),不参与合并。
            // 渐变拖拽每帧一条 Compound(逐目标 SetStyle)、多选拖拽每帧
            // 一条 Compound(逐目标 SetGeom):不可合并会把一次拖拽稀释成
            // 几百步 undo。仅当全部子命令为**同一类** Set* 时可合并,
            // key = 类别标记 + 目标 sid 集合。
            //
            // S4 外观面板(05-1):每次条目写回 = 同一目标的
            // Compound[SetStyle(重编译), SetAttrs(模型 JSON)] —— 这类
            // 「同目标混合」同样可合并,NumField 提交会话里拖数值只产生
            // 一条 undo。键必须带变体组成签名(纯 SetStyle 的单目标
            // Compound = 渐变拖拽每帧;混合 = 外观条目写回),两类绝不
            // 互并 —— 否则合并时 replace_new 找不到 SetAttrs 配对,模型
            // 属性会丢最后一帧更新。
            Command::Compound { cmds } if !cmds.is_empty() => {
                let mut same_target = true;
                let mut only_sid: Option<&String> = None;
                let mut has_style = false;
                let mut has_attrs = false;
                for c in cmds {
                    let sid = match c {
                        Command::SetStyle { sid, .. } => {
                            has_style = true;
                            sid
                        }
                        Command::SetAttrs { sid, .. } => {
                            has_attrs = true;
                            sid
                        }
                        _ => {
                            same_target = false;
                            break;
                        }
                    };
                    match only_sid {
                        None => only_sid = Some(sid),
                        Some(p) if p == sid => {}
                        _ => {
                            same_target = false;
                            break;
                        }
                    }
                }
                if same_target {
                    if let (Some(sid), true) = (only_sid, has_style && has_attrs) {
                        return Some((self.kind(), format!("mas{sid}")));
                    }
                    if let (Some(sid), false) = (only_sid, has_attrs) {
                        return Some((self.kind(), format!("ms{sid}")));
                    }
                }
                let mut key = String::new();
                let mut tag = ' ';
                for c in cmds {
                    let this_tag = match c {
                        Command::SetStyle { .. } => 's',
                        Command::SetGeom { .. } => 'g',
                        _ => return None,
                    };
                    if tag == ' ' {
                        tag = this_tag;
                    } else if tag != this_tag {
                        return None;
                    }
                    let sid = match c {
                        Command::SetStyle { sid, .. } | Command::SetGeom { sid, .. } => sid,
                        _ => unreachable!(),
                    };
                    key.push_str(sid);
                    key.push(',');
                }
                key.insert(0, tag);
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
            Command::SetSegs { .. } => "修改字符样式",
            Command::SetTextMode { .. } => "切换文本模式",
            Command::SetAttrs { .. } => "修改 HTML 属性",
            Command::Rename { .. } => "重命名",
            Command::SetTag { .. } => "切换语义标签",
            Command::SetVector { .. } => "编辑矢量路径",
            Command::SetFlags { .. } => "切换可见/锁定",
            Command::Group { .. } => "编组",
            Command::Ungroup { .. } => "取消编组",
            Command::Compound { .. } => "复合操作",
            Command::SetToken { .. } => "修改设计令牌",
            Command::PathBoolean { .. } => "路径查找器",
            Command::SetMetaTitle { .. } => "重命名文档",
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

    /// 段注记区间校验(model.rs 不变量):升序、互不重叠、不越界。
    /// apply/redo 双侧把守,坏区间不得落盘。
    fn validate_segs(segs: &[TextSeg], text_len: usize) -> Result<()> {
        let mut prev_end = 0usize;
        for s in segs {
            if s.start > s.end {
                return Err(VbError::Conflict(format!(
                    "段区间起止倒置: {}..{}",
                    s.start, s.end
                )));
            }
            if s.start < prev_end {
                return Err(VbError::Conflict(format!(
                    "段区间重叠或乱序: 上一区间止于 {prev_end},本区间始于 {}",
                    s.start
                )));
            }
            if s.end > text_len {
                return Err(VbError::Conflict(format!(
                    "段区间越界: {}..{} 超出文本长度 {text_len}",
                    s.start, s.end
                )));
            }
            prev_end = s.end;
        }
        Ok(())
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
                // root 之下只允许画板:非画板节点挂 root 会从导出中
                // 整体消失(渲染只走画板子树),必须拒绝(B5)
                {
                    let pid = doc.find_by_sid(parent_sid);
                    let is_root = pid == Some(doc.root);
                    let is_artboard = matches!(tree.node.kind, NodeKind::Artboard);
                    if is_root && !is_artboard {
                        return Err(VbError::Conflict(
                            "root 下只能挂画板(节点会脱离导出子树)".into(),
                        ));
                    }
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
                // root 之下只允许画板(同 Insert;B5)
                if np == doc.root
                    && !matches!(doc.nodes.get(id), Some(n) if matches!(n.kind, NodeKind::Artboard))
                {
                    return Err(VbError::Conflict(
                        "root 下只能挂画板(节点会脱离导出子树)".into(),
                    ));
                }
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
            Command::SetGeom {
                sid,
                new,
                old,
                old_declared,
            } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(n.geom);
                }
                if old_declared.is_none() {
                    *old_declared = Some(n.geom_declared);
                }
                // 用户显式几何编辑:声明几何(百分比锚/流式等)兑换为显式 px,
                // 此后该节点导出写显式值(不再原样回写原始声明)
                n.materialize_geom();
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
                        NodeKind::Text { text, segments, .. } => TextSnapshot {
                            text: text.clone(),
                            segs: segments.clone(),
                        },
                        _ => return Err(VbError::Unsupported("该对象不是文本".into())),
                    });
                }
                if let NodeKind::Text { text, segments, .. } = &mut n.kind {
                    // 模型不变量(model.rs:segments 注记「编辑 text 时必须
                    // 清空」在命令层强制执行):内容变化 → 字节区间全体失效,
                    // 段注记随内容一起清空;内容未变则不动(重提交不丢 run)。
                    if text != new {
                        *text = new.clone();
                        segments.clear();
                    }
                }
                Ok(ChangeSet {
                    structure: false,
                    style: false,
                    geometry: false,
                    text: true,
                })
            }
            Command::SetSegs { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(match &n.kind {
                        NodeKind::Text { segments, .. } => segments.clone(),
                        _ => return Err(VbError::Unsupported("该对象不是文本".into())),
                    });
                }
                if let NodeKind::Text { text, segments, .. } = &mut n.kind {
                    Self::validate_segs(new, text.len())?;
                    *segments = new.clone();
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: true,
                })
            }
            Command::SetTextMode { sid, new, old } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if old.is_none() {
                    *old = Some(match &n.kind {
                        NodeKind::Text { mode, .. } => *mode,
                        _ => return Err(VbError::Unsupported("该对象不是文本".into())),
                    });
                }
                if let NodeKind::Text { mode, .. } = &mut n.kind {
                    *mode = *new;
                }
                Ok(ChangeSet {
                    structure: false,
                    style: false,
                    geometry: true,
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
                        return Err(VbError::Conflict("编组成员必须同属一个父级".into()));
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
                    let removed_below = slots.iter().filter(|s| s.index < top_slot.index).count();
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
            Command::PathBoolean {
                lhs_sid,
                rhs_sid,
                new_path,
                new_geom,
                captured,
                ..
            } => {
                if captured.is_none() {
                    // 首次:捕获 lhs 原 (path, geom) + rhs 槽位/子树
                    let lhs_id = doc.find_by_sid(lhs_sid).ok_or_else(|| no_such(lhs_sid))?;
                    let lhs_old = match doc.nodes.get(lhs_id).unwrap().kind {
                        NodeKind::Vector { ref path } => Some(path.clone()),
                        _ => {
                            return Err(VbError::Conflict("路径查找器只作用于矢量路径节点".into()))
                        }
                    };
                    let lhs_geom = doc.nodes.get(lhs_id).unwrap().geom;
                    let slot = Self::slot_of(doc, rhs_sid)?;
                    let (_, tree) = doc
                        .extract_subtree(rhs_sid)
                        .ok_or_else(|| no_such(rhs_sid))?;
                    *captured = Some((lhs_old, lhs_geom, slot, tree));
                }
                // lhs 换成结果
                let lhs_id = doc.find_by_sid(lhs_sid).ok_or_else(|| no_such(lhs_sid))?;
                {
                    let n = doc.nodes.get_mut(lhs_id).unwrap();
                    match &mut n.kind {
                        NodeKind::Vector { path } => *path = new_path.clone(),
                        _ => return Err(VbError::Conflict("lhs 节点已不是矢量路径".into())),
                    }
                    n.geom = *new_geom;
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
            Command::SetMetaTitle { new, old } => {
                if old.is_none() {
                    *old = Some(doc.meta.title.clone());
                }
                doc.meta.title = new.clone();
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
            Command::SetGeom {
                sid,
                old,
                old_declared,
                ..
            } => {
                let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                let n = doc.nodes.get_mut(id).unwrap();
                if let Some(g) = old {
                    n.geom = *g;
                }
                if let Some(d) = old_declared {
                    n.geom_declared = *d;
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
                if let Some(s) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    let n = doc.nodes.get_mut(id).unwrap();
                    if let NodeKind::Text { text, segments, .. } = &mut n.kind {
                        *text = s.text.clone();
                        *segments = s.segs.clone();
                    }
                }
                Ok(ChangeSet {
                    structure: false,
                    style: false,
                    geometry: false,
                    text: true,
                })
            }
            Command::SetSegs { sid, old, .. } => {
                if let Some(s) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    let n = doc.nodes.get_mut(id).unwrap();
                    if let NodeKind::Text { segments, .. } = &mut n.kind {
                        *segments = s.clone();
                    }
                }
                Ok(ChangeSet {
                    structure: false,
                    style: true,
                    geometry: false,
                    text: true,
                })
            }
            Command::SetTextMode { sid, old, .. } => {
                if let Some(m) = old {
                    let id = doc.find_by_sid(sid).ok_or_else(|| no_such(sid))?;
                    let n = doc.nodes.get_mut(id).unwrap();
                    if let NodeKind::Text { mode, .. } = &mut n.kind {
                        *mode = *m;
                    }
                }
                Ok(ChangeSet {
                    structure: false,
                    style: false,
                    geometry: true,
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
            Command::PathBoolean {
                lhs_sid,
                rhs_sid,
                captured,
                ..
            } => {
                let (lhs_old, lhs_geom, slot, tree) = captured
                    .as_ref()
                    .ok_or_else(|| VbError::Parse("PathBoolean 未捕获快照".into()))?;
                // lhs 还原
                let lhs_id = doc.find_by_sid(lhs_sid).ok_or_else(|| no_such(lhs_sid))?;
                {
                    let n = doc.nodes.get_mut(lhs_id).unwrap();
                    if let (NodeKind::Vector { path }, Some(old_path)) = (&mut n.kind, lhs_old) {
                        *path = old_path.clone();
                    }
                    n.geom = *lhs_geom;
                }
                // rhs 放回原槽位
                doc.insert_tree_at(tree, &slot.parent_sid, slot.index)
                    .ok_or_else(|| no_such(rhs_sid))?;
                doc.sync_artboards();
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
            Command::SetMetaTitle { old, .. } => {
                if let Some(prev) = old {
                    doc.meta.title = prev.clone();
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
