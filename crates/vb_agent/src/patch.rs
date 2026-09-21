//! `vb_agent` 库:patch 操作解析/校验/事务(设计文档 08 篇 §五)。
//!
//! 所有 op 最终落到 `vb_doc::commands::Command` —— 与 GUI 菜单/快捷键同一条
//! 命令路径,天然获得可撤销、可回放、一致性(08 篇 §十)。

use vb_common::units::parse_px;
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, Node, NodeKind, NodeTree, TextMode};
use vb_doc::VbError;

/// 一组 patch 操作(事务)。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PatchRequest {
    /// 乐观锁:提供时必须与当前 rev 一致,否则拒绝(08 篇 §九)。
    #[serde(default)]
    pub base_rev: Option<u64>,
    pub ops: Vec<PatchOp>,
}

/// 单个 patch 操作(v0.1 支持集,08 篇 §五)。
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "op")]
pub enum PatchOp {
    #[serde(rename = "insert")]
    Insert {
        parent: String,
        #[serde(default)]
        index: Option<usize>,
        node: InsertNodeSpec,
    },
    #[serde(rename = "set_text")]
    SetText { id: String, text: String },
    #[serde(rename = "set_style")]
    SetStyle {
        id: String,
        css: std::collections::BTreeMap<String, String>,
    },
    #[serde(rename = "set_attr")]
    SetAttr {
        id: String,
        attrs: std::collections::BTreeMap<String, String>,
    },
    #[serde(rename = "move")]
    Move {
        id: String,
        parent: String,
        index: usize,
    },
    #[serde(rename = "set_box")]
    SetBox {
        id: String,
        #[serde(rename = "box")]
        box_: BoxSpec,
    },
    #[serde(rename = "rename")]
    Rename { id: String, name: String },
    #[serde(rename = "set_tag")]
    SetTag { id: String, tag: String },
    #[serde(rename = "duplicate")]
    Duplicate {
        id: String,
        #[serde(default)]
        offset: Option<OffsetSpec>,
    },
    #[serde(rename = "delete")]
    Delete { id: String },
    #[serde(rename = "group")]
    Group {
        ids: Vec<String>,
        #[serde(default)]
        name: Option<String>,
    },
    #[serde(rename = "ungroup")]
    Ungroup { id: String },
    #[serde(rename = "align")]
    Align {
        ids: Vec<String>,
        /// left|hcenter|right|top|vcenter|bottom
        mode: String,
        #[serde(default)]
        to: Option<String>,
    },
    #[serde(rename = "order")]
    Order { id: String, to: String },
    #[serde(rename = "set_token")]
    SetToken { name: String, value: String },
    #[serde(rename = "new_artboard")]
    NewArtboard {
        name: String,
        w: f64,
        h: f64,
        #[serde(default)]
        after: Option<String>,
    },
    /// 路径查找器(批次 C1):lhs 替换为布尔结果,rhs 删除。
    /// `mode` 避开 serde 内部 tag 字段名 `op`。
    #[serde(rename = "boolean")]
    Boolean {
        /// union / subtract / intersect / xor
        mode: String,
        lhs: String,
        rhs: String,
    },
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct InsertNodeSpec {
    #[serde(default = "default_tag")]
    pub tag: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub style: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub attrs: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub r#box: Option<BoxSpec>,
}

fn default_tag() -> String {
    "div".to_string()
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct BoxSpec {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct OffsetSpec {
    pub x: f64,
    pub y: f64,
}

/// patch 执行结果。
#[derive(Debug, Default, serde::Serialize)]
pub struct PatchOutcome {
    pub rev: u64,
    pub created_ids: Vec<String>,
    pub changed_ids: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn apply_patch(
    doc: &mut Document,
    undo: &mut vb_doc::UndoStack,
    req: &PatchRequest,
) -> Result<PatchOutcome, PatchError> {
    if let Some(base) = req.base_rev {
        if base != doc.rev {
            return Err(PatchError::Conflict {
                expected: base,
                current: doc.rev,
            });
        }
    }
    // 事务:全部命令先编译成功才应用(08 篇 §五:全部成功或全部回滚)
    let mut cmds = Vec::new();
    let mut warnings = Vec::new();
    for op in &req.ops {
        let (mut cs, mut ws) = compile_op(doc, op)?;
        cmds.append(&mut cs);
        warnings.append(&mut ws);
    }
    let mut outcome = PatchOutcome {
        rev: doc.rev,
        warnings,
        ..Default::default()
    };
    for c in &cmds {
        collect_affected(c, &mut outcome);
    }
    // Agent 事务禁用 undo 合并:两次相邻 patch 若目标集合相同,
    // 不允许被合并成一条 undo(08 篇:一次 patch = 一条 undo)
    let prev_merging = undo.merging_enabled;
    undo.merging_enabled = false;
    let pushed = undo.push_compound(doc, cmds);
    undo.merging_enabled = prev_merging;
    pushed.map_err(|e| PatchError::Op(e.to_string()))?;
    outcome.rev = doc.rev;
    Ok(outcome)
}

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("409 conflict: base_rev {expected} 过期,当前 {current}")]
    Conflict { expected: u64, current: u64 },
    #[error("{0}")]
    Op(String),
}

impl From<VbError> for PatchError {
    fn from(e: VbError) -> Self {
        PatchError::Op(e.to_string())
    }
}

/// 解析元素 sid 并拒绝文档根节点。root 挂在 arena 里且有确定性 sid
/// (`from_seed(0)`),`find_by_sid` 搜得到;但 root 无 parent、不属于任何
/// 画板,结构类 op 作用于它会 panic(曾击穿 MCP 主循环)或产出不可见节点。
/// 返回的 NodeId 保证非 root,调用方随后可以安全解包 parent。
fn require_child(doc: &Document, id: &str, op: &str) -> Result<vb_doc::model::NodeId, PatchError> {
    let nid = doc
        .find_by_sid(id)
        .ok_or_else(|| PatchError::Op(format!("{id} 不存在")))?;
    if nid == doc.root {
        return Err(PatchError::Op(format!("{op} 不能作用于文档根节点")));
    }
    Ok(nid)
}

fn compile_op(doc: &mut Document, op: &PatchOp) -> Result<(Vec<Command>, Vec<String>), PatchError> {
    let sid_str = |s: &str| s.to_string();
    let mut warnings = Vec::new();
    let cmds = match op {
        PatchOp::Insert {
            parent,
            index,
            node,
        } => {
            let pid = doc
                .find_by_sid(parent)
                .ok_or_else(|| PatchError::Op(format!("parent {parent} 不存在")))?;
            let mut n = build_node_from_spec(node, doc)?;
            // 容器子级坐标:相对画板累积由宿主保证;插入位置
            let idx = index.unwrap_or(doc.nodes.get(pid).unwrap().children.len());
            if let Some(b) = node.r#box {
                n.geom = Geom {
                    x: b.x,
                    y: b.y,
                    w: b.w,
                    h: b.h,
                };
            }
            let tree = NodeTree {
                node: n,
                children: vec![],
            };
            vec![Command::Insert {
                parent_sid: sid_str(parent),
                index: idx,
                tree,
            }]
        }
        PatchOp::SetText { id, text } => {
            vec![Command::SetText {
                sid: sid_str(id),
                new: text.clone(),
                old: None,
            }]
        }
        PatchOp::SetStyle { id, css } => {
            let nid = doc
                .find_by_sid(id)
                .ok_or_else(|| PatchError::Op(format!("{id} 不存在")))?;
            let mut style = doc.nodes.get(nid).unwrap().style.clone();
            for (p, v) in css {
                // 与 insert 路径一致:必须过 Decl::parse 校验,否则非法声明
                // 原样落盘损坏 CSS(导出时不做二次过滤)
                let d = vb_css::Decl::parse(&format!("{p}: {v}"))
                    .ok_or_else(|| PatchError::Op(format!("非法 CSS 声明: {p}: {v}")))?;
                if let Some(existing) = style.iter_mut().find(|e| e.prop == d.prop) {
                    existing.value = d.value;
                } else {
                    style.push(d);
                }
            }
            vec![Command::SetStyle {
                sid: sid_str(id),
                new: style,
                old: None,
            }]
        }
        PatchOp::SetAttr { id, attrs } => {
            let nid = doc
                .find_by_sid(id)
                .ok_or_else(|| PatchError::Op(format!("{id} 不存在")))?;
            // class/style/data-vb-* 由场景图字段或导出层生成,attrs 里再写
            // 一份会在导出时产生重复 HTML 属性(html5ever 只取第一个,静默丢编辑);
            // id 例外:导出层专门从 attrs 读 id 输出
            const RESERVED: &[&str] = &["class", "style", "data-vb-id", "data-vb-name"];
            for k in attrs.keys() {
                if RESERVED.contains(&k.as_str()) {
                    return Err(PatchError::Op(format!(
                        "set_attr 不允许写保留属性:{k}(class/style 走 set_style,名称走 rename)"
                    )));
                }
            }
            let mut merged = doc.nodes.get(nid).unwrap().attrs.clone();
            for (k, v) in attrs {
                merged.insert(k.clone(), v.clone());
            }
            let new: Vec<(String, String)> = merged.into_iter().collect();
            vec![Command::SetAttrs {
                sid: sid_str(id),
                new,
                old: None,
            }]
        }
        PatchOp::Move { id, parent, index } => {
            require_child(doc, id, "move")?;
            vec![Command::Move {
                sid: sid_str(id),
                new_parent_sid: sid_str(parent),
                new_index: *index,
                old: None,
            }]
        }
        PatchOp::SetBox { id, box_ } => vec![Command::SetGeom {
            sid: sid_str(id),
            new: Geom {
                x: box_.x,
                y: box_.y,
                w: box_.w,
                h: box_.h,
            },
            old: None,
            old_declared: None,
        }],
        PatchOp::Rename { id, name } => {
            vec![Command::Rename {
                sid: sid_str(id),
                new: name.clone(),
                old: None,
            }]
        }
        PatchOp::SetTag { id, tag } => {
            vec![Command::SetTag {
                sid: sid_str(id),
                new: tag.clone(),
                old: None,
            }]
        }
        PatchOp::Duplicate { id, offset } => {
            let nid = doc
                .find_by_sid(id)
                .ok_or_else(|| PatchError::Op(format!("{id} 不存在")))?;
            let parent_sid = doc
                .nodes
                .get(nid)
                .and_then(|n| n.parent)
                .and_then(|p| doc.nodes.get(p))
                .map(|p| p.sid.as_str().to_string())
                .ok_or_else(|| PatchError::Op("duplicate 需要有父级的节点".into()))?;
            // 深拷贝整棵子树(此前只复制根节点,容器内容全部丢失);
            // sid 是身份(ADR-0010),副本全树重新分配
            let mut tree = NodeTree::from_document(doc, nid)
                .ok_or_else(|| PatchError::Op("duplicate 取子树失败".into()))?;
            re_sid_tree(doc, &mut tree);
            let (dx, dy) = match offset {
                Some(o) => (o.x, o.y),
                None => (24.0, 24.0),
            };
            tree.node.geom.x += dx;
            tree.node.geom.y += dy;
            vec![Command::Insert {
                parent_sid,
                index: usize::MAX,
                tree,
            }]
        }
        PatchOp::Delete { id } => {
            require_child(doc, id, "delete")?;
            vec![Command::Delete {
                target_sid: sid_str(id),
                captured: None,
            }]
        }
        PatchOp::Group { ids, name } => {
            for id in ids {
                require_child(doc, id, "group")?;
            }
            let group_sid = doc.alloc_sid_for_dup().as_str().to_string();
            vec![Command::Group {
                member_sids: ids.clone(),
                name: name.clone().unwrap_or_else(|| "编组".to_string()),
                group_sid,
                old_slots: None,
            }]
        }
        PatchOp::Ungroup { id } => {
            require_child(doc, id, "ungroup")?;
            vec![Command::Ungroup {
                group_sid: sid_str(id),
                captured: None,
            }]
        }
        PatchOp::Align { ids, mode, to } => {
            // `to` 缺省 = 选区(公共包围盒);可选 artboard / key_object(03-5-2)
            let (c, w) = align_cmds(doc, ids, mode, to.as_deref())?;
            warnings = w;
            c
        }
        PatchOp::Order { id, to } => {
            let nid = require_child(doc, id, "order")?;
            let parent = doc
                .nodes
                .get(nid)
                .unwrap()
                .parent
                .ok_or_else(|| PatchError::Op(format!("{id} 没有父级,无法调序")))?;
            let len = doc.nodes.get(parent).unwrap().children.len();
            let new_index = match to.as_str() {
                "front" => len.saturating_sub(1),
                "back" => 0,
                "forward" => {
                    let cur = doc
                        .nodes
                        .get(parent)
                        .unwrap()
                        .children
                        .iter()
                        .position(|&c| c == nid)
                        .unwrap_or(0);
                    (cur + 1).min(len - 1)
                }
                "backward" => {
                    let cur = doc
                        .nodes
                        .get(parent)
                        .unwrap()
                        .children
                        .iter()
                        .position(|&c| c == nid)
                        .unwrap_or(0);
                    cur.saturating_sub(1)
                }
                other => return Err(PatchError::Op(format!("未知 order 目标:{other}"))),
            };
            vec![Command::Move {
                sid: sid_str(id),
                new_parent_sid: doc.nodes.get(parent).unwrap().sid.as_str().to_string(),
                new_index,
                old: None,
            }]
        }
        PatchOp::SetToken { name, value } => {
            vec![Command::SetToken {
                name: name.clone(),
                new: value.clone(),
                old: None,
            }]
        }
        PatchOp::NewArtboard { name, w, h, after } => {
            let sid = doc.alloc_sid_for_dup();
            let mut n = Node::new(NodeKind::Artboard, name.clone(), sid);
            n.geom = Geom {
                x: 0.0,
                y: 0.0,
                w: *w,
                h: *h,
            };
            // 画板纵向排布:放到现有画板最下方
            let y = doc
                .artboards
                .iter()
                .filter_map(|&a| doc.nodes.get(a).map(|n| n.geom.y + n.geom.h))
                .fold(0.0f64, f64::max)
                + 80.0;
            n.geom.y = y;
            let index = match after {
                Some(a_sid) => doc
                    .find_by_sid(a_sid)
                    .and_then(|aid| {
                        doc.nodes
                            .get(doc.root)
                            .unwrap()
                            .children
                            .iter()
                            .position(|&c| c == aid)
                    })
                    .map(|p| p + 1)
                    .unwrap_or(usize::MAX),
                None => usize::MAX,
            };
            vec![Command::Insert {
                parent_sid: doc.nodes.get(doc.root).unwrap().sid.as_str().to_string(),
                index,
                tree: NodeTree {
                    node: n,
                    children: vec![],
                },
            }]
        }
        PatchOp::Boolean { mode, lhs, rhs } => {
            let op_kind = vb_tools::boolean::BooleanOp::parse(mode)
                .ok_or_else(|| PatchError::Op(format!("未知布尔运算:{mode}")))?;
            let lhs_id = require_child(doc, lhs, "boolean")?;
            let rhs_id = require_child(doc, rhs, "boolean")?;
            let (new_path, new_geom) =
                vb_tools::boolean::path_boolean_nodes(doc, op_kind, lhs_id, rhs_id)
                    .map_err(PatchError::Op)?;
            vec![Command::PathBoolean {
                op: op_kind.as_str().to_string(),
                lhs_sid: lhs.clone(),
                rhs_sid: rhs.clone(),
                new_path,
                new_geom,
                captured: None,
            }]
        }
    };
    Ok((cmds, warnings))
}

/// 对齐(相对所属画板;08 篇 §五 align op)。
/// 对齐(阶段 2 / 03-5 的 Agent 侧入口)。
///
/// **坐标系纪律(2026-09-21 修复)**:节点的 `geom` 是**父相对**坐标,
/// 而"对齐"必须比较**绝对盒**。此前这里把 `geom.x/y` 当画板本地坐标直接
/// 算 `min_x / 中心 / max_r`,于是:
/// 1. 不同父级的成员被拿错参照系的数字比较 → 对齐结果错(浏览器里也跳);
/// 2. 写回的 `geom` 落盘后与重导入值不等 → **每存一次漂移一次**,L1 幂等被打穿。
///
/// 现在改为:目标盒与成员盒都取 [`vb_tools::align::AbsBox`](绝对盒),
/// 位移用绝对系差值平移节点自身 `geom`(平移与参照系无关)。
/// 跨画板的成员**不能混**进同一组 min/max,按画板分组各自对齐;
/// 需要 ≥2 个成员的模式下,落单成员跳过并给出 warning。
fn align_cmds(
    doc: &Document,
    ids: &[String],
    mode: &str,
    to: Option<&str>,
) -> Result<(Vec<Command>, Vec<String>), PatchError> {
    use vb_tools::align::{aligned_delta, AbsBox, AlignMode, AlignTo};

    let mode =
        AlignMode::parse(mode).ok_or_else(|| PatchError::Op(format!("未知对齐模式:{mode}")))?;
    let to = AlignTo::parse(to.unwrap_or("selection"))
        .ok_or_else(|| PatchError::Op(format!("未知对齐目标:{:?}", to)))?;
    // 「对齐到选区/关键对象」需要至少两个成员才有意义;「对齐到画板」单个也可
    let need_two = matches!(to, AlignTo::Selection | AlignTo::KeyObject);

    // 按公共画板分组(保序)
    type Member = (String, Geom, AbsBox);
    let mut groups: Vec<(vb_doc::model::NodeId, Vec<Member>)> = Vec::new();
    let mut warnings = Vec::new();
    for id in ids {
        let nid = doc
            .find_by_sid(id)
            .ok_or_else(|| PatchError::Op(format!("{id} 不存在")))?;
        if nid == doc.root {
            // root 的 geom 无意义且不属于任何画板:跳过而不是把它
            // 混进 root 哨兵分组一起挪动
            warnings.push("align 跳过文档根节点".to_string());
            continue;
        }
        let Some(bb) = AbsBox::of(doc, nid) else {
            warnings.push(format!("align 跳过 {id}:无绝对几何"));
            continue;
        };
        let ab = artboard_of(doc, nid);
        let g = doc.nodes.get(nid).unwrap().geom;
        if let Some(entry) = groups.iter_mut().find(|(a, _)| *a == ab) {
            entry.1.push((id.clone(), g, bb));
        } else {
            groups.push((ab, vec![(id.clone(), g, bb)]));
        }
    }

    let mut cmds = Vec::new();
    for (ab, members) in &groups {
        if need_two && members.len() < 2 {
            let name = doc
                .find_by_sid(&members[0].0)
                .and_then(|nid| doc.nodes.get(nid))
                .map(|n| n.name.clone())
                .unwrap_or_else(|| members[0].0.clone());
            warnings.push(format!("align 跳过 {name}:与其余成员不在同一画板或落单"));
            continue;
        }
        let boxes: Vec<AbsBox> = members.iter().map(|(_, _, b)| *b).collect();
        let target = match to {
            AlignTo::Selection => AbsBox::union(&boxes),
            // 关键对象 = **最后**选中者(与画布/面板同口径)
            AlignTo::KeyObject => boxes.last().copied(),
            AlignTo::Artboard => doc
                .nodes
                .get(*ab)
                .map(|n| AbsBox::new(0.0, 0.0, n.geom.w, n.geom.h)),
        };
        let Some(target) = target else {
            warnings.push("align 跳过:无法确定对齐目标".to_string());
            continue;
        };
        for (id, g, bb) in members {
            let (dx, dy) = aligned_delta(mode, bb, &target);
            cmds.push(Command::SetGeom {
                sid: id.clone(),
                new: Geom {
                    x: g.x + dx,
                    y: g.y + dy,
                    w: g.w,
                    h: g.h,
                },
                old: None,
                old_declared: None,
            });
        }
    }
    Ok((cmds, warnings))
}

/// 节点所属画板(沿 parent 链上溯);root 之下找不到画板时返回 root 哨兵。
fn artboard_of(doc: &Document, mut id: vb_doc::model::NodeId) -> vb_doc::model::NodeId {
    while let Some(n) = doc.nodes.get(id) {
        if matches!(n.kind, NodeKind::Artboard) {
            return id;
        }
        match n.parent {
            Some(p) => id = p,
            None => break,
        }
    }
    doc.root
}

/// 递归重分配子树内全部 sid(duplicate:副本是新元素,身份必须全新)。
fn re_sid_tree(doc: &mut Document, tree: &mut NodeTree) {
    tree.node.sid = doc.alloc_sid_for_dup();
    for c in &mut tree.children {
        re_sid_tree(doc, c);
    }
}

fn build_node_from_spec(spec: &InsertNodeSpec, doc: &mut Document) -> Result<Node, PatchError> {
    let name = spec.name.clone().unwrap_or_else(|| spec.tag.clone());
    let sid = doc.alloc_sid_for_dup();
    let mut n = match &spec.text {
        Some(t) => {
            let mut n = Node::new(
                NodeKind::Text {
                    text: t.clone(),
                    mode: TextMode::Point,
                    segments: Vec::new(),
                },
                name,
                sid,
            );
            n.tag = spec.tag.clone();
            n
        }
        None => {
            let mut n = Node::new(NodeKind::Box, name, sid);
            n.tag = spec.tag.clone();
            n
        }
    };
    if let Some(style) = &spec.style {
        n.style = parse_style_map(style);
    }
    if let Some(attrs) = &spec.attrs {
        const RESERVED: &[&str] = &["class", "style", "data-vb-id", "data-vb-name"];
        for k in attrs.keys() {
            if RESERVED.contains(&k.as_str()) {
                return Err(PatchError::Op(format!(
                    "insert 的 attrs 不允许写保留属性:{k}(class 走 style 之外的专用字段)"
                )));
            }
        }
        n.attrs = attrs.clone();
    }
    // 默认几何
    n.geom = Geom {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 40.0,
    };
    // style 中的几何键由导出层重建;此处直接读取(先取值后改字段,避免借用冲突)
    let get = |p: &str, style: &[vb_css::Decl]| -> Option<f64> {
        style
            .iter()
            .find(|d| d.prop == p)
            .map(|d| d.value.clone())
            .and_then(|v| parse_px(&v))
    };
    if let Some(x) = get("left", &n.style) {
        n.geom.x = x;
    }
    if let Some(y) = get("top", &n.style) {
        n.geom.y = y;
    }
    if let Some(w) = get("width", &n.style) {
        n.geom.w = w;
    }
    if let Some(h) = get("height", &n.style) {
        n.geom.h = h;
    }
    for p in ["left", "top", "width", "height", "position"] {
        n.style_remove(p);
    }
    Ok(n)
}

fn parse_style_map(css: &std::collections::BTreeMap<String, String>) -> Vec<vb_css::Decl> {
    css.iter()
        .filter_map(|(p, v)| vb_css::Decl::parse(&format!("{p}: {v}")))
        .collect()
}

fn collect_affected(cmd: &Command, out: &mut PatchOutcome) {
    match cmd {
        Command::Insert { tree, .. } => {
            let sid = tree.root_sid().to_string();
            if !out.created_ids.contains(&sid) {
                out.created_ids.push(sid.clone());
            }
            out.changed_ids.push(sid);
        }
        Command::Delete { target_sid, .. } => out.changed_ids.push(target_sid.clone()),
        Command::PathBoolean {
            lhs_sid, rhs_sid, ..
        } => {
            // lhs 保留(变为结果),rhs 被删除
            out.changed_ids.push(lhs_sid.clone());
            out.changed_ids.push(rhs_sid.clone());
        }
        Command::Move { sid, .. }
        | Command::SetGeom { sid, .. }
        | Command::SetStyle { sid, .. }
        | Command::SetText { sid, .. }
        | Command::SetSegs { sid, .. }
        | Command::SetTextMode { sid, .. }
        | Command::SetAttrs { sid, .. }
        | Command::Rename { sid, .. }
        | Command::SetTag { sid, .. }
        | Command::SetVector { sid, .. }
        | Command::SetFlags { sid, .. } => out.changed_ids.push(sid.clone()),
        Command::Group {
            member_sids,
            group_sid,
            ..
        } => {
            out.changed_ids.push(group_sid.clone());
            out.changed_ids.extend(member_sids.clone());
        }
        Command::Ungroup {
            group_sid,
            captured,
        } => {
            out.changed_ids.push(group_sid.clone());
            if let Some((_, tree)) = captured {
                for c in &tree.children {
                    out.changed_ids.push(c.node.sid.as_str().to_string());
                }
            }
        }
        Command::Compound { cmds } => {
            for c in cmds {
                collect_affected(c, out);
            }
        }
        // 令牌 / 文档标题:非节点级变更,无受影响 sid
        Command::SetToken { .. } | Command::SetMetaTitle { .. } => {}
    }
}

/// 全部 PatchOp 的 serde 名(穷尽 match:新增变体不补 arm 就编译不过,
/// MCP tools/list 与文档据此保持同步 —— 此前硬编码「13 种 op」漂移成 16)。
pub fn patch_op_name(op: &PatchOp) -> &'static str {
    match op {
        PatchOp::Insert { .. } => "insert",
        PatchOp::SetText { .. } => "set_text",
        PatchOp::SetStyle { .. } => "set_style",
        PatchOp::SetAttr { .. } => "set_attr",
        PatchOp::Move { .. } => "move",
        PatchOp::SetBox { .. } => "set_box",
        PatchOp::Rename { .. } => "rename",
        PatchOp::SetTag { .. } => "set_tag",
        PatchOp::Duplicate { .. } => "duplicate",
        PatchOp::Delete { .. } => "delete",
        PatchOp::Group { .. } => "group",
        PatchOp::Ungroup { .. } => "ungroup",
        PatchOp::Align { .. } => "align",
        PatchOp::Order { .. } => "order",
        PatchOp::SetToken { .. } => "set_token",
        PatchOp::NewArtboard { .. } => "new_artboard",
        PatchOp::Boolean { .. } => "boolean",
    }
}

pub const PATCH_OP_NAMES: &[&str] = &[
    "insert",
    "set_text",
    "set_style",
    "set_attr",
    "move",
    "set_box",
    "rename",
    "set_tag",
    "duplicate",
    "delete",
    "group",
    "ungroup",
    "align",
    "order",
    "set_token",
    "new_artboard",
    "boolean",
];
