//! CDP(Chrome DevTools Protocol)连接:同步 JSON-RPC over WebSocket。
//!
//! 事件与响应共用一条连接:`call` 等待匹配 id 的响应,途中收到的事件排队;
//! `drain_events` 由上层(PageSession)消费并维护状态标志。

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::ws::WsConn;

/// 入站事件队列容量上限(RB-05:循环/队列必须有上限):长 CDP 等待期间
/// 喋喋不休的页面(高频 Network/动画事件)不再无界积压 —— 超限丢最旧,
/// 计数器可观测。load/crashed 等状态标志由消费侧逐条置位,丢旧事件只
/// 影响 networkidle 的静默判定新鲜度,不影响正确性。
const MAX_QUEUED_EVENTS: usize = 4096;

pub struct Cdp {
    ws: WsConn,
    next_id: u64,
    responses: HashMap<u64, Result<Value, String>>,
    events: VecDeque<(String, Value)>,
    dropped_events: u64,
}

impl Cdp {
    pub fn new(ws: WsConn) -> Self {
        Cdp {
            ws,
            next_id: 0,
            responses: HashMap::new(),
            events: VecDeque::new(),
            dropped_events: 0,
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

    /// 可取消版 [`Cdp::call`](硬骨头 #3 静态快照车道收口):等待循环按
    /// **≤100ms 泵片**推进(片数 = timeout/100ms,即 60s/180s/300s 等待
    /// 被拆成 600/1800/3000 片),每片之间查一次取消探针——命中即返回
    /// 取消标记错误([`crate::cancel::wait_cancelled`],三态可判)。
    ///
    /// 取消瞬点后迟到的响应留在 `responses` 表(新 call 用新 id 永不误配,
    /// 随连接关闭释放);连接与浏览器进程由上层 Drop 收割,本方法只管停等。
    pub fn call_cancellable(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        probe: &dyn Fn() -> bool,
    ) -> Result<Value, String> {
        // 起跑前已取消:不发命令,直接按取消收口
        if probe() {
            return Err(crate::cancel::wait_cancelled(&format!("{method}(起跑前)")));
        }
        let id = self.send(method, params)?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(res) = self.responses.remove(&id) {
                return match res {
                    Ok(v) => Ok(v),
                    Err(e) => Err(format!("{method} 传输失败: {e}")),
                };
            }
            if probe() {
                return Err(crate::cancel::wait_cancelled(&format!("{method} 等待响应")));
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
                        if self.events.len() >= MAX_QUEUED_EVENTS {
                            self.events.pop_front();
                            self.dropped_events += 1;
                        }
                        self.events.push_back((
                            m.to_string(),
                            v.get("params").cloned().unwrap_or(Value::Null),
                        ));
                        return Ok(()); // 有进展即返回,让调用方重新检查
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

    /// 入站事件超限丢弃计数(可观测;诊断喋喋不休页面)。
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events
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
        // 回退到字符边界:CDP 消息常含中文(DOM 文本),按字节切在
        // 多字节字符中间会 panic
        let mut end = 200;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}
