//! 位图工具箱(v0.6):crop / stitch / blur / pad / info —— 纯 image crate,零外部依赖。
//!
//! 设计:系列物料的纯像素后处理(公众号合并封面、长图拼接、封面裁切等)
//! 从 Python/Pillow 下沉到 Kiln,一次构建随处调用:
//! `Kiln-noGUI-CLI.exe img stitch --inputs a.png,b.png --out merged.png --gap 57`
//!
//! 颜色格式统一 `#rgb` / `#rrggbb` / `#rrggbbaa`。

use image::{GenericImageView, RgbaImage};

/// 解析 `#rgb` / `#rrggbb` / `#rrggbbaa` → RGBA。
pub fn parse_hex_color(s: &str) -> Result<[u8; 4], String> {
    let t = s.trim().trim_start_matches('#');
    let hex_ok = |h: &str| h.len().is_multiple_of(2) && h.chars().all(|c| c.is_ascii_hexdigit());
    match t.len() {
        3 => {
            let v: Vec<u8> = t
                .chars()
                .filter_map(|c| u8::from_str_radix(&c.to_string(), 16).ok())
                .flat_map(|v| vec![v * 17])
                .collect();
            if v.len() == 3 {
                Ok([v[0], v[1], v[2], 255])
            } else {
                Err(format!("无效颜色: {s}"))
            }
        }
        6 if hex_ok(t) => Ok([
            u8::from_str_radix(&t[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&t[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&t[4..6], 16).unwrap_or(0),
            255,
        ]),
        8 if hex_ok(t) => Ok([
            u8::from_str_radix(&t[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&t[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&t[4..6], 16).unwrap_or(0),
            u8::from_str_radix(&t[6..8], 16).unwrap_or(255),
        ]),
        _ => Err(format!("无效颜色: {s}(支持 #rgb/#rrggbb/#rrggbbaa)")),
    }
}

use std::path::{Path, PathBuf};

fn open_rgba(path: &Path) -> Result<RgbaImage, String> {
    let img = image::open(path).map_err(|e| format!("读取 {path:?} 失败: {e}"))?;
    Ok(img.to_rgba8())
}

/// 裁剪:`box` = (left, top, right, bottom) 像素;`trim`:自动去除与角落同色的边缘。
pub fn crop(
    input: &Path,
    out: &Path,
    box_spec: Option<(u32, u32, u32, u32)>,
    trim_color: Option<[u8; 4]>,
) -> Result<RgbaImage, String> {
    let img = open_rgba(input)?;
    let (iw, ih) = (img.width(), img.height());
    let cropped = if let Some((l, t, r, b)) = box_spec {
        let l = l.min(iw);
        let t = t.min(ih);
        let r = r.clamp(l + 1, iw);
        let b = b.clamp(t + 1, ih);
        image::imageops::crop_imm(&img, l, t, r - l, b - l).to_image()
    } else if let Some(bg) = trim_color {
        trim_uniform_border(&img, bg)
    } else {
        img
    };
    cropped
        .save(out)
        .map_err(|e| format!("写出 {out:?} 失败: {e}"))?;
    Ok(cropped)
}

/// 自动去除四边与指定色一致(容差 ≤6/通道)的边缘。
fn trim_uniform_border(img: &RgbaImage, bg: [u8; 4]) -> RgbaImage {
    let (w, h) = img.dimensions();
    let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b.iter()).all(|(x, y)| x.abs_diff(*y) <= 6);
    let px = |x: u32, y: u32| -> [u8; 4] { img.get_pixel(x, y).0 };
    let mut top = 0;
    while top < h - 1 && (0..w).all(|x| near(px(x, top), bg)) {
        top += 1;
    }
    let mut bottom = h - 1;
    while bottom > top && (0..w).all(|x| near(px(x, bottom), bg)) {
        bottom -= 1;
    }
    let mut left = 0;
    while left < w - 1 && (top..=bottom).all(|y| near(px(left, y), bg)) {
        left += 1;
    }
    let mut right = w - 1;
    while right > left && (top..=bottom).all(|y| near(px(right, y), bg)) {
        right -= 1;
    }
    image::imageops::crop_imm(img, left, top, right - left + 1, bottom - top + 1).to_image()
}

/// 拼接:多图按方向排列,间隔 gap,背景色填充,align = 轴向对齐。
pub fn stitch(
    inputs: &[PathBuf],
    out: &Path,
    vertical: bool,
    gap: u32,
    bg: [u8; 4],
    align: &str,
) -> Result<RgbaImage, String> {
    if inputs.len() < 2 {
        return Err("stitch 至少需要 2 张输入".into());
    }
    let imgs: Vec<RgbaImage> = inputs
        .iter()
        .map(|p| open_rgba(p))
        .collect::<Result<_, _>>()?;
    let gap = gap as u64;

    let total_main: u64 = if vertical {
        imgs.iter().map(|i| i.height() as u64).sum::<u64>() + gap * (imgs.len() as u64 - 1)
    } else {
        imgs.iter().map(|i| i.width() as u64).sum::<u64>() + gap * (imgs.len() as u64 - 1)
    };
    let total_cross = if vertical {
        imgs.iter().map(|i| i.width() as u64).max().unwrap_or(0)
    } else {
        imgs.iter().map(|i| i.height() as u64).max().unwrap_or(0)
    };
    if total_main > u32::MAX as u64 || total_cross > u32::MAX as u64 {
        return Err("拼接结果超出画布上限".into());
    }
    let (tw, th) = if vertical {
        (total_cross as u32, total_main as u32)
    } else {
        (total_main as u32, total_cross as u32)
    };
    let mut canvas = RgbaImage::from_pixel(tw, th, image::Rgba(bg));

    let mut main_pos = 0u32;
    for img in &imgs {
        let cross_offset: u32 = if vertical {
            match align {
                "center" => (tw - img.width()) / 2,
                "end" | "right" | "bottom" => tw - img.width(),
                _ => 0,
            }
        } else {
            match align {
                "center" => (th - img.height()) / 2,
                "end" | "right" | "bottom" => th - img.height(),
                _ => 0,
            }
        };
        if vertical {
            image::imageops::overlay(&mut canvas, img, cross_offset as i64, main_pos as i64);
        } else {
            image::imageops::overlay(&mut canvas, img, main_pos as i64, cross_offset as i64);
        }
        main_pos += if vertical { img.height() } else { img.width() } + gap as u32;
    }
    canvas
        .save(out)
        .map_err(|e| format!("写出 {out:?} 失败: {e}"))?;
    Ok(canvas)
}

/// 高斯模糊(整图或区域)。
pub fn blur(
    input: &Path,
    out: &Path,
    radius: f32,
    box_spec: Option<(u32, u32, u32, u32)>,
) -> Result<(), String> {
    let img = open_rgba(input)?;
    let blurred = image::DynamicImage::ImageRgba8(img).blur(radius.max(0.5));
    let mut result = blurred.to_rgba8();
    if let Some((l, t, r, b)) = box_spec {
        // 区域模糊:整图模糊后只把 box 内的像素写回原位
        let full = open_rgba(input)?;
        let (iw, ih) = full.dimensions();
        let l = l.min(iw);
        let t = t.min(ih);
        let r = r.clamp(l + 1, iw);
        let b = b.clamp(t + 1, ih);
        let mut combined = result.clone();
        for y in t..b {
            for x in l..r {
                let p = result.get_pixel(x, y);
                combined.put_pixel(x, y, *p);
            }
        }
        result = combined;
    }
    result
        .save(out)
        .map_err(|e| format!("写出 {out:?} 失败: {e}"))?;
    Ok(())
}

/// 画布填充:把图像放到指定尺寸的画布上(居中/对齐),背景色填充。
pub fn pad(
    input: &Path,
    out: &Path,
    tw: u32,
    th: u32,
    bg: [u8; 4],
    align: &str,
) -> Result<(), String> {
    let img = open_rgba(input)?;
    let mut canvas = RgbaImage::from_pixel(tw, th, image::Rgba(bg));
    let ox = match align {
        "center" => tw.saturating_sub(img.width()) / 2,
        "end" | "right" | "bottom" => tw.saturating_sub(img.width()),
        _ => 0,
    };
    let oy = match align {
        "center" => th.saturating_sub(img.height()) / 2,
        "end" | "right" | "bottom" => th.saturating_sub(img.height()),
        _ => 0,
    };
    image::imageops::overlay(&mut canvas, &img, ox as i64, oy as i64);
    canvas
        .save(out)
        .map_err(|e| format!("写出 {out:?} 失败: {e}"))?;
    Ok(())
}

/// 信息:宽高。
pub fn info(input: &Path) -> Result<(u32, u32), String> {
    let img = image::open(input).map_err(|e| format!("读取失败: {e}"))?;
    Ok(img.dimensions())
}
