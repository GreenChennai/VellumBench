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
    for op in &req.ops {
        cmds.extend(compile_op(doc, op)?);
    }
    let mut outcome = PatchOutcome {
        rev: doc.rev,
        ..Default::default()
    };
    for c in &cmds {
        collect_affected(c, &mut outcome);
    }
    undo.push_compound(doc, cmds)
        .map_err(|e| PatchError::Op(e.to_string()))?;
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

fn style_map_to_decls(css: &std::collections::BTreeMap<String, String>) -> Vec<vb_css::Decl> {
    css.iter()
        .map(|(p, v)| vb_css::Decl {
            prop: p.to_ascii_lowercase(),
            value: v.clone(),
            important: false,
        })
        .collect()
}

fn compile_op(doc: &mut Document, op: &PatchOp) -> Result<Vec<Command>, PatchError> {
    let sid_str = |s: &str| s.to_string();
    Ok(match op {
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
            for d in style_map_to_decls(css) {
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
            let src = doc.nodes.get(nid).unwrap().clone();
            let mut copy = src.clone();
            copy.sid = doc.alloc_sid_for_dup();
            if let Some(o) = offset {
                copy.geom.x += o.x;
                copy.geom.y += o.y;
            } else {
                copy.geom.x += 24.0;
                copy.geom.y += 24.0;
            }
            let parent_sid = doc
                .nodes
                .get(nid)
                .and_then(|n| n.parent)
                .and_then(|p| doc.nodes.get(p))
                .map(|p| p.sid.as_str().to_string())
                .ok_or_else(|| PatchError::Op("duplicate 需要有父级的节点".into()))?;
            let tree = NodeTree {
                node: copy,
                children: vec![],
            };
            vec![Command::Insert {
                parent_sid,
                index: usize::MAX,
                tree,
            }]
        }
        PatchOp::Delete { id } => {
            vec![Command::Delete {
                target_sid: sid_str(id),
                captured: None,
            }]
        }
        PatchOp::Group { ids, name } => {
            let group_sid = doc.alloc_sid_for_dup().as_str().to_string();
            vec![Command::Group {
                member_sids: ids.clone(),
                name: name.clone().unwrap_or_else(|| "编组".to_string()),
                group_sid,
                old_slots: None,
            }]
        }
        PatchOp::Ungroup { id } => {
            vec![Command::Ungroup {
                group_sid: sid_str(id),
                captured: None,
            }]
        }
        PatchOp::Align { ids, mode, to } => {
            let _ = to; // v0.1:对齐到画板(selection 集合的公共画板)
            align_cmds(doc, ids, mode)?
        }
        PatchOp::Order { id, to } => {
            let nid = doc
                .find_by_sid(id)
                .ok_or_else(|| PatchError::Op(format!("{id} 不存在")))?;
            let parent = doc.nodes.get(nid).unwrap().parent.unwrap();
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
    })
}

/// 对齐(相对所属画板;08 篇 §五 align op)。
fn align_cmds(doc: &Document, ids: &[String], mode: &str) -> Result<Vec<Command>, PatchError> {
    let mut geoms: Vec<(String, Geom)> = Vec::new();
    for id in ids {
        let nid = doc
            .find_by_sid(id)
            .ok_or_else(|| PatchError::Op(format!("{id} 不存在")))?;
        geoms.push((id.clone(), doc.nodes.get(nid).unwrap().geom));
    }
    if geoms.is_empty() {
        return Ok(vec![]);
    }
    let min_x = geoms.iter().map(|(_, g)| g.x).fold(f64::INFINITY, f64::min);
    let min_y = geoms.iter().map(|(_, g)| g.y).fold(f64::INFINITY, f64::min);
    let max_r = geoms
        .iter()
        .map(|(_, g)| g.x + g.w)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_b = geoms
        .iter()
        .map(|(_, g)| g.y + g.h)
        .fold(f64::NEG_INFINITY, f64::max);
    let center_x = (min_x + max_r) / 2.0;
    let center_y = (min_y + max_b) / 2.0;

    let moved = |g: Geom, x: f64, y: f64| Command::SetGeom {
        sid: String::new(),
        new: Geom { x, y, ..g },
        old: None,
    };
    let mut cmds = Vec::new();
    for (id, g) in geoms {
        let mut c = match mode {
            "left" => moved(g, min_x, g.y),
            "right" => moved(g, max_r - g.w, g.y),
            "hcenter" => moved(g, center_x - g.w / 2.0, g.y),
            "top" => moved(g, g.x, min_y),
            "bottom" => moved(g, g.x, max_b - g.h),
            "vcenter" => moved(g, g.x, center_y - g.h / 2.0),
            other => return Err(PatchError::Op(format!("未知对齐模式:{other}"))),
        };
        if let Command::SetGeom { sid, .. } = &mut c {
            *sid = id;
        }
        cmds.push(c);
    }
    Ok(cmds)
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
            out.changed_ids.push(tree.root_sid().to_string());
        }
        Command::Delete { target_sid, .. } => out.changed_ids.push(target_sid.clone()),
        Command::Move { sid, .. }
        | Command::SetGeom { sid, .. }
        | Command::SetStyle { sid, .. }
        | Command::SetText { sid, .. }
        | Command::SetAttrs { sid, .. }
        | Command::Rename { sid, .. }
        | Command::SetTag { sid, .. }
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
        Command::SetToken { .. } => {}
    }
}
