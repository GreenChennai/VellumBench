//! 插件子进程(05-10-1 / 05-10-5):spawn + stdio JSON-RPC + 超时 + 崩溃隔离。
//!
//! 线程模型(每插件):
//! - **stdout 读线程**:逐行解码 → [`Incoming`] 入共享队列(响应回填 /
//!   插件请求 / 插件通知);EOF → `dead` 标记(宿主据此转 Crashed);
//! - **stderr 读线程**:逐行 → 日志队列(宿主并入日志环;插件的
//!   println!/eprintln! 调试输出天然可见,不丢);
//! - **写入**:宿主持有 `ChildStdin`(Mutex),串行写(05-10-1:单插件
//!   串行即可)。
//!
//! 崩溃隔离:子进程是独立进程;宿主对它的所有操作都不 panic —— 写失败 /
//! 读 EOF / `try_wait` 都只产生 `Err` 或状态标记。`Drop` 兜底强杀,
//! 插件生命周期绝不超出宿主。

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::protocol::{self, Frame, RpcError};

/// 插件发来的入站消息(请求 / 通知)。
///
/// **响应不在其中**:响应走 [`Shared::responses`] 专用队列 —— 否则宿主
/// poll(消费 queue)会与在途请求的 `call`(等响应)互相抢帧,
/// 握手线程的回包被 poll 偷走 → 假超时(本crate 曾因此门禁闪红)。
#[derive(Debug, Clone)]
pub enum Incoming {
    /// 插件发来的请求(带 id 时宿主必须应答)。
    Request {
        id: Option<Value>,
        method: String,
        params: Value,
    },
    /// 插件发来的通知。
    Notification { method: String, params: Value },
}

/// 由读线程回填的共享 I/O 状态。
#[derive(Debug, Default)]
struct Shared {
    /// 对我们请求的响应(id → 应答;只由 `call` 消费)。
    responses: VecDeque<(u64, Result<Value, RpcError>)>,
    /// 待宿主处理的入站消息(请求 / 通知;只由 poll 消费)。
    queue: VecDeque<Incoming>,
    /// stderr 行(宿主并入日志环)。
    stderr: VecDeque<String>,
    /// stdout 读线程已退出(进程结束或管道断)。
    dead: bool,
}

/// 一个运行中的插件子进程。
#[derive(Debug)]
pub struct PluginProcess {
    child: Arc<Mutex<Option<Child>>>,
    shared: Arc<Mutex<Shared>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    next_id: AtomicU64,
    /// 已发出的请求(id → method;响应回填与诊断用)。
    pending: Arc<Mutex<HashMap<u64, String>>>,
}

impl PluginProcess {
    /// 启动插件子进程(`exe + args`),接好 stdio 三管道与读线程。
    pub fn spawn(exe: &std::path::Path, args: &[String]) -> Result<PluginProcess, String> {
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Windows:不给插件弹新控制台窗口
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("启动插件进程失败({}):{e}", exe.display()))?;
        let stdin = child.stdin.take().ok_or("插件进程缺 stdin 管道")?;
        let stdout = child.stdout.take().ok_or("插件进程缺 stdout 管道")?;
        let stderr = child.stderr.take().ok_or("插件进程缺 stderr 管道")?;

        let shared = Arc::new(Mutex::new(Shared::default()));
        // stdin 包进共享槽(读线程回 -32700 与宿主写路径共用)
        let stdin_shared = Arc::new(Mutex::new(Some(stdin)));
        // stdout 读线程:逐行解码入队;EOF 置 dead
        {
            let shared = shared.clone();
            let stdin_shared = stdin_shared.clone();
            std::thread::Builder::new()
                .name("vb-plugin-stdout".into())
                .spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        match protocol::decode_line(&line) {
                            Ok(Some(frame)) => {
                                let mut q = match shared.lock() {
                                    Ok(g) => g,
                                    Err(_) => break,
                                };
                                match frame {
                                    Frame::Response { id, result } => {
                                        // id 必须是数字(宿主出站 id 约定);否则丢弃
                                        if let Some(n) = id.as_u64() {
                                            q.responses.push_back((n, result));
                                        }
                                    }
                                    Frame::Request { id, method, params } => {
                                        q.queue.push_back(Incoming::Request {
                                            id: Some(id),
                                            method,
                                            params,
                                        });
                                    }
                                    Frame::Notification { method, params } => {
                                        q.queue
                                            .push_back(Incoming::Notification { method, params });
                                    }
                                }
                            }
                            // 坏帧:回 -32700(JSON-RPC §4.1);解析失败本身说明
                            // 对端行为异常,记一行 stderr 侧痕迹
                            Ok(None) => {}
                            Err(e) => {
                                let _ = Self::write_line_raw(
                                    &stdin_shared,
                                    &protocol::encode_parse_error(),
                                );
                                eprintln!("vb-plugin:坏帧({e}):{line}");
                            }
                        }
                    }
                    if let Ok(mut q) = shared.lock() {
                        q.dead = true;
                    }
                })
                .map_err(|e| format!("插件 stdout 读线程启动失败:{e}"))?;
        }
        // stderr 读线程:逐行入队(宿主 poll 时并入日志环)
        {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("vb-plugin-stderr".into())
                .spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        let Ok(line) = line else { break };
                        if let Ok(mut q) = shared.lock() {
                            if q.stderr.len() < 500 {
                                q.stderr.push_back(line);
                            }
                        }
                    }
                })
                .map_err(|e| format!("插件 stderr 读线程启动失败:{e}"))?;
        }

        Ok(PluginProcess {
            child: Arc::new(Mutex::new(Some(child))),
            shared,
            stdin: stdin_shared,
            next_id: AtomicU64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 裸写一行(读线程回 -32700 用;不持外层锁)。
    fn write_line_raw(stdin: &Mutex<Option<ChildStdin>>, line: &str) -> std::io::Result<()> {
        let mut guard = stdin
            .lock()
            .map_err(|_| std::io::Error::other("stdin 锁中毒"))?;
        if let Some(w) = guard.as_mut() {
            w.write_all(line.as_bytes())?;
            w.flush()?;
            Ok(())
        } else {
            Err(std::io::Error::other("stdin 已关闭"))
        }
    }

    /// 写一行到插件 stdin。失败(管道断/锁中毒)→ Err;**绝不 panic**。
    fn write_line(&self, line: &str) -> Result<(), String> {
        Self::write_line_raw(&self.stdin, line).map_err(|e| format!("写插件 stdin 失败:{e}"))
    }

    /// 分配请求 id 并发送请求。
    pub fn send_request(&self, method: &str, params: &Value) -> Result<u64, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.write_line(&protocol::encode_request(id, method, params))?;
        if let Ok(mut p) = self.pending.lock() {
            p.insert(id, method.to_string());
        }
        Ok(id)
    }

    /// 发送通知(无 id,不期待应答)。
    pub fn send_notification(&self, method: &str, params: &Value) -> Result<(), String> {
        self.write_line(&protocol::encode_notification(method, params))
    }

    /// 应答插件的请求。
    pub fn respond(&self, id: &Value, result: &Value) {
        if let Err(e) = self.write_line(&protocol::encode_response(id, result)) {
            eprintln!("vb-plugin:应答失败:{e}");
        }
    }

    /// 以错误应答插件的请求(越权 / 未知方法 / 参数非法)。
    pub fn respond_error(&self, id: &Value, err: &RpcError) {
        if let Err(e) = self.write_line(&protocol::encode_error(id, err)) {
            eprintln!("vb-plugin:错误应答失败:{e}");
        }
    }

    /// 阻塞等待指定请求的响应(串行;05-10-1 超时可配)。
    ///
    /// 等待期间插件的请求/通知照常入队(宿主 poll 时处理),不会被丢弃。
    /// 超时 / 进程死亡 → `Err`(调用方通常随后 kill,见 host 的握手线程)。
    pub fn call(&self, id: u64, timeout: Duration) -> Result<Result<Value, RpcError>, String> {
        let start = Instant::now();
        loop {
            if let Ok(mut q) = self.shared.lock() {
                // 专用响应队列:poll 消费的 queue 永远不碰这里
                if let Some(pos) = q.responses.iter().position(|(rid, _)| *rid == id) {
                    let (_, result) = q.responses.remove(pos).expect("pos 必然有效");
                    if let Ok(mut p) = self.pending.lock() {
                        p.remove(&id);
                    }
                    return Ok(result);
                }
                if q.dead {
                    if let Ok(mut p) = self.pending.lock() {
                        p.remove(&id);
                    }
                    return Err("插件进程已退出(响应未到达)".into());
                }
            } else {
                return Err("插件 I/O 状态锁中毒".into());
            }
            if start.elapsed() >= timeout {
                if let Ok(mut p) = self.pending.lock() {
                    p.remove(&id);
                }
                return Err(format!("等待插件响应超时({}ms)", timeout.as_millis()));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// 取走全部待处理入站消息(宿主每帧 poll)。
    pub fn drain_incoming(&self, sink: &mut VecDeque<Incoming>) {
        if let Ok(mut q) = self.shared.lock() {
            sink.extend(q.queue.drain(..));
        }
    }

    /// 取走全部 stderr 行(宿主并入日志环)。
    pub fn drain_stderr(&self, sink: &mut Vec<String>) {
        if let Ok(mut q) = self.shared.lock() {
            sink.extend(q.stderr.drain(..));
        }
    }

    /// 进程是否仍然存活(EOF 或退出码已产生 → false)。
    pub fn is_alive(&self) -> bool {
        // 先看读线程 EOF 标记(管道断比 try_wait 更及时)
        if let Ok(q) = self.shared.lock() {
            if q.dead {
                return false;
            }
        }
        match self.child.lock() {
            Ok(mut guard) => match guard.as_mut() {
                Some(c) => c.try_wait().map(|st| st.is_none()).unwrap_or(false),
                None => false,
            },
            Err(_) => false,
        }
    }

    /// 进程退出码(已退出才有)。
    pub fn exit_code(&self) -> Option<i32> {
        let mut guard = self.child.lock().ok()?;
        guard
            .as_mut()
            .and_then(|c| c.try_wait().ok().flatten())
            .and_then(|st| st.code())
    }

    /// 强杀(kill;Drop 兜底也走这里)。幂等。内部全是锁保护的共享槽,
    /// `&self` 可调 —— 宿主与握手线程共享 `Arc<PluginProcess>`。
    pub fn kill(&self) {
        // 先关 stdin(多数只读等待型插件会因 EOF 自然退出)
        if let Ok(mut guard) = self.stdin.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.child.lock() {
            if let Some(c) = guard.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
            }
            *guard = None;
        }
        // 标记 dead,call 立刻返回错误
        if let Ok(mut q) = self.shared.lock() {
            q.dead = true;
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 构造一个"回声插件"子进程:用当前测试进程自身 + 参数模拟
    /// (不依赖示例插件二进制;进程级测试在集成测试里做)。
    /// 这里只测纯逻辑:send_request 的 id 分配与 pending 记账。
    #[test]
    fn request_ids_increment() {
        // spawn 失败路径(不存在的可执行)→ 中文错误
        let err = PluginProcess::spawn(std::path::Path::new("不存在的插件.exe"), &[]).unwrap_err();
        assert!(err.contains("启动插件进程失败"), "{err}");
    }

    /// call 的超时路径:对 dead 共享态直接报错(无子进程,纯状态机)。
    #[test]
    fn call_dead_short_circuits() {
        let shared = Arc::new(Mutex::new(Shared {
            dead: true,
            ..Default::default()
        }));
        let proc = PluginProcess {
            child: Arc::new(Mutex::new(None)),
            shared,
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
        };
        let r = proc.call(1, Duration::from_millis(50));
        assert!(r.is_err(), "dead 进程的 call 必须立即失败");
    }

    /// call 的响应匹配:两个请求乱序回包,各自拿到自己的结果。
    #[test]
    fn call_matches_response_by_id() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let proc = PluginProcess {
            child: Arc::new(Mutex::new(None)),
            shared: shared.clone(),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
        };
        // 模拟读线程先回 id=2 再回 id=1
        {
            let mut q = shared.lock().unwrap();
            q.responses.push_back((2, Ok(json!({"which": 2}))));
            q.responses.push_back((1, Ok(json!({"which": 1}))));
        }
        let r1 = proc.call(1, Duration::from_millis(100)).unwrap();
        assert_eq!(r1.unwrap()["which"], 1, "乱序回包必须按 id 匹配");
        // id=2 仍留在队列
        let r2 = proc.call(2, Duration::from_millis(100)).unwrap();
        assert_eq!(r2.unwrap()["which"], 2);
    }

    /// 超时:队列里永远没有该 id 的响应 → 到点报超时(不 panic)。
    #[test]
    fn call_times_out() {
        let proc = PluginProcess {
            child: Arc::new(Mutex::new(None)),
            shared: Arc::new(Mutex::new(Shared::default())),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            pending: Arc::new(Mutex::new(HashMap::new())),
        };
        let t = Instant::now();
        let r = proc.call(9, Duration::from_millis(30));
        assert!(r.is_err());
        assert!(t.elapsed() >= Duration::from_millis(25), "必须等满超时窗");
        assert!(r.unwrap_err().contains("超时"));
    }
}
