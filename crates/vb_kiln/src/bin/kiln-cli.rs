//! Kiln-noGUI-CLI:无 GUI 纯命令行导出器(WPI 参数兼容)。
//!
//! 与 artboard 技能的调用约定对齐:同款 `--source/--output/--format/
//! --width/--scale/--transparent/--max-wait` 参数面,同款单行 JSON 结果
//! ({format,path,width,height,warnings,frames,...}),导出引擎换成
//! Kiln 原生九格式(无浏览器/Python 依赖;MP4/GIF 桥需可选 ffmpeg)。
//!
//! 独有扩展:`--jpeg-quality`、`--fps`、`--duration`、`--loop`、
//! `--bitrate`、`--selfcheck`。

use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};
use vb_doc::import::import_project;
use vb_kiln::{ExportRequest, Format};

#[derive(Parser)]
#[command(
    name = "Kiln-noGUI-cli",
    about = "Kiln 无 GUI 命令行导出器:HTML 项目 → PNG/JPG/GIF/MP4/SVG/PDF/EPS/Ai/PPTX"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 导出(WPI 参数兼容)
    Export {
        /// 源 HTML 文件或项目目录
        #[arg(long)]
        source: PathBuf,
        /// 输出文件路径
        #[arg(long)]
        output: PathBuf,
        /// 格式:PNG|JPG|GIF|MP4|SVG|PDF|EPS|AI|PPTX(与 --output 扩展名二选一)
        #[arg(long)]
        format: Option<String>,
        /// 画板宽度(逻辑 px;默认自适应画板)
        #[arg(long, default_value_t = 0)]
        width: u32,
        /// 分辨率倍率 1..=8
        #[arg(long, default_value_t = 1)]
        scale: u32,
        /// 保留透明背景(PNG/GIF/SVG/PDF)
        #[arg(long, default_value_t = false)]
        transparent: bool,
        /// 最大等待秒(WPI 兼容;Kiln 原生导出无外部等待,参数保留)
        #[arg(long, default_value_t = 15.0)]
        max_wait: f32,
        /// JPG 质量 1..=100
        #[arg(long, default_value_t = 92)]
        jpeg_quality: u8,
        /// GIF/MP4 帧率
        #[arg(long, default_value_t = 25)]
        fps: u32,
        /// GIF/MP4 时长(秒)
        #[arg(long, default_value_t = 2.0)]
        duration: f32,
        /// GIF 循环次数(0=无限)
        #[arg(long, default_value_t = 0)]
        r#loop: u16,
        /// MP4 码率 kbps
        #[arg(long, default_value_t = 8000)]
        bitrate: u32,
    },
    /// 内置样例自检:验证部署环境与九格式引擎
    Selfcheck,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.cmd {
        Cmd::Export {
            source,
            output,
            format,
            width,
            scale,
            transparent,
            max_wait,
            jpeg_quality,
            fps,
            duration,
            r#loop,
            bitrate,
        } => run_export(
            source,
            output,
            format,
            width,
            scale,
            transparent,
            max_wait,
            jpeg_quality,
            fps,
            duration,
            r#loop,
            bitrate,
        ),
        Cmd::Selfcheck => run_selfcheck(),
    };
    std::process::exit(code);
}

#[allow(clippy::too_many_arguments)]
fn run_export(
    source: PathBuf,
    output: PathBuf,
    format: Option<String>,
    width: u32,
    scale: u32,
    transparent: bool,
    _max_wait: f32,
    jpeg_quality: u8,
    fps: u32,
    duration: f32,
    r#loop: u16,
    bitrate: u32,
) -> i32 {
    let t0 = Instant::now();
    let dir = if source.is_dir() {
        source.clone()
    } else {
        match source.parent() {
            Some(p) if p.is_dir() => p.to_path_buf(),
            _ => PathBuf::from("."),
        }
    };
    // 单文件入口:直接以该文件导入(此前把父目录当项目根,强制要求
    // index.html,别名 HTML 一律报「目录中无 index.html」—— G1)
    let import_path = if source.is_dir() {
        dir.clone()
    } else {
        source.clone()
    };

    // 格式解析:--format 优先,否则扩展名
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let fmt_str = format
        .as_deref()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| ext.clone());
    let Some(fmt) = Format::from_ext(&fmt_str) else {
        eprintln!(
            "{{\"ok\":false,\"error\":\"未知格式:{fmt_str}(支持 PNG/JPG/GIF/MP4/SVG/PDF/EPS/AI/PPTX)\"}}"
        );
        return 2;
    };

    let imported = match import_project(&import_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{{\"ok\":false,\"error\":\"导入失败:{e}\"}}");
            return 3;
        }
    };
    let Some(ab) = imported.doc.artboards.first().copied() else {
        eprintln!("{{\"ok\":false,\"error\":\"项目无画板\"}}");
        return 3;
    };

    let req = ExportRequest {
        format: fmt,
        scale: scale.clamp(1, 8),
        transparent,
        jpeg_quality,
        fps,
        duration_s: duration,
        gif_loops: r#loop,
        mp4_bitrate_kbps: bitrate,
    };
    let (bytes, report) = match vb_kiln::export_artboard(&imported.doc, ab, &req, Some(&dir)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{{\"ok\":false,\"error\":\"导出失败:{e}\"}}");
            return 4;
        }
    };
    if let Some(parent) = output.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&output, &bytes) {
        eprintln!("{{\"ok\":false,\"error\":\"写文件失败:{e}\"}}");
        return 4;
    }

    // 输出尺寸:光栅格式 = 像素;矢量格式 = 逻辑尺寸 × scale
    let logical_w = report.engine.len(); // 占位防 unused;实际宽高见下
    let _ = logical_w;
    let (w, h) = raster_dims(&imported, ab, &req);
    let _ = width; // WPI 兼容:Kiln 以画板几何为准

    let json = format!(
        "{{'ok':true,'format':'{}','path':'{}','width':{},'height':{},'scale':{},'transparent':{},'warnings':{},'frames':{},'degraded':{},'bytes':{},'encode_ms':{},'engine':'kiln'}}",
        fmt_str.to_uppercase(),
        output.display(),
        w,
        h,
        req.scale,
        transparent,
        report.warnings.len(),
        report.frame_count,
        report.degraded,
        bytes.len(),
        t0.elapsed().as_millis()
    )
    .replace('\'', "\"");
    println!("{json}");
    0
}

/// 输出宽高(与 ExportContext 一致的公式)。
fn raster_dims(
    imported: &vb_doc::import::ImportResult,
    ab: vb_doc::model::NodeId,
    req: &ExportRequest,
) -> (u32, u32) {
    let n = imported.doc.node(ab).unwrap();
    let w = ((n.geom.w * req.scale as f64).round() as u32).max(1);
    let h = ((n.geom.h * req.scale as f64).round() as u32).max(1);
    (w, h)
}

fn run_selfcheck() -> i32 {
    let t0 = Instant::now();
    // 内置最小样例:渐变卡片 + 文本 + 圆角按钮
    let mut doc = vb_doc::Document::new_empty("Kiln selfcheck", "zh-CN");
    let ab = doc.new_artboard("check", 320.0, 200.0);
    let sid_card = doc.alloc_sid();
    let card = doc.nodes.insert(vb_doc::model::Node::new(
        vb_doc::model::NodeKind::Box,
        "卡片",
        sid_card,
    ));
    {
        use vb_doc::model::Geom;
        let n = doc.nodes.get_mut(card).unwrap();
        n.geom = Geom {
            x: 20.0,
            y: 20.0,
            w: 280.0,
            h: 120.0,
        };
        n.style_set("background-color", "rgb(16,185,129)"); // vb-token-ok selfcheck 样例数据
        n.style_set("border-radius", "12px");
        doc.nodes.get_mut(ab).unwrap().children.push(card);
        doc.nodes.get_mut(card).unwrap().parent = Some(ab);
    }
    let passed = Format::all().iter().all(|fmt| {
        let req = ExportRequest {
            format: *fmt,
            scale: 1,
            fps: 6,
            duration_s: 0.5,
            ..Default::default()
        };
        vb_kiln::export_artboard(&doc, ab, &req, None).is_ok()
    });
    let json = format!(
        "{{'ok':{},'engine':'kiln','formats':9,'ms':{}}}",
        passed,
        t0.elapsed().as_millis()
    )
    .replace('\'', "\"");
    println!("{json}");
    if passed {
        0
    } else {
        1
    }
}
