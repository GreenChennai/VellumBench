//! Renderer-side application of the shared, host-independent CSS animation model.
//!
//! The document and `vb_animation` remain authoritative; these helpers only mutate
//! disposable `DrawList` projections used by preview and export.

use std::collections::HashMap;

use vb_animation::{
    eval_node, parse_keyframes, parse_transform_funcs, resolve_node_anim, NodeAnim,
};
use vb_doc::{Document, VbError};

use crate::encode::{self, DrawItem, DrawList};

/// Apply one CSS transform to a disposable draw item.
///
/// Unsupported functions and malformed/non-finite arguments are ignored, matching
/// the renderer's existing graceful-degradation policy.
pub fn apply_transform(item: &mut DrawItem, transform: &str) {
    let mut tx = 0.0_f64;
    let mut ty = 0.0_f64;
    let mut sx = 1.0_f64;
    let mut sy = 1.0_f64;
    let mut rot = 0.0_f64;
    for (name, args) in parse_transform_funcs(transform) {
        if args.iter().any(|value| !value.is_finite()) {
            continue;
        }
        let Some(value) = args.first().copied() else {
            continue;
        };
        match name.as_str() {
            "translatex" => tx += value,
            "translatey" => ty += value,
            "rotate" => rot += value,
            "scale" => {
                sx = value;
                sy = args.get(1).copied().unwrap_or(value);
            }
            "scalex" => sx = value,
            "scaley" => sy = value,
            _ => {}
        }
    }
    item.rect[0] += tx;
    item.rect[1] += ty;
    if (sx - 1.0).abs() > 1e-6 || (sy - 1.0).abs() > 1e-6 {
        let cx = item.rect[0] + item.rect[2] / 2.0;
        let cy = item.rect[1] + item.rect[3] / 2.0;
        item.rect[2] *= sx;
        item.rect[3] *= sy;
        item.rect[0] = cx - item.rect[2] / 2.0;
        item.rect[1] = cy - item.rect[3] / 2.0;
    }
    if rot.abs() > 1e-6 {
        item.rot += rot;
    }
}

/// Apply pre-resolved node animations at time `t` to a disposable draw list.
///
/// For compatibility with the original Kiln API, invalid times are ignored.
pub fn apply_frame_state(animations: &HashMap<String, NodeAnim>, list: &mut DrawList, t: f64) {
    if !t.is_finite() || t < 0.0 {
        return;
    }
    for item in &mut list.items {
        let Some(node_animation) = animations.get(&item.sid) else {
            continue;
        };
        let state = eval_node(node_animation, t);
        if let Some(opacity) = state.opacity.filter(|value| value.is_finite()) {
            item.opacity *= opacity.clamp(0.0, 1.0) as f32;
        }
        if let Some(transform) = state.transform.as_deref() {
            apply_transform(item, transform);
        }
        let [_, _, width, height] = item.rect;
        if let Some(clip_path) = state.clip_path.as_deref() {
            item.clip = encode::parse_clip_path(clip_path, width, height);
        }
        if let Some(filter) = state.filter.as_deref() {
            item.filter = encode::parse_filter(filter);
        }
    }
}

/// Checked variant of [`apply_frame_state`] for new callers that need errors.
///
/// # Errors
/// Returns an error when `t` is negative or non-finite.
pub fn try_apply_frame_state(
    animations: &HashMap<String, NodeAnim>,
    list: &mut DrawList,
    t: f64,
) -> Result<(), VbError> {
    if !t.is_finite() || t < 0.0 {
        return Err(VbError::Parse("动画预览时间必须为有限非负数".into()));
    }
    apply_frame_state(animations, list, t);
    Ok(())
}

/// Resolve animations for visible draw items and apply them at `t` to a disposable
/// draw list. Hidden nodes remain absent because they are not in the projection.
///
/// # Errors
/// Returns an error when `t` is negative or non-finite.
pub fn apply_document_animations(
    document: &Document,
    list: &mut DrawList,
    t: f64,
) -> Result<(), VbError> {
    if !t.is_finite() || t < 0.0 {
        return Err(VbError::Parse("动画预览时间必须为有限非负数".into()));
    }
    if list.items.is_empty() {
        return Ok(());
    }
    let keyframes = parse_keyframes(&document.raw_css);
    let animations = list
        .items
        .iter()
        .filter_map(|item| {
            let node = document.find_by_sid(&item.sid)?;
            let animation = resolve_node_anim(document, node, &keyframes)?;
            Some((item.sid.clone(), animation))
        })
        .collect();
    try_apply_frame_state(&animations, list, t)
}
