//! 05-8 符号 / 组件系统(ADR-VB-L10,台账 09-H)。
//!
//! **模型**(05-8-1):主件(Main)原型是文档内的一棵**真实节点子树**,
//! 挂在与 `root` 平级的 `Document::defs_root` 下;每个主件一个定义容器
//! (导出为 `<div class="vb-symbol-defs" hidden data-vb-id=… data-vb-name=…>`,
//! `hidden` 为原生属性,浏览器不渲染,CSS 不出规则)。实例 = 原型的
//! **真实 DOM 副本**(全新 sid)+ 根节点三个标记属性:
//! `data-vb-symbol`(主件名)/ `data-vb-symbol-ref`(主件容器 sid)/
//! `data-vb-symbol-overrides`(覆盖字段列表,可缺省)。
//!
//! **覆盖口径**(05-8-2,定死):实例子树即覆盖后的**真实内容**;
//! `data-vb-symbol-overrides` 只存「哪些字段被覆盖了」的字段键列表
//! (紧凑 JSON 字符串数组),供同步时保护。字段键格式:
//!
//! ```text
//! "{路径}:{字段}"      路径 = 实例根到目标节点的子节点下标,点分;
//!                      根自身 = 空串(键以 ':' 开头)
//! 字段 = "text"                文本内容(含富文本段注记,一体保护)
//!      | "style:{prop}"        单条 CSS 声明(实例缺该声明 = 同步时删除)
//!      | "attr:{name}"         单个 HTML 属性(实例缺该属性 = 同步时删除)
//!      | "geom"                节点几何(含显式几何标志位)
//! ```
//!
//! 例:`":text"` 根文本、`"0:style:color"` 第一个子节点的颜色、
//! `"1.0:attr:aria-label"` 第二个子节点第一个孙节点的 aria-label。
//! 键字符集限 `[a-zA-Z0-9:._-]`(prop/attr 名天然满足),因此 JSON 解析
//! 无需处理结构转义(序列化仍防御性转义 `"` 与 `\`)。
//!
//! **主件同步**(05-8-3):编辑主件(命令目标落在 `defs_root` 子树内)→
//! `UndoStack::push` 收口包装一条 [`Command::SymbolSync`](懒构建):首次
//! apply 时按**当次编辑之后**的主件内容,为每个实例折算「替换子树 +
//! 根字段同步」事务(内部复用 05-3 的 `MultiResult` 多结果底座:源 =
//! 实例根的既有子节点,结果 = 主件子树的新 sid 克隆,锚点 = 首子节点,
//! ReplaceAnchor 原位替换;z 序保留)。实例根**不整体替换** ——
//! sid / 名称 / 几何 / 标记全保留(`data-vb-id` 全生命周期稳定,
//! CONTEXT.md);根字段差异折算为 SetText/SetStyle/SetAttrs。被覆盖
//! 字段从旧实例子树按路径取值回填到新子树;主件已无对应路径的覆盖键
//! 被修剪。同步单向(主件 → 实例),编辑实例不反写主件。
//!
//! **已知边界**(诚实记录,不做假同步):实例内部的结构性编辑
//! (增删/移动/改标签/矢量路径)不登记覆盖,下一次主件同步以主件结构
//! 为准;主件根的 tag 变更不传播到实例根(根不整体替换)。

use std::collections::HashMap;

use vb_css::Decl;

use crate::commands::{Command, MultiResultSlot};
use crate::model::{Document, Geom, Node, NodeId, NodeKind, NodeTree, TextSeg};
use crate::VbError;

/// 主件定义容器标记类(导入识别 / 导出补写;实例标记用属性,不用类)。
pub const SYMBOL_DEF_CLASS: &str = "vb-symbol-defs";
/// 实例标记:主件名。
pub const ATTR_SYMBOL: &str = "data-vb-symbol";
/// 实例标记:主件定义容器 sid(`data-vb-id`)。
pub const ATTR_SYMBOL_REF: &str = "data-vb-symbol-ref";
/// 实例标记:覆盖字段键列表(紧凑 JSON 字符串数组;无覆盖不写该属性)。
pub const ATTR_OVERRIDES: &str = "data-vb-symbol-overrides";

/// 实例根的三个标记属性(同步 / 分离时统一处理)。
const MARKER_ATTRS: [&str; 3] = [ATTR_SYMBOL, ATTR_SYMBOL_REF, ATTR_OVERRIDES];

fn decl(prop: &str, value: &str) -> Decl {
    Decl {
        prop: prop.to_string(),
        value: value.to_string(),
        important: false,
    }
}

// ─────────────────────────── 覆盖键 JSON(无 serde,最小实现) ───────────────────────────

/// 覆盖键字符集白名单:字段键只允许这些字符(prop/attr 名天然满足),
/// 超出者**不登记**(防御:保证 JSON 无需转义也不会破坏结构)。
fn key_char_ok(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '_' | '-')
}

/// 序列化为紧凑 JSON 字符串数组(防御性转义 `"` 与 `\`;白名单键不会用到)。
fn serialize_keys(keys: &[String]) -> String {
    let mut out = String::from("[");
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('"');
        for c in k.chars() {
            if c == '"' || c == '\\' {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
    }
    out.push(']');
    out
}

/// 解析紧凑 JSON 字符串数组;坏结构一律按空表处理(属性是保真透传,
/// 不因手改坏值让整个文档打不开)。
fn parse_keys(raw: &str) -> Vec<String> {
    let s = raw.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return Vec::new();
    }
    let inner = &s[1..s.len() - 1];
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut esc = false;
    for c in inner.chars() {
        if in_str {
            if esc {
                cur.push(c);
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_str = true;
        }
        // 字符串外的逗号 / 空白忽略(键内不会出现逗号)
    }
    out.retain(|k| k.chars().all(key_char_ok));
    out
}

/// 读实例根当前的覆盖键列表(无属性 = 空)。
pub fn override_keys(attrs: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    attrs
        .get(ATTR_OVERRIDES)
        .map(|v| parse_keys(v))
        .unwrap_or_default()
}

/// 字段键:`{路径}:{字段}`(路径点分下标,根 = 空串)。
fn field_key(path: &[usize], field: &str) -> String {
    let mut k = String::new();
    for (i, p) in path.iter().enumerate() {
        if i > 0 {
            k.push('.');
        }
        k.push_str(&p.to_string());
    }
    k.push(':');
    k.push_str(field);
    k
}

/// 拆字段键 → (路径, 字段);坏键返回 None。
fn split_key(key: &str) -> Option<(Vec<usize>, &str)> {
    let (p, f) = key.split_once(':')?;
    let path = if p.is_empty() {
        Vec::new()
    } else {
        let mut v = Vec::new();
        for seg in p.split('.') {
            v.push(seg.parse::<usize>().ok()?);
        }
        v
    };
    Some((path, f))
}

// ─────────────────────────── 查询 ───────────────────────────

/// 节点是否带实例标记(实例根)。
pub fn is_instance_root(n: &Node) -> bool {
    n.attrs.contains_key(ATTR_SYMBOL)
}

/// 从节点(含自身)向上找最近的实例根。
pub fn instance_root_of(doc: &Document, mut id: NodeId) -> Option<NodeId> {
    loop {
        let n = doc.nodes.get(id)?;
        if is_instance_root(n) {
            return Some(id);
        }
        id = n.parent?;
    }
}

/// 从节点(含自身)向上找其所属**主件定义容器**(`defs_root` 的直接子
/// 节点)。`defs_root` 自身与其外部节点返回 None。
pub fn def_container_of(doc: &Document, mut id: NodeId) -> Option<NodeId> {
    let defs_root = doc.defs_root;
    loop {
        if id == defs_root {
            return None;
        }
        let n = doc.nodes.get(id)?;
        if n.parent == Some(defs_root) {
            return Some(id);
        }
        id = n.parent?;
    }
}

/// 节点(含自身)是否位于主件定义区(`defs_root` 子树内,不含其根)。
pub fn is_in_defs(doc: &Document, id: NodeId) -> bool {
    def_container_of(doc, id).is_some()
}

/// 引用某主件容器的全部实例根(slotmap 序,确定性)。
pub fn instances_of_container(doc: &Document, container: NodeId) -> Vec<NodeId> {
    let Some(c) = doc.nodes.get(container) else {
        return Vec::new();
    };
    let ref_sid = c.sid.as_str();
    doc.nodes
        .iter()
        .filter(|(_, n)| {
            n.attrs
                .get(ATTR_SYMBOL_REF)
                .map(|v| v == ref_sid)
                .unwrap_or(false)
        })
        .map(|(id, _)| id)
        .collect()
}

/// 按名字找主件定义容器。
pub fn def_container_by_name(doc: &Document, name: &str) -> Option<NodeId> {
    let root = doc.nodes.get(doc.defs_root)?;
    root.children
        .iter()
        .copied()
        .find(|&c| doc.nodes.get(c).map(|n| n.name == name).unwrap_or(false))
}

/// 取一个未被占用的主件名(`base`、`base 2`、`base 3`…)。
pub fn next_symbol_name(doc: &mut Document, base: &str) -> String {
    let base = base.trim();
    let base = if base.is_empty() { "组件" } else { base };
    if def_container_by_name(doc, base).is_none() {
        return base.to_string();
    }
    for i in 2..1000 {
        let cand = format!("{base} {i}");
        if def_container_by_name(doc, &cand).is_none() {
            return cand;
        }
    }
    // 极端兜底:用新 sid 拼名(不再查重,1000 重名不会发生)
    format!("{} {}", base, doc.alloc_sid())
}

// ─────────────────────────── 子树克隆 / 覆盖回填 ───────────────────────────

/// 深拷贝文档节点子树且**整树重新分配 sid**(实例是独立元素,按
/// ADR-0010 语义必须有自己的稳定 id)。
fn clone_tree_fresh(doc: &mut Document, id: NodeId) -> NodeTree {
    let node = doc.nodes.get(id).expect("clone_tree_fresh: 节点存在");
    let kids = node.children.clone();
    let mut node = node.clone();
    node.sid = doc.alloc_sid();
    node.parent = None;
    node.children = Vec::new();
    let children = kids.into_iter().map(|c| clone_tree_fresh(doc, c)).collect();
    NodeTree { node, children }
}

/// 深拷贝一份 `NodeTree` 快照并重新分配 sid(源不是文档节点 ——
/// 主件原型在构建期以快照形式传入时用)。
fn clone_snapshot_fresh(doc: &mut Document, src: &NodeTree) -> NodeTree {
    let mut node = src.node.clone();
    node.sid = doc.alloc_sid();
    node.parent = None;
    node.children = Vec::new();
    let children = src
        .children
        .iter()
        .map(|c| clone_snapshot_fresh(doc, c))
        .collect();
    NodeTree { node, children }
}

/// 从 NodeTree 按(根相对)下标路径取子树;根自身 = 空路径。
fn tree_at<'a>(root: &'a NodeTree, path: &[usize]) -> Option<&'a NodeTree> {
    let mut cur = root;
    for &i in path {
        cur = cur.children.get(i)?;
    }
    Some(cur)
}

/// 一个实例节点在同步时需要保护的字段值(从旧实例子树按路径提取)。
#[derive(Default, Clone)]
struct NodeProtections {
    /// "text" 字段:文本内容 + 段注记(一体保护)。
    text: Option<(String, Vec<TextSeg>)>,
    /// "style:{prop}" 字段:None = 实例已删除该声明(同步时一并删)。
    style: Vec<(String, Option<String>)>,
    /// "attr:{name}" 字段:None = 实例已删除该属性(同步时一并删)。
    attrs: Vec<(String, Option<String>)>,
    /// "geom" 字段:几何 + 显式几何标志位。
    geom: Option<(Geom, [bool; 4], Option<String>, bool)>,
}

/// 从旧实例树提取全部覆盖键的保护值;路径在旧树上不存在 → 丢弃该键。
fn extract_protections(
    old_root: &NodeTree,
    keys: &[String],
) -> HashMap<Vec<usize>, NodeProtections> {
    let mut map: HashMap<Vec<usize>, NodeProtections> = HashMap::new();
    for key in keys {
        let Some((path, field)) = split_key(key) else {
            continue;
        };
        let Some(t) = tree_at(old_root, &path) else {
            continue;
        };
        let p = map.entry(path).or_default();
        if field == "text" {
            if let NodeKind::Text { text, segments, .. } = &t.node.kind {
                p.text = Some((text.clone(), segments.clone()));
            }
        } else if field == "geom" {
            p.geom = Some((
                t.node.geom,
                t.node.authored,
                t.node.authored_position.clone(),
                t.node.geom_declared,
            ));
        } else if let Some(prop) = field.strip_prefix("style:") {
            let v = t.node.style_get(prop).map(str::to_string);
            p.style.push((prop.to_string(), v));
        } else if let Some(name) = field.strip_prefix("attr:") {
            let v = t.node.attrs.get(name).cloned();
            p.attrs.push((name.to_string(), v));
        }
    }
    map
}

/// 把保护值回填到新克隆子树的对应路径节点上(路径在新树不存在 → 忽略)。
fn patch_tree(
    tree: &mut NodeTree,
    path: &mut Vec<usize>,
    prot: &HashMap<Vec<usize>, NodeProtections>,
) {
    if let Some(p) = prot.get(path) {
        if let Some((text, segs)) = &p.text {
            if let NodeKind::Text {
                text: t, segments, ..
            } = &mut tree.node.kind
            {
                *t = text.clone();
                *segments = segs.clone();
            }
        }
        for (prop, v) in &p.style {
            match v {
                Some(val) => tree.node.style_set(prop, val),
                None => {
                    tree.node.style_remove(prop);
                }
            }
        }
        for (name, v) in &p.attrs {
            match v {
                Some(val) => {
                    tree.node.attrs.insert(name.clone(), val.clone());
                }
                None => {
                    tree.node.attrs.remove(name);
                }
            }
        }
        if let Some((g, authored, pos, declared)) = &p.geom {
            tree.node.geom = *g;
            tree.node.authored = *authored;
            tree.node.authored_position = pos.clone();
            tree.node.geom_declared = *declared;
        }
    }
    for (i, c) in tree.children.iter_mut().enumerate() {
        path.push(i);
        patch_tree(c, path, prot);
        path.pop();
    }
}

/// 整树平移(扁平模型:全部节点同加 dx/dy;MultiResult 纪律 ——
/// 结果几何相对锚点父级,帧换算是调用方职责)。
fn shift_tree(t: &mut NodeTree, dx: f64, dy: f64) {
    t.node.geom.x += dx;
    t.node.geom.y += dy;
    for c in &mut t.children {
        shift_tree(c, dx, dy);
    }
}

/// 整树剥离实例标记(实例内容升格为定义时不得带实例标记)。
fn strip_markers_tree(t: &mut NodeTree) {
    for m in MARKER_ATTRS {
        t.node.attrs.remove(m);
    }
    for c in &mut t.children {
        strip_markers_tree(c);
    }
}

/// (小工具)按名 upsert 键值对。
fn upsert(list: &mut Vec<(String, String)>, k: &str, v: &str) {
    match list.iter_mut().find(|(a, _)| a == k) {
        Some(slot) => slot.1 = v.to_string(),
        None => list.push((k.to_string(), v.to_string())),
    }
}

/// (小工具)BTreeMap 版 upsert(快照根 attrs 用)。
fn upsert_attr(map: &mut std::collections::BTreeMap<String, String>, k: &str, v: &str) {
    map.insert(k.to_string(), v.to_string());
}

/// 覆盖键修剪:根级键(路径空)全保留;内层键仅当新主件结构在该路径
/// 仍有节点且字段仍适用(text 键要求目标仍是文本)。
fn prune_keys(keys: &[String], proto: &NodeTree) -> Vec<String> {
    let mut out = Vec::new();
    for k in keys {
        let Some((path, field)) = split_key(k) else {
            continue;
        };
        let keep = if path.is_empty() {
            true
        } else {
            match tree_at(proto, &path) {
                Some(t) => {
                    if field == "text" {
                        matches!(t.node.kind, NodeKind::Text { .. })
                    } else {
                        // style:/attr:/geom 键只要有节点即可
                        true
                    }
                }
                None => false,
            }
        };
        if keep {
            out.push(k.clone());
        }
    }
    out
}

// ─────────────────────────── 实例同步构建(05-8-3 核心) ───────────────────────────

/// 为一个实例构建「替换子树 + 根字段同步」命令组。
///
/// - `proto`:主件原型快照(根 attrs 上带 `ATTR_SYMBOL`/`ATTR_SYMBOL_REF`
///   作为"实例标记基准",由调用方写入);
/// - `protect = true`:按实例根的覆盖键保护;`false`:全量还原
///   (reset_overrides 用,同时清空覆盖列表)。
fn build_instance_sync(
    doc: &mut Document,
    inst_id: NodeId,
    proto: &NodeTree,
    protect: bool,
) -> Result<Vec<Command>, VbError> {
    // 先取齐实例根的静态信息,随后 clone_* 需要 &mut doc
    let (inst_sid, inst_geom, inst_style, inst_attrs, old_keys) = {
        let n = doc
            .nodes
            .get(inst_id)
            .ok_or_else(|| VbError::NoSuchNode("(实例根)".into()))?;
        (
            n.sid.as_str().to_string(),
            n.geom,
            n.style.clone(),
            n.attrs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>(),
            if protect {
                override_keys(&n.attrs)
            } else {
                Vec::new()
            },
        )
    };
    let old_tree = NodeTree::from_document(doc, inst_id)
        .ok_or_else(|| VbError::NoSuchNode(inst_sid.clone()))?;
    let prot = extract_protections(&old_tree, &old_keys);

    // 平移量:实例根几何 − 主件根几何(扁平模型:子节点几何相对画板)
    let dx = inst_geom.x - proto.node.geom.x;
    let dy = inst_geom.y - proto.node.geom.y;

    let mut cmds: Vec<Command> = Vec::new();

    // ── 子节点替换 ──
    let mut new_children: Vec<NodeTree> = Vec::with_capacity(proto.children.len());
    for (i, c) in proto.children.iter().enumerate() {
        let mut t = clone_snapshot_fresh(doc, c);
        shift_tree(&mut t, dx, dy);
        let mut path = vec![i];
        patch_tree(&mut t, &mut path, &prot);
        new_children.push(t);
    }
    let old_child_sids: Vec<String> = old_tree
        .children
        .iter()
        .map(|c| c.node.sid.as_str().to_string())
        .collect();
    match (old_child_sids.is_empty(), new_children.is_empty()) {
        (false, false) => cmds.push(Command::MultiResult {
            op: "主件同步".into(),
            src_sids: old_child_sids,
            results: new_children,
            slot: MultiResultSlot::ReplaceAnchor,
            captured: None,
        }),
        (true, false) => {
            // 实例根原为叶子:逐个追加新子节点(保序)
            for t in new_children {
                cmds.push(Command::Insert {
                    parent_sid: inst_sid.clone(),
                    index: usize::MAX,
                    tree: t,
                });
            }
        }
        (false, true) => {
            // 主件已清空子节点:逐个删除实例子节点
            for s in old_child_sids {
                cmds.push(Command::Delete {
                    target_sid: s,
                    captured: None,
                });
            }
        }
        (true, true) => {}
    }

    // ── 根字段同步 ──
    let root_prot = prot.get(&[][..]).cloned().unwrap_or_default();

    // 根文本(两侧都是文本才同步;"text" 保护时整字段跳过)
    let text_protected = old_keys
        .iter()
        .any(|k| split_key(k) == Some((Vec::new(), "text")));
    if !text_protected {
        if let NodeKind::Text {
            text: pt,
            segments: ps,
            ..
        } = &proto.node.kind
        {
            if let NodeKind::Text {
                text: it,
                segments: is,
                ..
            } = &doc.nodes.get(inst_id).unwrap().kind
            {
                if pt != it || ps != is {
                    cmds.push(Command::SetText {
                        sid: inst_sid.clone(),
                        new: pt.clone(),
                        old: None,
                    });
                    if !ps.is_empty() {
                        // 主件有段注记则一并带上(SetText 清段后补)
                        cmds.push(Command::SetSegs {
                            sid: inst_sid.clone(),
                            new: ps.clone(),
                            old: None,
                        });
                    }
                }
            }
        }
    }

    // 根样式:主件根样式 + 受保护声明取实例值
    let mut new_style = proto.node.style.clone();
    for (prop, v) in &root_prot.style {
        match v {
            Some(val) => {
                if let Some(d) = new_style.iter_mut().find(|d| d.prop == *prop) {
                    d.value = val.clone();
                } else {
                    new_style.push(decl(prop, val));
                }
            }
            None => new_style.retain(|d| d.prop != *prop),
        }
    }
    if new_style != inst_style {
        cmds.push(Command::SetStyle {
            sid: inst_sid.clone(),
            new: new_style,
            old: None,
        });
    }

    // 根属性:主件根属性(剥离标记,防御)+ 实例标记 + 受保护属性 + 覆盖列表
    let kept_keys = if protect {
        prune_keys(&old_keys, proto)
    } else {
        Vec::new()
    };
    let mut new_attrs: Vec<(String, String)> = proto
        .node
        .attrs
        .iter()
        .filter(|(k, _)| !MARKER_ATTRS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (name, v) in &root_prot.attrs {
        if MARKER_ATTRS.contains(&name.as_str()) {
            continue;
        }
        match v {
            Some(val) => upsert(&mut new_attrs, name, val),
            None => new_attrs.retain(|(k, _)| k != name),
        }
    }
    let name = proto
        .node
        .attrs
        .get(ATTR_SYMBOL)
        .cloned()
        .unwrap_or_default();
    let ref_sid = proto
        .node
        .attrs
        .get(ATTR_SYMBOL_REF)
        .cloned()
        .unwrap_or_default();
    upsert(&mut new_attrs, ATTR_SYMBOL, &name);
    upsert(&mut new_attrs, ATTR_SYMBOL_REF, &ref_sid);
    if kept_keys.is_empty() {
        new_attrs.retain(|(k, _)| k != ATTR_OVERRIDES);
    } else {
        upsert(&mut new_attrs, ATTR_OVERRIDES, &serialize_keys(&kept_keys));
    }
    // SetAttrs 落地时收进 BTreeMap(按键排序);比较前先排序,避免仅因
    // 构造顺序不同而误判"有差异"
    new_attrs.sort();
    if new_attrs != inst_attrs {
        cmds.push(Command::SetAttrs {
            sid: inst_sid,
            new: new_attrs,
            old: None,
        });
    }
    Ok(cmds)
}

/// 主件原型快照(容器当前内容 + 实例标记基准)。
fn proto_snapshot(doc: &Document, container: NodeId) -> Result<(NodeTree, NodeId), VbError> {
    let container_node = doc
        .nodes
        .get(container)
        .ok_or_else(|| VbError::NoSuchNode("(主件容器)".into()))?;
    let proto_root = container_node
        .children
        .first()
        .copied()
        .ok_or_else(|| VbError::Conflict("主件定义没有原型内容".into()))?;
    let name = container_node.name.clone();
    let container_sid = container_node.sid.as_str().to_string();
    let mut proto = NodeTree::from_document(doc, proto_root)
        .ok_or_else(|| VbError::NoSuchNode(container_sid.clone()))?;
    upsert_attr(&mut proto.node.attrs, ATTR_SYMBOL, &name);
    upsert_attr(&mut proto.node.attrs, ATTR_SYMBOL_REF, &container_sid);
    Ok((proto, container))
}

/// 按当前文档内容为某主件容器构建全部实例的同步命令(懒构建入口,
/// `SymbolSync` 首次 apply 调用;此时主件编辑**已经落地**)。
pub fn build_sync_commands(doc: &mut Document, container: NodeId) -> Result<Command, VbError> {
    if doc
        .nodes
        .get(container)
        .ok_or_else(|| VbError::NoSuchNode("(主件容器)".into()))?
        .children
        .is_empty()
    {
        return Ok(Command::Compound { cmds: Vec::new() });
    }
    let (proto, _) = proto_snapshot(doc, container)?;
    let mut cmds: Vec<Command> = Vec::new();
    for inst in instances_of_container(doc, container) {
        cmds.extend(build_instance_sync(doc, inst, &proto, true)?);
    }
    Ok(Command::Compound { cmds })
}

// ─────────────────────────── 命令包装收口(UndoStack::push 调用) ───────────────────────────

/// 单条命令涉及的覆盖登记:(实例根 sid, 新增字段键)。
fn collect_override_records(doc: &Document, cmd: &Command, out: &mut Vec<(String, Vec<String>)>) {
    match cmd {
        Command::SetText { sid, .. } | Command::SetSegs { sid, .. } => {
            record_override(doc, sid, vec![field_key(&path_of(doc, sid), "text")], out);
        }
        Command::SetStyle { sid, new, .. } => {
            let Some(id) = doc.find_by_sid(sid) else {
                return;
            };
            let Some(n) = doc.nodes.get(id) else {
                return;
            };
            let mut keys = Vec::new();
            for d in new.iter() {
                if n.style_get(&d.prop).map(|v| v == d.value).unwrap_or(false) {
                    continue;
                }
                keys.push(field_key(&path_of(doc, sid), &format!("style:{}", d.prop)));
            }
            for d in n.style.iter() {
                if !new.iter().any(|nd| nd.prop == d.prop) {
                    keys.push(field_key(&path_of(doc, sid), &format!("style:{}", d.prop)));
                }
            }
            keys.sort();
            keys.dedup();
            record_override(doc, sid, keys, out);
        }
        Command::SetAttrs { sid, new, .. } => {
            let Some(id) = doc.find_by_sid(sid) else {
                return;
            };
            let Some(n) = doc.nodes.get(id) else {
                return;
            };
            let mut keys = Vec::new();
            for (k, v) in new.iter() {
                if MARKER_ATTRS.contains(&k.as_str()) {
                    continue;
                }
                if n.attrs.get(k).map(|old| old == v).unwrap_or(false) {
                    continue;
                }
                keys.push(field_key(&path_of(doc, sid), &format!("attr:{k}")));
            }
            for k in n.attrs.keys() {
                if MARKER_ATTRS.contains(&k.as_str()) {
                    continue;
                }
                if !new.iter().any(|(nk, _)| nk == k) {
                    keys.push(field_key(&path_of(doc, sid), &format!("attr:{k}")));
                }
            }
            keys.sort();
            keys.dedup();
            record_override(doc, sid, keys, out);
        }
        Command::SetGeom { sid, .. } => {
            // 实例根自身的几何属实例所有(移动/缩放实例天然安全),不登记;
            // 实例**内部**节点的几何编辑登记为 "geom" 字段。
            let Some(id) = doc.find_by_sid(sid) else {
                return;
            };
            let Some(root) = instance_root_of(doc, id) else {
                return;
            };
            if root != id {
                record_override(doc, sid, vec![field_key(&path_of(doc, sid), "geom")], out);
            }
        }
        Command::Compound { cmds } => {
            for c in cmds {
                collect_override_records(doc, c, out);
            }
        }
        _ => {}
    }
}

/// 把一条覆盖登记并入结果表(按实例根聚合;非实例子树忽略)。
fn record_override(
    doc: &Document,
    sid: &str,
    keys: Vec<String>,
    out: &mut Vec<(String, Vec<String>)>,
) {
    if keys.is_empty() {
        return;
    }
    let Some(id) = doc.find_by_sid(sid) else {
        return;
    };
    let Some(root) = instance_root_of(doc, id) else {
        return;
    };
    let root_sid = doc.nodes.get(root).unwrap().sid.as_str().to_string();
    match out.iter_mut().find(|(s, _)| *s == root_sid) {
        Some((_, ks)) => ks.extend(keys),
        None => out.push((root_sid, keys)),
    }
}

/// 节点相对实例根的子节点下标路径(目标自身是实例根 = 空路径)。
fn path_of(doc: &Document, sid: &str) -> Vec<usize> {
    let Some(mut id) = doc.find_by_sid(sid) else {
        return Vec::new();
    };
    let Some(root) = instance_root_of(doc, id) else {
        return Vec::new();
    };
    let mut rev = Vec::new();
    while id != root {
        let Some(n) = doc.nodes.get(id) else {
            break;
        };
        let Some(p) = n.parent else {
            break;
        };
        let idx = doc
            .nodes
            .get(p)
            .and_then(|p| p.children.iter().position(|&c| c == id))
            .unwrap_or(0);
        rev.push(idx);
        id = p;
    }
    rev.reverse();
    rev
}

/// 把 sid 对应节点所属主件容器并入命中表。
fn hit_container(doc: &Document, id: NodeId, out: &mut Vec<String>) {
    if let Some(c) = def_container_of(doc, id) {
        let cs = doc.nodes.get(c).unwrap().sid.as_str().to_string();
        if !out.contains(&cs) {
            out.push(cs);
        }
    }
}

/// 单条命令涉及的主件容器 sid(命令目标当前落在 defs 子树内 → 该命令
/// 是主件编辑,apply 后需要同步实例)。判定用**推送时**的文档状态:
/// 包装先于应用,故 symbol_create 内部的 Move/Insert 不会误触发。
fn collect_def_hits(doc: &Document, cmd: &Command, out: &mut Vec<String>) {
    match cmd {
        Command::SetText { sid, .. }
        | Command::SetSegs { sid, .. }
        | Command::SetStyle { sid, .. }
        | Command::SetAttrs { sid, .. }
        | Command::SetGeom { sid, .. }
        | Command::Rename { sid, .. }
        | Command::SetTag { sid, .. }
        | Command::SetVector { sid, .. }
        | Command::SetImageSrc { sid, .. }
        | Command::Delete {
            target_sid: sid, ..
        } => {
            if let Some(id) = doc.find_by_sid(sid) {
                hit_container(doc, id, out);
            }
        }
        Command::Insert { parent_sid, .. } => {
            // 插到 defs 子树内部(容器或原型节点之下)→ 新内容属主件;
            // 插到 defs_root 直下是新建容器(尚无实例),不触发。
            if let Some(pid) = doc.find_by_sid(parent_sid) {
                if let Some(c) = def_container_of(doc, pid) {
                    let cs = doc.nodes.get(c).unwrap().sid.as_str().to_string();
                    if !out.contains(&cs) {
                        out.push(cs);
                    }
                }
            }
        }
        Command::Move {
            sid,
            new_parent_sid,
            ..
        } => {
            if let Some(id) = doc.find_by_sid(sid) {
                hit_container(doc, id, out);
            }
            if let Some(pid) = doc.find_by_sid(new_parent_sid) {
                hit_container(doc, pid, out);
            }
        }
        Command::Compound { cmds } => {
            for c in cmds {
                collect_def_hits(doc, c, out);
            }
        }
        _ => {}
    }
}

/// **符号语义收口**(`UndoStack::push` 唯一调用点):把一条普通命令折算
/// 为带符号语义的命令 ——
/// ① 目标落在某实例子树内且属于可登记种类 → 追加实例根的覆盖登记
///    SetAttrs(与原命令同一条 undo);
/// ② 目标落在主件定义区 → 追加 [`Command::SymbolSync`](懒构建,首次
///    apply 时按编辑后的主件内容同步全部实例)。
///
/// 符号命令自身(MultiResult / SymbolSync / 覆盖 SetAttrs)与无符号关联
/// 的命令原样返回。主件编辑包装**不参与 undo 合并**(见 merge_target:
/// 合并会让缓存的内层事务在重做时用过期快照,重做结果漂移)。
pub fn wrap_symbol_effects(doc: &Document, cmd: Command) -> Command {
    // 快速路径:文档没有任何主件与实例 → 原样返回
    let has_defs = doc
        .nodes
        .get(doc.defs_root)
        .map(|r| !r.children.is_empty())
        .unwrap_or(false);
    let has_instances = doc
        .nodes
        .iter()
        .any(|(_, n)| n.attrs.contains_key(ATTR_SYMBOL));
    if !has_defs && !has_instances {
        return cmd;
    }
    let mut recs: Vec<(String, Vec<String>)> = Vec::new();
    collect_override_records(doc, &cmd, &mut recs);
    let mut defs: Vec<String> = Vec::new();
    collect_def_hits(doc, &cmd, &mut defs);
    if recs.is_empty() && defs.is_empty() {
        return cmd;
    }
    let mut cmds = vec![cmd];
    for (root_sid, mut keys) in recs {
        keys.retain(|k| k.chars().all(key_char_ok));
        if keys.is_empty() {
            continue;
        }
        let Some(root_id) = doc.find_by_sid(&root_sid) else {
            continue;
        };
        let n = doc.nodes.get(root_id).unwrap();
        let mut merged = override_keys(&n.attrs);
        for k in &keys {
            if !merged.contains(k) {
                merged.push(k.clone());
            }
        }
        let mut new_attrs: Vec<(String, String)> = n
            .attrs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        upsert(&mut new_attrs, ATTR_OVERRIDES, &serialize_keys(&merged));
        cmds.push(Command::SetAttrs {
            sid: root_sid,
            new: new_attrs,
            old: None,
        });
    }
    for c in defs {
        cmds.push(Command::SymbolSync {
            container_sid: c,
            inner: None,
        });
    }
    Command::Compound { cmds }
}

// ─────────────────────────── 五个符号命令的构建(05-8-4) ───────────────────────────

/// `object.symbol_create`:选中元素提升为主件 + 当前位置变为首个实例。
///
/// 折算为一条 Compound:[插入定义容器(挂 `defs_root`)→ 移动原元素入
/// 容器 → 在原槽位插入实例]。实例 = 原型的全新 sid 副本 + 根标记(无
/// 覆盖);主件保留原元素 sid(身份延续)。返回 (命令, 新实例根 sid)。
pub fn symbol_create_commands(
    doc: &mut Document,
    elem_sid: &str,
    name: &str,
) -> Result<(Command, String), VbError> {
    let elem_id = doc
        .find_by_sid(elem_sid)
        .ok_or_else(|| VbError::NoSuchNode(elem_sid.to_string()))?;
    {
        let n = doc.nodes.get(elem_id).unwrap();
        if matches!(n.kind, NodeKind::Artboard) {
            return Err(VbError::Conflict("画板不能创建为组件".into()));
        }
        if is_in_defs(doc, elem_id) {
            return Err(VbError::Conflict("该对象已是主件定义的一部分".into()));
        }
        if is_instance_root(n) {
            return Err(VbError::Conflict(
                "该对象已是组件实例(改定义请编辑主件,或用「替换主件定义」)".into(),
            ));
        }
    }
    let name = next_symbol_name(doc, name);
    // 原槽位(移动前捕获)
    let (orig_parent_sid, orig_index) = {
        let n = doc.nodes.get(elem_id).unwrap();
        let p = n.parent.expect("symbol_create: 元素必有父级");
        let idx = doc
            .nodes
            .get(p)
            .unwrap()
            .children
            .iter()
            .position(|&c| c == elem_id)
            .unwrap_or(0);
        (doc.nodes.get(p).unwrap().sid.as_str().to_string(), idx)
    };
    // 定义容器(全新 sid;无类,导出时补标记类;hidden 为原生属性)
    let container_sid = doc.alloc_sid();
    let mut container = Node::new(NodeKind::Box, name.clone(), container_sid.clone());
    container.tag = "div".into();
    container.attrs.insert("hidden".into(), String::new());
    // 定义区容器不参与布局;geom_declared = true 让导出侧不出几何规则
    container.geom_declared = true;
    let container_tree = NodeTree {
        node: container,
        children: Vec::new(),
    };
    // 实例 = 原型的全新 sid 副本(先克隆后移动,克隆的是移动前内容)
    let mut inst_tree = clone_tree_fresh(doc, elem_id);
    upsert_attr(&mut inst_tree.node.attrs, ATTR_SYMBOL, &name);
    upsert_attr(
        &mut inst_tree.node.attrs,
        ATTR_SYMBOL_REF,
        container_sid.as_str(),
    );
    let inst_root_sid = inst_tree.node.sid.as_str().to_string();
    let defs_root_sid = doc
        .nodes
        .get(doc.defs_root)
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let compound = Command::Compound {
        cmds: vec![
            Command::Insert {
                parent_sid: defs_root_sid,
                index: usize::MAX,
                tree: container_tree,
            },
            Command::Move {
                sid: elem_sid.to_string(),
                new_parent_sid: container_sid.as_str().to_string(),
                new_index: 0,
                old: None,
            },
            Command::Insert {
                parent_sid: orig_parent_sid,
                index: orig_index,
                tree: inst_tree,
            },
        ],
    };
    Ok((compound, inst_root_sid))
}

/// `object.symbol_detach`:实例转普通元素(去三个标记属性;内容原样保留)。
pub fn symbol_detach_commands(doc: &Document, inst_sid: &str) -> Result<Command, VbError> {
    let id = doc
        .find_by_sid(inst_sid)
        .ok_or_else(|| VbError::NoSuchNode(inst_sid.to_string()))?;
    let n = doc.nodes.get(id).unwrap();
    if !is_instance_root(n) {
        return Err(VbError::Conflict("该对象不是组件实例".into()));
    }
    let new_attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .filter(|(k, _)| !MARKER_ATTRS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(Command::SetAttrs {
        sid: inst_sid.to_string(),
        new: new_attrs,
        old: None,
    })
}

/// `object.symbol_reset_overrides`:实例还原为主件当前内容(清覆盖列表)。
pub fn symbol_reset_overrides_commands(
    doc: &mut Document,
    inst_sid: &str,
) -> Result<Command, VbError> {
    let inst_id = doc
        .find_by_sid(inst_sid)
        .ok_or_else(|| VbError::NoSuchNode(inst_sid.to_string()))?;
    if !doc
        .nodes
        .get(inst_id)
        .map(is_instance_root)
        .unwrap_or(false)
    {
        return Err(VbError::Conflict("该对象不是组件实例".into()));
    }
    let container = instance_container_of(doc, inst_id)?;
    let (proto, _) = proto_snapshot(doc, container)?;
    let cmds = build_instance_sync(doc, inst_id, &proto, false)?;
    // protect=false → 覆盖列表已随根属性同步清空
    Ok(Command::Compound { cmds })
}

/// `object.symbol_swap_main`:以某实例的当前内容替换主件定义,并同步其余
/// 全部实例(其余实例的覆盖仍受保护);该实例成为「零覆盖」实例。
pub fn symbol_swap_main_commands(doc: &mut Document, inst_sid: &str) -> Result<Command, VbError> {
    let inst_id = doc
        .find_by_sid(inst_sid)
        .ok_or_else(|| VbError::NoSuchNode(inst_sid.to_string()))?;
    let container = {
        let n = doc.nodes.get(inst_id).unwrap();
        if !is_instance_root(n) {
            return Err(VbError::Conflict(
                "替换主件定义:请先选中一个组件实例(其当前内容将成为新定义)".into(),
            ));
        }
        instance_container_of(doc, inst_id)?
    };
    let old_proto_sid = doc
        .nodes
        .get(container)
        .and_then(|c| {
            c.children
                .first()
                .and_then(|id| doc.nodes.get(*id))
                .map(|n| n.sid.as_str().to_string())
        })
        .ok_or_else(|| VbError::Conflict("主件定义没有原型内容".into()))?;
    let container_sid = doc.nodes.get(container).unwrap().sid.as_str().to_string();
    let name = doc.nodes.get(container).unwrap().name.clone();

    let mut cmds: Vec<Command> = Vec::new();
    // 新原型 = 该实例的当前内容(全新 sid,剥离实例标记),原位替换旧原型
    let mut new_proto = clone_tree_fresh(doc, inst_id);
    strip_markers_tree(&mut new_proto);
    cmds.push(Command::MultiResult {
        op: "替换主件定义".into(),
        src_sids: vec![old_proto_sid],
        results: vec![new_proto],
        slot: MultiResultSlot::ReplaceAnchor,
        captured: None,
    });
    // 其余实例同步到新定义(保护各自覆盖)。新原型内容 = 该实例的克隆,
    // 从构建期快照逐实例克隆(命令应用前容器里还是旧原型)。
    let others: Vec<NodeId> = instances_of_container(doc, container)
        .into_iter()
        .filter(|&i| i != inst_id)
        .collect();
    for other in others {
        let mut proto = clone_tree_fresh(doc, inst_id);
        strip_markers_tree(&mut proto);
        upsert_attr(&mut proto.node.attrs, ATTR_SYMBOL, &name);
        upsert_attr(&mut proto.node.attrs, ATTR_SYMBOL_REF, &container_sid);
        cmds.extend(build_instance_sync(doc, other, &proto, true)?);
    }
    // 该实例成为零覆盖实例(内容即定义;标记保留)
    let new_attrs: Vec<(String, String)> = doc
        .nodes
        .get(inst_id)
        .unwrap()
        .attrs
        .iter()
        .filter(|(k, _)| *k != ATTR_OVERRIDES)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    cmds.push(Command::SetAttrs {
        sid: inst_sid.to_string(),
        new: new_attrs,
        old: None,
    });
    Ok(Command::Compound { cmds })
}

/// 实例根 → 其主件定义容器(校验 ref 可解析)。
fn instance_container_of(doc: &Document, inst_id: NodeId) -> Result<NodeId, VbError> {
    let n = doc.nodes.get(inst_id).unwrap();
    let ref_sid = n.attrs.get(ATTR_SYMBOL_REF).cloned().unwrap_or_default();
    doc.find_by_sid(&ref_sid)
        .ok_or_else(|| VbError::Conflict(format!("主件定义不存在(ref={ref_sid})")))
}

/// `object.symbol_select_instances` 判据(纯函数,应用层写选区):
/// 种子可以是实例根 / 实例内部节点 / 主件定义区节点,返回同主件全部
/// 实例根 sid。
pub fn select_instances_of(doc: &Document, seed_sid: &str) -> Vec<String> {
    let Some(seed) = doc.find_by_sid(seed_sid) else {
        return Vec::new();
    };
    // 种子在定义区 → 该主件的全部实例
    let container = if let Some(c) = def_container_of(doc, seed) {
        Some(c)
    } else if let Some(root) = instance_root_of(doc, seed) {
        // 种子在实例内 → 同主件全部实例
        let ref_sid = doc.nodes.get(root).unwrap().attrs[ATTR_SYMBOL_REF].clone();
        doc.find_by_sid(&ref_sid)
    } else {
        None
    };
    match container {
        Some(c) => instances_of_container(doc, c)
            .into_iter()
            .map(|i| doc.nodes.get(i).unwrap().sid.as_str().to_string())
            .collect(),
        None => Vec::new(),
    }
}
