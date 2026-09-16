//! 光栅层:DrawList → RGBA8 / PNG 字节。
//!
//! Kiln 不重写光栅算法 —— vb_render::cpu(tiny-skia + swash 真字形)是
//! VellumBench 的单一渲染真相;Kiln 调用它并解回原始像素供各编码器消费。

use vb_render::encode::DrawList;

use crate::error::{KilnError, KilnResult};

/// DrawList → RGBA8。
pub fn rasterize_rgba(list: &DrawList, scale: f64) -> KilnResult<Vec<u8>> {
    let res = vb_render::cpu::render_png(list, scale as f32, true, None)
        .map_err(KilnError::Encode)?;
    let img =
        image::load_from_memory(&res.png).map_err(|e| KilnError::Encode(format!("光栅结果解码失败:{e}")))?;
    Ok(img.to_rgba8().into_raw())
}

/// DrawList → PNG 字节(按指定透明度)。
pub fn rasterize_png_bytes(list: &DrawList, scale: f32, transparent: bool) -> KilnResult<Vec<u8>> {
    let res = vb_render::cpu::render_png(list, scale, transparent, None)
        .map_err(KilnError::Encode)?;
    Ok(res.png)
}
