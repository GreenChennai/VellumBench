//! JSON-RPC 2.0 帧编解码(05-10-1;宿主与示例插件**共用同一实现**,
//! 与 `vellum-mcp` 的 stdio 换行分隔帧同构)。
//!
//! 帧形态(每帧一行 UTF-8 JSON,`\n` 结尾):
//! - 请求:  `{"jsonrpc":"2.0","id":N,"method":"...","params":{...}}`
//! - 响应:  `{"jsonrpc":"2.0","id":N,"result":...}` / `{"jsonrpc":"2.0","id":N,"error":{"code":..,"message":".."}}`
//! - 通知:  `{"jsonrpc":"2.0","method":"...","params":{...}}`(无 id)
//!
//! 方法名与错误码在这里集中(两侧硬编码同一张表,漂移会被集成测试拦住)。

use serde_json::{json, Value};

// ── 方法名(宿主 ⇄ 插件)──

/// 宿主 → 插件 请求:握手。
pub const M_INITIALIZE: &str = "initialize";
/// 宿主 → 插件 请求:礼貌停机(插件回包后自行退出;超时则宿主强杀)。
pub const M_SHUTDOWN: &str = "shutdown";
/// 宿主 → 插件 通知:面板按钮点击。
pub const M_EVENT_BUTTON: &str = "event/button";
/// 宿主 → 插件 通知:面板输入框提交(回车)。
pub const M_EVENT_INPUT: &str = "event/input";
/// 宿主 → 插件 通知:插件已进入 Running(插件此时可拉取投影/初始化面板)。
pub const M_EVENT_STARTED: &str = "event/started";

/// 插件 → 宿主 请求:调用宿主命令(**白名单约束**)。
pub const M_RUN_COMMAND: &str = "runCommand";
/// 插件 → 宿主 请求:取文档只读投影。
pub const M_DOC_PROJECTION: &str = "doc/projection";
/// 插件 → 宿主 通知:设置面板受控 UI 描述。
pub const M_PANEL_SET_UI: &str = "panel/setUI";
/// 插件 → 宿主 通知:写宿主日志环。
pub const M_LOG: &str = "log";

// ── 错误码(JSON-RPC 预留段 -32768..-32000 之外为自定义段)──

/// 解析错误(JSON-RPC 标准)。
pub const E_PARSE: i64 = -32700;
/// 无此方法(JSON-RPC 标准)。
pub const E_METHOD_NOT_FOUND: i64 = -32601;
/// 参数非法(JSON-RPC 标准)。
pub const E_INVALID_PARAMS: i64 = -32602;
/// 越权调用(自定义:**manifest.commands 白名单外**)。
pub const E_PERMISSION_DENIED: i64 = -32001;
/// 宿主命令执行失败(自定义)。
pub const E_COMMAND_FAILED: i64 = -32002;
/// 文档投影不可用(自定义:如宿主无打开文档)。
pub const E_PROJECTION_UNAVAILABLE: i64 = -32003;

/// 一条出站/入站帧(解码后的中立表示,两侧共用)。
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// 请求(带 id,需要应答)。
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    /// 通知(无 id,不要求应答)。
    Notification { method: String, params: Value },
    /// 对方对我们请求的应答。
    Response {
        id: Value,
        result: Result<Value, RpcError>,
    },
}

/// JSON-RPC 错误对象。
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        RpcError {
            code,
            message: message.into(),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({"code": self.code, "message": self.message})
    }

    pub fn from_value(v: &Value) -> Option<RpcError> {
        Some(RpcError {
            code: v.get("code")?.as_i64()?,
            message: v.get("message")?.as_str()?.to_string(),
        })
    }
}

/// 编码:请求帧(带行尾换行;调用方直接写 stdio 即可)。
pub fn encode_request(id: u64, method: &str, params: &Value) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
    )
}

/// 编码:通知帧。
pub fn encode_notification(method: &str, params: &Value) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "method": method, "params": params})
    )
}

/// 编码:成功响应帧。
pub fn encode_response(id: &Value, result: &Value) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": id, "result": result})
    )
}

/// 编码:错误响应帧。
pub fn encode_error(id: &Value, err: &RpcError) -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": id, "error": err.to_value()})
    )
}

/// 编码:解析错误应答(id 未知,回 null;JSON-RPC §4.1 要求必须回)。
pub fn encode_parse_error() -> String {
    format!(
        "{}\n",
        json!({"jsonrpc": "2.0", "id": null, "error": {"code": E_PARSE, "message": "Parse error"}})
    )
}

/// 解码一行(不含行尾换行)。
///
/// 空行 → `Ok(None)`(静默跳过);坏 JSON → `Err`(调用方回 -32700);
/// 语义不完整(缺 method / 非对象)→ `Err`。
pub fn decode_line(line: &str) -> Result<Option<Frame>, String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(line).map_err(|e| format!("JSON 解析失败:{e}"))?;
    let obj = v.as_object().ok_or("帧必须是 JSON 对象")?;
    if obj.get("jsonrpc").and_then(|x| x.as_str()) != Some("2.0") {
        return Err("缺少 jsonrpc:\"2.0\" 标记".into());
    }
    let method = obj.get("method").and_then(|x| x.as_str());
    let id = obj.get("id").cloned();
    match (method, id) {
        // 有 method 无 id(或显式 null)= 通知
        (Some(m), None | Some(Value::Null)) => Ok(Some(Frame::Notification {
            method: m.to_string(),
            params: obj.get("params").cloned().unwrap_or(Value::Null),
        })),
        // 有 method 有 id = 请求
        (Some(m), Some(id)) => Ok(Some(Frame::Request {
            id,
            method: m.to_string(),
            params: obj.get("params").cloned().unwrap_or(Value::Null),
        })),
        // 无 method 有 id = 对方对我们请求的响应
        (None, Some(id)) => {
            if let Some(err) = obj.get("error") {
                let e = RpcError::from_value(err).ok_or("error 对象缺少 code/message")?;
                Ok(Some(Frame::Response { id, result: Err(e) }))
            } else if obj.contains_key("result") {
                Ok(Some(Frame::Response {
                    id,
                    result: Ok(obj.get("result").cloned().unwrap_or(Value::Null)),
                }))
            } else {
                Err("响应帧缺少 result 或 error".into())
            }
        }
        (None, None) => Err("帧既无 method 也无 id(非法)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_roundtrip() {
        let s = encode_request(7, M_RUN_COMMAND, &json!({"command": "edit.undo"}));
        assert!(s.ends_with('\n'));
        let f = decode_line(&s).unwrap().unwrap();
        match f {
            Frame::Request { id, method, params } => {
                assert_eq!(id, json!(7));
                assert_eq!(method, "runCommand");
                assert_eq!(params["command"], "edit.undo");
            }
            other => panic!("应解出请求帧,得到 {other:?}"),
        }
    }

    #[test]
    fn notification_has_no_id() {
        let s = encode_notification(M_PANEL_SET_UI, &json!({"panel": "p1", "widgets": []}));
        match decode_line(&s).unwrap().unwrap() {
            Frame::Notification { method, params } => {
                assert_eq!(method, "panel/setUI");
                assert_eq!(params["panel"], "p1");
            }
            other => panic!("应解出通知帧,得到 {other:?}"),
        }
    }

    #[test]
    fn response_ok_and_error() {
        let ok = decode_line(&encode_response(&json!(1), &json!({"ok": true})))
            .unwrap()
            .unwrap();
        match ok {
            Frame::Response { id, result } => {
                assert_eq!(id, json!(1));
                assert_eq!(result.unwrap()["ok"], true);
            }
            other => panic!("{other:?}"),
        }
        let bad = decode_line(&encode_error(
            &json!(2),
            &RpcError::new(E_PERMISSION_DENIED, "越权"),
        ))
        .unwrap()
        .unwrap();
        match bad {
            Frame::Response { id, result } => {
                assert_eq!(id, json!(2));
                let e = result.unwrap_err();
                assert_eq!(e.code, E_PERMISSION_DENIED);
                assert_eq!(e.message, "越权");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn blank_and_malformed_lines() {
        assert!(decode_line("   ").unwrap().is_none(), "空行静默跳过");
        assert!(decode_line("{oops").is_err(), "坏 JSON 必须报错(回 -32700)");
        assert!(decode_line("[1,2]").is_err(), "非对象帧必须报错");
        assert!(
            decode_line(r#"{"jsonrpc":"2.0"}"#).is_err(),
            "既无 method 也无 id 的帧必须报错"
        );
    }

    #[test]
    fn parse_error_frame_shape() {
        let f = decode_line(&encode_parse_error()).unwrap().unwrap();
        match f {
            Frame::Response { id, result } => {
                assert_eq!(id, Value::Null);
                assert_eq!(result.unwrap_err().code, E_PARSE);
            }
            other => panic!("{other:?}"),
        }
    }
}
