//! Shared, named CSS animation definitions stored as ignored CSS metadata plus standard keyframes.

use crate::timeline::{
    serialize_keyframes_named, AnimModel, Easing, KeyValue, Keyframe, TrackProp,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

const METADATA_PREFIX: &str = "@vb-animation-preset-v1 ";

/// Stable CSS-safe identifier and user-visible name for a shared animation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnimationPresetRef {
    pub id: String,
    pub display_name: String,
}

impl AnimationPresetRef {
    /// Constructs a validated preset reference. IDs are lowercase CSS-safe slugs.
    pub fn new(id: impl Into<String>, display_name: impl Into<String>) -> Option<Self> {
        let id = id.into();
        let display_name = display_name.into();
        if !valid_preset_id(&id)
            || display_name.trim().is_empty()
            || display_name.len() > 120
            || display_name.chars().any(char::is_control)
        {
            return None;
        }
        Some(Self { id, display_name })
    }
}

/// One shared preset and its editable typed projection.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationPreset {
    pub reference: AnimationPresetRef,
    pub model: AnimModel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PresetWire {
    version: u8,
    id: String,
    display_name: String,
    duration_ms: f64,
    delay_ms: f64,
    iterations: String,
    tracks: Vec<(TrackProp, Vec<Keyframe>)>,
    extra: Vec<(f32, Vec<(String, String)>)>,
    keyframes_hash: u64,
}

/// CSS @keyframes name for a shared preset.
pub fn preset_keyframes_name(id: &str) -> String {
    format!("vb-anim-preset-{id}")
}

/// Produces a browser-ignored metadata at-rule and a standard CSS keyframes block.
pub fn serialize_preset_blocks(preset: &AnimationPreset) -> Option<(String, String)> {
    let reference = AnimationPresetRef::new(
        preset.reference.id.clone(),
        preset.reference.display_name.clone(),
    )?;
    if !valid_model(&preset.model) {
        return None;
    }
    let keyframes = serialize_keyframes_named(&preset_keyframes_name(&reference.id), &preset.model);
    let iterations = if preset.model.iterations.is_infinite() {
        "infinite".to_string()
    } else {
        preset.model.iterations.to_string()
    };
    let wire = PresetWire {
        version: 1,
        id: reference.id.clone(),
        display_name: reference.display_name.clone(),
        duration_ms: preset.model.duration_ms,
        delay_ms: preset.model.delay_ms,
        iterations,
        tracks: preset.model.tracks.clone(),
        extra: preset.model.extra.clone(),
        keyframes_hash: stable_hash(keyframes.trim().as_bytes()),
    };
    let json = serde_json::to_vec(&wire).ok()?;
    Some((
        format!("{METADATA_PREFIX}{} {};", reference.id, hex_encode(&json)),
        keyframes,
    ))
}

/// Loads valid presets from the two raw CSS blocks. Invalid metadata, mismatched keyframes,
/// duplicate IDs and malformed values remain inert and are not exposed as editable presets.
pub fn parse_animation_presets(raw_css: &[String]) -> Vec<AnimationPreset> {
    let keyframe_blocks = keyframe_blocks(raw_css);
    let mut seen = HashSet::new();
    let mut presets = Vec::new();
    for block in raw_css {
        let Some(payload) = block.trim().strip_prefix(METADATA_PREFIX) else {
            continue;
        };
        let payload = payload.trim().trim_end_matches(';').trim();
        let mut parts = payload.split_whitespace();
        let Some(marker_id) = parts.next() else {
            continue;
        };
        let Some(encoded) = parts.next() else {
            continue;
        };
        if parts.next().is_some() {
            continue;
        }
        let Some(bytes) = hex_decode(encoded) else {
            continue;
        };
        let Ok(wire) = serde_json::from_slice::<PresetWire>(&bytes) else {
            continue;
        };
        if wire.id != marker_id {
            continue;
        }
        let Some(reference) = AnimationPresetRef::new(wire.id, wire.display_name) else {
            continue;
        };
        if wire.version != 1 || !seen.insert(reference.id.clone()) {
            continue;
        }
        let name = preset_keyframes_name(&reference.id);
        let Some(raw_keyframes) = keyframe_blocks.get(&name) else {
            continue;
        };
        if stable_hash(raw_keyframes.trim().as_bytes()) != wire.keyframes_hash {
            continue;
        }
        let iterations = if wire.iterations == "infinite" {
            f64::INFINITY
        } else {
            let Ok(value) = wire.iterations.parse::<f64>() else {
                continue;
            };
            value
        };
        let mut model = AnimModel {
            duration_ms: wire.duration_ms,
            delay_ms: wire.delay_ms,
            iterations,
            tracks: wire.tracks,
            extra: wire.extra,
            preset: Some(reference.clone()),
        };
        if !valid_model(&model) {
            continue;
        }
        // Verify the referenced CSS block is parseable, not only hash-matched.
        if !crate::core::parse_keyframes(raw_css).contains_key(&name) {
            continue;
        }
        model.preset = Some(reference.clone());
        presets.push(AnimationPreset { reference, model });
    }
    presets
}

fn keyframe_blocks(raw_css: &[String]) -> HashMap<String, &str> {
    let mut blocks = HashMap::new();
    for block in raw_css {
        let trimmed = block.trim();
        let Some(rest) = trimmed.strip_prefix("@keyframes") else {
            continue;
        };
        let name = rest
            .trim_start()
            .split(|c: char| c.is_whitespace() || c == '{')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if !name.is_empty() {
            blocks.insert(name, trimmed);
        }
    }
    blocks
}

fn valid_preset_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !id.ends_with('-')
        && !id.contains("--")
}

fn valid_model(model: &AnimModel) -> bool {
    model.duration_ms.is_finite()
        && model.duration_ms > 0.0
        && model.delay_ms.is_finite()
        && model.delay_ms >= 0.0
        && (model.iterations.is_infinite()
            || (model.iterations.is_finite() && model.iterations >= 1.0))
        && model.tracks.iter().all(|(_, frames)| {
            frames.iter().all(|frame| {
                frame.t.is_finite()
                    && frame.t >= 0.0
                    && frame.t <= model.duration_ms / 1000.0
                    && valid_keyframe(frame)
            })
        })
}

fn valid_keyframe(frame: &Keyframe) -> bool {
    let value_valid = match frame.value {
        KeyValue::Offset(x, y) => x.is_finite() && y.is_finite(),
        KeyValue::Factor(value) | KeyValue::Alpha(value) | KeyValue::Angle(value) => {
            value.is_finite()
        }
    };
    let easing_valid = match frame.easing {
        Easing::Bezier(x1, y1, x2, y2) => {
            x1.is_finite()
                && (0.0..=1.0).contains(&x1)
                && y1.is_finite()
                && y1.abs() <= 10.0
                && x2.is_finite()
                && (0.0..=1.0).contains(&x2)
                && y2.is_finite()
                && y2.abs() <= 10.0
        }
        _ => true,
    };
    value_valid && easing_valid
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let (pairs, _) = value.as_bytes().as_chunks::<2>();
    for pair in pairs {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        bytes.push((high << 4) | low);
    }
    Some(bytes)
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::{
        parse_animation_presets, serialize_preset_blocks, AnimationPreset, AnimationPresetRef,
    };
    use crate::timeline::{AnimModel, Easing, KeyValue, Keyframe, TrackProp};

    fn preset(iterations: f64) -> AnimationPreset {
        let reference = AnimationPresetRef::new("bounce", "Bounce").unwrap();
        AnimationPreset {
            reference: reference.clone(),
            model: AnimModel {
                duration_ms: 1000.0,
                delay_ms: 0.0,
                iterations,
                tracks: vec![(
                    TrackProp::Position,
                    vec![Keyframe {
                        t: 1.0,
                        value: KeyValue::Offset(10.0, 0.0),
                        easing: Easing::EaseOut,
                    }],
                )],
                extra: Vec::new(),
                preset: Some(reference),
            },
        }
    }

    #[test]
    fn preset_metadata_and_keyframes_round_trip() {
        let source = preset(3.0);
        let (metadata, keyframes) = serialize_preset_blocks(&source).unwrap();
        let parsed = parse_animation_presets(&[metadata, keyframes]);
        assert_eq!(parsed, vec![source]);
    }

    #[test]
    fn preset_preserves_infinite_iterations_and_rejects_tampered_keyframes() {
        let source = preset(f64::INFINITY);
        let (metadata, mut keyframes) = serialize_preset_blocks(&source).unwrap();
        let parsed = parse_animation_presets(&[metadata.clone(), keyframes.clone()]);
        assert_eq!(parsed, vec![source]);
        keyframes = keyframes.replace("translateX(10px)", "translateX(11px)");
        assert!(parse_animation_presets(&[metadata, keyframes]).is_empty());
    }

    #[test]
    fn preset_ids_are_css_safe_and_display_names_are_bounded() {
        assert!(AnimationPresetRef::new("spring-1", "弹跳").is_some());
        assert!(AnimationPresetRef::new("Bad ID", "Bounce").is_none());
        assert!(AnimationPresetRef::new("spring-1", "\n").is_none());
        assert!(AnimationPresetRef::new("spring-1", "x".repeat(121)).is_none());
    }
}
