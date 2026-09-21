# ADR-0030: 仓库协议由 MIT 切换为 ACL-1.0(与 Artboard 同款)

- 状态:已采纳(2026-09-22,用户明确要求「将开源协议更换为 artboard 的同款」)
- 背景:
  1. ADR-0019(2026-09-09)以 **MIT** 开源本仓库,理由是"往返语料与门禁脚本就是可测量证据",MIT 影响面最小。
  2. 同一作者(© GreenChennai)的 **Artboard** 项目使用自定义协议 **ACL-1.0**(Artboard 社区开源协议):授予使用/修改/分发与**商业性内部使用**,产出物归使用者且无署名义务,但对**软件本体**施加传染性(再分发须以 ACL-1.0 开源、SaaS 形态视为再分发)并禁止**转售软件本体**。
  3. 用户要求两项目协议一致,避免"同一作者的姊妹项目授权口径分裂"。
- 决策:
  1. 本仓库改用 **ACL-1.0**,与 Artboard **共用同一份协议文本**;仅第一条「本软件」的定义各自指向自己仓库的全部内容(本仓为 `crates/`、`tools/`、`bench/`、`examples/`、`tests/`、`docs/`、`assets/`、`i18n/`、`dist/`、配置文件、README/CONTEXT.md)。
  2. 产出物范围按本产品语义明确:HTML/CSS 项目、PNG/JPG/GIF/MP4、SVG/EPS/AI、PDF、PPTX 与设计成果 —— 全部**归使用者、可商用、无开源/署名义务**,与 Artboard 的口径一致。
  3. 清单元数据改用 `license-file = "LICENSE"`(ACL-1.0 不是 SPDX 标准标识符),crate 经 `license-file.workspace = true` 继承;`cargo package` 会把 `LICENSE` 一并打包。
- 兼容性分析(切换前必须成立):
  1. **依赖方向**:全部依赖(kurbo / wgpu / vello / egui / taffy / flo_curves / usvg …)为 **MIT / Apache-2.0** 等宽松许可,可用于更严格的衍生项目,无冲突;`docs/deps.md` 记录不受影响。
  2. **第三方素材**:字体、图标、vendor 库各自授权不变 —— ACL-1.0 第三条第 2 款已显式排除,不因本协议被"传染"。
  3. **`dist/Kiln-noGUI-CLI.exe`**:本仓自产构建物,随本协议一并授权。
  4. **历史提交**:MIT 阶段的既有提交在法律上仍按其发布时的许可可被使用;本 ADR 只改变**当前及后续**版本的分发条款。
- 后果:
  1. README 的 License 段与徽章改为 ACL-1.0;ADR-0019 的"MIT 许可"表述由本 ADR 覆盖(该条其余决策"全量入库"继续有效)。
  2. **对下游的实质影响**:把本仓(或其实质部分)打包再分发、或包装成 SaaS/API 提供服务,必须整体以 ACL-1.0 开源并提供完整对应源码;内部使用、二次开发、销售产出物不受限。
  3. 若将来要把某个 crate 单独发布到 crates.io,须先确认其载体协议与本协议不冲突(crates.io 会按 `license`/`license-file` 校验)。
- 关联:ADR-0019(全量入库,协议部分被本 ADR 覆盖)、`LICENSE`、`README.md`、`docs/deps.md`、Artboard 仓库的 `LICENSE`(同一文本)。
