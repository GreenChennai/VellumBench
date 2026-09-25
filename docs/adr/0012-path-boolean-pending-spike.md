# ADR-0012: 路径布尔方案(Spike 已结题)

- 状态:**已采纳 flo_curves 0.8**(Spike 完成,证据见 `crates/vb_tools/tests/boolean_spike.rs`)
- 候选评估:
  - **flo_curves(采纳)** — 纯 Rust,crates.io 双许可 MIT/Apache-2.0(依赖自身许可,
    宽松可用于本仓;本仓协议后经 ADR-0030 改为 ACL-1.0,不冲突);
    `flo_curves::bezier::path` 提供贝塞尔级布尔(`path_add/path_sub/path_intersect/path_xor`),
    曲线进曲线出,无需先多边形化
  - skia-safe(否决)— 功能足够但捆绑整个 Skia C++ 库,构建链/体积代价与本项目不成比例
  - vello_cpu 光栅+矢量化(否决)— 质量差、边界抖动,只配做兜底兜不了"精确"
- Spike 证据(2026-09-13,`boolean_spike.rs` 5 测试全绿):
  1. 并集/差集/交集对重叠矩形面积全部命中预期(60000/30000/10000,采样近似 ±2.5%);
  2. kurbo BezPath → `BezierPathBuilder` 重建可行:本项目 Vector 节点的 M/L/C 元素流
     一一映射(`line_to`/`curve_to`);结果侧 `path.points()` 逐段产出
     (控制柄, 控制柄, 终点) 三元组,可回填 kurbo;
  3. 性能:debug 构建 1000 次两矩形并集 ~0.1s(单次约 100µs),交互式路径查找器
     单步成本可忽略,release 量级再低一个数量级。
- Spike 过程中确认的 API 事实(写实现时直接用):
  - `BezierPathBuilder::<SimpleBezierPath>::start(pt).line_to(..)..build()` 直接返回
    `(起点, Vec<(c1, c2, 终点)>)` 元组(`SimpleBezierPath` 即该元组),**不是** Result;
  - 布尔函数签名:`path_add::<SimpleBezierPath>(&Vec<P1>, &Vec<P2>, 精度) -> Vec<SimpleBezierPath>`,
    两参数都是**路径列表**(可一次对编组做布尔);
  - 结果采样走 `BezierPath` trait 的 `start_point()` + `points()`,crate 上没有 `curves()` 方法。
- 落地计划(v0.4 路径查找器):`vb_tools` 增加 `boolean.rs` 转换层
  (kurbo BezPath ↔ SimpleBezierPath,处理 MoveTo 起始/ClosePath 语义),
  命令层新增 `PathBoolean { op, lhs, rhs }` 可逆命令;GUI 路径查找器面板四键接通。
  开放问题:布尔结果的颜色/描边取自操作数哪一方(AI 默认取上层)。

---

## 追记(2026-09-24,05-3 / X-1:多结果底座 + 10 运算全通)

- **多结果事务**:`Command::MultiResult`(删 N 源 + 按序插 M 结果,一条撤销;
  结果 sid 由调用方经 `Document::alloc_sid` 预分配,undo/redo 往返稳定;
  落点策略 `ReplaceAnchor`/`OnTop`)。服务路径查找器多结果三运算与 05-8 主件同步。
- **分割/修边/轮廓**:`vb_tools::pathfinder` ——
  分割 = 增量原子分解(逐形状按交/差切开 + 补未覆盖部分,复用 flo 布尔核);
  修边 = 逐形状减去其上方形状(**逐个相减**,flo 路径列表是单个 even-odd 区域,
  重叠列表会被偶奇 XOR)+ 同填充色合并;轮廓 = 线段级 `curve_intersects_curve_clip`
  求交切分,输出开放描边线(同形状自交不切,显式边界)。
- **绕向规范化**:flo 布尔输出的孔洞子路径绕向不保证与外环相反(nonzero 填充
  会把孔渲染成实体),`piece_from_flo` 按包含深度定绕向(外环正 / 孔负)。
- **输出语义**写在对象菜单悬停提示(`PATHFINDER_TIPS`)与本模块单测;
  填充继承「覆盖碎片的最上层源」,轮廓置 `fill:none` 并继承描边(无描边补 1px 黑)。
- 已知边界:CPU 导出端 `parse_fill` 不读矢量节点的 `fill`/`stroke` 声明
  (只认 background-color 族),路径查找器产物在 PNG 导出中暂以默认色呈现 ——
  与画布矢量渲染的既有近似一致,归渲染侧后续补齐。
