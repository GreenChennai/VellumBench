# COUP-02 收口台账(Document 字段边界)

> 状态:S6(2026-10-05)落地「务实版」收口 —— **类型边界已立,存量调用点清零是进行时**。
> 本文是 P1 收口的唯一台账:每个残留调用点都可在此定位,后续批次按文件清零后从表中划掉。
> 审查源:`iteration-review-2026-10-04.md` §2.1 COUP-02 / §6 COUP-R3。

## 已落地(本批)

1. **访问器 API**(`vb_doc::model::Document`):`rev()/bump_rev()/meta()/meta_mut()/nodes()/nodes_mut()/root()/defs_root()/artboards()/artboards_mut()/media_rules()/media_rules_mut()/pseudo_rules()/pseudo_rules_mut()` —— 新代码的推荐出口(方法与字段同名,`doc.nodes()` 即访问器)。
2. **只读视图 trait** `vb_doc::model::DocumentView`(rev/meta/nodes/root/defs_root/artboards/node/find_by_sid/sid_in_use/is_descendant_or_self/artboard_origin/subtree),`Document` 已实现;跨 crate 遍历型代码(投影/校验/统计)按它收参,不与具体类型绑定(COUP-R3)。
3. **非 vb_app 消费者已全走访问器**:`vb_agent`(main/patch/vellum-mcp + tests)、`vb_plugin`(projection + 测试 mock)、`vb_export`(slice)——这批 crate 的 `.nodes/.artboards/.root/.defs_root/.meta/.rev` 直达点清零(grep 复核为 0)。
4. **`pub(crate)` 翻转**(零外部使用者核实后才翻):`trailing_raw`、`extra_html_attrs`、`extra_body_attrs`。`raw_css/head_extra/tokens/media_rules/pseudo_rules` 有 vb_app/vb_kiln 存量读者,保持 pub,随 vb_app 面板批次再收。
5. **`#[doc(hidden)]`**:结构性字段 `nodes/root/defs_root/artboards` —— 文档与 IDE 不再鼓励直达;字段本身保持 pub(下表调用面在清零前必须可编译)。

## 未清零的存量调用点(按 crate 汇总;计数 = 直读/直改出现次数,含 tests/examples)

总量约 1019 处。

| crate | 计数 | 主要文件(残留点数) |
|---|---|---|
| `vb_app` | 747 | `crates/vb_app/src/app/commands.rs`(56), `crates/vb_app/src/app/health.rs`(49), `crates/vb_app/src/app/panels/artboards.rs`(40), `crates/vb_app/src/app/clip_mask.rs`(38), `crates/vb_app/src/app/symbol_cmds.rs`(38), `crates/vb_app/src/app/control_panel/tests.rs`(29), `crates/vb_app/src/app/panels/layers/tests.rs`(28), `crates/vb_app/src/app/breakpoints.rs`(26) … |
| `vb_kiln` | 124 | `crates/vb_kiln/tests/kiln_smoke.rs`(30), `crates/vb_kiln/src/dompaint.rs`(24), `crates/vb_kiln/tests/vb_observability.rs`(20), `crates/vb_kiln/src/domexport.rs`(15), `crates/vb_kiln/src/bin/kiln-cli.rs`(9), `crates/vb_kiln/src/import_svg.rs`(7), `crates/vb_kiln/tests/anim_timeline.rs`(6), `crates/vb_kiln/src/import_pdf.rs`(4) … |
| `vb_tools` | 53 | `crates/vb_tools/src/lib.rs`(24), `crates/vb_tools/tests/pathfinder_doc.rs`(11), `crates/vb_tools/src/align.rs`(9), `crates/vb_tools/src/pathfinder.rs`(6), `crates/vb_tools/src/boolean.rs`(3) |
| `vb_session` | 31 | `crates/vb_session/src/snap.rs`(31) |
| `vb_layout` | 27 | `crates/vb_layout/tests/p01_faithful_roundtrip.rs`(10), `crates/vb_layout/tests/artboard_declared_height.rs`(5), `crates/vb_layout/tests/translate_materialize_roundtrip.rs`(5), `crates/vb_layout/tests/layout_test.rs`(4), `crates/vb_layout/src/lib.rs`(2), `crates/vb_layout/tests/transform_translate.rs`(1) |
| `vb_render` | 19 | `crates/vb_render/tests/render_test.rs`(13), `crates/vb_render/src/encode.rs`(6) |
| `vb_web` | 8 | `crates/vb_web/src/lib.rs`(8) |
| `vb_ui` | 6 | `crates/vb_ui/src/expr.rs`(4), `crates/vb_ui/src/components.rs`(2) |
| `vb_shell` | 4 | `crates/vb_shell/src/bin/canvas_spike/main.rs`(3), `crates/vb_shell/src/bin/canvas_spike/scene.rs`(1) |

## 清零口径(P1 进行时)

- **vb_app 面板文件**归属 UI 批(并行批次保留面):面板改字段的调用点随「面板 = fn(state)→Content 纯投影」重构(COUP-R6)自然消失。
- **vb_app 非面板组装面**(app/dispatch*/commands/external/nav/canvas_shot 等):下一工程批次按文件迁移,迁移一个文件即从本表划掉。
- **vb_kiln/vb_kit/vb_shell/vb_render**:只读遍历优先改走 `DocumentView`;写路径一律回命令层(单入口纪律),不允许新增直改点。
- **新增代码纪律(即刻生效)**:任何 crate 新代码禁止直读/直改 `#[doc(hidden)]` 字段;review 以本表为基线,只减不增。

## 门禁锚点

- 访问器/视图 trait 等价单测:`crates/vb_doc/src/model.rs`(`accessors_agree_with_fields` / `mutable_accessors_roundtrip` / `document_view_trait_matches_inherent`)。
- 迁移零直达复核(PCRE):`grep -rnP '\.(nodes|artboards|defs_root)(?!\()' crates/vb_agent/src crates/vb_plugin/src crates/vb_export/src` 应为空(`.root` 因 HtmlDom 等同名字段按接收方排除)。
