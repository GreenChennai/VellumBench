# K2 能力边界:Web(WASM)版 —— 评估与承诺范围

> 阶段 5-G / 05-11-2(台账 09-K)。本文是「裁剪版 web 壳」路线的**评估报告与承诺边界**:
> 为什么不搬完整 vb_app,Web 版承诺什么、不承诺什么、验证到什么程度。

## 一、评估:完整 vb_app 的 wasm32 可行性(实测)

`cargo check -p vb_app --target wasm32-unknown-unknown` 实测(2026-09,wasm-bindgen 0.2.128):

| 依赖 | wasm32 编译 | 阻碍点 | 评估 |
|---|---|---|---|
| egui 0.35 / eframe(wgpu feature) | **库本身可编译** | `eframe::run_native` / `NativeOptions` 在 wasm 不存在(入口必须换 `run_web`,整条 main/窗口/外壳层重写) | 中等工作量 |
| rfd 0.17 | **不可用** | `rfd::FileDialog` 在 wasm 目标下无该类型(原生对话框;异步 File System Access API 需另接) | 需 cfg 分叉或另接 web API |
| notify 8.2 | **不可用形态** | 文件监视基于 OS API(inotify/FSEvents/ReadDirectoryChangesW),wasm 无文件系统可监视 | Web 端语义不成立,须裁掉 |
| vello 0.10 / wgpu 29 | 可编译 | WebGPU 运行时要求(Chrome/Edge 113+;Firefox/Safari 仍在追赶);无 WebGPU 时整条画布链不可用 | 运行时风险,不是编译风险 |
| vb_kiln(pdfium-render) | **不可用** | `Pdfium::bind_to_library` / `load_pdf_from_file` 只有原生动态绑定 | 已 cfg 门控(`#[cfg(not(target_arch = "wasm32"))]`),PDF/AI 导入不承诺 Web |
| vb_doc / vb_html / vb_css / vb_render / vb_kiln(其余) / vb_layout | **全部可用** | 无 | 文档模型与渲染真相在 wasm 完整 |

**结论**:阻塞不在渲染或文档层(它们全绿),而在「桌面外壳」——原生文件对话框、
文件监视、原生窗口入口。把这些逐点 cfg 分叉,工作量与风险集中在 eframe web
runner + WebGPU 运行时验证,且收益只是复刻桌面壳。分册已授权裁剪路线:
> 允许做**裁剪版 web 壳**(新 crate 如 `vb_web/`:只含文档加载 + 渲染预览 +
> 画板切换 + 文字轻编辑,不搬完整 vb_app)。

## 二、承诺范围(已落地,`crates/vb_web/`)

1. **只读预览**:canonical HTML 本身就是预览(iframe srcdoc)。预览即真相 ——
   「文件真相源 + HTML 可 diff」在 Web 端原样成立,浏览器排版与桌面导出同一份字节。
2. **画板切换**:按 `data-vb-id` 注入一条只显 CSS 规则;只动预览串,不碰模型与导出。
3. **文字轻编辑**:走 `Command::SetText`(与桌面同一命令底座,捕获旧快照可逆,
   rev 随编辑递增),不是 contenteditable 野路子。
4. **导出**:`vb_doc::export::render_project` 同一 canonical 序列化 —— 产物
   index.html + styles/main.css 与桌面 Ctrl+S **字节同源**;另有 vb_kiln CPU
   光栅 PNG(当前画板 @2x)。
5. **文件进出**:载入 = fetch 内置示例或文件选择器(index.html + styles/main.css);
   外部 CSS 由壳先内联再导入(等价信息,告警如实透出,不静默)。

**验收闭环(分册判据「浏览器打开示例、切换画板、改文字并导出」)**:
无头 Edge 实测通过(见 `crates/vb_web/web/autotest-shot.png`,页面 `?autotest=1`
的 5 项机械断言全 PASS)。构建与运行步骤见 `crates/vb_web/build.ps1` 头注释。

## 三、不承诺(诚实清单)

| 项 | 原因 | 去向 |
|---|---|---|
| 完整编辑器 UI(面板/工具栏/撤销栈/多窗口) | 外壳层 wasm 化未做(见评估),Web 壳定位是预览+轻编辑 | 后续按需,先过 WebGPU 运行时验证 |
| 图像节点的相对路径显示 | iframe srcdoc 无文件系统;资产需 JS 侧改写或数据化 | 已在壳 UI 说明;模型与导出不受影响 |
| 光栅 PNG 的文字 | 浏览器无系统字体可枚举;须先经 `register_font` 注册字体字节 | HTML 预览不受限(浏览器自己排字);@font-face 的 family/weight 映射简化为 400 |
| PDF/AI 导入 | pdfium 原生绑定 | 桌面专属(已 cfg 门控,wasm 不编译) |
| 插件系统 / Agent / 浏览器导出车道(vb_browser) | 原生进程/CDP 依赖 | 桌面专属 |
| 自定义 .vb 之外的协议 | Web 壳只认项目目录契约(ADR-0028)的 index.html + styles/main.css | — |

## 四、为什么不把预览做成 WebGPU 画布

eframe/vello/wgpu 在 wasm32 能编译,但运行时押在 WebGPU 可用性上(Firefox/Safari
尚未全量),且文字渲染还要解决「浏览器内无系统字体」——预览会先于功能死在字体上。
canonical HTML 的 iframe 预览零这些依赖:任何浏览器、任何排版引擎、真文本真渐变,
且**展示的就是导出产物本身**。vb_kiln 光栅保留为可选 PNG 导出路径(与 CLI/CI 同一
渲染真相),不作为预览主路。
