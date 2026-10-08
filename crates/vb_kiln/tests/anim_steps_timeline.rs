//! UP-3 回归:`steps()` 时序函数在浏览器 GIF 车道输出错帧。
//!
//! 原始症状:同页同参数,MP4 车道正确、GIF 车道 40 帧全停在 `from` 色。
//! 根因是**产帧通道**而不是时钟算法 —— 浏览器无条件带
//! `--enable-begin-frame-control`,合成器被 BeginFrame 门控不发 BeginFrame
//! 就不产生新表面,`Page.captureScreenshot` 拿到的永远是首帧。
//!
//! 这里覆盖两件事:
//! 1. `frame_time_ms` 的**末帧触达片长**(纯函数,无需浏览器);
//! 2. 若环境有浏览器,真跑一次 GIF 车道,断言末段帧已跳到 `to` 色。
//!
//! 第 2 条需要系统 Edge/Chrome,缺浏览器时 `return`(与 `size_gate_browser.rs`
//! 的 opt-in 惯例一致),不把环境缺失误报成回归。

use vb_kiln::animlane::frame_time_ms;

/// 末帧必须触达 `t == duration`,否则「动画时长 == 片长」的 `forwards` 动画
/// 终值态一次都采不到 —— `steps()` 下就是全片停在 `from` 色。
///
/// 旧实现的网格是 `i/fps`,最大值 `(n-1)/fps` 恒小于片长。
#[test]
fn last_frame_lands_exactly_on_duration() {
    // 复现页口径:fps=10,duration=4s,40 帧
    let fps = 10u32;
    let duration = 4.0f32;
    let n = ((fps as f32 * duration).ceil().max(1.0)) as usize;
    assert_eq!(n, 40);

    // 网格帧不动(避免改变既有时序)
    assert_eq!(frame_time_ms(0, fps, n, duration), 0);
    assert_eq!(frame_time_ms(1, fps, n, duration), 100);
    assert_eq!(frame_time_ms(20, fps, n, duration), 2000);
    assert_eq!(frame_time_ms(39 - 1, fps, n, duration), 3800);

    // 末帧 = 片长终点 4000ms,而**不是** (n-1)/fps = 3900ms
    assert_eq!(
        frame_time_ms(39, fps, n, duration),
        4000,
        "末帧必须落在 duration 终点,否则 forwards/steps 动画的终值态永远采不到"
    );

    // 单帧(只有一帧时它就是末帧)
    assert_eq!(frame_time_ms(0, fps, 1, 1.5), 1500);
}

/// 「动画时长 == 片长」这一最危险组合:末帧必须命中动画终点。
/// 旧实现下末帧 = (n-1)/fps = 1.9s < 2s,`steps(1,end)` 全片停在 from 色。
#[test]
fn duration_equals_anim_length_hits_terminal_state() {
    let fps = 10u32;
    let duration = 2.0f32;
    let n = ((fps as f32 * duration).ceil().max(1.0)) as usize;
    assert_eq!(n, 20);
    // 动画 `jump 2s` 的终点就是 2000ms —— 末帧必须正好落在它上面,
    // steps(1,end) 才会输出 to 值(而不是 progress 恒 0 的 from 值)
    assert_eq!(frame_time_ms(19, fps, n, duration), 2000);
    // 倒数第二帧仍在网格上,不被拽到终点
    assert_eq!(frame_time_ms(18, fps, n, duration), 1800);
}

/// 时刻必须单调不减(否则 seek 状态会来回跳,反而制造抖动)。
#[test]
fn frame_times_are_monotonic() {
    let fps = 25u32;
    let duration = 6.0f32;
    let n = ((fps as f32 * duration).ceil().max(1.0)) as usize;
    let mut prev = 0u64;
    for i in 0..n {
        let t = frame_time_ms(i, fps, n, duration);
        assert!(t >= prev, "帧 {i} 的时刻 {t} 小于前一帧 {prev}");
        assert!(t <= 6000, "帧 {i} 的时刻 {t} 超过片长");
        prev = t;
    }
}

/// 真实浏览器车道回归:用 UP-3 的原始复现页跑一次 GIF,断言末段帧已跳到 to 色。
///
/// 需要系统 Edge/Chrome;缺则跳过(不把环境缺失误报成回归)。
#[test]
fn browser_gif_lane_renders_steps_animation() {
    if vb_browser::discover_browser(None).is_none() {
        eprintln!("跳过:未发现系统浏览器(UP-3 像素回归需要 Edge/Chrome)");
        return;
    }

    let dir = std::env::temp_dir().join(format!("kiln-up3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建临时目录");
    let html = dir.join("steps.html");
    std::fs::write(
        &html,
        r#"<!doctype html><meta charset="utf-8">
<style>
html,body{margin:0;padding:0;background:#fff}
.a{position:absolute;left:0;top:0;width:200px;height:400px;background:#f00;
   animation:jump 2s steps(1,end) forwards}
@keyframes jump{from{background:#f00}to{background:#00f}}
</style>
<div class="a"></div>
"#,
    )
    .expect("写复现页");

    let opts = vb_kiln::animlane::AnimPipeOpts {
        width: 200,
        height: 400,
        fps: 10,
        duration_s: 4.0,
        scale: 1,
        ..Default::default()
    };

    let out = vb_kiln::animlane::export_anim_inmemory(
        &html,
        vb_kiln::writer::Format::Gif,
        0,
        &opts,
    )
    .expect("GIF 车道应成功");

    // 断言「走了哪条调色板路径」不再是 unknown(UP-4)
    assert!(
        out.encoder_used.is_some(),
        "UP-4:GIF 车道必须声明实际调色板路径,不能是 unknown"
    );

    // 解码 GIF,读第 0 帧与末帧中心像素。
    // `into_frames` 是 `AnimationDecoder` trait 的方法(不是 inherent 方法),
    // 所以必须 import 该 trait;它会把每帧的局部调色板/帧矩形归一化,
    // 省掉一堆与本断言无关的细节。
    use image::AnimationDecoder;
    let mut frames: Vec<(u8, u8, u8)> = Vec::new();
    {
        let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&out.bytes))
            .expect("GIF 应可解码");
        for f in dec.into_frames() {
            let f = f.expect("帧应可解码");
            let img = f.into_buffer();
            // .a 覆盖整个 200x400 画布,取中心避开任何边框
            let p = img.get_pixel(100, 200);
            frames.push((p[0], p[1], p[2]));
        }
    }
    assert!(frames.len() >= 30, "应至少采到 30 帧,实际 {}", frames.len());

    let first = frames[0];
    assert_eq!(
        first,
        (255, 0, 0),
        "首帧应是 from 色 #f00,实际 {first:?}"
    );

    // UP-3 的核心断言:第 20 帧(2000ms = 动画终点)之后必须是 to 色 #00f。
    // 旧实现下这里全停在 (255,0,0)。
    let last = frames[frames.len() - 1];
    assert_eq!(
        last, (0, 0, 255),
        "UP-3 未修复:末帧应已跳到 to 色 #00f,实际 {last:?} —— \
         GIF 车道拿到的仍是首帧(begin-frame-control 下 captureScreenshot 产不出新表面)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
