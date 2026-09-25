# ADR-0034: 多项目采用 eframe 多 viewport,一项目一窗口

- 状态:已接受(阶段 2 / 副文档 02-5;兑付迭代附录 ADR-VB-L02,主文档 W3)
- 背景:目标③要求"新建项目(有新窗口)"。实测基线为单进程单窗口
  (`main.rs` 单 `ViewportBuilder` + 单次 `run_native`),无法并行编辑两个项目。
- 决策:
  1. 用 eframe 官方多 viewport(`ViewportId` / 子视口)实现多窗口;
     `ShellApp` 为外壳单例,持有根项目窗口与子项目窗口表
     (`wins: Vec<ProjectWindow>`);
  2. 每窗口一份 `VellumApp`:独立文档、独立撤销栈、独立选中态、独立标题;
  3. 全局配置单写入者:`recent.json` 只有外壳写(02-5-3),
     `workspace.json` 各窗口写、最后写入胜 + 原子替换(ADR-0029 承接);
  4. 关闭走确认(未保存改动时),关一窗不影响其余窗口。
- 落地:`crates/vb_app/src/shell.rs`(窗口编排 / `ShellRequest` 通道 /
  关闭确认 / 标题同步)、`crates/vb_app/src/app.rs`(`viewport_id`)。
- 取舍:多窗口状态管理复杂度上升(聚焦、标题、退出语义都要逐视口处理);
  换来可并行编辑与"新建即新窗口"的产品语义。**不做"隐藏后唤出"**
  (eframe 已知限制)。
- 被否决替代:(a) 单窗口内换项目(丢"新窗口"需求);(b) 多进程
  (配置写竞争与热重载协调成本更高)。
