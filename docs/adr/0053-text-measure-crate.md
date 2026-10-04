# ADR-0053: 文本量测上收为底层 crate `vb_textmeasure`——布局层与渲染层共同下依赖(COUP-01),字体注册表实例化(COUP-06)

- 状态:**已裁定并落地**(2026-10-05 迭代审查 COUP-01/COUP-06/PERF-01/DOC-10/DOC-11;实现见 `crates/vb_textmeasure`、`vb_render::text` 门面、`vb_kiln` 导出链)
- 背景:2026-10-04 全仓审查 §2.1/§2.2。`vb_layout` 仅为 `measure_text_weighted` **一处**文本量测反向依赖 `vb_render`(`Cargo.toml:15`),把纯几何布局 crate 拖进 vello/wgpu/swash/fontique 编译面,破坏「布局不依赖渲染」分层(COUP-R1);同时字体注册表是进程级 `static Mutex<HashMap>` + 一把全局串行锁,所有文本整形互相阻塞,跨项目不自动清(仅 kiln 手工 `clear_font_registry`),动画分段并行(`--workers>1`)下互相清注册表是真实竞态(COUP-06);热路径每次整形重建 fontique `Collection`/`SourceCache` 并整份拷贝字体字节,逐字形 `chars().nth(gi)` 为 O(n²)(PERF-01/DOC-09)。

## 1. 裁定

### 1.1 新 crate `vb_textmeasure`(否决「下沉 vb_common::text」)

- 方向:`vb_render/src/text.rs` **整体上收**为新 crate `vb_textmeasure`(fontique + swash + zeno + vb_common::geom),`vb_render::text` 保留为**纯 re-export 门面**(`pub use vb_textmeasure as text;`),`vb_kiln`/`vb_export`/`vb_web`/`vb_app` 调用点**零改动**。
- 依赖方向恢复:`vb_common ← vb_textmeasure ← (vb_layout, vb_render)`;`vb_layout` 不再依赖 `vb_render`,方向断言钉子 `tests/dep_direction.rs`(源码零 `vb_render` 引用 + manifest `[dependencies]` 无 vb_render)防回潮。
- **否决下沉 `vb_common::text`**:fontique/swash/zeno 是重外部依赖,vb_common 是全仓底座(被 19 crate 依赖),下沉等于把字体引擎塞进每一个 crate 的编译面(含 vb_common 自身的 wasm/最小构建),比原缺陷更糟;`vb_common` 亦不得新增渲染类依赖(硬约束)。独立底层 crate 让依赖精确落在布局/渲染两个真实消费者上。
- `vb_layout` 的 `artboard_declared_height.rs` 跨层几何回归(需以渲染视角复验布局结果)改为 **dev-dependency**,库本体依赖方向不受影响。

### 1.2 `FontRegistry` 实例化 + 作用域上下文(COUP-06)

- 全部状态(项目 webfont 表、选字缓存、fontique Collection/SourceCache、缺字体清单 DOC-10)收进 `FontRegistry` 实例(`Mutex<Inner>`,poison 恢复语义)。
- **作用域上下文**:`enter_font_scope(Arc<FontRegistry>) -> FontScopeGuard`(Drop 弹出,panic 安全);kiln 三处调用点(domexport ×2、kiln-cli)改为「新建实例 → 逐条注册 → 进入作用域」,实例随导出任务生灭,**跨项目零残留**,并行 worker 各持实例互不阻塞、互不清除——比旧「全局 clear+register」语义严格更优。
- **全局便捷函数保留**(deprecated 语义,文档标注不加 `#[deprecated]` 属性以免下游 `-D warnings` 断):委托默认进程实例。原因:`vb_render::cpu::render_png` 签名被 vb_agent(禁改)钉死,vb_app/vb_web 亦禁改;单项目 GUI/CLI 单实例语义本就正确。线程局部作用域栈保证 kiln 分段并行 worker(同线程走完整导出链)正确命中各自实例。
- 已知残留(诚实清单):同进程**跨线程**共享默认实例的调用方(vb_app 单 UI 线程、vb_agent 单线程)不受影响;若未来出现「同进程多线程、不经作用域、多项目」的调用形态,应改为持实例 API(方法已齐备)。

### 1.3 同批配套

| 项 | 落点 |
|---|---|
| DOC-10 `@font-face` 读盘失败不再静默成 0 字节字体 | `register_font_file → Result<(), String>`,失败记入实例 `missing_fonts()` 清单;kiln 把清单并入导出告警(不静默) |
| PERF-01/DOC-09 Collection/SourceCache 实例内复用 + 选字缓存 + 去整份字体拷贝 | `FontRegistry::resolve`(键 = 族/字重/文本,有界 256 条);`ShapedRun.font_data` 直接复用 `Arc` |
| PERF-01 O(n²) 逐字形索引 → 单趟字节偏移前缀表 | `char_offsets`(`vb_textmeasure` 公开工具);`break_lines`/`split_line_segments`/`for_each_styled_line`/`cpu.rs` 改查表 |
| DOC-11 公共 API 非空守卫 | `measure_text_weighted` 去 `expect("nonempty")`;`split_line_segments` 空行守卫。`break_lines` 空行输出语义**有意保留**(行计数进入布局高度,改动即像素变化,违反「渲染输出逐位不变」硬约束) |
| 基准 | `vb_textmeasure/examples/shape_bench.rs`(1000 字符 × 30 次整形 + 量测,前后对比入迭代报告) |

## 2. 验收

- `cargo test -p vb_textmeasure -p vb_render -p vb_layout -p vb_common -p vb_doc` 全绿;vb_render 既有确定性快照(golden/parity)测试逐位不变。
- `cargo check -p vb_kiln -p vb_export -p vb_web -p vb_app -p vb_agent` 全过(vb_app/vb_agent 零改动)。
- 方向断言:`vb_layout` src 零 `vb_render` 引用(测试钉死)。
- 基准:1000 字符整形 ≥50% 降幅(数据见 2026-10-05 迭代报告)。
