//! `vellum-mcp` — MCP Server(stdio 传输,设计文档 08 篇 §四)。
//!
//! 协议:MCP 规范 = stdio 上的换行分隔 JSON-RPC 2.0。
//! 工具与 CLI/菜单同一条命令路径(vb_doc::commands),天然可撤销、一致。
//!
//! 启动:`vellum-mcp [--doc <项目目录>]`(亦可在会话中调 vellum_open_document)。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

use serde_json::{json, Value};
use tracing::info_span;

use vb_doc::export::render_project;
use vb_doc::import::import_project;
use vb_doc::model::Document;
use vb_doc::undo::UndoStack;

struct Session {
    doc: Document,
    undo: UndoStack,
    dir: PathBuf,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// poison-safe 会话锁(RB-10 / AGT-01):工具闭包 panic 会毒化 Mutex,
/// `into_inner` 恢复守权 —— 一次工具 bug 不许永久废掉整个会话。
fn session_lock() -> MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// 工具 panic 后的会话重建(AGT-01):从磁盘重新导入当前项目目录;
/// 导入失败 → 清空会话(后续调用得到结构化「未打开文档」)。
/// 半程 panic 可能在内存文档里留下不一致状态,磁盘副本是唯一可信快照。
fn rebuild_session_after_panic() {
    let mut guard = session_lock();
    let Some(s) = guard.as_ref() else {
        return;
    };
    let dir = s.dir.clone();
    match import_project(&dir) {
        Ok(r) => {
            let had_rev = r.doc.rev;
            *guard = Some(Session {
                doc: r.doc,
                undo: UndoStack::new(),
                dir: r.project_dir,
            });
            eprintln!("vb-mcp:工具 panic 后会话已从磁盘重建(dir={dir:?},rev={had_rev})");
        }
        Err(e) => {
            *guard = None;
            eprintln!("vb-mcp:工具 panic 后会话重建失败({e}),已清空会话");
        }
    }
}

fn with_session(f: impl FnOnce(&mut Session) -> Result<Value, String>) -> Result<Value, String> {
    let mut guard = session_lock();
    let s = guard
        .as_mut()
        .ok_or("未打开文档:先调用 vellum_open_document")?;
    f(s)
}

fn outline_json(doc: &Document, depth: usize) -> Value {
    fn node_json(doc: &Document, id: vb_doc::model::NodeId, depth: usize) -> Value {
        // 容忍悬挂 id(防御:命令层已同步画板注册表,此处兜底不 panic——
        // MCP 进程无 catch_unwind,unwrap = 整个 server 崩溃)
        let Some(n) = doc.nodes.get(id) else {
            return Value::Null;
        };
        let children: Vec<Value> = if depth > 1 {
            n.children
                .iter()
                .map(|&c| node_json(doc, c, depth - 1))
                .collect()
        } else {
            vec![]
        };
        json!({
            "sid": n.sid.as_str(), "name": n.name, "tag": n.tag,
            "kind": n.kind.kind_name(),
            "box": {"x": n.geom.x, "y": n.geom.y, "w": n.geom.w, "h": n.geom.h},
            "children": children,
        })
    }
    json!({
        "rev": doc.rev,
        "artboards": doc.artboards.iter().map(|&a| node_json(doc, a, depth)).collect::<Vec<_>>(),
    })
}

fn tool_open(args: &Value) -> Result<Value, String> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("缺少 path")?;
    let r = import_project(Path::new(path)).map_err(|e| format!("导入失败:{e}"))?;
    let n = r.doc.artboards.len();
    let title = r.doc.meta.title.clone();
    let rev = r.doc.rev;
    *session_lock() = Some(Session {
        doc: r.doc,
        undo: UndoStack::new(),
        dir: r.project_dir,
    });
    Ok(json!({"ok": true, "title": title, "artboards": n, "rev": rev}))
}

fn need_artboard(doc: &Document, key: Option<&str>) -> Result<vb_doc::model::NodeId, String> {
    match key {
        Some(k) => {
            if let Some(id) = doc.find_by_sid(k) {
                return Ok(id);
            }
            doc.artboards
                .iter()
                .copied()
                .find(|&a| {
                    doc.nodes
                        .get(a)
                        .map(|n| {
                            n.name.eq_ignore_ascii_case(k)
                                || n.classes.iter().any(|c| c.eq_ignore_ascii_case(k))
                        })
                        .unwrap_or(false)
                })
                .ok_or_else(|| format!("画板 {k} 不存在"))
        }
        None => doc
            .artboards
            .first()
            .copied()
            .ok_or_else(|| "无画板".into()),
    }
}

fn tool_outline(args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(4) as usize;
        Ok(outline_json(&s.doc, depth.max(1)))
    })
}

fn tool_find(args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        let name = args.get("name").and_then(|v| v.as_str());
        let text = args.get("text").and_then(|v| v.as_str());
        let tag = args.get("tag").and_then(|v| v.as_str());
        let mut ids = Vec::new();
        for &ab in &s.doc.artboards {
            s.doc.subtree(ab, &mut ids);
        }
        let hits: Vec<Value> = ids
            .iter()
            .filter_map(|&id| s.doc.nodes.get(id))
            .filter(|n| {
                name.map(|q| n.name.contains(q)).unwrap_or(true)
                    && text
                        .map(|q| n.text().map(|t| t.contains(q)).unwrap_or(false))
                        .unwrap_or(true)
                    && tag.map(|q| n.tag == q).unwrap_or(true)
            })
            .map(|n| json!({"sid": n.sid.as_str(), "name": n.name, "kind": n.kind.kind_name()}))
            .collect();
        Ok(json!({"count": hits.len(), "hits": hits}))
    })
}

fn tool_get_element(args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        let id = args.get("id").and_then(|v| v.as_str()).ok_or("缺少 id")?;
        let nid = s
            .doc
            .find_by_sid(id)
            .ok_or_else(|| format!("sid {id} 不存在"))?;
        let n = s
            .doc
            .nodes
            .get(nid)
            .ok_or_else(|| format!("sid {id} 的内部节点缺失(悬挂 id)"))?;
        Ok(json!({
            "sid": n.sid.as_str(), "name": n.name, "tag": n.tag,
            "kind": n.kind.kind_name(),
            "box": {"x": n.geom.x, "y": n.geom.y, "w": n.geom.w, "h": n.geom.h},
            "style": n.style.iter().map(|d| (d.prop.clone(), Value::String(d.value.clone())))
                .collect::<serde_json::Map<_, _>>(),
            "attrs": n.attrs,
            "text": n.text(),
            "classes": n.classes,
        }))
    })
}

fn tool_apply_patch(args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        let req: vb_agent::PatchRequest =
            serde_json::from_value(args.clone()).map_err(|e| format!("patch 解析失败:{e}"))?;
        let outcome =
            vb_agent::apply_patch(&mut s.doc, &mut s.undo, &req).map_err(|e| e.to_string())?;
        Ok(serde_json::to_value(outcome).unwrap_or(Value::Null))
    })
}

/// 工具产物落盘:先建父目录(客户端给的 out 常带尚未创建的多级目录,
/// 如 D:\Temp\pc-test\deep\out.png),失败信息带完整路径——MCP 客户端
/// 只能看到这条字符串,没有它无法定位是哪个文件写不进去。
fn write_out(out: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("建目录失败 {}:{e}", parent.display()))?;
    }
    std::fs::write(out, bytes).map_err(|e| format!("写文件失败 {}:{e}", out.display()))
}

fn tool_export(args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        let fmt = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        // AGT-05:scale 上界 —— 巨值经 u64→u32 回绕/巨图分配必须前置拦截
        let scale_raw = args.get("scale").and_then(|v| v.as_u64()).unwrap_or(2);
        if !(1..=8).contains(&scale_raw) {
            return Err(format!(
                "scale {scale_raw} 超出上界(1–8):过大倍率会按画板尺寸平方量级分配内存"
            ));
        }
        let scale = scale_raw as u32;
        let transparent = args
            .get("transparent")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ab = need_artboard(&s.doc, args.get("artboard").and_then(|v| v.as_str()))?;
        let out = PathBuf::from(args.get("out").and_then(|v| v.as_str()).ok_or("缺少 out")?);
        match fmt.as_str() {
            "png" => {
                let (png, warnings) = vb_export::export_artboard_png(
                    &s.doc,
                    ab,
                    scale as f32,
                    transparent,
                    Some(&s.dir),
                )?;
                write_out(&out, &png)?;
                Ok(
                    json!({"ok": true, "out": out.display().to_string(), "bytes": png.len(), "warnings": warnings}),
                )
            }
            "svg" => {
                let svg =
                    vb_export::export_artboard_svg(&s.doc, ab, scale, transparent, Some(&s.dir))?;
                write_out(&out, svg.as_bytes())?;
                Ok(json!({"ok": true, "out": out.display().to_string(), "bytes": svg.len()}))
            }
            "pdf" | "gif" | "mp4" => {
                let Some(wpi_dir) = vb_export::wpi::resolve_wpi_dir() else {
                    return Err(
                        "WPI 不可用:未找到浏览器引擎(宿主可设置环境变量 VB_WPI_DIR 指向 WPI 仓库)"
                            .into(),
                    );
                };
                let wpi_fmt = match fmt.as_str() {
                    "pdf" => vb_export::wpi::WpiFormat::Pdf,
                    "gif" => vb_export::wpi::WpiFormat::Gif,
                    _ => vb_export::wpi::WpiFormat::Mp4,
                };
                let req = vb_export::wpi::WpiExportRequest {
                    format: wpi_fmt,
                    scale: if scale >= 4 {
                        4
                    } else if scale >= 2 {
                        2
                    } else {
                        1
                    },
                    width: 1920,
                    transparent,
                    out,
                    max_wait: 20.0,
                };
                let res = vb_export::wpi::export_via_wpi(&s.doc, &s.dir, &req, &wpi_dir)?;
                Ok(json!({"ok": true, "engine": "wpi", "out": res.out.display().to_string()}))
            }
            other => Err(format!("未知格式:{other}")),
        }
    })
}

fn tool_screenshot(args: &Value) -> Result<Value, String> {
    tool_export(&json!({
        "artboard": args.get("artboard"),
        "format": "png",
        "scale": 1,
        "out": args.get("out"),
    }))
}

fn tool_diff_since(args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        let since = args.get("rev").and_then(|v| v.as_u64()).unwrap_or(0);
        Ok(json!({
            "current_rev": s.doc.rev,
            "since": since,
            "note": "v0.1 返回粗粒度摘要;精确 op 级 diff 在 v0.2+",
            "changed": s.doc.rev > since,
        }))
    })
}

fn tool_save(_args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        vb_doc::export::write_project(&s.doc, &s.dir).map_err(|e| e.to_string())?;
        s.doc.rev += 1;
        Ok(json!({"ok": true, "rev": s.doc.rev}))
    })
}

fn tool_get_html(args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        let files = render_project(&s.doc);
        let scope = args
            .get("scope")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        // AGT-05:schema 声明 enum ["html","css"],非法值必须报错而不是
        // 静默当 html(客户端拼错 scope 却拿到错误产物更难排查)
        let which = match scope.as_deref() {
            None | Some("html") => "index.html",
            Some("css") => "styles/main.css",
            Some(other) => {
                return Err(format!(
                    "scope「{other}」非法:只支持 html | css(schema enum)"
                ))
            }
        };
        let content = files
            .files
            .iter()
            .find(|(p, _)| p == which)
            .map(|(_, c)| c.clone())
            .unwrap_or_default();
        Ok(json!({"path": which, "content": content}))
    })
}

const TOOLS_LIST: &str = r#"[
  {"name":"vellum_open_document","description":"打开项目目录(index.html 所在)","inputSchema":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}},
  {"name":"vellum_get_outline","description":"树形大纲(轻量,默认先调这个)","inputSchema":{"type":"object","properties":{"depth":{"type":"integer"}}}},
  {"name":"vellum_find","description":"按 name/text/tag 查找元素","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"text":{"type":"string"},"tag":{"type":"string"}}}},
  {"name":"vellum_get_element","description":"取元素详情(样式/文本/几何)","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}},
  {"name":"vellum_apply_patch","description":"应用 patch 事务(op: insert/set_text/set_style/set_attr/move/set_box/rename/set_tag/duplicate/delete/group/ungroup/align/order/set_token/new_artboard/boolean;支持 base_rev 乐观锁)","inputSchema":{"type":"object","properties":{"ops":{"type":"array"},"base_rev":{"type":"integer"}},"required":["ops"]}},
  {"name":"vellum_export","description":"导出画板 png/svg/pdf/gif/mp4","inputSchema":{"type":"object","properties":{"artboard":{"type":"string"},"format":{"type":"string","enum":["png","svg","pdf","gif","mp4"]},"scale":{"type":"integer"},"transparent":{"type":"boolean","description":"png/svg 有效:不铺画板底色"},"out":{"type":"string"}},"required":["out"]}},
  {"name":"vellum_screenshot","description":"画板截图 PNG(供多模态模型查看)","inputSchema":{"type":"object","properties":{"artboard":{"type":"string"},"out":{"type":"string"}},"required":["out"]}},
  {"name":"vellum_get_html","description":"取当前 HTML/CSS(共阅稿)","inputSchema":{"type":"object","properties":{"scope":{"type":"string","enum":["html","css"]}}}},
  {"name":"vellum_diff_since","description":"自某 rev 以来的变更摘要","inputSchema":{"type":"object","properties":{"rev":{"type":"integer"}}}},
  {"name":"vellum_save","description":"保存(canonical 重写项目目录)","inputSchema":{"type":"object"}}
]"#;

fn dispatch(method: &str, params: Value) -> Result<Value, Value> {
    match method {
        // AGT-04:版本协商 —— 客户端声明的 protocolVersion 在支持集内
        // → 回声锁定;不在/未声明 → 返回本服务器最新支持版(客户端据此
        // 自行决定继续或断开,MCP 规范语义)
        "initialize" => {
            const SUPPORTED: &[&str] = &["2024-11-05"];
            const LATEST: &str = "2024-11-05";
            let client = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let negotiated = if SUPPORTED.contains(&client) && !client.is_empty() {
                client
            } else {
                LATEST
            };
            Ok(json!({
                "protocolVersion": negotiated,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "vellum-mcp", "version": env!("CARGO_PKG_VERSION")},
            }))
        }
        "tools/list" => {
            // TOOLS_LIST 是编译期内嵌常量,解析失败不可达;按 RB-01 仍
            // 穷尽为协议错误,不做 expect
            let tools: Value = serde_json::from_str(TOOLS_LIST).map_err(
                |e| json!({"code": -32603, "message": format!("tools/list 内部错误:{e}")}),
            )?;
            Ok(json!({"tools": tools}))
        }
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            // panic 隔离:任何工具内的 bug 不得击穿主循环(此前 root sid
            // order 会 panic 掉整个 server,客户端只能等超时)。
            let span = info_span!("mcp_tool_call", tool = name);
            let t0 = Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _g = span.enter();
                match name {
                    "vellum_open_document" => tool_open(&args),
                    "vellum_get_outline" => tool_outline(&args),
                    "vellum_find" => tool_find(&args),
                    "vellum_get_element" => tool_get_element(&args),
                    "vellum_apply_patch" => tool_apply_patch(&args),
                    "vellum_export" => tool_export(&args),
                    "vellum_screenshot" => tool_screenshot(&args),
                    "vellum_get_html" => tool_get_html(&args),
                    "vellum_diff_since" => tool_diff_since(&args),
                    "vellum_save" => tool_save(&args),
                    other => Err(format!("未知工具:{other}")),
                }
            }))
            .unwrap_or_else(|p| {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "工具执行异常".into());
                // AGT-01/RB-10:panic 已毒化会话锁(poison-safe 恢复)且
                // 内存文档可能停在半程状态 —— 从磁盘重建会话再返回错误
                rebuild_session_after_panic();
                Err(format!("内部错误:{msg}(会话已从磁盘重建,未落盘修改已丢弃)"))
            });
            tracing::info!(
                tool = name,
                duration_ms = t0.elapsed().as_millis() as u64,
                "mcp tool call"
            );
            match result {
                Ok(v) => Ok(json!({
                    "content": [{"type": "text", "text": serde_json::to_string(&v).unwrap_or_default()}],
                    "isError": false,
                })),
                Err(e) => {
                    // 未知工具:升级为 JSON-RPC 协议错误(MCP 规范要求
                    // -32602,此前包成 isError 结果,严格客户端会判定违规)
                    if e.starts_with("未知工具:") {
                        Err(json!({"code": -32602, "message": e}))
                    } else {
                        Ok(json!({
                            "content": [{"type": "text", "text": e}],
                            "isError": true,
                        }))
                    }
                }
            }
        }
        other => Err(json!({
            "code": -32601,
            "message": format!("method not found: {other}"),
        })),
    }
}

/// tracing 初始化(RB-11):VB_LOG=debug/info/warn/error 控级别,缺省
/// warn;**必须写 stderr** —— stdio 传输下 stdout 是协议通道,任何日志
/// 侵入 stdout 都是协议损坏。不做全局复杂配置。
fn init_tracing() {
    let level = match std::env::var("VB_LOG")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::WARN,
    };
    let _ = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .try_init();
}

fn main() {
    init_tracing();
    // 可选启动参数:--doc <dir>(打开初始文档);缺值必须报错,
    // 不能静默跳过导致客户端首次调用才发现文档没开;导入失败(AGT-04)
    // 必须带原因退出 1,不能吞掉 —— 否则客户端面对的是"空会话"假象
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--doc") {
        match args.get(i + 1) {
            Some(p) if !p.starts_with('-') => {
                if let Err(e) = tool_open(&json!({"path": p})) {
                    eprintln!("错误:--doc 打开失败:{e}");
                    std::process::exit(1);
                }
            }
            _ => {
                eprintln!("错误:--doc 需要一个目录参数");
                std::process::exit(1);
            }
        }
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            // JSON-RPC 2.0 §4.1:解析失败必须回 -32700,否则客户端只能干等超时
            let resp = json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "Parse error"}});
            writeln!(stdout, "{resp}").ok();
            stdout.flush().ok();
            continue;
        };
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
        // 通知帧(无 id)不得执行有副作用的调用:无回执地址,执行了客户端也无从得知
        if id.is_none() {
            continue;
        }
        match dispatch(method, msg.get("params").cloned().unwrap_or(Value::Null)) {
            Ok(result) => {
                let resp = json!({"jsonrpc": "2.0", "id": id, "result": result});
                writeln!(stdout, "{resp}").ok();
                stdout.flush().ok();
            }
            Err(err) => {
                let resp = json!({"jsonrpc": "2.0", "id": id, "error": err});
                writeln!(stdout, "{resp}").ok();
                stdout.flush().ok();
            }
        }
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;

    /// tools/list 的 op 名单必须覆盖全部 PatchOp 变体
    /// (patch_op_name 是穷尽 match,新增变体编译期强制补名)。
    #[test]
    fn tools_list_lists_every_patch_op() {
        let tools: Value = serde_json::from_str(TOOLS_LIST).expect("TOOLS_LIST 合法 JSON");
        let desc = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "vellum_apply_patch")
            .expect("apply_patch 工具存在")["description"]
            .as_str()
            .unwrap()
            .to_string();
        for name in vb_agent::PATCH_OP_NAMES {
            assert!(desc.contains(name), "tools/list 缺 op 名:{name}");
        }
    }

    /// export 工具 schema 声明 transparent(与实现一致)。
    #[test]
    fn export_schema_has_transparent() {
        let tools: Value = serde_json::from_str(TOOLS_LIST).unwrap();
        let schema = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "vellum_export")
            .unwrap()["inputSchema"]
            .clone();
        assert!(
            schema["properties"]["transparent"].is_object(),
            "vellum_export schema 缺 transparent:{schema}"
        );
    }
}

#[cfg(test)]
mod agt_tests {
    use super::*;

    /// AGT 用例共享进程级 `static SESSION`:cargo test 默认并行,
    /// 三个开文档的用例必须串行,否则互相替换/清空对方的会话。
    static AGT_TEST_SER: Mutex<()> = Mutex::new(());

    fn agt_guard() -> std::sync::MutexGuard<'static, ()> {
        AGT_TEST_SER.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 最小可导入项目(index.html 带 vb-artboard 画板)。
    fn make_project(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vb-mcp-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.html"),
            r#"<!DOCTYPE html>
<html lang="zh-CN"><head><meta charset="UTF-8"><title>t</title></head>
<body>
  <section class="vb-artboard" data-vb-id="ab00000001" data-vb-name="画板" style="width: 800px; height: 600px;">
    <div class="vb-el-a" data-vb-id="el00000001" data-vb-name="块" style="left: 10px; top: 10px; width: 100px; height: 40px;"></div>
  </section>
</body></html>"#,
        )
        .unwrap();
        dir
    }

    /// AGT-01/RB-10 注入用例:工具 panic 毒化锁之后,后续 tools/call
    /// 仍可成功(此前 lock().map_err(|_| "session poisoned") 恒失败,
    /// 一次 panic = 会话永久报废)。
    #[test]
    fn session_survives_tool_panic() {
        let _g = agt_guard();
        let dir = make_project("poison");
        // 建立会话 → 在持锁路径内注入 panic(毒化 Mutex)
        tool_open(&json!({"path": dir.display().to_string()})).expect("打开项目");
        let injected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = with_session(|_s| -> Result<Value, String> { panic!("注入:工具 panic") });
        }));
        assert!(injected.is_err(), "panic 必须真的发生(毒化锁)");
        // panic 后 tools/call 仍成功(结构化结果,而非 lock 恒败)
        let resp = dispatch(
            "tools/call",
            json!({"name": "vellum_get_outline", "arguments": {}}),
        )
        .expect("panic 后 tools/call 必须仍在 JSON-RPC 层成功");
        assert_eq!(resp["isError"], false, "大纲读取必须成功:{resp}");
        assert!(
            resp["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .contains("画板"),
            "会话内容仍可用:{resp}"
        );
        // panic 后重建路径:直接调用重建,磁盘副本回到会话
        rebuild_session_after_panic();
        let resp2 = dispatch(
            "tools/call",
            json!({"name": "vellum_get_outline", "arguments": {}}),
        )
        .expect("重建后 tools/call 成功");
        assert_eq!(resp2["isError"], false, "{resp2}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// AGT-01:panic 后会话重建失败(目录已不可导入)→ 会话清空,
    /// 后续调用得到**结构化**「未打开文档」,不是 panic/挂死。
    #[test]
    fn rebuild_failure_degrades_to_structured_error() {
        let _g = agt_guard();
        let dir = make_project("rebuild-fail");
        tool_open(&json!({"path": dir.display().to_string()})).expect("打开项目");
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = with_session(|_s| -> Result<Value, String> { panic!("注入") });
        }));
        // 目录被删 → 重建失败 → 会话清空
        let _ = std::fs::remove_dir_all(&dir);
        rebuild_session_after_panic();
        let resp = dispatch(
            "tools/call",
            json!({"name": "vellum_get_outline", "arguments": {}}),
        )
        .expect("仍是 JSON-RPC 层成功");
        assert_eq!(resp["isError"], true, "无文档应回 isError 结果:{resp}");
        assert!(
            resp["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .contains("未打开文档"),
            "{resp}"
        );
    }

    /// AGT-04:initialize 版本协商 —— 支持集内回声锁定,未知版本回
    /// 服务器最新支持版(由客户端决定继续或断开)。
    #[test]
    fn initialize_negotiates_protocol_version() {
        let echo = dispatch("initialize", json!({"protocolVersion": "2024-11-05"}))
            .expect("initialize 成功");
        assert_eq!(echo["protocolVersion"], "2024-11-05");
        let fallback =
            dispatch("initialize", json!({"protocolVersion": "2099-01-01"})).expect("成功");
        assert_eq!(
            fallback["protocolVersion"], "2024-11-05",
            "未知版本回退到最新支持版"
        );
        let bare = dispatch("initialize", json!({})).expect("成功");
        assert_eq!(bare["protocolVersion"], "2024-11-05");
    }

    /// AGT-05:get_html 的 scope 非法值必须报错(schema enum),不再静默
    /// 当 html;导出 scale 上界前置拦截。
    #[test]
    fn get_html_scope_and_export_scale_validated() {
        let _g = agt_guard();
        let dir = make_project("scope");
        tool_open(&json!({"path": dir.display().to_string()})).expect("打开项目");

        let bad_scope = dispatch(
            "tools/call",
            json!({"name": "vellum_get_html", "arguments": {"scope": "js"}}),
        )
        .expect("JSON-RPC 层成功");
        assert_eq!(bad_scope["isError"], true, "{bad_scope}");
        assert!(
            bad_scope["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .contains("scope"),
            "报错要点名 scope:{bad_scope}"
        );
        // 合法值不受影响
        let ok_html = dispatch(
            "tools/call",
            json!({"name": "vellum_get_html", "arguments": {"scope": "html"}}),
        )
        .expect("成功");
        assert_eq!(ok_html["isError"], false, "{ok_html}");

        let huge_scale = dispatch(
            "tools/call",
            json!({"name": "vellum_export",
                   "arguments": {"scale": 4294967297i64, "out": "D:/Temp/x.png"}}),
        )
        .expect("JSON-RPC 层成功");
        assert_eq!(huge_scale["isError"], true, "{huge_scale}");
        assert!(
            huge_scale["content"][0]["text"]
                .as_str()
                .unwrap_or("")
                .contains("scale"),
            "报错要点名 scale:{huge_scale}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod b6_tests {
    use super::*;

    /// B6:未知工具返回 JSON-RPC 协议错误 -32602(MCP 规范),
    /// 不再包成 isError 结果。
    #[test]
    fn unknown_tool_is_protocol_error() {
        let resp = dispatch(
            "tools/call",
            json!({"name": "vellum_no_such_tool", "arguments": {}}),
        );
        match resp {
            Ok(v) => panic!("未知工具不应返回成功结果:{v}"),
            Err(err) => {
                assert_eq!(err["code"], -32602, "{err}");
                assert!(
                    err["message"].as_str().unwrap_or("").contains("未知工具"),
                    "{err}"
                );
            }
        }
    }
}
