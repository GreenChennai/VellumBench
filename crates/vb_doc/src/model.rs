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
        /// 富文本段注记:`text` 字节区间 [start,end) 的样式覆盖(升序不重叠,
        /// 区间外为无样式文本;`\n` 表示 `<br>`)。编辑 text 时必须清空。
        segments: Vec<TextSeg>,
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

/// 行内段样式(相对节点自身样式的覆盖;None = 继承节点)。
///
/// 04 阶段(字符面板)扩展:在原 color/bold/italic/font_size/font_family
/// 基础上增加 line_height / letter_spacing / baseline_shift(px,Option)与
/// underline / strikethrough(Option<bool>)。**捕获与发射必须对称**
/// (import `inline_style_of` ⇔ export `seg_style_attr`),扩展字段必须带
/// 「HTML → 模型 → HTML」往返幂等测试(`tests/charseg_roundtrip.rs`)。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SegStyle {
    pub color: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub font_size: Option<f64>,
    pub font_family: Option<String>,
    /// 行距,px(捕获:显式 px,或无单位倍数 × 本段 font_size)。
    pub line_height: Option<f64>,
    /// 字距(tracking),px。
    pub letter_spacing: Option<f64>,
    /// 基线偏移,px(正 = 升,负 = 降;落点 `vertical-align`)。
    pub baseline_shift: Option<f64>,
    /// 下划线 / 删除线(同落点 `text-decoration`;仅 Some(true) 生效,
    /// None = 继承/无)。
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
}

/// 富文本段:`text` 的字节区间 + 样式。
#[derive(Debug, Clone, PartialEq)]
pub struct TextSeg {
    pub start: usize,
    pub end: usize,
    pub style: SegStyle,
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
    pub comment_before: Vec<String>,
    pub geom: Geom,
    /// 四维 [x, y, w, h] 是否为作者 CSS 显式声明(导入记录;布局层据此
    /// 区分「显式尺寸」与「默认占位」,未声明者走 auto 语义)。
    pub authored: [bool; 4],
    /// 作者声明的 position(导入记录;布局层区分显式定位与推断)。
    pub authored_position: Option<String>,
    /// 作者原始几何声明仍完整保留在 `style` 中(P0-1 保真往返的分层标志)。
    ///
    /// - `true`(声明层):该节点的定位/尺寸**不能无损折叠**为 px left/top +
    ///   width/height(百分比锚 / inset / right|bottom 锚 / 流式 / 自动尺寸),
    ///   原始声明原样留在 `style`;`geom` 只是 vb_layout 的内存求值结果,供画布
    ///   与几何查询使用,**保存时不得把 geom 烤进 CSS**(导出只写 style)。
    /// - `false`(规范化层):导入期已把几何无损折叠进 `geom`,或用户已通过
    ///   [`Node::materialize_geom`] 显式编辑过几何;导出由 `geom` 写显式 px,
    ///   并过滤 style 中遗留的几何声明(防双写/级联覆盖)。
    pub geom_declared: bool,
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
            comment_before: Vec::new(),
            geom: Geom::default(),
            authored: [false; 4],
            authored_position: None,
            geom_declared: false,
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

    /// 把声明几何兑换为显式几何(用户**显式移动/缩放/对齐**该节点时调用;
    /// `Command::SetGeom` 应用时触发)。
    ///
    /// 只翻状态位、不改 `style`:原始几何声明仍留在 style 中,由导出层对
    /// `geom_declared == false` 的节点统一过滤(见 export::geom_decls)——
    /// 这样撤销(SetGeom revert 恢复 geom)无需额外捕获 style 快照。
    /// materialize 之后该节点的导出与布局都以显式 px 为准。
    pub fn materialize_geom(&mut self) {
        if self.geom_declared {
            self.geom_declared = false;
        }
        self.authored = [true, true, true, true];
        self.authored_position = Some("absolute".to_string());
    }

    /// 解析填充色(纯色)。
    pub fn fill_color(&self) -> Option<Rgba> {
        let v = self
            .style_get("background-color")
            .or_else(|| self.style_get("background"))?;
        vb_common::color::parse_color(v)
    }
}

/// 05-5 断点覆盖规则:`@media (max-width: {max_width}px)` 块内的一条类规则。
///
/// 以**节点 sid** 寻址(命令层同款纪律,Undo/重排下 NodeId 会变);
/// 导出期解析为该节点的**首类**选择器(`finalize_classes` 保证唯一),
/// 序列化为 canonical `@media (max-width: Npx) { .cls { … } }` 块。
#[derive(Debug, Clone, PartialEq)]
pub struct MediaRule {
    /// 断点宽(px;查询固定为 max-width 语义)。
    pub max_width: u32,
    /// 目标节点 sid。
    pub sid: String,
    /// 覆盖声明(空声明条目不存在 —— 命令层以空值表达删除)。
    pub decls: Vec<Decl>,
}

/// 05-5 伪类规则(最小闭环:仅 `:hover`;其余伪类保持冻结块原文)。
#[derive(Debug, Clone, PartialEq)]
pub struct PseudoRule {
    /// 目标节点 sid。
    pub sid: String,
    /// 伪类名(当前固定 "hover";预留扩展位,序列化 `{selector}:{pseudo}`)。
    pub pseudo: String,
    pub decls: Vec<Decl>,
}

#[derive(Debug, Clone)]
pub struct Document {
    /// 修订号:每次命令应用 +1(Agent 乐观锁)。
    ///
    /// COUP-02:读取走 [`Document::rev`],保存路径的落盘后自增走
    /// [`Document::bump_rev`] —— 字段仍 `pub`(存量调用面大,P1 收口进行时,
    /// 台账 `docs/design/coup02-followup.md`)。
    pub rev: u64,
    pub meta: Meta,
    /// 场景图 arena。**COUP-02**:新代码请走 [`Document::nodes`] /
    /// [`Document::nodes_mut`] 访问器(受控出口,方法与字段同名,
    /// `doc.nodes()` 即访问器);字段保留 pub 供存量调用面
    /// (vb_app/vb_kiln/vb_kit 面板与导入路径),`#[doc(hidden)]`
    /// 让文档与 IDE 不再鼓励直达。存量清单与收口口径见
    /// `docs/design/coup02-followup.md`。
    #[doc(hidden)]
    pub nodes: SlotMap<NodeId, Node>,
    #[doc(hidden)]
    pub root: NodeId,
    /// 画板顺序 = 导出顺序。COUP-02:读取走 [`Document::artboards`]。
    #[doc(hidden)]
    pub artboards: Vec<NodeId>,
    /// 05-8 符号主件定义区(ADR-VB-L10):与 `root` 平级的第二棵 arena 树,
    /// **不在任何画板下** —— 画布/布局/导出页面内容都只走 `artboards`,
    /// 主件原型因此天然不参与页面渲染。
    ///
    /// 每个直接子节点 = 一个主件定义容器(tag div、`hidden` 属性、
    /// `data-vb-name` = 主件名;导出时补写标记类 `vb-symbol-defs`),
    /// 容器的**子树**即主件原型(真实节点,可被普通命令编辑 —— 这是
    /// 「编辑主件 → 同步实例」能用现有命令底座的前提)。
    pub defs_root: NodeId,
    /// 设计令牌 → `:root` CSS 变量(不带 `--` 前缀存储)。
    pub tokens: Vec<(String, String)>,
    /// 白名单外/复杂选择器 CSS 块(verbatim 保底)。
    ///
    /// COUP-02:vb_kiln/vb_app 有存量读者(断点/时间轴/资产面板),保持
    /// pub;收口随 vb_app 面板批次走(台账 `docs/design/coup02-followup.md`)。
    pub raw_css: Vec<String>,
    /// body 末尾原样透传片段(`<script>` 等)。COUP-02:零外部使用者 →
    /// `pub(crate)`(文档构成只经 import/export 命令路径改写)。
    pub(crate) trailing_raw: Vec<String>,
    /// head 中无法建模的原样透传片段(meta/link 等,除 charset/viewport/title 外)。
    ///
    /// COUP-02:vb_app(断点/文档设置)有存量读者,保持 pub(同 `raw_css`)。
    pub head_extra: Vec<String>,
    /// `<html>` 元素除 lang 外的保真属性(B5)。COUP-02:零外部使用者 →
    /// `pub(crate)`。
    pub(crate) extra_html_attrs: Vec<(String, String)>,
    /// `<body>` 元素的保真属性(B5)。COUP-02:零外部使用者 → `pub(crate)`。
    pub(crate) extra_body_attrs: Vec<(String, String)>,
    /// 05-5:断点覆盖规则(`@media (max-width: Npx)` 内的类规则,可编辑)。
    pub media_rules: Vec<MediaRule>,
    /// 05-5:伪类规则(最小闭环 :hover;可编辑)。
    pub pseudo_rules: Vec<PseudoRule>,
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
        // 主件定义区根:与 root 平级的 arena 节点(parent = None),
        // sid 固定 "defs"(parse 允许 [a-z0-9-],不与短码空间冲突 ——
        // 短码纯 base36,不会撞上带连字符的保留名)。
        let defs_root = nodes.insert(Node::new(
            NodeKind::Layer,
            "__symbol_defs__",
            StableId::parse("defs").expect("保留 sid 合法"),
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
            defs_root,
            artboards: Vec::new(),
            tokens: Vec::new(),
            raw_css: Vec::new(),
            trailing_raw: Vec::new(),
            head_extra: Vec::new(),
            extra_html_attrs: Vec::new(),
            extra_body_attrs: Vec::new(),
            media_rules: Vec::new(),
            pseudo_rules: Vec::new(),
            sid_next: 1,
        }
    }

    /// 新建默认文档:单画板 1440×900(设计文档 00 篇 §七 MVP 链路第 1 步)。
    pub fn new_default() -> Document {
        Document::new("未命名", "zh-CN")
    }

    pub fn alloc_sid(&mut self) -> StableId {
        // 占用表快照一次(RB 审查 PERF-06):旧实现每候选一次全表线性扫,
        // 冲突链一长即 O(n²);快照后每候选 O(1)。快照而非常驻索引:
        // `nodes` 是 pub 字段,vb_app/vb_agent 直接 insert(见其 assets/
        // breakpoints/align 面板),常驻 sid→NodeId 索引无法保证失效同步。
        let used: std::collections::HashSet<&str> =
            self.nodes.values().map(|n| n.sid.as_str()).collect();
        loop {
            let id = StableId::from_seed(self.sid_next);
            self.sid_next += 1;
            if !used.contains(id.as_str()) {
                return id;
            }
        }
    }

    /// 批量分配伴侣(PERF-06):`used` 由调用方以 [`Document::sid_set`]
    /// 初始化并**随导入全程更新** —— 导入期节点逐个入 arena,每分配一次
    /// 重扫全表是 O(n²);走本入口后每分配 O(1) 摊销。
    /// 语义与 [`Document::alloc_sid`] 完全一致(冲突即跳 seed)。
    pub fn alloc_sid_tracked(&mut self, used: &mut std::collections::HashSet<String>) -> StableId {
        loop {
            let id = StableId::from_seed(self.sid_next);
            self.sid_next += 1;
            if used.insert(id.as_str().to_string()) {
                return id;
            }
        }
    }

    /// 全文档占用 sid 集合快照(导入期批量分配的初始 [`HashSet`])。
    pub fn sid_set(&self) -> std::collections::HashSet<String> {
        self.nodes
            .values()
            .map(|n| n.sid.as_str().to_string())
            .collect()
    }

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

    /// 节点相对其画板原点的 bbox(DOC-12:无效 id 走 Option 出口,不 panic)。
    pub fn local_bbox(&self, id: NodeId) -> Option<kurbo::Rect> {
        let n = self.nodes.get(id)?;
        Some(kurbo::Rect::new(
            n.geom.x,
            n.geom.y,
            n.geom.x + n.geom.w,
            n.geom.y + n.geom.h,
        ))
    }

    /// 深度优先收集子树(含自身)。
    ///
    /// 显式栈迭代(RB-02/DOC-01:访问序 = 前序 DFS,与旧递归版一致;
    /// 导出 CSS 规则序依赖本序,不得改变)。
    pub fn subtree(&self, id: NodeId, out: &mut Vec<NodeId>) {
        let mut stack = vec![id];
        while let Some(x) = stack.pop() {
            out.push(x);
            if let Some(n) = self.nodes.get(x) {
                for &c in n.children.iter().rev() {
                    stack.push(c);
                }
            }
        }
    }

    /// 深拷贝子树为新节点(保留 sid 之外的属性;sid 重新分配,sid 是唯一身份——
    /// 复制体是新元素,按 ADR-0010 语义必须拿到自己的稳定 id)。
    pub fn clone_subtree(&mut self, id: NodeId, new_parent: NodeId) -> Option<NodeId> {
        let src = self.nodes.get(id)?.clone();
        Some(self.insert_cloned_rec(&src, new_parent))
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

    // ── COUP-02(2026-10-05,S6):访问器 API + 只读视图 ──
    //
    // 口径(诚实评估,不为翻字段而破坏编译面):
    // - 访问器是**推荐出口**;`nodes`/`root`/`defs_root`/`artboards` 因
    //   vb_app/vb_kiln/vb_kit 存量调用面大(数百处),字段保持 pub +
    //   `#[doc(hidden)]`,清零是进行时(台账 `docs/design/coup02-followup.md`);
    // - 零外部使用者的字段已翻 `pub(crate)`;
    // - 跨 crate 只读遍历走 [`DocumentView`] trait(未来可换实现)。

    /// 修订号(命令应用计数;Agent 乐观锁比对用)。
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// 落盘后自增修订号(vb_agent 保存路径;命令路径不走这里 ——
    /// 命令应用在 [`crate::commands`] 内部推进)。
    pub fn bump_rev(&mut self) -> u64 {
        self.rev += 1;
        self.rev
    }

    /// 文档元信息(只读)。
    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    /// 文档元信息(受控可变出口:标题/语言/输出模式)。
    pub fn meta_mut(&mut self) -> &mut Meta {
        &mut self.meta
    }

    /// 场景图 arena(只读)。
    pub fn nodes(&self) -> &SlotMap<NodeId, Node> {
        &self.nodes
    }

    /// 场景图 arena(受控可变出口:插入/逐点改节点;结构性变更加完
    /// 请调 [`Document::sync_artboards`] 保持画板注册表一致)。
    pub fn nodes_mut(&mut self) -> &mut SlotMap<NodeId, Node> {
        &mut self.nodes
    }

    /// 文档根(`root` 哨兵,不在任何画板内;寻址校验用)。
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// 符号主件定义区根(ADR-VB-L10;与 [`Document::root`] 平级)。
    pub fn defs_root(&self) -> NodeId {
        self.defs_root
    }

    /// 画板序(导出序;只读)。
    pub fn artboards(&self) -> &[NodeId] {
        &self.artboards
    }

    /// 画板序(受控可变出口:重排导出顺序用;增删请走
    /// [`Document::new_artboard`] / 命令层 + [`Document::sync_artboards`])。
    pub fn artboards_mut(&mut self) -> &mut Vec<NodeId> {
        &mut self.artboards
    }

    /// 断点覆盖规则(只读;05-5)。
    pub fn media_rules(&self) -> &[MediaRule] {
        &self.media_rules
    }

    /// 断点覆盖规则(受控可变出口)。
    pub fn media_rules_mut(&mut self) -> &mut Vec<MediaRule> {
        &mut self.media_rules
    }

    /// 伪类规则(只读;05-5)。
    pub fn pseudo_rules(&self) -> &[PseudoRule] {
        &self.pseudo_rules
    }

    /// 伪类规则(受控可变出口)。
    pub fn pseudo_rules_mut(&mut self) -> &mut Vec<PseudoRule> {
        &mut self.pseudo_rules
    }
}

/// 跨 crate **只读**视图(COUP-02 / COUP-R3:跨 crate 只读访问走访问器
/// / trait,不暴露可变内部)。
///
/// 用途:vb_agent / vb_plugin / vb_session 等下层 crate 的遍历型代码
/// (投影/校验/统计)以 `impl DocumentView` / `&dyn DocumentView` 收参,
/// 不与 `Document` 具体类型绑定 —— 未来 arena 换型或加校验层时不破调用面。
/// 只含读;写一律走 [`Document`] 受控出口或命令层。
pub trait DocumentView {
    /// 修订号。
    fn rev(&self) -> u64;
    /// 文档元信息。
    fn meta(&self) -> &Meta;
    /// 场景图 arena(只读)。
    fn nodes(&self) -> &SlotMap<NodeId, Node>;
    /// 文档根哨兵。
    fn root(&self) -> NodeId;
    /// 符号定义区根。
    fn defs_root(&self) -> NodeId;
    /// 画板序(导出序)。
    fn artboards(&self) -> &[NodeId];
    /// 按 id 取节点。
    fn node(&self, id: NodeId) -> Option<&Node>;
    /// 按 sid 查 NodeId。
    fn find_by_sid(&self, sid: &str) -> Option<NodeId>;
    /// sid 是否已被占用。
    fn sid_in_use(&self, sid: &str) -> bool;
    /// `target` 是否为 `ancestor` 自身或其后代。
    fn is_descendant_or_self(&self, ancestor: NodeId, target: NodeId) -> bool;
    /// 画板原点(画板本地几何的参照)。
    fn artboard_origin(&self, artboard: NodeId) -> (f64, f64);
    /// 深度优先收集子树(含自身,前序)。
    fn subtree(&self, id: NodeId, out: &mut Vec<NodeId>);
}

impl DocumentView for Document {
    fn rev(&self) -> u64 {
        Document::rev(self)
    }
    fn meta(&self) -> &Meta {
        Document::meta(self)
    }
    fn nodes(&self) -> &SlotMap<NodeId, Node> {
        Document::nodes(self)
    }
    fn root(&self) -> NodeId {
        Document::root(self)
    }
    fn defs_root(&self) -> NodeId {
        Document::defs_root(self)
    }
    fn artboards(&self) -> &[NodeId] {
        Document::artboards(self)
    }
    fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id)
    }
    fn find_by_sid(&self, sid: &str) -> Option<NodeId> {
        Document::find_by_sid(self, sid)
    }
    fn sid_in_use(&self, sid: &str) -> bool {
        Document::sid_in_use(self, sid)
    }
    fn is_descendant_or_self(&self, ancestor: NodeId, target: NodeId) -> bool {
        Document::is_descendant_or_self(self, ancestor, target)
    }
    fn artboard_origin(&self, artboard: NodeId) -> (f64, f64) {
        Document::artboard_origin(self, artboard)
    }
    fn subtree(&self, id: NodeId, out: &mut Vec<NodeId>) {
        Document::subtree(self, id, out)
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
    /// 从 arena 抽取子树快照。
    ///
    /// 显式栈迭代 + 自底向上组装(RB-02/DOC-01):旧递归版在异常深
    /// 文档上会栈溢出。语义不变 —— 只跟随 `parent` 指回自身的孩子,
    /// 快照节点自身的 `children` id 列表清空(重插时由快照 children 重建)。
    pub fn from_document(doc: &Document, id: NodeId) -> Option<NodeTree> {
        doc.nodes.get(id)?;
        let mut stack: Vec<(NodeId, bool)> = vec![(id, false)];
        let mut built: std::collections::HashMap<NodeId, NodeTree> =
            std::collections::HashMap::new();
        while let Some((nid, done)) = stack.pop() {
            if done {
                let mut node = doc.nodes.get(nid)?.clone();
                let children = node
                    .children
                    .iter()
                    .filter(|&&c| {
                        doc.nodes
                            .get(c)
                            .map(|n| n.parent == Some(nid))
                            .unwrap_or(false)
                    })
                    .filter_map(|&c| built.remove(&c))
                    .collect::<Vec<_>>();
                node.children = Vec::new(); // 重插时由 children 重建(语义同旧实现)
                built.insert(nid, NodeTree { node, children });
                continue;
            }
            stack.push((nid, true));
            let n = doc.nodes.get(nid)?;
            for &c in &n.children {
                if doc
                    .nodes
                    .get(c)
                    .map(|cn| cn.parent == Some(nid))
                    .unwrap_or(false)
                {
                    stack.push((c, false));
                }
            }
        }
        built.remove(&id)
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

// ─────────────────────── COUP-02 门禁(单测) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 访问器与字段同值(访问器是推荐出口,字段为存量兼容保留)。
    #[test]
    fn accessors_agree_with_fields() {
        let doc = Document::new_default();
        assert_eq!(doc.rev(), doc.rev);
        assert_eq!(doc.root(), doc.root);
        assert_eq!(doc.defs_root(), doc.defs_root);
        assert_eq!(doc.artboards(), doc.artboards.as_slice());
        assert_eq!(doc.nodes().len(), doc.nodes.len());
        assert_eq!(doc.meta().title, doc.meta.title);
        assert_eq!(doc.media_rules().len(), doc.media_rules.len());
        assert_eq!(doc.pseudo_rules().len(), doc.pseudo_rules.len());
    }

    /// 受控可变出口与直改同效(nodes_mut / artboards_mut / meta_mut /
    /// bump_rev),并保持派生状态一致。
    #[test]
    fn mutable_accessors_roundtrip() {
        let mut doc = Document::new_default();
        let ab = doc.artboards()[0];
        let sid = doc.alloc_sid();
        let id = doc
            .nodes_mut()
            .insert(Node::new(NodeKind::Box, "x", sid.clone()));
        doc.nodes_mut().get_mut(ab).unwrap().children.push(id);
        doc.nodes_mut().get_mut(id).unwrap().parent = Some(ab);
        doc.meta_mut().title = "受控出口".into();
        doc.media_rules_mut().push(MediaRule {
            max_width: 640,
            sid: sid.as_str().to_string(),
            decls: Vec::new(),
        });
        assert_eq!(doc.meta().title, "受控出口");
        assert_eq!(doc.media_rules()[0].max_width, 640);
        assert_eq!(doc.node(id).map(|n| n.name.as_str()), Some("x"));
        // bump_rev:保存路径自增(命令路径不经过它)
        let before = doc.rev;
        assert_eq!(doc.bump_rev(), before + 1);
    }

    /// 只读视图 trait 与固有方法同值(跨 crate 遍历的稳定出口)。
    #[test]
    fn document_view_trait_matches_inherent() {
        let doc = Document::new_default();
        fn view_summary(v: &impl DocumentView) -> (u64, usize, String) {
            let mut out = Vec::new();
            for &ab in v.artboards() {
                v.subtree(ab, &mut out);
            }
            (
                v.rev(),
                out.len(),
                v.find_by_sid("nonexistent")
                    .map(|_| "x".to_string())
                    .unwrap_or_else(|| "无".into()),
            )
        }
        let (rev, nodes, miss) = view_summary(&doc);
        assert_eq!(rev, doc.rev());
        assert!(nodes >= 1, "子树遍历至少含画板自身");
        assert_eq!(miss, "无");
        // 动态派发同样可用(对象安全)
        let dyn_view: &dyn DocumentView = &doc;
        assert_eq!(dyn_view.artboards(), doc.artboards());
        assert!(dyn_view.sid_in_use(doc.node(doc.artboards()[0]).unwrap().sid.as_str()));
        let ab = doc.artboards()[0];
        assert!(dyn_view.is_descendant_or_self(doc.root(), ab));
        assert_eq!(dyn_view.artboard_origin(ab), doc.artboard_origin(ab));
    }
}
