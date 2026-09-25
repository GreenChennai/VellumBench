//! 05-9 动效时间轴面板(台账 09-I,ADR-VB-L11;副文档 05 §4.9)。
//!
//! **真相与求值单一化**:关键帧的真相 = 文档 CSS —— 每对象一个
//! `@keyframes vb-anim-<sid>` 块(raw_css 冻结块,往返 L0/L1 走既有通道)
//! + 节点 `animation` 简写。本模块只做「模型 ⇄ CSS」投影与编辑,预览与
//! 导出共用 `vb_kiln::anim::apply_frame_state` 同一求值入口(分叉在结构上
//! 不可能;一致性断言见 `vb_kiln/tests/anim_timeline.rs`)。
//!
//! **轨道定死**(分册 05-9-1「以文档模型实际支持的为准」):文档模型为
//! HTML 视觉文档,可动画量 = 平移 / 缩放 / 不透明度 / 旋转 四轨;分册原文
//! 提到的 volume 无对应建模(无音频节点、无音量声明),剔除 —— 引入音频
//! 建模时在此扩展 [`TrackProp`]。四轨落盘:平移/缩放/旋转 → `transform`
//! 函数(同一停靠点合并),不透明度 → `opacity`。
//!
//! **每帧缓动**(05-9-2):每关键帧携带 easing,落盘为停靠点内
//! `animation-timing-function`(CSS 语义:管「该帧 → 下一帧」段;求值侧
//! 见 `anim::track_value_at`)。封闭枚举 + cubic-bezier 自定义;自绘曲线
//! 编辑器承接 ADR-0003(渐变色标条)的自绘约定 —— 曲线预览用
//! `Timing::eval` 同一求解,只做预览不做拖柄编辑(Partial,见台账)。
//!
//! **外源动画诚实边界**:节点动画若引用非 `vb-anim-<sid>` 命名的关键帧
//! (导入内容常见),面板只读展示、不提供编辑 —— 防止整块重写吞掉
//! clip-path/filter 等本面板未建模的关键帧声明( [`AnimSource::Foreign`])。
//! 时间轴自己产物中的未建模声明(offset/声明原样保留,见 [`AnimModel::extra`])
//! 在编辑时随块保留。

use egui::{Color32, Rect, Stroke};

use vb_doc::commands::Command;
use vb_doc::model::Document;
use vb_kiln::anim::{parse_keyframes, parse_timing, resolve_node_anim, Timing};
use vb_ui::components::{caption, icon_button, NumField};
use vb_ui::icons::Name;
use vb_ui::theme;

use super::VellumApp;

// ─────────────────────── 模型(纯数据,门禁测试直接打这里) ───────────────────────

/// 轨道属性(定死,见模块注释:volume 无文档建模,剔除)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    pub fn label(self) -> &'static str {
        match self {
            TrackProp::Position => "位置",
            TrackProp::Scale => "缩放",
            TrackProp::Opacity => "不透明度",
            TrackProp::Rotate => "旋转",
        }
    }
}

/// 缓动:封闭枚举 + cubic-bezier 自定义(x1/x2 ∈ [0,1],y 可越界)。
#[derive(Debug, Clone, Copy, PartialEq)]
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

    pub fn label(&self) -> String {
        match self {
            Easing::Linear => "线性".into(),
            Easing::Ease => "ease".into(),
            Easing::EaseIn => "ease-in".into(),
            Easing::EaseOut => "ease-out".into(),
            Easing::EaseInOut => "ease-in-out".into(),
            Easing::Bezier(..) => self.css(),
        }
    }
}

/// 关键帧:t = 秒(0..=时长),值随轨道而异,每帧自带缓动。
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframe {
    pub t: f64,
    pub value: KeyValue,
    pub easing: Easing,
}

/// 关键帧值(按轨道;与 [`TrackProp`] 一一对应)。
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq, Default)]
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
            // 同停靠点多轨道各带缓动时取先到者(行序 = Position 先)
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

    let mut out = format!("@keyframes {} {{\n", vb_doc::commands::anim_block_name(sid));
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
    let iter = if m.iterations.is_infinite() {
        "infinite".to_string()
    } else {
        format!("{}", m.iterations.round().max(1.0) as i64)
    };
    format!(
        "vb-anim-{sid} {}ms linear {}ms {iter} both",
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
    if first != vb_doc::commands::anim_block_name(sid) {
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
                    for (name, args) in vb_kiln::anim::parse_transform_funcs(&d.value) {
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
fn static_values(doc: &Document, sid: &str) -> ((f64, f64), f64, f64, f64) {
    let mut out = ((0.0, 0.0), 1.0, 1.0, 0.0); // (offset, factor, alpha, angle)
    let Some(nid) = doc.find_by_sid(sid) else {
        return out;
    };
    let node = doc.nodes.get(nid).unwrap();
    if let Some(tf) = node.style_get("transform") {
        for (name, args) in vb_kiln::anim::parse_transform_funcs(tf) {
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

// ─────────────────────── 写回与播放会话(VellumApp) ───────────────────────

impl VellumApp {
    /// 模型写回(全帧清除 = 块 + 声明一起删;05-9-4)。走命令层可撤销。
    pub(crate) fn anim_write_model(&mut self, sid: &str, m: &AnimModel) {
        let cmd = if m.is_empty() {
            Command::SetNodeAnimation {
                sid: sid.to_string(),
                new_keyframes: None,
                new_animation: None,
                old: None,
            }
        } else {
            Command::SetNodeAnimation {
                sid: sid.to_string(),
                new_keyframes: Some(serialize_keyframes(sid, m)),
                new_animation: Some(serialize_animation(sid, m)),
                old: None,
            }
        };
        self.exec(cmd);
    }

    /// 当前选中对象的动画投影(无选中 / 画板 → None)。
    pub(crate) fn anim_source_of_selection(&self) -> Option<(String, AnimSource)> {
        let sid = self.selection.last()?.clone();
        let src = parse_anim_source(&self.doc, &sid);
        Some((sid, src))
    }

    /// 文档动画实例(rev 键缓存;与 ExportContext::build 同款遍历 ——
    /// 逐画板子树、sid 键;预览复用,免逐帧重复解析)。
    pub(crate) fn anim_instances(
        &mut self,
    ) -> std::collections::HashMap<String, vb_kiln::anim::NodeAnim> {
        let rev = self.doc.rev;
        if let Some((r, m)) = &self.anim_cache {
            if *r == rev {
                return m.clone();
            }
        }
        let kf = parse_keyframes(&self.doc.raw_css);
        let mut out = std::collections::HashMap::new();
        let mut stack: Vec<vb_doc::model::NodeId> = self.doc.artboards.to_vec();
        while let Some(nid) = stack.pop() {
            if let Some(node) = self.doc.nodes.get(nid) {
                if let Some(na) = vb_kiln::anim::resolve_node_anim(&self.doc, nid, &kf) {
                    out.insert(node.sid.as_str().to_string(), na);
                }
                stack.extend(node.children.iter().copied());
            }
        }
        self.anim_cache = Some((rev, out.clone()));
        out
    }

    /// 预览总时长(秒):全文档动画实例的 max(delay + duration × 迭代;
    /// 无限迭代按 1 轮计)。播放头/循环以此为准。
    pub(crate) fn anim_total_duration(&mut self) -> f64 {
        let anims = self.anim_instances();
        let mut total = 0.0f64;
        for na in anims.values() {
            for (inst, _) in &na.instances {
                let iters = if inst.iterations.is_infinite() {
                    1.0
                } else {
                    inst.iterations
                };
                total = total.max(inst.delay + inst.duration * iters);
            }
        }
        total
    }

    /// 预览是否生效(播放中或播放头不在 0 → 画布按求值结果呈现)。
    pub(crate) fn anim_preview_active(&self) -> bool {
        self.anim_playing || self.anim_time > 1e-6
    }

    /// 播放节拍(frame.rs 每帧调用):按真实 dt 推进播放头;
    /// 循环回绕 / 到尾暂停;无动画自动停。
    pub(crate) fn tick_anim_preview(&mut self, ctx: &egui::Context) {
        if !self.anim_playing {
            self.anim_last_clock = None;
            return;
        }
        let now = ctx.input(|i| i.time);
        let dt = self.anim_last_clock.map(|t0| now - t0).unwrap_or(0.0);
        self.anim_last_clock = Some(now);
        let total = self.anim_total_duration();
        if total <= 0.0 {
            self.anim_playing = false;
            return;
        }
        let mut t = self.anim_time + dt;
        if t >= total {
            if self.anim_loop {
                t %= total;
            } else {
                t = total;
                self.anim_playing = false;
            }
        }
        self.anim_time = t;
        ctx.request_repaint();
    }

    // ── 面板命令(dispatch_view 派发;登记 09-I 台账)──

    /// `anim.play_toggle`:播放 / 暂停(会话态,不进 undo)。
    pub(crate) fn anim_play_toggle(&mut self) {
        if self.anim_playing {
            self.anim_playing = false;
            self.say(format!("暂停 @ {:.2}s", self.anim_time));
        } else {
            let total = self.anim_total_duration();
            if total <= 0.0 {
                self.toast_warn("文档没有可播放的动画(先在时间轴加关键帧)");
                return;
            }
            if self.anim_time >= total {
                self.anim_time = 0.0;
            }
            self.anim_playing = true;
            self.say("播放动画预览(与导出同一求值路径)");
        }
    }

    /// `anim.stop`:停止并回零(回到静态呈现)。
    pub(crate) fn anim_stop(&mut self) {
        self.anim_playing = false;
        self.anim_time = 0.0;
        self.say("预览已停止(播放头回 0,画布恢复静态)");
    }

    /// `anim.loop_toggle`:循环开关(播放到尾回绕 / 停在末帧)。
    pub(crate) fn anim_loop_toggle(&mut self) {
        self.anim_loop = !self.anim_loop;
        self.say(if self.anim_loop {
            "循环播放:开"
        } else {
            "循环播放:关(到尾暂停)"
        });
    }

    /// `anim.keyframe_add`:在播放头处给选中对象**四轨补齐播放头快照**
    /// (语义定死:已有帧的时刻不重复加;新帧值 = 对象静态值;经
    /// SetNodeAnimation 走命令层可撤销)。
    pub(crate) fn anim_keyframe_add(&mut self) {
        let Some((sid, src)) = self.anim_source_of_selection() else {
            self.toast_warn("加关键帧:请先选中一个对象");
            return;
        };
        let AnimSource::Timeline(mut m) = src else {
            self.toast_warn("该对象的动画非时间轴命名,不能在此加帧");
            return;
        };
        let t = self.anim_time.clamp(0.0, m.duration_ms / 1000.0);
        let ((dx, dy), factor, alpha, angle) = static_values(&self.doc, &sid);
        let mut added = 0usize;
        for prop in TrackProp::ALL {
            let has_at_t = m.frames_of(prop).iter().any(|k| (k.t - t).abs() < 1e-4);
            if has_at_t {
                continue;
            }
            // 该轨完全没有帧且不是本次目标(未选中轨)时,只加有帧轨 +
            // 全加模式:这里按「四轨全部补齐播放头快照」语义,值 = 静态值
            let v = match prop {
                TrackProp::Position => KeyValue::Offset(dx, dy),
                TrackProp::Scale => KeyValue::Factor(factor),
                TrackProp::Opacity => KeyValue::Alpha(alpha),
                TrackProp::Rotate => KeyValue::Angle(angle),
            };
            if let Some((_, kfs)) = m.tracks.iter_mut().find(|(tp, _)| *tp == prop) {
                kfs.push(Keyframe {
                    t,
                    value: v,
                    easing: Easing::Linear,
                });
                kfs.sort_by(|a, b| a.t.total_cmp(&b.t));
            } else {
                m.tracks.push((
                    prop,
                    vec![Keyframe {
                        t,
                        value: v,
                        easing: Easing::Linear,
                    }],
                ));
            }
            added += 1;
        }
        if added == 0 {
            self.say("播放头处各轨道已有关键帧");
            return;
        }
        self.anim_write_model(&sid, &m);
        self.say(format!(
            "已在播放头 {:.2}s 加 {added} 个关键帧(Ctrl+Z 撤销)",
            self.anim_time
        ));
    }

    /// `anim.keyframe_delete`:删除时间轴选中的关键帧。
    pub(crate) fn anim_keyframe_delete(&mut self) {
        let Some((track, kf)) = self.anim_sel else {
            self.toast_warn("删除关键帧:先在时间轴上点选一个关键帧");
            return;
        };
        let Some((sid, src)) = self.anim_source_of_selection() else {
            return;
        };
        let AnimSource::Timeline(mut m) = src else {
            return;
        };
        let Some((_, kfs)) = m.tracks.iter_mut().find(|(tp, _)| *tp as usize == track) else {
            return;
        };
        if kf >= kfs.len() {
            self.anim_sel = None;
            return;
        }
        kfs.remove(kf);
        if kfs.is_empty() {
            m.tracks.remove(track);
        }
        self.anim_sel = None;
        self.anim_write_model(&sid, &m);
        self.say("已删除关键帧(Ctrl+Z 撤销)");
    }

    /// `anim.clear`:清除对象全部关键帧(= 移除对应 CSS,05-9-4)。
    pub(crate) fn anim_clear(&mut self) {
        let Some((sid, src)) = self.anim_source_of_selection() else {
            self.toast_warn("清除动画:请先选中一个对象");
            return;
        };
        match src {
            AnimSource::None => self.say("该对象没有动画"),
            AnimSource::Foreign(name) => {
                self.toast_warn(format!(
                    "动画「{name}」来自外部命名关键帧,请手工编辑 CSS 或改名后处理"
                ));
            }
            AnimSource::Timeline(_) => {
                self.anim_write_model(
                    &sid,
                    &AnimModel {
                        duration_ms: 1000.0,
                        delay_ms: 0.0,
                        iterations: 1.0,
                        tracks: Vec::new(),
                        extra: Vec::new(),
                    },
                );
                self.anim_sel = None;
                self.say("已清除对象动画(@keyframes 与 animation 声明一并移除;Ctrl+Z 撤销)");
            }
        }
    }
}

// ─────────────────────── 面板正文(次级坞「时间轴」组) ───────────────────────

/// 轨道行高 / 播放头半宽(命中判定用)。
const LANE_H: f32 = 26.0;
const HEAD_GRAB: f32 = 6.0;

impl VellumApp {
    pub(crate) fn timeline_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some((sid, src)) = self.anim_source_of_selection() else {
            // U-5:时间轴空态 = 统一「图标 + 一句短话 + 动作按钮」
            let t = vb_ui::theme::tokens(ui.ctx());
            ui.add_space(vb_ui::theme::space::S3);
            ui.horizontal(|ui| {
                ui.add_space(vb_ui::theme::space::S2);
                ui.label(vb_ui::icons::rich(vb_ui::icons::Name::Play, 18.0).color(t.text_3));
                ui.label("未选中对象 —— 选中一个对象后可为它编排关键帧动画。");
            });
            ui.add_space(vb_ui::theme::space::S2);
            ui.horizontal_wrapped(|ui| {
                if vb_ui::components::icon_button(
                    ui,
                    vb_ui::icons::Name::ToolSelect,
                    "选择工具(V):点选要动画的对象",
                )
                .clicked()
                {
                    self.run_command("tool.select", false, false);
                }
                if ui.button("播放文档动画").clicked() {
                    self.run_command("anim.play_toggle", false, false);
                }
            });
            return;
        };
        let AnimSource::Timeline(mut model) = src else {
            match parse_anim_source(&self.doc, &sid) {
                AnimSource::Foreign(name) => {
                    ui.label(caption(
                        ui,
                        &format!("该对象的动画引用外部关键帧「{name}」。\n时间轴只编辑 vb-anim-<id> 命名的动画(防吞导入内容);可用「对象 → 动画 → 清除动画」移除后重排。"),
                    ));
                    if ui.button("清除动画").clicked() {
                        self.anim_clear();
                    }
                }
                _ => {
                    ui.label(caption(ui, "对象已不存在。"));
                }
            }
            return;
        };

        let total_s = (model.duration_ms / 1000.0).max(0.001);
        let mut dirty = false;

        // ── 播放控制行 ──
        ui.horizontal(|ui| {
            let play_icon = if self.anim_playing {
                Name::Pause
            } else {
                Name::Play
            };
            if icon_button(
                ui,
                play_icon,
                if self.anim_playing {
                    "暂停"
                } else {
                    "播放"
                },
            )
            .clicked()
            {
                self.anim_play_toggle();
            }
            if icon_button(ui, Name::Close, "停止并回零").clicked() {
                self.anim_stop();
            }
            if ui
                .selectable_label(self.anim_loop, "循环")
                .on_hover_text("循环播放(关 = 到尾暂停)")
                .clicked()
            {
                self.anim_loop_toggle();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{:.2}s / {:.2}s",
                        self.anim_time.min(total_s),
                        total_s
                    ))
                    .monospace(),
                );
            });
        });

        // ── 时序参数(时长 / 延迟 / 次数;窄坞两行排布)──
        ui.horizontal(|ui| {
            let mut dur = model.duration_ms;
            let r = NumField::new("时长", &mut dur)
                .unit("ms")
                .speed(10.0)
                .step(100.0)
                .range(100.0, 60000.0)
                .width(64.0)
                .ui(ui);
            if r.changed {
                model.duration_ms = dur;
                dirty = true;
            }
            let mut delay = model.delay_ms;
            let r = NumField::new("延迟", &mut delay)
                .unit("ms")
                .speed(10.0)
                .step(100.0)
                .range(0.0, 10000.0)
                .width(56.0)
                .ui(ui);
            if r.changed {
                model.delay_ms = delay;
                dirty = true;
            }
        });
        ui.horizontal(|ui| {
            let mut iters = if model.iterations.is_infinite() {
                1.0
            } else {
                model.iterations
            };
            let r = NumField::new("次数", &mut iters)
                .speed(0.2)
                .step(1.0)
                .range(1.0, 50.0)
                .width(52.0)
                .ui(ui);
            if r.changed {
                model.iterations = iters.round().max(1.0);
                dirty = true;
            }
            if ui
                .button("清除动画")
                .on_hover_text("移除全部关键帧(= 删除对应 CSS)")
                .clicked()
            {
                self.anim_clear();
            }
        });

        ui.add_space(theme::space::S2);
        ui.separator();

        // ── 时间刻度 + 轨道(4 行 + 刻度 ≈ 130px,直排不滚动 ——
        // 滚动区宽度核算会挤占 lane 导致溢出坞边,实测后改直排)──
        dirty |= self.timeline_lanes(ui, &sid, &mut model, total_s);

        ui.separator();

        // ── 选中关键帧编辑 ──
        self.timeline_kf_editor(ui, &mut model, &sid, &mut dirty);
        if dirty {
            self.anim_write_model(&sid, &model);
        }
    }

    /// 时间刻度 + 播放头 + 轨道行。返回是否有编辑(写回)。
    fn timeline_lanes(
        &mut self,
        ui: &mut egui::Ui,
        sid: &str,
        model: &mut AnimModel,
        total_s: f64,
    ) -> bool {
        let mut dirty = false;
        let avail = ui.available_width();
        let label_w = 52.0f32;
        // 轨道原点对齐:行头固定 label_w,刻度行留同宽空位 → 刻度与
        // 轨道同 x 基准(scrub / 播放头 / 关键帧共用一套 t↔x 映射)
        let lane_w = (avail - label_w - 12.0).max(80.0);
        let tokens = theme::tokens(ui.ctx());

        // ── 刻度行(scrub 播放头;与轨道同原点)──
        ui.horizontal(|ui| {
            ui.allocate_exact_size(egui::vec2(label_w, 18.0), egui::Sense::hover());
            let (ruler_rect, _) =
                ui.allocate_exact_size(egui::vec2(lane_w, 18.0), egui::Sense::click_and_drag());
            let painter = ui.painter_at(ruler_rect);
            painter.rect_filled(ruler_rect, 2.0, tokens.bg_panel);
            let t_of_x =
                |x: f32| ((x - ruler_rect.left()) / lane_w).clamp(0.0, 1.0) as f64 * total_s;
            if ruler_rect.contains(ui.input(|i| i.pointer.hover_pos().unwrap_or_default())) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            let scrub = ui.interact(ruler_rect, ui.id().with("vb-tl-ruler"), egui::Sense::drag());
            if scrub.dragged() {
                if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
                    self.anim_time = t_of_x(p.x);
                    self.anim_playing = false;
                }
            }
            // 刻度:主刻度带标签,次刻度细线;标签步距按像素密度自适应
            // (窄坞 6s 一行放不下 0.5s 间隔的 9pt 文本,实测会互相叠字)
            let px_per_s = lane_w as f64 / total_s;
            let label_step: f64 = if px_per_s >= 70.0 {
                0.5
            } else if px_per_s >= 34.0 {
                1.0
            } else {
                2.0
            };
            let minor: f64 = (label_step / 5.0).max(0.1);
            let mut tick = 0.0f64;
            while tick <= total_s + 1e-6 {
                let x = ruler_rect.left() + (tick / total_s) as f32 * lane_w;
                let is_major = (tick / label_step - (tick / label_step).round()).abs() < 1e-6;
                painter.line_segment(
                    [
                        egui::pos2(x, ruler_rect.bottom() - if is_major { 8.0 } else { 4.0 }),
                        egui::pos2(x, ruler_rect.bottom()),
                    ],
                    Stroke::new(1.0, tokens.text_3),
                );
                if is_major {
                    painter.text(
                        egui::pos2(x + 2.0, ruler_rect.top() + 1.0),
                        egui::Align2::LEFT_TOP,
                        format!("{tick:.1}s"),
                        egui::FontId::monospace(9.0),
                        tokens.text_3,
                    );
                }
                tick += minor;
            }
            draw_playhead(
                &painter,
                ruler_rect,
                self.anim_time,
                total_s,
                lane_w,
                tokens.accent,
            );
        });

        // ── 轨道行 ──
        for (row, prop) in TrackProp::ALL.into_iter().enumerate() {
            ui.horizontal(|ui| {
                // 行头(固定宽自绘标签):点选 = 画布联动选中(双向联动的
                // 一半;画布选中改变时本面板随之换对象,是另一半)
                let (head, head_resp) =
                    ui.allocate_exact_size(egui::vec2(label_w, LANE_H), egui::Sense::click());
                let hp = ui.painter_at(head);
                let active = self.anim_sel.map(|(t, _)| t) == Some(row);
                hp.text(
                    egui::pos2(head.left(), head.center().y),
                    egui::Align2::LEFT_CENTER,
                    prop.label(),
                    egui::FontId::proportional(11.0),
                    if active { tokens.accent } else { tokens.text_2 },
                );
                if head_resp.clicked() {
                    self.selection = vec![sid.to_string()];
                }
                let (lane, resp) = ui
                    .allocate_exact_size(egui::vec2(lane_w, LANE_H), egui::Sense::click_and_drag());
                // 放开 4px 裁剪:0%/100% 边缘菱形中心在车道边界,完整可见
                let p = ui.painter_at(lane.expand(4.0));
                p.rect_filled(lane, 2.0, tokens.bg_canvas);
                // 轨道基线
                p.line_segment(
                    [
                        egui::pos2(lane.left(), lane.center().y),
                        egui::pos2(lane.right(), lane.center().y),
                    ],
                    Stroke::new(1.0, tokens.text_3.gamma_multiply(0.4)),
                );
                let kfs = model.frames_of(prop);
                let x_of_t = |t: f64| lane.left() + (t / total_s).clamp(0.0, 1.0) as f32 * lane_w;
                // 关键帧菱形
                for (i, kf) in kfs.iter().enumerate() {
                    let cx = x_of_t(kf.t);
                    let cy = lane.center().y;
                    let sel = self.anim_sel == Some((row, i));
                    let color = if sel { tokens.accent } else { tokens.text };
                    let d = if sel { 6.0 } else { 5.0 };
                    let pts = [
                        egui::pos2(cx, cy - d),
                        egui::pos2(cx + d, cy),
                        egui::pos2(cx, cy + d),
                        egui::pos2(cx - d, cy),
                    ];
                    p.add(egui::Shape::convex_polygon(
                        pts.to_vec(),
                        color,
                        Stroke::NONE,
                    ));
                }
                // 交互:点选 / 拖移 / 双击加帧 / Alt+点击删帧
                let pointer = ui.input(|i| i.pointer.hover_pos());
                if let Some(hp) = pointer {
                    if lane.contains(hp) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
                if resp.double_clicked() {
                    if let Some(hp) = ui.input(|i| i.pointer.hover_pos()) {
                        let t = (((hp.x - lane.left()) / lane_w).clamp(0.0, 1.0) as f64 * total_s)
                            .min(total_s);
                        timeline_add_kf_at(model, prop, t, &self.doc, sid);
                        self.anim_sel = Some((row, model.frames_of(prop).len() - 1));
                        dirty = true;
                    }
                } else if resp.clicked() {
                    if let Some(hp) = ui.input(|i| i.pointer.hover_pos()) {
                        // Alt+点击命中菱形 = 删除
                        if ui.input(|i| i.modifiers.alt) {
                            if let Some(i) = hit_kf(kfs, hp.x, x_of_t) {
                                let (_, kf_list) =
                                    model.tracks.iter_mut().find(|(tp, _)| *tp == prop).unwrap();
                                kf_list.remove(i);
                                self.anim_sel = None;
                                dirty = true;
                            }
                        } else {
                            self.anim_sel = hit_kf(kfs, hp.x, x_of_t).map(|i| (row, i));
                        }
                    }
                } else if resp.dragged()
                    && self.anim_sel.is_some()
                    && self.anim_sel.map(|(t, _)| t) == Some(row)
                {
                    // 拖拽移动关键帧(undo 会话合并为一条)
                    if !self.anim_drag_open {
                        self.anim_drag_open = true;
                        self.undo.begin_session();
                    }
                    if let Some(hp) = ui.input(|i| i.pointer.hover_pos()) {
                        let t = (((hp.x - lane.left()) / lane_w).clamp(0.0, 1.0) as f64 * total_s)
                            .min(total_s);
                        if let Some((_, kf_list)) =
                            model.tracks.iter_mut().find(|(tp, _)| *tp == prop)
                        {
                            if let Some(k) =
                                self.anim_sel.map(|(_, i)| i).filter(|&i| i < kf_list.len())
                            {
                                kf_list[k].t = t;
                                kfs_sort(kf_list);
                            }
                        }
                        dirty = true;
                    }
                } else if resp.drag_stopped() && self.anim_drag_open {
                    self.anim_drag_open = false;
                    self.undo.end_session();
                }
                // 拖拽中的播放头参考线
                draw_playhead(&p, lane, self.anim_time, total_s, lane_w, tokens.accent);
            });
        }
        // 拖拽会话兜底收口(指针移出行后松手)
        if self.anim_drag_open && !ui.input(|i| i.pointer.primary_down()) {
            self.anim_drag_open = false;
            self.undo.end_session();
        }
        ui.label(caption(
            ui,
            "双击轨道加关键帧 · 点选后拖动改时刻 · Alt+点击删帧 · 拖顶部刻度 scrub",
        ));
        dirty
    }

    /// 选中关键帧的值 / 缓动编辑(05-9-2;自绘缓动曲线承接 ADR-0003)。
    fn timeline_kf_editor(
        &mut self,
        ui: &mut egui::Ui,
        model: &mut AnimModel,
        _sid: &str,
        dirty: &mut bool,
    ) {
        let Some((track, kfi)) = self.anim_sel else {
            ui.label(caption(ui, "选中轨道上的关键帧后可改值与缓动。"));
            return;
        };
        let Some((prop, kfs)) = model
            .tracks
            .iter_mut()
            .find(|(tp, _)| *tp as usize == track)
        else {
            self.anim_sel = None;
            return;
        };
        if kfi >= kfs.len() {
            self.anim_sel = None;
            return;
        }
        let prop_label = prop.label();
        ui.horizontal(|ui| {
            ui.label(format!("关键帧 {kfi}/{} · {prop_label}", kfs.len()));
            if ui.button("删除此帧").clicked() {
                kfs.remove(kfi);
                self.anim_sel = None;
                *dirty = true;
            }
        });
        let kf = &mut kfs[kfi];
        // 值(随轨道类型)
        match &mut kf.value {
            KeyValue::Offset(dx, dy) => {
                ui.horizontal(|ui| {
                    let r = NumField::new("X", dx)
                        .unit("px")
                        .speed(2.0)
                        .range(-10000.0, 10000.0)
                        .width(70.0)
                        .ui(ui);
                    *dirty |= r.changed;
                    let r = NumField::new("Y", dy)
                        .unit("px")
                        .speed(2.0)
                        .range(-10000.0, 10000.0)
                        .width(70.0)
                        .ui(ui);
                    *dirty |= r.changed;
                });
            }
            KeyValue::Factor(s) => {
                let r = NumField::new("因子", s)
                    .speed(0.02)
                    .step(0.1)
                    .range(0.01, 20.0)
                    .width(70.0)
                    .ui(ui);
                *dirty |= r.changed;
            }
            KeyValue::Alpha(a) => {
                let r = NumField::new("不透明度", a)
                    .speed(0.01)
                    .step(0.05)
                    .range(0.0, 1.0)
                    .width(70.0)
                    .ui(ui);
                *dirty |= r.changed;
            }
            KeyValue::Angle(d) => {
                let r = NumField::new("角度", d)
                    .unit("deg")
                    .speed(1.0)
                    .step(5.0)
                    .range(-3600.0, 3600.0)
                    .width(70.0)
                    .ui(ui);
                *dirty |= r.changed;
            }
        }
        // 缓动:封闭枚举 + 自定义
        ui.horizontal(|ui| {
            ui.label("缓动");
            for e in Easing::PRESETS {
                if ui
                    .selectable_label(kf.easing == e, e.label())
                    .on_hover_text(e.css())
                    .clicked()
                {
                    kf.easing = e;
                    *dirty = true;
                }
            }
            let custom = matches!(kf.easing, Easing::Bezier(..));
            if ui.selectable_label(custom, "自定义").clicked() {
                if !custom {
                    kf.easing = Easing::Bezier(0.42, 0.0, 0.58, 1.0);
                }
                *dirty = true;
            }
        });
        if let Easing::Bezier(x1, y1, x2, y2) = &mut kf.easing {
            let (mut bx1, mut by1, mut bx2, mut by2) = (*x1, *y1, *x2, *y2);
            let mut changed = false;
            ui.horizontal(|ui| {
                let r = NumField::new("x1", &mut bx1)
                    .speed(0.01)
                    .step(0.1)
                    .range(-2.0, 2.0)
                    .width(56.0)
                    .ui(ui);
                changed |= r.changed;
                let r = NumField::new("y1", &mut by1)
                    .speed(0.01)
                    .step(0.1)
                    .range(-2.0, 2.0)
                    .width(56.0)
                    .ui(ui);
                changed |= r.changed;
                let r = NumField::new("x2", &mut bx2)
                    .speed(0.01)
                    .step(0.1)
                    .range(-2.0, 2.0)
                    .width(56.0)
                    .ui(ui);
                changed |= r.changed;
                let r = NumField::new("y2", &mut by2)
                    .speed(0.01)
                    .step(0.1)
                    .range(-2.0, 2.0)
                    .width(56.0)
                    .ui(ui);
                changed |= r.changed;
            });
            if changed {
                *x1 = bx1;
                *y1 = by1;
                *x2 = bx2;
                *y2 = by2;
                *dirty = true;
            }
            bezier_curve_preview(ui, bx1, by1, bx2, by2);
            ui.label(caption(
                ui,
                "cubic-bezier 曲线预览(与预览/导出同一求解;拖柄编辑留后续)",
            ));
        }
    }
}

/// 时间轴上「在 t 处加帧」(值 = 对象静态值;面板双击与命令共用)。
fn timeline_add_kf_at(model: &mut AnimModel, prop: TrackProp, t: f64, doc: &Document, sid: &str) {
    let ((dx, dy), factor, alpha, angle) = static_values(doc, sid);
    let v = match prop {
        TrackProp::Position => KeyValue::Offset(dx, dy),
        TrackProp::Scale => KeyValue::Factor(factor),
        TrackProp::Opacity => KeyValue::Alpha(alpha),
        TrackProp::Rotate => KeyValue::Angle(angle),
    };
    if let Some((_, kfs)) = model.tracks.iter_mut().find(|(tp, _)| *tp == prop) {
        kfs.push(Keyframe {
            t,
            value: v,
            easing: Easing::Linear,
        });
        kfs_sort(kfs);
    } else {
        model.tracks.push((
            prop,
            vec![Keyframe {
                t,
                value: v,
                easing: Easing::Linear,
            }],
        ));
    }
}

/// 命中判定:x 附近 HEAD_GRAB 内最近的帧下标。
fn hit_kf(kfs: &[Keyframe], x: f32, x_of_t: impl Fn(f64) -> f32) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (i, kf) in kfs.iter().enumerate() {
        let d = (x_of_t(kf.t) - x).abs();
        if d <= HEAD_GRAB && best.map(|(bd, _)| d < bd).unwrap_or(true) {
            best = Some((d, i));
        }
    }
    best.map(|(_, i)| i)
}

fn kfs_sort(kfs: &mut [Keyframe]) {
    kfs.sort_by(|a, b| a.t.total_cmp(&b.t));
}

/// 播放头(竖线 + 顶部三角)。
fn draw_playhead(
    p: &egui::Painter,
    rect: Rect,
    t: f64,
    total_s: f64,
    lane_w: f32,
    accent: Color32,
) {
    if total_s <= 0.0 {
        return;
    }
    let x = rect.left() + (t / total_s).clamp(0.0, 1.0) as f32 * lane_w;
    p.line_segment(
        [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
        Stroke::new(1.5, accent),
    );
    let tri = [
        egui::pos2(x - 4.0, rect.top()),
        egui::pos2(x + 4.0, rect.top()),
        egui::pos2(x, rect.top() + 5.0),
    ];
    p.add(egui::Shape::convex_polygon(
        tri.to_vec(),
        accent,
        Stroke::NONE,
    ));
}

/// cubic-bezier 自绘曲线预览(ADR-0003 自绘约定;求解复用 Timing::eval)。
fn bezier_curve_preview(ui: &mut egui::Ui, x1: f64, y1: f64, x2: f64, y2: f64) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 64.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let tokens = theme::tokens(ui.ctx());
    p.rect_filled(rect, 3.0, tokens.bg_canvas);
    // 网格:1/4 分隔
    for i in 1..4 {
        let x = rect.left() + rect.width() * i as f32 / 4.0;
        let y = rect.top() + rect.height() * i as f32 / 4.0;
        p.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(0.5, tokens.text_3.gamma_multiply(0.3)),
        );
        p.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            Stroke::new(0.5, tokens.text_3.gamma_multiply(0.3)),
        );
    }
    let timing = Timing::CubicBezier(x1, y1, x2, y2);
    let n = 48;
    let mut pts = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let x = i as f64 / n as f64;
        let y = timing.eval(x);
        // y 允许越界(回弹),钳到绘制区 ±50%
        let y = y.clamp(-0.5, 1.5);
        let px = rect.left() + (x as f32) * rect.width();
        let py = rect.bottom() - ((y as f32 - -0.5) / 2.0) * rect.height();
        pts.push(egui::pos2(px, py));
    }
    let shape = egui::epaint::PathShape::line(pts, Stroke::new(2.0, tokens.accent));
    p.add(egui::Shape::Path(shape));
}

// ─────────────────────── 门禁(单测:序列化 / 投影 / 写回 / 级联) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::commands::anim_block_name;
    use vb_doc::model::{Document, NodeKind};

    /// 构造:文档 + 画板 + 一个 200×100 盒(sid 返回)。
    fn doc_with_box() -> (Document, String) {
        let mut doc = Document::new_default();
        let ab = doc.artboards[0];
        let sid = doc.alloc_sid();
        let parent_sid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
        let mut node = vb_doc::model::Node::new(NodeKind::Box, "动盒", sid.clone());
        node.geom = vb_doc::model::Geom {
            x: 40.0,
            y: 40.0,
            w: 200.0,
            h: 100.0,
        };
        node.style = vec![vb_css::Decl {
            prop: "background-color".into(),
            value: "#334455".into(),
            important: false,
        }];
        let tree = vb_doc::model::NodeTree {
            node,
            children: vec![],
        };
        doc.insert_tree_at(&tree, &parent_sid, 0).expect("插盒成功");
        doc.sync_artboards();
        (doc, sid.as_str().to_string())
    }

    fn sample_model() -> AnimModel {
        // 位置 0→2s 两帧 + 不透明度 1s 单帧 + 旋转 2s 帧(多属性合并停靠点)
        AnimModel {
            duration_ms: 2000.0,
            delay_ms: 0.0,
            iterations: 1.0,
            tracks: vec![
                (
                    TrackProp::Position,
                    vec![
                        Keyframe {
                            t: 0.0,
                            value: KeyValue::Offset(0.0, 0.0),
                            easing: Easing::Linear,
                        },
                        Keyframe {
                            t: 2.0,
                            value: KeyValue::Offset(120.0, -40.0),
                            easing: Easing::Bezier(0.25, 0.1, 0.25, 1.0),
                        },
                    ],
                ),
                (
                    TrackProp::Opacity,
                    vec![
                        Keyframe {
                            t: 0.0,
                            value: KeyValue::Alpha(1.0),
                            easing: Easing::Linear,
                        },
                        Keyframe {
                            t: 1.0,
                            value: KeyValue::Alpha(0.5),
                            easing: Easing::Linear,
                        },
                        Keyframe {
                            t: 2.0,
                            value: KeyValue::Alpha(0.0),
                            easing: Easing::Linear,
                        },
                    ],
                ),
                (
                    TrackProp::Rotate,
                    vec![Keyframe {
                        t: 2.0,
                        value: KeyValue::Angle(90.0),
                        easing: Easing::Linear,
                    }],
                ),
            ],
            extra: Vec::new(),
        }
    }

    /// 05-9-4:序列化产出合法 CSS 结构(块名 / 停靠点合并 / 帧内缓动)。
    #[test]
    fn serialize_merges_multi_property_stops() {
        let (doc, sid) = doc_with_box();
        let m = sample_model();
        let block = serialize_keyframes(&sid, &m);
        let head = format!("@keyframes {} {{", anim_block_name(&sid));
        assert!(block.starts_with(&head), "块名必须正确:{block}");
        // 位置/旋转同在 100% 停靠点:transform 一条声明含 translateX 与 rotate
        assert!(
            block.contains("100% { transform: translateX(120px) translateY(-40px) rotate(90deg);"),
            "停靠点必须合并 transform 函数:{block}"
        );
        // 帧内缓动落盘
        assert!(block.contains("animation-timing-function: cubic-bezier(0.25, 0.1, 0.25, 1)"));
        // 简写:名字 + ms + fill both
        let sh = serialize_animation(&sid, &m);
        assert_eq!(sh, format!("vb-anim-{sid} 2000ms linear 0ms 1 both"));
        // 块能被导出期解析器吃回去(合法 CSS)
        let kf = parse_keyframes(std::slice::from_ref(&block));
        assert!(
            kf.contains_key(&anim_block_name(&sid)),
            "序列化块必须可被 parse_keyframes 解析"
        );
        let _ = doc;
    }

    /// 05-9-6①:模型 → CSS → 模型 往返稳定(投影 == 原模型)。
    #[test]
    fn model_css_model_roundtrip_is_stable() {
        let (mut doc, sid) = doc_with_box();
        let m = sample_model();
        doc.raw_css.push(serialize_keyframes(&sid, &m));
        if let Some(id) = doc.find_by_sid(&sid) {
            doc.nodes
                .get_mut(id)
                .unwrap()
                .style_set("animation", &serialize_animation(&sid, &m));
        }
        match parse_anim_source(&doc, &sid) {
            AnimSource::Timeline(m2) => {
                assert!((m2.duration_ms - 2000.0).abs() < 1e-6);
                assert!((m2.delay_ms - 0.0).abs() < 1e-6);
                // 位置轨 2 帧,值往返一致(顺序按 t 升序)
                let pos = m2.frames_of(TrackProp::Position);
                assert_eq!(pos.len(), 2);
                assert_eq!(pos[0].value, KeyValue::Offset(0.0, 0.0));
                assert_eq!(pos[1].value, KeyValue::Offset(120.0, -40.0));
                // 帧内缓动往返
                assert_eq!(pos[1].easing, Easing::Bezier(0.25, 0.1, 0.25, 1.0));
                // 不透明度 3 帧 / 旋转 1 帧
                assert_eq!(m2.frames_of(TrackProp::Opacity).len(), 3);
                assert_eq!(m2.frames_of(TrackProp::Rotate).len(), 1);
                // 再序列化幂等(同一停靠点集合)
                assert_eq!(
                    serialize_keyframes(&sid, &m2),
                    serialize_keyframes(&sid, &m)
                );
            }
            other => panic!("必须是 Timeline 投影:{other:?}"),
        }
    }

    /// 05-9-6①(续):写回经命令层,undo 精确还原 raw_css 与声明;
    /// 清除(全 None)= 块与声明一起消失;redo 重放。
    #[test]
    fn write_clear_undo_redo_via_command_layer() {
        use vb_doc::undo::UndoStack;
        let (mut doc, sid) = doc_with_box();
        let m = sample_model();
        let mut undo = UndoStack::new();
        // 连续写回同 sid 在合并窗口内默认并成一条(生产语义);本测试
        // 要分步断言撤销/重做,先关合并(与历史面板测试同款手法)
        undo.merging_enabled = false;
        let block = serialize_keyframes(&sid, &m);
        let sh = serialize_animation(&sid, &m);
        undo.push(
            &mut doc,
            Command::SetNodeAnimation {
                sid: sid.clone(),
                new_keyframes: Some(block.clone()),
                new_animation: Some(sh.clone()),
                old: None,
            },
        )
        .unwrap();
        assert!(doc
            .raw_css
            .iter()
            .any(|b| b.trim_start().starts_with("@keyframes vb-anim-")));
        assert_eq!(
            doc.find_by_sid(&sid).map(|id| doc
                .nodes
                .get(id)
                .unwrap()
                .style_get("animation")
                .map(str::to_string)),
            Some(Some(sh.clone()))
        );
        // 清除
        undo.push(
            &mut doc,
            Command::SetNodeAnimation {
                sid: sid.clone(),
                new_keyframes: None,
                new_animation: None,
                old: None,
            },
        )
        .unwrap();
        assert!(!doc.raw_css.iter().any(|b| b.contains("vb-anim-")));
        assert_eq!(
            doc.find_by_sid(&sid).map(|id| doc
                .nodes
                .get(id)
                .unwrap()
                .style_get("animation")
                .is_none()),
            Some(true)
        );
        // 撤销清除 → 恢复;再撤销写入 → 全空;重做各一次
        undo.undo(&mut doc).unwrap();
        assert!(doc.raw_css.iter().any(|b| b.contains("vb-anim-")));
        undo.undo(&mut doc).unwrap();
        assert!(!doc.raw_css.iter().any(|b| b.contains("vb-anim-")));
        undo.redo(&mut doc).unwrap();
        assert!(doc.raw_css.iter().any(|b| b.contains("vb-anim-")));
    }

    /// 05-9-6④:删除动画对象 → @keyframes 块级联清理;撤销 → 块按原样
    /// 放回(节点内联 animation 声明随子树快照还原)。
    #[test]
    fn delete_cascades_keyframes_and_undo_restores() {
        use vb_doc::undo::UndoStack;
        let (mut doc, sid) = doc_with_box();
        let m = sample_model();
        let mut undo = UndoStack::new();
        undo.merging_enabled = false;
        undo.push(
            &mut doc,
            Command::SetNodeAnimation {
                sid: sid.clone(),
                new_keyframes: Some(serialize_keyframes(&sid, &m)),
                new_animation: Some(serialize_animation(&sid, &m)),
                old: None,
            },
        )
        .unwrap();
        assert!(doc.raw_css.iter().any(|b| b.contains("vb-anim-")));
        // 删除对象
        undo.push(
            &mut doc,
            Command::Delete {
                target_sid: sid.clone(),
                captured: None,
            },
        )
        .unwrap();
        assert!(
            !doc.raw_css.iter().any(|b| b.contains("vb-anim-")),
            "删除动画对象必须级联清理 @keyframes 块"
        );
        // 撤销 → 对象与块都回来
        undo.undo(&mut doc).unwrap();
        assert!(doc.find_by_sid(&sid).is_some(), "撤销删除后对象回来");
        assert!(
            doc.raw_css.iter().any(|b| b.contains("vb-anim-")),
            "撤销删除后 @keyframes 块必须放回"
        );
        // 重做 → 再清
        undo.redo(&mut doc).unwrap();
        assert!(!doc.raw_css.iter().any(|b| b.contains("vb-anim-")));
    }

    /// 外源动画诚实边界:非 vb-anim-<sid> 命名 → Foreign(只读)。
    #[test]
    fn foreign_named_animation_is_readonly() {
        let (mut doc, sid) = doc_with_box();
        doc.raw_css
            .push("@keyframes rise-in { from { opacity: 0; } to { opacity: 1; } }".into());
        if let Some(id) = doc.find_by_sid(&sid) {
            doc.nodes
                .get_mut(id)
                .unwrap()
                .style_set("animation", "rise-in 1s ease both");
        }
        assert!(
            matches!(parse_anim_source(&doc, &sid), AnimSource::Foreign(ref n) if n == "rise-in")
        );
    }

    /// 05-9-2:缓动 CSS 双向(封闭枚举 + 自定义贝塞尔)。
    #[test]
    fn easing_css_roundtrip() {
        for e in Easing::PRESETS {
            assert_eq!(Easing::parse(&e.css()), e);
        }
        let b = Easing::Bezier(0.25, 0.1, 0.25, 1.0);
        assert_eq!(Easing::parse(&b.css()), b);
        assert_eq!(Easing::parse("garbage"), Easing::Linear);
    }
}
