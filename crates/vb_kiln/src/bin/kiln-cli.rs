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
    /// 导入外部 PDF/AI/SVG → 规范化 HTML 项目(M4 反向能力)
    Import {
        /// 源文件(pdf/ai)
        #[arg(long)]
        source: PathBuf,
        /// 输出项目目录(写入 index.html + styles/main.css + assets/)
        #[arg(long)]
        output: PathBuf,
    },
    /// 位图工具箱:crop/stitch/blur/pad/info(M4.5 系列物料后处理)
    Img {
        #[command(subcommand)]
        op: ImgOp,
    },
    /// 内置样例自检:验证部署环境与九格式引擎
    Selfcheck,
}

#[derive(Subcommand)]
enum ImgOp {
    /// 裁剪(--box l,t,r,b 像素;或 --trim #bg 自动去边)
    Crop {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long = "box", group = "crop_mode")]
        box_spec: Option<String>,
        #[arg(long, group = "crop_mode")]
        trim: Option<String>,
    },
    /// 拼接(垂直/水平,--inputs 逗号分隔)
    Stitch {
        #[arg(long, value_delimiter = ',')]
        inputs: Vec<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "vertical")]
        direction: String,
        #[arg(long, default_value_t = 0)]
        gap: u32,
        #[arg(long, default_value = "#ffffff")]
        bg: String,
        #[arg(long, default_value = "start")]
        align: String,
    },
    /// 高斯模糊(整图或区域)
    Blur {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 8.0)]
        radius: f32,
        #[arg(long = "box")]
        box_spec: Option<String>,
    },
    /// 画布填充(把图像放到指定尺寸画布上)
    Pad {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        width: u32,
        #[arg(long)]
        height: u32,
        #[arg(long, default_value = "#ffffff")]
        bg: String,
        #[arg(long, default_value = "center")]
        align: String,
    },
    /// 图像信息(宽×高)
    Info {
        #[arg(long)]
        input: PathBuf,
    },
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
        Cmd::Import { source, output } => run_import(source, output),
        Cmd::Img { op } => run_img(op),
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

    let mut imported = match import_project(&import_path) {
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

    // 项目 webfont(@font-face)注册:家庭+字重 → 字体文件。
    // woff2(压缩容器,ttf 解析器不识别)自动尝试同名 .ttf/.otf——
    // GEO 存量项目 @font-face 全为 woff2,不回退则中文整篇走系统兜底
    vb_render::text::clear_font_registry();
    for f in &imported.font_faces {
        let path = dir.join(&f.src);
        let is_woff2 = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("woff2") || e.eq_ignore_ascii_case("woff"))
            .unwrap_or(false);
        if is_woff2 {
            let mut registered = false;
            for ext in ["ttf", "otf"] {
                let cand = path.with_extension(ext);
                if cand.is_file() {
                    vb_render::text::register_font_file(&f.family, f.weight, cand);
                    registered = true;
                    break;
                }
            }
            if registered {
                continue;
            }
        }
        vb_render::text::register_font_file(&f.family, f.weight, path);
    }

    // 文档流布局求值(M1.0):flow/flex/absolute → 具体矩形写回 geom;
    // 矢量模式文档(全显式定位)求值结果与作者输入一致,无副作用
    let synthetic = imported.synthetic_artboard;
    let mut degraded_artboard = false;
    for w in vb_layout::apply_to_doc(&mut imported.doc, ab, Some(&dir), synthetic) {
        // 画板尺寸回填 / 裁剪 / grid 降级 = 合成画板与作者声明可能不一致,
        // 输出结构化标记供脚本判定(此前静默 ok:true,存量项目失真无告警)
        if w.contains("尺寸回填") || w.contains("裁剪") || w.contains("grid") {
            degraded_artboard = true;
        }
        eprintln!("{{\"warn\":\"{w}\"}}");
    }

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
        "{{'ok':true,'format':'{}','path':'{}','width':{},'height':{},'scale':{},'transparent':{},'warnings':{},'frames':{},'degraded':{},'degraded_artboard':{},'bytes':{},'encode_ms':{},'engine':'kiln'}}",
        fmt_str.to_uppercase(),
        output.display(),
        w,
        h,
        req.scale,
        transparent,
        report.warnings.len(),
        report.frame_count,
        report.degraded,
        degraded_artboard,
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

fn run_import(source: PathBuf, output: PathBuf) -> i32 {
    let ext = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let assets = output.join("assets");
    let result = match ext.as_str() {
        "pdf" | "ai" => vb_kiln::import_pdf::import_pdf_to_doc(&source, Some(&assets)),
        "svg" => vb_kiln::import_svg::import_svg_to_doc(&source, Some(&assets)),
        other => {
            eprintln!("{{\"ok\":false,\"error\":\"import 不支持 .{other}(支持 pdf/ai/svg)\"}}");
            return 2;
        }
    };
    match result {
        Ok((mut doc, mut warnings)) => {
            // 布局求值:导入文档全为绝对定位,此调用保持几何并回填画板尺寸
            let abs: Vec<vb_doc::model::NodeId> = doc.artboards.clone();
            for &ab in &abs {
                warnings.extend(vb_layout::apply_to_doc(&mut doc, ab, Some(&output), true));
            }
            match vb_doc::export::write_project(&doc, &output) {
                Ok(files) => {
                    let list: Vec<String> = files
                        .iter()
                        .map(|p| p.display().to_string().replace('\\', "/"))
                        .collect();
                    println!(
                        "{{\"ok\":true,\"files\":[\"{}\"],\"count\":{}}}",
                        list.join("\",\""),
                        list.len()
                    );
                    0
                }
                Err(e) => {
                    eprintln!("{{\"ok\":false,\"error\":\"HTML 写出失败:{e}\"}}");
                    4
                }
            }
        }
        Err(e) => {
            eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
            3
        }
    }
}

fn parse_box(s: &str) -> Result<(u32, u32, u32, u32), String> {
    let nums: Vec<u32> = s
        .split(',')
        .map(|v| {
            v.trim()
                .parse::<u32>()
                .map_err(|e| format!("坐标解析失败: {e}"))
        })
        .collect::<Result<_, _>>()?;
    if nums.len() != 4 {
        return Err(format!("--box 需要 4 个值 l,t,r,b: got {}", nums.len()));
    }
    Ok((nums[0], nums[1], nums[2], nums[3]))
}

fn run_img(op: ImgOp) -> i32 {
    use vb_kiln::img;
    match op {
        ImgOp::Crop {
            input,
            output,
            box_spec,
            trim,
        } => {
            let box_v = box_spec.as_deref().map(parse_box).transpose();
            let trim_v = trim.as_deref().map(img::parse_hex_color).transpose();
            match (box_v, trim_v) {
                (Ok(b), Ok(t)) => match img::crop(&input, &output, b, t) {
                    Ok(img) => {
                        println!(
                            "{{\"ok\":true,\"width\":{},\"height\":{}}}",
                            img.width(),
                            img.height()
                        );
                        0
                    }
                    Err(e) => {
                        eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                        1
                    }
                },
                (Err(e), _) | (_, Err(e)) => {
                    eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                    2
                }
            }
        }
        ImgOp::Stitch {
            inputs,
            output,
            direction,
            gap,
            bg,
            align,
        } => {
            let color = match img::parse_hex_color(&bg) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                    return 2;
                }
            };
            let vertical = direction == "vertical";
            match img::stitch(&inputs, &output, vertical, gap, color, &align) {
                Ok(img) => {
                    println!(
                        "{{\"ok\":true,\"width\":{},\"height\":{}}}",
                        img.width(),
                        img.height()
                    );
                    0
                }
                Err(e) => {
                    eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                    1
                }
            }
        }
        ImgOp::Blur {
            input,
            output,
            radius,
            box_spec,
        } => {
            let box_v = box_spec.as_deref().map(parse_box).transpose();
            match box_v {
                Ok(b) => match img::blur(&input, &output, radius, b) {
                    Ok(()) => {
                        println!("{{\"ok\":true}}");
                        0
                    }
                    Err(e) => {
                        eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                        1
                    }
                },
                Err(e) => {
                    eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                    2
                }
            }
        }
        ImgOp::Pad {
            input,
            output,
            width,
            height,
            bg,
            align,
        } => {
            let color = match img::parse_hex_color(&bg) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                    return 2;
                }
            };
            match img::pad(&input, &output, width, height, color, &align) {
                Ok(()) => {
                    println!("{{\"ok\":true,\"width\":{width},\"height\":{height}}}");
                    0
                }
                Err(e) => {
                    eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                    1
                }
            }
        }
        ImgOp::Info { input } => match img::info(&input) {
            Ok((w, h)) => {
                println!("{{\"ok\":true,\"width\":{w},\"height\":{h}}}");
                0
            }
            Err(e) => {
                eprintln!("{{\"ok\":false,\"error\":\"{e}\"}}");
                1
            }
        },
    }
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
