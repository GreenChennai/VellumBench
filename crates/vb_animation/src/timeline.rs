//! Pure timeline data model and CSS projection for CSS animations.
//!
//! This module owns no UI state; the source of truth remains the Document CSS.

use crate::core::{
    parse_keyframes, parse_timing, parse_transform_funcs, resolve_node_anim, Timing,
};
use crate::preset::AnimationPresetRef;
use serde::{Deserialize, Serialize};
use vb_doc::model::Document;

// ─────────────────────── 模型(纯数据,门禁测试直接打这里) ───────────────────────

/// 轨道属性(定死,见模块注释:volume 无文档建模,剔除)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackProp {
    /// 平移偏移(px,相对对象静态位置;CSS translateX/Y)
    Position,
    /// 等比缩放因子(1 = 原尺寸;CSS scale)
    Scale,
    /// 不透明度(0..1;CSS opacity)
    Opacity,
    /// 旋转角(deg,CSS 顺时针;CSS rotate)
    Rotate,
}

impl TrackProp {
    /// 全部轨道(面板行序)。
    pub const ALL: [TrackProp; 4] = [
        TrackProp::Position,
        TrackProp::Scale,
        TrackProp::Opacity,
        TrackProp::Rotate,
    ];
}

/// 缓动:封闭枚举 + cubic-bezier 自定义(x1/x2 ∈ [0,1],y 可越界)。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    Bezier(f64, f64, f64, f64),
}

impl Easing {
    /// 封闭枚举(自定义之外的选项表)。
    pub const PRESETS: [Easing; 5] = [
        Easing::Linear,
        Easing::Ease,
        Easing::EaseIn,
        Easing::EaseOut,
        Easing::EaseInOut,
    ];

    /// CSS 值(与 vb_css canonical 同为小写函数名)。
    pub fn css(&self) -> String {
        match self {
            Easing::Linear => "linear".into(),
            Easing::Ease => "ease".into(),
            Easing::EaseIn => "ease-in".into(),
            Easing::EaseOut => "ease-out".into(),
            Easing::EaseInOut => "ease-in-out".into(),
            Easing::Bezier(x1, y1, x2, y2) => format!(
                "cubic-bezier({}, {}, {}, {})",
                fmt_num(*x1),
                fmt_num(*y1),
                fmt_num(*x2),
                fmt_num(*y2)
            ),
        }
    }

    /// CSS 值 → Easing(未知值回退 Linear;调用方保证值来自已解析声明)。
    pub fn parse(css: &str) -> Easing {
        match parse_timing(css) {
            Some(Timing::Linear) | None => Easing::Linear,
            Some(Timing::Ease) => Easing::Ease,
            Some(Timing::EaseIn) => Easing::EaseIn,
            Some(Timing::EaseOut) => Easing::EaseOut,
            Some(Timing::EaseInOut) => Easing::EaseInOut,
            Some(Timing::CubicBezier(x1, y1, x2, y2)) => Easing::Bezier(x1, y1, x2, y2),
        }
    }

    /// → 求值侧 Timing(预览与导出同一套曲线求解)。
    pub fn to_timing(self) -> Timing {
        match self {
            Easing::Linear => Timing::Linear,
            Easing::Ease => Timing::Ease,
            Easing::EaseIn => Timing::EaseIn,
            Easing::EaseOut => Timing::EaseOut,
            Easing::EaseInOut => Timing::EaseInOut,
            Easing::Bezier(x1, y1, x2, y2) => Timing::CubicBezier(x1, y1, x2, y2),
        }
    }
}

/// 关键帧:t = 秒(0..=时长),值随轨道而异,每帧自带缓动。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keyframe {
    pub t: f64,
    pub value: KeyValue,
    pub easing: Easing,
}

/// 关键帧值(按轨道;与 [`TrackProp`] 一一对应)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyValue {
    /// 平移偏移 (dx, dy) px
    Offset(f64, f64),
    /// 缩放因子
    Factor(f64),
    /// 不透明度
    Alpha(f64),
    /// 旋转角 deg
    Angle(f64),
}

/// 单对象时间轴模型(CSS 投影;编辑后经 [`serialize_keyframes`] 写回)。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AnimModel {
    /// 时长 ms(> 0;面板钳 100..=60000)。
    pub duration_ms: f64,
    /// 延迟 ms。
    pub delay_ms: f64,
    /// 迭代次数(整数 ≥ 1;INFINITY = 无限,简写落盘为 `infinite`)。
    pub iterations: f64,
    /// 轨道(只含有帧轨道;每轨按 t 升序)。
    pub tracks: Vec<(TrackProp, Vec<Keyframe>)>,
    /// 时间轴未建模声明(clip-path/filter 等)按停靠点 offset 保留,
    /// 序列化时随块写回 —— 编辑不吞导入内容(诚实边界)。
    pub extra: Vec<(f32, Vec<(String, String)>)>,
    /// Shared preset identity, present only when the model was projected from a named preset.
    #[serde(skip)]
    pub preset: Option<AnimationPresetRef>,
}

impl AnimModel {
    /// 指定轨道的关键帧(无则空)。
    pub fn frames_of(&self, p: TrackProp) -> &[Keyframe] {
        self.tracks
            .iter()
            .find(|(tp, _)| *tp == p)
            .map(|(_, k)| k.as_slice())
            .unwrap_or(&[])
    }

    /// 是否完全无帧(清除语义:块 + 声明一起删)。
    pub fn is_empty(&self) -> bool {
        self.tracks.iter().all(|(_, k)| k.is_empty()) && self.extra.is_empty()
    }
}

/// 将动画时间(秒)映射为稳定的 CSS 千分位停靠点;无效/越界时间返回 None。
pub fn keyframe_offset_permille(t: f64, duration_seconds: f64) -> Option<u16> {
    if !t.is_finite() || !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return None;
    }
    let progress = t / duration_seconds;
    if !progress.is_finite() || !(0.0..=1.0).contains(&progress) {
        return None;
    }
    let offset = (progress * 1000.0).round();
    (0.0..=1000.0).contains(&offset).then_some(offset as u16)
}

/// 面板可编辑性(诚实边界,见模块注释)。
#[derive(Debug, Clone, PartialEq)]
pub enum AnimSource {
    /// 无动画(未设置):可新建。
    None,
    /// 时间轴命名动画(`vb-anim-<sid>`):可编辑。
    Timeline(AnimModel),
    /// 外部命名动画:只读展示动画名,不给编辑入口(防整块重写吞内容)。
    Foreign(String),
}

// ─────────────────────── 序列化(模型 → CSS;纯函数) ───────────────────────

/// 序列化 @keyframes 块:多属性关键帧**合并为百分比停靠点**(05-9-4);
/// 每停靠点携带 animation-timing-function(管该点 → 下一点段)。
/// 未建模声明([`AnimModel::extra`])按 offset 合并回停靠点原样保留。
pub fn serialize_keyframes(sid: &str, m: &AnimModel) -> String {
    serialize_keyframes_named(&vb_doc::commands::anim_block_name(sid), m)
}

/// 序列化共享或本地 animation 名称对应的 @keyframes 块。
pub(crate) fn serialize_keyframes_named(name: &str, m: &AnimModel) -> String {
    // 时长换算成秒(模型 t 单位 = 秒,duration_ms 单位 = 毫秒)
    let dur_s = (m.duration_ms / 1000.0).max(0.001);
    let mut stops: Vec<(i64, StopProxy)> = Vec::new();
    // 轨道按 TrackProp::ALL 定序遍历(与模型内存储顺序无关 → 序列化确定)
    for prop in TrackProp::ALL {
        let Some((_, kfs)) = m.tracks.iter().find(|(tp, _)| *tp == prop) else {
            continue;
        };
        for kf in kfs {
            let off = (kf.t / dur_s).clamp(0.0, 1.0);
            let key = (off * 1000.0).round() as i64;
            let st = stop_slot(&mut stops, key);
            match kf.value {
                KeyValue::Offset(dx, dy) => st.tf.push(format!(
                    "translateX({}px) translateY({}px)",
                    fmt_num(dx),
                    fmt_num(dy)
                )),
                KeyValue::Factor(s) => st.tf.push(format!("scale({})", fmt_num(s))),
                KeyValue::Alpha(a) => st.opacity = Some(a.clamp(0.0, 1.0)),
                KeyValue::Angle(d) => st.tf.push(format!("rotate({}deg)", fmt_num(d))),
            }
            // 同停靠点的缓动须由调用方校验一致;这里确定性地采用先到者。
            if st.easing.is_none() {
                st.easing = Some(kf.easing);
            }
        }
    }
    for (off, decls) in &m.extra {
        let key = ((*off as f64) * 1000.0).round().clamp(0.0, 1000.0) as i64;
        let st = stop_slot(&mut stops, key);
        st.extra.extend(decls.iter().cloned());
    }
    stops.sort_by_key(|(k, _)| *k);

    let mut out = format!("@keyframes {name} {{\n");
    for (key, st) in &stops {
        let pct = *key as f64 / 10.0;
        out.push_str(&format!("  {}% {{ ", fmt_num(pct)));
        let mut parts: Vec<String> = Vec::new();
        if !st.tf.is_empty() {
            parts.push(format!("transform: {}", st.tf.join(" ")));
        }
        if let Some(a) = st.opacity {
            parts.push(format!("opacity: {}", fmt_num(a)));
        }
        for (p, v) in &st.extra {
            parts.push(format!("{p}: {v}"));
        }
        if let Some(e) = st.easing {
            parts.push(format!("animation-timing-function: {}", e.css()));
        }
        out.push_str(&parts.join("; "));
        out.push_str("; }\n");
    }
    out.push('}');
    out
}

/// 序列化 animation 简写(时序 nominal 用 linear:段缓动由帧内
/// animation-timing-function 承载,见 `anim::track_value_at`)。
pub fn serialize_animation(sid: &str, m: &AnimModel) -> String {
    serialize_animation_named(&format!("vb-anim-{sid}"), m)
}

/// 序列化 animation 简写并指定关键帧名称(共享预设与本地动画共用)。
pub fn serialize_animation_named(name: &str, m: &AnimModel) -> String {
    let iter = if m.iterations.is_infinite() {
        "infinite".to_string()
    } else {
        format!("{}", m.iterations.round().max(1.0) as i64)
    };
    format!(
        "{name} {}ms linear {}ms {iter} both",
        fmt_num(m.duration_ms.max(1.0)),
        fmt_num(m.delay_ms.max(0.0))
    )
}

/// 序列化停靠点(模块内):transform 片段 / opacity / 帧内缓动 / 未建模声明。
#[derive(Default)]
struct StopProxy {
    tf: Vec<String>,
    opacity: Option<f64>,
    easing: Option<Easing>,
    extra: Vec<(String, String)>,
}

/// 取(或建)指定 permille 键的停靠点(可变)。
fn stop_slot(stops: &mut Vec<(i64, StopProxy)>, key: i64) -> &mut StopProxy {
    if let Some(i) = stops.iter().position(|(k, _)| *k == key) {
        &mut stops[i].1
    } else {
        stops.push((key, StopProxy::default()));
        let n = stops.len() - 1;
        &mut stops[n].1
    }
}

// ─────────────────────── 投影(CSS → 模型;纯函数) ───────────────────────

/// 节点动画投影(面板读路径)。规则:
/// - 无 animation 声明 → [`AnimSource::None`];
/// - 简写首名 == `vb-anim-<sid>` 且块可解析 → [`AnimSource::Timeline`];
/// - 其余(外部命名关键帧)→ [`AnimSource::Foreign`] 只读。
pub fn parse_anim_source(doc: &Document, sid: &str) -> AnimSource {
    let Some(nid) = doc.find_by_sid(sid) else {
        return AnimSource::None;
    };
    let node = doc.nodes.get(nid).unwrap();
    let Some(raw) = node.style_get("animation") else {
        return AnimSource::None;
    };
    if raw.trim().is_empty() {
        return AnimSource::None;
    }
    // 简写首 token = 动画名(与导出期 parse_animation_shorthand 同一口径)
    let first = raw
        .split(',')
        .next()
        .unwrap_or(raw)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let local_name = vb_doc::commands::anim_block_name(sid);
    let preset = if first == local_name {
        None
    } else if let Some(id) = first.strip_prefix("vb-anim-preset-") {
        crate::preset::parse_animation_presets(&doc.raw_css)
            .into_iter()
            .find(|preset| preset.reference.id == id)
            .map(|preset| preset.reference)
    } else {
        None
    };
    if first != local_name && preset.is_none() {
        return AnimSource::Foreign(first);
    }
    let kf = parse_keyframes(&doc.raw_css);
    let Some(na) = resolve_node_anim(doc, nid, &kf) else {
        return AnimSource::Foreign(first);
    };
    let Some((inst, frames)) = na.instances.first() else {
        return AnimSource::Foreign(first);
    };
    let mut m = AnimModel {
        duration_ms: (inst.duration * 1000.0).max(1.0),
        delay_ms: inst.delay * 1000.0,
        iterations: inst.iterations,
        tracks: Vec::new(),
        extra: Vec::new(),
        preset,
    };
    // 帧投影:同帧的 transform 函数并进同一批轨道关键帧
    for (off, decls) in &frames.frames {
        let t = (*off as f64) * m.duration_ms / 1000.0;
        let mut easing = Easing::Linear;
        let mut pos: Option<(f64, f64)> = None;
        let mut factor: Option<f64> = None;
        let mut angle: Option<f64> = None;
        let mut alpha: Option<f64> = None;
        let mut extras: Vec<(String, String)> = Vec::new();
        for d in decls {
            match d.prop.as_str() {
                "transform" => {
                    for (name, args) in parse_transform_funcs(&d.value) {
                        match name.as_str() {
                            "translatex" => {
                                let p = pos.get_or_insert((0.0, 0.0));
                                p.0 = args[0];
                            }
                            "translatey" => {
                                let p = pos.get_or_insert((0.0, 0.0));
                                p.1 = args[0];
                            }
                            "scale" => factor = Some(args[0]),
                            "rotate" => angle = Some(args[0]),
                            _ => {}
                        }
                    }
                }
                "opacity" => alpha = d.value.trim().parse::<f64>().ok(),
                "animation-timing-function" => easing = Easing::parse(&d.value),
                // 未建模声明:原样保留(编辑不吞)
                _ => extras.push((d.prop.clone(), d.value.clone())),
            }
        }
        if let Some(p) = pos {
            push_kf(
                &mut m,
                TrackProp::Position,
                t,
                KeyValue::Offset(p.0, p.1),
                easing,
            );
        }
        if let Some(s) = factor {
            push_kf(&mut m, TrackProp::Scale, t, KeyValue::Factor(s), easing);
        }
        if let Some(a) = alpha {
            push_kf(&mut m, TrackProp::Opacity, t, KeyValue::Alpha(a), easing);
        }
        if let Some(d) = angle {
            push_kf(&mut m, TrackProp::Rotate, t, KeyValue::Angle(d), easing);
        }
        if !extras.is_empty() {
            m.extra.push((*off, extras));
        }
    }
    for (_, kfs) in m.tracks.iter_mut() {
        kfs.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
    AnimSource::Timeline(m)
}

fn push_kf(m: &mut AnimModel, p: TrackProp, t: f64, v: KeyValue, e: Easing) {
    if let Some((_, kfs)) = m.tracks.iter_mut().find(|(tp, _)| *tp == p) {
        kfs.push(Keyframe {
            t,
            value: v,
            easing: e,
        });
    } else {
        m.tracks.push((
            p,
            vec![Keyframe {
                t,
                value: v,
                easing: e,
            }],
        ));
    }
}

/// 对象静态值(无帧轨道「+ 关键帧」的取值来源):transform 函数 + opacity。
pub fn static_values(doc: &Document, sid: &str) -> ((f64, f64), f64, f64, f64) {
    let mut out = ((0.0, 0.0), 1.0, 1.0, 0.0); // (offset, factor, alpha, angle)
    let Some(nid) = doc.find_by_sid(sid) else {
        return out;
    };
    let node = doc.nodes.get(nid).unwrap();
    if let Some(tf) = node.style_get("transform") {
        for (name, args) in parse_transform_funcs(tf) {
            match name.as_str() {
                "translatex" => out.0 .0 = args[0],
                "translatey" => out.0 .1 = args[0],
                "scale" => out.1 = args[0],
                "rotate" => out.3 = args[0],
                _ => {}
            }
        }
    }
    if let Some(a) = node
        .style_get("opacity")
        .and_then(|v| v.trim().parse().ok())
    {
        out.2 = a;
    }
    out
}

/// 数字文本(去尾零;CSS 友好)。
fn fmt_num(v: f64) -> String {
    vb_common::units::fmt_num((v * 1000.0).round() / 1000.0)
}

#[cfg(test)]
mod time_position_tests {
    use super::keyframe_offset_permille;

    #[test]
    fn timeline_time_maps_to_css_permille_with_safe_bounds() {
        assert_eq!(keyframe_offset_permille(0.0, 2.0), Some(0));
        assert_eq!(keyframe_offset_permille(1.0, 2.0), Some(500));
        assert_eq!(keyframe_offset_permille(2.0, 2.0), Some(1000));
        assert_eq!(keyframe_offset_permille(1.002, 2.0), Some(501));
        assert_eq!(keyframe_offset_permille(-0.1, 2.0), None);
        assert_eq!(keyframe_offset_permille(2.1, 2.0), None);
        assert_eq!(keyframe_offset_permille(f64::NAN, 2.0), None);
        assert_eq!(keyframe_offset_permille(1.0, 0.0), None);
        assert_eq!(keyframe_offset_permille(1.0, f64::INFINITY), None);
    }
}
