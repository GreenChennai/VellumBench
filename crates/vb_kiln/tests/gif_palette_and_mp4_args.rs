//! UP-4 回归:GIF 调色板路径必须**可观测**。
//!
//! 原缺陷:两条调色板路径(ffmpeg `palettegen+paletteuse` 两 pass vs 纯 Rust
//! 感知量化 LZW 单 pass)质量不同,但门控只是 `if ffmpeg_available()`,报告里
//! **完全看不出走了哪条**;ffmpeg 桥失败还被 `let _ = e;` 静默吞掉。
//!
//! MP4 侧的 GOP / B 帧 / 色彩标签测试在 `animlane.rs` 的单元测试里
//! (`push_gop_args` 是 `pub(crate)`,集成测试访问不到)。

use vb_kiln::context::ExportContext;
use vb_kiln::error::KilnWarning;
use vb_kiln::frames::{encode_gif_reporting, GifPalettePath};
use vb_kiln::writer::Format;

fn ctx_with_frames() -> ExportContext {
    // 4 帧 2x2 红,足以让两条路径都产出合法 GIF
    let rgba_red = vec![255u8, 0, 0, 255].repeat(4);
    let frames = (0..4)
        .map(|_| vb_kiln::context::Frame {
            width: 2,
            height: 2,
            rgba: rgba_red.clone(),
            delay_ms: 100,
        })
        .collect::<Vec<_>>();
    ExportContext {
        artboard_name: String::new(),
        doc_title: String::new(),
        logical_w: 2.0,
        logical_h: 2.0,
        out_w: 2,
        out_h: 2,
        scale: 1,
        transparent: false,
        requested_transparent: false,
        list: vb_render::encode::DrawList {
            w: 2.0,
            h: 2.0,
            background: [1.0, 1.0, 1.0, 1.0],
            items: Vec::new(),
        },
        frames,
        fps: 10,
        duration_s: 0.4,
        gif_loops: 0,
        mp4_bitrate_kbps: 8000,
        mp4_gop: None,
        mp4_b_frames: None,
        mp4_color_tags: true,
        mp4_pix_fmt: None,
        jpeg_quality: 92,
        build_warnings: Vec::new(),
        anim_coverage: None,
        project_dir: None,
        cancel: None,
    }
}

/// 无论走了哪条路径,结果**必须**带声明,且声明与实际路径一致。
#[test]
fn gif_encode_always_declares_palette_path() {
    let ctx = ctx_with_frames();
    let out = encode_gif_reporting(&ctx).expect("GIF 编码应成功(两条路径都产出合法 GIF89a)");

    assert!(
        !out.path.as_str().is_empty(),
        "GifPalettePath 必须有可读文本"
    );

    let w = out.warning();
    assert_eq!(
        w.kind(),
        "gif_palette_path",
        "必须有稳定的 kind 键供下游门禁聚合"
    );
    // 声明与实际路径一致:via_ffmpeg ⟺ FfmpegTwoPass
    match (&w, out.path) {
        (KilnWarning::GifPalettePath { via_ffmpeg, .. }, GifPalettePath::FfmpegTwoPass) => {
            assert!(*via_ffmpeg, "走了两 pass 就必须声明 via_ffmpeg=true")
        }
        (KilnWarning::GifPalettePath { via_ffmpeg, .. }, GifPalettePath::RustSinglePass) => {
            assert!(*via_ffmpeg == false, "走了单 pass 就必须声明 via_ffmpeg=false")
        }
        // 声明与路径来自同一个 outcome,不应出现第四种组合;真出现了要能看出来
        (other, path) => panic!("声明 {:?} 与路径 {:?} 不匹配", other, path),
    }

    // 非降级:两条路径内容一致,只是质量档位不同 —— 不能置 degraded
    assert!(
        !w.is_degrading(),
        "调色板路径差异不是语义降级(输出仍是合法 GIF、内容一致),不应进 is_degrading"
    );

    // 消息必须说清是哪条 + 纯 Rust 时要给出可行动建议
    let msg = w.message();
    assert!(msg.contains("调色板"), "消息应点明是调色板路径: {msg}");
    if out.path == GifPalettePath::RustSinglePass {
        assert!(
            msg.contains("ffmpeg"),
            "单 pass 时消息应提到 ffmpeg(可行动建议): {msg}"
        );
    }
}

/// 回退时必须**带上失败原因** —— 否则「本机 ffmpeg 坏了」会被当成
/// 「没装 ffmpeg」,排错方向完全不同。
#[test]
fn gif_palette_path_carries_ffmpeg_error_on_fallback() {
    let w = KilnWarning::GifPalettePath {
        via_ffmpeg: false,
        ffmpeg_error: Some("boom: 参数错误".into()),
    };
    let msg = w.message();
    assert!(msg.contains("回退"), "应说明是回退: {msg}");
    assert!(msg.contains("boom"), "应带上原始失败原因: {msg}");
}

/// `Format::Gif` 的写出器必须把声明带进 report(native 车道出口)。
#[test]
fn gif_writer_report_declares_palette_path() {
    use vb_kiln::writer::FormatWriter;
    let ctx = ctx_with_frames();
    let mut out = Vec::new();
    let report = vb_kiln::formats::gif::GifWriter
        .write(&ctx, &mut out)
        .expect("GIF 写出应成功");
    assert!(
        report.warnings_by_kind().contains_key("gif_palette_path"),
        "native 车道 report 必须含 gif_palette_path 声明,实际: {:?}",
        report.warnings_by_kind().keys().collect::<Vec<_>>()
    );
    assert_eq!(Format::Gif, vb_kiln::formats::gif::GifWriter.format());
}

/// MP4 写出器在**降级成 GIF 流**时也要带调色板声明(产物是 GIF 写进 .mp4,
/// 调色板质量同源,同样不能默默用单 pass)。
#[test]
fn mp4_writer_declares_palette_path_when_downgraded() {
    use vb_kiln::writer::FormatWriter;
    let ctx = ctx_with_frames();
    let mut out = Vec::new();
    // 不断言成功/失败(取决于本机有没有 ffmpeg 与可用编码器),只断言:
    // 走到降级分支时,report 里必须出现 gif_palette_path
    if let Ok(report) = vb_kiln::formats::mp4::Mp4Writer.write(&ctx, &mut out) {
        if report.warnings_by_kind().contains_key("mp4_downgraded_to_gif") {
            assert!(
                report.warnings_by_kind().contains_key("gif_palette_path"),
                "降级成 GIF 流时必须同时声明调色板路径"
            );
        }
    }
}
