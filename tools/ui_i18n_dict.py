#!/usr/bin/env python3
"""gen_ui_ftl.py 的词典数据(COMMON / EN_EXACT / PHRASES / BANNED_EN)。

- COMMON:源码文本 → 语义化 key(跨文件复用、高频、或需要稳定语义名);
- EN_EXACT:源码文本(zh 原文)→ 英文;遵守 CONTEXT.md 术语表
  (画板=Artboard、编组=Group、对象=Object、面板坞=Tab Dock、
  主页=Launcher、能力台账=Capability Ledger、组件语境用 Symbol;
  禁 Frame / Component / smart object);
- PHRASES:中文词 → 英文词(EN_EXACT 未命中时按最长分词组合兜底;
  全角标点映射英文标点;符号类字形原样保留);
- BANNED_EN:英文禁用词(与 tools/check_terminology.py 一致)。
"""

BANNED_EN = ["frame", "smart object", "component"]

COMMON = {
    "取消": "ui-common-cancel",
    "确定": "ui-common-ok",
    "确认": "ui-common-confirm",
    "保存": "ui-common-save",
    "关闭": "ui-common-close",
    "删除": "ui-common-delete",
    "撤销": "ui-common-undo",
    "重做": "ui-common-redo",
    "全选": "ui-common-select-all",
    "复制": "ui-common-copy",
    "剪切": "ui-common-cut",
    "粘贴": "ui-common-paste",
    "隐藏": "ui-common-hide",
    "显示": "ui-common-show",
    "锁定": "ui-common-lock",
    "解锁": "ui-common-unlock",
    "重命名": "ui-common-rename",
    "导出": "ui-common-export",
    "导入": "ui-common-import",
    "新建项目": "ui-common-new-project",
    "打开项目": "ui-common-open-project",
    "文字": "ui-common-text",
    "画板": "ui-common-artboard",
    "颜色": "ui-common-color",
    "填充": "ui-common-fill",
    "描边": "ui-common-stroke",
    "缩放": "ui-common-zoom",
    "对齐": "ui-common-align",
    "变换": "ui-common-transform",
    "旋转": "ui-common-rotate",
    "圆角": "ui-common-corner",
    "图层": "ui-common-layer",
    "外观": "ui-common-appearance",
    "命令面板": "ui-common-command-palette",
    "未选中对象": "ui-common-no-selection",
    "对象不存在": "ui-common-object-missing",
    "解析失败": "ui-common-parse-failed",
    "关于": "ui-common-about",
    "自定义": "ui-common-custom",
    "字号": "ui-common-font-size",
    "角度": "ui-common-angle",
    "标题": "ui-common-title",
    "冻结块": "ui-common-freeze-block",
    "粗细": "ui-common-weight",
    "其他": "ui-common-other",
    "宽度": "ui-common-width",
    "高度": "ui-common-height",
    "位置": "ui-common-position",
    "不透明度": "ui-common-opacity",
    "背景": "ui-common-background",
    "网格": "ui-common-grid",
    "参考线": "ui-common-guides",
    "预设": "ui-common-preset",
    "名称": "ui-common-name",
    "类型": "ui-common-type",
    "状态": "ui-common-state",
    "默认": "ui-common-default",
    "全部": "ui-common-all",
    "无": "ui-common-none",
    "开": "ui-common-on",
    "关": "ui-common-off",
    "重试": "ui-common-retry",
    "应用": "ui-common-apply",
    "重置": "ui-common-reset",
    "搜索": "ui-common-search",
    "帮助": "ui-common-help",
    "窗口": "ui-common-window",
    "文件": "ui-common-file",
    "编辑": "ui-common-edit",
    "对象": "ui-common-object",
    "选择": "ui-common-select",
    "效果": "ui-common-effect",
    "视图": "ui-common-view",
    # ── 二轮(频次 ≥2 的中频串)──
    "工作区": "ui-common-workspace",
    "未命名": "ui-common-untitled",
    "未命名项目": "ui-common-untitled-project",
    "线性": "ui-common-linear",
    "径向": "ui-common-radial",
    "横向": "ui-common-landscape",
    "纵向": "ui-common-portrait",
    "打开": "ui-common-open",
    "文本": "ui-common-text-obj",
    "时间轴": "ui-common-timeline",
    "渐变": "ui-common-gradient",
    "界面缩放": "ui-common-ui-scale",
    "插件": "ui-common-plugin",
    "直线": "ui-common-line-tool",
    "矩形": "ui-common-rect",
    "椭圆": "ui-common-ellipse",
    "羽化": "ui-common-feather",
    "镜像": "ui-common-mirror",
    "内阴影": "ui-common-inner-shadow",
    "内发光": "ui-common-inner-glow",
    "外发光": "ui-common-outer-glow",
    "投影": "ui-common-drop-shadow",
    "前移一层": "ui-common-bring-forward",
    "后移一层": "ui-common-send-backward",
    "切片": "ui-common-slice",
    "历史": "ui-common-history",
    "属性": "ui-common-properties",
    "主题": "ui-common-theme",
    "中心": "ui-common-center",
    "倍率": "ui-common-scale-factor",
    "内存": "ui-common-memory",
    "磁盘": "ui-common-disk",
    "冲突对比": "ui-common-conflict-diff",
    "反向": "ui-common-reverse",
    "取向": "ui-common-orientation",
    "取消固定": "ui-common-unpin",
    "固定": "ui-common-pin",
    "图像": "ui-common-image",
    "垂直": "ui-common-vertical",
    "水平": "ui-common-horizontal",
    "字体": "ui-common-font",
    "字体替换": "ui-common-font-substitute",
    "字色": "ui-common-text-color",
    "字距": "ui-common-letter-spacing",
    "崩溃恢复": "ui-common-crash-recovery",
    "布局": "ui-common-layout",
    "度量": "ui-common-measure",
    "我的项目": "ui-common-my-projects",
    "快照": "ui-common-snapshot",
    "抓手": "ui-common-hand-tool",
    "斜体": "ui-common-italic",
    "无障碍": "ui-common-a11y",
    "显示网格": "ui-common-show-grid",
    "替换": "ui-common-replace",
    "最近项目": "ui-common-recent-projects",
    "标点挤压": "ui-common-punct-squeeze",
    "模糊": "ui-common-blur",
    "段前": "ui-common-space-before",
    "段后": "ui-common-space-after",
    "浏览器校对": "ui-common-browser-proof",
    "清除动画": "ui-common-clear-animation",
    "画板底": "ui-common-artboard-bg",
    "画板数": "ui-common-artboard-count",
    "界面语言": "ui-common-ui-language",
    "移除": "ui-common-remove",
    "符号": "ui-common-symbol",
    "粗体": "ui-common-bold",
    "组": "ui-common-group",
    "编组": "ui-common-grouping",
    "首页": "ui-common-home",
    "行距": "ui-common-line-height",
    "资产": "ui-common-assets",
    "跳转确认": "ui-common-jump-confirm",
    "轮廓": "ui-common-outline",
    "输出模式": "ui-common-output-mode",
    "连字": "ui-common-ligatures",
    "避头尾": "ui-common-line-break-rules",
    "键位方案": "ui-common-keymap",
    "间距": "ui-common-gap",
    "面板": "ui-common-panel",
    "自动保存": "ui-common-autosave",
    "恢复": "ui-common-restore",
    "刷新": "ui-common-refresh",
    "重启": "ui-common-restart",
    "播放": "ui-common-play",
    "暂停": "ui-common-pause",
    "提交": "ui-common-commit",
    "展开": "ui-common-expand",
    "收起": "ui-common-collapse",
    "放大": "ui-common-zoom-in",
    "缩小": "ui-common-zoom-out",
    "退出": "ui-common-quit",
    "打印": "ui-common-print",
    "卸载": "ui-common-uninstall",
    "启用": "ui-common-enable",
    "丢弃": "ui-common-discard",
    "跳过": "ui-common-skip",
    "对比": "ui-common-compare",
    "深色": "ui-common-dark",
    "浅色": "ui-common-light",
    "中文": "ui-common-chinese",
    "常规": "ui-common-general",
    "性能": "ui-common-performance",
    "数据": "ui-common-data",
    "交互": "ui-common-interaction",
    "健康": "ui-common-health",
    "日志": "ui-common-log",
    "值": "ui-common-value",
    "修改": "ui-common-modify",
    "偏好": "ui-common-preferences",
    "模板": "ui-common-template",
    "计划": "ui-common-planned",
    "路径": "ui-common-path",
    "链接": "ui-common-link",
    "样式": "ui-common-style",
    "格式": "ui-common-format",
    "字符": "ui-common-char",
    "段落": "ui-common-para",
    "动画": "ui-common-animation",
    "缓动": "ui-common-easing",
    "时长": "ui-common-duration",
    "延迟": "ui-common-delay",
    "因子": "ui-common-factor",
    "循环": "ui-common-loop",
    "混合模式": "ui-common-blend-mode",
    "蒙版": "ui-common-mask",
    "不透明": "ui-common-opacity-short",
    "黑体": "ui-common-font-hei",
    "宋体": "ui-common-font-song",
    "楷体": "ui-common-font-kai",
    "仿宋": "ui-common-font-fangsong",
    "等线": "ui-common-font-dengxian",
    "微软雅黑": "ui-common-font-msyh",
    "思源黑体": "ui-common-font-source-han",
    "华文黑体": "ui-common-font-sthei",
}

# 英文精确表(zh 原文 → en;格式串用 Fluent 占位符 { $x } 书写)。
# 句子与易错串;未命中走 PHRASES 分词。遵守 CONTEXT.md 术语表。
EN_EXACT = {
    "单字段可序列化": "single-field serializable",
    "哪个窗口要关闭/聚焦": "which window to close/focus",
    "插件/面板/输入id": "plugin/panel/input id",
    "命令失败:{ $e }": "Command failed: { $e }",
    "界面缩放 { $a1 }%(叠加在系统 DPI 之上;视图 → 界面缩放可调)":
        "UI scale { $a1 }% (stacked on system DPI; adjustable in View → UI Scale)",
    "两处各算一份、参照系还不一样": "counted once in each place, in different reference frames",
    "自己": "itself",
    "分布间距 2 键": "Distribute Spacing (2 keys)",
    "分布间距:已均匀,无需移动": "Distribute spacing: already even, no move needed",
    "对齐(6 键)": "Align (6 keys)",
    "关键对象 = 最后选中者(选中框已加粗)。":
        "Key object = last selected (its bounding box is drawn bold).",
    "圆角仅对盒对象有效(路径/文字对象不支持)":
        "Corner radius applies to box objects only (paths/text unsupported)",
    "冻结块内部不可编辑(样式作用于原样保留的 HTML 片段,无渲染落点)":
        "Freeze blocks are not editable inside (styles apply to the verbatim HTML fragment; no style target)",
    "文字不支持内阴影/内发光(CSS text-shadow 无 inset 语义)":
        "Text does not support inner shadow/inner glow (CSS text-shadow has no inset)",
    "矢量路径的渐变填充暂不支持(SVG defs 未建模,计划 v2);可先用纯色":
        "Gradient fill on vector paths is not yet supported (SVG defs unmodeled, planned v2); use a solid color for now",
    "文字渐变填充暂不支持(计划 v2);可先用纯色":
        "Gradient fill on text is not yet supported (planned v2); use a solid color for now",
    "虚线最多 6 组(3 对值/间隙)": "Dashes support up to 6 groups (3 value/gap pairs)",
    "实时上色在 v1 不支持;可用路径查找器或形状生成器代替":
        "Live Paint is unsupported in v1; use Pathfinder or Shape Builder instead",
    "渐变网格无 HTML 对应;建议用多层径向渐变叠加模拟(计划于 v2 支持)":
        "Gradient mesh has no HTML equivalent; layer several radial gradients to approximate it (planned v2)",
    "图像描摹暂不支持(计划于 v2 支持)": "Image Trace is not yet supported (planned v2)",
    "3D / 透视": "3D / Perspective",
    "3D 与透视网格不支持": "3D and perspective grids are unsupported",
    "符号将于 v2 以『组件』形式提供": "Symbols will arrive in v2 as reusable masters",
    "变量已以『设计令牌』提供:右侧面板坞 · 令牌 Tab(F4 切换)":
        "Variables are provided as design tokens: Tab Dock on the right · Tokens tab (F4 to switch)",
    "圆点": "round dot",
    "未选中对象 —— 选中任意元素后可设描边。":
        "No object selected — select any element to set its stroke.",
    "冻结块:内部不可编辑;描边请编辑其源 HTML。":
        "Freeze block: not editable inside; edit its source HTML for the stroke.",
    "盒对象(border/outline)": "Box object (border/outline)",
    "文字(-webkit-text-stroke)": "Text (-webkit-text-stroke)",
    "矢量路径(stroke 系)": "Vector path (stroke family)",
    "通用描边入口 · 落点:{ $target_name }":
        "General stroke entry · CSS target: { $target_name }",
    "该对象暂无描边条目。": "This object has no stroke entry yet.",
    "+ 为该对象添加描边": "+ Add a stroke to this object",
    "端点仅矢量路径有落点(stroke-linecap);盒对象边框/文字描边无端点语义":
        "Caps only apply to vector paths (stroke-linecap); box borders/text strokes have no cap semantics",
    "边角仅矢量路径有落点(stroke-linejoin);盒对象的边角由圆角(border-radius)决定":
        "Joins only apply to vector paths (stroke-linejoin); box-object corners come from corner radius (border-radius)",
    "隙": "gap",
    "加一组值/间隙": "Add a value/gap pair",
    "删末组": "Delete last group",
    "盒对象:CSS 无自定义虚线,按「有虚线 → dashed」近似;文字描边无虚线":
        "Box objects: CSS has no custom dash pattern; approximated as \"any dash → dashed\"; text strokes cannot be dashed",
    "矢量路径的 SVG 描边恒居中(stroke-align 为 SVG2 草案);文字描边无对齐":
        "SVG strokes on vector paths are always centered (stroke-align is an SVG2 draft); text strokes have no alignment",
    "居中描边:CSS border 恒内侧,已按内侧落盘(与 AI 观感差半线宽;外侧走 outline)":
        "Centered stroke: CSS border is always inside, so it is written as inside (half a stroke-width off AI; use outline for outside)",
    "箭头仅矢量路径可登记(盒/文字无端点)":
        "Arrows can only be attached to vector paths (boxes/text have no caps)",
    "箭头为冻结登记:模型保留但不落盘(CSS/SVG marker 未建模,计划 v2)":
        "Arrows are registered frozen: kept in the model but not written out (CSS/SVG markers unmodeled, planned v2)",
    "未选中对象 —— 选中后在此管理填充/描边/效果条目。":
        "No object selected — select one to manage fill/stroke/effect entries here.",
    "冻结块:内部不可编辑(原样保留的 HTML 片段);可移动/缩放/删除。":
        "Freeze block: not editable inside (verbatim HTML fragment); it can be moved/scaled/deleted.",
    "盒对象": "Box object",
    "{ $target_name } · 条目顺序 = CSS 叠加顺序(首条最上)":
        "{ $target_name } · entry order = CSS stacking order (first entry on top)",
    "删除条目": "Delete entry",
    "复制条目": "Duplicate entry",
    "下移(CSS 中更靠底)": "Move down (further back in CSS)",
    "上移(CSS 中更靠顶)": "Move up (further front in CSS)",
    "暂无条目 —— 从下方添加填充/描边/效果。":
        "No entries yet — add fills/strokes/effects below.",
    "不支持的能力(点击查看说明):": "Unsupported capabilities (click for details):",
    "SVG 滤镜(冻结)": "SVG filter (frozen)",
    "SVG 滤镜原样保留(冻结);v1 不提供编辑器":
        "SVG filters are kept as-is (frozen); v1 provides no editor",
    "SVG 滤镜效果原样保留(冻结);v1 不提供编辑器":
        "SVG filter effects are kept as-is (frozen); v1 provides no editor",
    "上一个效果": "previous effect",
    "色标编辑 → 渐变面板(Ctrl+F9);此处保真往返。":
        "Stop editing → Gradient panel (Ctrl+F9); this area stays faithful round-trip.",
    "原样保真:{ $value }": "Kept as-is: { $value }",
    "快调颜色/粗细;全字段 → 描边面板(Ctrl+F10)":
        "Quick color/width; all fields → Stroke panel (Ctrl+F10)",
    "扩展": "Expand",
    "原样保真:{ $prop }: { $value }": "Kept as-is: { $prop }: { $value }",
    "{ $w } — 已使用默认布局": "{ $w } — default layout applied",
    "就绪 — V 选择 · A 直接选择 · M 矩形 · Alt+拖动 复制 · Shift 约束 · Space 平移 · Ctrl+0 适合":
        "Ready — V select · A direct select · M rect · Alt+drag duplicate · Shift constrain · Space pan · Ctrl+0 fit",
    "本地未保存的标题编辑": "unsaved local title edit",
    "图片": "picture",
    "项目还没有 assets/ 目录 —— 拖入或置入图像后会自动出现。":
        "The project has no assets/ directory yet — it appears after you drop or place an image.",
    "图像置入…": "Place Image…",
    "{ $a1 } 个资产,{ $a2 } 个未被引用(CSS url() 只计数,无可定位节点)":
        "{ $a1 } assets, { $a2 } unreferenced (CSS url() counted only; no locatable node)",
    "(已不存在的节点)": "(node no longer exists)",
    "点击选中该图层(定位到引用)": "Click to select that layer (locate the reference)",
    "└ css url() ×{ $a1 }(样式表引用,无节点可定位)":
        "└ css url() x{ $a1 } (stylesheet reference; no locatable node)",
    "└ 未使用 —— 没有任何 src/href/CSS 引用(可归档或删除)":
        "└ unused — no src/href/CSS reference at all (archive or delete)",
    "资产面板:已定位图层({ $sid })": "Assets panel: located layer ({ $sid })",
    "该图层已不存在(文档可能已变更;点「刷新」更新面板)":
        "That layer no longer exists (the document may have changed; click Refresh to update the panel)",
    "替换图像引用": "Replace image reference",
    "把节点 { $a1 } 的引用 { $a2 } 指向另一个资产:":
        "Point node { $a1 }'s reference { $a2 } to another asset:",
    "项目里没有其他图片类资产可换(先放一张图进 assets/)。":
        "No other image assets in the project to switch to (put an image into assets/ first).",
    "替换走命令层(SetImageSrc,kind 与 attrs 双写同步;Ctrl+Z 可撤销)。":
        "Replacement goes through the command layer (SetImageSrc, kind+attrs written in sync; Ctrl+Z to undo).",
    "已替换图像引用 → { $new_rel }(可撤销)":
        "Replaced image reference → { $new_rel } (undoable)",
    "该对象不带 src 图像引用,无法替换": "This object has no src image reference to replace",
    "文档没有断点:可在「文件 → 文档设置」添加(vb-breakpoints)":
        "No breakpoints in this document: add them in File → Document Settings (vb-breakpoints)",
    "断点预览:画布宽度 → { $w }px(覆盖样式以浏览器校对为准)":
        "Breakpoint preview: canvas width → { $w }px (overrides verified via Browser Proof)",
    "断点预览:回到默认画布": "Breakpoint preview: back to default canvas",
    "属性面板:hover 态编辑(落 selector:hover 规则)":
        "Properties panel: editing :hover state (written as selector:hover rules)",
    "属性面板:正常态编辑": "Properties panel: normal-state editing",
    "旧矩形 ∩ 当前矩形": "old rect ∩ current rect",
    "画板被无辜裁掉": "artboard cropped away wrongly",
    "预览为近似渲染 · 视图 → 浏览器校对可对比":
        "Preview is approximate rendering · compare via View → Browser Proof",
    "替换图像…": "Replace Image…",
    "从对象生成参考线(取对象边)": "Make guides from object (use its edges)",
    "坏了": "broken",
    "GPU 画布初始化失败,画布区空白:{ $e }":
        "GPU canvas init failed; canvas area blank: { $e }",
    "覆盖样式以浏览器校对为准": "overrides verified via Browser Proof",
    "像素预览 { $a1 }×(世界 1px = { $a2 } 物理 px;视图 → 像素预览可关)":
        "Pixel preview { $a1 }x (world 1px = { $a2 } physical px; toggle in View → Pixel Preview)",
    "曲率:请在矢量路径的锚点附近单击(先经钢笔/铅笔建路径)":
        "Curvature: click near an anchor of a vector path (build the path with Pen/Pencil first)",
    "曲率:目标不是矢量路径": "Curvature: target is not a vector path",
    "曲率:路径锚点不足,无法拟合": "Curvature: not enough anchors on the path to fit",
    "曲率:已为 { $a1 } 个锚点拟合平滑控制点(直接选择 A 可微调手柄)":
        "Curvature: fitted smooth control points for { $a1 } anchors (fine-tune handles with Direct Selection A)",
    "铅笔:笔画太短(按住拖动绘制)": "Pencil: stroke too short (press and drag to draw)",
    "铅笔:{ $a1 } 点笔迹 → { $a2 } 锚点路径(保真度 { $a3 }px,编辑 → 设置可调)":
        "Pencil: { $a1 }-point stroke → { $a2 }-anchor path (fidelity { $a3 }px; adjustable in Edit → Settings)",
    "铅笔:笔画无法成路径": "Pencil: stroke could not become a path",
    "度量:「{ $a1 }」 { $a2 } × { $a3 } px(原点 { $a4 },{ $a5 };拖动可量任意两点距离)":
        "Measure: \"{ $a1 }\" { $a2 } x { $a3 } px (origin { $a4 },{ $a5 }; drag to measure any two points)",
    "度量:单击对象标注尺寸,或拖动量两点距离(Esc 退出)":
        "Measure: click an object to label its size, or drag to measure two points (Esc to exit)",
    "度量:距离 { $d }px(ΔX { $dx },ΔY { $dy };Esc 退出度量)":
        "Measure: distance { $d }px (dX { $dx }, dY { $dy }; Esc exits)",
    "已移除渐变(恢复纯色)": "Removed gradient (back to solid)",
    "渐变:先选中对象,再拖动设定方向":
        "Gradient: select an object first, then drag to set the direction",
    "线性渐变已应用 { $angle }°(起=原填充 → 止=#ffffff;双击色标改色;Alt+单击移除)":
        "Linear gradient applied at { $angle }° (start = original fill → end = #ffffff; double-click a stop to recolor; Alt+click to remove)",
    "编辑文本:{ $a1 }(Ctrl+Enter/Esc 提交,再按 Esc 放弃)":
        "Editing text: { $a1 } (Ctrl+Enter/Esc commit, Esc again to discard)",
    "切片:拖框建立切片,或先选中对象再单击(从选区建立)":
        "Slice: drag a rect to create a slice, or select an object then click (slice from selection)",
    "缩放到区域 { $a1 }%": "Zoom to region { $a1 }%",
    "{ $name }中心已设定({ $a1 },{ $a2 }),拖拽对象即围绕它变换":
        "{ $name } center set ({ $a1 },{ $a2 }); dragging the object now transforms around it",
    "先选中对象,再单击设中心 / 拖拽变换":
        "Select an object first, then click to set the center / drag to transform",
    "自由变换:先选中对象": "Free Transform: select an object first",
    "自由变换:拖选区四角之一(对角锚定缩放)":
        "Free Transform: drag one corner of the selection (opposite corner anchors the scale)",
    "旋转 { $a1 }°(Shift 约束 15°)": "Rotated { $a1 }° (Shift constrains to 15°)",
    "自由变换 ×{ $kx }/×{ $ky }": "Free transform x{ $kx }/x{ $ky }",
    "旋转完成(Esc 回选择工具)": "Rotation done (Esc returns to Select)",
    "左右镜像(竖直轴)": "Mirror horizontally (vertical axis)",
    "上下镜像(水平轴)": "Mirror vertically (horizontal axis)",
    "镜像完成:{ $name }(Esc 回选择工具)": "Mirror done: { $name } (Esc returns to Select)",
    "自由变换完成(Esc 回选择工具)": "Free transform done (Esc returns to Select)",
    "    命令:{ $ids }": "    commands: { $ids }",
    "剪切蒙版:选中「内容 + 形状」(形状最后选)后再按 Ctrl+7":
        "Clipping mask: select \"content + shape\" (shape last), then press Ctrl+7",
    "剪切蒙版:找不到蒙版对象 { $mask_sid }":
        "Clipping mask: mask object { $mask_sid } not found",
    "剪切蒙版:蒙版形状必须是矩形/椭圆盒(顶层内容)":
        "Clipping mask: the mask shape must be a rect/ellipse box (topmost)",
    "剪切蒙版:取不到蒙版几何": "Clipping mask: cannot read mask geometry",
    "剪切蒙版:找不到内容对象 { $sid }": "Clipping mask: content object { $sid } not found",
    "剪切蒙版:内容与蒙版不能互为祖先/后代":
        "Clipping mask: content and mask cannot be ancestor/descendant of each other",
    "剪切蒙版:取不到内容几何": "Clipping mask: cannot read content geometry",
    "剪切蒙版:没有可收编的内容对象": "Clipping mask: no content objects to adopt",
    "释放剪切蒙版:找不到 { $sid }": "Release clipping mask: { $sid } not found",
    "释放剪切蒙版:选中对象不是剪切蒙版容器":
        "Release clipping mask: the selection is not a mask container",
    "释放剪切蒙版:蒙版没有父级": "Release clipping mask: the mask has no parent",
    "冻结对象(原样片段)不可改色": "Frozen objects (verbatim fragments) cannot be recolored",
    "该对象没有填充色可交换": "This object has no fill color to swap",
    "该对象没有描边色可交换": "This object has no stroke color to swap",
    "颜色面板:作用于描边(X 切回填充)": "Color panel: editing stroke (X switches back to fill)",
    "颜色面板:作用于填充(X 切到描边)": "Color panel: editing fill (X switches to stroke)",
    "未选中对象 —— 选中后调整填充/描边色与全局色板。":
        "No object selected — select one to adjust fill/stroke colors and the global palette.",
    "X 切换目标": "X switches target",
    "D:填充白 / 描边黑": "D: white fill / black stroke",
    "该对象没有描边色 —— 可在描边面板(^F10)添加描边。":
        "This object has no stroke color — add one in the Stroke panel (^F10).",
    "该对象没有填充色 —— 可在外观面板(⇧F6)添加填充。":
        "This object has no fill — add one in the Appearance panel (Shift+F6).",
    "CMYK 仅作输入换算(CSS 只有 sRGB);显示为近似值。":
        "CMYK is input-converted only (CSS has sRGB only); shown as approximate values.",
    "还没有全局色 —— 在下方色板里新建。": "No global colors yet — create one in the swatches below.",
    "点全局色即写入 var(--name):改令牌值全站生效(令牌 Tab F4)。":
        "Clicking a global color writes var(--name): editing the token value applies site-wide (Tokens tab F4).",
    "当前颜色 `{ $text }` 不可解析(可能是变量或复杂函数),已在色板区展示。":
        "Current color `{ $text }` cannot be parsed (variable or complex function); shown in the swatch area.",
    "把当前填充色登记为全局色(--vb-color-N)":
        "Register the current fill as a global color (--vb-color-N)",
    "全局色 = CSS 变量(--vb-color-N),存于文档令牌,改动全站生效。":
        "Global colors = CSS variables (--vb-color-N) stored in document tokens; edits apply site-wide.",
    "未选中对象 —— 先选中对象再应用全局色":
        "No object selected — select one before applying a global color",
    "从页面提取颜色:依赖 WPI color_profiler,当前未落地 —— 暂不提供按钮(计划 v2)。":
        "Extract colors from page: needs WPI color_profiler, not landed yet — button withheld for now (planned v2).",
    "至少保留一块画板": "at least one artboard must remain",
    "取消编组:选中对象里没有编组": "Ungroup: no group in the selection",
    "剪贴板:未选中对象": "Clipboard: no object selected",
    "剪贴板为空": "Clipboard is empty",
    "(就地)": "(in place)",
    "路径查找器:只支持矢量路径(钢笔创建的形状)":
        "Pathfinder: vector paths only (shapes created with the Pen)",
    "路径查找器:{ $a1 }:产出 { $a2 } 个对象": "Pathfinder: { $a1 }: produced { $a2 } object(s)",
    "对齐:找不到目标框": "Align: target box not found",
    "对齐:未知模式 { $mode }": "Align: unknown mode { $mode }",
    "对齐:无需移动": "Align: nothing to move",
    "没有可再次的变换(先移动/缩放/旋转一次)":
        "Nothing to transform again (move/scale/rotate once first)",
    "再次变换(Ctrl+D):{ $a1 } 个对象(位移 { $a2 },{ $a3 } · 缩放 ×{ $a4 }/×{ $a5 } · 旋转 { $a6 }°)":
        "Transform Again (Ctrl+D): { $a1 } object(s) (offset { $a2 },{ $a3 } · scale x{ $a4 }/x{ $a5 } · rotate { $a6 }°)",
    "文本 { $a1 }": "text { $a1 }",
    "已创建区域文本(拖框宽度即换行宽度)":
        "Area text created (the dragged width becomes the wrap width)",
    "已创建点文本(输入内容,Ctrl+Enter 提交)":
        "Point text created (type content; Ctrl+Enter to commit)",
    "切片 { $count }": "slice { $count }",
    "已建立切片「{ $name }」({ $a1 }×{ $a2 },Shift+K 拖框可再建;vellum-cli export --slice 按名出图)":
        "Slice \"{ $name }\" created ({ $a1 }x{ $a2 }; Shift+K drag to add more; vellum-cli export --slice exports by name)",
    "切片:未选中对象": "Slice: no object selected",
    "吸管:未命中对象": "Eyedropper: no object hit",
    "吸管:目标没有样式可吸取": "Eyedropper: target has no styles to pick",
    "已吸取 { $a1 } 条样式(先选中对象再点应用)":
        "Picked { $a1 } style declaration(s) (select an object, then click Apply)",
    "已应用全部样式({ $a1 } 条声明)": "Applied all styles ({ $a1 } declaration(s))",
    "吸管:目标没有填充色(Alt 可吸全部样式)":
        "Eyedropper: target has no fill (Alt picks all styles)",
    "已取色 { $hex }(先选中对象再点应用)":
        "Picked { $hex } (select an object, then click Apply)",
    "剪刀:请在矢量路径的锚点上单击": "Scissors: click on an anchor of the vector path",
    "剪刀:已剪开(路径开放,填充按隐式闭合渲染)":
        "Scissors: cut open (path now open; fill renders as implicitly closed)",
    "剪刀:锚点已在路径端点,无需剪": "Scissors: anchor is already an endpoint; nothing to cut",
    "剪刀:已剪开为两段(两段均已选中)": "Scissors: cut into two segments (both selected)",
    "{ $a1 }:不可读/缺失": "{ $a1 }: unreadable/missing",
    "内存当前态": "memory current state",
    "(一方缺失,无法做双方 diff;缺失方:{ $a1 })":
        "(one side missing; cannot diff; missing side: { $a1 })",
    "磁盘上的 index.html 已被外部修改,而本地有未保存编辑(未自动采用)。":
        "index.html on disk was modified externally while local edits are unsaved (not auto-adopted).",
    "统一视图;差异 { $changed } 行。「−」= 前者独有,「+」= 后者独有。":
        "Unified view; { $changed } differing lines. \"−\" = only in the former, \"+\" = only in the latter.",
    "以磁盘为准重载(放弃本地编辑)": "Reload from disk (discard local edits)",
    "以内存为准存回(覆盖磁盘)": "Save memory over disk (overwrite)",
    "先不动": "Keep as-is",
    "快照列为最近一次自动保存(.vb-autosave/);项目没有快照时该方显示缺失。":
        "The snapshot column is the latest autosave (.vb-autosave/); shows missing when the project has none.",
    "已放弃本地编辑,采用磁盘版本(画板 { $n })":
        "Discarded local edits; using the disk version (artboard { $n })",
    "重载磁盘版本失败:{ $e }": "Failed to reload disk version: { $e }",
    "已以内存版本写回磁盘(外部改动被覆盖)":
        "Memory version written back to disk (external changes overwritten)",
    "面板上每个控件真的写文档": "every control on the panel actually writes the document",
    "选中矢量路径后可改锚点坐标": "Select a vector path to edit anchor coordinates",
    "点了没反应": "clicked but nothing happened",
    "先选中对象(按 V 点选 · M 拖框创建 · 双击文字进入编辑)":
        "Select an object (V click-select · M drag-create · double-click text to edit)",
    "共 { $a1 } 块": "{ $a1 } in total",
    "放大一档": "zoom in one step",
    "缩小一档": "zoom out one step",
    "本产品独有": "unique to this product",
    "命令路径": "command path",
    "拖画布锚点改位;手柄/转换点 → 阶段 2":
        "Drag canvas anchors to move; handles/conversion points → phase 2",
    "点击落锚点 · 点击起点或 Enter 自动闭合":
        "Click to place anchors · click the start point or press Enter to close",
    "色标编辑 → 阶段 4(05 外观/渐变)": "Stop editing → phase 4 (05 appearance/gradient)",
    "适配内容 → 阶段 2(画板面板增强)": "Fit content → phase 2 (Artboards panel enhancement)",
    "抓手:拖动平移(Space 同) · 缩放:单击放大 / Alt+单击缩小 / 拖框 · Ctrl+0 适合窗口":
        "Hand: drag to pan (Space same) · Zoom: click in / Alt+click out / drag rect · Ctrl+0 fit",
    "吸管:单击对象取色应用到选区 · Alt+单击吸取全部样式":
        "Eyedropper: click an object to apply its color to the selection · Alt+click picks all styles",
    "剪刀:在矢量路径的锚点上单击剪开(闭路开口 / 开路分段)":
        "Scissors: click an anchor on a vector path to cut (open closed paths / split open ones)",
    "变换工具:单击画布点设定中心 → 拖拽对象按工具语义变换 · Shift 15°/等比 · Alt 从对象中心 · Esc 回选择":
        "Transform tools: click a canvas point to set the center → drag objects to transform · Shift 15°/proportional · Alt from object center · Esc back to Select",
    "铅笔:按住拖动自由绘制,松手按保真度容差抽稀为矢量路径(编辑 → 设置 → 铅笔保真度)":
        "Pencil: hold and drag to draw freehand; on release the stroke is thinned into a vector path by fidelity tolerance (Edit → Settings → Pencil Fidelity)",
    "曲率:在矢量路径锚点附近单击,自动拟合平滑控制点(直接选择 A 可微调)":
        "Curvature: click near a vector path anchor to auto-fit smooth control points (fine-tune with Direct Selection A)",
    "切片:拖框建立 data-vb-slice 切片;选中对象后单击 = 从选区建立;vellum-cli export --slice 按名出图":
        "Slice: drag to create data-vb-slice slices; with a selection, click = slice from selection; vellum-cli export --slice exports by name",
    "度量:拖动量两点距离;单击对象标注尺寸;Esc 退出":
        "Measure: drag to measure two points; click an object to label its size; Esc to exit",
    "盒子": "box", "盒子2": "box2", "主标题": "main title",
    "aria-label=\"主标题\"": "aria-label=\"main title\"",
    "英雄区": "hero section", "产品官网": "product site",
    "data-vb-name=\"英雄区\"": "data-vb-name=\"hero section\"",
    "<title>产品官网</title>": "<title>product site</title>",
    "W+H 应合并为一条 SetGeom": "W+H should merge into one SetGeom",
    "只改一个": "only one changed",
    "外部改动信息": "external change info",
    "同族工具弹层": "same-family tool popover",
    "关于 Vellum Bench": "About Vellum Bench",
    "Vellum Bench v{ $a1 } · 绘台": "Vellum Bench v{ $a1 } · workbench",
    "用 Illustrator 的操作心智,编辑标准 HTML/CSS 文档。":
        "Illustrator-style interactions for editing standard HTML/CSS documents.",
    "HTML 是文档格式,不是编译产物。": "HTML is the document format, not a build artifact.",
    "文本已提交(Ctrl+Enter 提交 · Esc 提交 · 再按 Esc 放弃)":
        "Text committed (Ctrl+Enter commit · Esc commit · Esc again to discard)",
    "透明背景仅 PNG 导出支持": "Transparent background is PNG-export only",
    "目标:当前画板({ $a1 })": "Target: current artboard ({ $a1 })",
    "搜索命令…": "Search commands…",
    "没有匹配的命令": "No matching commands",
    "已有导出任务在后台进行,完成后再试":
        "An export is already running in the background; try again when it finishes",
    "先保存项目(选一个目录)再导出":
        "Save the project (choose a directory) before exporting",
    "WPI 回退不可用:请设置 VB_WPI_DIR 指向 WPI 仓库":
        "WPI fallback unavailable: set VB_WPI_DIR to the WPI repository",
    "WPI 回退导出 { $a1 }({ $a2 } KB)": "WPI fallback export { $a1 } ({ $a2 } KB)",
    "WPI 回退导出失败:{ $e }": "WPI fallback export failed: { $e }",
    "写文件失败:{ $e }": "Failed to write file: { $e }",
    "导出中…(后台执行,完成见状态栏)":
        "Exporting… (runs in background; see the status bar when done)",
    "导出线程启动失败:{ $e }": "Failed to start export thread: { $e }",
    "导出线程异常退出(panic),见终端日志":
        "Export thread exited abnormally (panic); see the terminal log",
    "多给几条候选": "offer more candidates",
    "自由变换": "Free Transform", "统计": "Stats", "键位": "Key binding",
    "缩到两条击键": "down to two keystrokes",
    "执行此命令": "Run this command",
    ";{ $a1 } 条近似警告": "; { $a1 } approximation warning(s)",
    "拖拽进行中:先松手或 Esc 取消":
        "Drag in progress: release the mouse or press Esc to cancel",
    "命令未实现:{ $id }": "Command not implemented: { $id }",
    "打开项目目录(含 index.html)": "Open project directory (containing index.html)",
    "当前不是多窗口模式,无法关闭窗口":
        "Not in multi-window mode; cannot close windows",
    "当前不是多窗口模式,无主页": "Not in multi-window mode; no Launcher",
    "文档设置:项目名 / 输出模式 / 网格与参考线(应用后生效)":
        "Document Settings: project name / output mode / grid & guides (takes effect after Apply)",
    "外部冲突对比:磁盘 / 内存 / 自动快照 三方差异与处置动作":
        "External conflict compare: disk / memory / auto-snapshot diff and resolution actions",
    "当前没有待处理的外部冲突(磁盘与内存一致,或本会话无外部改动)":
        "No pending external conflicts (disk and memory agree, or no external changes this session)",
    "没有可撤销/重做的操作": "Nothing to undo/redo",
    "首选项:常规 / 文字 / 单位与标尺 / 参考线与网格 / 智能参考线 / 画板 / 性能 / 外观 / 数据":
        "Preferences: General / Text / Units & Rulers / Guides & Grid / Smart Guides / Artboards / Performance / Appearance / Data",
    "键盘快捷键:命令列表 + 冲突检测 + 录制新键(方案存 keymap.json)":
        "Keyboard Shortcuts: command list + conflict detection + record new keys (scheme stored in keymap.json)",
    "已提交旧宿主执行": "Submitted to the legacy host for execution",
    "未知命令": "Unknown command", "未知命令:{ $id }": "Unknown command: { $id }",
    "已锁定所选": "Locked the selection",
    "没有已锁定的对象": "No locked objects",
    "已隐藏所选": "Hid the selection",
    "没有已隐藏的对象": "No hidden objects",
    "已放弃文本修改(未入撤销栈)": "Discarded text edits (not pushed to the undo stack)",
    "钢笔:路径已结束(开放)": "Pen: path already finished (open)",
    "已取消(未入撤销栈)": "Cancelled (not pushed to the undo stack)",
    "已回到选择工具(Esc 退出工具态)": "Back to Select tool (Esc exits tool mode)",
    "钢笔:路径已结束": "Pen: path already finished",
    "正在退出…": "Quitting…",
    "轮廓模式:开(Mod+Y)": "Outline mode: on (Mod+Y)",
    "轮廓模式:关(Mod+Y)": "Outline mode: off (Mod+Y)",
    "像素预览:开(缩放 ≥{ $a1 }× 时对齐物理像素网格并显示边界)":
        "Pixel preview: on (aligns to the physical pixel grid and shows boundaries at zoom ≥{ $a1 }x)",
    "像素预览:关": "Pixel preview: off",
    "已隐藏所有面板(Tab 恢复)": "Hid all panels (Tab to restore)",
    "已恢复所有面板(Tab 再隐藏)": "Restored all panels (Tab hides again)",
    "未绑定": "unbound",
    "从选区生成 { $added } 条参考线({ $key })":
        "Created { $added } guide(s) from the selection ({ $key })",
    "外观面板:显示(⇧F6 关闭)": "Appearance panel: shown (Shift+F6 closes)",
    "外观面板:隐藏(⇧F6 显示)": "Appearance panel: hidden (Shift+F6 shows)",
    "透明度面板:显示(⇧^F10 关闭)": "Opacity panel: shown (Shift+^F10 closes)",
    "透明度面板:隐藏(⇧^F10 显示)": "Opacity panel: hidden (Shift+^F10 shows)",
    "能力台账:显示(再点关闭)": "Capability Ledger: shown (click again to close)",
    "变换面板:显示(⇧F8 关闭)": "Transform panel: shown (Shift+F8 closes)",
    "变换面板:隐藏(⇧F8 显示)": "Transform panel: hidden (Shift+F8 shows)",
    "对齐面板:显示(⇧F7 关闭)": "Align panel: shown (Shift+F7 closes)",
    "对齐面板:隐藏(⇧F7 显示)": "Align panel: hidden (Shift+F7 shows)",
    "开发者统计:显示(帧率/帧时间/节点/显卡只在此可见)":
        "Developer Stats: shown (FPS/tick time/nodes/GPU visible only here)",
    "界面动效:开(对话框/面板淡入、悬停过渡)":
        "UI motion: on (dialog/panel fade-ins, hover transitions)",
    "界面动效:关(所有过渡立即到位;视图菜单或首选项可再开)":
        "UI motion: off (all transitions jump to the end; re-enable in View menu or Preferences)",
    "界面缩放 100%(跟随系统 DPI)": "UI scale 100% (follows system DPI)",
    "未支持工具:显示(置灰,点击见计划版本)":
        "Unsupported tools: shown (greyed out; click for the planned version)",
    "未支持工具:隐藏(工具箱保持整洁)": "Unsupported tools: hidden (keeps the Toolbar tidy)",
    "历史面板:显示(点击历史项可跳转;回退遇重做尾需确认)":
        "History panel: shown (click an entry to jump; jumping past a redo tail asks for confirmation)",
    "资产面板:显示(点击引用可定位图层;支持替换引用)":
        "Assets panel: shown (click a reference to locate the layer; supports replacing references)",
    "时间轴面板:显示(选中对象 → 双击轨道加关键帧 → 播放预览)":
        "Timeline panel: shown (select an object → double-click a track to add keyframes → play preview)",
    "时间轴面板:隐藏(预览若在播放将继续)":
        "Timeline panel: hidden (a playing preview keeps playing)",
    "插件管理:已打开(插件 = 外部进程,默认零权限,首次启用需授权)":
        "Plugin Manager: opened (plugins = external processes, zero permissions by default, first enable requires authorization)",
    "插件面板:显示(Running 插件的注册面板;按钮点击回发插件通知)":
        "Plugin panel: shown (registered panels of Running plugins; button clicks are sent back as plugin notifications)",
    "文字模式 → { $pending }(待用 + { $n } 个选中文本对象已转换)":
        "Text mode → { $pending } (pending + { $n } selected text object(s) converted)",
    "文字模式 → { $pending }(下次新建生效)":
        "Text mode → { $pending } (applies to newly created text)",
    "项目名(= 标题)": "Project name (= title)",
    "单文件(CSS 内联)": "Single file (CSS inlined)",
    "h=水平线 y,v=垂直线 x,逗号分隔(如 h0,v120)":
        "h=horizontal line y, v=vertical line x, comma-separated (e.g. h0,v120)",
    "px 逗号分隔(如 375,750,1080;空 = 无)":
        "px, comma-separated (e.g. 375,750,1080; empty = none)",
    "网格与参考线存项目级(index.html 的 vb-grid / vb-guides meta),随文件走;保存后生效。":
        "Grid & guides are stored per project (vb-grid / vb-guides meta in index.html) and travel with the file; takes effect on save.",
    "画板尺寸在「画板」面板逐块调整;默认预设见首选项「画板」页。":
        "Adjust each artboard's size in the Artboards panel; default presets are in Preferences → Artboards.",
    "断点:{ $a1 } 项中有 { $a2 } 项无效已忽略(px 正整数)":
        "Breakpoints: { $a2 } of { $a1 } entries invalid and ignored (positive integers, px)",
    "文档设置已应用(Ctrl+S 或自动保存写盘)":
        "Document settings applied (Ctrl+S or autosave writes to disk)",
    "workspace.json 解析失败,已回退默认布局:{ $e }":
        "workspace.json failed to parse; fell back to the default layout: { $e }",
    "workspace.json v1 迁移失败,已回退默认布局:{ $e }":
        "workspace.json v1 migration failed; fell back to the default layout: { $e }",
    "workspace.json 版本 { $version } 与当前 { $SCHEMA_VERSION } 不符,已回退默认布局":
        "workspace.json version { $version } does not match current { $SCHEMA_VERSION }; fell back to the default layout",
    "找不到配置目录,本次布局不会持久化(可用 VB_WORKSPACE 指定)":
        "Config directory not found; this layout will not persist (set VB_WORKSPACE to choose one)",
    "找不到配置目录,窗口布局不会持久化":
        "Config directory not found; window layout will not persist",
    "窗口布局文件 { $a1 } 解析失败,已回退默认布局:{ $e }":
        "Window layout file { $a1 } failed to parse; fell back to the default layout: { $e }",
    "找不到配置目录,窗口布局未持久化":
        "Config directory not found; window layout not persisted",
    "创建配置目录失败:{ $e }": "Failed to create config directory: { $e }",
    "序列化窗口布局失败:{ $e }": "Failed to serialize window layout: { $e }",
    "写窗口布局失败:{ $e }": "Failed to write window layout: { $e }",
    "提交窗口布局失败:{ $e }": "Failed to commit window layout: { $e }",
    "序列化工作区配置失败:{ $e }": "Failed to serialize workspace config: { $e }",
    "写工作区配置失败:{ $e }": "Failed to write workspace config: { $e }",
    "提交工作区配置失败:{ $e }": "Failed to commit workspace config: { $e }",
    "找不到配置目录,布局未持久化": "Config directory not found; layout not persisted",
    "…等 N 个": "… and N more",
    "检测到外部修改,已自动采用(Agent 热重载,{ $n } 画板)":
        "External changes detected and auto-adopted (Agent hot reload, { $n } artboard(s))",
    "热重载失败:{ $e }": "Hot reload failed: { $e }",
    "检测到磁盘修改,但本地有未保存编辑(未自动采用;先 Ctrl+S 或撤销)":
        "Disk changes detected, but local edits are unsaved (not auto-adopted; Ctrl+S or undo first)",
    "检测到 { $a1 } 种文档使用的字体不在本机可用集合内(判定口径:通用族 + 随包/系统回退 + 常见系统字体;非逐字体枚举)。":
        "{ $a1 } font(s) used by the document are not in the locally available set (heuristic: generic families + bundled/system fallback + common system fonts; not a per-font enumeration).",
    "全部跳过": "Skip All",
    "全部替换(各自首选候选)": "Replace All (with each one's top candidate)",
    "先装者更靠外": "first-installed renders further out",
    "已适合窗口:{ $a1 }%(打开项目自动适配)":
        "Fit to window: { $a1 }% (projects auto-fit on open)",
    "文字对象的渐变填充计划于 v2(CSS 无文字渐变);可先改纯色":
        "Gradient fill for text is planned for v2 (CSS has no text gradient); use a solid color for now",
    "矢量路径的渐变填充计划于 v2(SVG defs 未建模);可先改纯色":
        "Gradient fill for vector paths is planned for v2 (SVG defs unmodeled); use a solid color for now",
    "冻结对象(原样片段)不可编辑渐变": "Frozen objects (verbatim fragments) cannot have gradients edited",
    "已选中渐变色标 { $a1 }/{ $a2 } —— 在渐变面板改颜色/位置":
        "Gradient stop { $a1 }/{ $a2 } selected — edit color/position in the Gradient panel",
    "未选中对象 —— 选中一个盒对象后可编辑其渐变。":
        "No object selected — select a box object to edit its gradient.",
    "文字对象:文字渐变计划于 v2(可先用字符面板改字色)。":
        "Text object: text gradients are planned for v2 (change the text color in the Character panel for now).",
    "矢量路径:渐变填充计划于 v2(SVG defs 未建模)。":
        "Vector path: gradient fill is planned for v2 (SVG defs unmodeled).",
    "冻结对象(原样片段)不可编辑渐变。":
        "Frozen objects (verbatim fragments) cannot have gradients edited.",
    "该对象还没有渐变 —— 点下方「生成渐变」":
        "This object has no gradient yet — click \"Generate Gradient\" below",
    "该对象当前没有渐变。": "This object currently has no gradient.",
    "用当前填充生成线性渐变": "Generate a linear gradient from the current fill",
    "色标 = 现填充色 0% → 白色 100%(与画布拖动同一落点)":
        "Stops = current fill 0% → white 100% (same CSS target as canvas dragging)",
    "双击色标:在下方改颜色/位置/不透明度":
        "Double-click a stop: edit color/position/opacity below",
    "拖动圆点改位置 · 点空白加点 · Alt+点击删点 · 拖菱形改中点 · 双击选中改色":
        "Drag the dot to move · click empty space to add · Alt+click to delete · drag the diamond for the midpoint · double-click to select and recolor",
    "＋ 加点": "+ Add stop",
    "在选中色标与下一个之间插入中点色":
        "Insert a midpoint color between the selected stop and the next",
    "渐变至少保留两个色标": "A gradient keeps at least two stops",
    "线性 = 角度 +180°;径向 = 色标镜像": "Linear = angle +180°; radial = mirrored stops",
    "项目相对路径解析": "project-relative path resolution",
    "图片「{ $a1 }」({ $a2 })缺 alt 属性 —— 加 alt 说明内容;纯装饰图给空 alt=\"\"":
        "Image \"{ $a1 }\" ({ $a2 }) is missing the alt attribute — add alt describing the content; use empty alt=\"\" for purely decorative images",
    "交互元素「{ $a1 }」({ $a2 })无可访问名称 —— 加 aria-label 或可见文本":
        "Interactive element \"{ $a1 }\" ({ $a2 }) has no accessible name — add aria-label or visible text",
    "文本「{ $a1 }」({ $a2 })对比度 { $a3 }:1,低于 WCAG AA 建议 ≥{ $threshold }:1(按节点级样式粗判,仅提示)":
        "Text \"{ $a1 }\" ({ $a2 }) contrast { $a3 }:1 is below the WCAG AA suggestion of ≥{ $threshold }:1 (rough node-level style check, advisory only)",
    "{ $attr } 指向的「{ $rel }」不存在(节点 { $sid })":
        "{ $attr } points to \"{ $rel }\" which does not exist (node { $sid })",
    "冻结块「{ $a1 }」({ $a2 })—— 内部不可编辑,样式由原样 HTML 承载":
        "Freeze block \"{ $a1 }\" ({ $a2 }) — not editable inside; styling is carried by the verbatim HTML",
    "assets/ 里的「{ $a1 }」没有被任何引用(可归档或删除)":
        "\"{ $a1 }\" in assets/ has no references (archive or delete)",
    "{ $name } 有 { $a1 } MB(阈值 { $a2 } MB)—— 影响打开/导出速度":
        "{ $name } is { $a1 } MB (threshold { $a2 } MB) — slows down open/export",
    "{ $name } 第 { $a1 } 行超长({ $a2 } 字符 > { $MAX_LINE_CHARS })—— 多半是内联大图/压缩产物":
        "{ $name } line { $a1 } is too long ({ $a2 } chars > { $MAX_LINE_CHARS }) — usually an inlined image or compressed artifact",
    "未发现问题(缺失资源 / 失效链接 / 冻结块 / 未使用资产 / 超长文件 / 无障碍 全部通过)。":
        "No issues found (missing assets / broken links / freeze blocks / unused assets / oversize files / accessibility all pass).",
    "发现 { $a1 } 项(冻结块与无障碍为提示项,其余建议处理):":
        "Found { $a1 } item(s) (freeze blocks and accessibility are advisory; the rest are recommended):",
    "健康检查:已定位图层({ $sid })": "Health check: located layer ({ $sid })",
    "该图层已不存在(文档可能已变更)": "That layer no longer exists (the document may have changed)",
    "没有可撤销的操作": "Nothing to undo",
    "没有可撤销的操作(先做一次编辑)": "Nothing to undo (make an edit first)",
    "没有可重做的操作": "Nothing to redo",
    "没有可重做的操作(先撤销一步)": "Nothing to redo (undo a step first)",
    "{ $a1 } 步 / 待重做 { $a2 }": "{ $a1 } step(s) / { $a2 } to redo",
    "还没有可撤销的操作 —— 画一笔就有了。":
        "Nothing to undo yet — draw a stroke and there will be.",
    "跳转历史?": "Jump in history?",
    "跳转到该状态将丢弃其后的 { $redo_n } 步重做记录(不可恢复)。":
        "Jumping to this state discards the { $redo_n } redo step(s) after it (irreversible).",
    "跳转并丢弃": "Jump and discard",
    "历史跳转中断:{ $e }": "History jump interrupted: { $e }",
    "已回退 { $n } 步(深度 { $depth })": "Undone { $n } step(s) (depth { $depth })",
    "读选区 → exec → 反馈": "read selection → exec → feedback",
    "已建立剪切蒙版(overflow 容器;Ctrl+Alt+7 释放,Ctrl+Z 撤销)":
        "Clipping mask created (overflow container; Ctrl+Alt+7 releases, Ctrl+Z undoes)",
    "释放剪切蒙版:先选中蒙版容器": "Release clipping mask: select the mask container first",
    "已释放剪切蒙版(内容回到原位置)": "Clipping mask released (content back to its original position)",
    "置入图像": "Place Image",
    "置入图像:先保存或打开一个项目(图像进入 assets/ 目录)":
        "Place Image: save or open a project first (images go into assets/)",
    "置入图像失败:{ $e }": "Place Image failed: { $e }",
    "已置入并替换图像引用:{ $rel }": "Placed and replaced the image reference: { $rel }",
    "已置入图像:{ $rel }(新 img 节点,原图尺寸)":
        "Placed image: { $rel } (new img node, original size)",
    "置入图像:无法读取图片尺寸": "Place Image: cannot read the image dimensions",
    "替换图像:先选中一个图像对象": "Replace Image: select an image object first",
    "替换图像:选中对象不是图像": "Replace Image: the selection is not an image",
    "替换图像(保持几何)": "Replace Image (keep geometry)",
    "替换图像:先保存或打开一个项目(图像进入 assets/ 目录)":
        "Replace Image: save or open a project first (images go into assets/)",
    "已替换图像引用:{ $rel }(几何不变)": "Replaced image reference: { $rel } (geometry unchanged)",
    "替换图像失败:{ $e }": "Replace Image failed: { $e }",
    "铅笔保真度:{ $next }px(容差越大笔迹越简洁;下一档继续点)":
        "Pencil fidelity: { $next }px (larger tolerance = simpler strokes; click again for the next level)",
    "路径没有文件名": "path has no file name",
    "建 assets/ 失败:{ $e }": "Failed to create assets/: { $e }",
    "图像 { $a1 }": "image { $a1 }",
    "键盘快捷键": "Keyboard Shortcuts", "分组": "Group",
    "命令名 / id…": "Command name / id…",
    "正在录制「{ $label }」:请按下新的组合键(Esc 取消)":
        "Recording \"{ $label }\": press the new key combo (Esc to cancel)",
    "恢复默认方案": "Restore Default Scheme",
    "方案文件:{ $a1 }": "Scheme file: { $a1 }",
    "方案文件:不可用(未找到配置目录)": "Scheme file: unavailable (config directory not found)",
    "无法识别组合键「{ $combo }」": "Cannot recognize the key combo \"{ $combo }\"",
    "已拒绝:「{ $combo }」已被 { $a1 } 使用(同一组合键只能绑定一个命令)":
        "Rejected: \"{ $combo }\" is already used by { $a1 } (one combo binds one command)",
    "键位已更新:{ $label } → { $a1 }": "Key binding updated: { $label } → { $a1 }",
    "「{ $label }」已还原默认键位({ $a1 })":
        "\"{ $label }\" restored to the default binding ({ $a1 })",
    "无绑定": "no binding",
    "键位方案已恢复默认(所有自定义覆盖已清除)":
        "Keymap restored to defaults (all custom overrides cleared)",
    "文本对象": "text object", "相同": "same",
    "查找字体:文档没有缺失字体(判定口径见对话框说明)":
        "Find Fonts: the document has no missing fonts (see the dialog for the criteria)",
    "查找字体:发现缺失字体,已打开替换对话框":
        "Find Fonts: missing fonts found; opened the substitution dialog",
    "更改大小写:选中对象里没有可改的文本":
        "Change Case: no editable text in the selection",
    "已更改 { $n } 个文本对象的大小写": "Changed the letter case of { $n } text object(s)",
    "选择:当前画板没有可选对象": "Select: no selectable objects on the current artboard",
    "选择相同:先选中一个参照对象": "Select Same: select a reference object first",
    "选择相同:参照对象没有该属性": "Select Same: the reference object lacks that attribute",
    "应用上一个效果:还没有可重复的效果(先用「效果」菜单添加一个)":
        "Apply Last Effect: no repeatable effect yet (add one via the Effect menu first)",
    "工作区:可保存当前布局为命名预设,并可切换/删除":
        "Workspaces: save the current layout as a named preset; switch/delete anytime",
    "工作区:基本功能(工具箱在左 + 属性面板)":
        "Workspace: Basics (Toolbar left + Properties panel)",
    "工作区:排版(工具箱在左 + 字符/段落面板停靠)":
        "Workspace: Typography (Toolbar left + Character/Paragraph panels docked)",
    "效果:先选中一个对象": "Effect: select an object first",
    "已为 { $ok } 个对象添加效果": "Added the effect to { $ok } object(s)",
    "preset:<名>": "preset:<name>",
    "会撤销什么": "what will be undone",
    "画布为空,已重建默认画板": "Canvas was empty; rebuilt the default artboard",
    "组内相互挖空": "knock out each other inside the group",
    "盒类": "box-like",
    "该对象还没有蒙版": "This object has no mask yet",
    "该蒙版不是可反转的渐变蒙版(原样保留)":
        "This mask is not an invertible gradient mask (kept as-is)",
    "未选中对象 —— 选中后调整不透明度 / 混合 / 蒙版。":
        "No object selected — select one to adjust opacity / blend / mask.",
    "混合模式对文字/矢量对象落 CSS 无独立语义(随父层生效)。":
        "Blend mode has no standalone CSS semantics for text/vector objects (takes effect via the parent layer).",
    "CSS 落点:isolation: isolate(近似;组内相互挖空的渲染级还原计划 v2)":
        "CSS target: isolation: isolate (approximate; render-level knockout-group restoration planned v2)",
    "写入 mask-image 渐变(黑→透明);顶部对象作蒙版的黑白稿未建模":
        "Writes a mask-image gradient (black→transparent); the top-object-as-mask luminance model is not built",
    "释放蒙版": "Release Mask",
    "蒙版以 mask-image 渐变表示;mask-repeat/position 等不在白名单(计划 v2)。":
        "The mask is expressed as a mask-image gradient; mask-repeat/position etc. are outside the whitelist (planned v2).",
    "填充条目级混合:{ $b }(逐层 background-blend-mode,外观面板可改)。":
        "Per-entry fill blending: { $b } (layer-by-layer background-blend-mode; editable in the Appearance panel).",
    "变换组或独立历史组": "transform group or separate history group",
    "{ $a1 }(点击展开并切换)": "{ $a1 } (click to expand and switch)",
    "{ $a1 }(窗口过窄,仅切换)": "{ $a1 } (window too narrow; switch only)",
    "该组面板均已关闭 —— 用「窗口」菜单或快捷键打开(如 ⇧F6 外观、Ctrl+T 字符)。":
        "All panels in this group are closed — open one from the Window menu or a shortcut (e.g. Shift+F6 Appearance, Ctrl+T Character).",
    "该面板当前是浮窗;点击停靠回面板坞":
        "This panel is currently a floating window; click to dock it back into the Tab Dock",
    "把该面板改为浮窗(位置自动排布,不级联)":
        "Turn this panel into a floating window (auto-positioned, no cascade)",
    "移动 750×1334": "Mobile 750x1334", "移动 375×667": "Mobile 375x667",
    "{ $a1 } 副本": "{ $a1 } duplicate(s)",
    "画板已复制(纵向落到最下方)": "Artboard duplicated (placed below the last one)",
    "复制:先选中一块画板": "Duplicate: select an artboard first",
    "删除:至少保留一块画板": "Delete: at least one artboard must remain",
    "全部画板按{ $a1 }重新排列(经 SetGeom 复合命令,一次撤销)":
        "All artboards rearranged by { $a1 } (via a SetGeom compound command; one undo)",
    "重新排列:画板已在位": "Rearrange: artboards are already in place",
    "已按{ $a1 }排列 { $n } 块画板": "Arranged { $n } artboard(s) by { $a1 }",
    "画板几何 = 内容包围盒(选中画板;经 SetGeom)":
        "Artboard geometry = content bounding box (artboard selected; via SetGeom)",
    "画板已适配图稿边界": "Artboard fitted to artwork bounds",
    "适配图稿边界:先选中一块画板": "Fit to Artwork: select an artboard first",
    "横竖互换(w/h 互换,经 SetGeom)": "Swap orientation (w/h swap, via SetGeom)",
    "画板取向已互换": "Artboard orientation swapped",
    "画板已改名(data-vb-name 同步)": "Artboard renamed (data-vb-name synced)",
    "只有一个默认画板 —— 点「+ 新建」,或 Shift+O 用画板工具拖框。":
        "Only the default artboard — click \"+ New\", or Shift+O and drag with the Artboard tool.",
    "未选中文本对象 —— 选中后在此编辑字符样式;下方为新建文本默认样式。":
        "No text object selected — select one to edit character styles here; below are the defaults for new text.",
    "作用于:段内 run": "Applies to: runs within the paragraph",
    "作用于:整段": "Applies to: whole paragraph",
    "移除全部 run": "Remove all runs",
    "已移除段内 run(回到整段样式)": "Removed in-paragraph runs (back to whole-paragraph styling)",
    "整段转 run": "Convert paragraph to run",
    "已把全文包成单 run(字符样式现作用于 run)":
        "Wrapped the whole text into a single run (character styles now target the run)",
    "基线偏移:仅段内 run(整段落点无有效 CSS)":
        "Baseline shift: runs only (whole paragraph has no effective CSS)",
    "语言": "Language",
    "语言 / 抗锯齿:仅整段(继承节点)": "Language / anti-aliasing: whole paragraph only (inherits node)",
    "字偶距 / 垂直缩放 / 水平缩放 / 字符旋转:冻结点 —— 无对称 CSS 往返落点,不做假控件(04a 报告处置表)":
        "Kerning / vertical scale / horizontal scale / character rotation: frozen — no symmetric CSS round-trip target, so no fake controls (04a report disposition)",
    "画布文字为近似渲染(ADR-0017);导出为真字形,以浏览器校对为准。":
        "Canvas text is approximate rendering (ADR-0017); export produces true glyphs — Browser Proof is the arbiter.",
    "新建文本默认样式": "New text default style",
    "默认样式为会话状态(持久化 → workspace.json,阶段 7 登记)。":
        "Default styles are session state (persistence → workspace.json, registered in phase 7).",
    "(继承节点;输入数值即写 run 覆盖)": "(inherits node; typing a value writes a run override)",
    "{ $label }:未声明(继承)": "{ $label }: not declared (inherited)",
    "允许末行悬挂": "allow last line to hang",
    "强制末行悬挂": "force last-line hanging",
    "两端对齐·末行两端": "Justify · last line justified",
    "全部两端(含末行)": "justify all (including last line)",
    "强制撑满(末行两端)": "force full justify (last line justified)",
    "未选中文本对象 —— 段落属性作用于整个文本对象,请先选中。":
        "No text object selected — paragraph properties apply to the whole text object; select one first.",
    "文本溢出约 { $a1 }px": "Text overflows by ~{ $a1 }px",
    "自动扩高(补足内容高度)": "Auto-grow height (fills the content height)",
    "区域文本:内容未溢出": "Area text: content does not overflow",
    "双击画布区域文本右下角溢出红点也可自动扩高。":
        "Double-clicking the red overflow dot at an area text's bottom-right corner also auto-grows the height.",
    "区域文本属性仅作用于区域文本(点文本宽度自适应)":
        "Area-text properties apply to area text only (point text wraps by width automatically)",
    "{ $a1 }:未声明(0 / 继承)": "{ $a1 }: not declared (0 / inherited)",
    "已复制(副本插到原对象之后;Ctrl+Z 可撤销)":
        "Copied (the duplicate is inserted after the original; Ctrl+Z to undo)",
    "隔离(进入隔离模式)": "Isolate (enter isolation mode)",
    "锁定其他:没有其他对象": "Lock Others: no other objects",
    "已锁定其他 { $n } 个对象": "Locked { $n } other object(s)",
    "隐藏其他:没有其他对象": "Hide Others: no other objects",
    "已隐藏其他 { $n } 个对象": "Hid { $n } other object(s)",
    "转换为编组:该对象不支持": "Convert to Group: unsupported for this object",
    "已复制到目标位置(Alt+拖拽)": "Copied to the target position (Alt+drag)",
    "已移入目标位置(世界位置保持)": "Moved to the target position (world position kept)",
    "已调整层序": "Reordered layers",
    "没有画板:先用画板工具(Shift+O)在画布上创建":
        "No artboard: create one on the canvas with the Artboard tool (Shift+O) first",
    "已新建图层 { $sid }(画板末尾;Ctrl+Z 可撤销)":
        "New layer { $sid } (at the end of the artboard; Ctrl+Z to undo)",
    "搜索图层名": "Search layer names",
    "用左侧工具创建,或双击下方空白新建图层":
        "Create with the tools on the left, or double-click the empty area below to add a layer",
    "没有匹配的图层(清空搜索框恢复)": "No matching layers (clear the search box to restore)",
    "双击:新建图层(加到当前画板末尾)":
        "Double-click: new layer (appended to the current artboard)",
    "⌖ 定位对象": "* Locate object",
    "定位对象:先在图层树选中一个节点":
        "Locate object: select a node in the layer tree first",
    "颜色标记 → { $c }(data-vb-mark,已入文档)":
        "Color mark → { $c } (data-vb-mark, saved into the document)",
    "颜色标记已清除": "Color mark cleared",
    "图层颜色标记(点击循环;颜色随文档保存)":
        "Layer color mark (click to cycle; saved with the document)",
    "已重命名(data-vb-name 同步)": "Renamed (data-vb-name synced)",
    "Alt+拖拽:复制到目标位置": "Alt+drag: copy to the target position",
    "冻结块:含不支持的 CSS,原样保留(❄)":
        "Freeze block: contains unsupported CSS, kept as-is (*)",
    "盒A": "boxA", "盒B": "boxB", "盒C": "boxC", "盒a": "boxa",
    "画板不得进入容器": "artboard must not enter a container",
    "上一画板(Ctrl+PageUp)": "Previous artboard (Ctrl+PageUp)",
    "下一画板(Ctrl+PageDown)": "Next artboard (Ctrl+PageDown)",
    "适合窗口(Ctrl+0):全部画板可见;滚轮 / Ctrl+滚轮缩放":
        "Fit in Window (Ctrl+0): all artboards visible; scroll / Ctrl+scroll to zoom",
    "导出在后台线程执行;完成后经 toast 与状态栏提示":
        "Export runs on a background thread; a toast and the status bar announce completion",
    "导出仍在后台进行(关闭对话框不会取消)":
        "Export still running in the background (closing this dialog does not cancel it)",
    "自动保存快照写入项目 .vb-autosave/(滚动保留 3 份;不覆盖 index.html)":
        "Autosave snapshots go to the project's .vb-autosave/ (rolling 3 copies; index.html is never overwritten)",
    "外部已改动(已重载)": "Changed externally (reloaded)",
    "外部已改动(未采用)": "Changed externally (not adopted)",
    "外部未采用": "external changes not adopted",
    "点击查看最近外部改动的时间与触发文件":
        "Click to see when external changes last happened and which file triggered them",
    "本地有未保存编辑,未自动采用 —— 点击查看触发文件":
        "Unsaved local edits; not auto-adopted — click to see the trigger file",
    "GPU/渲染器不可用,画布内容未按完整管线渲染":
        "GPU/renderer unavailable; canvas content was not rendered through the full pipeline",
    "画布文字为近似渲染(ADR-0017);导出为真字形,可用「视图 → 浏览器校对」对拍":
        "Canvas text is approximate rendering (ADR-0017); export produces true glyphs — compare via View → Browser Proof",
    "最近外部改动": "recent external changes",
    "本会话还没有检测到外部改动。": "No external changes detected this session.",
    "未采用": "not adopted",
    "本地有未保存编辑:未自动采用。可对比磁盘版本后取舍。":
        "Unsaved local edits: not auto-adopted. Compare with the disk version and choose.",
    "触发文件:": "Trigger file:",
    "…(仅记录最近的触发文件)": "… (only the most recent trigger file is kept)",
    "渲染:无 GPU(降级)": "Render: no GPU (degraded)",
    "FPS 采样中… · 渲染:无 GPU(降级)": "FPS sampling… · Render: no GPU (degraded)",
    "调试数据仅开发者可见;用户界面默认不显示任何性能/硬件信息(04-4)。":
        "Debug data is developer-only; the user UI shows no performance/hardware info by default (04-4).",
    "项目目录未知(文档未保存到磁盘)": "Project directory unknown (document not saved to disk)",
    "浏览器截图解码失败:{ $e }": "Failed to decode the browser screenshot: { $e }",
    "浏览器校对:等待画布帧…": "Browser Proof: waiting for a canvas tick…",
    "浏览器校对:画布侧已取,浏览器渲染中…":
        "Browser Proof: canvas side captured; browser rendering…",
    "浏览器校对:画布侧读取失败({ $e })":
        "Browser Proof: failed to read the canvas side ({ $e })",
    "浏览器校对不可用(降级,未产出分数)":
        "Browser Proof unavailable (degraded; no score produced)",
    "浏览器校对线程异常退出": "Browser Proof thread exited abnormally",
    "⚠ 浏览器校对不可用:{ $err }。本次校对降级,不产出分数(不假绿)。":
        "! Browser Proof unavailable: { $err }. This proof degrades and produces no score (no fake green).",
    "等待画布帧(相机对准中)…": "Waiting for a canvas tick (camera aligning)…",
    "画布侧已取;浏览器渲染中(系统 Edge/Chrome,后台)…":
        "Canvas side captured; browser rendering (system Edge/Chrome, in background)…",
    "无分数(浏览器侧降级)。画布侧截图仍可查看。":
        "No score (browser side degraded). The canvas-side screenshot is still viewable.",
    "画布侧未采样。": "Canvas side not sampled.",
    "并排": "side by side", "滑块": "slider", "分割": "split",
    "导出对比图": "Export compare image",
    "上次导出:{ $p }": "Last export: { $p }",
    "浏览器": "browser",
    "叠加:透明度滑块(0 = 画布,1 = 浏览器)":
        "Overlay: opacity slider (0 = canvas, 1 = browser)",
    "滑块对比:左侧 = 画布,右侧 = 浏览器":
        "Slider compare: left = canvas, right = browser",
    "差异热力图(红 = 不一致)": "Diff heatmap (red = mismatch)",
    "对比图导出失败(缺画布/浏览器/热力图之一)":
        "Compare-image export failed (missing canvas/browser/heatmap)",
    "对比图导出失败:建目录失败": "Compare-image export failed: could not create the directory",
    "对比图已导出:{ $a1 }": "Compare image exported: { $a1 }",
    "对比图导出失败:{ $e }": "Compare-image export failed: { $e }",
    "已选中主件原型(定义区不在画布;编辑主件将同步全部实例)":
        "Master prototype selected (the definition area is not on canvas; editing the master syncs all instances)",
    "(内部编辑将登记为覆盖,主件同步时保留)":
        "(internal edits register as overrides, preserved when the master syncs)",
    "(编辑主件将同步全部实例;定义区不随页面导出为可见内容)":
        "(editing the master syncs all instances; the definition area is not exported as visible page content)",
    "标签 { $tag_sel }": "tag { $tag_sel }",
    "已选 { $a1 } 个对象 · 编辑作用于全部(一条撤销)":
        "{ $a1 } object(s) selected · edits apply to all (one undo)",
    "倾斜 / 参考点九宫格 / 缩放描边和效果 → 阶段 2(03)":
        "Skew / reference-point grid / scale stroke & effects → phase 2 (03)",
    "外观面板 ⇧F6": "Appearance panel Shift+F6",
    "多填充 / 描边 / 效果条目(排序·禁用·混合模式)→ 外观面板;描边全字段 → 描边面板":
        "Multiple fill/stroke/effect entries (reorder · disable · blend mode) → Appearance panel; all stroke fields → Stroke panel",
    "主轴 { $justify }": "main axis { $justify }",
    "交叉轴 { $align }": "cross axis { $align }",
    "字体族 / 字距 / 行距 → 字符面板(Ctrl+T);对齐/缩进/段距 → 段落面板(Ctrl+Alt+T)":
        "Font family / letter spacing / line height → Character panel (Ctrl+T); alignment/indent/paragraph spacing → Paragraph panel (Ctrl+Alt+T)",
    "目标 { $a1 }": "target { $a1 }",
    "名称 → { $a1 }": "name → { $a1 }",
    "导出倍率 @1x/@2x/@3x:暂无节点级存储位,现用导出对话框统一倍率(遗留项见 02c 报告)":
        "Export scale @1x/@2x/@3x: no per-node storage yet; the export dialog's single scale is used (ledger item, see 02c report)",
    "联集": "Union", "减去顶层": "Minus Front", "交集": "Intersect", "差集": "Exclude",
    "点选画布对象,或从下面开始。": "Click-select an object on the canvas, or start below.",
    "选中后按 变换/外观/布局/文本/交互/无障碍/导出 分组编辑;数值框支持拖标签改值与表达式(如 320/2、50%)。":
        "When selected, edit in Transform/Appearance/Layout/Text/Interaction/Accessibility/Export groups; number fields support label-drag and expressions (e.g. 320/2, 50%).",
    "选择工具(V):点选 / 拖框选": "Select tool (V): click-select / drag a rect",
    "缩放到全部画板可见(Ctrl+0)": "Zoom to show all artboards (Ctrl+0)",
    "切换到图层面板": "switch to the Layers panel",
    "切换到画板面板(可新建画板)": "switch to the Artboards panel (create artboards there)",
    "提示:V 点选 · M 矩形 · L 椭圆 · Ctrl+0 适合窗口。":
        "Hints: V select · M rectangle · L ellipse · Ctrl+0 fit window.",
    "插件不得调用插件管理入口(防递归与自我授权)":
        "Plugins may not call the Plugin Manager entry (guards recursion and self-authorization)",
    "命令 { $command } 不是宿主已注册命令":
        "Command { $command } is not a registered host command",
    "写 { $a1 } 失败:{ $e }": "Failed to write { $a1 }: { $e }",
    "导出格式 { $other } 不受宿主支持(仅 png/svg)":
        "Export format { $other } is not supported by the host (png/svg only)",
    "插件 = 外部进程(stdio JSON-RPC);默认零权限,首次启用需授权。":
        "A plugin = an external process (stdio JSON-RPC); zero permissions by default, first enable requires authorization.",
    "注意:插件以你的用户权限原生运行,启用前会展示完整权限告知并要求显式确认。":
        "Note: plugins run natively with your user permissions; a full permission notice is shown and explicit confirmation required before enabling.",
    "插件已安装:{ $id }(启用前需授权)": "Plugin installed: { $id } (authorization required before enabling)",
    "重载已登记插件": "Reload registered plugins",
    "插件注册表已重载(运行中的插件会被停止)":
        "Plugin registry reloaded (running plugins will be stopped)",
    "尚未安装任何插件。仓库自带示例:plugins/example-stats(统计元素)。":
        "No plugins installed yet. The repository ships an example: plugins/example-stats (counts elements).",
    "授权文件:{ $a1 }(与 recent.json 同范式;撤销授权即停用)":
        "Authorization file: { $a1 } (same scheme as recent.json; revoking disables the plugin)",
    "(未配置,本会话不持久化)": "(not configured; not persisted this session)",
    "插件 { $id }:启动中…": "Plugin { $id }: starting…",
    "未授权(启用时需在弹窗确认 manifest 权限)":
        "Unauthorized (confirm the manifest permissions in the dialog when enabling)",
    "命令权限:{ $a1 }": "Command permissions: { $a1 }",
    "命令权限:无(零权限)": "Command permissions: none (zero permissions)",
    "日志(最近 { $LOG_CAP } 条,新在下):": "Log (last { $LOG_CAP } entries, newest at bottom):",
    "该插件声明的全部权限:": "All permissions declared by this plugin:",
    "· 调用宿主命令「{ $c }」({ $label })": "· call host command \"{ $c }\" ({ $label })",
    "· 注册面板「{ $a1 }」(受控 UI,禁任意代码)": "· register panel \"{ $a1 }\" (controlled UI, no arbitrary code)",
    "· 注册导出动作「{ $a1 }」(输出只落你选择的目录)":
        "· register export action \"{ $a1 }\" (output only goes to the directory you choose)",
    "⚠ 权限告知(必读)": "! Permission Notice (must read)",
    "授权后插件只能调用上列命令;越权调用会被拒绝并记录。":
        "After authorization the plugin may only call the commands above; unauthorized calls are rejected and logged.",
    "插件是独立进程:崩溃 / 超时只影响它自己,宿主可一键重启。":
        "A plugin is a separate process: crashes/timeouts affect only itself; the host can restart it with one click.",
    "插件没有直改文档文件的通道,修改文档只能经宿主命令(可撤销)。":
        "Plugins have no direct channel to document files; document changes go only through host commands (undoable).",
    "我已阅读并理解:此插件是以我本人权限运行的原生进程":
        "I have read and understood: this plugin runs as a native process with my own user permissions",
    "插件 { $id }:已授权并启动": "Plugin { $id }: authorized and started",
    "已授权但启动失败:{ $e }": "Authorized but failed to start: { $e }",
    "插件 { $a1 }:未授权,保持停用": "Plugin { $a1 }: unauthorized, remains disabled",
    "尚无运行中的插件面板。": "No running plugin panels.",
    "在「编辑 → 插件管理…」安装并启用插件(首次启用需授权)。":
        "Install and enable plugins in Edit → Plugin Manager… (first enable requires authorization).",
    "导出已取消:未选择目录": "Export cancelled: no directory chosen",
    "等待插件提交面板内容…(点插件面板按钮,或在管理窗口重启插件)":
        "Waiting for the plugin to submit panel content… (click the plugin panel button, or restart the plugin in the manager)",
    "已提交输入到插件 { $pid }": "Submitted input to plugin { $pid }",
    "插件已装载:{ $id }(启用前需授权)": "Plugin loaded: { $id } (authorization required before enabling)",
    "改动即时生效,自动写入 workspace.json":
        "Changes take effect immediately and are written to workspace.json automatically",
    "界面动效(对话框/面板淡入、悬停过渡;关闭后立即到位)":
        "UI motion (dialog/panel fade-ins, hover transitions; off = jump to end)",
    "紧凑密度(列表行高 24;默认 comfortable 28)":
        "Compact density (list rows 24; comfortable default 28)",
    "显示未支持工具(置灰展示,点击见计划说明)":
        "Show unsupported tools (greyed out; click for the plan)",
    "铅笔保真度容差": "Pencil fidelity tolerance",
    "(自由绘制抽稀容差,越大越平滑)":
        "(freehand thinning tolerance; larger = smoother)",
    "界面语言 → { $a1 }": "UI language → { $a1 }",
    "对应菜单:编辑 → 设置 → 显示未支持工具 / 铅笔保真度":
        "Menu equivalent: Edit → Settings → Show Unsupported Tools / Pencil Fidelity",
    "(继承)": "(inherited)",
    "只影响此后新建的文本对象;已有文本不变":
        "Affects only text objects created afterwards; existing text is unchanged",
    "长度单位:像素 px(HTML/CSS 唯一单位,不可换算)":
        "Length unit: pixel px (the only HTML/CSS unit; no conversion)",
    "对应菜单:视图 → 显示标尺;默认显隐随工作区记忆":
        "Menu equivalent: View → Show Rulers; default visibility remembered per workspace",
    "(缩放时按 4× 分级,小于 16px/格自动放大)":
        "(grid scales in 4x steps below 16px/cell automatically)",
    "对应菜单:视图 → 显示网格 / 显示参考线":
        "Menu equivalent: View → Show Grid / Show Guides",
    "对应菜单:视图 → 智能参考线;提示色品红,行为对齐 AI":
        "Menu equivalent: View → Smart Guides; hint color magenta, behavior matches AI",
    "只影响此后新建的画板;改已有画板尺寸用「画板」面板":
        "Affects only artboards created afterwards; resize existing artboards in the Artboards panel",
    "渲染后端:Vello(wgpu)": "Render backend: Vello (wgpu)",
    "GPU 画布:不可用(降级渲染;见状态栏标注)":
        "GPU canvas: unavailable (degraded rendering; see the status bar note)",
    "本版本不提供渲染后端切换;FPS/显卡型号见「视图 → 开发者统计」":
        "This version has no render backend switch; FPS/GPU model in View → Developer Stats",
    "(叠加在系统 DPI 之上)": "(stacked on system DPI)",
    "显示提示条(操作提示/入门教学)":
        "Show the hints bar (operation tips / getting-started guidance)",
    "对应菜单:视图 → 浅色主题 / 界面缩放 / 提示":
        "Menu equivalent: View → Light Theme / UI Scale / Hints",
    "自动保存间隔": "Autosave interval",
    "自动保存:关闭(请常按 Ctrl+S)": "Autosave: off (press Ctrl+S often)",
    "自动保存:每 { $s } 秒": "Autosave: every { $s } s",
    "快照保留数": "Snapshots kept",
    "(滚动保留,写入项目 .vb-autosave/)": "(rolling retention, written to the project's .vb-autosave/)",
    "快照绝不覆盖 index.html;「设置 → 自动保存间隔」入口保留兼容":
        "Snapshots never overwrite index.html; the Settings → Autosave Interval entry is kept for compatibility",
    "首选项已恢复默认(布局归「窗口 → 工作区」管理)":
        "Preferences restored to defaults (layout is managed by Window → Workspaces)",
    "自动保存:每 { $s } 秒(快照写入项目 .vb-autosave/,不覆盖 index.html)":
        "Autosave: every { $s } s (snapshots go to the project's .vb-autosave/; index.html never overwritten)",
    "发现未保存的自动快照": "Unsaved auto-snapshot found",
    "「{ $name }」检测到上次异常退出留下的自动快照({ $when }写入)。":
        "\"{ $name }\" has an auto-snapshot left by the last abnormal exit (written { $when }).",
    "磁盘上的 index.html 未被快照覆盖;恢复只载入内存,何时写回由你决定。":
        "The disk index.html is not overwritten by the snapshot; restoring only loads memory — you decide when to write back.",
    "查看差异": "View diff", "暂不处理": "Not now",
    "快照保留期间会随编辑继续滚动;选择后本窗口关闭。":
        "The snapshot keeps rolling as you edit; this window closes after you choose.",
    "已丢弃自动快照(继续使用磁盘版本)":
        "Auto-snapshot discarded (continuing with the disk version)",
    "快照已保留;可继续编辑,下次打开仍会提示":
        "Snapshot kept; you can keep editing — you will be asked again next open",
    "从快照恢复:磁盘文件未动,Ctrl+S 写回(撤销栈已清空)":
        "Restored from snapshot: disk untouched, Ctrl+S writes back (undo stack cleared)",
    "磁盘 index.html:不可读(文件缺失?)": "disk index.html: unreadable (file missing?)",
    "内存当前态:{ $mem_lines } 行({ $a1 })": "Memory current state: { $mem_lines } lines ({ $a1 })",
    "内存当前态:{ $mem_lines } 行(磁盘方缺失,无从对比)":
        "Memory current state: { $mem_lines } lines (disk side missing; nothing to compare)",
    "(磁盘 index.html 不可读,无法做双方 diff;快照内容见下)":
        "(disk index.html unreadable; cannot diff; snapshot content below)",
    "「磁盘 −」/「快照 +」统一视图;差异 { $changed } 行。删除行=磁盘独有,新增行=快照独有。":
        "\"disk −\" / \"snapshot +\" unified view; { $changed } differing lines. Removed = disk only, added = snapshot only.",
    "创建组件:请先选中一个对象(多选不支持)":
        "Create Symbol: select one object first (multi-select unsupported)",
    "已创建组件「{ $name }」(当前位置成为首个实例;编辑主件将同步全部实例)":
        "Symbol \"{ $name }\" created (the current position becomes the first instance; editing the master syncs all instances)",
    "分离实例:选中对象里没有组件实例":
        "Detach Instance: no symbol instance in the selection",
    "已分离为普通元素(不再随主件同步)": "Detached into a plain element (no longer syncs with the master)",
    "重置覆盖:选中对象里没有组件实例":
        "Reset Overrides: no symbol instance in the selection",
    "替换主件定义:请选中一个组件实例": "Replace Master Definition: select a symbol instance first",
    "已用该实例内容替换主件定义,并同步其余实例(各自的覆盖仍保留)":
        "Replaced the master definition with this instance's content and synced the other instances (their overrides are preserved)",
    "选择所有实例:先选中一个实例或主件":
        "Select All Instances: select an instance or the master first",
    "选择所有实例:选中对象不属于任何组件":
        "Select All Instances: the selection does not belong to any symbol",
    "已选中同主件全部实例({ $n } 个)": "Selected all instances of the master ({ $n })",
    "文档没有可播放的动画(先在时间轴加关键帧)":
        "The document has no playable animation (add keyframes in the Timeline first)",
    "播放动画预览(与导出同一求值路径)":
        "Play the animation preview (same evaluation path as export)",
    "预览已停止(播放头回 0,画布恢复静态)":
        "Preview stopped (playhead back to 0; canvas back to static)",
    "循环播放:开": "Loop: on",
    "循环播放:关(到尾暂停)": "Loop: off (pauses at the end)",
    "加关键帧:请先选中一个对象": "Add Keyframe: select an object first",
    "该对象的动画非时间轴命名,不能在此加帧":
        "This object's animation is not Timeline-named; frames cannot be added here",
    "播放头处各轨道已有关键帧": "All tracks already have keyframes at the playhead",
    "删除关键帧:先在时间轴上点选一个关键帧":
        "Delete Keyframe: click-select a keyframe on the Timeline first",
    "清除动画:请先选中一个对象": "Clear Animation: select an object first",
    "该对象没有动画": "This object has no animation",
    "动画「{ $name }」来自外部命名关键帧,请手工编辑 CSS 或改名后处理":
        "Animation \"{ $name }\" comes from externally named keyframes; edit the CSS manually or rename it first",
    "已清除对象动画(@keyframes 与 animation 声明一并移除;Ctrl+Z 撤销)":
        "Cleared the object's animation (@keyframes and animation declarations removed together; Ctrl+Z to undo)",
    "未选中对象 —— 选中一个对象后可为它编排关键帧动画。":
        "No object selected — select one to choreograph its keyframe animation.",
    "选择工具(V):点选要动画的对象": "Select tool (V): click the object to animate",
    "循环播放(关 = 到尾暂停)": "Loop (off = pause at the end)",
    "移除全部关键帧(= 删除对应 CSS)": "Remove all keyframes (= delete the corresponding CSS)",
    "双击轨道加关键帧 · 点选后拖动改时刻 · Alt+点击删帧 · 拖顶部刻度 scrub":
        "Double-click a track to add a keyframe · select and drag to retime · Alt+click to delete · drag the top ruler to scrub",
    "选中轨道上的关键帧后可改值与缓动。":
        "Select a keyframe on a track to edit its value and easing.",
    "删除此帧": "Delete this keyframe",
    "释放剪切蒙版:取不到内容几何": "Release clipping mask: cannot read content geometry",
    "cubic-bezier 曲线预览(与预览/导出同一求解;拖柄编辑留后续)":
        "cubic-bezier curve preview (same solver as preview/export; handle editing comes later)",
    "往该边放一个 Panel": "place a panel on this edge",
    "拖动到窗口边缘吸附停靠(当前:{ $a1 }{ $a2 })":
        "Drag to a window edge to dock (current: { $a1 }{ $a2 })",
    "工具栏:未吸附到边缘,已回弹(不做浮动)":
        "Toolbar: not snapped to an edge, snapped back (no floating)",
    "Figma UI3 标志设计": "Figma UI3 signature design",
    "单/双列仅在左/右停靠时可用(顶/底停靠是单行)":
        "Single/two columns available only when docked left/right (top/bottom dock is a single row)",
    "布局未持久化:{ $e }": "Layout not persisted: { $e }",
    "窗口布局未持久化:{ $e }": "Window layout not persisted: { $e }",
    "工作区未持久化:{ $e }": "Workspace not persisted: { $e }",
    "找不到配置目录,工作区预设不会持久化":
        "Config directory not found; workspace presets will not persist",
    "工作区名不能为空": "Workspace name cannot be empty",
    "工作区「{ $name }」不存在(可能已在其它窗口删除)":
        "Workspace \"{ $name }\" does not exist (it may have been deleted in another window)",
    "投影 → 构建命令": "projection → build command",
    "双真相": "double truth", "旋转后仍按 bbox 选中": "still selected by bbox after rotation",
    "轴心不动": "pivot stays fixed",
    "未选中对象 —— 选中后可数值化变换。":
        "No object selected — select one for numeric transforms.",
    "多选:数值作用于公共包围盒。": "Multi-select: values apply to the common bounding box.",
    "锁定等比(改 W 时 H 同比例)": "Lock ratio (H scales with W)",
    "参考点(缩放/倾斜轴心)": "Reference point (scale/skew pivot)",
    "倾斜X": "Skew X", "倾斜Y": "Skew Y",
    "HTML 无「描边随框缩放」语义:统一为几何缩放(border-width 不随之变)":
        "HTML has no \"stroke scales with box\" semantics: unified as geometric scaling (border-width does not follow)",
    "几何取整到整数像素(拖动/数值输入同源)":
        "Geometry rounded to whole pixels (same source for dragging and numeric input)",
    "文件不存在:{ $a1 }": "File does not exist: { $a1 }",
    "调用系统默认程序失败:{ $e }": "Failed to open with the system default program: { $e }",
    "打印:画板「{ $name }」已导出临时 PDF({ $a1 } KB,{ $a2 })并交系统打开({ $a3 });在 PDF 程序中执行打印":
        "Print: artboard \"{ $name }\" exported to a temporary PDF ({ $a1 } KB, { $a2 }) and opened by the system ({ $a3 }); print from the PDF program",
    "PDF 已生成({ $a1 }),但打开失败:{ $e }":
        "PDF generated ({ $a1 }) but opening failed: { $e }",
    "写临时 PDF 失败:{ $e }": "Failed to write the temporary PDF: { $e }",
    "把当前布局(工具箱停靠 / 面板坞 / 次级面板摆放)存为预设:":
        "Save the current layout (Toolbar dock / Tab Dock / secondary panel arrangement) as a preset:",
    "工作区名…": "Workspace name…",
    "内置:基本功能 / 排版 / 导出(见「窗口」菜单);下面是自定义预设:":
        "Built-in: Basics / Typography / Export (see the Window menu); custom presets below:",
    "(暂无自定义工作区)": "(no custom workspaces yet)",
    "切换": "switch",
    "vellumbench-print-未命名-": "vellumbench-print-untitled-",
    "空标题兜底": "empty-title fallback", "时间戳防同名覆盖": "timestamp guards same-name overwrite",
    "测试工作区": "test workspace",
    "预设必须落盘": "preset must hit the disk",
    "应用预设必须还原列数": "applying a preset must restore the column count",
    "删除必须落盘": "deletion must hit the disk",
    "窗口A的预设": "window A preset", "窗口B的预设": "window B preset",
    "B 的内存快照初始为空": "window B's memory snapshot starts empty",
    "预设保存以磁盘列表为基:A 的预设必须被保住":
        "preset saving is based on the disk list: A's preset must survive",
    "两窗预设都在": "presets present in both windows",
    "快照本身写坏": "snapshot itself corrupted",
    "创建快照目录失败:{ $e }": "Failed to create the snapshot directory: { $e }",
    "序列化快照失败:{ $e }": "Failed to serialize the snapshot: { $e }",
    "滚动快照失败:{ $e }": "Failed to roll snapshots: { $e }",
    "写快照失败:{ $e }": "Failed to write the snapshot: { $e }",
    "读快照失败:{ $e }": "Failed to read the snapshot: { $e }",
    "快照解析失败(疑似写坏):{ $e }": "Snapshot failed to parse (possibly corrupted): { $e }",
    "快照版本 { $a1 } 与当前 { $SCHEMA_VERSION } 不符":
        "Snapshot version { $a1 } does not match current { $SCHEMA_VERSION }",
    "创建恢复中转目录失败:{ $e }": "Failed to create the recovery staging directory: { $e }",
    "快照文件路径非法(越出项目目录): { $rel }":
        "Snapshot file path illegal (outside the project directory): { $rel }",
    "创建快照子目录失败:{ $e }": "Failed to create the snapshot subdirectory: { $e }",
    "写快照文件 { $rel } 失败:{ $e }": "Failed to write snapshot file { $rel }: { $e }",
    "…(中段 { $a1 }+{ $a2 } 行过长,整块省略)…":
        "… (middle section of { $a1 }+{ $a2 } lines too long, omitted)…",
    "相机缩 compensate": "camera scaling compensation",
    "无 wgpu 渲染状态": "no wgpu render state",
    "GPU 画布未初始化(vello 不可用?)": "GPU canvas not initialized (vello unavailable?)",
    "画布纹理不存在": "canvas texture does not exist",
    "map_async 通道关闭": "map_async channel closed",
    "纹理映射失败:{ $e }": "Texture mapping failed: { $e }",
    "画板裁剪矩形为空({ $x0 },{ $y0 })-({ $x1 },{ $y1 });纹理 { $tw }x{ $th }":
        "Artboard crop rect empty ({ $x0 },{ $y0 })-({ $x1 },{ $y1 }); texture { $tw }x{ $th }",
    "找不到画板 `{ $a1 }`": "Artboard `{ $a1 }` not found",
    "建目录失败:{ $e }": "Failed to create the directory: { $e }",
    "PNG 落盘失败:{ $e }": "Failed to write the PNG: { $e }",
    "vello(形状/图像;egui 文字叠加层不在读回纹理内)":
        "vello (shapes/images; the egui text overlay is not in the read-back texture)",
    "关于": "About", "全部": "All", "外观": "Appearance", "资产": "Assets",
    "背景": "Background", "前移一层": "Bring Forward", "中心": "center",
    "中文": "Chinese", "对比": "compare", "冲突对比": "conflict compare",
    "数据": "Data", "椭圆": "Ellipse", "文件": "File", "格式": "format",
    "常规": "General", "组": "Group", "健康": "Health", "首页": "Home",
    "图像": "Image", "交互": "Interaction", "跳转确认": "jump confirm",
    "键位方案": "keymap", "布局": "layout", "直线": "Line", "链接": "link",
    "日志": "Log", "循环": "loop", "镜像": "Mirror", "修改": "modify",
    "名称": "name", "关": "off", "开": "on", "不透明": "opacity",
    "其他": "Other", "轮廓": "outline", "段落": "Paragraph",
    "性能": "Performance", "固定": "pin", "计划": "planned",
    "偏好": "Preferences", "属性": "Properties", "矩形": "Rectangle",
    "旋转": "Rotate", "搜索": "Search", "后移一层": "Send Backward",
    "切片": "slice", "样式": "style", "文字": "Text", "文本": "text",
    "主题": "theme", "标题": "title", "变换": "Transform", "类型": "type",
    "取消固定": "unpin", "未命名": "Untitled", "未命名项目": "Untitled Project",
    "窗口": "Window",
    "键位方案:无法识别组合键「{ $a1 }」(已跳过)":
        "Keymap: cannot recognize the combo \"{ $a1 }\" (skipped)",
    "键位方案:{ $a1 } 与 { $a2 } 同时绑定「{ $a3 }」,已保留前者":
        "Keymap: { $a1 } and { $a2 } both bind \"{ $a3 }\"; kept the former",
    "找不到配置目录,自定义键位不会持久化(可用 VB_KEYMAP 指定)":
        "Config directory not found; custom key bindings will not persist (set VB_KEYMAP to choose one)",
    "keymap.json 版本 { $a1 } 与当前 { $SCHEMA_VERSION } 不符,已回退默认键位":
        "keymap.json version { $a1 } does not match current { $SCHEMA_VERSION }; fell back to the default keymap",
    "keymap.json 解析失败,已回退默认键位:{ $e }":
        "keymap.json failed to parse; fell back to the default keymap: { $e }",
    "序列化键位方案失败:{ $e }": "Failed to serialize the keymap: { $e }",
    "写键位方案失败:{ $e }": "Failed to write the keymap: { $e }",
    "提交键位方案失败:{ $e }": "Failed to commit the keymap: { $e }",
    "找不到配置目录,键位方案未持久化": "Config directory not found; keymap not persisted",
    "Agent 可复现": "Agent-reproducible",
    "恢复上次会话({ $session_len })": "Restore last session ({ $session_len })",
    "搜索名称或路径…(Ctrl+F)": "Search name or path… (Ctrl+F)",
    "v{ $a1 } · 许可 ACL-1.0": "v{ $a1 } · license ACL-1.0",
    "主页是主窗口:关闭主页 = 退出 Vellum Bench":
        "The Launcher is the main window: closing it quits Vellum Bench",
    "没有匹配「{ $a1 }」的项目": "No projects matching \"{ $a1 }\"",
    "来源:artboard 项目(index.html 带画板标记类,可直接打开编辑)":
        "Source: artboard project (index.html carries artboard marker classes; opens for editing directly)",
    "路径已失效": "path no longer exists",
    "在资源管理器中显示": "Show in File Explorer",
    "{ $a1 }(路径已失效)": "{ $a1 } (path stale)",
    "三条路": "three paths",
    "从下面开始,或打开一个含 index.html 的项目目录;也可以把目录拖进本窗口。":
        "Start below, or open a project directory containing index.html; you can also drop a directory onto this window.",
    "空白画板,从零开始": "Blank artboard, from scratch",
    "选择含 index.html 的目录": "Choose a directory containing index.html",
    "内置 landing 等起手模板": "Built-in starter templates like landing",
    "正在打开「{ $name }」…": "Opening \"{ $name }\"…",
    "(首次打开含渲染初始化,约需几秒)":
        "(first open includes render init, a few seconds)",
    "移除最近项目记录?": "Remove the recent-project record?",
    "只移除记录,不删除磁盘上的项目文件。":
        "Only the record is removed; project files on disk are untouched.",
    "能力台账(做了什么 / 没做什么)":
        "Capability Ledger (what is done / not done)",
    "已落地": "landed", "部分": "partial", "不做": "won't do",
    "可交互强调": "interactive emphasis",
    "设 env + 触发 workspace 读写": "set env + trigger workspace read/write",
    "移动 · 750×1334": "Mobile · 750x1334", "移动 · 375×667": "Mobile · 375x667",
    "项目名不能为空": "Project name cannot be empty",
    "目录已存在:{ $a1 }(换个项目名或位置)":
        "Directory already exists: { $a1 } (use another name or location)",
    "创建项目目录失败:{ $e }": "Failed to create the project directory: { $e }",
    "创建 assets 失败:{ $e }": "Failed to create assets: { $e }",
    "写项目文件失败:{ $e }": "Failed to write the project file: { $e }",
    "存在但为空": "exists but empty",
    "找不到模板目录(examples);可用 VB_TEMPLATES 指定":
        "Template directory not found (examples); set VB_TEMPLATES to point at one",
    "创建目录 { $a1 } 失败:{ $e }": "Failed to create directory { $a1 }: { $e }",
    "读 { $a1 } 失败:{ $e }": "Failed to read { $a1 }: { $e }",
    "读目录项失败:{ $e }": "Failed to read directory entries: { $e }",
    "宽 ": "W ", "高 ": "H ",
    "浏览…": "Browse…",
    "创建(新窗口打开)": "Create (opens in a new window)",
    "外壳": "shell",
    "关闭根窗口 = 退出进程": "closing the root window quits the process",
    "隐藏后唤出": "recall after hide",
    "并存 + 关闭": "Keep + Close", "并存+关闭": "Keep + Close",
    "路径已失效,无法恢复:{ $dir }": "Path stale, cannot restore: { $dir }",
    "显式退出不确认": "explicit quit does not confirm",
    "「{ $a1 }」有未保存的修改。": "\"{ $a1 }\" has unsaved changes.",
    "关闭{ $a1 }将同时关闭其余 { $n } 个窗口。":
        "Closing { $a1 } will also close the other { $n } window(s).",
    "主窗口": "main window",
    "有未保存的修改,直接退出将丢失。":
        "There are unsaved changes; quitting now loses them.",
    "直接退出": "Quit without saving",
    "导出 {} @2x({} KB){}": "Exported { $a1 } @2x ({ $a2 } KB){ $a3 }",
    "导出:{}…": "Export: { $a1 }…",
    "导出中:{ $a1 }({ $a2 }s)…": "Exporting: { $a1 } ({ $a2 }s)…",
    "导出中…": "Exporting…",
    "导出失败:{ $err }": "Export failed: { $err }",
    "导出失败:{ $e }": "Export failed: { $e }",
    "导出完成:{ $a1 }": "Export finished: { $a1 }",
    "已保存 { $a1 } → { $a2 }": "Saved { $a1 } → { $a2 }",
    "已保存基线": "baseline saved",
    "自动保存失败:{ $e }": "Autosave failed: { $e }",
    "上一画板": "Previous Artboard", "下一画板": "Next Artboard",
    "上方的下一个对象": "next object above", "下方的下一个对象": "next object below",
    "上次导出(当前画板 PNG @2x)": "Repeat Export (current artboard PNG @2x)",
    "交集:仅保留全部所选的重叠区域(不重叠则报错);样式保留最先选中对象。":
        "Intersect: keep only the overlapping area of the whole selection (errors when there is no overlap); style from the first-selected object.",
    "从选区生成参考线": "Guides from Selection",
    "修边:每件减去其上方的对象,去掉描边,同填充色的碎片合并为一件;填充保留。一次撤销恢复原状。":
        "Trim: each shape subtracts the objects above it, strokes are removed, same-fill fragments merge into one; fills are kept. One undo restores.",
    "全部(当前画板)": "All (current artboard)",
    "减去后方对象:z 序最上的对象减去其下方全部对象,保留最上(与「减去顶层」保留对象相反)。":
        "Minus Back: the topmost object subtracts everything below it and stays (keeps the opposite object versus Minus Front).",
    "减去顶层:保留下方对象,减去它与最上层对象的重叠部分(结果 = 下方 − 上方,样式保留下方);其余删除。":
        "Minus Front: keep the lower objects and subtract their overlap with the topmost (result = lower − upper, style from the lower); the rest are deleted.",
    "分割:全部所选互相求交,重组为互不重叠的原子闭合区域(开放路径按闭合参与);每块填充取覆盖它的最上层对象。一次撤销恢复原状。":
        "Divide: all selected shapes intersect and recombine into non-overlapping atomic closed regions (open paths participate as closed); each region's fill comes from the topmost object covering it. One undo restores.",
    "切片 → 从选区建立": "Slice → From Selection",
    "创建轮廓": "Create Outlines",
    "动画 → 删除选中的关键帧": "Animation → Delete Selected Keyframe",
    "动画 → 在播放头处加关键帧": "Animation → Add Keyframe at Playhead",
    "动画 → 循环开 / 关": "Animation → Toggle Loop",
    "合并:与联集同一几何内核 —— AI 在两操作数下的合并即并集;描边/填充保留原对象样式。":
        "Merge: same geometry kernel as Union — in AI, merging two operands is the union; strokes/fills keep the original object styles.",
    "命令搜索…": "Command Search…",
    "图层面板显隐": "toggle the Layers panel",
    "对象 → 全部文本对象": "Object → All Text Objects",
    "对象 → 所有锁定对象": "Object → All Locked Objects",
    "对象 → 所有隐藏对象": "Object → All Hidden Objects",
    "对齐到 → 关键对象": "Align To → Key Object",
    "工作区 → 基本功能": "Workspace → Basics",
    "工作区 → 排版": "Workspace → Typography",
    "差集:挖去全部重叠区域,保留各自未重叠部分(可产出多块,合为一个路径)。":
        "Exclude: carve out every overlapping area, keeping each object's non-overlapping parts (may produce several pieces, merged into one path).",
    "帮助": "Help",
    "应用上一个效果": "Apply Last Effect",
    "建立剪切蒙版": "Make Clipping Mask",
    "扭曲和变换…": "Distort & Transform…",
    "显示全部对象": "Show All Objects",
    "更改大小写 → 大写": "Change Case → UPPERCASE",
    "更改大小写 → 小写": "Change Case → lowercase",
    "查找字体…": "Find Fonts…",
    "检查更新…": "Check for Updates…",
    "浅色主题": "Light Theme",
    "点文字 / 区域文字": "point / area text",
    "界面缩放 · 减小": "UI Scale · Decrease",
    "界面缩放 · 增大": "UI Scale · Increase",
    "界面缩放 · 复位 100%": "UI Scale · Reset to 100%",
    "相同 → 填充色": "Same → Fill Color",
    "相同 → 描边粗细": "Same → Stroke Weight",
    "相同 → 描边色": "Same → Stroke Color",
    "编辑": "Edit",
    "置入图像…": "Place Image…",
    "联集:所选对象合并为一件(并集轮廓);结果保留最先选中对象的样式与位置。":
        "Union: the selection merges into one object (union outline); the result keeps the first-selected object's style and position.",
    "裁剪:只保留全部所选的重叠区域并把边界裁齐(与交集同一几何内核)。":
        "Crop: keep only the overlapping area of the whole selection and trim the boundaries (same geometry kernel as Intersect).",
    "视图": "View",
    "解锁全部对象": "Unlock All Objects",
    "计划于 v2:扭曲与变换效果(无 CSS 无损对应)":
        "Planned v2: Distort & Transform effects (no lossless CSS equivalent)",
    "计划于 v2:把外部 HTML 作为新画板导入(当前请用「打开项目」)":
        "Planned v2: import external HTML as a new artboard (use Open Project for now)",
    "计划于 v2:检查更新": "Planned v2: check for updates",
    "设置 → 显示未支持工具": "Settings → Show Unsupported Tools",
    "设置 → 自动保存间隔": "Settings → Autosave Interval",
    "设置 → 铅笔保真度": "Settings → Pencil Fidelity",
    "路径查找器 → 交集": "Pathfinder → Intersect",
    "路径查找器 → 修边": "Pathfinder → Trim",
    "路径查找器 → 减去后方对象": "Pathfinder → Minus Back",
    "路径查找器 → 减去顶层": "Pathfinder → Minus Front",
    "路径查找器 → 分割": "Pathfinder → Divide",
    "路径查找器 → 合并": "Pathfinder → Merge",
    "路径查找器 → 差集": "Pathfinder → Exclude",
    "路径查找器 → 联集": "Pathfinder → Union",
    "路径查找器 → 裁剪": "Pathfinder → Crop",
    "路径查找器 → 轮廓": "Pathfinder → Outline",
    "轮廓:所有边线在与其他对象的交点处切开,输出为无填充的开放描边线(每段一件);描边继承来源对象,无描边时补 1px 黑。同对象自身交点不切。":
        "Outline: every edge is cut at intersections with other objects, producing unfilled open stroke lines (one per segment); strokes inherit from the source object, defaulting to 1px black when none. Self-intersections of one object are not cut.",
    "轮廓化描边": "Outline Stroke",
    "轮廓模式(线框)": "Outline Mode (wireframe)",
    "锁定所选对象": "Lock Selection",
    "键位速查表…": "Key Binding Cheat Sheet…",
    "键盘快捷键…": "Keyboard Shortcuts…",
    "隐藏所有面板": "Hide All Panels",
    "隐藏所选对象": "Hide Selection",
    "隐藏边缘": "Hide Edges",
    "风格化 → 内发光": "Stylize → Inner Glow",
    "风格化 → 内阴影": "Stylize → Inner Shadow",
    "风格化 → 圆角": "Stylize → Rounded Corners",
    "风格化 → 外发光": "Stylize → Outer Glow",
    "风格化 → 投影": "Stylize → Drop Shadow",
    "计划于 v2:键位速查表(当前请用「命令搜索」)":
        "Planned v2: key binding cheat sheet (use Command Search for now)",
    "计划于 v2:隐藏边缘(选中框细节开关)":
        "Planned v2: hide edges (selection-box detail toggle)",
    "计划于 v2:文字转轮廓(需字形轮廓导出)":
        "Planned v2: convert text to outlines (needs glyph outline export)",
    "计划于 v2:轮廓化描边(当前请用视图菜单的轮廓模式查看线框)":
        "Planned v2: outline stroke (use View menu's Outline mode for the wireframe for now)",
}

# 分词组合兜底(最长优先;含全角标点与符号字形)。由 --gen 的 TODO 报告迭代补齐。
PHRASES = {
    # ── 全角标点 → 英文标点(组合时左贴)──
    ",": ",", "。": ".", ":": ": ", "::": ": ", ";": "; ",
    "!": "!", "?": "?", "、": ", ", "(": " (", ")": ")", "《": "《",
    "》": "》", "「": "\u201c", "」": "\u201d", "『": "\u201c", "』": "\u201d",
    "——": " — ", "—": "—", "…": "…", "·": "·", "≥": ">=", "≤": "<=",
    "×": "x", "÷": "/", "↔": "<>", "↕": "<->", "⇄": "<=>", "≡": "=",
    "⌖": "*", "└": "L.", "⚠": "!", "⟲": "Undo", "⟳": "Redo", "⤢": "Fit",
    "⬌": "<>", "⬍": "<>", "🗑": "Delete", "＋": "+", "°": "°", "❄": "*",
    "←": "<-", "→": "->", "↑": "Up", "↓": "Down",
    # ── 面板 / 界面区域 ──
    "面板坞": "Tab Dock", "面板": "panel", "图层面板": "Layers panel",
    "属性面板": "Properties panel", "画板面板": "Artboards panel",
    "令牌面板": "Tokens panel", "渐变面板": "Gradient panel",
    "描边面板": "Stroke panel", "对齐面板": "Align panel",
    "变换面板": "Transform panel", "外观面板": "Appearance panel",
    "字符面板": "Character panel", "段落面板": "Paragraph panel",
    "颜色面板": "Color panel", "透明度面板": "Opacity panel",
    "时间轴面板": "Timeline panel", "历史面板": "History panel",
    "资产面板": "Assets panel", "能力台账": "Capability Ledger",
    "插件管理": "Plugin Manager", "插件面板": "plugin panel",
    "工具箱": "Toolbar", "工具栏": "toolbar", "工具": "tool",
    "状态栏": "status bar", "菜单": "menu", "对话框": "dialog",
    "控制条": "context bar", "启动器": "Launcher", "主页": "Launcher",
    "首选项": "Preferences", "设置": "Settings",
    "文档设置": "Document Settings", "工作区": "workspace",
    "命令面板": "Command Palette", "项目体检": "Project Health Check",
    "项目健康检查": "Project Health Check", "健康检查": "Health check",
    "浏览器校对": "Browser Proof", "开发者统计": "Developer Stats",
    "像素预览": "Pixel Preview", "智能参考线": "Smart Guides",
    "参考线": "guides", "网格": "grid", "标尺": "rulers",
    "断点": "breakpoint", "提示条": "hints bar", "提示": "hint",
    "画布": "canvas", "画板": "artboard", "文档": "document",
    "图层": "layer", "图层树": "layer tree", "对象": "object",
    "节点": "node", "元素": "element", "内容": "content",
    "选区": "selection", "选中": "selected", "选择": "select",
    "路径查找器": "Pathfinder", "路径": "path", "矢量路径": "vector path",
    "锚点": "anchor point", "手柄": "handle", "端点": "endpoint",
    "边角": "corner", "箭头": "arrow", "止箭头": "end arrow",
    "起箭头": "start arrow", "曲率": "Curvature", "剪刀": "Scissors",
    "钢笔": "Pen", "铅笔": "Pencil", "吸管": "Eyedropper",
    "抓手": "Hand", "直接选择": "Direct Selection", "编组选择": "Group Select",
    "文字工具": "Type tool", "矩形工具": "Rectangle tool",
    "椭圆工具": "Ellipse tool", "选择工具": "Selection tool",
    "同族工具": "same-family tools", "实时上色": "Live Paint",
    "图像描摹": "Image Trace", "符号": "Symbol", "实例": "instance",
    "主件": "master", "组件": "Symbol", "隔离模式": "isolation mode",
    "隔离组": "isolation group", "挖空组": "knockout group",
    "冻结块": "freeze block", "冻结对象": "frozen object",
    "剪切蒙版": "clipping mask", "蒙版": "mask", "制作蒙版": "Make Mask",
    "释放剪切蒙版": "Release Clipping Mask", "反转蒙版": "Invert Mask",
    "不透明度蒙版": "opacity mask",
    "编组": "Group", "取消编组": "Ungroup", "解组": "Ungroup",
    "转换为编组": "Convert to Group",
    # ── 外观属性 ──
    "填充": "Fill", "描边": "stroke", "效果": "effect", "渐变": "gradient",
    "线性渐变": "linear gradient", "径向渐变": "radial gradient",
    "渐变网格": "gradient mesh", "色标": "color stop", "颜色": "color",
    "色板": "swatches", "全局色": "global color",
    "不透明度": "opacity", "透明度": "opacity", "透明背景": "transparent background",
    "混合模式": "blend mode", "混合": "blend", "叠加": "overlay",
    "内阴影": "inner shadow", "内发光": "inner glow",
    "外发光": "outer glow", "投影": "drop shadow", "羽化": "feather",
    "模糊": "blur", "高斯模糊": "Gaussian blur",
    "圆角": "corner radius", "粗细": "weight", "线宽": "stroke width",
    "圆头": "round cap", "平头": "flat cap", "方头": "square cap",
    "斜接": "miter", "斜接限": "miter limit", "斜切": "bevel",
    "虚线": "dash", "短线": "short dash", "抗锯齿": "anti-aliasing",
    "对齐像素网格": "snap to pixel grid", "缩放描边": "scale stroke",
    "居中描边": "centered stroke", "内侧": "inside", "外侧": "outside",
    "字色": "text color", "填充色": "fill color", "描边色": "stroke color",
    "参考点": "reference point", "轴心": "pivot", "等距中心": "equidistant center",
    # ── 排版 ──
    "字号": "font size", "字体": "font", "字体族": "font family",
    "字体替换": "Font Substitution", "字体安装": "font install",
    "字距": "letter spacing", "行距": "line height", "基线": "baseline",
    "基线偏移": "baseline shift", "段前": "space before", "段后": "space after",
    "缩进": "indent", "左缩进": "left indent", "右缩进": "right indent",
    "首行缩": "first-line indent", "标点挤压": "punctuation squeeze",
    "避头尾": "line-break rules", "连字": "ligatures", "悬挂": "hanging",
    "粗体": "bold", "斜体": "italic", "下划线": "underline", "删除线": "strikethrough",
    "大小写": "letter case", "正常名": "normal-case name", "富文本": "rich text",
    "整段": "whole paragraph", "点文本": "point text", "区域文本": "area text",
    "自动扩高": "auto-grow height", "溢出": "overflow",
    "微软雅黑": "Microsoft YaHei", "思源黑体": "Source Han Sans",
    "华文黑体": "STHeiti", "黑体": "Hei", "宋体": "Song", "楷体": "Kai",
    "仿宋": "FangSong", "等线": "DengXian",
    # ── 视图 / 状态 ──
    "显示": "show", "隐藏": "hide", "折叠": "collapse", "展开": "expand",
    "收起": "collapse", "停靠": "dock", "浮窗": "floating window",
    "次级坞": "secondary dock", "主坞": "main dock",
    "轮廓模式": "Outline mode", "适合窗口": "Fit in Window",
    "实际大小": "Actual Size", "适配图稿边界": "fit artboard to artwork",
    "缩放": "zoom", "放大": "zoom in", "缩小": "zoom out",
    "平移": "pan", "滚轮": "scroll wheel", "拖框": "drag a rect",
    "拖动": "drag", "拖拽": "drag", "单击": "click", "双击": "double-click",
    "点选": "click-select", "框选": "rubber-band select",
    "长按": "long-press", "松手": "release", "回弹": "snap back",
    "吸附": "snap", "约束": "constrain", "等比": "proportional",
    "主题:浅色": "Theme: light", "主题:深色": "Theme: dark",
    "浅色": "light", "深色": "dark", "界面动效": "UI motion",
    "界面缩放": "UI scale", "密度": "density", "紧凑": "compact",
    "行高": "row height", "列": "column(s)", "单列": "single column",
    "双列": "two columns", "按列": "by column", "按行": "by row",
    # ── 文件 / 项目 ──
    "新建": "New", "新建项目": "New Project", "新建文档": "New Document",
    "新建图层": "New Layer", "新建画板": "New Artboard",
    "新建文本": "new text", "新画板": "new artboard", "新图层": "new layer",
    "打开项目": "Open Project", "打开": "Open", "保存": "Save",
    "保存全部并退出": "Save All and Quit", "不保存": "Don't Save",
    "保存为预设": "Save as Preset", "保存并关闭": "Save and Close",
    "保存失败": "Save failed", "另存": "Save As",
    "导出": "Export", "导入": "Import", "打印": "Print", "退出": "Quit",
    "关闭窗口": "Close Window", "关闭": "close", "关闭前保存": "Save before closing",
    "项目": "project", "项目名": "project name", "项目目录": "project directory",
    "文档标题": "document title", "文档已有": "Document already has",
    "模板": "template", "从模板新建": "New from Template",
    "最近项目": "Recent Projects", "我的项目": "My Projects",
    "还没有项目": "No projects yet", "移除记录": "Remove record",
    "选择项目位置": "Choose project location",
    "选择项目保存目录": "Choose a directory to save the project",
    "磁盘": "disk", "内存": "memory", "快照": "snapshot",
    "自动保存": "Autosave", "自动快照": "auto snapshot",
    "恢复快照": "Restore snapshot", "恢复默认": "Restore Defaults",
    "崩溃恢复": "Crash Recovery", "外部改动": "external changes",
    "外部已改": "changed externally", "冲突": "conflict",
    "对比并合并": "Compare and Merge", "统一视图": "unified view",
    "差异": "diff", "已重载": "reloaded", "已自动保存": "autosaved",
    "分钟前": "min ago", "秒前": "s ago", "小时前": "h ago",
    "刚刚": "just now", "多久前": "how long ago",
    # ── 命令/操作动词 ──
    "撤销": "Undo", "重做": "Redo", "重复": "Repeat", "再次变换": "Transform Again",
    "上一次变换": "last transform", "全选": "Select All",
    "反向选择": "Invert Selection", "选择相同": "Select Same",
    "选择同类": "Select Similar", "选择所有实例": "Select All Instances",
    "复制": "Copy", "剪切": "Cut", "粘贴": "Paste", "删除": "Delete",
    "移除": "Remove", "清除": "Clear", "替换": "Replace",
    "重命名": "Rename", "改名": "rename", "锁定": "Lock", "解锁": "Unlock",
    "隐藏其他": "Hide Others", "锁定其他": "Lock Others",
    "显示(取消隐藏)": "Show (unhide)", "已锁定": "locked",
    "未锁定": "unlocked", "已隐藏": "hidden", "已显示全部": "showed all",
    "已解锁全部": "unlocked all", "已全选": "selected all",
    "上移": "Move Up", "下移": "Move Down", "前移": "Bring Forward",
    "后移": "Send Backward", "置于顶层": "Bring to Front",
    "置于底层": "Send to Back", "交换": "swap", "分布": "distribute",
    "等距": "equidistant", "等间隙": "equal gap", "交换目标": "swap target",
    "对齐到": "align to", "对齐": "align", "重排": "rearrange",
    "重新排列": "Rearrange", "横竖互换": "swap width/height",
    "适配内容": "fit content", "几何取整": "round geometry",
    "复位到中心": "reset to center", "停止并回零": "stop and reset to zero",
    "跳到主件": "Go to Master", "还原": "revert", "恢复": "restore",
    "覆盖": "override", "重置覆盖": "reset overrides",
    "分离实例": "Detach Instance", "创建组件": "Create Symbol",
    "替换主件定义": "Replace Master Definition", "编辑主件": "edit master",
    "编辑对象": "Edit object", "编辑文本": "Edit text",
    "提交": "Commit", "确认": "confirm", "确定": "OK", "取消": "Cancel",
    "应用": "Apply", "重试": "Retry", "跳过": "Skip", "丢弃": "Discard",
    "刷新": "Refresh", "重启": "Restart", "卸载": "Uninstall",
    "启用": "Enable", "禁用": "Disable", "临时禁用": "temporarily disable",
    "安装": "Install", "装载": "load", "授权": "authorization",
    "权限": "permissions", "权限告知": "permission notice",
    "只读": "read-only", "受控": "controlled",
    # ── 面板动词短语(冒号后状态)──
    "已打开": "opened", "已关闭": "closed", "已显示": "shown",
    "已隐藏": "hidden", "已折叠": "collapsed", "已恢复": "restored",
    "已清除": "cleared", "已创建": "created", "已删除": "deleted",
    "已取消": "cancelled", "已提交": "committed", "已应用": "applied",
    "已建立": "created", "已释放": "released", "已进入": "entered",
    "已退出": "exited", "已切换": "switched", "已转换": "converted",
    "已替换": "replaced", "已移除": "removed", "已添加": "added",
    "已复制": "copied", "已保存": "saved", "已写回": "written back",
    "已重命名": "renamed", "已更新": "updated", "已还原": "reverted",
    "已拒绝": "rejected", "已跳过": "skipped", "已忽略": "ignored",
    "已回退": "undone", "已重做": "redone", "已重启": "restarted",
    "已停止": "stopped", "已启动": "started", "已装载": "loaded",
    "已安装": "installed", "已授权": "authorized", "已适配": "fitted",
    "已落盘": "written to disk", "已收编": "adopted", "已取色": "picked color",
    "已吸取": "picked styles", "已撤销": "undone",
    # ── 名词杂项 ──
    "关键帧": "keyframe", "轨道": "track", "播放头": "playhead",
    "播放": "play", "暂停": "pause", "预览": "preview", "帧": "frame(s)",
    "动画": "animation", "缓动": "easing", "时长": "duration",
    "延迟": "delay", "循环播放": "loop", "时间轴": "Timeline",
    "历史": "history", "历史跳转": "history jump", "步": "step(s)",
    "令牌": "tokens", "设计令牌": "design tokens",
    "变量": "variable", "值": "value", "单位": "unit", "标尺": "rulers",
    "长度单位": "length unit", "像素": "pixel", "物理像素": "physical pixel",
    "世界坐标": "world coords", "度量": "Measure", "距离": "distance",
    "位移": "offset", "尺寸": "size", "大小": "size", "宽度": "width",
    "高度": "height", "位置": "position", "角度": "angle", "半径": "radius",
    "间距": "gap", "边距": "margin", "内边距": "padding", "外边距": "margin",
    "倍率": "scale", "因子": "factor", "次数": "count", "阶数": "order",
    "网格间距": "grid spacing", "网格基础间距": "grid base spacing",
    "输出模式": "output mode", "单文件": "single file", "外链": "external link",
    "封面图": "cover image", "画板背景": "artboard background",
    "画板预设": "Artboard Presets", "画板数": "artboard count",
    "兜底尺寸": "fallback size", "预设": "preset", "自定义": "Custom",
    "纵向": "portrait", "横向": "landscape", "竖": "portrait", "横": "landscape",
    "垂直": "vertical", "水平": "horizontal", "居中": "center",
    "左": "left", "右": "right", "上": "top", "下": "bottom",
    "顶": "top", "底": "bottom", "左侧": "left side", "右侧": "right side",
    "顶部": "top", "底部": "bottom", "左上": "top-left", "右上": "top-right",
    "左下": "bottom-left", "右下": "bottom-right", "左中": "middle-left",
    "右中": "middle-right", "上中": "top-center", "下中": "bottom-center",
    "居中对齐": "align center", "左对齐": "align left", "右对齐": "align right",
    "两端对齐": "justify", "末行": "last line", "强制撑满": "force justify",
    "水平等距": "distribute horizontally", "垂直等距": "distribute vertically",
    "水平等间隙": "equal horizontal gap", "垂直等间隙": "equal vertical gap",
    "世界": "world", "本地": "local", "参照系": "reference frame",
    "方向": "direction", "反向": "reverse", "取向": "orientation",
    "线性": "linear", "径向": "radial", "角度": "angle",
    # ── 工具提示常用 ──
    "先选中对象": "select an object first",
    "未选中对象": "No object selected",
    "没有画板": "no artboard", "没有对象": "no objects",
    "还没有对象": "No objects yet",
    "先保存或打开一个项目": "save or open a project first",
    "可撤销": "undoable", "不可恢复": "irreversible",
    "一次撤销": "one undo", "一条撤销": "one undo",
    "点击": "click", "点击查看": "click to view", "点击定位": "click to locate",
    "按住": "hold", "按住拖动": "drag while holding",
    "Shift 约束": "Shift constrains", "Alt 从对象中心": "Alt from object center",
    "Esc 退出": "Esc to exit", "Esc 取消": "Esc to cancel",
    "Ctrl+Z 可撤销": "Ctrl+Z to undo", "Ctrl+Z 撤销": "Ctrl+Z to undo",
    "回选择工具": "back to Select tool",
    "计划于 v2": "planned for v2", "计划 v2": "planned for v2",
    "暂不支持": "not yet supported", "不支持": "unsupported",
    "原样保留": "kept as-is", "近似": "approximate", "近似渲染": "approximate rendering",
    "降级": "degraded", "降级渲染": "degraded rendering",
    "无 GPU": "no GPU", "不可用": "unavailable", "可用": "available",
    "失败": "failed", "成功": "succeeded", "完成": "done",
    "中": "in progress", "进行中": "in progress", "就绪": "Ready",
    "未发现": "none found", "存在": "exists", "不存在": "missing",
    "已存在": "already exists", "为空": "empty", "缺失": "missing",
    "未知": "unknown", "无效": "invalid", "合法": "valid",
    "超长": "too long", "过短": "too short", "越界": "out of range",
    # ── 插件 / 权限 ──
    "插件": "plugin", "插件清单": "plugin manifest",
    "零权限": "zero permissions", "宿主命令": "host command",
    "注册面板": "registered panel", "注册导出动作": "registered export action",
    "外部进程": "external process", "原生进程": "native process",
    "独立进程": "separate process", "启动失败": "start failed",
    "装载失败": "load failed", "授权失败": "authorization failed",
    "未授权": "unauthorized", "已授权": "authorized",
    # ── 无障碍 / 体检 ──
    "无障碍": "Accessibility", "可访问名": "accessible name",
    "对比度": "contrast", "缺 alt": "missing alt",
    "失效链接": "broken link", "未使用资产": "unused assets",
    "缺失资源": "missing assets", "缺失字体": "missing fonts",
    "超长文件": "oversize files", "重名资产过多": "too many duplicate asset names",
    "体检": "health check", "发现": "found", "未发现问题": "no issues found",
    "建议处理": "recommended", "提示项": "advisory",
    # ── 浏览器校对 ──
    "开始校对": "Start Proof", "画布侧": "canvas side",
    "浏览器侧": "browser side", "等待画布帧": "waiting for canvas frame",
    "渲染中": "rendering", "采样中": "sampling",
    "差异热力图": "diff heatmap", "差异分数": "diff score",
    "滑块对比": "slider compare", "无分数": "no score",
    "不产出分数": "no score produced", "不假绿": "no fake green",
    # ── 状态栏 / 自动化 ──
    "脏": "dirty", "计时中": "timing", "与磁盘一致": "in sync with disk",
    "与磁盘不同": "differs from disk", "磁盘版": "disk version",
    "保存项目": "Save Project",
    # ── 杂项连接词 ──
    "同时": "at the same time", "然后": "then", "之后": "after",
    "之前": "before", "再次": "again", "重新": "re-",
    "当前": "current", "目前": "currently", "现在": "now",
    "已经": "already", "仍": "still", "都": "all", "均": "all",
    "不": "not", "无": "no", "没有": "no", "还没有": "not yet",
    "可以": "can", "无法": "cannot", "需要": "need", "必须": "must",
    "请": "please", "即将": "about to", "将": "will", "已": "already",
    "被": "was", "是": "is", "为": "as", "与": "with", "和": "and",
    "或": "or", "在": "in", "从": "from", "到": "to", "向": "toward",
    "用": "use", "使用": "use", "经": "via", "通过": "via",
    "后": "after", "前": "before", "时": "when", "中": "in",
    "个": "", "条": "", "项": "item(s)", "块": "block(s)", "步": "step(s)",
    "份": "copy(ies)", "秒": "s", "分钟": "min", "行": "line(s)",
    "字": "chars", "字符": "chars", "点": "point", "段": "segment",
    "约": "~", "至少": "at least", "最多": "at most", "恰好": "exactly",
    "至少保留": "keep at least", "至少需要": "need at least",
    "只支持": "only supports", "仅支持": "only supports",
    "只影响": "only affects", "只读": "read-only",
    "默认": "default", "手动": "manual", "自动": "auto",
    "严格": "strict", "宽松": "loose", "智能": "smart",
    "一般": "normal", "通用": "general", "常用": "common",
    "内置": "built-in", "外部": "external", "本地": "local",
    "全局": "global", "公共": "common", "私有": "private",
    "简短": "brief",
}
