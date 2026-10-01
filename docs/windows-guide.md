# VellumBench / Kiln Windows 使用指南(从零到能用)

> 面向:在一台干净的 Windows x64 机器上,把 VellumBench 跑起来、导出第一份产物、
> 并在开发迭代时不把磁盘/内存搞炸。项目是什么看 [README](../README.md),本文只讲"怎么跑"。
> 更新:2026-10-02(v0.13.x,开发机 GT 710 / 16GB 实测口径)

## 1. 环境要求

| 依赖 | 版本 | 用途 | 缺了会怎样 |
|------|------|------|-----------|
| Windows | 10 x64 及以上 | — | — |
| Rust + Cargo | 1.85+(MSVC 工具链) | 构建 | 无法编译 |
| VS Build Tools 2022 | 含 C++ 工具集 | 链接器 link.exe | 链接阶段报错 |
| Edge 或 Chrome | 系统安装即可 | 浏览器导出车道/动画渲染 | `engine=auto` 降级自研引擎(矢量能力受限) |
| ffmpeg | 可选(任意完整版) | MP4 编码、GIF 管道 | 动画导出自动降级 GIF 流;`--format gif` 照常 |
| Git Bash / PowerShell 5+ | 系统自带 | 跑 tools/ 脚本 | 只能手敲命令 |

> GPU 说明:有 N 卡(AV1/NVENC)/A 卡(AMF)会自动用上硬编码;没有也全功能可用,
> 只是 MP4 编码走 CPU 候选链(x264)。核显/老卡(GT 710 级)注意 §6 的并发会话上限。

## 2. 获取与构建

```powershell
git clone https://github.com/GreenChennai/VellumBench.git
cd VellumBench
cargo build --release -p vb_app -p vb_agent
```

产物在 `target\release\`:

| 可执行 | 干什么 |
|--------|--------|
| `vellumbench.exe` | 桌面 GUI(主程序,双击即用) |
| `vellum-cli.exe` | Agent/脚本接口(selfcheck、打开工程等) |
| `vellum-mcp.exe` | MCP 服务(接 Claude/ZCode 等 Agent 工作流) |
| `kiln-cli.exe` | Kiln 无 GUI 导出器(CI/无人值守首选) |
| `kiln.exe` | Kiln 常驻服务模式 |

一键打包(归档 + 冒烟):`powershell -ExecutionPolicy Bypass -File tools/build.ps1`。

首次运行 GUI 后建议先跑一遍自检:`vellum-cli selfcheck`(无头,不依赖 GPU/浏览器)。

## 3. 导出第一份产物(Kiln)

Kiln 的输入就是标准 HTML/CSS 项目目录(`examples/` 里有现成样例):

```powershell
# 静态:海报 → PNG @2x
kiln-cli export --source examples/poster --output out.png --scale 2

# 矢量:→ PDF(默认 chrome 打印路线)/ AI(默认 dom 可编辑路线)
kiln-cli export --source examples/landing --output out.pdf
kiln-cli export --source examples/landing --output out.ai

# 动画:2 秒 25fps → MP4(有 window.SEEK 约定接口的页面自动走确定性渲染)
kiln-cli export --source examples/landing --output out.mp4 --fps 25 --duration 2

# 动画 → GIF(--loop 0 = 无限循环)
kiln-cli export --source examples/landing --output out.gif --fps 25 --duration 2 --loop 0

# 九格式自检(验证部署环境)
kiln-cli selfcheck
```

关键参数速查(`kiln-cli export --help` 有全表):

| 参数 | 说明 |
|------|------|
| `--render auto\|webcodecs\|screenshot` | 动画车道:auto 时 canvas+SEEK 页优先 WebCodecs 页内硬编(全 GPU,实测 70+ 帧/s),失败自动回退截图车道 |
| `--img jpeg\|png` | 中间帧格式;jpeg 快 ~6×,画质敏感交付用 png |
| `--workers N` | MP4 分段并行;**默认 1**(实测多实例负收益),确要并发用 `--workers 2..16` |
| `--encoder auto\|x264\|nvenc\|amf\|qsv` | H.264 编码器候选链,auto 自动探测降档 |
| `--engine auto\|browser\|native` | auto=浏览器可用即用;native=纯自研(无浏览器环境时) |
| `--wall` | 墙钟实时采样(旧行为);默认确定性寻址(逐帧 seek,可复现可并行) |

## 4. 接入 Agent 工作流(可选)

- **MCP**:`vellum-mcp.exe` 注册进 ZCode/Claude 等 Agent 的 MCP 配置即可让 Agent 直接读写工程。
- **约定接口**:页面提供 `window.SEEK(t)`(或 `seek`/`__SEEK__`)即可获得确定性逐帧渲染;
  `POST /__kiln-upload` 供 WebCodecs 车道回传编码流(内部使用,无需手动调)。

## 5. 开发与测试:磁盘/内存自救手册

### 5.1 target/debug 为什么会膨胀,怎么治

`cargo test` 每轮都会在 `target\debug\deps\` 留下**旧哈希**的测试 exe/pdb(单个几百 MB),
`incremental\` 缓存只增不减;Cargo.toml 已设 `profile.dev debug=1`(行级调试表)减缓增速,
但**每测试一次清一次**才是治本:

```powershell
# 推荐:测试 + 自动清理一步到位(测试无论成败都会清)
powershell -ExecutionPolicy Bypass -File tools/test.ps1
# 快档(只测 git 已改的 crate):tools/test.ps1 -Fast

# 已手动跑过 cargo test?补一刀:
powershell -ExecutionPolicy Bypass -File tools/test.ps1 # 或单独:
powershell -ExecutionPolicy Bypass -File tools/test-clean.ps1   # 只清理
tools/test-clean.ps1 -Full                                       # 磁盘告急时整目录清(下次全量重编)
```

清理规则:删 `incremental`;deps/examples 里旧哈希 exe/pdb 按主干分组只留最新;
清 `%TEMP%` 超 24h 的 `kiln-*` 残留;清完仍 >8GB 会提示 `-Full`。

Kiln 自身的中间产物(段 mp4、浏览器 user-data-dir 等)写 `%TEMP%\kiln-*`,
程序内有 6h 兜底清扫;异常退出残留也可被上面第 3 条规则回收。

### 5.2 rustc 崩溃 0xc0000409 / "配额不足"(os error 1453)= 提交内存耗尽

症状:cargo 并行编译时多个 rustc 秒崩,退出码 `0xc0000409 (STATUS_STACK_BUFFER_OVERRUN)`,
或报 `os error 1453`。这不是代码 bug:是 **RAM+页面文件的 commit 上限被打穿**。

自救顺序:
1. `set CARGO_BUILD_JOBS=2`(甚至 1)再跑——限制并行 rustc 数量;
2. 扩页面文件:`系统属性 → 高级 → 性能设置 → 高级 → 虚拟内存`,设在**空间大的盘**,
   初始 8192MB / 最大 16384MB,重启生效;
3. 仍崩就关掉大内存应用再编。

### 5.3 本机(C:)满了会不会炸构建

会:rustc/link 的临时文件写满即截断工件,症状是 `E0462/E0463/E0786 invalid metadata`
级联乱报。预防:构建环境的 `TEMP`/`TMP` 指到空间大的盘(本机惯例 `D:\Temp`),
报"随机编译失败"先查 C 盘剩余空间;`tools/test.ps1` 已内置该保护。

## 6. 已知限制(硬件相关)

- **NVENC 并发会话上限**:GT 710 级老卡 = 3 路,第 4 路报 `OpenEncodeSessionEx out of memory`。
  程序会自动探测降档 workers,一般无需干预;真硬编档位建议在有像样 GPU 的机器上跑。
- **多实例并行目前是负收益**(9070 GRE 两轮实测,轻页面也随段数变慢),默认 workers=1,
  并发是显式 opt-in。
- **Edge 无 `HeadlessExperimental.beginFrame` 域**:自动识别并直走 captureScreenshot,无需配置。
- **PDF/AI 导入**依赖 pdfium.dll:默认探测 `E:\Tools\pdfium\pdfium.dll`、`C:\Tools\pdfium\pdfium.dll`,
  其他位置用环境变量 `PDFIUM_DLL` 指定。

## 7. 故障速查

| 症状 | 处理 |
|------|------|
| `未发现系统浏览器` | 装 Edge/Chrome,或 `VB_BROWSER_PATH` 指到浏览器 exe |
| MP4 导出变成 GIF | ffmpeg 缺失时的自动降级(warnings 里有说明);装完整版 ffmpeg 后重跑 |
| 动画导出报 `Output file is empty` | 旧版 bug 修复于 0.13.x,先更新;仍现请附 warnings 里"实例 N 统计"行提 issue |
| 导出结果黑屏/残影 | 多为 DOM+canvas 混合页,确认用 0.13.0+(mixed-dom 自动回退);仍现加 `--render screenshot` 验证 |
| 编译随机乱报 E0462/E0786 | §5.3:C 盘满截断工件;清 TEMP 指向 + `cargo clean` 重编 |
| rustc 0xc0000409 | §5.2:提交内存耗尽;限 jobs + 扩页面文件 |
| GitHub push 卡死 | 用 SSH 443 通道(仓库已配 ssh.github.com:443 别名) |

## 8. 路线图速览(截至 0.13.x)

已完成:九格式导出、WebCodecs 页内硬编车道(实测 9×)、确定性渲染、自检、MCP。
进行中:WebCodecs 正确性自验(解码回读逐帧比对)、Windows 硬编 flaky 兜底。
规划:音频导出、GIF 流式化、i18n en(现仅关键文案,全量是周级工程)、undo 溢出写盘、
自绘控件键盘可达性分批补齐。
