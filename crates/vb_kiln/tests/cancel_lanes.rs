//! 协作式取消回归(硬骨头 #6,R0):三车道入口边界 + native 分段边界。
//!
//! 浏览器在场的完整逐帧取消属于手动/下游实测范畴(依赖真浏览器与
//! ffmpeg);此处覆盖**确定性**部分:预取消令牌必须在任何外部资源
//! (浏览器进程/静态服务/ffmpeg)拉起前就把导出按取消收口,且错误
//! 可与失败三态区分。

use vb_kiln::cancel::{is_lane_cancelled, lane_cancelled, CancelToken};
use vb_kiln::context::{ExportContext, Frame};
use vb_kiln::writer::Format;
use vb_kiln::{ExportRequest, KilnError};

/// 预取消令牌:入口边界即命中,不拉起浏览器/静态服务。
#[test]
fn precancelled_anim_pipe_stops_at_entry_boundary() {
    let token = CancelToken::new();
    let opts = vb_kiln::animlane::AnimPipeOpts {
        cancel: Some(token.child()),
        ..Default::default()
    };
    token.cancel();
    let err = vb_kiln::animlane::export_anim_pipe(std::path::Path::new("no-such.html"), &opts)
        .err()
        .expect("预取消必须 Err");
    assert!(
        is_lane_cancelled(&err),
        "取消必须带标记串(区别于失败):{err}"
    );
    assert!(err.contains("入口"), "检查点标注应保留:{err}");
    // 同一令牌不取消时:同一个错误应该是普通失败(找不到源文件),不是取消
    let opts = vb_kiln::animlane::AnimPipeOpts {
        cancel: Some(CancelToken::new()),
        ..Default::default()
    };
    let err = vb_kiln::animlane::export_anim_pipe(std::path::Path::new("no-such.html"), &opts)
        .err()
        .expect("缺源文件必须 Err");
    assert!(!is_lane_cancelled(&err), "普通失败不得误标取消:{err}");
}

#[test]
fn precancelled_gif_inmemory_stops_at_entry_boundary() {
    let token = CancelToken::new();
    let opts = vb_kiln::animlane::AnimPipeOpts {
        cancel: Some(token.child()),
        ..Default::default()
    };
    token.cancel();
    let err = vb_kiln::animlane::export_anim_inmemory(
        std::path::Path::new("no-such.html"),
        Format::Gif,
        0,
        &opts,
    )
    .err()
    .expect("预取消必须 Err");
    assert!(is_lane_cancelled(&err), "{err}");
}

#[test]
fn precancelled_webcodecs_stops_at_entry_boundary() {
    let token = CancelToken::new();
    let opts = vb_kiln::animlane::AnimPipeOpts {
        cancel: Some(token.child()),
        ..Default::default()
    };
    token.cancel();
    let err =
        vb_kiln::webcodecs_lane::export_anim_webcodecs(std::path::Path::new("no-such.html"), &opts)
            .err()
            .expect("预取消必须 Err");
    assert!(is_lane_cancelled(&err), "{err}");
}

/// 取消标记串的三态区分:失败串不以取消前缀开头。
#[test]
fn lane_cancel_marker_is_triple_state_discriminable() {
    assert!(is_lane_cancelled(&lane_cancelled("实例 0 帧边界 3")));
    assert!(!is_lane_cancelled("逐帧截屏失败(帧 3):timeout"));
    assert!(!is_lane_cancelled("分段 1 渲染失败:页面已崩溃"));
}

/// native 车道:export_artboard_with_cancel 入口边界检查(建上下文之前)。
#[test]
fn precancelled_native_export_returns_cancelled_error() {
    let mut doc = vb_doc::Document::new_empty("取消样例", "zh-CN");
    let ab = doc.new_artboard("画板", 10.0, 10.0);
    let token = CancelToken::new();
    token.cancel();
    let req = ExportRequest {
        format: Format::Png,
        ..Default::default()
    };
    let err = vb_kiln::export_artboard_with_cancel(&doc, ab, &req, None, Some(token))
        .expect_err("预取消必须 Err");
    assert!(
        matches!(err, KilnError::Cancelled),
        "native 车道取消必须是强类型 Cancelled:{err:?}"
    );
    // 不取消:同文档正常导出(既有行为回归)
    let req = ExportRequest {
        format: Format::Png,
        ..Default::default()
    };
    let (_, report) =
        vb_kiln::export_artboard_with_cancel(&doc, ab, &req, None, Some(CancelToken::new()))
            .expect("有效令牌必须正常导出");
    assert!(report.instances.is_empty(), "native 车道无逐实例统计");
}

/// native 车道 GIF 写出器的分段边界检查:已取消的令牌在帧落盘/编码
/// 边界命中,返回 Cancelled(先于任何 ffmpeg 调用与产物写出)。
#[test]
fn native_gif_writer_checks_cancel_at_segment_boundary() {
    let frame = Frame {
        rgba: vec![255, 0, 0, 255],
        width: 1,
        height: 1,
        delay_ms: 100,
    };
    let ctx = ExportContext {
        artboard_name: "取消样例".into(),
        doc_title: String::new(),
        logical_w: 1.0,
        logical_h: 1.0,
        out_w: 1,
        out_h: 1,
        scale: 1,
        transparent: false,
        requested_transparent: false,
        list: vb_render::encode::DrawList {
            w: 1.0,
            h: 1.0,
            background: [1.0, 1.0, 1.0, 1.0],
            items: Vec::new(),
        },
        frames: vec![frame],
        fps: 1,
        duration_s: 1.0,
        gif_loops: 0,
        mp4_bitrate_kbps: 8000,
        jpeg_quality: 92,
        build_warnings: Vec::new(),
        anim_coverage: None,
        project_dir: None,
        cancel: Some(CancelToken::new()),
    };
    // 令牌在写出中途置位:入口已过,帧边界检查必须兜住
    if let Some(t) = &ctx.cancel {
        t.cancel();
    }
    let err = vb_kiln::frames::encode_gif(&ctx).expect_err("取消必须 Err");
    assert!(matches!(err, KilnError::Cancelled), "{err:?}");
}

/// 不可取消口径回归:cancel = None 时 GIF 写出器照常产出(既有行为不变)。
#[test]
fn native_gif_writer_without_token_still_encodes() {
    let frame = Frame {
        rgba: vec![255, 0, 0, 255],
        width: 1,
        height: 1,
        delay_ms: 100,
    };
    let ctx = ExportContext {
        artboard_name: "无令牌样例".into(),
        doc_title: String::new(),
        logical_w: 1.0,
        logical_h: 1.0,
        out_w: 1,
        out_h: 1,
        scale: 1,
        transparent: false,
        requested_transparent: false,
        list: vb_render::encode::DrawList {
            w: 1.0,
            h: 1.0,
            background: [1.0, 1.0, 1.0, 1.0],
            items: Vec::new(),
        },
        frames: vec![frame],
        fps: 1,
        duration_s: 1.0,
        gif_loops: 0,
        mp4_bitrate_kbps: 8000,
        jpeg_quality: 92,
        build_warnings: Vec::new(),
        anim_coverage: None,
        project_dir: None,
        cancel: None,
    };
    let bytes = vb_kiln::frames::encode_gif(&ctx).expect("无令牌必须正常编码");
    assert!(bytes.len() > 16, "GIF 产物过小:{len}", len = bytes.len());
}

/// worker 派生句柄传播:父取消 → 分段任务 child 立即可见(并发语义)。
#[test]
fn cancel_propagates_to_worker_children() {
    let parent = CancelToken::new();
    let workers: Vec<CancelToken> = (0..4).map(|_| parent.child()).collect();
    let stops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let handles: Vec<_> = workers
        .into_iter()
        .enumerate()
        .map(|(wi, tok)| {
            let stops = stops.clone();
            std::thread::spawn(move || {
                // 模拟分段 worker 的帧边界轮询
                for i in 0..2_000_000u64 {
                    if tok.is_cancelled() {
                        stops.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return (wi, i);
                    }
                    std::hint::spin_loop();
                }
                (wi, u64::MAX)
            })
        })
        .collect();
    parent.cancel();
    for h in handles {
        let (wi, at) = h.join().unwrap();
        assert!(at < u64::MAX, "worker {wi} 未在取消后停止");
    }
    assert_eq!(stops.load(std::sync::atomic::Ordering::Relaxed), 4);
}
