# Changelog

## 0.10.0(2026-09-26)

### Fixed

- **VB-1(P0)· 静态服务越界判定与目录联接不兼容**(ADR-0045):本地静态
  服务的 404 判定原用 `canonicalize()`,会把项目内声明的目录联接
  (`src/fonts` → 技能字体库等减重影子)解析到 root 之外,合法字体/脚本
  全部 404 且浏览器静默回退系统字体。现改为词法规范化主判定(折叠 `./..`,
  拒绝越出 root)+ 对词法越出的请求再查 canonicalize 是否落在允许根集合;
  `../` 逃逸、绝对路径与反斜杠注入仍被拒绝(安全回归锁定)。联接下
  woff2/js 恢复 200。
- **VB-2(P0)· native/矢量道静默丢弃不支持原语**(ADR-0046):自研引擎与
  矢量写出器对 3D/透视 transform、mask、box-shadow、mix-blend-mode、不可
  解析的 clip-path 形状、内联 SVG 子树此前丢弃/栅格化后零告警。现于
  构建期扫描、采集计数与写出器检出三处收集,同属性同车道聚合计数,命中置
  `degraded:true`;SVG/EPS/Ai/PPTX 写出器不表达 clip-path 也在交付前检出。

### Added

- 新告警变体:`AssetNotFound{src}`(静态资源 404 必然留痕,不再静默)、
  `UnsupportedPropertyDropped{prop,count,lane}`、
  `InlineSvgRasterized{count,format}`、`ClipShapeApproximated{shape,count}`、
  `ImportObjectSkipped{kind,count}`(均含稳定 `kind()` 键与
  `is_degrading()` 语义)。
- `KilnReport.warnings_by_kind` / `count_of(kind)`:按稳定小写蛇形键聚合
  告警计数,导出/导入结果 JSON 同步输出 `warnings_by_kind`,下游可直接写
  门禁(如 `unsupported_dropped > 0` 拒绝交付矢量稿);kiln-cli 的 dom /
  anim / native / import 四路结果 JSON 均补齐 `degraded` 判定。
- `anim_coverage` 动画覆盖矩阵(VB-3):`anim.rs` 新增 `AnimCoverage`,
  按车道(browser 全量 / native 四类轨道)把本源关键帧属性分为
  animated / static_fallback / unsupported,随 GIF/MP4 导出结果输出。
- SVG 导入跳过清单强类型化(VB-4):`import_svg` 的跳过/近似类别映射为
  `ImportObjectSkipped` 进 KilnReport,倾斜变换计入清单;kiln-cli import
  结果与 export 同构(`warnings[]` + `degraded` + `warnings_by_kind`)。
- ADR-0045(静态服务越界判定与联接语义)、ADR-0046(降级可观测性契约);
  `crates/vb_kiln/docs/DESIGN.md` 增「降级必须可观测」一节。
