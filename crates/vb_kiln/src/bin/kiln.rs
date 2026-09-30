//! Kiln CLI:导出与基准入口(kiln export / kiln bench)。
//!
//! `kiln export` 把 HTML 项目导入 vb_doc 后按九格式导出(引擎热路径
//! 与 GUI 完全一致);`kiln bench` 输出计时 JSON 供对比报告消费。
//!
//! 错误契约(与 kiln-cli 一致):所有错误路径向 stderr 输出单行
//! `{"ok":false,"error":"…"}`,退出码:0 = 成功/--help/--version,
//! 2 = 参数用法错,3 = 输入文件/格式错,4 = IO/内部错。
//! 不用 panic/expect——exit 101 + backtrace 对脚本调用方不可判定。

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

use vb_doc::import::import_project;
use vb_kiln::{ExportRequest, Format};

const EXIT_OK: i32 = 0;
const EXIT_USAGE: i32 = 2;
const EXIT_INPUT: i32 = 3;
const EXIT_IO: i32 = 4;

/// 最小 JSON 字符串转义(与 kiln-cli 的 `jesc` 同逻辑)。
///
/// 错误信息含 Windows 路径(`\`)或引号时直接插入会产出非法 JSON,
/// 调用方解析不到 `ok`/`error`;换行也转义,保证错误恒为单行。
/// kiln.rs 是独立 bin,在此各持一份,不跨 bin 引用。
fn jesc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\u0027"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// stderr 单行 JSON 错误 + 对应退出码(所有错误出口唯一经此)。
fn fail(code: i32, msg: &str) -> i32 {
    eprintln!("{{\"ok\":false,\"error\":\"{}\"}}", jesc(msg));
    code
}

#[derive(Parser)]
#[command(name = "kiln", version, about = "Kiln - VellumBench 导出核心(九格式)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 导入 HTML 项目并导出指定格式
    Export {
        /// 项目目录(index.html)或 HTML 文件
        #[arg(long)]
        source: PathBuf,
        /// 输出文件(扩展名决定格式:png/jpg/gif/mp4/svg/pdf/eps/ai/pptx)
        #[arg(long)]
        output: PathBuf,
        /// 倍率 1..=8
        #[arg(long, default_value_t = 2)]
        scale: u32,
        #[arg(long, default_value_t = false)]
        transparent: bool,
        /// GIF/MP4 帧率
        #[arg(long, default_value_t = 30)]
        fps: u32,
        /// GIF/MP4 时长(秒)
        #[arg(long, default_value_t = 2.0)]
        duration: f32,
    },
    /// 基准:对样例逐格式计时,输出 JSON
    Bench {
        #[arg(long)]
        source: PathBuf,
        /// 输出目录(九格式全出)
        #[arg(long)]
        out_dir: PathBuf,
        #[arg(long, default_value_t = 2)]
        scale: u32,
    },
}

fn main() {
    std::process::exit(real_main());
}

fn real_main() -> i32 {
    // clap 默认对参数错误自带退出(2)并打印多行人类可读文本;拦下来
    // 统一走单行 JSON 契约。例外:--help/--version 是正常求助,
    // 帮助文本走 stdout、退出 0 —— 脚本用 $? 区分「看帮助」与「出错」。
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let help_requested = matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            );
            if help_requested {
                let _ = e.print();
                return EXIT_OK;
            }
            return fail(
                EXIT_USAGE,
                &format!("参数错误:{}", flatten_ws(&e.to_string())),
            );
        }
    };
    match cli.cmd {
        Cmd::Export {
            source,
            output,
            scale,
            transparent,
            fps,
            duration,
        } => {
            let dir = if source.is_dir() {
                source.clone()
            } else {
                source.parent().unwrap_or(&source).to_path_buf()
            };
            let imported = match import_project(&dir) {
                Ok(r) => r,
                Err(e) => return fail(EXIT_INPUT, &format!("导入失败:{e}")),
            };
            let Some(ab) = imported.doc.artboards.first().copied() else {
                return fail(EXIT_INPUT, "项目无画板");
            };
            let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("png");
            let Some(fmt) = Format::from_ext(ext) else {
                return fail(
                    EXIT_INPUT,
                    &format!("未知扩展名:{ext}(支持 png/jpg/gif/mp4/svg/pdf/eps/ai/pptx)"),
                );
            };
            let req = ExportRequest {
                format: fmt,
                scale: scale.clamp(1, 8),
                transparent,
                fps,
                duration_s: duration,
                ..Default::default()
            };
            let t = Instant::now();
            let report = match vb_kiln::export_artboard_to_file(
                &imported.doc,
                ab,
                &req,
                Some(&dir),
                &output,
            ) {
                Ok(r) => r,
                Err(e) => return fail(EXIT_IO, &format!("导出失败:{e}")),
            };
            println!(
                "{fmt:?} -> {} ({}, encode {}ms)",
                output.display(),
                report.summary(),
                t.elapsed().as_millis()
            );
            EXIT_OK
        }
        Cmd::Bench {
            source,
            out_dir,
            scale,
        } => {
            if let Err(e) = std::fs::create_dir_all(&out_dir) {
                return fail(EXIT_IO, &format!("建目录失败 {}:{e}", out_dir.display()));
            }
            let dir = if source.is_dir() {
                source.clone()
            } else {
                source.parent().unwrap_or(&source).to_path_buf()
            };
            let imported = match import_project(&dir) {
                Ok(r) => r,
                Err(e) => return fail(EXIT_INPUT, &format!("导入失败:{e}")),
            };
            let Some(ab) = imported.doc.artboards.first().copied() else {
                return fail(EXIT_INPUT, "项目无画板");
            };
            let mut rows = Vec::new();
            for fmt in Format::all() {
                let req = ExportRequest {
                    format: fmt,
                    scale: scale.clamp(1, 8),
                    fps: 6,
                    duration_s: 0.5,
                    ..Default::default()
                };
                let out = out_dir.join(format!("sample.{}", fmt.ext()));
                let t = Instant::now();
                let report = match vb_kiln::export_artboard_to_file(
                    &imported.doc,
                    ab,
                    &req,
                    Some(&dir),
                    &out,
                ) {
                    Ok(r) => r,
                    Err(e) => return fail(EXIT_IO, &format!("{fmt:?} 失败:{e}")),
                };
                let ms = t.elapsed().as_millis() as u64;
                println!("{fmt:?} {ms}ms {} bytes", report.bytes);
                rows.push(format!(
                    r#"{{"format":"{}","ms":{},"bytes":{},"frames":{},"warnings":{}}}"#,
                    fmt.ext(),
                    ms,
                    report.bytes,
                    report.frame_count,
                    report.warnings.len()
                ));
            }
            let json = format!("[{}]", rows.join(","));
            let json_path = out_dir.join("kiln-bench.json");
            if let Err(e) = std::fs::write(&json_path, json) {
                return fail(
                    EXIT_IO,
                    &format!("写 JSON 失败 {}:{e}", json_path.display()),
                );
            }
            println!("bench -> {}/kiln-bench.json", out_dir.display());
            EXIT_OK
        }
    }
}

/// clap 错误文本(自带 usage 的多行排版)先折叠连续空白成一行,
/// 再交由 `fail`/`jesc` 转义——错误 JSON 契约是单行,折叠后人也能读。
fn flatten_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    out.trim().to_string()
}
