//! 插件宿主(09-J,副文档 05 §05-10;ADR-VB-L12)。
//!
//! **架构裁定(ADR-VB-L12)**:插件 = **外部子进程**,经 **stdio 上的
//! 换行分隔 JSON-RPC 2.0** 与宿主通信 —— 与 `vellum-mcp`(vb_agent)
//! 同构,**零新外部依赖**。被否决:WASM(需 wasmtime/wasmer,重依赖)、
//! 内置脚本引擎(引入新语言栈)。
//!
//! 三条安全红线(违反即回归):
//! 1. **默认零权限**:插件只能调用其 `plugin.json` manifest `commands`
//!    白名单内的宿主命令;越权调用 → 拒绝(-32001)+ 记录(日志环);
//! 2. **崩溃隔离**:插件是独立进程 —— 崩溃 / 超时被杀,宿主只把该插件
//!    状态转为 [`host::PluginState::Crashed`],主程序绝不受影响;Windows
//!    上另有 Job Object 兜底([`sandbox`],KILL_ON_JOB_CLOSE,整树收割);
//! 3. **无直改通道**:插件**没有**直改 index.html 的通道,只能经
//!    `runCommand` 走宿主命令(可撤销、与 UI 同一路径);文档侧只有
//!    [`projection`] 的只读投影,不发写捷径。
//!
//! **白名单的边界(ADR-0052,如实声明)**:上述红线只约束**协议面**;
//! 插件进程本身是原生进程,拥有用户全权 —— 进程级遏制靠 Job Object 最小
//! 沙箱 + 安装/授权期显式告知([`NATIVE_PROCESS_DISCLOSURE`])。
//!
//! 模块地图:
//! - [`manifest`] —— `plugin.json` 严格 schema 解析(未知字段拒绝,中文报错);
//! - [`protocol`] —— JSON-RPC 2.0 帧编解码(两侧共用,宿主与示例插件同源);
//! - [`process`] —— 子进程 spawn / stdio 读写线程 / 请求-响应 + 超时;
//! - [`sandbox`] —— Windows Job Object 最小沙箱(KILL_ON_JOB_CLOSE + 内存/
//!   进程数限额;PLG-01/08,ADR-0052);
//! - [`auth`] —— 授权与安装登记持久化(`plugins.json`,与 recent.json 同范式;
//!   授权绑定入口可执行路径 + SHA-256,PLG-02);
//! - [`digest`] —— SHA-256(零依赖实现,授权入口指纹用);
//! - [`host`] —— 插件注册表、权限闸门、状态机(Stopped/Starting/Running/
//!   Crashed/Unauthorized)、日志环与逐帧 poll;
//! - [`projection`] —— 文档只读投影(结构摘要:画板/节点树 JSON + 计数)。
//!
//! ABI 速览(给插件作者):
//! - 握手:宿主发请求 `initialize`
//!   `{pluginId, hostVersion, protocolVersion, capabilities:[...]}` → 插件回
//!   `result:{protocolVersion, name, version}`(**版本协商**:不一致握手失败,
//!   PLG-06);
//! - 之后宿主可发请求 `shutdown`;通知 `event/button {panel, action}`、
//!   `event/input {panel, id, value}`(面板交互回流);
//! - 插件可发请求 `runCommand {command}`(白名单约束)、
//!   `doc/projection {}`(只读投影);通知 `panel/setUI {panel, widgets}`
//!   (受控 UI 描述)、`log {level, message}`。
//!
//! **原生进程权限告知(PLG-01 (c),ADR-0052)**:插件不是解释执行的受限
//! 代码,而是拥有用户全部权限的原生进程。授权/安装对话框必须原样展示
//! [`NATIVE_PROCESS_DISCLOSURE`] 并取得用户显式勾选
//! ([`NativeProcessConsent`]),再走 [`host::PluginHost::authorize_with_consent`]。

pub mod auth;
pub mod digest;
pub mod host;
pub mod manifest;
pub mod process;
pub mod projection;
pub mod protocol;
pub mod sandbox;

/// 协议版本(握手回包里回给宿主;插件侧也用它核对宿主声明)。
pub const PROTOCOL_VERSION: &str = "1.0";

/// 宿主能力名(`initialize.capabilities` 数组元素;插件据此自适应)。
pub const HOST_CAPABILITIES: &[&str] = &["runCommand", "docProjection", "panels", "exports"];

/// 默认请求超时(毫秒;05-10-1:每请求可配,缺省 5s)。
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;

/// 安装/授权对话框必须原样展示的权限告知文案(PLG-01 (c);ADR-0052)。
/// 语气裁定:不淡化("等同你本人权限"是事实,不是修辞)。
pub const NATIVE_PROCESS_DISCLOSURE: &str = "插件是原生进程:它拥有与你本人相同的系统权限,可以直接读写你的文件、访问网络、执行程序。宿主通过命令白名单与进程限额约束它,但这些约束只对诚实插件有约束力。请只安装你信任来源的插件。";

/// 用户显式同意的类型化凭证(PLG-01 (c)):只能由
/// [`NativeProcessConsent::from_dialog_checkbox`] 在 UI 拿到用户勾选后
/// 构造,拿不到勾选就构造不出来 —— 授权路径把它当必填合同。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeProcessConsent {
    granted: bool,
}

impl NativeProcessConsent {
    /// UI 在展示 [`NATIVE_PROCESS_DISCLOSURE`] 并取得用户显式勾选后调用;
    /// `false`(未勾选)返回 None —— **没有默认同意**。
    pub fn from_dialog_checkbox(checked: bool) -> Option<Self> {
        if checked {
            Some(NativeProcessConsent { granted: true })
        } else {
            None
        }
    }

    /// 是否已勾选(恒 true;存在性即同意)。
    pub fn is_granted(&self) -> bool {
        self.granted
    }
}
