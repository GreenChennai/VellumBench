//! 车道 B 动画逐帧(WPI 理论,21 篇后续)。
//!
//! 理论(WPI capture_engine.capture_frames):动画在真浏览器里**实时播放**,
//! 按 1/fps 节奏 CDP 截屏采样(截图耗时超过间隔时自适应不等待),帧序走
//! 既有 ffmpeg 桥(palettegen GIF / libx264 MP4)。此前 GIF/MP4 走 Lane K
//! 静态逐帧求值(anim.rs 只支持 opacity/transform/clip-path/filter 四类
//! 轨道,width/stroke-dashoffset/@property 计数全部静态),用户人工评分
//! 仅 35% —— 本模块成为 GIF/MP4 的浏览器主路,Lane K 降级为无浏览器兜底。

use std::path::Path;
use std::time::{Duration, Instant};

use crate::context::{ExportContext, Frame};
use crate::writer::Format;
use vb_render::encode::DrawList;

pub struct AnimLaneResult {
    pub bytes: Vec<u8>,
    pub frames: usize,
    pub browser: String,
    pub warnings: Vec<String>,
    /// 动画覆盖矩阵(VB-3):浏览器车道全量播放,`animated` = 源中声明的
    /// 全部关键帧属性;无动画声明时为 None。
    pub anim_coverage: Option<crate::anim::AnimCoverage>,
}

/// 从 HTML 源提取 `<style>` 块内容(anim_coverage 判定用;不入 Document,
/// 只读文本,与浏览器实际播放的声明同源——内联 style 块)。
fn style_blocks_of_html(html: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string(html) else {
        return out;
    };
    let mut rest = text.as_str();
    while let Some(start) = rest.find("<style") {
        let Some(open_rel) = rest[start..].find('>') else {
            break;
        };
        let after = start + open_rel + 1;
        let Some(close) = rest[after..].find("</style>") else {
            break;
        };
        out.push(rest[after..after + close].to_string());
        rest = &rest[after + close + "</style>".len()..];
    }
    out
}

/// 单源动画导出:浏览器实时采样 → 帧序 → GIF/MP4。
#[allow(clippy::too_many_arguments)]
pub fn export_anim(
    source: &Path,
    format: Format,
    width: u32,
    height: u32,
    fps: u32,
    duration_s: f32,
    scale: u32,
    bitrate_kbps: u32,
    gif_loops: u16,
) -> Result<AnimLaneResult, String> {
    let (mount_dir, html_path) = crate::domexport::resolve_source(source)?;
    let srv = vb_browser::staticsrv::StaticServer::start(&mount_dir)?;
    let url = if source.is_dir() {
        srv.url_for_dir()?
    } else {
        let name = html_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("源文件名非法")?;
        format!(
            "http://127.0.0.1:{}/{}",
            srv.port(),
            crate::domexport::url_encode(name)
        )
    };
    let exe = vb_browser::discover_browser(None)
        .ok_or("未发现系统浏览器(Edge/Chrome);动画浏览器路线不可用")?;
    let proc = vb_browser::browser::BrowserProcess::launch(&exe)?;
    let browser = proc.version();
    let mut page = vb_browser::page::PageSession::attach(&proc)?;
    let vw = if width > 0 { width } else { 1080 };
    let vh = if height > 0 { height } else { vw };
    let dsf = scale.clamp(1, 8);
    page.set_device_metrics(vw, vh, dsf)?;
    page.navigate(&url)?;
    page.wait_network_idle(Duration::from_secs(3));
    vb_browser::capture::wait_assets(&mut page);
    page.sleep(250); // 首帧稳定(不冻结动画、不仿真 reduced-motion)

    // 实时采样:首帧立即,其后按 1/fps 节奏;截图慢于间隔则不等待(自适应)。
    // 精简逐帧路径:直接 CDP 截屏——capture_png 的完整协议(settle 视觉
    // 稳定等待 + reduced-motion 仿真 + 每帧重设 metrics)在动画页面上
    // 每帧要等 ~2.5s,且 reduced-motion 会禁掉 CSS 动画,均不可用
    let fps = fps.clamp(1, 60);
    let n = ((fps as f32 * duration_s.max(0.1)).ceil().max(1.0)) as usize;
    let interval = 1.0f64 / fps as f64;
    let t0 = Instant::now();
    let mut captures: Vec<(f64, Frame)> = Vec::with_capacity(n);
    for i in 0..n {
        if i > 0 {
            let target = t0 + Duration::from_secs_f64(i as f64 * interval);
            let now = Instant::now();
            if target > now {
                std::thread::sleep(target - now);
            }
        }
        let png = page
            .screenshot(
                "png",
                None,
                Some((0.0, 0.0, vw as f64, vh as f64)),
                false,
                false,
            )
            .map_err(|e| format!("逐帧截屏失败(帧 {i}):{e}"))?;
        let mut img = image::load_from_memory(&png)
            .map_err(|e| format!("帧解码失败(帧 {i}):{e}"))?
            .to_rgba8();
        // 白底展平(与 capture_png 非透明口径一致)
        for px in img.pixels_mut() {
            if px[3] < 255 {
                let a = px[3] as f32 / 255.0;
                for c in 0..3 {
                    px[c] = (px[c] as f32 * a + 255.0 * (1.0 - a)).round() as u8;
                }
                px[3] = 255;
            }
        }
        let (w, h) = (img.width(), img.height());
        // 记录真实采集时刻(WPI:times 供按实际节奏计算播放时长);
        // 截图慢于间隔时,墙钟时间与帧序号脱钩,必须靠它重采样还原速度
        let wall = t0
            .elapsed()
            .as_secs_f64()
            .min(duration_s as f64)
            .max(interval);
        captures.push((
            wall,
            Frame {
                rgba: img.into_raw(),
                width: w,
                height: h,
                delay_ms: (1000.0 / fps as f64).round() as u32,
            },
        ));
    }
    // 时间重采样:输出统一 fps 网格,每个输出时刻取墙钟最近的采样帧
    // (采样快于间隔 → 跳帧;慢于间隔 → 复制帧;播放速度 = 真实时间)
    captures.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut frames: Vec<Frame> = Vec::with_capacity(n);
    let mut ptr = 0usize;
    for i in 0..n {
        let t = i as f64 * interval;
        while ptr + 1 < captures.len()
            && (captures[ptr + 1].0 - t).abs() <= (captures[ptr].0 - t).abs()
        {
            ptr += 1;
        }
        let mut f = Frame {
            rgba: captures[ptr].1.rgba.clone(),
            width: captures[ptr].1.width,
            height: captures[ptr].1.height,
            delay_ms: captures[ptr].1.delay_ms,
        };
        // 输出帧延迟恒定 1/fps(CFR;GIF 也用恒定延迟,速度与真实一致)
        f.delay_ms = (1000.0 / fps as f64).round() as u32;
        frames.push(f);
    }
    page.close();
    drop(proc);
    if frames.is_empty() {
        return Err("采样得到 0 帧".into());
    }
    // 尺寸一致性守卫(ffmpeg 要求恒定帧尺寸;异常帧裁到首帧尺寸)
    let (fw, fh) = (frames[0].width, frames[0].height);
    let mut warnings: Vec<String> = Vec::new();
    for f in frames.iter_mut() {
        if f.width != fw || f.height != fh {
            warnings.push(format!(
                "帧尺寸漂移 {}x{}→{fw}x{fh},已裁齐",
                f.width, f.height
            ));
            if let Some(img) =
                image::RgbaImage::from_raw(f.width, f.height, std::mem::take(&mut f.rgba))
            {
                let cw = fw.min(f.width);
                let ch = fh.min(f.height);
                let cropped = image::imageops::crop_imm(&img, 0, 0, cw, ch).to_image();
                f.width = cropped.width();
                f.height = cropped.height();
                f.rgba = cropped.into_raw();
            }
        }
    }

    let ctx = ExportContext {
        artboard_name: String::new(),
        doc_title: String::new(),
        logical_w: vw as f64,
        logical_h: vh as f64,
        out_w: fw,
        out_h: fh,
        scale: dsf,
        transparent: false,
        requested_transparent: false,
        list: DrawList {
            w: vw as f64,
            h: vh as f64,
            background: [1.0, 1.0, 1.0, 1.0],
            items: Vec::new(),
        },
        frames,
        fps,
        duration_s,
        gif_loops,
        mp4_bitrate_kbps: bitrate_kbps,
        jpeg_quality: 92,
        build_warnings: Vec::new(),
        anim_coverage: None,
        project_dir: None,
    };
    let bytes = match format {
        Format::Mp4 => {
            if !crate::frames::ffmpeg_available() {
                warnings.push("ffmpeg 缺失,MP4 降级 GIF 流".into());
            }
            crate::frames::encode_mp4(&ctx).map_err(|e| e.to_string())?
        }
        _ => crate::frames::encode_gif(&ctx).map_err(|e| e.to_string())?,
    };
    // VB-1:静态资源 404 不许静默;VB-3:覆盖矩阵随结果输出
    warnings.extend(
        srv.take_not_found()
            .iter()
            .map(|s| vb_browser::staticsrv::asset_not_found_message(s)),
    );
    let anim_coverage = crate::anim::AnimCoverage::from_keyframes(
        &crate::anim::parse_keyframes(&style_blocks_of_html(&html_path)),
        "browser",
    );
    Ok(AnimLaneResult {
        bytes,
        frames: n,
        browser,
        warnings,
        anim_coverage,
    })
}
