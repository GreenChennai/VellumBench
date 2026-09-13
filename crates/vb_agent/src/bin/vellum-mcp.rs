//! `vellum-mcp` — MCP Server(stdio 传输,设计文档 08 篇 §四)。
//!
//! 协议:MCP 规范 = stdio 上的换行分隔 JSON-RPC 2.0。
//! 工具与 CLI/菜单同一条命令路径(vb_doc::commands),天然可撤销、一致。
//!
//! 启动:`vellum-mcp [--doc <项目目录>]`(亦可在会话中调 vellum_open_document)。

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

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

fn with_session(f: impl FnOnce(&mut Session) -> Result<Value, String>) -> Result<Value, String> {
    let mut guard = SESSION.lock().map_err(|_| "session poisoned")?;
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
    *SESSION.lock().map_err(|_| "session poisoned")? = Some(Session {
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
        let n = s.doc.nodes.get(nid).unwrap();
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

fn tool_export(args: &Value) -> Result<Value, String> {
    with_session(|s: &mut Session| {
        let fmt = args
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let scale = args.get("scale").and_then(|v| v.as_u64()).unwrap_or(2) as u32;
        let ab = need_artboard(&s.doc, args.get("artboard").and_then(|v| v.as_str()))?;
        let out = PathBuf::from(args.get("out").and_then(|v| v.as_str()).ok_or("缺少 out")?);
        match fmt.as_str() {
            "png" => {
                let (png, warnings) =
                    vb_export::export_artboard_png(&s.doc, ab, scale as f32, false, Some(&s.dir))?;
                std::fs::write(&out, &png).map_err(|e| e.to_string())?;
                Ok(
                    json!({"ok": true, "out": out.display().to_string(), "bytes": png.len(), "warnings": warnings}),
                )
            }
            "svg" => {
                let svg = vb_export::export_artboard_svg(&s.doc, ab, scale, false)?;
                std::fs::write(&out, &svg).map_err(|e| e.to_string())?;
                Ok(json!({"ok": true, "out": out.display().to_string(), "bytes": svg.len()}))
            }
            "pdf" | "gif" | "mp4" => {
                let Some(wpi_dir) = vb_export::wpi::resolve_wpi_dir() else {
                    return Err("WPI 不可用:未找到浏览器引擎(宿主可设置环境变量 VB_WPI_DIR 指向 WPI 仓库)".into());
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
                    transparent: false,
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
        let which = match scope.as_deref() {
            Some("css") => "styles/main.css",
            _ => "index.html",
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
  {"name":"vellum_apply_patch","description":"应用 patch 事务(13 种 op,支持 base_rev 乐观锁)","inputSchema":{"type":"object","properties":{"ops":{"type":"array"},"base_rev":{"type":"integer"}},"required":["ops"]}},
  {"name":"vellum_export","description":"导出画板 png/svg/pdf/gif/mp4","inputSchema":{"type":"object","properties":{"artboard":{"type":"string"},"format":{"type":"string","enum":["png","svg","pdf","gif","mp4"]},"scale":{"type":"integer"},"out":{"type":"string"}},"required":["out"]}},
  {"name":"vellum_screenshot","description":"画板截图 PNG(供多模态模型查看)","inputSchema":{"type":"object","properties":{"artboard":{"type":"string"},"out":{"type":"string"}},"required":["out"]}},
  {"name":"vellum_get_html","description":"取当前 HTML/CSS(共阅稿)","inputSchema":{"type":"object","properties":{"scope":{"type":"string","enum":["html","css"]}}}},
  {"name":"vellum_diff_since","description":"自某 rev 以来的变更摘要","inputSchema":{"type":"object","properties":{"rev":{"type":"integer"}}}},
  {"name":"vellum_save","description":"保存(canonical 重写项目目录)","inputSchema":{"type":"object"}}
]"#;

fn dispatch(method: &str, params: Value) -> Result<Value, Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "vellum-mcp", "version": env!("CARGO_PKG_VERSION")},
        })),
        "tools/list" => {
            let tools: Value =
                serde_json::from_str(TOOLS_LIST).expect("TOOLS_LIST 必须是合法 JSON");
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
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match name {
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
            }))
            .unwrap_or_else(|p| {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_string()))
                    .unwrap_or_else(|| "工具执行异常".into());
                Err(format!("内部错误:{msg}"))
            });
            match result {
                Ok(v) => Ok(json!({
                    "content": [{"type": "text", "text": serde_json::to_string(&v).unwrap_or_default()}],
                    "isError": false,
                })),
                Err(e) => Ok(json!({
                    "content": [{"type": "text", "text": e}],
                    "isError": true,
                })),
            }
        }
        other => Err(json!({
            "code": -32601,
            "message": format!("method not found: {other}"),
        })),
    }
}

fn main() {
    // 可选启动参数:--doc <dir>(打开初始文档)
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--doc") {
        if let Some(p) = args.get(i + 1) {
            let _ = tool_open(&json!({"path": p}));
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
