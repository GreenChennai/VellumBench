# ADR-0021: AI/PDF 矢量写入走「浏览器布局 → Kiln 写入器」(DOM 快照路线)

- 状态:已接受(2026-09-19,设计文档 20 篇裁定)
- 背景:v0.7.0 车道 B 的 AI/PDF 直接采用 Chrome printToPDF 产物,像素门禁全过(99.37),但 Illustrator 实测暴露结构性溃败:letter-spacing 文本按字形逐个 Tj 且拆进 17 个 Type3 字体(逐字断开、无法整串编辑)、86 个 clip 全部变剪切蒙版、SMask/Type3/Shading 组合触发"未知的阴影类型/图像结构"报错、blend 层文字被栅格化(转曲)。
- 决策:AI 格式(及需要可编辑结构的 PDF)不再使用 Chrome 的 PDF 流,改为:浏览器完成加载/settle 后注入 domsnap.js 采集 PaintList(元素几何/样式/逐行文本 run/图层分类/裁剪计数),由 vb_kiln 新模块 dompaint 映射为 DrawList,走既有 PDF 写入器输出(Type0/CID 整行文本 + Tc 字距、OCG 双图层、多页多画板、渐变栅格化、图像 SMask)。`--vector chrome|dom` 保留 printToPDF 作为打印/阅读快速通道。
- 理由:修补 Chrome PDF(合并 Tj、剥离 clip)治标且无法救回栅格化文字;DOM 快照让浏览器只做它最强的布局测量,Kiln 写入器做它最强的结构矢量,两类问题从根上不再产生;车道 K 的 PDF 写入器升格为双车道共用矢量后端。
- 后果:新增 domsnap/dompaint 两个受维护资产与 DOM 采集覆盖面风险(伪元素/堆叠上下文渐近补);字形 advance 与 Blink 的残余差异由 Tc 反推行宽 + 视觉门禁(A7 ≥97)约束;门禁新增 ai_struct.py 结构断言(A1–A8);AI 场景的优先级按用户二轮反馈调整为可编辑性优先(blend 文字以采样实色救活,差异记 warning),PNG/打印 PDF 的像素保真路线不变。
