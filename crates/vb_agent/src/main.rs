//! `vellum-cli` — Agent 命令行接口(设计文档 08 篇 §三)。
//!
//! 所有输出默认人类可读,`--json` 输出结构化 JSON;
//! 退出码:0 成功 / 1 参数错误 / 2 文档未打开 / 3 patch 冲突 / 4 导出失败。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
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
    /// 性能基准(P5.3,B1–B6 简版):合成画板 → 计时编码+渲染
    Bench {
        #[arg(long, default_value_t = 200)]
        objects: usize,
    },
    /// 数据驱动批量(v1.4):CSV 行 × ops 模板(支持 {列名} 占位)→ 事务 patch
    Batch {
        /// CSV 文件(首行为表头)
        #[arg(long)]
        csv: PathBuf,
        /// ops 模板 JSON:{"ops":[...]} 中字符串值支持 {列名} 占位符
        #[arg(long)]
        template: PathBuf,
    },
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
    let need_doc = !matches!(cli.command, Cmd::Selfcheck | Cmd::Bench { .. });
    let doc_path = if need_doc {
        cli.doc.clone().ok_or_else(|| {
            CliError::NoDoc("未指定 --doc <项目目录|index.html>(文档未打开,退出码 2)".into())
        })?
    } else {
        PathBuf::new()
    };

    match cli.command {
        Cmd::Selfcheck => selfcheck(cli.json),
        Cmd::Bench { objects } => {
            let t0 = std::time::Instant::now();
            let mut doc = Document::new("Bench", "zh-CN");
            let ab = doc.artboards[0];
            doc.nodes.get_mut(ab).unwrap().geom.h = (objects as f64 / 20.0 + 10.0) * 40.0;
            for i in 0..objects {
                let sid = doc.alloc_sid();
                let mut n = vb_doc::model::Node::new(
                    vb_doc::model::NodeKind::Box,
                    format!("对象 {i}"),
                    sid.clone(),
                );
                n.geom = vb_doc::model::Geom {
                    x: (i % 20) as f64 * 40.0 + 10.0,
                    y: (i / 20) as f64 * 40.0 + 10.0,
                    w: 30.0,
                    h: 30.0,
                };
                n.style.push(vb_css::Decl {
                    prop: "background-color".into(),
                    value: format!("#{:06x}", 0x3399ff + (i % 16) * 0x001111),
                    important: false,
                });
                n.style.push(vb_css::Decl {
                    prop: "border-radius".into(),
                    value: format!("{}px", 4 + i % 8),
                    important: false,
                });
                let pid = doc
                    .find_by_sid(doc.nodes.get(ab).unwrap().sid.as_str())
                    .unwrap();
                let id = doc.nodes.insert(n);
                doc.nodes.get_mut(id).unwrap().parent = Some(pid);
                doc.nodes.get_mut(pid).unwrap().children.push(id);
            }
            let build_t = t0.elapsed();
            let list = vb_render::encode::encode_artboard(&doc, ab)
                .map_err(|e| CliError::Export(e.to_string()))?;
            let encode_t = t0.elapsed() - build_t;
            let res =
                vb_render::cpu::render_png(&list, 1.0, false, None).map_err(CliError::Export)?;
            let render_t = t0.elapsed() - build_t - encode_t;
            if cli.json {
                println!(
                    "{}",
                    json!({
                        "objects": objects,
                        "build_ms": build_t.as_millis(),
                        "encode_ms": encode_t.as_millis(),
                        "render_ms": render_t.as_millis() as u64,
                        "total_ms": t0.elapsed().as_millis(),
                        "png_bytes": res.png.len(),
                    })
                );
            } else {
                println!(
                    "bench {} objects: build {:?} / encode {:?} / render {:?} / total {:?} → {} KB",
                    objects,
                    build_t,
                    encode_t,
                    render_t,
                    t0.elapsed(),
                    res.png.len() / 1024
                );
            }
            Ok(())
        }
        Cmd::Batch { csv, template } => {
            let (mut doc, mut undo, _) = open_doc(&doc_path)?;
            // 解析 CSV(首行表头;支持带引号字段)
            let csv_text = std::fs::read_to_string(&csv)
                .with_context(|| format!("读取 {}", csv.display()))
                .map_err(|e| CliError::Other(format!("{e:#}")))?;
            let mut rows: Vec<Vec<String>> = Vec::new();
            for line in csv_text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let mut fields = Vec::new();
                let mut cur = String::new();
                let mut in_q = false;
                let mut chars = line.chars().peekable();
                while let Some(c) = chars.next() {
                    match c {
                        '"' if in_q && chars.peek() == Some(&'"') => {
                            cur.push('"');
                            chars.next();
                        }
                        '"' => in_q = !in_q,
                        ',' if !in_q => fields.push(std::mem::take(&mut cur)),
                        c => cur.push(c),
                    }
                }
                fields.push(cur);
                rows.push(fields);
            }
            if rows.len() < 2 {
                return Err(CliError::Usage("CSV 至少需要表头 + 1 行数据".into()));
            }
            let headers = rows[0].clone();
            let tpl = std::fs::read_to_string(&template)
                .with_context(|| format!("读取 {}", template.display()))
                .map_err(|e| CliError::Other(format!("{e:#}")))?;

            // 逐行展开模板:{列名} 占位符替换(含索引 {row})
            let mut all_ops: Vec<Value> = Vec::new();
            for (ri, row) in rows[1..].iter().enumerate() {
                let mut expanded = tpl.clone();
                for (ci, h) in headers.iter().enumerate() {
                    let v = row.get(ci).map(String::as_str).unwrap_or("");
                    expanded = expanded.replace(&format!("{{{h}}}"), v);
                }
                expanded = expanded.replace("{row}", &(ri + 1).to_string());
                let ops: Value = serde_json::from_str(&expanded)
                    .with_context(|| format!("第 {ri} 行展开后解析失败"))
                    .map_err(|e| CliError::Other(format!("{e:#}")))?;
                if let Some(arr) = ops.get("ops").and_then(|v| v.as_array()) {
                    all_ops.extend(arr.iter().cloned());
                }
            }
            let req = vb_agent::PatchRequest {
                base_rev: None,
                ops: serde_json::from_value(Value::Array(all_ops))
                    .map_err(|e| CliError::Usage(format!("展开后的 ops 非法:{e}")))?,
            };
            let outcome = apply_patch(&mut doc, &mut undo, &req)
                .map_err(|e| CliError::Usage(format!("{e}")))?;
            save_doc(&mut doc, &doc_path)?;
            if cli.json {
                println!(
                    "{}",
                    json!({"ok": true, "rows": rows.len() - 1, "rev": outcome.rev, "changed_ids": outcome.changed_ids})
                );
            } else {
                println!("✔ 批量完成:{} 行 → rev {}", rows.len() - 1, outcome.rev);
            }
            Ok(())
        }
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
            let fmt = format.to_ascii_lowercase();
            // 浏览器引擎格式(PDF/GIF/MP4)走 WPI 桥
            if matches!(fmt.as_str(), "pdf" | "gif" | "mp4") {
                let (doc, _, dir) = open_doc(&doc_path)?;
                let _ab_id = match &artboard {
                    Some(a) => resolve_artboard(&doc, a),
                    None => doc.artboards.first().copied(),
                }
                .ok_or_else(|| CliError::Export("未找到画板".into()))?;
                let wpi_dir = std::path::PathBuf::from(vb_export::wpi::DEFAULT_WPI_DIR);
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
                    out: out.clone(),
                    max_wait: 20.0,
                };
                let res = vb_export::wpi::export_via_wpi(&doc, &dir, &req, &wpi_dir)
                    .map_err(CliError::Export)?;
                if cli.json {
                    println!(
                        "{}",
                        json!({"ok": true, "engine": "wpi", "out": res.out.display().to_string(), "warnings": res.warnings})
                    );
                } else {
                    println!("✔ 浏览器引擎 → {}", res.out.display());
                    for w in &res.warnings {
                        eprintln!("⚠ {w}");
                    }
                }
                return Ok(());
            }
            let is_svg = fmt == "svg";
            if !is_svg && fmt != "png" {
                return Err(CliError::Export(format!(
                    "未知格式 {format}:支持 png | svg | pdf | gif | mp4"
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
            let ext = if is_svg { "svg" } else { "png" };
            let mut results = Vec::new();
            for (i, (name, id)) in targets.iter().enumerate() {
                let file_name = vb_export::expand_name_template(
                    template,
                    &doc.meta.title,
                    name,
                    scale,
                    ext,
                    i + 1,
                    0,
                    0,
                );
                let out_path = if targets.len() > 1 {
                    out.parent().unwrap_or(Path::new(".")).join(&file_name)
                } else {
                    out.clone()
                };
                if is_svg {
                    let svg = vb_export::export_artboard_svg(&doc, *id, scale)
                        .map_err(CliError::Export)?;
                    std::fs::write(&out_path, &svg)
                        .with_context(|| format!("写出 {}", out_path.display()))
                        .map_err(|e| CliError::Other(format!("{e:#}")))?;
                    results.push(json!({
                        "artboard": name, "out": out_path.display().to_string(),
                        "bytes": svg.len(),
                    }));
                    continue;
                }
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
