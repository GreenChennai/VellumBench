# Changelog

## 0.10.1(2026-09-30)

本轮为「一条龙体检」批次:三路并行深度审查(核心文档层 / 导出与 Agent 层 /
UI 与应用层)后逐项核实修复,重点打掉 Windows 发布阻断项与 L0/L1 往返承诺
的破口。

### Fixed(核心 / 往返幂等)

- **多行 Raw 片段每存一轮吸收一层缩进(打穿 L1 字节幂等)**:frozen 块 /
  `trailing_raw` / `head_extra` 的内部行在 canonical 序列化时逐行垫缩进,
  再导入时缩进进入 verbatim 内容,保存-重开循环里无限增长。现只给首行垫
  (首行 pad 落在开标签之前,不进入下次捕获),内部行逐字节原样写出。
- **`<pre>`/`<textarea>` 实体回写不转义(L0/L1 双破)**:`&lt;` 解码成 `<`
  后原样写回,二次解析变成真元素。现对该两类元素重新转义
  (`&amp;`→`&lt;`→`&gt;`),对已转义源字节稳定,对真 `<`/`&` 修复破坏。
- **NBSP 邻接处多插可见空格(L0 漂移)**:行内序列化的首尾空白探测与折叠
  口径不一致(Unicode vs ASCII),`&nbsp;` 相邻处误判"源有空白"。现统一
  `is_foldable_ws` 口径。
- **富文本段 lead 修剪错位**:`finalize_group` 的 text 用 ASCII 修剪而
  lead 用 Unicode 修剪,NBSP 起始的行内组段区间整体错位。
- **段注记区间缺 UTF-8 字符边界校验(导出 panic)**:Agent `set_segs` 对
  中文文本给非边界字节偏移,导出侧 `text[pos..s]` 字节切片直接 panic。
  现 `validate_segs` 在 apply/redo 双侧拒绝非边界区间。
- **画板可被移入非 root 容器后静默消失**:Move/Insert 只拦「非画板挂
  root」,不拦「画板挂非 root」,后者被 `sync_artboards` 除名后整棵子树
  从导出中消失且无告警。现补对称不变量(画板只能挂 root)。
- **带 BOM 的 CSS 首条规则静默丢失**:Windows 记事本出品的 `styles/main.css`
  带 UTF-8 BOM,`:root` 令牌与首个类选择器匹配失败整表降级。导入现剥
  BOM(html5ever 的 HTML 侧本就安全)。
- 命令面板「没有匹配的命令」用 `Color32::GRAY` 硬编码,归 `text_3` 令牌。

### Fixed(Kiln / 导出 / 浏览器车道)

- **`kiln-cli img blur --box` 区域语义反转**:box 外本应保持原图,实际
  whole 图全被模糊(combined 从模糊图克隆再写回模糊像素)。现从原图出发,
  仅 box 内写回模糊像素。
- **`kiln-cli` 诊断行 JSON 不转义**:`{"domwarn":"…"}` / `{"warn":"…"}` /
  `{"ok":false,"error":"…"}` 由 `format!` 手拼,warning 含 `"` 或 `\`
  (Windows 资产路径必然带 `\`)时产出非法 JSON,下游逐行 `json.loads`
  必炸。全部过 `jesc()`(主结果此前已修,告警/错误行漏网)。
- **静态服务 Drop 后端口泄漏**:accept 线程阻塞在 `incoming()`,Drop 只置
  `alive=0` 不够;批量导出时端口持续累积。现 Drop 发哑连接唤醒 accept,
  循环头见到停止位即退出。
- **浏览器启动失败泄漏临时目录**:`BrowserProcess` 未构造时 Drop 不执行,
  user-data-dir 遗留。现失败路径手动清理。
- **目录无 index.html 时两车道选不同入口**:domexport 取字母序最后一个、
  staticsrv 取第一个 → PNG 与 PDF/AI 可能渲染不同 HTML。统一为「index.html
  → index.htm → 字母序第一个」。
- **CDP 调试截断按字节切中文 panic**(`KILN_CDP_DEBUG` 门控):回退到字符
  边界再截断。
- **WebSocket connect 无超时 + 帧长无上限**:SYN 重试可挂数秒;对端声明
  64 位巨帧会无上限 `reserve`。connect 带与握手同源 deadline,帧长设
  256 MiB 上限。
- **SVG/PPTX 导出丢字体**:`<text>` 无 `font-family`,PPTX run 无
  `latin/ea/cs typeface`,设计字体全丢成查看器默认(度量按原字体整形,
  换默认字体行宽错位)。现按 run 落盘字体族。
- **PDF 导入 20 页截断静默**(ADR-0046 纪律):超出页数现在 warnings 里
  留痕。
- **SVG 导入退化几何 NaN 排序 panic**:`partial_cmp().unwrap()` 改
  `total_cmp`,含 NaN 直接放弃近似。
- **浏览器车道告警被吞**:kiln-cli 车道 B 分支只报 `warnings` 计数,
  告警内容不落 stderr,违反「降级必须可观测」(ADR-0046)。现逐条
  `{"domwarn":…}` 留痕。
- **favicon.ico 恒定 404 噪声**:favicon 是浏览器自发请求而非文档引用
  的资源,不计入 AssetNotFound 告警(文档引用的资源照旧留痕)。

### Fixed(UI / 应用层)

- **随包字体编译期绝对路径(Windows 发布阻断)**:`CARGO_MANIFEST_DIR`
  在用户机器上不存在,Inter/JetBrains Mono/MiSans 全体静默失效。现运行期
  多候选探测:`VB_FONTS_DIR` → `<exe>/assets/fonts` → `<exe>/../assets/fonts`
  → 开发布局;并新增 `%WINDIR%` 解析的系统 CJK 回退(此前硬编码
  `C:\Windows\Fonts`,装非 C 盘的机器中文变占位块),候选补等线/黑体。
- **画布纹理按逻辑点渲染(高分屏发糊)**:150%/200% 缩放下纹理只有物理
  像素的 1/1.5~1/2。现纹理按物理像素创建,vello 场景变换外乘
  `pixels_per_point`,画布内容与 UI 同等锐度。
- **外部改动热重载无去抖**:注释声称 200ms 窗口,实际事件到达即整页重载,
  编辑器连写多文件触发双次。现 200ms 静默窗收敛后统一处理。
- **自动保存快照落盘无路径消毒**:恢复中转对快照 `files` 表的相对路径
  直接 join,`..`/绝对路径可写出项目目录之外。现拒绝越界路径。
- **LayerRow 双击眼睛/锁误触重命名**:双击开关同时命中行级
  `double_clicked` 语义。现一并排除。
- **NumField 键盘步进覆盖输入草稿**:聚焦输入中按 ↑/↓,已键入未回车的
  表达式被格式化回显直接覆盖。现步进前先吸收草稿(步进基于草稿求值)。
- **最近项目全部固定时 LRU 删固定项**:`unwrap_or(len-1)` 兜底违反
  「固定项不淘汰」承诺。现全固定时容忍暂时超限。
- **启动主页缩略图缓存永不失效**:缓存键加 mtime,外部替换 `thumb.png`
  即时刷新。
- **导出对话框「透明背景」静默隐藏**:改置灰 + 悬停说明(仅 PNG 支持)。
- **画布底色硬编码 hex 与主题令牌漂移**:归 `bg_canvas` 唯一令牌源,
  与隔离遮罩同源。
- **插件面板状态/日志色硬编码 RGB**:归 `danger/success/warn/text_3` 令牌,
  深浅主题各自正确。
- **新建项目名未处理 Windows 保留设备名**:CON/PRN/AUX/NUL/COM1-9/LPT1-9
  与结尾 `.`/空格会让 `create_dir_all` 以难懂的 OS 错误失败。清洗层现
  加 `_` 前缀兜住(带单测)。

### Fixed(CLI / Agent)

- **`vellum-cli --help` 退出码 1**:求助与出错无法用 `$?` 区分。`--help`/
  `--version` 现退出 0,参数错误仍按退出码表。
- **WPI 桥裸跑 `python` 命中微软商店 stub**:优先 `py -3`(以即退的
  `--version` 探测,绝不裸跑 `py -3` 防 REPL 挂死),失败回退 `python`。
- **WPI 临时目录毫秒级碰撞**:`subsec_millis` 每秒重复,同进程并发导出可
  互删临时目录。改 `subsec_nanos`。
- **批量 CSV 编码错误信息不可读**:GBK/ANSI(中文 Excel 默认)报裸 UTF-8
  错误。现附「另存为 UTF-8」提示。

### Changed

- `kiln-cli --max-wait` help 文本改为如实声明「保留参数,当前不生效」
  (此前声称浏览器车道 settle 预算,误导)。

### Added

- `dist/package.ps1`:Windows 便携发行包脚本(GUI + 双 CLI + 示例 +
  调用封装 + 随包字体说明目录),与 `vb_ui::fonts` 的运行期查找布局对应。
- 浏览器发现补 `%LOCALAPPDATA%` 的 per-user Chrome 与
  `%PROGRAMFILES%`/`%ProgramFiles(x86)%` 环境变量派生路径。

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
