# example-stats — 统计元素(示例插件,09-J)

VellumBench 自带的最小插件模板(05-10-6),演示插件 ABI 的完整链路:

```
装载 → 授权(弹窗列权限)→ 启动(握手)→ 拉只读投影 → 受控面板显示
```

## 功能

- 收到 `event/started` 或点面板「刷新统计」→ 调 `doc/projection` 取文档
  **只读投影**,把 节点/文本/图片/编组/矢量 计数以受控元件
  (text/metric/button/input)显示在「插件」次级坞的「元素统计」面板;
- 面板按钮「全选对象」→ 调 `runCommand edit.select_all`(manifest 白名单
  内的宿主命令,可撤销、与 UI 同一路径)。

## 文件

- `plugin.json` — manifest(严格 schema):`id/name/version/entry/
  commands(命令白名单)/panels/exports`;
- 插件本体是本 workspace 的 bin:`crates/vb_plugin/src/bin/example-stats-plugin.rs`,
  manifest `entry` 用 `bin:example-stats-plugin` 表示「与宿主可执行同目录」,
  cargo 构建后两者同在 `target/debug/`。

## 安装/启用

1. 构建 workspace(`cargo build` 出 `target/debug/example-stats-plugin(.exe)`);
2. VellumBench → 编辑 → 插件管理… → 安装…(选择本目录的 `plugin.json`);
3. 勾选启用 → 首次弹授权窗(列出上面 manifest 的全部权限)→ 确认启用;
4. 视图 → 插件面板(或授权后自动打开)查看统计。

## 冒烟

从仓库根启动:

```
VB_PLUGIN_SMOKE=1 vellumbench
```

自动装载本示例 + 授权 + 启动 + 打开管理窗与插件面板(**自动授权仅限冒烟
夹具**,正常 UI 必须人工确认)。

## 门禁(05-10-7)

`cargo test -p vb_plugin` 覆盖:install→enable→run→面板可见;越权调用被拒
并记录;插件崩溃宿主存活且可重启;manifest schema 校验;JSON-RPC 编解码与
握手超时杀。
