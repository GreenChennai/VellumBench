# ADR-0052: 插件进程沙箱最小版——Job Object 兜底 + 安装期显式告知(WASM 路线重估否决)

- 状态:**已裁定并落地**(2026-10-04 迭代审查 PLG-01/PLG-08 P0;实现见 `vb_plugin::sandbox`、`vb_plugin::process`、`vb_plugin::lib::NATIVE_PROCESS_DISCLOSURE`)
- 背景:2026-10-04 全仓审查 §3.4 PLG-01(P0)。此前"零权限/无直改通道"三条安全红线只约束 **JSON-RPC 协议面**:插件进程本身是普通用户权限的原生进程,可直接读写文件、联网、执行任意命令(`process.rs::spawn` 只设 stdio 三管道),完全绕过白名单;白名单只对诚实插件有约束力。同时无 job/进程组兜底,插件派生的孙进程可能成为孤儿(PLG-08)。

## 1. 裁定

方向 **(a) + (c) 组合**;**(b) WASM/WASI 路线重估后否决**。

### 1.1 (a) Windows Job Object 最小沙箱(已落地)

- `CreateJobObjectW` + `SetInformationJobObject(JobObjectExtendedLimitInformation)`:
  - **`KILL_ON_JOB_CLOSE`**:job 句柄由 `JobGuard` 持有,随 `PluginProcess` Drop 关闭 → job 内全部进程被内核终止。插件(及其派生的**孙进程**,自动继承 job)在宿主崩溃/退出时被整树收割,不留孤儿——**同时关闭 PLG-08**;
  - **`PROCESS_MEMORY` 限额 = 1 GiB**(`sandbox::PLUGIN_MEMORY_LIMIT_BYTES`):失控分配在插件进程内失败,不拖垮宿主;
  - **`ACTIVE_PROCESS` 限额 = 32**(`sandbox::PLUGIN_MAX_PROCESSES`):防 fork 炸弹。
- **零新依赖**:Win32 API(5 函数 + 3 结构体)以 `extern "system"` 手工声明,维持 ADR-VB-L12 的插件宿主零新外部依赖纪律。
- **降级必须可观测(RB-06)**:assign/limit 失败(如宿主已处于不兼容 job 的极端环境)→ 插件照常运行,但 `sandbox_note` 如实写入插件日志环(`log::warn` 同步)。**有意 fail-open**:fail-closed 会在这类宿主上整体杀死插件功能,残余风险靠 (c) 与本 ADR 声明兜住。

### 1.2 (c) 安装/授权期显式告知(合同已落地,UI 接线待办)

- 事实文案 [`NATIVE_PROCESS_DISCLOSURE`](插件 = 原生进程、等同用户本人权限、白名单只约束诚实插件),语气裁定:**不淡化**——"等同你本人权限"是事实不是修辞;
- 类型化同意凭证 `NativeProcessConsent`:只能由 `from_dialog_checkbox(true)` 构造,未勾选构造不出 → `PluginHost::authorize_with_consent` 拒绝授权(**没有默认同意**);
- 既有 `authorize()` 保留为兼容路径(vb_app 现网调用),但打 `tracing::warn` 点名走的是兼容路径。
- **待办(下一 UI 批)**:vb_app 插件管理/授权对话框展示该文案 + 强制勾选 + 改调 `authorize_with_consent`。本轮受"不动 vb_app"硬约束未接线。

### 1.3 (b) WASM/WASI 重估:否决

- 收益面:真·能力沙箱(无文件系统/网络访问,除非显式授给)确实优于 Job Object;
- 成本面:wasmtime/wasmer 为数十 MB 量级的重依赖(与 ADR-VB-L12 零新依赖裁定冲突),现有插件 ABI(stdio JSON-RPC + 受控面板 + 宿主命令白名单)需整体重写,示例插件/门禁/授权持久化全部迁移;WASI preview 2 的组件模型仍在快速演化,存在绑定层返工风险;
- 替代面:本轮 Job Object(资源限额 + 生命周期兜底)+ 授权哈希绑定(PLG-02)+ 路径包含校验(PLG-03)+ 流量上限(PLG-04)已覆盖"失控/越界/孤儿"三类主要事故面;WASM 的增量收益集中在"恶意插件主动越权"场景,而该场景在安装期告知 + 来源信任模型下已有显式用户决策点。
- **结论**:维持子进程形态;WASM 作为"插件市场/不受信来源"立项时的前置项重评,不进当前迭代。

## 2. 同批配套(防线完整性)

| 项 | 落点 |
|---|---|
| PLG-02 授权绑定入口路径 + SHA-256(`digest.rs` 零依赖实现,FIPS 向量钉正确性) | `auth.rs`(schema v2;v1 授权无入口绑定 → 迁移为未授权,fail-safe) |
| PLG-03 entry 路径穿越:`..` 直接拒绝 + 词法规范化主判定 + canonicalize 兜底(ADR-0045 同款两层),含目录联接逃逸用例 | `manifest.rs::resolve_entry_checked` |
| PLG-04 stdout 单行 8MB 上限 + 入站/响应队列 1024 上限,超限断连且原因可观测 | `process.rs`(读线程有界行读 + `dead_reason`) |
| PLG-05 poll 每帧入站处理上限 32(UI 线程帧时间有界) | `host.rs::MAX_INCOMING_PER_POLL` |
| PLG-06 握手版本协商:插件协议版本与宿主不一致 → 握手失败 | `host.rs::handshake` |
| PLG-07 `call` 忙等 5ms → Condvar 定时等待;pending 登记挪入 `call`,退出路径全清理 | `process.rs` |

## 3. 残余风险(如实声明)

1. **无令牌降权/系统调用过滤**:插件仍是用户全权进程,Job Object 只限资源与生命周期,不限访问对象;恶意插件可以越权读写——防线是安装期显式告知 + 来源信任,不是技术隔离;
2. **assign 竞态窗口**:`spawn` 与 `AssignProcessToJobObject` 之间插件理论上可抢先派生脱离进程(需要子进程在几微秒内故意作恶;挂起再 assign 需放弃 `std::process::Command`,列为后续);
3. **授权快照哈希在授权时点计算**:授权与启动之间文件被换 → 启动时现算哈希拦截;启动后运行中被换 → 不在防线内(进程已在跑);
4. **PLG-05 只做每帧预算,未做后台线程化**:`HostServices` trait 携带 `&mut dyn`(非 Send),命令/投影真正移出 UI 线程需要 vb_app 侧接线(本轮硬约束不动 vb_app);
5. **非 Windows 平台无进程树沙箱**:恒降级并如实记录(`sandbox::attach` 恒 Err → `sandbox_note` 入日志环)。
