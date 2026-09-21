# ADR-0027: 菜单栏采用 AI 规范 9 项,「关于」归「帮助」

- 状态:已采纳(2026-09-20 用户拍板 V1 按推荐;2026-09-21 落地)
- 背景:菜单栏此前只有 5 项(文件/编辑/对象/视图/帮助);用户原始清单为
  "文件/编辑/文字/选择/效果/视图/窗口/帮助/关于"(含独立「关于」、**无「对象」**)。
  AI 规范为"文件/编辑/**对象**/文字/选择/效果/视图/窗口/帮助",「关于」在「帮助」内。
- 决策:
  1. 采用 AI 规范 **9 项**,「关于」**归「帮助」**(顶层不另设"关于");
  2. 菜单结构是**数据**:标题取 `shortcuts::MENU_TITLES`(9 项定长数组),
     条目取 `shortcuts::MENUS`,渲染层(`app/menus.rs`)只按注册表渲染;
  3. **键位文本一律查注册表**(`key_text_for`),**禁止在 label 里手写**;
  4. 未落地项**保留在菜单里但置灰**,悬停即见「计划于 vX」
     (`shortcuts::PLANNED`);这些 id **同样注册为命令**,因此经命令面板 /
     Agent 调用时走**同一句中文提示**,不静默。
- 实现:`crates/vb_app/src/shortcuts.rs`(结构 + 计划表 + 门禁)、
  `crates/vb_app/src/app/menus.rs`(渲染)、
  `crates/vb_app/src/app/menu_commands.rs`(42 条新命令的实现,27 实现 / 15 计划),
  `commands.yaml` + `crates/vb_app/tests/commands_yaml.rs` 三方对拍。
- 验证(2026-09-21 机器实测):
  - `menu_bar_has_nine_ai_standard_menus`:9 个标题逐项相等、每菜单非空、
    条目 id 带域前缀、「关于」不在顶层、`app.about` 在帮助内;
  - `planned_items_are_registered_and_reasoned`:15 条计划项均已注册、
    说明含「计划于 v」、且都在某个菜单里;
  - `commands_yaml_matches_registry`:id 集合 / 顺序 / label / 键位逐条相等;
  - `menu_select_doc.rs` 7 例:选择类判据的文档状态级断言。
- 理由:「对象」菜单是编组/排列/路径/蒙版的唯一归宿,缺了它这些能力无家可归;
  「关于」放帮助是 AI 的既定位置,零学习成本。
- 后果:**每条菜单项必须有命令 ID**(`menu_items_are_implemented` 门禁);
  计划项落地时须从 `PLANNED` 移除并替换 `run_menu_command` 的分支。
- 关联:副文档 06;`06a` 报告;ADR-0010(HTML 源格式,决定菜单里的导入导出语义)。
