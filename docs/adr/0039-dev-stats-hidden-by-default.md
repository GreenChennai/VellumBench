# ADR-0039: 开发者统计默认隐藏,「视图」菜单开关

- 状态:已接受(阶段 4a / 副文档 04-4;兑付迭代附录 ADR-VB-L07,主文档 W6)
- 背景:实测 P1-① 标题栏 + 状态栏常驻 FPS / 显卡型号 / 温度 / 功耗 / 帧时间 /
  节点数 —— 调试数据被当产品常态展示,还泄露用户硬件。
- 决策:
  1. 全部调试数字收进**开发者统计浮层**,`视图 → 开发者统计` 开关
     (`view.developer_stats` 命令),开关状态入 `workspace.json`
     (`dev_stats`,默认关);
  2. FPS **采样**与显示同门:只在浮层可见时采样(`frame_times` 环),
     显示本身不造成隐性常驻计算;
  3. 画布角落不常驻任何数字;渲染后端信息只在首选项「渲染」页以文案说明,
     不做后端切换。
- 落地:`crates/vb_app/src/app/dock_layout.rs`(`dev_stats` 持久化)、
  `crates/vb_app/src/app/frame.rs`(采样门)、
  `crates/vb_app/src/app/panels/mod.rs`(浮层 `show_dev_stats`)、
  `crates/vb_app/src/app/prefs_dialog.rs`。
- 取舍:开发者要看数字需多点一下;换来产品观感与信息隐私。
- 被否决替代:编译期 `debug_assertions` 区分(发布构建会丢诊断能力,
  与"实测取证"的工作方式冲突)。
