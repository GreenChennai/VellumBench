//! CDP(Chrome DevTools Protocol)连接:同步 JSON-RPC over WebSocket。
//!
//! 事件与响应共用一条连接:`call` 等待匹配 id 的响应,途中收到的事件排队;
//! `drain_events` 由上层(PageSession)消费并维护状态标志。

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ws::WsConn;

pub struct Cdp {
    ws: WsConn,
    next_id: u64,
    responses: HashMap<u64, Result<Value, String>>,
    events: VecDeque<(String, Value)>,
}

impl Cdp {
    pub fn new(ws: WsConn) -> Self {
        Cdp {
            ws,
            next_id: 0,
            responses: HashMap::new(),
            events: VecDeque::new(),
        }
    }

    /// 发送命令并阻塞等待其响应(途中事件排队,不丢)。
    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.send(method, params)?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(res) = self.responses.remove(&id) {
                return match res {
                    // pump 已解到 CDP result 层,此处不得再取 .result
                    // (captureScreenshot/printToPDF 的返回值顶层即 data 键)
                    Ok(v) => Ok(v),
                    Err(e) => Err(format!("{method} 传输失败: {e}")),
                };
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!("{method} 等待响应超时"));
            }
            self.pump(remaining.min(Duration::from_millis(100)))?;
        }
    }

    fn send(&mut self, method: &str, params: Value) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = if params.is_null() {
            json!({ "id": id, "method": method })
        } else {
            json!({ "id": id, "method": method, "params": params })
        };
        if std::env::var("KILN_CDP_DEBUG").is_ok() {
            eprintln!("[cdp-send] {}", truncate(&msg.to_string()));
        }
        self.ws.send_text(&msg.to_string())?;
        Ok(id)
    }

    /// 泵一次 socket(至多 wait 时长),分发响应与事件。
    pub fn pump(&mut self, wait: Duration) -> Result<(), String> {
        let deadline = Instant::now() + wait;
        loop {
            match self.ws.poll_message() {
                Ok(Some(msg)) => {
                    let text = msg.text()?;
                    if std::env::var("KILN_CDP_DEBUG").is_ok() {
                        eprintln!("[cdp-recv] {}", truncate(text));
                    }
                    let v: Value = serde_json::from_str(text)
                        .map_err(|e| format!("CDP 消息解析失败: {e}: {}", truncate(text)))?;
                    if let Some(id) = v.get("id").and_then(Value::as_u64) {
                        if let Some(err) = v.get("error") {
                            self.responses.insert(id, Err(err.to_string()));
                        } else {
                            self.responses
                                .insert(id, Ok(v.get("result").cloned().unwrap_or(Value::Null)));
                        }
                        return Ok(()); // 有进展即返回,让调用方重新检查
                    } else if let Some(m) = v.get("method").and_then(Value::as_str) {
                        self.events.push_back((
                            m.to_string(),
                            v.get("params").cloned().unwrap_or(Value::Null),
                        ));
                        return Ok(());
                    }
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// 取走全部已排队事件。
    pub fn drain_events(&mut self) -> Vec<(String, Value)> {
        self.events.drain(..).collect()
    }

    /// 泵至谓词成立(事件已入队即检查),带总超时。
    pub fn pump_until(
        &mut self,
        predicate: impl Fn(&Cdp) -> bool,
        timeout: Duration,
    ) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            if predicate(self) {
                return Ok(());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("等待事件超时".into());
            }
            self.pump(remaining.min(Duration::from_millis(50)))?;
        }
    }

    pub fn close(&mut self) {
        self.ws.close();
    }
}

fn truncate(s: &str) -> String {
    if s.len() <= 200 {
        s.to_string()
    } else {
        format!("{}…", &s[..200])
    }
}
