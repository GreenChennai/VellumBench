# K1 非 Windows 运行时实机验证清单

> 阶段 5-G / 05-11-1(台账 09-K)。本机为 Windows:已做到**编译级验证**
> (`cargo check --workspace --all-targets` 对 `x86_64-unknown-linux-gnu` 与
> `aarch64-apple-darwin` 双目标全绿)+ CI 双 runner(ubuntu-latest /
> macos-latest,cargo check + cargo test 级,见 `.github/workflows/ci.yml`)。
> 以下差异**无法在 Windows 上验证**,需非 Windows 实机逐项过(每项都给
> 机械判据;全部通过前 K1 保持 Partial):

## Linux(X11/Wayland 桌面)

- [ ] **启动**:窗口正常出现、无崩溃(`cargo run -p vb_app`;Wayland 下
      eframe 走 X11 后端是否需要 `WINIT_X11_SCALE_FACTOR` 记录现象)。
- [ ] **字体回退**:缺 Segoe UI / Microsoft YaHei / SimHei 的系统上,画布
      与 UI 文本回退到 fontique 可枚举的字体(如 DejaVu / Noto),中文
      **不出现豆腐块**;`.vb-autosave` 差异视图、字体缺失对话框(09-O)
      在 Linux 字体生态下的行为。
- [ ] **fontconfig dlopen**:`vb_render` 默认特性 `fontconfig-dlopen`
      (构建期无需 libfontconfig1-dev);运行时需 `libfontconfig1` 本体
      (CI 已装)。极简容器(无 fontconfig)启动时字体枚举降级是否优雅。
- [ ] **窗口/DPI**:fractional scaling(如 125%/150%)下 egui 缩放与
      Vello 画布清晰度;拖动停靠工具栏(0023)不漂移。
- [ ] **导出**:打开示例(landing)→ 导出 PNG(vb_kiln CPU 路径)与
      Edge 无头路径(vb_browser,需系统装有 Edge/Chrome)各一次,与
      Windows 基线像素级对比(CI runner 无 CDP 时按设计降级,见
      1543e47)。
- [ ] **路径分隔符**:打开/保存/最近项目(recent.json)、`.vb-cache/`
      缩略图、`.vb-autosave/` 快照全部经 `std::path` 拼接(代码审计已过
      —— 全仓无手拼 `/`/`\` 的路径字面量;实机开一次含子目录的项目确认)。

## macOS(aarch64)

- [ ] **启动**:`cargo run -p vb_app`(Apple Silicon,macOS 14+);Retina
      2x 下画布分辨率正确(不糊)。
- [ ] **字体回退**:PingFang SC 渲染中文示例(landing 用 Inter + PingFang);
      字重就近匹配(registry 缺项时)走系统字体。
- [ ] **菜单栏**:egui 菜单在 macOS 的快捷键修饰键(Ctrl vs Cmd)表现;
      快捷键注册表(门禁 9)以 `win` 键位列出,macOS 映射行为记录。
- [ ] **导出/CDP**:vb_browser 对 Safari 无 CDP —— 确认降级路径(检测
      不到 Edge/Chrome 时显式报错,不假红)。
- [ ] **文件监视**:notify 的 FSEvents 后端热重载(Agent 改 HTML →
      画布自动采用)延迟与去抖正常。

## 门禁补充(实机跑一次即可)

- [ ] `cargo test --workspace` 全绿(CI 双 runner 已代跑;实机复核一次
      GPU 相关的 wgpu 枚举告警)。
- [ ] `tools/ci.ps1` 的 Windows 专属门禁(硬编码颜色棘轮、UI 截图基线)
      不要求在 Linux/macOS 复跑 —— CI 已按平台拆分。

## 结论(用于回填台账)

- 编译级:Windows 交叉 check 双目标全绿(2026-09)——机械判据已闭合。
- 运行时级:上表逐项过完后,K1 才从 Partial 转 Done;任何一项不通过,
  在台账 09-K 写明现象与平台版本。
