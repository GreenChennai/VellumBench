//! Kiln CLI:导出与基准入口(kiln export / kiln bench)。
//!
//! `kiln export` 把 HTML 项目导入 vb_doc 后按九格式导出(引擎热路径
//! 与 GUI 完全一致);`kiln bench` 输出计时 JSON 供对比报告消费。

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

use vb_doc::import::import_project;
use vb_kiln::{ExportRequest, Format};

#[derive(Parser)]
#[command(name = "kiln", about = "Kiln - VellumBench 导出核心(九格式)")]
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
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Export { source, output, scale, transparent, fps, duration } => {
            let dir = if source.is_dir() {
                source.clone()
            } else {
                source.parent().unwrap_or(&source).to_path_buf()
            };
            let imported = import_project(&dir).expect("导入失败");
            let ab = *imported.doc.artboards.first().expect("无画板");
            let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("png");
            let fmt = Format::from_ext(ext).unwrap_or_else(|| panic!("未知扩展名:{ext}"));
            let req = ExportRequest {
                format: fmt,
                scale: scale.clamp(1, 8),
                transparent,
                fps,
                duration_s: duration,
                ..Default::default()
            };
            let t = Instant::now();
            let report =
                vb_kiln::export_artboard_to_file(&imported.doc, ab, &req, Some(&dir), &output)
                    .expect("导出失败");
            println!(
                "{fmt:?} -> {} ({}, encode {}ms)",
                output.display(),
                report.summary(),
                t.elapsed().as_millis()
            );
        }
        Cmd::Bench { source, out_dir, scale } => {
            std::fs::create_dir_all(&out_dir).expect("建目录失败");
            let dir = if source.is_dir() {
                source.clone()
            } else {
                source.parent().unwrap_or(&source).to_path_buf()
            };
            let imported = import_project(&dir).expect("导入失败");
            let ab = *imported.doc.artboards.first().expect("无画板");
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
                let report = vb_kiln::export_artboard_to_file(
                    &imported.doc, ab, &req, Some(&dir), &out,
                )
                .unwrap_or_else(|e| panic!("{fmt:?} 失败:{e}"));
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
            std::fs::write(out_dir.join("kiln-bench.json"), json).expect("写 JSON 失败");
            println!("bench -> {}/kiln-bench.json", out_dir.display());
        }
    }
}
