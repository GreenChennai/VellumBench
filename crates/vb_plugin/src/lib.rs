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
//!    状态转为 [`host::PluginState::Crashed`],主程序绝不受影响;
//! 3. **无直改通道**:插件**没有**直改 index.html 的通道,只能经
//!    `runCommand` 走宿主命令(可撤销、与 UI 同一路径);文档侧只有
//!    [`projection`] 的只读投影,不发写捷径。
//!
//! 模块地图:
//! - [`manifest`] —— `plugin.json` 严格 schema 解析(未知字段拒绝,中文报错);
//! - [`protocol`] —— JSON-RPC 2.0 帧编解码(两侧共用,宿主与示例插件同源);
//! - [`process`] —— 子进程 spawn / stdio 读写线程 / 请求-响应 + 超时;
//! - [`auth`] —— 授权与安装登记持久化(`plugins.json`,与 recent.json 同范式);
//! - [`host`] —— 插件注册表、权限闸门、状态机(Stopped/Starting/Running/
//!   Crashed/Unauthorized)、日志环与逐帧 poll;
//! - [`projection`] —— 文档只读投影(结构摘要:画板/节点树 JSON + 计数)。
//!
//! ABI 速览(给插件作者):
//! - 握手:宿主发请求 `initialize`
//!   `{pluginId, hostVersion, capabilities:[...]}` → 插件回
//!   `result:{protocolVersion:"1.0", name, version}`;
//! - 之后宿主可发请求 `shutdown`;通知 `event/button {panel, action}`、
//!   `event/input {panel, id, value}`(面板交互回流);
//! - 插件可发请求 `runCommand {command}`(白名单约束)、
//!   `doc/projection {}`(只读投影);通知 `panel/setUI {panel, widgets}`
//!   (受控 UI 描述)、`log {level, message}`。

pub mod auth;
pub mod host;
pub mod manifest;
pub mod process;
pub mod projection;
pub mod protocol;

/// 协议版本(握手回包里回给宿主;插件侧也用它核对宿主声明)。
pub const PROTOCOL_VERSION: &str = "1.0";

/// 宿主能力名(`initialize.capabilities` 数组元素;插件据此自适应)。
pub const HOST_CAPABILITIES: &[&str] = &["runCommand", "docProjection", "panels", "exports"];

/// 默认请求超时(毫秒;05-10-1:每请求可配,缺省 5s)。
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;
