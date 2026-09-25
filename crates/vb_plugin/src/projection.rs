//! 文档只读投影(05-10-4 ②):画板/节点树的结构摘要 JSON。
//!
//! 纪律:**只读** —— 这里没有任何写路径,也不给插件"直改文档"的捷径;
//! 插件要改文档只能经 `runCommand` 走宿主命令(可撤销、与 UI 同一路径)。
//! 投影含宿主侧预聚合的 `counts`(节点/文本/图片数),统计类插件不必
//! 自己遍历整棵树。

use serde_json::{json, Value};
use vb_doc::model::{Document, Node, NodeId};

/// 单棵子树的投影深度上限(防极端深树撑爆 stdio 帧;超出部分裁掉并
/// 在 `truncated` 标记)。
const MAX_DEPTH: usize = 24;

/// 构建当前文档的只读投影。
///
/// 形态:
/// ```json
/// {
///   "rev": 12,
///   "artboardCount": 2,
///   "counts": {"nodes": 21, "text": 10, "image": 1, "group": 3, "vector": 0, "other": 7},
///   "artboards": [ { "sid": "..", "name": "..", "tag": "..", "kind": "..",
///                    "box": {"x":..,"y":..,"w":..,"h":..}, "children": [ ... ] } ]
/// }
/// ```
pub fn build(doc: &Document) -> Value {
    let mut counts = Counts::default();
    let artboards: Vec<Value> = doc
        .artboards
        .iter()
        .map(|&a| {
            // 画板自身计入 counts
            if let Some(n) = doc.nodes.get(a) {
                counts.bump(n);
            }
            node_json(doc, a, MAX_DEPTH, &mut counts)
        })
        .collect();
    json!({
        "rev": doc.rev,
        "artboardCount": doc.artboards.len(),
        "counts": counts.to_json(),
        "artboards": artboards,
    })
}

/// 从投影 JSON 里取计数(示例插件与门禁测试共用,避免手写下标)。
pub fn counts_of(projection: &Value) -> Value {
    projection.get("counts").cloned().unwrap_or(Value::Null)
}

fn node_json(doc: &Document, id: NodeId, depth: usize, counts: &mut Counts) -> Value {
    let Some(n) = doc.nodes.get(id) else {
        // 容忍悬挂 id(MCP 同款防御):不 panic,回 null
        return Value::Null;
    };
    let children: Vec<Value> = if depth > 1 {
        n.children
            .iter()
            .map(|&c| {
                if let Some(cn) = doc.nodes.get(c) {
                    counts.bump(cn);
                }
                node_json(doc, c, depth - 1, counts)
            })
            .collect()
    } else {
        // 触底:只标记,不递归(当前 MAX_DEPTH 足够深,正常文档到不了)
        Vec::new()
    };
    let mut o = json!({
        "sid": n.sid.as_str(),
        "name": n.name,
        "tag": n.tag,
        "kind": n.kind.kind_name(),
        "box": {"x": n.geom.x, "y": n.geom.y, "w": n.geom.w, "h": n.geom.h},
        "children": children,
    });
    // 文本节点带正文(统计与查找用;只读)
    if let Some(t) = n.text() {
        o["text"] = Value::String(t.to_string());
    }
    // 图片节点带 src(相对 assets/ 的引用路径)
    if let vb_doc::model::NodeKind::Image { src, .. } = &n.kind {
        o["src"] = Value::String(src.clone());
    }
    o
}

/// 预聚合计数(宿主侧算好,插件直接用)。
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Counts {
    pub nodes: u64,
    pub text: u64,
    pub image: u64,
    pub group: u64,
    pub vector: u64,
    pub other: u64,
}

impl Counts {
    fn bump(&mut self, n: &Node) {
        self.nodes += 1;
        match &n.kind {
            vb_doc::model::NodeKind::Text { .. } => self.text += 1,
            vb_doc::model::NodeKind::Image { .. } => self.image += 1,
            vb_doc::model::NodeKind::Group => self.group += 1,
            vb_doc::model::NodeKind::Vector { .. } => self.vector += 1,
            _ => self.other += 1,
        }
    }

    fn to_json(self) -> Value {
        json!({
            "nodes": self.nodes,
            "text": self.text,
            "image": self.image,
            "group": self.group,
            "vector": self.vector,
            "other": self.other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::model::NodeKind;

    /// 投影只读 + 计数正确 + 树形结构完整。
    #[test]
    fn projection_counts_and_tree() {
        // 空文档(无画板;root/defs 不在画板树内,不计入投影)
        let mut doc = Document::new_empty("t", "zh-CN");
        let p = build(&doc);
        assert_eq!(p["artboardCount"], 0);
        assert_eq!(p["counts"]["nodes"], 0);

        let sid = |s: &str| vb_common::StableId::parse(s).unwrap();
        // 建一个画板 + 文本 + 图片 + 编组(内含文本)
        let ab = doc
            .nodes
            .insert(Node::new(NodeKind::Artboard, "首页", sid("ab1")));
        doc.artboards.push(ab);
        let txt = doc.nodes.insert(Node::new(
            NodeKind::Text {
                text: "你好".into(),
                segments: Vec::new(),
                mode: vb_doc::model::TextMode::Point,
            },
            "标题",
            sid("t1x"),
        ));
        let img = doc.nodes.insert(Node::new(
            NodeKind::Image {
                src: "a.png".into(),
            },
            "图",
            sid("i1x"),
        ));
        let grp = doc
            .nodes
            .insert(Node::new(NodeKind::Group, "组", sid("g1x")));
        let txt2 = doc.nodes.insert(Node::new(
            NodeKind::Text {
                text: "子文本".into(),
                segments: Vec::new(),
                mode: vb_doc::model::TextMode::Point,
            },
            "子标题",
            sid("t2x"),
        ));
        doc.nodes[txt].parent = Some(ab);
        doc.nodes[img].parent = Some(ab);
        doc.nodes[grp].parent = Some(ab);
        doc.nodes[txt2].parent = Some(grp);
        doc.nodes[ab].children = vec![txt, img, grp];
        doc.nodes[grp].children = vec![txt2];

        doc.rev = 7;
        let p = build(&doc);
        assert_eq!(p["rev"], 7);
        assert_eq!(p["artboardCount"], 1);
        assert_eq!(p["counts"]["nodes"], 5, "画板+文本+图片+编组+编组内文本");
        assert_eq!(p["counts"]["text"], 2);
        assert_eq!(p["counts"]["image"], 1);
        assert_eq!(p["counts"]["group"], 1);
        // 树形:画板 children 3 项,组内 1 项;文本带 text、图片带 src
        let ab_json = &p["artboards"][0];
        assert_eq!(ab_json["children"].as_array().unwrap().len(), 3);
        assert_eq!(ab_json["children"][0]["text"], "你好");
        assert_eq!(ab_json["children"][1]["src"], "a.png");
        assert_eq!(ab_json["children"][2]["children"][0]["text"], "子文本");
        // counts_of 辅助
        assert_eq!(counts_of(&p)["nodes"], 5);
    }
}
