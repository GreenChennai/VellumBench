# ADR-0028: artboard 接线契约 = 项目目录 + 无标记启发式识别(显式标记优先)

- 状态:已采纳(**2026-09-21 按探针事实修订**;原建议稿的"必须带 vb-artboard 标记"被实测推翻)
- 背景:artboard 技能消费 `src/index.html + styles/main.css + assets/`
  (`export.py:154`)。原建议稿(附录 10 ADR-VB-U06)写的是
  「以**项目目录 + `vb-artboard` 标记**为契约」。但 08a/08e 的独立探针给出了
  两个**实测事实**:
  1. **无标记稿件同样必须能打开**:`cf-cover-src` 这类第三方/历史项目
     只有普通 `<div class="poster">`,`import_project` 会把内容包围盒
     **合成为一个画板**(`synthetic_artboard = true`),并按
     `overflow: hidden` 语义钳制尺寸(实测 1080×1920);
  2. **`vs-` / `vsm-` 前缀只兼容读**;保存时一律写 `vb-`
     (`import.rs` 的前缀识别 + 导出侧的 `vb-` 契约)。
- 决策:
  1. 契约 = **项目目录形态**(`index.html` + `styles/` + `assets/`),
     **不要求**带 `vb-artboard` 标记;
  2. 识别优先级:**显式 `vb-artboard` 标记 > `vs-`/`vsm-` 兼容前缀 > 无标记启发式**
     (无标记 → 合成一个画板,包围盒尊重 `overflow: hidden`,并给**中文告警**
     说明"画板标记缺失,已合成");
  3. 保存一律写 `vb-` 前缀(标记、`data-vb-*` 属性);
  4. 画板尺寸存在**硬约束**:显式几何(width/max-width/height)是上限,
     内容溢出不撑大画布(浏览器 `overflow-x: hidden` 语义)。
- 实现:`crates/vb_doc/src/import.rs`(前缀识别 + 合成画板)、
  `crates/vb_doc/src/export.rs`(`vb-` 统一写入)、
  `crates/vb_layout/src/lib.rs`(`synthetic` 合成画板尺寸回填 + 硬约束钳制)。
- 验证(2026-09-21 机器实测):
  - `p01_unmarked_project_dir_recognized_as_artboard`:无标记 → 1 个画板 +
    尺寸 1080×1920 + 告警含"画板标记";
  - `p01_geometry_matches_css_semantics_s2kv` / `…_show2card`:百分比锚 /
    `inset:0` / 流式堆叠 / `transform: translate()` 折算;
  - 步骤 0 外部探针:三次连存字节幂等、`vs-` 零残留、`vb-artboard` 契约完整。
- 理由:要求"必须带标记"会把绝大多数真实 HTML 拒之门外 —— 而"能打开别人的
  HTML 项目"正是本产品与 SVG 系设计工具的**差异化**。标记只是**上限信息**
  (画板切分),不是准入门槛。
- 后果:导出侧必须保证标记一致,以便二次导入走"显式标记"这条**确定**路径
  (合成路径只用于首次导入无标记稿件)。
- 关联:副文档 08;`08a` / `08e` / `01a` 报告;ADR-0018(项目目录优先)。
