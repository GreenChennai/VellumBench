//! Compatibility façade for animation APIs used by Kiln and the legacy host.
//!
//! CSS parsing/evaluation lives in `vb_animation`; DrawList application lives in
//! `vb_render`, alongside the renderer-owned projection it mutates.

pub use vb_animation::{
    eval_node, parse_animation_shorthand, parse_keyframes, parse_timing, parse_transform_funcs,
    resolve_node_anim, track_value_at, AnimCoverage, AnimInstance, FrameState, Keyframes, NodeAnim,
    Timing, Track,
};
pub use vb_render::animation::{
    apply_document_animations, apply_frame_state, apply_transform, try_apply_frame_state,
};
