//! 示例插件「统计元素」(09-J 05-10-6):仓库自带的最小插件模板。
//!
//! 形态:独立子进程,stdio 上的换行分隔 JSON-RPC 2.0(与宿主
//! `vb_plugin::protocol` 同一套编解码)。Manifest 在
//! `plugins/example-stats/plugin.json`,入口用 `bin:example-stats-plugin`
//! (宿主可执行同目录解析;cargo 构建后两者同在 `target/debug/`)。
//!
//! 功能:收到 `event/started` / 面板「刷新」按钮后,向宿主请求文档
//! **只读投影**(`doc/projection`),把 节点/文本/图片/编组 计数以
//! 受控 UI(`panel/setUI`)显示在「插件」次级坞的面板里;面板按钮
//! 「全选对象」演示**白名单内**的宿主命令调用(`runCommand`)。
//!
//! 线程模型(与宿主对偶,缺一即死锁:**不能在"读 stdin"的循环体里
//! 阻塞等响应** —— 否则响应永远轮不到被读):
//! - **读线程**:stdin 逐行解码 → 响应进 `pending`、请求/通知进 `events`,
//!   EOF 置 `dead`;
//! - **主线程**:事件循环,处理宿主请求/通知;发出自己的请求后轮询
//!   `pending` 等响应(单请求串行)。
//!
//! 测试夹具模式(只服务 05-10-7 门禁与冒烟,正常使用不涉及):
//! - `--crash` 握手后立即 `exit(3)`(崩溃隔离门禁);
//! - `--no-handshake` 收到 initialize 不回包(超时门禁);
//! - `--probe <command>` 握手后调用一次 runCommand(越权门禁;结果打
//!   stderr 后退出);
//! - `--hang` 握手后挂起(停止/杀进程路径)。

use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use vb_plugin::protocol::{
    self, decode_line, encode_error, encode_notification, encode_parse_error, encode_request,
    encode_response, Frame, RpcError, E_METHOD_NOT_FOUND,
};
use vb_plugin::PROTOCOL_VERSION;

/// 插件自己的请求 id 计数器起点(与宿主请求 id 空间隔离,便于人读)。
const SELF_ID_BASE: u64 = 1000;
/// 单次请求的等待上限(宿主 poll 是逐帧的,给足余量)。
const CALL_TIMEOUT: Duration = Duration::from_secs(8);

/// 宿主发来的事件(请求 / 通知;响应走 `pending`)。
enum Ev {
    Req {
        id: Value,
        method: String,
        params: Value,
    },
    Notif {
        method: String,
        params: Value,
    },
}

/// 共享连接状态(读线程与主线程共享)。
#[derive(Default)]
struct Conn {
    next_id: u64,
    /// 在途请求的响应槽(id → 宿主应答)。
    pending: Vec<(u64, Result<Value, RpcError>)>,
    /// 待主线程处理的宿主请求 / 通知。
    events: VecDeque<Ev>,
    /// stdin 已关闭(读线程退出)。
    dead: bool,
}

impl Conn {
    fn push_event(&mut self, ev: Ev) {
        self.events.push_back(ev);
    }

    /// 收一个响应槽(按 id 匹配)。
    fn take(&mut self, id: u64) -> Option<Result<Value, RpcError>> {
        let pos = self.pending.iter().position(|(i, _)| *i == id)?;
        Some(self.pending.remove(pos).1)
    }
}

/// 夹具模式配置(--crash / --no-handshake / --probe <cmd> / --hang)。
struct Fixture {
    mode: String,
    probe_cmd: Option<String>,
}

impl Fixture {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let mode = args.first().cloned().unwrap_or_default();
        let probe_cmd = if mode == "--probe" {
            Some(args.get(1).cloned().unwrap_or_default())
        } else {
            None
        };
        Fixture { mode, probe_cmd }
    }
}

/// 主线程持有的会话件:stdin 连接 + 最近一次投影缓存。
struct Session {
    conn: Arc<Mutex<Conn>>,
    last_projection: Mutex<Option<Value>>,
}

fn main() {
    let fixture = Fixture::from_args();
    let Fixture { mode, probe_cmd } = &fixture;

    let session = Session {
        conn: Arc::new(Mutex::new(Conn {
            next_id: SELF_ID_BASE,
            ..Conn::default()
        })),
        last_projection: Mutex::new(None),
    };
    let conn = session.conn.clone();
    let out = std::io::stdout();

    // ── 读线程:stdin → 帧 → conn ──
    {
        let conn = conn.clone();
        std::thread::Builder::new()
            .name("vb-plugin-stdin".into())
            .spawn(move || {
                let reader = std::io::BufReader::new(std::io::stdin());
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    match decode_line(&line) {
                        Ok(Some(Frame::Response { id, result })) => {
                            if let Some(n) = id.as_u64() {
                                if let Ok(mut c) = conn.lock() {
                                    c.pending.push((n, result));
                                }
                            }
                        }
                        Ok(Some(Frame::Request { id, method, params })) => {
                            if let Ok(mut c) = conn.lock() {
                                c.push_event(Ev::Req { id, method, params });
                            }
                        }
                        Ok(Some(Frame::Notification { method, params })) => {
                            if let Ok(mut c) = conn.lock() {
                                c.push_event(Ev::Notif { method, params });
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            // 坏帧:回 -32700(JSON-RPC §4.1)
                            eprintln!("vb-example-stats:坏帧({e})");
                            write_out(&std::io::stdout(), &encode_parse_error());
                        }
                    }
                }
                if let Ok(mut c) = conn.lock() {
                    c.dead = true;
                }
            })
            .expect("读线程必须能启动");
    }

    // ── 主线程事件循环 ──
    loop {
        // 取一批事件
        let evs: Vec<Ev> = match conn.lock() {
            Ok(mut c) => {
                let n = c.events.len();
                c.events.drain(..n).collect()
            }
            Err(_) => break,
        };
        if evs.is_empty() {
            // 无事件:stdin 关了就退(宿主 stop 也会先杀我们)
            if let Ok(c) = conn.lock() {
                if c.dead {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        for ev in evs {
            match ev {
                Ev::Req { id, method, params } => {
                    handle_request(&out, &session, mode, probe_cmd, id, method, &params);
                }
                Ev::Notif { method, params } => {
                    handle_notification(&out, &session, &method, &params)
                }
            }
        }
    }
    // 收到过 shutdown 时 exit(0);EOF 正常退出码 0
    std::process::exit(0);
}

/// 处理宿主请求(initialize / shutdown / 其他 → -32601)。
fn handle_request(
    out: &std::io::Stdout,
    session: &Session,
    mode: &str,
    probe_cmd: &Option<String>,
    id: Value,
    method: String,
    params: &Value,
) {
    let conn = &session.conn;
    let last_projection = &session.last_projection;
    if method == protocol::M_INITIALIZE {
        // --no-handshake 夹具:保持沉默,逼宿主走超时路径
        if mode == "--no-handshake" {
            eprintln!("vb-example-stats:--no-handshake 夹具,拒绝握手,挂起");
            hang();
        }
        let result = json!({
            "protocolVersion": PROTOCOL_VERSION,
            "name": "统计元素(示例)",
            "version": env!("CARGO_PKG_VERSION"),
        });
        write_out(out, &encode_response(&id, &result));
        eprintln!("vb-example-stats:握手完成");
        // ── 测试夹具分支(握手后生效)──
        match mode {
            "--crash" => {
                // 稍候再自杀:给宿主握手线程留出看到 Running 的窗口,
                // 门禁 ③ 才能覆盖"Running → 崩溃检出"的完整路径
                std::thread::sleep(Duration::from_millis(300));
                eprintln!("vb-example-stats:--crash 夹具,自杀退出(退出码 3)");
                std::process::exit(3);
            }
            "--hang" => {
                eprintln!("vb-example-stats:--hang 夹具,挂起等待宿主处理");
                hang();
            }
            _ => {}
        }
        // --probe:握手后调一次 runCommand,结果打 stderr 后退出
        if let Some(cmd) = probe_cmd {
            let r = call_host(
                out,
                conn,
                protocol::M_RUN_COMMAND,
                &json!({ "command": cmd }),
            );
            match r {
                Ok(v) => eprintln!("PROBE_RESULT:ok:{v}"),
                Err(e) => eprintln!("PROBE_RESULT:err:{}:{}", e.code, e.message),
            }
            std::process::exit(0);
        }
        return;
    }
    if method == protocol::M_SHUTDOWN {
        write_out(out, &encode_response(&id, &json!({ "ok": true })));
        eprintln!("vb-example-stats:收到 shutdown,退出");
        std::process::exit(0);
    }
    let _ = (params, last_projection);
    write_out(
        out,
        &encode_error(
            &id,
            &RpcError::new(E_METHOD_NOT_FOUND, format!("未知方法:{method}")),
        ),
    );
}

/// 处理宿主通知(event/started / event/button / event/input)。
fn handle_notification(out: &std::io::Stdout, session: &Session, method: &str, params: &Value) {
    let conn = &session.conn;
    let last_projection = &session.last_projection;
    match method {
        protocol::M_EVENT_STARTED => {
            // 启动即拉一次统计并渲染面板(05-10-6:run → 面板可见)
            refresh(out, conn, last_projection);
        }
        protocol::M_EVENT_BUTTON => {
            let panel = params.get("panel").and_then(|v| v.as_str()).unwrap_or("");
            let action = params.get("action").and_then(|v| v.as_str()).unwrap_or("");
            eprintln!("vb-example-stats:面板 {panel} 按钮 {action}");
            if action == "select_all" {
                // 白名单内宿主命令(edit.select_all):演示授权调用路径
                match call_host(
                    out,
                    conn,
                    protocol::M_RUN_COMMAND,
                    &json!({"command": "edit.select_all"}),
                ) {
                    Ok(_) => plugin_log(out, "info", "已调用宿主命令 edit.select_all(白名单内)"),
                    Err(e) => plugin_log(
                        out,
                        "error",
                        &format!("edit.select_all 失败({}):{}", e.code, e.message),
                    ),
                }
            }
            refresh(out, conn, last_projection);
        }
        protocol::M_EVENT_INPUT => {
            let v = params.get("value").and_then(|v| v.as_str()).unwrap_or("");
            eprintln!("vb-example-stats:输入提交:{v}(示例插件不使用)");
        }
        other => eprintln!("vb-example-stats:未知通知 {other}(忽略)"),
    }
}

/// 写一行并冲刷 stdout(所有出站帧的统一出口)。
fn write_out(out: &std::io::Stdout, line: &str) {
    let mut o = out.lock();
    let _ = o.write_all(line.as_bytes());
    let _ = o.flush();
}

/// 挂起直到被杀(测试夹具)。
fn hang() -> ! {
    loop {
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// 向宿主发一个请求并轮询等响应;返回宿主应答(成功/错误)。
fn call_host(
    out: &std::io::Stdout,
    conn: &Arc<Mutex<Conn>>,
    method: &str,
    params: &Value,
) -> Result<Value, RpcError> {
    let (req_id, line) = {
        let mut c = conn.lock().unwrap();
        let rid = c.next_id;
        c.next_id += 1;
        (rid, encode_request(rid, method, params))
    };
    write_out(out, &line);
    let deadline = Instant::now() + CALL_TIMEOUT;
    loop {
        if let Ok(mut c) = conn.lock() {
            if let Some(r) = c.take(req_id) {
                match &r {
                    Ok(_) => eprintln!("vb-example-stats:{method} 成功"),
                    Err(e) => eprintln!(
                        "vb-example-stats:{method} 被拒(code {}):{}",
                        e.code, e.message
                    ),
                }
                return r;
            }
        }
        if Instant::now() >= deadline {
            eprintln!("vb-example-stats:{method} 等待宿主响应超时");
            return Err(RpcError::new(-32009, "等待宿主响应超时"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 拉取文档只读投影 → panel/setUI 渲染统计面板(05-10-6 主功能)。
fn refresh(out: &std::io::Stdout, conn: &Arc<Mutex<Conn>>, cache: &Mutex<Option<Value>>) {
    let projection = match call_host(out, conn, protocol::M_DOC_PROJECTION, &json!({})) {
        Ok(v) => v,
        Err(e) => {
            plugin_log(
                out,
                "error",
                &format!("拉取文档投影失败({}):{}", e.code, e.message),
            );
            return;
        }
    };
    // 从投影取计数(宿主已预聚合,见 vb_plugin::projection)
    let c = projection.get("counts").cloned().unwrap_or(json!({}));
    let rev = projection.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
    let get = |k: &str| -> u64 { c.get(k).and_then(|v| v.as_u64()).unwrap_or(0) };
    let ab = projection
        .get("artboardCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    // 受控 UI 描述(有限元件:text / metric / button / input,禁任意代码)
    let widgets = json!([
        {"kind": "text", "text": "示例插件:统计当前文档的元素构成(只读,不修改文档)。"},
        {"kind": "metric", "label": "文档 rev", "value": rev.to_string()},
        {"kind": "metric", "label": "画板数", "value": ab.to_string()},
        {"kind": "metric", "label": "节点总数", "value": get("nodes").to_string()},
        {"kind": "metric", "label": "文本数", "value": get("text").to_string()},
        {"kind": "metric", "label": "图片数", "value": get("image").to_string()},
        {"kind": "metric", "label": "编组数", "value": get("group").to_string()},
        {"kind": "metric", "label": "矢量数", "value": get("vector").to_string()},
        {"kind": "button", "action": "refresh", "label": "刷新统计"},
        {"kind": "button", "action": "select_all", "label": "全选对象(白名单内命令)"},
        {"kind": "input", "id": "note", "placeholder": "示例输入(回车发 event/input)"},
    ]);
    write_out(
        out,
        &encode_notification(
            protocol::M_PANEL_SET_UI,
            &json!({"panel": "stats", "widgets": widgets}),
        ),
    );
    *cache.lock().unwrap() = Some(projection);
}

/// 写一条插件日志通知(进宿主日志环,面板可看)。
fn plugin_log(out: &std::io::Stdout, level: &str, msg: &str) {
    let line = encode_notification(protocol::M_LOG, &json!({"level": level, "message": msg}));
    write_out(out, &line);
}
