//! 外观命令构建器(门禁测试走这里)+ 不支持处理登记(05-6)。
//!
//! 06-1 自 `appearance.rs` 按职责拆出(纯搬移,零行为变化)。

use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeId, NodeKind};

use super::codec::item_own_tag;
use super::codec::*;
use super::model::*;

// ═══════════════════════ 6. 命令构建器(门禁测试走这里) ═══════════════════════

/// 构建器错误 = 用户可见提示文案(UI 直接 toast;构建器测试断言文案)。
pub type AppearanceResult = Result<Option<Command>, String>;

/// 条目的接管组标记。
/// 条目写回单命令:`Compound[SetStyle(重编译), SetAttrs(模型)]`。
/// `m` 为**新**模型(own 已含 prev 的接管集;删到 0 条也保持接管)。
pub fn appearance_write_cmd(doc: &Document, sid: &str, m: AppearanceModel) -> AppearanceResult {
    let nid = doc
        .find_by_sid(sid)
        .ok_or_else(|| format!("对象不存在:{sid}"))?;
    let n = doc.node(nid).ok_or_else(|| format!("对象不存在:{sid}"))?;
    let mut own = m.own.clone();
    for it in &m.items {
        let tag = item_own_tag(it);
        AppearanceModel::own_add(&mut own, &tag);
    }
    let style = compile_style(&n.kind, &n.style, &own, &m.items);
    let mut attrs = n.attrs.clone();
    attrs.insert(APPEARANCE_ATTR.to_string(), encode_model(&m));
    let attr_list: Vec<(String, String)> = attrs.into_iter().collect();
    Ok(Some(Command::Compound {
        cmds: vec![
            Command::SetStyle {
                sid: sid.to_string(),
                new: style,
                old: None,
            },
            Command::SetAttrs {
                sid: sid.to_string(),
                new: attr_list,
                old: None,
            },
        ],
    }))
}

fn load_model(doc: &Document, sid: &str) -> Option<(NodeId, AppearanceModel)> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.node(nid)?;
    let attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Some((nid, decode_model(&n.kind, &n.style, &attrs)))
}

pub(super) fn validate_effect(kind: &NodeKind, e: &Effect) -> Result<(), String> {
    match e {
        Effect::RoundCorners { .. }
            if !accepts_round_corners(kind) && target_of(kind) != AppearanceTarget::Frozen =>
        {
            Err("圆角仅对盒对象有效(路径/文字对象不支持)".into())
        }
        Effect::RoundCorners { .. } if target_of(kind) == AppearanceTarget::Frozen => {
            Err("冻结块内部不可编辑(样式作用于原样保留的 HTML 片段,无渲染落点)".into())
        }
        Effect::InnerShadow { .. } | Effect::InnerGlow { .. }
            if target_of(kind) == AppearanceTarget::Text =>
        {
            Err("文字不支持内阴影/内发光(CSS text-shadow 无 inset 语义)".into())
        }
        _ => Ok(()),
    }
}

// ── 填充 ──

/// 添加填充(默认半透明灰,入栈可撤销)。
pub fn add_fill_cmd(doc: &Document, sid: &str, body: FillBody) -> AppearanceResult {
    let (nid, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    let kind = doc.node(nid).unwrap().kind.clone();
    match (&body, target_of(&kind)) {
        (FillBody::Gradient { .. }, AppearanceTarget::Vector) => {
            return Err("矢量路径的渐变填充暂不支持(SVG defs 未建模,计划 v2);可先用纯色".into())
        }
        (FillBody::Gradient { .. }, AppearanceTarget::Text) => {
            return Err("文字渐变填充暂不支持(计划 v2);可先用纯色".into())
        }
        _ => {}
    }
    // AI 语义:「+ 填充」新条目在最上层(index 0 = 最上,CSS background-image 首层)
    m.items.insert(
        0,
        AppearanceItem::Fill(FillItem {
            enabled: true,
            blend: None,
            body,
        }),
    );
    appearance_write_cmd(doc, sid, m)
}

/// 编辑第 `index` 条填充。
pub fn set_fill_body_cmd(
    doc: &Document,
    sid: &str,
    index: usize,
    body: FillBody,
) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    if let AppearanceItem::Fill(f) = &mut m.items[index] {
        f.body = body;
    } else {
        return Ok(None);
    }
    appearance_write_cmd(doc, sid, m)
}

// ── 描边 ──

/// 为任意对象添加描边(05-3-3 通用入口;默认 1px 黑内侧)。
pub fn add_stroke_cmd(doc: &Document, sid: &str) -> AppearanceResult {
    let (_nid, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    // AI 语义:新描边条目在最上层
    m.items.insert(
        0,
        AppearanceItem::Stroke(StrokeItem {
            enabled: true,
            blend: None,
            spec: StrokeSpec {
                color: Some("#1a1a1a".into()), // vb-token-ok: 默认描边色(文档内容,非 UI 皮肤)
                ..StrokeSpec::default()
            },
        }),
    );
    appearance_write_cmd(doc, sid, m)
}

/// 编辑第 `index` 条描边(描边面板全字段走这里)。
pub fn set_stroke_spec_cmd(
    doc: &Document,
    sid: &str,
    index: usize,
    spec: StrokeSpec,
) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if spec.dash.len() > 6 {
        return Err("虚线最多 6 组(3 对值/间隙)".into());
    }
    if index >= m.items.len() {
        return Ok(None);
    }
    if let AppearanceItem::Stroke(s) = &mut m.items[index] {
        s.spec = spec;
    } else {
        return Ok(None);
    }
    appearance_write_cmd(doc, sid, m)
}

// ── 效果 ──

/// 添加效果(六种映射 + 羽化;圆角/内阴影对非支持目标的拒绝含提示)。
pub fn add_effect_cmd(doc: &Document, sid: &str, effect: Effect) -> AppearanceResult {
    let (nid, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    let kind = doc.node(nid).unwrap().kind.clone();
    validate_effect(&kind, &effect)?;
    // AI 语义:新效果条目在最上层
    m.items.insert(
        0,
        AppearanceItem::Effect(EffectItem {
            enabled: true,
            blend: None,
            effect,
        }),
    );
    appearance_write_cmd(doc, sid, m)
}

/// 编辑第 `index` 条效果(参数数值化;校验同添加)。
pub fn set_effect_cmd(doc: &Document, sid: &str, index: usize, effect: Effect) -> AppearanceResult {
    let (nid, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    let kind = doc.node(nid).unwrap().kind.clone();
    validate_effect(&kind, &effect)?;
    if index >= m.items.len() {
        return Ok(None);
    }
    if let AppearanceItem::Effect(e) = &mut m.items[index] {
        e.effect = effect;
    } else {
        return Ok(None);
    }
    appearance_write_cmd(doc, sid, m)
}

// ── 条目操作(排序 / 禁用 / 复制 / 删除 / 混合模式) ──

/// 上移/下移(05-1-1 条目顺序 = CSS 叠加顺序;dir -1 = 上移)。
pub fn move_item_cmd(doc: &Document, sid: &str, index: usize, dir: i32) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    let to = index as i32 + dir;
    if index >= m.items.len() || to < 0 || to as usize >= m.items.len() {
        return Ok(None);
    }
    m.items.swap(index, to as usize);
    appearance_write_cmd(doc, sid, m)
}

/// 眼睛开关(临时禁用 = 从编译产物移除该条,模型保留)。
pub fn toggle_item_cmd(doc: &Document, sid: &str, index: usize, enabled: bool) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    m.items[index].set_enabled(enabled);
    appearance_write_cmd(doc, sid, m)
}

/// 复制条目(插到原条目上方;AI 行为)。
pub fn duplicate_item_cmd(doc: &Document, sid: &str, index: usize) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    let copy = m.items[index].clone();
    m.items.insert(index, copy);
    appearance_write_cmd(doc, sid, m)
}

/// 删除条目(删到 0 条保持接管:清空 = 移除受管声明,不静默残留)。
pub fn remove_item_cmd(doc: &Document, sid: &str, index: usize) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    m.items.remove(index);
    appearance_write_cmd(doc, sid, m)
}

/// 条目混合模式(填充 → background-blend-mode 逐层;描边/效果 → 冻结登记)。
pub fn set_item_blend_cmd(
    doc: &Document,
    sid: &str,
    index: usize,
    blend: Option<String>,
) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    m.items[index].set_blend(blend);
    appearance_write_cmd(doc, sid, m)
}

/// 只改描边粗细(外观面板条目编辑器快调用;避免整份 spec 来回克隆的
/// 取值竞态:粗细数值框每帧回读模型)。
pub fn update_stroke_width(
    doc: &Document,
    sid: &str,
    index: usize,
    width: f64,
) -> AppearanceResult {
    let (_, mut m) = load_model(doc, sid).ok_or_else(|| "未选中对象".to_string())?;
    if index >= m.items.len() {
        return Ok(None);
    }
    if let AppearanceItem::Stroke(s) = &mut m.items[index] {
        s.spec.width = width;
    } else {
        return Ok(None);
    }
    appearance_write_cmd(doc, sid, m)
}

// ═══════════════════════ 7. 不支持处理(05-6;design/06 §七) ═══════════════════════

/// 明确不支持的能力:点击给提示而非沉默(design/06 §七)。
/// 文案与 design/06 §七 一致,并按 05-6-2 补「计划于 vX」。
pub struct UnsupportedFeature {
    pub id: &'static str,
    pub label: &'static str,
    pub message: &'static str,
}

pub const UNSUPPORTED: &[UnsupportedFeature] = &[
    UnsupportedFeature {
        id: "live_paint",
        label: "实时上色",
        message: "实时上色在 v1 不支持;可用路径查找器或形状生成器代替",
    },
    UnsupportedFeature {
        id: "mesh_gradient",
        label: "渐变网格",
        message: "渐变网格无 HTML 对应;建议用多层径向渐变叠加模拟(计划于 v2 支持)",
    },
    UnsupportedFeature {
        id: "image_trace",
        label: "图像描摹",
        message: "图像描摹暂不支持(计划于 v2 支持)",
    },
    UnsupportedFeature {
        id: "perspective_3d",
        label: "3D / 透视",
        message: "3D 与透视网格不支持",
    },
    UnsupportedFeature {
        id: "symbols",
        label: "符号",
        message: "符号将于 v2 以『组件』形式提供",
    },
    UnsupportedFeature {
        id: "variables",
        label: "变量",
        message: "变量已以『设计令牌』提供:右侧面板坞 · 令牌 Tab(F4 切换)",
    },
];

/// 按 id 查不支持提示文案(门禁测试 + UI 共用)。
pub fn unsupported_message(id: &str) -> Option<&'static str> {
    UNSUPPORTED.iter().find(|f| f.id == id).map(|f| f.message)
}
