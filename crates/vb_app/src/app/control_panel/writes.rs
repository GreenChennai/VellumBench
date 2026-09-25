//! 控制面板 · 共享写回构建器(纯函数,门 2 测试走这里)。
//!
//! 06-1 自 `control_panel.rs` 按职责拆出(纯搬移,零行为变化)。

use vb_css::Decl;
use vb_doc::commands::Command;

use crate::app::{fmt_deg, parse_rotate_deg, set_style_prop};

// ═══════════════════ 2. 共享写回构建器(纯函数,门 2 测试走这里) ═══════════════════

/// 几何分量(geom_field_cmds 的替换目标)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeomAxis {
    X,
    Y,
    W,
    H,
}

/// 多选写回收敛:单条 → 原样;多条 → Compound(一次 undo);空 → None。
pub fn combine(cmds: Vec<Command>) -> Option<Command> {
    match cmds.len() {
        0 => None,
        1 => cmds.into_iter().next(),
        _ => Some(Command::Compound { cmds }),
    }
}

/// SetGeom:把选中对象的某一几何分量替换为 `v`(其余分量保持各自原值;
/// 多选 → Compound)。属性面板变换组与控制面板共用。
pub fn geom_field_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    axis: GeomAxis,
    v: f64,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let mut g = doc.nodes.get(nid)?.geom;
            match axis {
                GeomAxis::X => g.x = v,
                GeomAxis::Y => g.y = v,
                GeomAxis::W => g.w = v.max(1.0),
                GeomAxis::H => g.h = v.max(1.0),
            }
            Some(Command::SetGeom {
                sid: sid.clone(),
                new: g,
                old: None,
                old_declared: None,
            })
        })
        .collect()
}

/// SetGeom:同时替换**同一对象**的多个几何分量(画板预设/取向等
/// 双分量编辑)。逐分量各建一条 SetGeom 会以同一基准互相覆盖 ——
/// 必须在一条命令里合并。
pub fn geom_axes_cmd(
    doc: &vb_doc::model::Document,
    sid: &str,
    sets: &[(GeomAxis, f64)],
) -> Option<Command> {
    let nid = doc.find_by_sid(sid)?;
    let mut g = doc.nodes.get(nid)?.geom;
    for (axis, v) in sets {
        match axis {
            GeomAxis::X => g.x = *v,
            GeomAxis::Y => g.y = *v,
            GeomAxis::W => g.w = v.max(1.0),
            GeomAxis::H => g.h = v.max(1.0),
        }
    }
    Some(Command::SetGeom {
        sid: sid.to_string(),
        new: g,
        old: None,
        old_declared: None,
    })
}

/// SetStyle:在每个选中对象**自身样式**上设一个属性(保留其余声明;
/// 多选 → Compound)。属性面板外观/布局/文本组与控制面板共用。
pub fn style_prop_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    prop: &str,
    value: &str,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let style = doc.nodes.get(nid)?.style.clone();
            Some(Command::SetStyle {
                sid: sid.clone(),
                new: set_style_prop(style, prop, value),
                old: None,
            })
        })
        .collect()
}

/// SetStyle 属性删除(颜色框「清除」语义:声明整条移除;本无此声明
/// 的对象不产生命令)。
pub fn style_prop_remove_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    prop: &str,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let style = doc.nodes.get(nid)?.style.clone();
            let mut next = style.clone();
            next.retain(|d: &Decl| d.prop != prop);
            if next.len() == style.len() {
                return None;
            }
            Some(Command::SetStyle {
                sid: sid.clone(),
                new: next,
                old: None,
            })
        })
        .collect()
}

/// SetAttrs:href / alt / aria-label / target;`value` 空 = 删除属性
/// (属性本就不存在时不产生命令)。
pub fn attr_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    key: &str,
    value: &str,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let n = doc.nodes.get(nid)?;
            let mut merged: Vec<(String, String)> = n
                .attrs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            if value.is_empty() {
                let before = merged.len();
                merged.retain(|(k, _)| k != key);
                if merged.len() == before {
                    return None;
                }
            } else {
                match merged.iter_mut().find(|(k, _)| k == key) {
                    Some(slot) => slot.1 = value.to_string(),
                    None => merged.push((key.to_string(), value.to_string())),
                }
            }
            Some(Command::SetAttrs {
                sid: sid.clone(),
                new: merged,
                old: None,
            })
        })
        .collect()
}

/// 旋转数值化:SetStyle `transform: rotate(<deg>deg)`(与画布角点
/// 旋转拖拽同一条写回路径;现模型 transform 只承载 rotate,倾斜 →
/// 阶段 2 时再改为合并写)。
pub fn rotation_cmds(doc: &vb_doc::model::Document, sids: &[String], deg: f64) -> Vec<Command> {
    let v = format!("rotate({}deg)", fmt_deg(deg));
    style_prop_cmds(doc, sids, "transform", &v)
}

/// 节点当前旋转角(style.transform;无声明 = 0)。
pub fn rotation_deg_of(style: &[Decl]) -> f64 {
    style
        .iter()
        .find(|d| d.prop == "transform")
        .and_then(|d| parse_rotate_deg(&d.value))
        .unwrap_or(0.0)
}

/// Rename:图层名(导出 → `data-vb-name`;导出组「名称」字段共用)。
pub fn rename_cmds(doc: &vb_doc::model::Document, sids: &[String], name: &str) -> Vec<Command> {
    sids.iter()
        .filter(|sid| doc.find_by_sid(sid).is_some())
        .map(|sid| Command::Rename {
            sid: sid.clone(),
            new: name.to_string(),
            old: None,
        })
        .collect()
}

// ────────────────── 渐变数值化(02-2:现有拖方向能力 + 角度/类型/反向) ──────────────────
//
// 阶段 4(05-2)把渐变**结构化模型**上收到 `vb_ui::gradient`(唯一真相:色标有
// 位置/显式标记/中点/不透明度)。本节的字符串级接口保留给控制面板的数值化
// 微调(角度/类型/反向),内部改为**委托**结构化解析,保证两处口径不会分叉。

/// 渐变类型(权威定义在 `vb_ui::gradient`;此处再导出以保持既有调用路径)。
pub use vb_ui::gradient::GradKind;

/// 解析 background-image 渐变值 → (类型, 角度, 色标串列表)。
///
/// 线性:`linear-gradient(45deg, #a 0%, #b 100%)`;径向:角度无意义(0),
/// 锚点段(`circle at 50% 50%`)保真留在 `stops[0]`。
pub fn parse_gradient(value: &str) -> Option<(GradKind, f64, Vec<String>)> {
    let g = vb_ui::gradient::parse(value)?;
    let mut stops: Vec<String> = Vec::new();
    if let Some(h) = &g.head {
        stops.push(h.clone());
    }
    // 段序 = 色标序;中点提示以裸百分比段紧跟其所属色标
    for (i, s) in g.stops.iter().enumerate() {
        stops.push(s.to_css());
        if let Some((_, p)) = g.hints.iter().find(|(hi, _)| *hi == i) {
            stops.push(format!("{}%", vb_common::units::fmt_num(p * 100.0)));
        }
    }
    Some((g.kind, g.angle, stops))
}

/// 拼回 background-image 值(径向固定 `circle at 50% 50%` 锚点)。
pub fn build_gradient(kind: GradKind, angle: f64, stops: &[String]) -> String {
    match kind {
        GradKind::Linear => {
            format!(
                "linear-gradient({}deg, {})",
                fmt_deg(angle),
                stops.join(", ")
            )
        }
        // stops[0] 是锚点段(缺省补 50% 50%);其余为色标
        GradKind::Radial => match stops.split_first() {
            Some((head, rest)) if head.trim().starts_with("circle") => {
                format!("radial-gradient({}, {})", head, rest.join(", "))
            }
            _ => format!("radial-gradient(circle at 50% 50%, {})", stops.join(", ")),
        },
    }
}

/// 渐变角度数值化:改写选中对象已有渐变的角度;无渐变时回退 =
/// 按该角度生成「现填充 → 白」双色渐变(与画布拖方向同一落点)。
pub fn gradient_angle_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    angle: f64,
) -> Vec<Command> {
    angle_or_kind_edit(doc, sids, Some(angle), None)
}

/// 渐变类型切换:线性 ↔ 径向(保留色标;径向锚点固定中心,
/// 线性角回退 90° = 自左向右,与画布默认拖向一致)。
pub fn gradient_kind_cmds(
    doc: &vb_doc::model::Document,
    sids: &[String],
    kind: GradKind,
) -> Vec<Command> {
    angle_or_kind_edit(doc, sids, None, Some(kind))
}

/// 渐变反向:线性 = 角度 +180(方向逆转,色标不动);径向 = 色标
/// 顺序逆序(位置跟着色标走,CSS 渲染端会归一化停点顺序)。
/// 无渐变的对象**跳过**(反向不无中生有)。
pub fn gradient_reverse_cmds(doc: &vb_doc::model::Document, sids: &[String]) -> Vec<Command> {
    edit_each_gradient(doc, sids, None, |kind, angle, stops| match kind {
        GradKind::Linear => build_gradient(kind, (angle + 180.0).rem_euclid(360.0), &stops),
        GradKind::Radial => {
            // stops[0] 是锚点段(circle at …):保持在首位,只逆序色标
            let (head, rest) = match stops.split_first() {
                Some((h, r)) if h.trim().starts_with("circle") => (Some(h), r),
                _ => (None, &stops[..]),
            };
            let mut rev = rest.to_vec();
            rev.reverse();
            let mut out = head.map(|h| vec![h.clone()]).unwrap_or_default();
            out.extend(rev);
            build_gradient(kind, angle, &out)
        }
    })
}

fn angle_or_kind_edit(
    doc: &vb_doc::model::Document,
    sids: &[String],
    angle: Option<f64>,
    kind: Option<GradKind>,
) -> Vec<Command> {
    // 回退生成规格(无既有渐变的对象):只改角度 → (线性, 该角度);
    // 切类型 → (目标类型, 线性默认 90°)
    let fallback = match (angle, kind) {
        (Some(a), _) => (GradKind::Linear, a),
        (None, Some(k)) => (k, 90.0),
        (None, None) => unreachable!("角度与类型至少给一个"),
    };
    edit_each_gradient(doc, sids, Some(fallback), |k, a, mut stops| {
        let nk = kind.unwrap_or(k);
        let na = match (angle, kind) {
            (Some(x), None) => x, // 只改角度
            (None, Some(GradKind::Linear)) => 90.0,
            (None, Some(GradKind::Radial)) | (Some(_), Some(_)) | (None, None) => a,
        };
        // 径向 → 线性:剥掉锚点段(stops[0] 的 circle at …)
        if nk == GradKind::Linear
            && k == GradKind::Radial
            && stops.first().map(|x| x.trim().starts_with("circle")) == Some(true)
        {
            stops.remove(0);
        }
        build_gradient(nk, na, &stops)
    })
}

/// 对每个选中对象做一次渐变值改写。
///
/// `fallback` = 对象**没有**渐变时的生成规格((类型, 角度);
/// `None` = 跳过无渐变对象,不无中生有)。回退生成的色标 =
/// 「现填充 → 白」,与画布拖方向写回(`apply_gradient_to_selection`)
/// 完全同构。
fn edit_each_gradient(
    doc: &vb_doc::model::Document,
    sids: &[String],
    fallback: Option<(GradKind, f64)>,
    rewrite: impl Fn(GradKind, f64, Vec<String>) -> String,
) -> Vec<Command> {
    sids.iter()
        .filter_map(|sid| {
            let nid = doc.find_by_sid(sid)?;
            let n = doc.nodes.get(nid)?;
            let cur = n
                .style
                .iter()
                .find(|d| d.prop == "background-image")
                .map(|d| d.value.clone());
            let value = match cur.as_deref().and_then(parse_gradient) {
                Some((k, a, stops)) => rewrite(k, a, stops),
                None => {
                    let (fk, fa) = fallback?;
                    let c1 = n
                        .fill_color()
                        .map(|c| c.to_shortest_hex())
                        .unwrap_or_else(|| "#d4d4d4".into()); // vb-token-ok: 文档内容色
                    let stops: Vec<String> =
                        [format!("{c1} 0%"), "#ffffff 100%".to_string()].to_vec();
                    build_gradient(fk, fa, &stops)
                }
            };
            Some(Command::SetStyle {
                sid: sid.clone(),
                new: set_style_prop(n.style.clone(), "background-image", &value),
                old: None,
            })
        })
        .collect()
}
