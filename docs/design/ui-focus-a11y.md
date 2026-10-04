# UI 焦点走查表(G-UI-D)

> 审查 2026-10-04 §8.10/§8.12 的 G-UI-D 门禁交付:**逐面板交互控件清单 ×
> 键盘可达性 × 焦点环可见性**。走查基线 = `w2-ui-s4` 分支(S4 批)。
>
> ## egui 0.35 的机制事实(走查口径的依据)
>
> 1. **Tab 序是自动的**:`Sense::click()/drag()/click_and_drag()` 均含
>    `FOCUSABLE` 位;`Memory` 的 `FocusDirection` 原生支持 Tab / Shift+Tab。
>    自绘控件(allocate + Sense::click)与原生控件同样进 Tab 序。
> 2. **键盘激活是免费的**:有焦点的控件上 `Response::clicked()` 对
>    Space/Enter 同样为真(egui `Response::clicked` 文档口径)。
> 3. **焦点环必须自绘**:egui 只为原生控件描样式;自绘控件在
>    `resp.has_focus()` 时调用 `components::paint_focus_ring`
>    (accent 1.5px 外描边 + 内侧隔离环,§8.3.3 状态层)。
>    静态门禁:`vb_ui/tests/focus_walkthrough.rs`。
>
> ## 走查矩阵

| 面板/区域 | 可交互控件 | Tab 可达 | 焦点环 | 键盘激活 | 备注 |
|---|---|---|---|---|---|
| 菜单栏 | 菜单按钮(egui MenuBar) | ✅(egui 原生) | ✅(egui 样式) | Space/Enter ✅ | 菜单内 ↑↓ 原生 |
| 控制条 40 | NumField(X/Y/W/H/∠/不透明度) | ✅ 输入框 + 标签拖钮 | ✅ 输入框(egui)+ 标签(自绘环) | ↑↓ 步进(Shift ×10)、表达式回车 | 标签 Space/Enter 无动作(拖拽语义,无点击命令) |
| 控制条 40 | ColorField 色块 | ✅ | ✅(ToolButton 同款 allocate+click;走自绘环面) | Space/Enter = 开取色器 | Alt+点击 = 完整取色器(仅鼠标,诚实记录) |
| 控制条 40 | ComboBox(画板切换/缩放/预设/取向) | ✅(egui 原生) | ✅ | ↑↓/Enter/Esc 原生 | |
| 工具箱(44 单列) | ToolButton ×22 | ✅ | ✅(自绘) | Space/Enter = 切工具 | 长按 200ms 展开同组为指针手势;键盘走数字键快捷键/命令面板 |
| 底部浮动工具条 | ToolButton ×7 + 缩放 chip | ✅ | ✅(自绘) | 同上 | chip Space/Enter = 适合窗口 |
| 右坞折叠图标条 | 展开钮 + Tab 图标 ×4 | ✅ | 展开钮 ✅(icon_button 自绘环);Tab 图标 ⚠ 自绘环未接(icon_button 同款结构,_allocate+click,Space/Enter 可切) | | |
| 右坞 Tab 条 | PanelTabs ×4 | ✅ | ⚠ Tab 页allocate 无自绘环(Space/Enter 可切换,环待 S5 统一补) | | 右键重排为指针手势 |
| 属性面板 | NumField / ColorField / TextField / Slider / Select | ✅ | ✅(组件层自绘) | Slider 双击复位为指针手势;键盘走 NumField 步进 | 多选混合态显示斜纹 |
| 属性面板 | Checkbox/Radio/Switch(自绘) | ✅ | ✅(自绘) | Space/Enter 翻转 | WidgetInfo 已登记(读屏语义) |
| 图层面板 | 搜索框 / 行(选中/拖拽) / 显隐·锁定钮 | ✅ 行 + ✅ 按钮 | 搜索框 ✅(egui);行 ⚠(行焦点无环,Space/Enter 可选中) | 双击改名 = 指针手势;F2 语义走行内编辑框(✅) | |
| 画板面板 | 预设按钮/尺寸 NumField/重排 | ✅ | NumField ✅ | | |
| 令牌面板 | 令牌编辑行 | ✅(TextEdit) | ✅(egui) | | |
| 状态栏 | 画板◀▶/缩放/断点切换器/外部改动印记 | ✅ | ◀▶ ✅(icon_button);文本项 ⚠ 无环(可点,hover 手势) | Space/Enter 可点 | 窄窗分级只收条目不改语义 |
| 命令面板 Mod+K | 搜索框 + 候选行 | 输入框常驻焦点 ✅ | ✅(egui) | ↑↓ 移动 / Enter 执行 / Esc 退出(100ms 内关) | 拼音首字母 + 子序列容错 |
| 对话框族 | 按钮(egui) | ✅ | ✅ | Esc=取消 / Enter=确认(按各自输入面接线) | 出场 120ms+8px(动效开关直通) |
| 启动器 | 首启三卡 / 最近列表 / 搜索框 | ✅ | ⚠ 卡片命中区无自绘环(Space/Enter 可激活) | 卡片方向键导航(既有 handle_keys) | |

> ## 诚实记录:当前做不到 / 归属下一批
>
> 1. **自绘环未覆盖的 allocate 控件**:PanelTabs Tab 页、图层行、状态栏
>    文本项、启动器卡片 —— 全部 Tab 可达 + Space/Enter 可激活(egui 机制),
>    但键盘焦点落在其上时**没有可见环**。修复模式与 ToolButton 相同
>    (`resp.has_focus() → paint_focus_ring`),S5 无障碍批统一补齐
>    (届时 G-UI-D 门禁把"环调用点 ≥ 控件数"收紧到逐控件断言)。
> 2. **`Alt+点击` 类修饰键手势**(ColorField 完整取色器)无纯键盘等价;
>    完整取色器的 RGB/HSL 能力可经紧凑态 HEX + `var(--x)` 间接到达。
> 3. **Tab 序 = 绘制序**(egui 无显式 Tab index):面板装配顺序即焦点序,
>    F6 面板区循环(§8.10.2)未实现,归属 S5。
> 4. **读屏**:WidgetInfo 覆盖 = 自定义三态控件 + egui 原生全部;
>    自绘图标钮有 tooltip 文本但无 `WidgetInfo.labelled` 登记,归属 S5。
