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

# ── 新宿主 vb_kit / vb_shell(R0 补缺批;G-UI3:渲染路径禁裸文案)──
# 手工维护段:在 gen_cmd_ftl.py 的 BEGIN/END 生成块之外,重跑生成器不影响;
# key 规范:`ui-<面板>-<语义>`,连字符。en 遵守 CONTEXT.md 术语表。

# 能力台账面板(过滤 chips)
ui-cap-filter-all = 全部
ui-cap-filter-done = 已落地
ui-cap-filter-partial = 部分
ui-cap-filter-dropped = 不做
ui-cap-filter-agent-only = 仅 Agent 可复现
# 能力台账面板(计数行 / 空态 / 页脚 / Agent 列)
ui-cap-counts = 共 { $total } 条 · 已落地 { $done } · 部分 { $partial } · 不做 { $dropped }
ui-cap-empty = 无匹配条目(当前过滤组合)
ui-cap-footer = 数据源:vb_session::capabilities(单一真相) · 三态:已落地 / 部分+去向 / 不做
ui-cap-agent-repro = 可复现 ×{ $n }
# 能力台账面板(三态徽章;Planned 由门禁保证为空,仍保留兜底)
ui-cap-badge-done = 已落地
ui-cap-badge-partial = 部分
ui-cap-badge-planned = 计划
ui-cap-badge-dropped = 不做
# 壳(vb_shell)
ui-shell-window-title = VellumBench · sable 预览宿主
ui-shell-panel-capabilities = 能力台账
ui-shell-panel-canvas = 画布(R1)
ui-shell-canvas-placeholder = 画布(R1)— 画布上屏通道按 ADR-0047 裁定后接管
ui-shell-canvas-subtitle = R0 预览宿主:窗口 / 主题 / dock 布局骨架

# ── BEGIN cmd-catalog(由 tools/gen_cmd_ftl.py 生成;勿手改)──
cmd-file-new = 新建项目…(对话框,新窗口打开)
cmd-file-open = 打开项目…
cmd-file-save = 保存
cmd-file-export-dialog = 导出…
cmd-file-export-repeat = 上次导出(当前画板 PNG @2x)
cmd-app-quit = 退出
cmd-edit-undo = 撤销
cmd-edit-redo = 重做
cmd-edit-select-all = 全选(当前画板)
cmd-edit-copy = 复制
cmd-edit-cut = 剪切
cmd-edit-paste = 粘贴
cmd-edit-paste-in-place = 贴在前面(就地)
cmd-object-group = 编组
cmd-object-ungroup = 取消编组
cmd-object-transform-again = 再次变换
cmd-object-bring-forward = 前移一层
cmd-object-bring-to-front = 置于顶层
cmd-object-send-backward = 后移一层
cmd-object-send-to-back = 置于底层
cmd-object-delete = 删除对象
cmd-object-lock = 锁定所选
cmd-object-unlock-all = 解锁全部
cmd-object-hide = 隐藏所选
cmd-object-show-all = 显示全部
cmd-align-left = 水平左对齐
cmd-align-hcenter = 水平居中对齐
cmd-align-right = 水平右对齐
cmd-align-top = 垂直顶对齐
cmd-align-vcenter = 垂直居中对齐
cmd-align-bottom = 垂直底对齐
cmd-path-union = 路径查找器:联集
cmd-path-subtract = 路径查找器:减去顶层
cmd-path-intersect = 路径查找器:交集
cmd-path-xor = 路径查找器:差集
cmd-object-distribute-h = 水平等距分布
cmd-object-distribute-v = 垂直等距分布
cmd-view-zoom-in = 放大
cmd-view-zoom-out = 缩小
cmd-view-fit = 适合窗口
cmd-view-actual-size = 实际大小 100%
cmd-view-outline = 轮廓模式(线框)
cmd-view-toggle-grid = 显示 / 隐藏网格
cmd-view-toggle-smart-guides = 智能参考线开关
cmd-view-toggle-theme = 深色 / 浅色主题
cmd-tool-select = 选择工具
cmd-tool-rect = 矩形工具
cmd-tool-ellipse = 椭圆工具
cmd-tool-line = 直线工具
cmd-tool-pen = 钢笔工具
cmd-tool-direct-select = 直接选择工具
cmd-tool-zoom = 缩放工具
cmd-tool-hand = 抓手工具
cmd-tool-text = 文字工具
cmd-tool-eyedropper = 吸管工具
cmd-tool-artboard = 画板工具
cmd-tool-gradient = 渐变工具
cmd-tool-scissors = 剪刀工具
cmd-tool-group-select = 编组选择工具
cmd-tool-rotate = 旋转工具(单击设中心,拖拽旋转;Shift 15°)
cmd-tool-mirror = 镜像工具(单击设中心,拖动定镜像轴)
cmd-tool-scale = 缩放工具(单击设中心,拖拽缩放;Shift 等比)
cmd-tool-free-transform = 自由变换工具(拖选区四角,对角锚定)
cmd-tool-pencil = 铅笔工具(自由绘制,按保真度抽稀为路径)
cmd-tool-curvature = 曲率工具(点击矢量路径自动拟合平滑控制点)
cmd-edit-pencil-fidelity = 设置 → 铅笔保真度(档位循环 1–16px)
cmd-tool-slice = 切片工具(Shift+K 拖框建立 data-vb-slice 切片)
cmd-object-slice-from-selection = 切片 → 从选区建立
cmd-file-place-image = 置入图像…(选文件入 assets/,选中设 src 或新建 img)
cmd-object-replace-image = 替换图像…(保持几何,SetImageSrc 可撤销)
cmd-view-pixel-preview = 像素预览(缩放 ≥8× 对齐物理像素网格)
cmd-tool-measure = 度量工具(拖动量距,单击标注对象尺寸)
cmd-canvas-cancel = 取消 / 清空选区
cmd-canvas-pen-finish = 钢笔:结束路径
cmd-canvas-nudge-left = 微移左 1px
cmd-canvas-nudge-right = 微移右 1px
cmd-canvas-nudge-up = 微移上 1px
cmd-canvas-nudge-down = 微移下 1px
cmd-view-toggle-rulers = 显示 / 隐藏标尺
cmd-view-toggle-guides = 显示 / 隐藏参考线
cmd-view-lock-guides = 锁定参考线
cmd-view-guides-from-selection = 从选区生成参考线
cmd-app-command-palette = 命令面板
cmd-view-next-artboard = 下一画板
cmd-view-prev-artboard = 上一画板
cmd-view-next-panel-tab = 切换右侧面板 Tab
cmd-view-zoom-to-selection = 缩放到选区
cmd-app-about = 关于
cmd-view-toggle-layers-panel = 图层面板显隐
cmd-view-toggle-all-panels = 隐藏 / 恢复所有面板
cmd-view-toggle-char-panel = 字符面板显隐
cmd-view-toggle-para-panel = 段落面板显隐
cmd-tool-text-cycle-mode = 文字工具:点/区域循环
cmd-view-toggle-appearance-panel = 外观面板显隐
cmd-view-toggle-stroke-panel = 描边面板显隐
cmd-view-toggle-gradient-panel = 渐变面板显隐
cmd-view-toggle-opacity-panel = 透明度面板显隐
cmd-view-toggle-color-panel = 颜色面板显隐
cmd-color-toggle-target = 颜色:切换填充/描边
cmd-color-swap-fill-stroke = 颜色:交换填充与描边
cmd-color-default-fill-stroke = 颜色:恢复默认填色/描边色
cmd-file-close = 关闭窗口(文档)
cmd-file-import-html = 导入 HTML…
cmd-file-doc-settings = 文档设置…(项目名/输出模式/网格与参考线,应用后生效)
cmd-file-resolve-conflict = 对比并合并…(外部改动三方对比:磁盘/内存/自动快照)
cmd-file-print = 打印…(当前画板 → 临时 PDF → 系统程序打开)
cmd-edit-preferences = 首选项…(九分类;改动即时生效)
cmd-edit-keyboard-shortcuts = 键盘快捷键…(键位方案编辑器,方案存 keymap.json)
cmd-object-clip-mask = 建立剪切蒙版
cmd-object-release-clip-mask = 释放剪切蒙版
cmd-object-outline-stroke = 轮廓化描边
cmd-text-upper-case = 更改大小写 → 大写
cmd-text-lower-case = 更改大小写 → 小写
cmd-text-create-outlines = 创建轮廓
cmd-text-find-font = 查找字体…(缺失字体检测与替换,可撤销)
cmd-select-inverse = 选择反向
cmd-select-next-object = 选择上方的下一个对象
cmd-select-prev-object = 选择下方的下一个对象
cmd-select-same-fill = 选择相同填充色
cmd-select-same-stroke = 选择相同描边色
cmd-select-same-stroke-width = 选择相同描边粗细
cmd-select-all-text = 选择全部文本对象
cmd-select-all-locked = 选择所有锁定对象
cmd-select-all-hidden = 选择所有隐藏对象
cmd-effect-repeat-last = 应用上一个效果
cmd-effect-drop-shadow = 效果:投影
cmd-effect-inner-shadow = 效果:内阴影
cmd-effect-outer-glow = 效果:外发光
cmd-effect-inner-glow = 效果:内发光
cmd-effect-round-corners = 效果:圆角
cmd-effect-gaussian-blur = 效果:高斯模糊
cmd-effect-feather = 效果:羽化
cmd-effect-distort = 扭曲和变换…
cmd-view-hide-edges = 隐藏边缘
cmd-view-browser-proof = 浏览器校对…
cmd-window-workspace-basic = 工作区:基本功能
cmd-window-workspace-type = 工作区:排版
cmd-window-workspace-export = 工作区:导出
cmd-window-new-workspace = 新建工作区…(保存当前布局为命名预设,可切换/删除)
cmd-window-tab-properties = 面板坞:属性
cmd-window-tab-layers = 面板坞:图层
cmd-window-tab-artboards = 面板坞:画板
cmd-window-tab-tokens = 面板坞:令牌
cmd-help-shortcuts = 键位速查表…
cmd-help-check-update = 检查更新…
cmd-help-capabilities = 能力台账(做了什么/没做什么)
cmd-view-dock-toolbar-top = 工具箱:停靠到顶部
cmd-view-dock-toolbar-left = 工具箱:停靠到左侧
cmd-view-dock-toolbar-right = 工具箱:停靠到右侧
cmd-view-dock-toolbar-bottom = 工具箱:停靠到底部
cmd-view-toolbar-columns-1 = 工具箱:单列
cmd-view-toolbar-columns-2 = 工具箱:双列
cmd-path-merge = 路径查找器:合并
cmd-path-subtract-back = 路径查找器:减去后方对象
cmd-path-crop = 路径查找器:裁剪
cmd-path-divide = 路径查找器:分割
cmd-path-trim = 路径查找器:修边
cmd-path-outline = 路径查找器:轮廓
cmd-view-toggle-transform-panel = 变换面板显隐
cmd-view-toggle-align-panel = 对齐面板显隐
cmd-align-to-selection = 对齐到:选区
cmd-align-to-key-object = 对齐到:关键对象
cmd-align-to-artboard = 对齐到:画板
cmd-object-distribute-hspace = 水平等间隙分布
cmd-object-distribute-vspace = 垂直等间隙分布
cmd-file-home = 主页…(打开主页窗口)
cmd-home-new-project = 主页:新建项目
cmd-home-open-project = 主页:打开项目…
cmd-home-new-from-template = 主页:从模板新建
cmd-home-open-selected = 主页:打开选中的最近项目
cmd-home-remove-selected = 主页:移除选中的最近记录
cmd-home-select-next = 主页:选中下一项
cmd-home-select-prev = 主页:选中上一项
cmd-home-pin-selected = 主页:固定/取消固定选中项
cmd-home-search = 主页:搜索过滤最近项目
cmd-home-restore-session = 主页:恢复上次会话
cmd-home-capabilities = 主页:能力台账
cmd-view-developer-stats = 开发者统计(调试数据默认隐藏)
cmd-view-toggle-hints = 提示条(操作提示/入门教学,可关)
cmd-view-toggle-motion = 界面动效(淡入/过渡,可关;首选项「常规」同款开关)
cmd-view-ui-scale-up = 界面缩放:增大一档
cmd-view-ui-scale-down = 界面缩放:减小一档
cmd-view-ui-scale-reset = 界面缩放:复位 100%
cmd-edit-toggle-unsupported-tools = 工具 → 显示未支持工具
cmd-edit-autosave-interval = 设置 → 自动保存间隔(关/30/60/120/300 档位循环)
cmd-view-toggle-history-panel = 历史面板显隐(撤销历史可跳转)
cmd-file-health-check = 项目健康检查…
cmd-view-toggle-assets-panel = 资产面板显隐(assets/ 清单 + 引用关系 + 定位/替换)
cmd-view-breakpoint-cycle = 断点预览:循环切换(默认 → 各断点)
cmd-style-state-toggle = 属性面板状态:正常 / 悬停(hover)
cmd-object-symbol-create = 创建组件(选中提升为主件,当前位置变为首个实例)
cmd-object-symbol-detach = 分离实例(实例转普通元素,不再随主件同步)
cmd-object-symbol-reset-overrides = 重置覆盖(实例还原为主件当前内容)
cmd-object-symbol-swap-main = 替换主件定义(以实例当前内容为新定义并同步其余实例)
cmd-object-symbol-select-instances = 选择所有实例(同主件)
cmd-view-toggle-timeline-panel = 时间轴面板(关键帧轨道 + 播放预览)
cmd-anim-play-toggle = 动画预览:播放 / 暂停
cmd-anim-stop = 动画预览:停止并回零
cmd-anim-loop-toggle = 动画预览:循环开 / 关
cmd-anim-keyframe-add = 在播放头处加关键帧(选中对象;值取静态值)
cmd-anim-keyframe-delete = 删除选中的关键帧
cmd-anim-clear = 清除对象动画(移除 @keyframes 与 animation)
cmd-edit-plugins = 插件管理…(安装/授权/启停/日志/重启;插件 = 外部进程,默认零权限)
cmd-view-toggle-plugins-panel = 插件面板显隐(Running 插件的注册面板,受控 UI)
# ── END cmd-catalog ──

# ── vb_shell(R0 启动器/命令行;手工维护段;单行消息——门禁解析器只认单行)──
ui-shell-usage = vellum-sable — VellumBench 新宿主预览壳(R0)| 用法: vellum-sable 先开启动器窗口 · --project <目录> 直达项目窗口(ADR-0033)· <项目目录> 同 --project · --help | --version
ui-shell-project-arg-required = vellum-sable: --project 需要一个项目目录参数
ui-shell-arg-unknown = vellum-sable: 未知参数 {$arg}(--help 查看用法)
ui-shell-project-dir-missing = vellum-sable: 项目目录不存在
ui-shell-launcher-title = VellumBench · 启动器
ui-shell-launcher-search-placeholder = 搜索名称或路径…
ui-shell-launcher-invalid-path = 路径已失效
ui-shell-launcher-empty = 还没有项目
ui-shell-launcher-empty-hint = 用 vellum-sable --project <目录> 打开第一个项目;打开后自动进入最近列表
ui-shell-launcher-no-match = 没有匹配「{$query}」的项目
ui-shell-launcher-footer = ↑↓ 选择 · Enter 打开 · R 移除记录 · 单击行打开
