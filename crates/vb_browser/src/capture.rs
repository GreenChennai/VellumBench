//! WPI 捕获协议:settle 收敛 + 整页 PNG(单拍优先/分块拼接/白底展平)。

use std::time::{Duration, Instant};

use serde_json::Value;

use crate::page::{
    PageSession, ARTBOARD_RECT_JS, BODY_MARGIN_RESET_JS, CONTENT_SIZE_JS, FREEZE_ANIMATIONS_JS,
    RAF_THROTTLE_JS, WAIT_ASSETS_JS,
};

/// 捕获参数(与 WPI CLI 参数面一致)。
#[derive(Debug, Clone)]
pub struct CaptureOptions {
    /// 视口宽(CSS px)= 导出宽。
    pub width: u32,
    /// 高度锁定(None = 整页)。
    pub height_lock: Option<u32>,
    /// 原生倍率 deviceScaleFactor 1..=8。
    pub scale: u32,
    /// 保留透明背景。
    pub transparent: bool,
    /// 画板取景(P0-3):重置 body 页边距,PNG 按画板矩形裁剪,
    /// 尺寸 = 声明画板尺寸(高度不因内容收缩,尺寸门要求严格相等)。
    pub artboard: bool,
    /// 取第几个画板(0 起;03-4 浏览器校对用,默认 0 = 旧行为)。
    pub artboard_index: usize,
}

pub struct CaptureOutcome {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
    /// 定格时仍在跑的无限动画数(诊断用)。
    pub infinite_animations: u32,
}

// --------------------------------------------------------------- settle
/// 资源等待(要素 5):fonts.ready + 懒加载转 eager + img.complete。
pub fn wait_assets(page: &mut PageSession) {
    let _ = page.evaluate(WAIT_ASSETS_JS, true);
    page.sleep(200);
}

/// 滚动触发 reveal(要素 6):Python 侧驱动,≤40 步。
pub fn trigger_scroll_reveals(page: &mut PageSession) {
    let _ = page.evaluate(
        "() => { document.documentElement.style.scrollBehavior='auto'; return true; }",
        false,
    );
    let Ok(vh) = page.inner_height() else { return };
    let step = (vh as f64 * 0.85).max(150.0) as u32;
    let Ok(total) = page.content_size() else {
        return;
    };
    let total = total.1;
    let mut steps = 0u32;
    let mut y = 0u32;
    while y <= total {
        steps += 1;
        if steps > 40 {
            break;
        }
        page.scroll_to(y);
        page.sleep(130);
        y += step;
    }
    page.scroll_to(0);
    page.sleep(130);
}

/// 有限动画 finish 定格(要素 7);返回无限动画数。
pub fn freeze_animations(page: &mut PageSession) -> u32 {
    page.evaluate(FREEZE_ANIMATIONS_JS, false)
        .ok()
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32
}

/// 128×128 缩略哈希视觉稳定兜底(要素 8;预算 6s)。
pub fn wait_visual_stability(page: &mut PageSession) -> Result<bool, String> {
    page.sleep(400);
    let Some(mut prev) = fast_hash(page) else {
        return Ok(false);
    };
    let mut stable = 0;
    let deadline = Instant::now() + Duration::from_secs(6);
    while Instant::now() < deadline {
        page.sleep(200);
        page.ensure_alive()?;
        let cur = fast_hash(page);
        match cur {
            Some(h) if h == prev => {
                stable += 1;
                if stable >= 3 {
                    return Ok(true);
                }
            }
            Some(h) => {
                stable = 0;
                prev = h;
            }
            None => return Ok(false),
        }
    }
    Ok(false)
}

fn fast_hash(page: &mut PageSession) -> Option<[u8; 32]> {
    let (w, h) = page.content_size().ok()?;
    let data = page
        .screenshot(
            "jpeg",
            Some(50),
            Some((0.0, 0.0, w as f64, h as f64)),
            true,
            false,
        )
        .ok()?;
    let img = image::load_from_memory(&data).ok()?.to_rgb8();
    let small = image::imageops::resize(&img, 128, 128, image::imageops::FilterType::Nearest);
    // 128*128*3 → 压成 32 字节摘要(逐 48 字节折叠 XOR)
    let mut digest = [0u8; 32];
    for (i, b) in small.iter().enumerate() {
        digest[i % 32] ^= *b;
    }
    Some(digest)
}

/// 完整 settle(要素 5-8 编排;WPI `settle` 时序)。返回无限动画数。
pub fn settle(page: &mut PageSession) -> Result<u32, String> {
    wait_assets(page);
    page.ensure_alive()?;
    trigger_scroll_reveals(page);
    let infinite = freeze_animations(page);
    if infinite > 0 {
        page.sleep(3000);
        trigger_scroll_reveals(page);
        freeze_animations(page);
        page.sleep(3000);
    } else {
        // K5:稳定性探测失败不阻断(settle 兜底),显式丢弃
        let _ = wait_visual_stability(page);
    }
    page.ensure_alive()?;
    Ok(infinite)
}

// -------------------------------------------------------------- capture
/// 整页 PNG 导出(要素 2-4、9;WPI PNG 全页分支的等价实现)。
pub fn capture_png(
    page: &mut PageSession,
    opts: &CaptureOptions,
) -> Result<CaptureOutcome, String> {
    // 要素 10:脚本执行前注入 rAF 节流 + reduced-motion
    let _ = page.add_init_script(RAF_THROTTLE_JS);
    page.emulate_static()?;
    if opts.artboard {
        // 画板即画布(P0-3):页边距属页面 chrome,重置后再收敛,
        // 使画板矩形落在文档原点(与 native 车道同语义)
        let _ = page.evaluate(BODY_MARGIN_RESET_JS, false);
        page.sleep(120);
    }
    // 要素 1:load + networkidle + 200ms(调用方已 navigate 亦可,这里由 caller 控制时序)
    let (mut sw, sh) = page.content_size()?;
    sw = sw.max(opts.width);
    let warnings = page.collect_resource_warnings();
    let infinite = settle(page)?;

    // 高度锁定:视口高 = 锁定值,导出顶部 min(lock, contentH)
    let (clip_h, viewport_h) = if let Some(lock) = opts.height_lock {
        page.set_device_metrics(opts.width, lock, opts.scale)?;
        page.sleep(150);
        let (_, content_h) = page.content_size()?;
        (lock.min(content_h), lock)
    } else {
        (sh, opts.width)
    };

    let png: Vec<u8> = if opts.artboard {
        // P0-3 画板取景:单拍按画板矩形裁剪(视口已按声明尺寸设定)。
        // 尺寸恒为声明画板尺寸——高度不做 min(content) 收缩,尺寸门要求
        // 物理像素与声明严格相等;命中不到画板元素时回落文档原点。
        page.set_device_metrics(
            opts.width,
            opts.height_lock.unwrap_or(opts.width),
            opts.scale,
        )?;
        page.sleep(150);
        page.scroll_to(0);
        page.wait_two_raf();
        let rect = artboard_rect_of(
            page,
            opts.width,
            opts.height_lock.unwrap_or(0),
            opts.artboard_index,
        );
        let clip_h = match opts.height_lock {
            Some(lock) => lock,
            None => page.content_size()?.1,
        };
        let (cx, cy, cw, ch) = artboard_clip_rect(rect, opts.width, clip_h);
        page.screenshot("png", None, Some((cx, cy, cw, ch)), true, opts.transparent)?
    } else if opts.height_lock.is_some() {
        // 锁定高度:单次视口相对 clip(视口已设为锁定高)
        page.scroll_to(0);
        page.wait_two_raf();
        page.screenshot(
            "png",
            None,
            Some((0.0, 0.0, opts.width as f64, clip_h as f64)),
            false,
            opts.transparent,
        )?
    } else {
        let below_fold_canvas = page.has_below_fold_canvas();
        let fits_limit = (opts.width as u64 * opts.scale as u64) <= 15_000
            && (sh as u64 * opts.scale as u64) <= 15_000;
        if fits_limit && !below_fold_canvas {
            // 单拍:captureBeyondViewport(要素 4 优先路径)
            page.screenshot(
                "png",
                None,
                Some((0.0, 0.0, sw as f64, sh as f64)),
                true,
                opts.transparent,
            )?
        } else {
            capture_tiled(page, opts, sw, sh)?
        }
    };

    let (mut img, rgba) = decode_rgba(&png)?;
    if !opts.transparent {
        flatten_white(&mut img);
    }
    let out_w = img.width();
    let out_h = img.height();
    let bytes = encode_png(&img, rgba && opts.transparent)?;
    let _ = viewport_h;
    Ok(CaptureOutcome {
        png: bytes,
        width: out_w,
        height: out_h,
        warnings,
        infinite_animations: infinite,
    })
}

/// 画板矩形定位(采集侧):返回 `Some((x, y, w, h))` 或 None(未命中)。
fn artboard_rect_of(
    page: &mut PageSession,
    w: u32,
    h: u32,
    index: usize,
) -> Option<(f64, f64, f64, f64)> {
    let js = format!("({ARTBOARD_RECT_JS})({w}, {h}, {index})");
    let v = page.evaluate(&js, false).ok()?;
    let arr = v.as_array()?;
    let nums: Vec<f64> = arr.iter().filter_map(Value::as_f64).collect();
    (nums.len() == 4).then_some((nums[0], nums[1], nums[2], nums[3]))
}

/// 画板取景裁剪框(P0-3,纯函数):命中画板元素时用其**原点**(页边距/
/// 居中等偏移不进画布),未命中回落文档原点;尺寸恒为声明画板尺寸
/// (不采信元素实测宽高,尺寸门要求物理像素与声明严格相等)。
pub fn artboard_clip_rect(
    rect: Option<(f64, f64, f64, f64)>,
    width: u32,
    height: u32,
) -> (f64, f64, f64, f64) {
    match rect {
        Some((x, y, _, _)) => (x, y, width as f64, height as f64),
        None => (0.0, 0.0, width as f64, height as f64),
    }
}

/// 分块滚动截图 + 纵向拼接(WPI `capture_highres` 协议)。
fn capture_tiled(
    page: &mut PageSession,
    opts: &CaptureOptions,
    sw: u32,
    sh: u32,
) -> Result<Vec<u8>, String> {
    let total = sh;
    let viewport_h = opts.width; // WPI:视口高 = 宽
    page.set_device_metrics(opts.width, viewport_h, opts.scale)?;
    let mut tiles: Vec<image::RgbaImage> = Vec::new();
    let mut y = 0u32;
    let mut first = true;
    while y < total {
        page.scroll_to(y);
        page.sleep(400);
        page.wait_two_raf();
        let actual = page.scroll_y();
        let rel = y.saturating_sub(actual);
        let take = (viewport_h - rel).min(total - y);
        if take == 0 {
            y += viewport_h;
            continue;
        }
        if first {
            page.toggle_fixed_topbar(false);
            first = false;
        } else {
            page.toggle_fixed_topbar(true);
        }
        let data = page.screenshot(
            "png",
            None,
            Some((0.0, rel as f64, opts.width as f64, take as f64)),
            false,
            opts.transparent,
        )?;
        let (img, _) = decode_rgba(&data)?;
        tiles.push(img);
        y += viewport_h;
    }
    page.toggle_fixed_topbar(false);
    page.scroll_to(0);
    if tiles.is_empty() {
        return page.screenshot("png", None, None, false, opts.transparent);
    }
    let width = tiles.iter().map(|t| t.width()).max().unwrap_or(1);
    let height: u32 = tiles.iter().map(|t| t.height()).sum();
    let mut canvas = image::RgbaImage::new(width, height);
    let mut ty = 0u32;
    for t in tiles {
        image::imageops::overlay(&mut canvas, &t, 0, ty as i64);
        ty += t.height();
    }
    let _ = sw;
    encode_png(&canvas, true)
}

fn decode_rgba(png: &[u8]) -> Result<(image::RgbaImage, bool), String> {
    let img = image::load_from_memory(png).map_err(|e| format!("PNG 解码失败: {e}"))?;
    let has_alpha = img.color().has_alpha();
    Ok((img.to_rgba8(), has_alpha))
}

/// 白底展平(WPI `PNGExporter.write` 不透明分支;src-over on white)。
fn flatten_white(img: &mut image::RgbaImage) {
    for px in img.pixels_mut() {
        let a = px.0[3] as u32;
        if a == 255 {
            continue;
        }
        if a == 0 {
            px.0[0] = 255;
            px.0[1] = 255;
            px.0[2] = 255;
            px.0[3] = 255;
            continue;
        }
        for c in px.0.iter_mut().take(3) {
            let v = *c as u32;
            *c = ((v * a + 255 * (255 - a)) / 255) as u8;
        }
        px.0[3] = 255;
    }
}

fn encode_png(img: &image::RgbaImage, keep_alpha: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(1024 * 1024);
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        std::io::Cursor::new(&mut out),
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    );
    if keep_alpha {
        image::ImageEncoder::write_image(
            encoder,
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    } else {
        let flat = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
        image::ImageEncoder::write_image(
            encoder,
            flat.as_raw(),
            flat.width(),
            flat.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| format!("PNG 编码失败: {e}"))?;
    }
    Ok(out)
}

/// 供打印(settle 后)读取内容尺寸的便捷封装。
pub fn content_size_value(page: &mut PageSession) -> Result<(u32, u32), String> {
    let v = page.evaluate(CONTENT_SIZE_JS, false)?;
    let arr = v.as_array().cloned().unwrap_or_default();
    let w = arr.first().and_then(Value::as_f64).unwrap_or(1.0) as u32;
    let h = arr.get(1).and_then(Value::as_f64).unwrap_or(1.0) as u32;
    Ok((w.max(1), h.max(1)))
}

/// DOM 快照采集结果(ADR-0021:位图降级源 + 采集清单)。
pub struct DomCapture {
    pub list: serde_json::Value,
    pub page_png: Vec<u8>,
    pub url_prefix: String,
    pub warnings: Vec<String>,
}

/// 加载源页面(DSF=1)→ settle → DOM 快照 + 整页截图(PNG)。
pub fn capture_dom(page: &mut PageSession, _url: &str) -> Result<DomCapture, String> {
    let mut warnings = Vec::new();
    let (sw, sh) = page.content_size()?;
    let _ = sw;
    let infinite = settle(page)?;
    if infinite > 0 {
        warnings.push(format!("存在 {infinite} 个无限循环动画,采集为当前帧"));
    }
    let list = crate::domsnap::snapshot(page)?;
    let page_png = page.screenshot(
        "png",
        None,
        Some((0.0, 0.0, sw as f64, sh as f64)),
        true,
        false,
    )?;
    let _ = sh;
    Ok(DomCapture {
        list,
        page_png,
        url_prefix: String::new(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_rect_uses_element_origin_with_declared_size() {
        // P0-3:body 默认 margin 8px 使画板位于 (8,8)、整页 766×1350,
        // 裁剪框必须取画板原点 + **声明**尺寸(examples/poster 形态)
        let clip = artboard_clip_rect(Some((8.0, 8.0, 766.0, 1350.0)), 750, 1334);
        assert_eq!(clip, (8.0, 8.0, 750.0, 1334.0));
    }

    #[test]
    fn clip_rect_falls_back_to_origin() {
        // 未命中画板元素:回落文档原点,尺寸仍为声明值
        let clip = artboard_clip_rect(None, 1920, 1080);
        assert_eq!(clip, (0.0, 0.0, 1920.0, 1080.0));
    }

    #[test]
    fn clip_rect_declared_size_not_content_size() {
        // 尺寸门:高度不因内容收缩(此前 height-lock 分支取 min(lock, contentH))
        let clip = artboard_clip_rect(Some((0.0, 0.0, 1920.0, 800.0)), 1920, 1080);
        assert_eq!(clip, (0.0, 0.0, 1920.0, 1080.0));
    }
}
