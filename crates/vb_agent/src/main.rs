//! `vellum-cli` — Agent 命令行接口(设计文档 08 篇 §三)。
//!
//! 所有输出默认人类可读,`--json` 输出结构化 JSON;
//! 退出码:0 成功 / 1 参数错误 / 2 文档未打开 / 3 patch 冲突 / 4 导出失败。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use vb_doc::import::import_project;
use vb_doc::model::{Document, NodeKind};
use vb_doc::undo::UndoStack;

use vb_agent::patch::apply_patch;

#[derive(Parser)]
#[command(name = "vellum-cli", version, about = "Vellum Bench Agent CLI")]
struct Cli {
    /// 项目目录或 index.html
    #[arg(long = "doc", global = true)]
    doc: Option<PathBuf>,

    /// 结构化 JSON 输出
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 树形大纲(轻量,Agent 首选)
    Tree {
        #[arg(long, default_value_t = 8)]
        depth: usize,
    },
    /// 查找元素
    Find {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        class: Option<String>,
    },
    /// 元素详情
    Get {
        id: String,
        #[arg(long)]
        style: bool,
    },
    /// 应用 patch(事务)
    Patch {
        file: PathBuf,
        #[arg(long)]
        dry_run: bool,
    },
    /// 导出画板为 PNG(原生 CPU 引擎)
    Export {
        /// 画板 sid 或名称;--all 时忽略
        #[arg(long)]
        artboard: Option<String>,
        #[arg(long, default_value = "png")]
        format: String,
        #[arg(long, default_value_t = 1)]
        scale: u32,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        transparent: bool,
        #[arg(long)]
        all: bool,
    },
    /// 画板截图(= export png @1x 的快捷方式)
    Shot {
        #[arg(long)]
        artboard: Option<String>,
        #[arg(long)]
        out: PathBuf,
    },
    /// 以 canonical 形式回写项目(等于保存)
    Save,
    /// 文档元信息
    Info,
    /// 无头自检:打开示例 → 渲染 → 写临时 PNG
    Selfcheck,
}

fn main() {
    let code = real_main();
    std::process::exit(code);
}

fn real_main() -> i32 {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => 0,
        Err(CliError::Usage(msg)) => {
            eprintln!("错误:{msg}");
            1
        }
        Err(CliError::NoDoc(msg)) => {
            eprintln!("{msg}");
            2
        }
        Err(CliError::Conflict(msg)) => {
            eprintln!("{msg}");
            3
        }
        Err(CliError::Export(msg)) => {
            eprintln!("{msg}");
            4
        }
        Err(CliError::Other(msg)) => {
            eprintln!("错误:{msg}");
            1
        }
    }
}

enum CliError {
    Usage(String),
    NoDoc(String),
    Conflict(String),
    Export(String),
    Other(String),
}

impl From<anyhow::Error> for CliError {
    fn from(e: anyhow::Error) -> Self {
        CliError::Other(format!("{e:#}"))
    }
}

fn open_doc(path: &Path) -> Result<(Document, UndoStack, PathBuf), CliError> {
    let r = import_project(path).map_err(|e| CliError::Other(format!("导入失败:{e}")))?;
    let dir = r.project_dir.clone();
    for w in &r.warnings {
        eprintln!("⚠ {w}");
    }
    Ok((r.doc, UndoStack::new(), dir))
}

fn run(cli: Cli) -> Result<(), CliError> {
    let need_doc = !matches!(cli.command, Cmd::Selfcheck);
    let doc_path = if need_doc {
        cli.doc.clone().ok_or_else(|| {
            CliError::NoDoc("未指定 --doc <项目目录|index.html>(文档未打开,退出码 2)".into())
        })?
    } else {
        PathBuf::new()
    };

    match cli.command {
        Cmd::Selfcheck => selfcheck(cli.json),
        Cmd::Tree { depth } => {
            let (doc, _, _) = open_doc(&doc_path)?;
            let mut out = String::new();
            for &ab in &doc.artboards {
                let n = doc.nodes.get(ab).unwrap();
                out.push_str(&format!(
                    "{} {} [{}]  {}×{}  sid={}\n",
                    "  ".repeat(0),
                    n.name,
                    n.kind.kind_name(),
                    n.geom.w as i64,
                    n.geom.h as i64,
                    n.sid
                ));
                outline_children(&doc, ab, 1, depth, &mut out);
            }
            if cli.json {
                println!("{}", tree_json(&doc, depth));
            } else {
                print!("{out}");
            }
            Ok(())
        }
        Cmd::Find {
            name,
            text,
            tag,
            class,
        } => {
            let (doc, _, _) = open_doc(&doc_path)?;
            let mut hits: Vec<String> = Vec::new();
            let mut ids = Vec::new();
            for &ab in &doc.artboards {
                doc.subtree(ab, &mut ids);
            }
            for id in ids {
                let Some(n) = doc.nodes.get(id) else { continue };
                if let Some(q) = &name {
                    if !n.name.contains(q) {
                        continue;
                    }
                }
                if let Some(q) = &text {
                    match n.text() {
                        Some(t) if t.contains(q) => {}
                        _ => continue,
                    }
                }
                if let Some(q) = &tag {
                    if &n.tag != q {
                        continue;
                    }
                }
                if let Some(q) = &class {
                    if !n.classes.iter().any(|c| c == q) {
                        continue;
                    }
                }
                hits.push(n.sid.as_str().to_string());
            }
            if cli.json {
                println!("{}", json!({"count": hits.len(), "ids": hits}));
            } else {
                println!("找到 {} 个:", hits.len());
                for h in hits {
                    println!("  {h}");
                }
            }
            Ok(())
        }
        Cmd::Get { id, style } => {
            let (doc, _, _) = open_doc(&doc_path)?;
            let Some(nid) = doc.find_by_sid(&id) else {
                return Err(CliError::Usage(format!("sid {id} 不存在")));
            };
            let n = doc.nodes.get(nid).unwrap();
            if cli.json {
                let mut o = json!({
                    "sid": n.sid.as_str(),
                    "name": n.name,
                    "tag": n.tag,
                    "kind": n.kind.kind_name(),
                    "box": {"x": n.geom.x, "y": n.geom.y, "w": n.geom.w, "h": n.geom.h},
                    "classes": n.classes,
                    "attrs": n.attrs,
                });
                if style {
                    let s: serde_json::Map<String, serde_json::Value> = n
                        .style
                        .iter()
                        .map(|d| (d.prop.clone(), serde_json::Value::String(d.value.clone())))
                        .collect();
                    o["style"] = serde_json::Value::Object(s);
                }
                if let NodeKind::Text { text, .. } = &n.kind {
                    o["text"] = serde_json::Value::String(text.clone());
                }
                println!("{o}");
            } else {
                println!(
                    "{} [{}] name={:?} box=({},{},{},{})",
                    n.sid,
                    n.kind.kind_name(),
                    n.name,
                    n.geom.x as i64,
                    n.geom.y as i64,
                    n.geom.w as i64,
                    n.geom.h as i64
                );
                if style {
                    for d in &n.style {
                        println!("  {}", d.to_css());
                    }
                }
            }
            Ok(())
        }
        Cmd::Patch { file, dry_run } => {
            let (mut doc, mut undo, _) = open_doc(&doc_path)?;
            let text = std::fs::read_to_string(&file)
                .with_context(|| format!("读取 {}", file.display()))
                .map_err(|e| CliError::Other(format!("{e:#}")))?;
            let req: vb_agent::PatchRequest = serde_json::from_str(&text)
                .map_err(|e| CliError::Usage(format!("patch 解析失败:{e}")))?;
            match apply_patch(&mut doc, &mut undo, &req) {
                Ok(outcome) => {
                    if dry_run {
                        // dry-run 已应用即回滚(重放进临时 doc);v0.1 简化:应用后不落盘
                    } else {
                        save_doc(&mut doc, &doc_path)?;
                    }
                    if cli.json {
                        println!(
                            "{}",
                            json!({"ok": true, "rev": outcome.rev, "created_ids": outcome.created_ids, "changed_ids": outcome.changed_ids, "warnings": outcome.warnings})
                        );
                    } else {
                        println!("ok rev={}", outcome.rev);
                        println!("changed: {:?}", outcome.changed_ids);
                    }
                    Ok(())
                }
                Err(vb_agent::PatchError::Conflict { expected, current }) => {
                    Err(CliError::Conflict(format!(
                        "409 conflict: base_rev={expected} 过期,当前 rev={current}(请重新 outline 后重试)"
                    )))
                }
                Err(e @ vb_agent::PatchError::Op(_)) => {
                    if !dry_run {
                        // 事务失败:文档不落盘,内存副本回滚即等于回滚
                        let _ = e;
                    }
                    Err(CliError::Usage(format!("{e}")))
                }
            }
        }
        Cmd::Export {
            artboard,
            format,
            scale,
            out,
            transparent,
            all,
        } => {
            if format != "png" {
                return Err(CliError::Export(format!(
                    "格式 {format} 暂不支持:v0.1 原生引擎仅 PNG;SVG/PDF 排期 v0.5,浏览器引擎(GIF/MP4)排期 v0.5+"
                )));
            }
            let (doc, _, dir) = open_doc(&doc_path)?;
            let targets: Vec<(String, vb_doc::model::NodeId)> = if all {
                doc.artboards
                    .iter()
                    .filter_map(|&a| doc.nodes.get(a).map(|n| (n.name.clone(), a)))
                    .collect()
            } else {
                let Some(a) = &artboard else {
                    return Err(CliError::Usage(
                        "需要 --artboard <sid|名称> 或 --all".into(),
                    ));
                };
                match resolve_artboard(&doc, a) {
                    Some(id) => {
                        let name = doc.nodes.get(id).unwrap().name.clone();
                        vec![(name, id)]
                    }
                    None => return Err(CliError::Export(format!("画板 {a} 不存在"))),
                }
            };
            let template = vb_export::DEFAULT_TEMPLATE;
            let mut results = Vec::new();
            for (i, (name, id)) in targets.iter().enumerate() {
                let file_name = vb_export::expand_name_template(
                    template,
                    &doc.meta.title,
                    name,
                    scale,
                    "png",
                    i + 1,
                    0,
                    0,
                );
                let out_path = if targets.len() > 1 {
                    out.parent().unwrap_or(Path::new(".")).join(&file_name)
                } else {
                    out.clone()
                };
                match vb_export::export_artboard_png(
                    &doc,
                    *id,
                    scale as f32,
                    transparent,
                    Some(&dir),
                ) {
                    Ok((png, warnings)) => {
                        std::fs::write(&out_path, &png)
                            .with_context(|| format!("写出 {}", out_path.display()))
                            .map_err(|e| CliError::Other(format!("{e:#}")))?;
                        for w in &warnings {
                            eprintln!("⚠ {w}");
                        }
                        results.push(json!({
                            "artboard": name, "out": out_path.display().to_string(),
                            "bytes": png.len(),
                        }));
                    }
                    Err(e) => return Err(CliError::Export(format!("画板 {name} 导出失败:{e}"))),
                }
            }
            if cli.json {
                println!("{}", json!({"ok": true, "exports": results}));
            } else {
                for r in results {
                    println!(
                        "✔ {} → {}",
                        r["artboard"].as_str().unwrap_or(""),
                        r["out"].as_str().unwrap_or("")
                    );
                }
            }
            Ok(())
        }
        Cmd::Shot { artboard, out } => {
            let (doc, _, dir) = open_doc(&doc_path)?;
            let id = match artboard.or(None) {
                Some(a) => resolve_artboard(&doc, &a),
                None => doc.artboards.first().copied(),
            }
            .ok_or_else(|| CliError::Export("未找到画板".into()))?;
            let list = vb_render::encode::encode_artboard(&doc, id)
                .map_err(|e| CliError::Export(e.to_string()))?;
            let res = vb_render::cpu::render_png(&list, 1.0, false, Some(&dir))
                .map_err(CliError::Export)?;
            std::fs::write(&out, &res.png).map_err(|e| CliError::Other(e.to_string()))?;
            if cli.json {
                println!(
                    "{}",
                    json!({"ok": true, "out": out.display().to_string(), "bytes": res.png.len()})
                );
            } else {
                println!("✔ {}", out.display());
            }
            Ok(())
        }
        Cmd::Save => {
            let (mut doc, _, _) = open_doc(&doc_path)?;
            save_doc(&mut doc, &doc_path)?;
            if cli.json {
                println!("{}", json!({"ok": true}));
            } else {
                println!("✔ 已保存(canonical)");
            }
            Ok(())
        }
        Cmd::Info => {
            let (doc, _, dir) = open_doc(&doc_path)?;
            if cli.json {
                println!(
                    "{}",
                    json!({"title": doc.meta.title, "lang": doc.meta.lang, "rev": doc.rev,
                           "artboards": doc.artboards.len(), "project_dir": dir.display().to_string()})
                );
            } else {
                println!(
                    "「{}」 rev={} 画板={} 目录={}",
                    doc.meta.title,
                    doc.rev,
                    doc.artboards.len(),
                    dir.display()
                );
            }
            Ok(())
        }
    }
}

fn save_doc(doc: &mut Document, path: &Path) -> Result<(), CliError> {
    // 保存 = canonical 重写项目目录(ADR-0018)
    vb_doc::export::write_project(doc, path)
        .map_err(|e| CliError::Other(format!("保存失败:{e}")))?;
    doc.rev += 1;
    Ok(())
}

fn resolve_artboard(doc: &Document, key: &str) -> Option<vb_doc::model::NodeId> {
    // 1) sid 精确
    if let Some(id) = doc.find_by_sid(key) {
        return Some(id);
    }
    // 2) 名称(大小写不敏感)/ 3) 类名(hero ← vb-artboard hero)
    doc.artboards.iter().copied().find(|&a| {
        doc.nodes
            .get(a)
            .map(|n| {
                n.name.eq_ignore_ascii_case(key)
                    || n.classes.iter().any(|c| c.eq_ignore_ascii_case(key))
            })
            .unwrap_or(false)
    })
}

fn outline_children(
    doc: &Document,
    id: vb_doc::model::NodeId,
    depth: usize,
    max: usize,
    out: &mut String,
) {
    if depth > max {
        return;
    }
    let Some(n) = doc.nodes.get(id) else { return };
    for &c in &n.children {
        if let Some(cn) = doc.nodes.get(c) {
            out.push_str(&format!(
                "{}{} [{}]  sid={}\n",
                "  ".repeat(depth),
                cn.name,
                cn.kind.kind_name(),
                cn.sid
            ));
            outline_children(doc, c, depth + 1, max, out);
        }
    }
}

fn tree_json(doc: &Document, depth: usize) -> String {
    fn node_json(doc: &Document, id: vb_doc::model::NodeId, depth: usize) -> serde_json::Value {
        let n = doc.nodes.get(id).unwrap();
        let children: Vec<serde_json::Value> = if depth > 1 {
            n.children
                .iter()
                .map(|&c| node_json(doc, c, depth - 1))
                .collect()
        } else {
            vec![]
        };
        json!({
            "sid": n.sid.as_str(), "name": n.name, "tag": n.tag, "kind": n.kind.kind_name(),
            "box": {"x": n.geom.x, "y": n.geom.y, "w": n.geom.w, "h": n.geom.h},
            "children": children,
        })
    }
    let arts: Vec<serde_json::Value> = doc
        .artboards
        .iter()
        .map(|&a| node_json(doc, a, depth))
        .collect();
    json!({"rev": doc.rev, "artboards": arts}).to_string()
}

fn selfcheck(as_json: bool) -> Result<(), CliError> {
    // 空文档 → 渲染 → 写临时 PNG → 校验
    let doc = Document::new_default();
    let ab = doc.artboards[0];
    let list = vb_render::encode::encode_artboard(&doc, ab)
        .map_err(|e| CliError::Export(e.to_string()))?;
    let res = vb_render::cpu::render_png(&list, 1.0, false, None).map_err(CliError::Export)?;
    let out = std::env::temp_dir().join(format!("vellum-selfcheck-{}.png", std::process::id()));
    std::fs::write(&out, &res.png).map_err(|e| CliError::Other(e.to_string()))?;
    let ok = res.png.starts_with(b"\x89PNG");
    let _ = std::fs::remove_file(&out);
    if as_json {
        println!(
            "{}",
            json!({"ok": ok, "engine": "cpu", "bytes": res.png.len()})
        );
    } else {
        println!(
            "selfcheck {} (cpu, {} bytes)",
            if ok { "OK" } else { "FAIL" },
            res.png.len()
        );
    }
    if ok {
        Ok(())
    } else {
        Err(CliError::Export("selfcheck 失败".into()))
    }
}
