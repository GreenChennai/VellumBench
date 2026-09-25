# ADR-0045: 静态服务越界判定用词法规范化为主,联接(junction/symlink)不视为越界

- 状态:已接受(第四轮迭代 B2 / VB-1,2026-09)
- 背景:`vb_browser::staticsrv` 的路径逃逸防护原用 `Path::canonicalize()`
  做 `starts_with` 判定。canonicalize **会解析符号链接/目录联接**,于是
  「项目内声明的联接」(artboard 脚手架默认形态:`src/fonts` → 技能字体库、
  `src/vendor` → 共享库,减重影子)被解析到 root 之外 → 合法字体/脚本全部
  404,浏览器**静默回退系统字体**,且导出 `ok:true` 零告警——下游直到肉眼
  看图才能发现(下游实测缺陷报告 VB-1)。
- 决策(方案 C:词法主判定 + canonicalize 兜底):
  1. **主判定**:词法规范化(折叠 `.`/`..`,`..` 越出顶层即拒绝,不解析
     联接、不触盘)后 `target` 仍以 `root` 开头 → 放行。`../` 类逃逸在词法
     层即可拦住;联接资源在此通过,不再被 canonicalize 误伤;
  2. **兜底**:对词法越出的请求,再查 `canonicalize()`(解析联接后的真实
     位置)是否落在允许根集合(默认 = root 自身规范形)内,任一成立即放行;
  3. **404 必须留痕**:静态服务线程把 404 请求路径写入共享收集器
     (`NotFoundLog`),导出结束并入 `KilnReport.warnings`
     (`KilnWarning::AssetNotFound{src}`)——字体/脚本 404 的静默降级从此可见。
- 安全边界(诚实声明):
  - 词法判定不解析联接,root 内指向 root 外的联接**不再拦截**。这是明确的
    工程取舍而非漏洞:联接是项目作者的选择,服务只监听 `127.0.0.1` 随机端口,
    攻击面为本地进程;
  - `../` 词法逃逸、绝对路径注入、反斜杠变体(`..%5C`)仍被拒绝
    (回归用例 `path_allowed_rejects_escape_and_accepts_inside` 与
    `junction_assets_are_served` 锁定);
  - 监听/端口策略不变(127.0.0.1,随机端口)。
- 落地:`crates/vb_browser/src/staticsrv.rs`(`lexically_normalize` /
  `path_allowed` / `NotFoundLog` / `take_not_found`)、
  `vb_browser::export_source`、`vb_kiln::domexport`/`animlane` 并入报告;
  版本 0.10.0。
- 取舍:方案 B(调用方显式声明允许根)安全最强但要求下游改调用,集成成本
  高;方案 A(纯词法)最小但丢了「联接解析后落回 root」的兜底。方案 C 以
  零调用方改动获得 A+B 的并集。
- 被否决替代:保留 canonicalize 主判定 + 白名单联接目录(要求 artboard
  脚手架声明全部联接,破坏「零配置减重」默认形态)。
