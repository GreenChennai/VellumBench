# ADR-0032: Web 版走裁剪壳(vb_web),不搬完整 vb_app

- 状态:已接受(阶段 5-G / 05-11-2;台账 09-K)
- 背景:分册 K2 原计划「复用 egui web runner + wgpu(WebGPU)」做只读预览
  + 轻编辑。实测 `cargo check -p vb_app --target wasm32-unknown-unknown`
  (评估全文见 `docs/k2-web-capability.md`):文档/渲染层(vb_doc、
  vb_render、vb_kiln 除 pdfium)在 wasm32 全部可编译,阻碍集中在桌面外壳 ——
  `rfd::FileDialog` 无 wasm 形态、`eframe::run_native` 不存在、notify 文件
  监视语义不成立、pdfium 仅原生动态绑定;且 wgpu/WebGPU 是运行时风险
  (Firefox/Safari 未全量),真文本光栅在浏览器里还缺系统字体枚举。
- 决策:新 crate `crates/vb_web`(仅 wasm32 使用,宿主编译只为过 workspace
  门禁),承诺四件事,全部压在**已在 wasm 全绿**的底座上:
  1. **预览 = canonical HTML 本身**(iframe srcdoc)—— 预览即真相,
     浏览器排版,零字体/零 WebGPU 依赖;
  2. 画板切换(按 `data-vb-id` 注入只显 CSS 规则,只动预览串);
  3. 文字轻编辑走 `Command::SetText`(与桌面同一命令底座,rev 推进);
  4. 导出走 `export::render_project`(与桌面 Ctrl+S 字节同源)。
  分层:业务在宿主可测的 `WebCore`(`Result<_, String>`),wasm-bindgen
  门面只做 String→JsValue 转接 —— `cargo test --workspace` 跑在宿主,
  测试必须打在 WebCore。
- 取舍:Web 端不承诺完整编辑器 UI/插件/Agent/PDF 导入;光栅 PNG 需先注册
  字体字节(`register_font_bytes`);pdfium 模块 `cfg(not(wasm32))` 门控。
  换来:任何浏览器可用的最小闭环(打开示例、切画板、改文字、导出),
  无头 Edge 自动验收 5 项机械断言(`?autotest=1`)。
- 被否决替代:完整 vb_app 的 eframe web runner 化(工作量集中在外壳层
  cfg 分叉与 WebGPU 运行时验证,收益只是复刻桌面壳);纯 WebGPU 画布预览
  (运行时兼容性 + 字体双重风险,预览会先死在字体上)。
