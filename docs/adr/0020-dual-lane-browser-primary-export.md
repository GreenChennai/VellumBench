# ADR-0020: 双车道导出——浏览器车道为对外交付主路,自研车道降级为离线兜底

- 状态:已接受(2026-09-19,设计文档 19 篇裁定,待实施)
- 背景:Kiln v0.6.0 的 PNG/PDF 对外导出全部走自研链路(vb_layout + vb_render::cpu + vb_kiln),真实工程(验收集 6 类 26 文件)大量使用 grid/mask/blend/backdrop-filter/可变字体等结构性缺失特性,导出结果不可用;`bench/pdf_fidelity.py` 的 98.63 为自洽对拍分(无浏览器真值)。用户验收口径:PNG 对 WPI(系统 Chromium 渲染)相似度 99%±,PDF/AI 对 PNG 97–99%,优先级 保真度>可编辑性>转换速度。
- 决策:
  1. 新增浏览器车道(车道 B):kiln-cli 内建 Rust 原生 CDP 客户端(新 crate `vb_browser`),驱动系统 Edge/Chrome headless——PNG 用 `captureScreenshot`(captureBeyondViewport),PDF/AI 用 `Page.printToPDF`(screen 媒体仿真 + 精确纸张 + 后处理);**直接渲染源 HTML,不走 Document 往返**。
  2. 自研车道(车道 K)保留:无浏览器环境兜底、编辑器实时预览;导出结果携带 `degraded_engine` 警告,不再承担对外保真承诺。
  3. 选路:`--engine auto`(默认,浏览器可用即 B)· `browser` · `native`;`selfcheck` 增加浏览器探活。
  4. WPI Python 进程桥(`vb_export/src/wpi.rs`)降为开发期基线对照工具,不再是对外依赖。
- 理由:与 WPI 同引擎同协议,99% 对拍由构造保证、由 `bench/acceptance.py` 夹具验证;自研引擎补齐到浏览器级保真不具可行性(验收集特性矩阵见 19 篇 §3);单二进制分发约束(ADR-0009/0019)不受影响,无常驻浏览器、无 JS 执行需求。
- 与既有 ADR 的关系:修正 ADR-0001 的适用范围(自研的是场景图/文档模型,栅格与打印可外购"浏览器作为渲染后端");承接 ADR-0005 的浏览器渲染思路但去 Python 化;ADR-0016 的 CPU 引擎职责收窄为车道 K 与 CI;不推翻 artboard 侧"WPI 退役"(退役的是 Python 进程依赖,不是浏览器渲染)。
- 后果:对外保真分必须由 acceptance 夹具机器生成(禁止手写宣称);开源用户需系统 Edge/Chrome(缺失时车道 K 降级并明示);CDP 协议面(十要素捕获协议,19 篇附录 A)成为需维护的资产;PDF 的渐变/阴影等不可矢量化效果按局部栅格化降级,可编辑性以检查表与占比指标显式化。
