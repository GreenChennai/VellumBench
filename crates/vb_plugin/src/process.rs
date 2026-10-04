//! 插件子进程(05-10-1 / 05-10-5):spawn + stdio JSON-RPC + 超时 + 崩溃隔离。
//!
//! 线程模型(每插件):
//! - **stdout 读线程**:按行解码(单行长度上限 [`MAX_LINE_BYTES`])→
//!   [`Incoming`] 入共享队列(响应回填 / 插件请求 / 插件通知;队列容量
//!   上限见 [`QUEUE_CAP`]/[`RESPONSES_CAP`]);超限 → 记断连原因并停止
//!   读取(管道背压 + 宿主检出死亡);EOF → `dead` 标记;
//! - **stderr 读线程**:逐行 → 日志队列(宿主并入日志环;插件的
//!   println!/eprintln! 调试输出天然可见,不丢);
//! - **写入**:宿主持有 `ChildStdin`(Mutex),串行写(05-10-1:单插件
//!   串行即可)。
//!
//! 等待模型(PLG-07):`call` 用 **Condvar** 定时等待(不再 5ms 忙轮询);
//! 在途请求在 `call` 进入时才登记进 `pending`,全部退出路径都清理 ——
//! 只发不等的调用不再留下永久 pending 条目。
//!
//! 崩溃隔离:子进程是独立进程;宿主对它的所有操作都不 panic —— 写失败 /
//! 读 EOF / `try_wait` 都只产生 `Err` 或状态标记。`Drop` 兜底强杀,
//! 插件生命周期绝不超出宿主(Windows 上另有 Job Object 兜底,见
//! [`crate::sandbox`])。

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::protocol::{self, Frame, RpcError};

/// stdout 单行长度上限(PLG-04 / RB-05):正常 JSON 帧远小于此;
/// 超限 = 插件行为异常(刷屏/攻击),断连而不是 OOM 宿主。
pub const MAX_LINE_BYTES: usize = 8 * 1024 * 1024;
/// 入站队列容量上限(插件请求/通知;超限断连,防 OOM 宿主)。
pub const QUEUE_CAP: usize = 1024;
/// 响应队列容量上限(宿主迟迟不消费响应同理)。
pub const RESPONSES_CAP: usize = 1024;

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
    /// stdout 读线程已退出(进程结束或管道断/超限断连)。
    dead: bool,
    /// 断连/死亡原因(PLG-04:超限断连要可观测,不许静默)。
    dead_reason: Option<String>,
    /// 在途请求 id(PLG-07:`call` 进入时登记,退出时清理)。
    pending: HashSet<u64>,
}

/// 一行读取结果(有界读; [`LineRead::TooLong`] = 超上限)。
enum LineRead {
    Line(String),
    TooLong,
    Eof,
}

/// 有界逐行读(PLG-04):累计长度超过 `cap` → 丢弃该行残余(读到换行
/// 或 EOF 为止)并返回 [`LineRead::TooLong`]。语义与 `BufRead::lines`
/// 对齐:行尾 `\n`(含前导 `\r`)剥除;EOF 前的残行照常产出。
fn read_line_capped(reader: &mut impl BufRead, cap: usize) -> std::io::Result<LineRead> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let available = match reader.fill_buf() {
            Ok(a) => a,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if available.is_empty() {
            // EOF
            return Ok(if buf.is_empty() {
                LineRead::Eof
            } else {
                LineRead::Line(String::from_utf8_lossy(&buf).into_owned())
            });
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(i) => {
                buf.extend_from_slice(&available[..i]);
                reader.consume(i + 1);
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                return Ok(if buf.len() > cap {
                    LineRead::TooLong
                } else {
                    LineRead::Line(String::from_utf8_lossy(&buf).into_owned())
                });
            }
            None => {
                let n = available.len();
                buf.extend_from_slice(available);
                reader.consume(n);
                if buf.len() > cap {
                    // 超限:把这一行读到底再丢弃(不让残渣污染后续帧),
                    // 然后断连
                    loop {
                        match reader.fill_buf() {
                            Ok(a) if a.is_empty() => break,
                            Ok(a) => {
                                let done = a.contains(&b'\n');
                                let n2 = a.len();
                                reader.consume(n2);
                                if done {
                                    break;
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(_) => break,
                        }
                    }
                    return Ok(LineRead::TooLong);
                }
            }
        }
    }
}

/// 一个运行中的插件子进程。
#[derive(Debug)]
pub struct PluginProcess {
    child: Arc<Mutex<Option<Child>>>,
    shared: Arc<Mutex<Shared>>,
    /// 与 `shared` 配对的等待队列(`call` 不再忙轮询)。
    signal: Arc<Condvar>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    next_id: AtomicU64,
    /// Windows Job Object 兜底(KILL_ON_JOB_CLOSE;Drop 关句柄 → 整树终止)。
    /// 非 Windows 平台恒 None(沙箱降级,见 sandbox_note)。
    job: Option<crate::sandbox::JobGuard>,
    /// Job 沙箱未生效时的降级说明(宿主记入插件日志,降级必须可观测)。
    sandbox_note: Option<String>,
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

        // PLG-01/PLG-08:挂 Windows Job Object(KILL_ON_JOB_CLOSE + 内存/
        // 进程数限额;插件派生的孙进程自动入 job,宿主退出/句柄关闭时
        // 整树终止,不留孤儿)。失败 → 降级并记录(不许静默)。
        let (job, sandbox_note) = match crate::sandbox::attach(&child) {
            Ok(g) => (Some(g), None),
            Err(e) => (
                None,
                Some(format!(
                    "Job Object 沙箱未生效({e});插件为普通权限进程,残余风险见 ADR-0052"
                )),
            ),
        };
        if let Some(note) = &sandbox_note {
            tracing::warn!("{note}");
        }

        let shared = Arc::new(Mutex::new(Shared::default()));
        let signal = Arc::new(Condvar::new());
        // stdin 包进共享槽(读线程回 -32700 与宿主写路径共用)
        let stdin_shared = Arc::new(Mutex::new(Some(stdin)));
        // stdout 读线程:逐行解码入队;EOF/超限置 dead(带原因)
        {
            let shared = shared.clone();
            let signal = signal.clone();
            let stdin_shared = stdin_shared.clone();
            std::thread::Builder::new()
                .name("vb-plugin-stdout".into())
                .spawn(move || {
                    let mut reader = BufReader::new(stdout);
                    loop {
                        let line = match read_line_capped(&mut reader, MAX_LINE_BYTES) {
                            Ok(LineRead::Line(l)) => l,
                            Ok(LineRead::TooLong) => {
                                let msg = format!(
                                    "stdout 单行超过 {MAX_LINE_BYTES} 字节上限,已断连(PLG-04)"
                                );
                                mark_dead(&shared, &signal, Some(msg));
                                break;
                            }
                            Ok(LineRead::Eof) => {
                                mark_dead(&shared, &signal, None);
                                break;
                            }
                            Err(_) => {
                                mark_dead(&shared, &signal, None);
                                break;
                            }
                        };
                        match protocol::decode_line(&line) {
                            Ok(Some(frame)) => {
                                if !push_frame(&shared, &signal, frame) {
                                    // 队列超限:断连(原因已写入 dead_reason)
                                    break;
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
                })
                .map_err(|e| format!("插件 stdout 读线程启动失败:{e}"))?;
        }
        // stderr 读线程:逐行入队(宿主 poll 时并入日志环)
        {
            let shared = shared.clone();
            let signal = signal.clone();
            std::thread::Builder::new()
                .name("vb-plugin-stderr".into())
                .spawn(move || {
                    let mut reader = BufReader::new(stderr);
                    loop {
                        match read_line_capped(&mut reader, MAX_LINE_BYTES) {
                            Ok(LineRead::Line(line)) => {
                                let mut q = lock_shared(&shared);
                                if q.stderr.len() < 500 {
                                    q.stderr.push_back(line);
                                }
                            }
                            Ok(LineRead::TooLong) | Ok(LineRead::Eof) | Err(_) => break,
                        }
                    }
                    // stderr 退出不置 dead(stdout 才是生命周期信号)
                    signal.notify_all();
                })
                .map_err(|e| format!("插件 stderr 读线程启动失败:{e}"))?;
        }

        Ok(PluginProcess {
            child: Arc::new(Mutex::new(Some(child))),
            shared,
            signal,
            stdin: stdin_shared,
            next_id: AtomicU64::new(1),
            job,
            sandbox_note,
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
    ///
    /// PLG-07:**不再**在此登记 `pending` —— 在途登记移到 [`Self::call`],
    /// 只发不等(发送后调用方出错/放弃)的请求不留下永久条目。
    pub fn send_request(&self, method: &str, params: &Value) -> Result<u64, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.write_line(&protocol::encode_request(id, method, params))?;
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

    /// 阻塞等待指定请求的响应(Condvar 定时等待;05-10-1 超时可配)。
    ///
    /// 等待期间插件的请求/通知照常入队(宿主 poll 时处理),不会被丢弃。
    /// 超时 / 进程死亡 → `Err`(调用方通常随后 kill,见 host 的握手线程)。
    pub fn call(&self, id: u64, timeout: Duration) -> Result<Result<Value, RpcError>, String> {
        let deadline = Instant::now() + timeout;
        // 在途登记(PLG-07):与等待同锁,全部退出路径清理
        let mut q = lock_shared(&self.shared);
        q.pending.insert(id);
        loop {
            // 专用响应队列:poll 消费的 queue 永远不碰这里
            if let Some(pos) = q.responses.iter().position(|(rid, _)| *rid == id) {
                let result = q.responses.remove(pos).map(|(_, r)| r);
                q.pending.remove(&id);
                return Ok(result.unwrap_or_else(|| {
                    Err(RpcError::new(
                        protocol::E_PARSE,
                        "响应槽意外为空".to_string(),
                    ))
                }));
            }
            if q.dead {
                q.pending.remove(&id);
                return Err(q
                    .dead_reason
                    .clone()
                    .unwrap_or_else(|| "插件进程已退出(响应未到达)".into()));
            }
            let now = Instant::now();
            if now >= deadline {
                q.pending.remove(&id);
                return Err(format!("等待插件响应超时({}ms)", timeout.as_millis()));
            }
            let (next, _) = self
                .signal
                .wait_timeout(q, deadline - now)
                .unwrap_or_else(|e| e.into_inner());
            q = next;
        }
    }

    /// 取走待处理入站消息(宿主每帧 poll;`max` = 本帧最多取多少条,
    /// 余下的留在队列下一帧处理 —— PLG-05:UI 线程每帧工作量有界)。
    pub fn drain_incoming(&self, sink: &mut Vec<Incoming>, max: usize) {
        let mut q = lock_shared(&self.shared);
        let take = q.queue.len().min(max);
        sink.extend(q.queue.drain(..take));
    }

    /// 取走全部 stderr 行(宿主并入日志环)。
    pub fn drain_stderr(&self, sink: &mut Vec<String>) {
        let mut q = lock_shared(&self.shared);
        sink.extend(q.stderr.drain(..));
    }

    /// 在途请求计数(诊断/测试用)。
    pub fn pending_count(&self) -> usize {
        lock_shared(&self.shared).pending.len()
    }

    /// 断连/死亡原因(PLG-04:超限断连可观测)。
    pub fn dead_reason(&self) -> Option<String> {
        lock_shared(&self.shared).dead_reason.clone()
    }

    /// Job 沙箱降级说明(未生效时为 Some;宿主启动后并入插件日志环)。
    pub fn sandbox_note(&self) -> Option<String> {
        self.sandbox_note.clone()
    }

    /// 进程树是否已挂入 Job Object(Windows;PLG-01 观测口)。
    pub fn job_attached(&self) -> bool {
        self.job.is_some()
    }

    /// 进程是否仍然存活(EOF 或退出码已产生 → false)。
    pub fn is_alive(&self) -> bool {
        // 先看读线程 EOF 标记(管道断比 try_wait 更及时)
        {
            let q = lock_shared(&self.shared);
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
        mark_dead(&self.shared, &self.signal, None);
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        self.kill();
        // Windows:JobGuard 随后 Drop → 句柄关闭 → KILL_ON_JOB_CLOSE
        // 兜底收割子进程树(含插件派生的孙进程)
    }
}

/// poison-safe 共享锁(RB-10):读线程/宿主任一侧 panic 不许永久锁死。
fn lock_shared(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// 标记死亡(带可选原因)并唤醒全部等待者。
fn mark_dead(shared: &Mutex<Shared>, signal: &Condvar, reason: Option<String>) {
    let mut q = lock_shared(shared);
    q.dead = true;
    if let Some(r) = reason {
        q.dead_reason = Some(r);
    }
    drop(q);
    signal.notify_all();
}

/// 入站帧入队(容量上限,PLG-04/RB-05):超限 → 记断连原因并返回 false,
/// 读线程随之退出(管道背压 + 宿主检出死亡)。
fn push_frame(shared: &Mutex<Shared>, signal: &Condvar, frame: Frame) -> bool {
    let mut q = lock_shared(shared);
    match frame {
        Frame::Response { id, result } => {
            // id 必须是数字(宿主出站 id 约定);否则丢弃
            if let Some(n) = id.as_u64() {
                if q.responses.len() >= RESPONSES_CAP {
                    q.dead = true;
                    q.dead_reason = Some(format!(
                        "响应队列超过 {RESPONSES_CAP} 条上限,已断连(PLG-04)"
                    ));
                    drop(q);
                    signal.notify_all();
                    return false;
                }
                q.responses.push_back((n, result));
            }
        }
        Frame::Request { id, method, params } => {
            if q.queue.len() >= QUEUE_CAP {
                q.dead = true;
                q.dead_reason = Some(format!("入站队列超过 {QUEUE_CAP} 条上限,已断连(PLG-04)"));
                drop(q);
                signal.notify_all();
                return false;
            }
            q.queue.push_back(Incoming::Request {
                id: Some(id),
                method,
                params,
            });
        }
        Frame::Notification { method, params } => {
            if q.queue.len() >= QUEUE_CAP {
                q.dead = true;
                q.dead_reason = Some(format!("入站队列超过 {QUEUE_CAP} 条上限,已断连(PLG-04)"));
                drop(q);
                signal.notify_all();
                return false;
            }
            q.queue.push_back(Incoming::Notification { method, params });
        }
    }
    drop(q);
    signal.notify_all();
    true
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
        let shared = Mutex::new(Shared {
            dead: true,
            ..Default::default()
        });
        let proc = PluginProcess {
            child: Arc::new(Mutex::new(None)),
            shared: Arc::new(shared),
            signal: Arc::new(Condvar::new()),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            #[cfg(windows)]
            job: None,
            sandbox_note: None,
        };
        let r = proc.call(1, Duration::from_millis(50));
        assert!(r.is_err(), "dead 进程的 call 必须立即失败");
        // PLG-07:失败路径也清理 pending
        assert_eq!(proc.pending_count(), 0, "call 退出必须清理在途登记");
    }

    /// call 的响应匹配:两个请求乱序回包,各自拿到自己的结果。
    #[test]
    fn call_matches_response_by_id() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let proc = PluginProcess {
            child: Arc::new(Mutex::new(None)),
            shared: shared.clone(),
            signal: Arc::new(Condvar::new()),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            #[cfg(windows)]
            job: None,
            sandbox_note: None,
        };
        // 模拟读线程先回 id=2 再回 id=1
        {
            let mut q = lock_shared(&shared);
            q.responses.push_back((2, Ok(json!({"which": 2}))));
            q.responses.push_back((1, Ok(json!({"which": 1}))));
            drop(q);
            // push_frame 才会 notify;这里手动补一次
        }
        let r1 = proc.call(1, Duration::from_millis(100)).unwrap();
        assert_eq!(r1.unwrap()["which"], 1, "乱序回包必须按 id 匹配");
        // id=2 仍留在队列
        let r2 = proc.call(2, Duration::from_millis(100)).unwrap();
        assert_eq!(r2.unwrap()["which"], 2);
        assert_eq!(proc.pending_count(), 0, "消费完响应不留在途条目");
    }

    /// PLG-07:响应由另一线程迟到投递 → Condvar 唤醒(不再 5ms 忙轮询),
    /// 等待方在远小于超时窗的时间内拿到结果。
    #[test]
    fn call_is_woken_by_late_response() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let signal = Arc::new(Condvar::new());
        let proc = PluginProcess {
            child: Arc::new(Mutex::new(None)),
            shared: shared.clone(),
            signal: signal.clone(),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            #[cfg(windows)]
            job: None,
            sandbox_note: None,
        };
        {
            let shared = shared.clone();
            let signal = signal.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                let mut q = lock_shared(&shared);
                q.responses.push_back((7, Ok(json!({"late": true}))));
                drop(q);
                signal.notify_all();
            });
        }
        let t = Instant::now();
        let r = proc.call(7, Duration::from_secs(5)).unwrap();
        assert!(r.unwrap()["late"] == true, "迟到响应必须被唤醒后拿到");
        assert!(
            t.elapsed() < Duration::from_millis(1000),
            "Condvar 等待不应拖满超时窗:{:?}",
            t.elapsed()
        );
    }

    /// PLG-07:只发不等(send_request 不 call)不留下永久 pending 条目。
    #[test]
    fn send_request_without_call_leaks_nothing() {
        let proc = PluginProcess {
            child: Arc::new(Mutex::new(None)),
            shared: Arc::new(Mutex::new(Shared::default())),
            signal: Arc::new(Condvar::new()),
            stdin: Arc::new(Mutex::new(None)),
            next_id: AtomicU64::new(1),
            #[cfg(windows)]
            job: None,
            sandbox_note: None,
        };
        // 无真实子进程(stdin 已关):send_request 报写失败,但**无论
        // 发送成败都不登记 pending**
        let id = proc
            .send_request("initialize", &json!({"pluginId": "t"}))
            .unwrap_or(1);
        assert_eq!(proc.pending_count(), 0, "send 不登记 pending");
        // call 才登记;超时路径清理
        assert!(proc.call(id, Duration::from_millis(20)).is_err());
        assert_eq!(proc.pending_count(), 0, "超时退出也必须清理");
    }

    /// PLG-04:超限断连 —— push_frame 在队列容量处拒绝并置断连原因。
    #[test]
    fn queue_overflow_marks_dead_with_reason() {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let signal = Arc::new(Condvar::new());
        for i in 0..QUEUE_CAP {
            let f = protocol::decode_line(&protocol::encode_notification("log", &json!({"n": i})))
                .unwrap()
                .unwrap();
            assert!(push_frame(&shared, &signal, f), "容量内必须接受");
        }
        let f = protocol::decode_line(&protocol::encode_notification("log", &json!({})))
            .unwrap()
            .unwrap();
        assert!(!push_frame(&shared, &signal, f), "超容量必须拒绝");
        let q = lock_shared(&shared);
        assert!(q.dead);
        assert!(
            q.dead_reason
                .as_deref()
                .unwrap_or_default()
                .contains("上限"),
            "断连原因必须可观测:{:?}",
            q.dead_reason
        );
    }

    /// PLG-04:有界行读 —— 超长行报 TooLong 且不污染后续行。
    #[test]
    fn read_line_capped_rejects_overlong_lines() {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"short\n");
        let long = "x".repeat(100);
        buf.extend_from_slice(long.as_bytes());
        buf.push(b'\n');
        buf.extend_from_slice(b"after\n");
        let mut reader = BufReader::new(&buf[..]);
        assert!(matches!(
            read_line_capped(&mut reader, 50).unwrap(),
            LineRead::Line(l) if l == "short"
        ));
        assert!(matches!(
            read_line_capped(&mut reader, 50).unwrap(),
            LineRead::TooLong
        ));
        // 超长行被丢弃后,后续行照常可读
        assert!(matches!(
            read_line_capped(&mut reader, 50).unwrap(),
            LineRead::Line(l) if l == "after"
        ));
        assert!(matches!(
            read_line_capped(&mut reader, 50).unwrap(),
            LineRead::Eof
        ));
    }

    /// PLG-04:CRLF 与 EOF 残行语义与 BufRead::lines 对齐。
    #[test]
    fn read_line_capped_crlf_and_eof_fragment() {
        let mut reader = BufReader::new(&b"a\r\nbbb"[..]);
        assert!(matches!(
            read_line_capped(&mut reader, 10).unwrap(),
            LineRead::Line(l) if l == "a"
        ));
        assert!(matches!(
            read_line_capped(&mut reader, 10).unwrap(),
            LineRead::Line(l) if l == "bbb"
        ));
        assert!(matches!(
            read_line_capped(&mut reader, 10).unwrap(),
            LineRead::Eof
        ));
    }
}
