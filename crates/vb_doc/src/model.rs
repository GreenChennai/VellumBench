//! 场景图模型:Arena(slotmap)+ 稳定短码 sid(设计文档 04 篇 §一)。
//!
//! 关键不变量:
//! - `sid` 落盘为 `data-vb-id`,全生命周期不变;`NodeId` 是内存索引,Undo/重排后可能变化。
//! - 元素几何(x/y/w/h)相对**所属画板**左上角(Y 向下,ADR-0007)。
//! - `style` 是声明列表;白名单外声明(unknown)同样保存在内,导出时原样回写。

use std::collections::BTreeMap;

use slotmap::{new_key_type, SlotMap};
use vb_common::{Rgba, StableId};
use vb_css::Decl;

new_key_type! {
    /// 内存节点键(不落盘)。
    pub struct NodeIdKey;
}
pub type NodeId = NodeIdKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// index.html + styles/main.css(默认)
    #[default]
    ExternalCss,
    /// <style> 内联
    InlineCss,
    /// 单文件(图片仍引用相对路径)
    SingleFile,
}

#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub title: String,
    pub lang: String,
    pub output: OutputMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextMode {
    /// 点文本:宽度随内容
    #[default]
    Point,
    /// 区域文本:定宽自动换行
    Area,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Artboard,
    Layer,
    Group,
    /// 盒对象(矩形/圆角矩形/椭圆容器)
    Box,
    Text {
        text: String,
        mode: TextMode,
    },
    Image {
        src: String,
    },
    /// 矢量路径(v0.1 导入为 Frozen;变体保留占位)
    Vector {
        path: kurbo::BezPath,
    },
    Slice,
    /// 冻结块:原样保留的 HTML 片段(可见、可移动/删除、内部不可编辑)
    Frozen {
        html: String,
    },
}

impl NodeKind {
    pub fn kind_name(&self) -> &'static str {
        match self {
            NodeKind::Artboard => "artboard",
            NodeKind::Layer => "layer",
            NodeKind::Group => "group",
            NodeKind::Box => "box",
            NodeKind::Text { .. } => "text",
            NodeKind::Image { .. } => "image",
            NodeKind::Vector { .. } => "vector",
            NodeKind::Slice => "slice",
            NodeKind::Frozen { .. } => "frozen",
        }
    }

    pub fn is_container(&self) -> bool {
        matches!(
            self,
            NodeKind::Artboard | NodeKind::Layer | NodeKind::Group | NodeKind::Box
        )
    }
}

/// 几何:相对所属画板的本地 px。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geom {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Default for Geom {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    /// 稳定短码 → `data-vb-id`。
    pub sid: StableId,
    pub parent: Option<NodeId>,
    /// 顺序即 z 序(后 = 上)。
    pub children: Vec<NodeId>,
    pub kind: NodeKind,
    /// AI 图层名(如「主标题」)→ `data-vb-name`。
    pub name: String,
    /// 序列化标签(div/section/h1/p/a/button/span/img …;文本子片段用 "#text")。
    pub tag: String,
    pub classes: Vec<String>,
    /// 其余 HTML 属性(href/alt/aria-*/role …;class/style/id 之外)。
    pub attrs: BTreeMap<String, String>,
    /// CSS 声明(含 unknown;导出时按 PROP_ORDER 排序)。
    pub style: Vec<Decl>,
    /// 节点前注释(导入保序,导出还原)。
    pub comment_before: Option<String>,
    pub geom: Geom,
    pub hidden: bool,
    pub locked: bool,
}

impl Node {
    pub fn new(kind: NodeKind, name: impl Into<String>, sid: StableId) -> Node {
        let tag = match &kind {
            NodeKind::Artboard => "section".to_string(),
            NodeKind::Layer => "div".to_string(),
            NodeKind::Group => "div".to_string(),
            NodeKind::Box => "div".to_string(),
            NodeKind::Text { .. } => "p".to_string(),
            NodeKind::Image { .. } => "img".to_string(),
            NodeKind::Vector { .. } => "svg".to_string(),
            NodeKind::Slice => "div".to_string(),
            NodeKind::Frozen { .. } => "#frozen".to_string(),
        };
        Node {
            sid,
            parent: None,
            children: Vec::new(),
            kind,
            name: name.into(),
            tag,
            classes: Vec::new(),
            attrs: BTreeMap::new(),
            style: Vec::new(),
            comment_before: None,
            geom: Geom::default(),
            hidden: false,
            locked: false,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match &self.kind {
            NodeKind::Text { text, .. } => Some(text),
            _ => None,
        }
    }

    /// 取 CSS 属性值(精确匹配属性名)。
    pub fn style_get(&self, prop: &str) -> Option<&str> {
        self.style
            .iter()
            .find(|d| d.prop == prop)
            .map(|d| d.value.as_str())
    }

    /// 设置/更新 CSS 属性(已存在则替换,否则追加)。
    pub fn style_set(&mut self, prop: &str, value: &str) {
        if let Some(d) = self.style.iter_mut().find(|d| d.prop == prop) {
            d.value = value.to_string();
        } else {
            self.style.push(Decl {
                prop: prop.to_string(),
                value: value.to_string(),
                important: false,
            });
        }
    }

    pub fn style_remove(&mut self, prop: &str) -> bool {
        let before = self.style.len();
        self.style.retain(|d| d.prop != prop);
        self.style.len() != before
    }

    /// 解析填充色(纯色)。
    pub fn fill_color(&self) -> Option<Rgba> {
        let v = self
            .style_get("background-color")
            .or_else(|| self.style_get("background"))?;
        vb_common::color::parse_color(v)
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    /// 修订号:每次命令应用 +1(Agent 乐观锁)。
    pub rev: u64,
    pub meta: Meta,
    pub nodes: SlotMap<NodeId, Node>,
    pub root: NodeId,
    /// 画板顺序 = 导出顺序。
    pub artboards: Vec<NodeId>,
    /// 设计令牌 → `:root` CSS 变量(不带 `--` 前缀存储)。
    pub tokens: Vec<(String, String)>,
    /// 白名单外/复杂选择器 CSS 块(verbatim 保底)。
    pub raw_css: Vec<String>,
    /// body 末尾原样透传片段(`<script>` 等)。
    pub trailing_raw: Vec<String>,
    /// head 中无法建模的原样透传片段(meta/link 等,除 charset/viewport/title 外)。
    pub head_extra: Vec<String>,
    sid_next: u64,
}

impl Default for Document {
    fn default() -> Self {
        Self::new("未命名", "zh-CN")
    }
}

impl Document {
    pub fn new(title: &str, lang: &str) -> Document {
        let mut doc = Document::new_empty(title, lang);
        doc.new_artboard("画板 1", 1440.0, 900.0);
        doc
    }

    /// 无画板空文档(导入器用;编辑器「新建」走 [`Document::new`])。
    pub fn new_empty(title: &str, lang: &str) -> Document {
        let mut nodes = SlotMap::with_key();
        let root = nodes.insert(Node::new(
            NodeKind::Layer,
            "__root__",
            StableId::from_seed(0),
        ));
        Document {
            rev: 0,
            meta: Meta {
                title: title.to_string(),
                lang: lang.to_string(),
                output: OutputMode::default(),
            },
            nodes,
            root,
            artboards: Vec::new(),
            tokens: Vec::new(),
            raw_css: Vec::new(),
            trailing_raw: Vec::new(),
            head_extra: Vec::new(),
            sid_next: 1,
        }
    }

    /// 新建默认文档:单画板 1440×900(设计文档 00 篇 §七 MVP 链路第 1 步)。
    pub fn new_default() -> Document {
        Document::new("未命名", "zh-CN")
    }

    pub fn alloc_sid(&mut self) -> StableId {
        loop {
            let id = StableId::from_seed(self.sid_next);
            self.sid_next += 1;
            if !self.sid_in_use(id.as_str()) {
                return id;
            }
        }
    }

    /** 4e3a590d52364f53/patch 65b05efa828270b95206914d77ed7801(8bed4e49522b540d)3002 */
    pub fn alloc_sid_for_dup(&mut self) -> StableId {
        self.alloc_sid()
    }

    pub fn sid_in_use(&self, sid: &str) -> bool {
        self.nodes.values().any(|n| n.sid.as_str() == sid)
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    /// 按 sid 查 NodeId。
    pub fn find_by_sid(&self, sid: &str) -> Option<NodeId> {
        self.nodes
            .iter()
            .find(|(_, n)| n.sid.as_str() == sid)
            .map(|(id, _)| id)
    }

    pub fn new_artboard(&mut self, name: &str, w: f64, h: f64) -> NodeId {
        let mut n = Node::new(NodeKind::Artboard, name, self.alloc_sid());
        n.geom = Geom {
            x: 0.0,
            y: 0.0,
            w,
            h,
        };
        let id = self.nodes.insert(n);
        self.nodes.get_mut(self.root).unwrap().children.push(id);
        self.nodes.get_mut(id).unwrap().parent = Some(self.root);
        self.artboards.push(id);
        id
    }

    /// 从 `root.children` 重建 `artboards`(导出顺序 = root 子序)。
    ///
    /// 命令层(Insert/Delete/Move/Group/Ungroup)只改 arena 与 children,
    /// 不感知画板注册表;每次结构命令落地后调用本函数保持派生状态一致。
    pub fn sync_artboards(&mut self) {
        self.artboards = self
            .nodes
            .get(self.root)
            .map(|root| {
                root.children
                    .iter()
                    .copied()
                    .filter(|&c| {
                        self.nodes
                            .get(c)
                            .is_some_and(|n| matches!(n.kind, NodeKind::Artboard))
                    })
                    .collect::<Vec<NodeId>>()
            })
            .unwrap_or_default();
    }

    /// `target` 是否为 `ancestor` 自身或其后代(移动/编组的环防护)。
    pub fn is_descendant_or_self(&self, ancestor: NodeId, target: NodeId) -> bool {
        if ancestor == target {
            return true;
        }
        let mut stack = vec![ancestor];
        while let Some(id) = stack.pop() {
            if id == target {
                return true;
            }
            if let Some(n) = self.nodes.get(id) {
                stack.extend_from_slice(&n.children);
            }
        }
        false
    }

    /// 画板内所有节点的世界坐标 bbox(画板偏移 + 本地坐标;扁平模型,v0.1 无旋转累积)。
    pub fn artboard_origin(&self, artboard: NodeId) -> (f64, f64) {
        self.nodes
            .get(artboard)
            .map(|n| (n.geom.x, n.geom.y))
            .unwrap_or((0.0, 0.0))
    }

    /// 节点相对其画板原点的 bbox。
    pub fn local_bbox(&self, id: NodeId) -> kurbo::Rect {
        let n = self.nodes.get(id).expect("node");
        kurbo::Rect::new(n.geom.x, n.geom.y, n.geom.x + n.geom.w, n.geom.y + n.geom.h)
    }

    /// 深度优先收集子树(含自身)。
    pub fn subtree(&self, id: NodeId, out: &mut Vec<NodeId>) {
        out.push(id);
        if let Some(n) = self.nodes.get(id) {
            for &c in &n.children {
                self.subtree(c, out);
            }
        }
    }

    /// 深拷贝子树为新节点(保留 sid 之外的属性;sid 重新分配,sid 是唯一身份——
    /// 复制体是新元素,按 ADR-0010 语义必须拿到自己的稳定 id)。
    pub fn clone_subtree(&mut self, id: NodeId, new_parent: NodeId) -> NodeId {
        let src = self.nodes.get(id).expect("src").clone();
        self.insert_cloned_rec(&src, new_parent)
    }

    fn insert_cloned_rec(&mut self, src: &Node, parent: NodeId) -> NodeId {
        let mut n = src.clone();
        n.sid = self.alloc_sid();
        n.parent = Some(parent);
        n.children = Vec::new();
        let id = self.nodes.insert(n);
        self.nodes.get_mut(parent).unwrap().children.push(id);
        let children = src.children.clone();
        for c in children {
            if let Some(cs) = self.nodes.get(c).cloned() {
                self.insert_cloned_rec(&cs, id);
            }
        }
        id
    }

    /// 从父节点摘除子树(不删除节点本身)。
    pub fn detach(&mut self, id: NodeId) -> Option<usize> {
        let parent = self.nodes.get(id)?.parent?;
        let siblings = &mut self.nodes.get_mut(parent).unwrap().children;
        let pos = siblings.iter().position(|&c| c == id)?;
        siblings.remove(pos);
        Some(pos)
    }

    /// 按 sid 从文档中**完整取出**子树:从父级摘除 + 从 arena 删除(含全部后代)。
    /// 返回 (原父级中的位置, 子树快照)。
    pub fn extract_subtree(&mut self, sid: &str) -> Option<(usize, NodeTree)> {
        let id = self.find_by_sid(sid)?;
        let _parent = self.nodes.get(id)?.parent?;
        let index = self.detach(id)?;
        let mut sids = Vec::new();
        self.subtree(id, &mut sids);
        let tree = NodeTree::from_document(self, id)?;
        for nid in sids {
            self.nodes.remove(nid);
        }
        Some((index, tree))
    }

    /// 把树插回指定父级(sid 寻址版,给命令 revert 用)。
    pub fn insert_tree_at(
        &mut self,
        tree: &NodeTree,
        parent_sid: &str,
        index: usize,
    ) -> Option<NodeId> {
        let parent = self.find_by_sid(parent_sid)?;
        let mut created = Vec::new();
        Some(tree.insert_into(self, parent, index, &mut created))
    }
}

/// 便于测试/工具的节点访问别名。
pub type NodeSlot = (NodeId, Node);

/// 独立的子树快照(命令捕获/重插的基本单位)。
/// 节点连同其 `sid` 一起保存;重新插入时保持原 sid(身份不变)。
#[derive(Debug, Clone)]
pub struct NodeTree {
    pub node: Node,
    pub children: Vec<NodeTree>,
}

impl NodeTree {
    pub fn from_document(doc: &Document, id: NodeId) -> Option<NodeTree> {
        let node = doc.nodes.get(id)?.clone();
        let children = node
            .children
            .iter()
            .filter_map(|&c| {
                // 只跟随自己名下的孩子(parent 指回自己)
                doc.nodes
                    .get(c)
                    .filter(|n| n.parent == Some(id))
                    .and_then(|_| NodeTree::from_document(doc, c))
            })
            .collect();
        let mut t = NodeTree { node, children };
        t.node.children = Vec::new(); // 重插时由 children 重建
        Some(t)
    }

    pub fn root_sid(&self) -> &str {
        self.node.sid.as_str()
    }

    /// 按 sid 取出一个直接子树(从本树移除)。
    pub fn take_child(&mut self, sid: &str) -> Option<NodeTree> {
        let pos = self
            .children
            .iter()
            .position(|c| c.node.sid.as_str() == sid)?;
        Some(self.children.remove(pos))
    }

    /// 把整棵树种回文档(新 NodeId,原 sid);返回创建的根 NodeId。
    pub fn insert_into(
        &self,
        doc: &mut Document,
        parent: NodeId,
        index: usize,
        created: &mut Vec<NodeId>,
    ) -> NodeId {
        let mut n = self.node.clone();
        n.parent = Some(parent);
        n.children = Vec::new();
        let id = doc.nodes.insert(n);
        created.push(id);
        let parent_ref = doc.nodes.get_mut(parent).expect("parent");
        let idx = index.min(parent_ref.children.len());
        parent_ref.children.insert(idx, id);
        for c in &self.children {
            c.insert_into(doc, id, usize::MAX, created);
        }
        id
    }
}
