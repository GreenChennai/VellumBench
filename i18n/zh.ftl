# Vellum Bench UI 文案(中文,基准语言)
# 05-7 i18n 骨架:本文件是 UI 文案单一来源之一(与 en.ftl 键集合必须一致,
# 由 vb_app::i18n 的门禁测试校验)。格式:纯 `key = value` 行,`#` 开头为注释。
# 覆盖面(诚实声明):**仅覆盖本批新增的对话框 / 菜单 / 面板文案**
# (断点切换器、断点编辑、伪类状态、首选项常规页语言行、文档设置断点行);
# 既有界面文案仍在代码内中文硬编码,全量抽取不现实,留后续批次。

# ── 首选项 · 常规页 ──
prefs.ui-language = 界面语言
prefs.ui-language-help = 骨架覆盖:仅本批新增的对话框与菜单跟随此语言;其余界面仍为中文,后续批次逐步抽取

# ── 断点(响应式)──
bp.switcher = 断点
bp.default = 默认
bp.edit-banner = 断点覆盖编辑
bp.edit-note = 当前断点下仅支持:宽/高/位置/显隐/字号;其余属性请切回「默认」画布编辑(导出为对应 @media 块)
bp.unsupported = 当前断点不支持该项
bp.doc-settings = 断点(px,逗号分隔;空 = 无)
bp.doc-settings-help = 保存后写入 index.html 的 vb-breakpoints meta;状态栏可切换预览宽度
bp.status-hint = 断点预览:画布以预览带标出断点宽度区域,内容仍按默认样式渲染;覆盖样式以「视图 → 浏览器校对」为准

# ── 伪类状态(最小闭环)──
state.label = 状态
state.normal = 正常
state.hover = 悬停 (hover)
state.hover-note = hover 最小闭环:填充/字色/不透明度/字号/显示,落 selector:hover 规则;画布不模拟 hover,以「视图 → 浏览器校对」为准
