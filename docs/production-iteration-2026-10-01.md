# 生产化迭代计划 · 2026-10-01

> 目标:把 VellumBench 从「能跑的半成品」推进到「生产环境可用」。
> 本文是本轮迭代的单一真相:工作包、验收标准、派发与验收协议。
> 执行方式:主 Agent 拆包派发,子 Agent 在独立 git worktree 实现,主 Agent 验收合并。

## 背景与现状判定

0.10.1(体检修复)→ 0.11.0-0.11.2(Kiln 动画流水线)之后,动画导出链路已达
生产可用;但以下缺口让它仍不能放心交付给第三方生产使用:

| # | 缺口 | 生产风险 | 级别 |
|---|---|---|---|
| PA | `write_project` 直接 `fs::write`,无原子性(v0.2 延期项) | 保存/导出中途断电或崩溃 → `index.html` 截断,**用户文档永久损坏** | P0 |
| PB | 静态 PNG 车道在确定性页面(SEEK 驱动)上单帧导出 ~27.5s | 下游单帧导出不可用;settle 协议对"不变页面"不收敛 | P0 |
| PC | `kiln` 入口用 `expect/panic`(退出码 101 + backtrace 噪音);vellum-mcp 写 out 不建父目录且错误无路径 | 上游脚本无法稳定解析失败;MCP 客户端首用即挫败 | P0 |
| PD | 无 GUI 冒烟、无发布冒烟清单落地 | 版本出门无底线验证 | P1 |

明确**不在本轮**范围(记录在案,后续批次):音频导出、GIF 流式化、i18n en
补全、自绘控件键盘可达性、vb_platform 实体化、undo 溢出写盘。

## 工作包

### PA · 断电安全写盘(vb_doc)

- 位置:`crates/vb_doc/src/export.rs::write_project`。
- 方案:全部落盘改「写临时文件(`.tmp-<name>-<pid>`)→ `fs::rename` 覆盖」;
  Windows 上 rename 可覆盖已存在文件(与 `vb_app/autosave.rs` 既有原子写同
  口径)。多文件项目按 index.html 最后写(存在即可打开的概率最大化)。
- 验收标准:
  1. 新增单测:注入 write 失败(中途 panic/错误)后,原文件保持完整旧内容;
  2. 既有 roundtrip/corpus 测试全绿(L0/L1 不回归);
  3. `cargo test -p vb_doc` 全绿,clippy 零警告。

### PB · 静态 PNG 车道 settle 收敛(vb_browser / vb_kiln)

- 现象:`kiln-cli export --format PNG`(静态车道)对 MV 类页面 ~27.5s。
- 方法:先复现计时定位(settle 多轮截图 × 每轮 ~0.5s?视觉稳定判据永不满足?),
  再修:页面无动画时 settle 立即收敛;有动画时按既定预算截断并告警,
  **不许静默吃满预算**。
- 验收标准:
  1. `kiln-cli export --source <MV工程> --output t.png --format PNG --width 1920 --height 1080`
     耗时 < 10s(基线 27.5s),PNG 内容非黑、含 MV 首帧画面;
  2. `examples/landing`、`examples/poster` 静态导出结果与基线像素一致
     (或差异可解释且 ≤ 既有口径);
  3. `cargo test -p vb_kiln -p vb_browser` 全绿。

### PC · CLI 错误一致性(kiln / vellum-mcp)

- `crates/vb_kiln/src/bin/kiln.rs`:消灭 `expect/unwrap/panic` 路径,错误改为
  stderr 单行 JSON `{"ok":false,"error":"…"}` + 退出码 2(用法)/3(输入)/4
  (IO/内部),与 kiln-cli 对齐;`--help` 退出 0。
- `crates/vb_agent/src/bin/vellum-mcp.rs`:写输出文件前 `create_dir_all(父目录)`;
  失败错误信息带完整路径。
- 验收标准:
  1. `kiln import --source 不存在.pdf` → stderr JSON 错误、退出码非 0 且非 101;
  2. `kiln-cli`/`vellum-mcp` 既有测试全绿;vellum-mcp 对不存在目录的 out 能成功
     落盘;
  3. clippy 零警告(`-D warnings` 口径)。

### PD · 发布物与冒烟(主 Agent 自留)

- `dist/README.md` 更新(新旗标、JPEG 默认、shell 自动发现);
- `dist/RELEASE.md` 冒烟清单落地(九格式自检 + MV 工程短段 + GUI 启动);
- 构建最新 `kiln-cli.exe` 单文件 → 桌面 `Kiln-noGUI-CLI.exe`;
- 全量门禁 + 提交推送。

## 派发与验收协议

1. 每个工作包一个 git worktree(`wt-pa/`/`wt-pb/`/`wt-pc/`)+ 分支
   `iter/pa`…;子 Agent 在 worktree 内实现、自跑包级门禁、`git commit`。
2. 子 Agent 回报:改动文件清单、测试输出原文、自查结论。
3. 主 Agent 验收:复核 diff、重跑该包验收命令、交叉运行**全量**门禁;
   不合格打回(带具体失败输出);合格 merge 进 main。
4. 合并后主 Agent 跑全 workspace `fmt --check + clippy + test + build --release`,
   全绿才算迭代完成。

## 验收底线(迭代完成定义)

- [ ] PA/PB/PC 三包验收全过;
- [ ] 全量 726+ 测试零失败、clippy 零警告、fmt 干净;
- [ ] 桌面单文件 + dist 包为同一构建产物;
- [ ] 推送 origin/main(网络恢复后)。
