# ADR-0035: 最近项目存配置目录 recent.json(与 workspace.json 同级)

- 状态:已接受(阶段 2 / 副文档 02-3;兑付迭代附录 ADR-VB-L03,主文档 W4)
- 背景:主页要有内容可列(最近项目 / 恢复会话),但**不能污染用户项目**
  (写进项目目录既脏又会在删除项目时丢记录)。
- 决策:
  1. `recent.json` 落配置目录(与 `workspace.json` 同一目录解析函数,
     Windows 即 `%APPDATA%\VellumBench\`);
  2. 原子写(tmp+rename)+ 损坏回退到空表 + 告警(不静默);
  3. 上限 LRU 条目;条目含路径 / 名称 / 时间 / 固定位;支持 `VB_RECENT`
     环境变量重定向(测试 / 便携);
  4. 会话(一次运行中同时打开的项目集合)随条目存,主页可"恢复上次会话"。
- 落地:`crates/vb_app/src/recent.rs`(`RecentStore` / 路径解析 / 原子写)、
  `crates/vb_app/src/shell.rs`(单写入者)。
- 取舍:跨机器不同步(交给文件盘 / 网盘);换来不写用户项目目录。
- 被否决替代:存每个项目目录内(污染 + 删项目丢记录)。
