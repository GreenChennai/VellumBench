# ADR-0046: 降级必须可观测 —— KilnWarning + degraded 的完备性契约

- 状态:已接受(第四轮迭代 B2 / VB-2~VB-5,2026-09)
- 背景:Kiln 的可观测性通道早已存在(`KilnReport.warnings` / `degraded`),
  导入侧也早有「该丢就丢 + 清单告知」的实践(`import_svg` 的跳过清单),
  但**导出侧两条最关键的降级路径没有往通道里写**(下游实测 VB-2):
  native/矢量道对 3D/透视 transform、mask、box-shadow、mix-blend-mode、
  不可解析 clip-path、内联 SVG 等构造**静默丢弃/栅格化**,结果 JSON 里
  `warnings: []`、`degraded: false`,下游「改了没效果」却查不到原因。
- 决策(全导出路径的契约):
  1. **任何「输出与源不等价」的分支必须发强类型 `KilnWarning`**,不许只
     打日志或不留痕。本轮补齐的变体:
     `UnsupportedPropertyDropped{prop,count,lane}`(属性丢弃,同属性同车道
     聚合计数)、`InlineSvgRasterized{count,format}`、
     `ClipShapeApproximated{shape,count}`、`AssetNotFound{src}`(VB-1 的
     静态资源 404)、`ImportObjectSkipped{kind,count}`(VB-4 导入跳过);
  2. **聚合后发送**:同类告警合并为一条 + `count`(10 个同属性元素 →
     1 条 count=10),防告警刷屏;
  3. **命中即 `degraded = true`**(语义:输出与源不等价;
     `KilnWarning::is_degrading()` 为唯一判定来源,`report_with` 统一置位,
     写出器只做 `|=` 叠加,不得覆盖);
  4. **lane 三值**:browser(浏览器车道)/ native(自研光栅引擎)/
     vector(矢量写出器),下游按道分流;矢量写出器不表达的构造
     (SVG/EPS/Ai/PPTX 无 clip-path 发射)在交付前由公共告警统一检出;
  5. **JSON 结果可编程判定**:`warnings_by_kind`(键 = `KilnWarning::kind()`
     稳定小写蛇形键,如 `unsupported_dropped`)与 `anim_coverage`
     (动画覆盖矩阵,VB-3)随导出/导入结果输出;import 与 export 的
     结果 JSON 同构(均有 `warnings[]` + `degraded` + `warnings_by_kind`)。
- 落地:`error.rs`(变体 + `kind()` + `is_degrading()`)、`report.rs`
  (`warnings_by_kind` / `count_of`)、`context.rs`(画板子树属性扫描)、
  `dompaint.rs`/`domsnap.rs`(采集期计数)、`domexport.rs`
  (`meta_warnings` 定型层)、`writer.rs`(写出器丢弃检出)、
  `import_svg.rs`(跳过清单强类型化)、`anim.rs`/`animlane.rs`
  (`AnimCoverage`)、`bin/kiln-cli.rs`(四处结果 JSON);版本 0.10.0。
- 取舍:告警口径**宁多勿漏**(vector 道丢 2D matrix 的「近似」暂不告警,
  见下),换取下游可以放心地把「双道支持矩阵」从人工维护改为读取 JSON;
  聚合上限与 404 收集上限(64 条)防病态页面刷爆报告。
- 已知边界(carry-forward,非本轮承诺):
  - dom 快照道对 **2D** matrix 的不应用暂不发告警(本轮口径只覆盖
    3D/透视;2D 属能力扩张);
  - filter blur 在矢量写出器(EPS/Ai/SVG)中不可表达,暂未发告警;
  - `@property` / `stroke-dashoffset` 的静态化是**能力缺口**而非本轮
    缺陷,由 `anim_coverage.static_fallback/unsupported` 如实呈现。
