# ADR-0029: 布局持久化到 workspace.json(损坏回退 + 告警)

- 状态:已采纳(2026-09-21;副文档 07-2 落地)
- 背景:`design/11 v0.9` 已规划"工作区布局保存";目标⑥的工具栏停靠位、
  单双列需要持久化,否则每次重启都回到默认。`design/03 §八` 还规划了
  首选项九分类与键位方案编辑器(后两者仍未做,见能力台账 `09-L`)。
- 决策:
  1. 新增 `workspace.json`,落在配置目录
     (`VB_WORKSPACE` 环境变量 → `%APPDATA%\VellumBench\`(Windows)/
     `$XDG_CONFIG_HOME|$HOME/.config/vellum-bench/`);
  2. 字段:工具栏停靠位 + 列数 + 面板坞折叠态 + **面板 Tab 顺序** +
     当前 Tab + 「隐藏所有面板」态;带 `schema_version`;
  3. **损坏 / 版本不符 → 回退默认 + 中文告警**(写日志与状态栏),**不静默**;
  4. 文件**不存在不是错误**(首次运行静默用默认);
  5. 写入**原子**:先写 `*.json.tmp` 再 `rename`,不留半截 JSON;
  6. 写通策略:`ui()` 每帧做**逐字段脏检查**(不构造 Vec),有变化才落盘;
     布局相关命令(停靠/列数/工作区预设)则立即落盘。
- 实现:`crates/vb_app/src/app/dock_layout.rs`(`WorkspaceConfig` /
  `config_path` / `load_from` / `save_to` / `normalize`)、
  `crates/vb_app/src/app/toolbar.rs`(`workspace_config` / `save_workspace` /
  `workspace_dirty`)、`VellumApp::new` 启动还原 6 项。
- 验证(2026-09-21 机器实测):
  - `dock_layout::tests` 10 例(含 `corrupt_file_falls_back_with_warning`、
    `missing_file_is_not_an_error`、`save_then_load_roundtrip`、
    `stale_schema_version_falls_back`、`normalize_pulls_illegal_columns_back`);
  - `workspace_persist.rs` 2 例:经**真实路径解析**(`VB_WORKSPACE`)写入 →
    重新 `load()` 逐字段还原(「重启还原」的机械等价物);损坏 → 回退 + 告警。
- 理由:停靠位/列数/面板态是**用户对工作环境的投资**,丢了会持续挫败;
  而 `schema_version` + 告警是"文件可以被手改"的现实让步 —— 手改坏了必须
  看得见原因,而不是表现成"软件坏了"。
- 后果:新增字段必须抬 `schema_version` 并写迁移;`DockSide` 的
  `snake_case` 序列化是兼容契约(见 ADR-0023)。
  **未纳入**:主题深浅、面板坞宽度(240–420 拖动值)、图层展开态 ——
  建议与"键位方案编辑器"一起做成完整的"工作区导入导出"。
- 关联:副文档 07;ADR-0023(停靠)、ADR-0024(面板固定停靠)。
