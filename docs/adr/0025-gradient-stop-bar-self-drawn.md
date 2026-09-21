# ADR-0025: 渐变色标条自绘,且渐变值以 canonical 形式回写

- 状态:已采纳(2026-09-21;副文档 05 §05-2 V4 决策落地)
- 背景:ADR-0003:7 已注明"复杂自绘控件(渐变编辑器)需自绘";此前渐变能力只有
  "拖方向 + 色标写死",且色标是**字符串**(`Vec<String>`),拖不动、点不了、
  改不了颜色。同时发现一个更隐蔽的问题:面板直写 `background-image` 时
  放进 `Decl` 的是**原样字符串**,而导出后重导入会过 `vb_css::canonical_value`
  (`0%` → `0`、`#ffffff` → `#fff`),于是「编辑 → 保存 → 重开 → 保存」
  首轮就改写文件 —— **判据 C(L1 字节幂等)破损**。
- 决策:
  1. 渐变上收为**结构化模型** `vb_ui::gradient::Gradient`
     (`kind / angle / angle_explicit / head / stops / hints`);
     `angle_explicit == false` = 源串没写角度(CSS 缺省 `to bottom`),
     **回写不得补出 `180deg`**;
  2. 色标条**自绘**,几何与命中判定全部是纯函数
     (`stop_x` / `pos_at_x` / `hit_stop` / `hit_hint` / `sample`);
  3. **`Gradient::to_css()` 输出前过一遍 `vb_css::canonical_value`** ——
     面板写回的值与二次导入后的值**逐字节相同**;
  4. 解析必须接受 canonical 输出:`0%` 规范化成 `0` 后,位置解析要认
     **无单位零长**;`skewX(` 规范化成 `skewx(` 后,函数名解析要**大小写不敏感**。
- 实现:`crates/vb_ui/src/gradient.rs`(+ `vb_ui` 新增 `vb_css` 依赖)、
  `crates/vb_app/src/app/gradient_panel.rs`;`control_panel::parse_gradient`
  改为**委托**该模型(消灭第二份解析口径)。
- 验证(2026-09-21 机器实测):
  - `vb_ui` 11 例:canonical 形式 + 幂等(`parse(to_css(x)).to_css() == to_css(x)`)、
    无角度线性、中点提示往返、反向、几何与命中;
  - `s4b_panels_doc.rs` 11 例:三层背景只替换渐变层(`url()` 不丢)、
    外观条目同源、透明度四属性白名单、全局色令牌驱动 `:root`、
    三处「导出→重导入→再导出」逐字节相同;
  - 顺带修掉一个既有缺陷:`vb_css::canonical_value` 原先把 `0deg` 规范化成
    **非法的 `0`**(`linear-gradient(0, …)` 不是合法 CSS),现按
    `ZERO_KEEPS_UNIT`(deg/grad/rad/turn/s/ms)保留单位。
- 理由:色标条是"能用"与"不能用"的分界;而 canonical 回写不是洁癖 ——
  L1 是"这个文件是你的,不是它的"的唯一保证。
- 后果:任何**新写** CSS 值的面板都必须遵守"回写即 canonical",
  否则会重新引入 L1 破损。渐变模型新增字段时必须同步更新
  `to_css` / `parse` 的往返测试。
- 关联:副文档 05;ADR-0003(egui/自绘);`05b` 报告(含三处缺陷的完整记录)。
