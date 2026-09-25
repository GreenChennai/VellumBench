//! 字符/段落面板 · 纯函数层:段落对齐九式、字符构建器、区域文本溢出。
//!
//! 06-1 自 `panels/charpara.rs` 按「模型 / 字段表 / 渲染」拆出(纯搬移,零行为变化):
//! 字段清单与规格在 `fields`,渲染在 `char`/`para`,门禁测试在 `tests`。

use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, NodeId, NodeKind, SegStyle, TextMode, TextSeg};

use crate::app::control_panel::{style_prop_cmds, style_prop_remove_cmds};
use crate::app::set_style_prop;

// ═══════════════════ 1. 段落对齐九式(design/03 §5.10) ═══════════════════

/// 段落对齐九式。每式对应一组**互不相同**的白名单声明组合
/// (`text-align` + `text-align-last`),投影 [`Align9::from_style`] 与
/// 写回 [`Align9::decls`] 对称 → 往返幂等。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align9 {
    /// 左对齐
    Left,
    /// 居中对齐
    Center,
    /// 右对齐
    Right,
    /// 两端对齐 · 末行左(`text-align:justify` 的 CSS 默认末行行为)
    Justify,
    /// 两端对齐 · 末行居中
    JustifyLastCenter,
    /// 两端对齐 · 末行右
    JustifyLastRight,
    /// 两端对齐 · 末行两端
    JustifyLastJustify,
    /// 全部两端(含末行;`text-align:justify-all`)
    JustifyAll,
    /// 强制撑满(全部两端 + 末行两端声明)
    JustifyAllLast,
}

impl Align9 {
    pub const ALL: [Align9; 9] = [
        Align9::Left,
        Align9::Center,
        Align9::Right,
        Align9::Justify,
        Align9::JustifyLastCenter,
        Align9::JustifyLastRight,
        Align9::JustifyLastJustify,
        Align9::JustifyAll,
        Align9::JustifyAllLast,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Align9::Left => "左对齐",
            Align9::Center => "居中对齐",
            Align9::Right => "右对齐",
            Align9::Justify => "两端对齐·末行左",
            Align9::JustifyLastCenter => "两端对齐·末行居中",
            Align9::JustifyLastRight => "两端对齐·末行右",
            Align9::JustifyLastJustify => "两端对齐·末行两端",
            Align9::JustifyAll => "全部两端(含末行)",
            Align9::JustifyAllLast => "强制撑满(末行两端)",
        }
    }

    /// 按钮短符号(段落面板 9 式网格)。
    pub fn short(self) -> &'static str {
        match self {
            Align9::Left => "⇤",
            Align9::Center => "↔",
            Align9::Right => "⇥",
            Align9::Justify => "≡",
            Align9::JustifyLastCenter => "≡·中",
            Align9::JustifyLastRight => "≡·右",
            Align9::JustifyLastJustify => "≡≡",
            Align9::JustifyAll => "▮",
            Align9::JustifyAllLast => "▮▮",
        }
    }

    /// 该式的声明组(全部落白名单;顺序固定)。
    pub fn decls(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Align9::Left => vec![("text-align", "left")],
            Align9::Center => vec![("text-align", "center")],
            Align9::Right => vec![("text-align", "right")],
            Align9::Justify => vec![("text-align", "justify")],
            Align9::JustifyLastCenter => {
                vec![("text-align", "justify"), ("text-align-last", "center")]
            }
            Align9::JustifyLastRight => {
                vec![("text-align", "justify"), ("text-align-last", "right")]
            }
            Align9::JustifyLastJustify => {
                vec![("text-align", "justify"), ("text-align-last", "justify")]
            }
            Align9::JustifyAll => vec![("text-align", "justify-all")],
            Align9::JustifyAllLast => vec![
                ("text-align", "justify-all"),
                ("text-align-last", "justify"),
            ],
        }
    }

    /// 从节点 style 投影当前对齐式(未声明 text-align = None,面板视为
    /// 「未设置」;左/中/右式不携带 text-align-last)。
    pub fn from_style(style: &[Decl]) -> Option<Align9> {
        let get = |p: &str| style.iter().find(|d| d.prop == p).map(|d| d.value.clone());
        let ta = get("text-align")?;
        let tal = get("text-align-last");
        Some(match (ta.as_str(), tal.as_deref()) {
            ("left", _) => Align9::Left,
            ("center", _) => Align9::Center,
            ("right", _) => Align9::Right,
            ("justify", None | Some("auto")) => Align9::Justify,
            ("justify", Some("center")) => Align9::JustifyLastCenter,
            ("justify", Some("right")) => Align9::JustifyLastRight,
            ("justify", Some("justify")) => Align9::JustifyLastJustify,
            ("justify-all", None | Some("auto")) => Align9::JustifyAll,
            ("justify-all", Some("justify")) => Align9::JustifyAllLast,
            _ => return None,
        })
    }
}

/// 段落对齐写回:按式的声明组设置;式**不携带**的对齐声明
/// (如左对齐时的 `text-align-last`)整条移除,避免残留声明把投影
/// 推到另一式(往返幂等的另一半)。
pub fn para_align_cmds(doc: &Document, sids: &[String], a: Align9) -> Vec<Command> {
    let decls = a.decls();
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let mut st = doc.nodes.get(nid)?.style.clone();
            for (p, v) in &decls {
                st = set_style_prop(st, p, v);
            }
            for p in ["text-align", "text-align-last"] {
                if !decls.iter().any(|(dp, _)| *dp == p) {
                    st.retain(|d| d.prop != p);
                }
            }
            Some(Command::SetStyle {
                sid: sid.clone(),
                new: st,
                old: None,
            })
        })
        .collect()
}

// ═══════════════════ 2. 字符面板构建器(run / 整段两条写回路径) ═══════════════════

/// 「整段转 run」:把文本全文包成单 run(`0..len` 默认样式)。
/// 已有 run / 空文本 / 非文本节点 → None(按钮置灰的理由)。
pub fn make_full_run_cmd(doc: &Document, sid: &str) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    match &doc.nodes.get(nid)?.kind {
        NodeKind::Text { text, segments, .. } => {
            if !segments.is_empty() || text.is_empty() {
                return None;
            }
            Some(Command::SetSegs {
                sid: sid.to_string(),
                new: vec![TextSeg {
                    start: 0,
                    end: text.len(),
                    style: SegStyle::default(),
                }],
                old: None,
            })
        }
        _ => None,
    }
}

/// 「移除全部 run」:回到整段样式(SetSegs 空表;无 run → None)。
pub fn clear_runs_cmd(doc: &Document, sid: &str) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    match &doc.nodes.get(nid)?.kind {
        NodeKind::Text { segments, .. } if !segments.is_empty() => Some(Command::SetSegs {
            sid: sid.to_string(),
            new: Vec::new(),
            old: None,
        }),
        _ => None,
    }
}

/// run 作用域写字段:对节点**全部段注记**应用一次 `SegStyle` 覆盖,
/// 整批一条 `SetSegs`(可撤销)。无 run / 非文本 / 覆盖无效果 → None。
pub fn seg_field_cmd(doc: &Document, sid: &str, apply: impl Fn(&mut SegStyle)) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    let orig = match &doc.nodes.get(nid)?.kind {
        NodeKind::Text { segments, .. } if !segments.is_empty() => segments.clone(),
        _ => return None,
    };
    let mut new = orig.clone();
    for s in &mut new {
        apply(&mut s.style);
    }
    if new == orig {
        return None;
    }
    Some(Command::SetSegs {
        sid: sid.to_string(),
        new,
        old: None,
    })
}

/// 整段作用域:粗体开关 → `font-weight` 700/400(多选 → Compound)。
pub fn char_bold_cmds(doc: &Document, sids: &[String], bold: bool) -> Vec<Command> {
    style_prop_cmds(doc, sids, "font-weight", if bold { "700" } else { "400" })
}

/// 整段作用域:斜体开关 → `font-style` italic/normal。
pub fn char_italic_cmds(doc: &Document, sids: &[String], italic: bool) -> Vec<Command> {
    style_prop_cmds(
        doc,
        sids,
        "font-style",
        if italic { "italic" } else { "normal" },
    )
}

/// 整段作用域:下划线/删除线 → `text-decoration` 关键字合成。
/// 两项都关 = 整条移除(继承);开 = 按值序 `underline line-through` 重建。
pub fn char_deco_cmds(
    doc: &Document,
    sids: &[String],
    underline: bool,
    strike: bool,
) -> Vec<Command> {
    if !underline && !strike {
        return style_prop_remove_cmds(doc, sids, "text-decoration");
    }
    let mut v: Vec<&str> = Vec::new();
    if underline {
        v.push("underline");
    }
    if strike {
        v.push("line-through");
    }
    style_prop_cmds(doc, sids, "text-decoration", &v.join(" "))
}

// ═══════════════════ 3. 区域文本溢出(design/03 §六 红点 / 06 §3.6 自动扩高) ═══════════════════

pub(super) fn decl_px(style: &[Decl], prop: &str) -> Option<f64> {
    style
        .iter()
        .find(|d| d.prop == prop)
        .and_then(|d| vb_common::units::parse_px(&d.value))
}

/// 区域文本溢出估算:内容需要高度 − 框高(px;≤0 = 不溢出)。
///
/// 与导出同款量测(`vb_render::text::measure_text_weighted`,贪心断行 +
/// 禁则 + 字重感知);行高 = 显式 px / 无单位倍数 × 字号 / 默认 1.32×
/// (与 vb_layout 同默认)。点文本宽度自适应,不判溢出。
pub fn area_overflow_px(doc: &Document, nid: NodeId) -> f64 {
    let Some(n) = doc.nodes.get(nid) else {
        return 0.0;
    };
    let NodeKind::Text {
        text,
        mode: TextMode::Area,
        ..
    } = &n.kind
    else {
        return 0.0;
    };
    let fs = decl_px(&n.style, "font-size").unwrap_or(16.0).max(1.0);
    let lh = match n.style_get("line-height") {
        Some(v) => {
            let t = v.trim();
            if t.ends_with("px") || t.ends_with("pt") {
                vb_common::units::parse_px(t).unwrap_or(fs * 1.32)
            } else {
                t.parse::<f64>().unwrap_or(1.32) * fs
            }
        }
        None => fs * 1.32,
    };
    let weight: u16 = n
        .style_get("font-weight")
        .and_then(|w| match w.trim() {
            "bold" => Some(700),
            "normal" => Some(400),
            other => other.parse().ok(),
        })
        .unwrap_or(400);
    let ls = decl_px(&n.style, "letter-spacing").unwrap_or(0.0);
    let family = n.style_get("font-family").unwrap_or("");
    let (max_w, lines) = vb_render::text::measure_text_weighted(
        text,
        family,
        fs as f32,
        weight,
        n.geom.w.max(1.0) as f32,
        ls as f32,
    );
    let _ = max_w;
    (lines as f64 * lh) - n.geom.h
}

/// 自动扩高(几何命令):把区域文本框高补足内容需要高度(向上取整);
/// 不溢出 / 非区域文本 → None。经 `SetGeom` 入 undo。
pub fn area_fit_height_cmd(doc: &Document, sid: &str) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    let overflow = area_overflow_px(doc, nid);
    if overflow <= 0.0 {
        return None;
    }
    let n = doc.nodes.get(nid)?;
    let mut g: Geom = n.geom;
    g.h = (g.h + overflow).ceil().max(1.0);
    Some(Command::SetGeom {
        sid: sid.to_string(),
        new: g,
        old: None,
        old_declared: None,
    })
}
