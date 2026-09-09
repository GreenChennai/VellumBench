# 04 · 文档模型与 HTML 序列化

> **核心命题**：内存里的场景图 ↔ 磁盘上的 HTML，必须能无限次无损往返。
> 这条不成立，产品就不成立（见 00 篇 Q2）。

---

## 一、场景图结构（内存真相）

单一真相源 `Document`，所有变更走命令（见 09 篇）。用 **Arena（slotmap）+ 稳定 id** 而非 `Rc<RefCell>` 树：便于并行遍历、避免循环引用、Undo 便宜。

```rust
// vb_doc/src/model.rs（示意）
pub struct Document {
    pub rev: u64,                 // 每次变更 +1，Agent 乐观锁用
    pub meta: Meta,               // 标题、描述、语言、输出模式、命名策略
    pub nodes: Arena<Node>,       // 所有节点（含画板/图层/编组/元素）
    pub root: NodeId,             // 虚拟根
    pub artboards: Vec<NodeId>,   // 顺序 = 画板顺序
    pub layers: Vec<NodeId>,      // 顺序 = 图层面板顺序（自顶向下）
    pub tokens: TokenTable,       // 设计令牌 → CSS 变量
    pub assets: AssetStore,       // 图片/字体引用与缓存
    pub view: ViewState,          // 缩放/滚动/网格/参考线 —— 不入 HTML
}

pub struct Node {
    pub nid: NodeId,              // 内部 arena 索引（不落盘）
    pub sid: StableId,            // 稳定短码 "a7f3c2" → data-vb-id（永不因改名而变）
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,    // 顺序 = z 序（后 = 上）
    pub kind: NodeKind,
    pub name: String,             // AI 图层名，如「主标题」
    pub tag: Tag,                 // div/section/h1/p/a/button/img/svg/…
    pub classes: Vec<String>,
    pub attrs: BTreeMap<String, String>,   // href/src/alt/aria-*/role…
    pub style: StyleSet,          // CSS 白名单内属性
    pub local: Affine,            // 本地变换（平移/旋转/缩放/倾斜）
    pub flags: Flags,             // locked / hidden / isArtboard / isLayer …
    pub payload: Payload,         // 按 kind 区分的内容
}

pub enum NodeKind {
    Artboard,                     // 画板
    Layer,                        // 图层（顶层容器）
    Group,                        // 编组
    Box,                          // 盒对象（矩形/圆角矩形/椭圆→border-radius:50%）
    Text { content: TextContent, mode: TextMode },  // 点文本 / 区域文本
    Image { src: AssetId, fit: ObjectFit },
    Vector { path: BezPath, style: SvgStyle },      // 钢笔/铅笔产物
    Slice,
    Frozen(FrozenBlock),          // 白名单外的原始 HTML 片段
}

pub struct StyleSet {
    known: EnumMap<Prop, Option<Value>>,  // 白名单内，可编辑
    unknown: Vec<RawDecl>,                // 白名单外，原样保留（不丢失！）
    var_refs: BTreeMap<Prop, TokenId>,    // 值为 CSS 变量时记录引用
}
```

**关键设计点**：
1. **`unknown` 字段保命**：即使某个 CSS 属性当前不支持，也从不在导入时丢弃它。能解析就存 `RawDecl`，序列化原样写回。这保证"不编辑也不损坏"。
2. **`sid` 与 `nid` 分离**：`sid` 是给 Agent 和 HTML 用的稳定锚点；`nid` 是内存索引，可被 Undo/重排改变。
3. **`view` 不落 HTML**：网格、参考线、缩放、面板布局只存 `.vbdoc`。

---

## 二、坐标与几何

| 项 | 规则 |
|---|---|
| 元素坐标 | 存**相对父级画板**的本地坐标（px，Y 向下） |
| 定位方式 | 默认 `position:absolute; left/top`（矢量式精确摆放）；启用"自动布局"后改为 flex 文档流 |
| 尺寸 | `width/height`；盒描边默认 `box-sizing:border-box` |
| 变换 | `Affine`；序列化分解为 `translate() rotate() scale() skew()`，顺序固定 |
| 角度 | 内部 AI 语义（逆时针为正）；**输出 CSS 时取负**（见 01 篇第五节） |
| 精度 | 计算用 f64；输出 string 时保留 **最多 4 位小数并去尾随 0** |

> **定位策略开关**（新建文档时选，可随时改）：
> - **矢量模式**（默认）：一切绝对定位，所见即所得，像 AI
> - **流式模式**：用 flex/grid 组织，元素靠约束与间距定位，导出代码更"前端友好"
> - 混合：画板内可局部流式（容器开 flex，子元素流式，容器本身绝对定位）

---

## 三、CSS 白名单（分级支持）

### L1 — v0.1 必须支持（覆盖 80% 落地页场景）

| 组 | 属性 |
|---|---|
| 盒模型 | `width` `height` `min-*` `max-*` `padding` `margin` `box-sizing` `aspect-ratio` |
| 定位 | `position`(static/relative/absolute) `left` `top` `right` `bottom` `z-index` |
| 布局 | `display`(block/flex/inline-flex/none) `flex-direction` `flex-wrap` `gap` `justify-content` `align-items` `align-self` `flex` |
| 背景 | `background-color` `background-image`(url / linear-gradient / radial-gradient) `background-size` `background-position` `background-repeat` |
| 边框 | `border-*-width/style/color` `border-radius`（含四角分别） `outline` |
| 阴影 | `box-shadow`（多层） `text-shadow` |
| 文本 | `font-family` `font-size` `font-weight` `font-style` `line-height` `letter-spacing` `text-align` `color` `text-decoration` `text-transform` `white-space` `overflow-wrap` |
| 变换 | `transform` `transform-origin` |
| 视觉 | `opacity` `mix-blend-mode` `overflow` `visibility` `clip-path`(inset/circle/path) |
| 交互 | `cursor` |

### L2 — v0.7（网页能力补齐）

`display:grid` + `grid-template-*` + `grid-area`、`@media` 断点、伪类 `hover/active/focus/disabled`、CSS 变量与 `var()`、`transition`、`filter`(blur/brightness/contrast/saturate/grayscale/drop-shadow)、`backdrop-filter`、`mask-image`、`object-fit/object-position`、`conic-gradient`、`writing-mode`、`list-style`、`gap` 全系、`@font-face`。

### L3 — v2+（动效与高级）

`@keyframes` + `animation`（时间轴驱动）、`scroll-behavior`、`@container` 容器查询、`subgrid`、`scroll-snap`、`position:sticky`（编辑态特殊处理）、`text-wrap:balance`。

### 永不编辑（→ 冻结块）

| 类型 | 处理 |
|---|---|
| `<script>` | 保留在输出中（原样写回），编辑态不执行；提示"脚本不会在编辑器内运行" |
| `<canvas>` `<video>` `<iframe>` | 冻结块，画布内显示占位框（canvas 显示静态首帧/灰框，video 显示 `<video>` 海报或灰框） |
| `:has()` / `@supports` / 复杂选择器 | 原样保留，不参与样式编辑 |
| 未知 CSS 属性 | 存入 `unknown`，原样写回 |
| CSS 框架类名（Tailwind 等） | 保留 class 名不删除；仅当用户在属性面板修改对应属性时，追加内联覆盖或提示 |

---

## 四、冻结块（Frozen Block）

**定义**：无法被场景图无损表达的子树。

**表现**：
- 渲染：用一次快照（Vello 或 Chromium 截屏）绘制为一张图，随缩放重采样；缩放到 >150% 时提示"精度有限"
- 交互：可移动/缩放/旋转/删除/锁定/隐藏，**双击**打开「代码视图」弹窗（只读高亮 + 可选"降级为可编辑"）
- 图标：❄ 角标；图层面板中灰色斜体

**降级为可编辑**（尽力而为）：把 `<div style="backdrop-filter: blur(8px)">` 中的 `backdrop-filter` 丢进 `unknown`（渲染时忽略），其余属性正常解析 → 用户可编辑，代价是该效果在编辑态不显示（导出时仍在）。

---

## 五、HTML 序列化规则（必须严格，保证可 diff）

### 5.1 输出文件

```
MyProject/
├── index.html
├── styles/
│   └── main.css        （默认；可选内联 / 单文件）
└── assets/
    ├── hero.png
    └── hero@2x.png
```

> 输出模式（新建文档时选，可改）：
> 1. **外链 CSS**（默认）：`index.html` + `styles/main.css`
> 2. **`<style>` 内联**：单目录少文件，方便丢给别人
> 3. **单文件 HTML**：一切内联（图片转 base64 / 或保留相对路径），便于分享

### 5.2 格式化（与 Prettier 对齐，CI 校验）

- 缩进 2 空格；不写 `><` 同行的深度嵌套
- 属性顺序（固定）：`class` → `id` → `data-vb-id` → `data-vb-name` → 语义属性（`href`/`src`/`alt`/`type`/`aria-*`/`role`）→ `style`（仅当用户显式内联时）
- CSS 声明顺序：按 `PROP_ORDER` 表（位置→尺寸→盒→背景→边框→文本→变换→其他）
- CSS 选择器顺序：`:root` 变量 → 元素选择器（`body`,`h1`）→ 类选择器（按首次出现顺序）
- 数值：`0` 不加单位；`1.0` → `1`；最多 4 位小数；`±0` → `0`
- 颜色：小写 hex；`#ffffff` → `#fff`；能用变量就用 `var(--vb-color-1)`
- 文本：中文原样（不转实体）；仅转义 `&` `<` `>`；保留换行与空格语义（`white-space` 非默认时用 `&nbsp;`/`\n` 谨慎处理）
- 空元素：`<img …>` 自闭合；非空元素即使空也写成双标签（`<div></div>` 不缩成 `<div/>`）
- 文件末尾换行（LF），统一 LF（Windows 也写 LF，避免 git 噪音）

### 5.3 输出示例

AI 图层结构：
```
📁 内容
  📁 Hero 编组
    ▢ Hero 背景（矩形 1440×600，线性渐变）
    ▢ 主标题（文本 "香醇，从一颗豆开始"）
    ▢ CTA 按钮（圆角矩形 + 填充 #FF5A1F）
```

输出 `index.html`：
```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>咖啡品牌落地页</title>
  <link rel="stylesheet" href="styles/main.css">
</head>
<body>
  <section class="vs-artboard hero" data-vb-id="b1e0aa" data-vb-name="Hero">
    <div class="vs-layer content" data-vb-id="c2f1b3" data-vb-name="内容">
      <div class="hero-group" data-vb-id="d3a2c4" data-vb-name="Hero 编组">
        <div class="hero-bg" data-vb-id="e4b3d5" data-vb-name="Hero 背景"></div>
        <h1 class="hero-title" data-vb-id="f5c4e6" data-vb-name="主标题">香醇，从一颗豆开始</h1>
        <a class="cta-button" data-vb-id="a6d5f7" data-vb-name="CTA 按钮" href="#">立即购买</a>
      </div>
    </div>
  </section>
</body>
</html>
```

输出 `styles/main.css`：
```css
:root {
  --vb-brand-1: #ff5a1f;
}
.vb-artboard {
  position: relative;
  overflow: hidden;
}
.hero {
  width: 1440px;
  height: 600px;
  background-color: #fff;
}
.hero-bg {
  position: absolute;
  left: 0;
  top: 0;
  width: 1440px;
  height: 600px;
  background-image: linear-gradient(180deg, #2b1a12 0%, #6b3f24 100%);
}
.hero-title {
  position: absolute;
  left: 120px;
  top: 220px;
  width: 720px;
  font-family: Inter, "PingFang SC", sans-serif;
  font-size: 64px;
  font-weight: 700;
  line-height: 1.2;
  color: #fff;
}
.cta-button {
  position: absolute;
  left: 120px;
  top: 360px;
  padding: 16px 32px;
  border-radius: 8px;
  background-color: var(--vb-brand-1);
  font-size: 18px;
  color: #fff;
  text-decoration: none;
}
```

> 注意：`data-vb-id` / `data-vb-name` 是**可选输出**（设置项，默认开）。关掉后输出更干净，但 Agent 只能靠 class/选择器寻址。

---

## 六、往返保真（Round-trip）

### 6.1 三级保真

| 级别 | 定义 | 目标 |
|---|---|---|
| **L0 不损坏** | 导入 → 不编辑 → 保存，输出与输入语义等价 | **100%**（硬指标） |
| **L1 幂等** | 导入 → 保存 → 再导入 → 再保存，两次输出字节相同 | 100% |
| **L2 可编辑** | 白名单内属性可编辑且视觉一致 | L1 属性 100% |

### 6.2 实现手段

1. **导入时先做 canonicalize**：把输入 HTML 解析为 AST → 归一化（属性排序、空白合并、单位统一为 px）→ 与输出比较，用于测试。
2. **`unknown` 兜底**：任何无法解释的声明/属性/注释原样保留（注释挂到最近节点，作为 `leading_comments`）。
3. **属性保留表**：`<!-- vs:keep -->` 标记的区域完全不解析（v2）。
4. **测试语料**：100 个真实页面（自建 20 + 开源模板 50 + Agent 生成 30），作为 CI 的 round-trip 语料库，每次改动跑一遍。`tests/roundtrip/`。

### 6.3 外部修改（三向合并）

Agent 或用户用别的编辑器改了 `index.html` 时：

```
base  = 上次保存时的文档快照
ours  = 当前内存文档
theirs= 磁盘上的最新 HTML
```

合并粒度：**节点级（按 `sid`）+ 属性级（按 CSS 属性名）**

| 情况 | 处理 |
|---|---|
| 只有一方改 | 采用改动方 |
| 双方改同一节点的**不同属性** | 自动合并 |
| 双方改同一**属性** | 弹「外部修改冲突」对话框，逐项选择（保留磁盘/保留我的/两者都留） |
| 磁盘删除了我正在编辑的节点 | 提示并保留我的（默认）或采用删除（可选） |
| 新元素（无 sid） | 直接插入，分配新 sid |

默认策略可在设置里配：**自动采用外部修改**（Agent 场景推荐）/ **总是询问**（手工场景）。

---

## 七、工程文件 `.vbdoc`

```
MyProject.vbdoc  (ZIP 容器)
├── project.json     # 场景图 + 视图状态 + 元数据（人可读，2 空格缩进）
├── guides.json      # 参考线/网格/画板选项
├── history/         # 撤销历史（可选，按大小滚动）
├── cache/           # 冻结块快照、缩略图
└── manifest.json    # 版本、源 HTML 路径、assets 引用
```

- `project.json` 不存图片二进制（只存引用 + 相对 `assets/` 路径）
- 打开 `.vbdoc` 时若 `assets/` 缺失 → 提示定位
- 提供 `文件 → 另存为可版本控制目录`，输出 `project.json` + `index.html` + `styles/`（便于 git diff 工程状态）

**自动保存**：每 60s + 每次重大操作后，写 `.vbdoc.autosave`；崩溃后启动询问恢复。

---

## 八、Agent 友好约定（与 08 篇配套）

| 约定 | 内容 |
|---|---|
| 稳定 id | 每个元素 `data-vb-id="<6位>[a-z0-9]">`，全生命周期不变（改名/移动/重排都不变） |
| 可读名 | `data-vb-name="主标题"`，与图层面板同步 |
| 语义 class | 由图层名按命名策略生成（默认 kebab-case / 拼音 slug） |
| 注释锚点 | 画板开始处写 `<!-- vs:artboard hero -->`，便于 Agent 与人类定位（可选输出） |
| 输出稳定 | 属性顺序、CSS 顺序固定 → diff 最小 |

---

## 九、验收点

- [ ] `tests/roundtrip` 100 个语料：`L0` 通过率 100%，`L1` 幂等 100%
- [ ] 修改任一 L1 属性后保存，浏览器打开与编辑态视觉一致（自动像素比对，偏差 ≤1px）
- [ ] 含 `<script>`、`@media`、Tailwind class 的页面导入后保存，脚本与类名完整保留
- [ ] 外部改 HTML → 三向合并，三种冲突场景均按预期处理
- [ ] 输出 `index.html` 通过 W3C Nu Validator（无 error）
- [ ] 输出经 `prettier --check` 通过
- [ ] 角度/单位/颜色归一化均有单元测试（属性表驱动，≥200 条用例）
