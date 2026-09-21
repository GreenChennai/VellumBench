//! S4 外观面板/描边面板/效果写回的**文档状态级**验收门(05a 报告门 1–4)。
//!
//! 「文档状态级」= 走 UI 与 Agent 共用的同一条命令路径:纯函数构建器
//! (`vb_app::app::appearance`)产出 `Compound[SetStyle, SetAttrs]` →
//! `UndoStack::push` → 断言 Document 的 CSS 声明与模型属性 → 逐条 undo
//! 逆回。效果映射表、条目排序/禁用/复制、描边全字段、不支持提示清单
//! 在此逐条落断言;无损往返另见 `corpus/22-appearance-stack`。

use vb_app::app::appearance::{
    add_effect_cmd, add_fill_cmd, add_stroke_cmd, decode_model, duplicate_item_cmd, move_item_cmd,
    remove_item_cmd, set_item_blend_cmd, set_stroke_spec_cmd, toggle_item_cmd, unsupported_message,
    AppearanceItem, AppearanceModel, Arrowhead, Effect, FillBody, StrokeAlign, StrokeCap,
    StrokeJoin, StrokeSpec, UNSUPPORTED,
};
use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, Node, NodeKind};
use vb_doc::undo::UndoStack;

// ────────────────── 夹具 ──────────────────

fn make_node(doc: &mut Document, kind: NodeKind, name: &str) -> String {
    let ab = doc.artboards.first().copied().expect("默认文档有一块画板");
    let sid = doc.alloc_sid();
    let mut n = Node::new(kind, name, sid.clone());
    n.geom = Geom {
        x: 40.0,
        y: 40.0,
        w: 200.0,
        h: 120.0,
    };
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    doc.nodes.get_mut(ab).unwrap().children.push(id);
    sid.as_str().to_string()
}

fn box_doc() -> (Document, UndoStack, String) {
    let (doc, sid) = box_only();
    (doc, UndoStack::new(), sid)
}

fn box_only() -> (Document, String) {
    let mut doc = Document::new_default();
    let sid = make_node(&mut doc, NodeKind::Box, "盒子");
    (doc, sid)
}

fn push(stack: &mut UndoStack, doc: &mut Document, cmd: Option<Command>) {
    stack
        .push(doc, cmd.expect("构建器应产出命令"))
        .expect("命令应成功应用");
}

fn push_result(stack: &mut UndoStack, doc: &mut Document, r: Result<Option<Command>, String>) {
    match r {
        Ok(Some(cmd)) => push(stack, doc, Some(cmd)),
        Ok(None) => panic!("构建器不应返回 None"),
        Err(msg) => panic!("构建器不应报错:{msg}"),
    }
}

/// 两段式推送:先用不可变借用构建命令,再可变推送(规避参数求值借用冲突)。
/// 离散操作纪律:禁用合并(与 UI `exec_appearance` 一致,每次点击 = 一条 undo)。
fn push_with<F>(stack: &mut UndoStack, doc: &mut Document, build: F)
where
    F: FnOnce(&Document) -> Result<Option<Command>, String>,
{
    let r = build(doc);
    stack.merging_enabled = false;
    push_result(stack, doc, r);
    stack.merging_enabled = true;
}

/// 参数编辑纪律:一次提交会话(与 UI `num_commit` 一致,拖数值多帧合并)。
fn push_param<F>(stack: &mut UndoStack, doc: &mut Document, build: F)
where
    F: FnOnce(&Document) -> Result<Option<Command>, String>,
{
    stack.begin_session();
    let r = build(doc);
    push_result(stack, doc, r);
    stack.end_session();
}

fn style_of<'a>(doc: &'a Document, sid: &str) -> &'a Vec<Decl> {
    &doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().style
}

fn decl<'a>(style: &'a [Decl], prop: &str) -> &'a str {
    style
        .iter()
        .find(|d| d.prop == prop)
        .unwrap_or_else(|| panic!("缺声明 {prop}"))
        .value
        .as_str()
        .trim()
}

fn attr_of<'a>(doc: &'a Document, sid: &str, key: &str) -> &'a str {
    doc.nodes
        .get(doc.find_by_sid(sid).unwrap())
        .unwrap()
        .attrs
        .get(key)
        .unwrap_or_else(|| panic!("缺属性 {key}"))
}

fn model_of(doc: &Document, sid: &str) -> AppearanceModel {
    let n = doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap();
    let attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    decode_model(&n.kind, &n.style, &attrs)
}

fn undo_count(stack: &mut UndoStack, doc: &mut Document) -> usize {
    let mut n = 0;
    while stack.can_undo() {
        stack.undo(doc).expect("undo 不应失败");
        n += 1;
    }
    n
}

// ────────────────── 门 1:条目模型(排序/禁用/复制,经命令路径) ──────────────────

/// 门 1a:多填充 → background-image 层序 = 条目序 + background-color 底条;
/// 模型属性落盘;undo 逆回后 style 与 attrs 全量还原。
#[test]
fn multi_fill_writes_css_and_attr_and_reverts() {
    let (mut doc, mut stack, sid) = box_doc();
    push_with(&mut stack, &mut doc, |d| {
        add_fill_cmd(
            d,
            &sid,
            FillBody::Solid {
                value: "#102030".into(),
            },
        )
    });
    push_with(&mut stack, &mut doc, |d| {
        add_fill_cmd(
            d,
            &sid,
            FillBody::Gradient {
                value: "linear-gradient(90deg, #204060 0%, #90c0f0 100%)".into(),
            },
        )
    });
    // 「+ 填充」插在最上层(AI 语义):红在最上
    push_with(&mut stack, &mut doc, |d| {
        add_fill_cmd(
            d,
            &sid,
            FillBody::Solid {
                value: "#ff0000".into(),
            },
        )
    });
    assert_eq!(decl(style_of(&doc, &sid), "background-color"), "#102030");
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "linear-gradient(#ff0000, #ff0000), linear-gradient(90deg, #204060 0%, #90c0f0 100%)"
    );
    let model = model_of(&doc, &sid);
    assert_eq!(model.items.len(), 3, "条目模型三条");
    assert_eq!(
        attr_of(&doc, &sid, "data-vb-appearance"),
        vb_app::app::appearance::encode_model(&model),
        "属性 = 模型紧凑 JSON"
    );

    // undo 全量逆回:样式/属性逐条还原,最终回到空样式空属性
    let n = undo_count(&mut stack, &mut doc);
    assert_eq!(n, 3, "三次条目操作 = 三条 undo");
    assert!(
        style_of(&doc, &sid).is_empty(),
        "样式应逆回为空:{:?}",
        style_of(&doc, &sid)
    );
    assert!(
        !doc.nodes
            .get(doc.find_by_sid(&sid).unwrap())
            .unwrap()
            .attrs
            .contains_key("data-vb-appearance"),
        "模型属性应随 undo 消失"
    );
}

/// 门 1b:排序(上移/下移)= CSS 叠加顺序;禁用(眼睛)= 从编译产物移除;
/// 复制 = 条目克隆;每步可逆。
#[test]
fn item_order_toggle_duplicate_roundtrip() {
    let (mut doc, mut stack, sid) = box_doc();
    for c in ["#ff0000", "#00ff00", "#0000ff"] {
        push_with(&mut stack, &mut doc, |d| {
            add_fill_cmd(
                d,
                &sid,
                FillBody::Solid {
                    value: c.to_string(),
                },
            )
        });
    }
    // 添加序:blue 最上,red 最底(background-color=red)
    assert_eq!(decl(style_of(&doc, &sid), "background-color"), "#ff0000");
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "linear-gradient(#0000ff, #0000ff), linear-gradient(#00ff00, #00ff00)"
    );
    // 上移 red(下标 2 → 1):red 变中层
    push_with(&mut stack, &mut doc, |d| move_item_cmd(d, &sid, 2, -1));
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "linear-gradient(#0000ff, #0000ff), linear-gradient(#ff0000, #ff0000)",
        "上移后层序跟随"
    );
    // 下移 blue(0 → 1)
    push_with(&mut stack, &mut doc, |d| move_item_cmd(d, &sid, 0, 1));
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "linear-gradient(#ff0000, #ff0000), linear-gradient(#0000ff, #0000ff)"
    );
    // 禁用中层 blue:层列表收缩(此时层序 = [red, blue])
    push_with(&mut stack, &mut doc, |d| toggle_item_cmd(d, &sid, 1, false));
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "linear-gradient(#ff0000, #ff0000)"
    );
    let m = model_of(&doc, &sid);
    assert_eq!(m.items.len(), 3, "禁用条目保留在模型");
    assert!(!m.items[1].enabled());
    // 复制末条(绿)→ 新条目插在原条目上方
    push_with(&mut stack, &mut doc, |d| duplicate_item_cmd(d, &sid, 2));
    let m = model_of(&doc, &sid);
    assert_eq!(m.items.len(), 4);
    assert!(
        matches!(&m.items[2], AppearanceItem::Fill(_)),
        "复制体插在原条目上方"
    );
    // 删除一条
    push_with(&mut stack, &mut doc, |d| remove_item_cmd(d, &sid, 0));
    assert_eq!(model_of(&doc, &sid).items.len(), 3);
    // 全部逆回(8 条命令:3 添加 + 移动×2 + 禁用 + 复制 + 删除),样式为空
    let n = undo_count(&mut stack, &mut doc);
    assert_eq!(n, 8);
    assert!(style_of(&doc, &sid).is_empty());
}

/// 门 1c(会话合并):NumField 提交会话里连续条目编辑只产生一条 undo
/// (同目标 SetStyle+SetAttrs 混合 Compound 可合并,05-6-3)。
#[test]
fn appearance_session_merges_into_single_undo() {
    let (mut doc, mut stack, sid) = box_doc();
    push_with(&mut stack, &mut doc, |d| {
        add_effect_cmd(d, &sid, Effect::GaussianBlur { radius: 2.0 })
    });
    // 会话:同一效果的连续参数编辑(等效拖数值多帧)→ 合并为一条
    stack.begin_session();
    let r1 = set_effect_at(&doc, &sid, 0, 3.0);
    push_result(&mut stack, &mut doc, r1);
    let r2 = set_effect_at(&doc, &sid, 0, 4.0);
    push_result(&mut stack, &mut doc, r2);
    stack.end_session();
    assert_eq!(decl(style_of(&doc, &sid), "filter"), "blur(4px)");
    let n = undo_count(&mut stack, &mut doc);
    assert_eq!(n, 2, "添加 + 会话合并条目,应为 2 条 undo(而非 3)");
}

fn set_effect_at(
    doc: &Document,
    sid: &str,
    index: usize,
    radius: f64,
) -> Result<Option<Command>, String> {
    vb_app::app::appearance::set_effect_cmd(doc, sid, index, Effect::GaussianBlur { radius })
}

// ────────────────── 门 2:效果映射表(六种 → 预期 CSS 声明) ──────────────────

/// 门 2a:六种效果逐一添加 → 精确 CSS 声明 + 模型属性;undo 精确逆回。
#[test]
fn effect_mapping_table_doc_state() {
    /// 效果映射表行:(名称, 效果, 期望声明集)。
    type EffectCase = (&'static str, Effect, Vec<(&'static str, &'static str)>);
    let cases: Vec<EffectCase> = vec![
        (
            "投影",
            Effect::DropShadow {
                x: 8.0,
                y: 12.0,
                blur: 24.0,
                spread: 4.0,
                color: "#00000059".into(),
            },
            vec![("box-shadow", "8px 12px 24px 4px #00000059")],
        ),
        (
            "内阴影",
            Effect::InnerShadow {
                x: 0.0,
                y: 2.0,
                blur: 6.0,
                spread: 0.0,
                color: "#00000066".into(),
            },
            vec![("box-shadow", "inset 0 2px 6px 0 #00000066")],
        ),
        (
            "外发光",
            Effect::OuterGlow {
                blur: 12.0,
                color: "#2e86ff80".into(),
            },
            vec![("box-shadow", "0 0 12px #2e86ff80")],
        ),
        (
            "内发光",
            Effect::InnerGlow {
                blur: 10.0,
                color: "#ffffff40".into(),
            },
            vec![("box-shadow", "inset 0 0 10px #ffffff40")],
        ),
        (
            "高斯模糊",
            Effect::GaussianBlur { radius: 4.0 },
            vec![("filter", "blur(4px)")],
        ),
        (
            "圆角",
            Effect::RoundCorners { radius: 8.0 },
            vec![("border-radius", "8px")],
        ),
    ];
    for (name, effect, expect) in cases {
        let (mut doc, mut stack, sid) = box_doc();
        push_with(&mut stack, &mut doc, |d| add_effect_cmd(d, &sid, effect));
        let st = style_of(&doc, &sid);
        for (prop, value) in &expect {
            assert_eq!(decl(st, prop), *value, "{name} 的 {prop} 映射");
        }
        assert_eq!(st.len(), expect.len(), "{name} 应只产生映射声明");
        assert_eq!(undo_count(&mut stack, &mut doc), 1, "{name} 一条 undo 逆回");
        assert!(style_of(&doc, &sid).is_empty(), "{name} undo 后无残留");
    }
}

/// 门 2b:羽化(mask-image 近似)与文字效果(text-shadow;内阴影拒绝)。
#[test]
fn feather_and_text_effects_doc_state() {
    let (mut doc, mut stack, sid) = box_doc();
    push_with(&mut stack, &mut doc, |d| {
        add_effect_cmd(d, &sid, Effect::Feather { radius: 12.0 })
    });
    assert_eq!(
        decl(style_of(&doc, &sid), "mask-image"),
        "radial-gradient(circle, #000 calc(100% - 12px), transparent)"
    );

    // 文字:投影 → text-shadow;内阴影 → 拒绝 + 提示文案
    let mut doc = Document::new_default();
    let tsid = make_node(
        &mut doc,
        NodeKind::Text {
            text: "标题".into(),
            mode: Default::default(),
            segments: Vec::new(),
        },
        "标题",
    );
    let mut stack = UndoStack::new();
    push_with(&mut stack, &mut doc, |d| {
        add_effect_cmd(
            d,
            &tsid,
            Effect::DropShadow {
                x: 2.0,
                y: 2.0,
                blur: 3.0,
                spread: 0.0,
                color: "#00000080".into(),
            },
        )
    });
    assert_eq!(
        decl(style_of(&doc, &tsid), "text-shadow"),
        "2px 2px 3px #00000080"
    );
    let err = add_effect_cmd(
        &doc,
        &tsid,
        Effect::InnerShadow {
            x: 0.0,
            y: 0.0,
            blur: 4.0,
            spread: 0.0,
            color: "#000".into(),
        },
    )
    .unwrap_err();
    assert!(err.contains("内阴影"), "拒绝提示应点名内阴影:{err}");
}

/// 门 2c:多填充/多效果组合经 **真实 HTML 管线**(导出 → 导入)无损往返:
/// 模型属性逐字节返回,受管声明保持。
#[test]
fn appearance_stack_survives_html_roundtrip() {
    let html = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>往返</title></head>
<body>
<section class="vb-artboard" data-vb-id="aa0000" data-vb-name="画板 1" style="position: relative; width: 400px; height: 300px">
  <div class="vb-card" data-vb-id="aa0001" data-vb-name="卡片" style="position: absolute; left: 40px; top: 40px; width: 200px; height: 120px; background-color: #102030; background-image: linear-gradient(#ff0000, #ff0000), linear-gradient(90deg, #204060 0%, #90c0f0 100%); background-blend-mode: multiply; box-shadow: 8px 12px 24px 4px #00000059, inset 0 0 10px #ffffff40; filter: blur(2px); border-radius: 6px" data-vb-appearance='{"v":1,"own":["fill","shadow","blur","radius"],"items":[{"kind":"fill","enabled":true,"blend":"multiply","body":{"type":"solid","value":"#ff0000"}},{"kind":"fill","enabled":true,"blend":null,"body":{"type":"gradient","value":"linear-gradient(90deg, #204060 0%, #90c0f0 100%)"}},{"kind":"fill","enabled":true,"blend":null,"body":{"type":"solid","value":"#102030"}},{"kind":"effect","enabled":true,"blend":null,"effect":{"type":"drop_shadow","x":8.0,"y":12.0,"blur":24.0,"spread":4.0,"color":"#00000059"}},{"kind":"effect","enabled":true,"blend":null,"effect":{"type":"inner_glow","blur":10.0,"color":"#ffffff40"}},{"kind":"effect","enabled":true,"blend":null,"effect":{"type":"gaussian_blur","radius":2.0}},{"kind":"effect","enabled":true,"blend":null,"effect":{"type":"round_corners","radius":6.0}}]'}>卡片</div>
</section>
</body>
</html>
"##;
    let dir = std::env::temp_dir().join(format!("vb-appearance-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();
    let r1 = vb_doc::import::import_project(&dir).unwrap();
    let out1 = vb_doc::export::render_project(&r1.doc);
    for (rel, content) in &out1.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let r2 = vb_doc::import::import_project(&dir).unwrap();
    let out2 = vb_doc::export::render_project(&r2.doc);
    let _ = std::fs::remove_dir_all(&dir);

    // L1:逐文件字节幂等
    assert_eq!(out1.files.len(), out2.files.len(), "文件数漂移");
    for ((pa, a), (pb, b)) in out1.files.iter().zip(out2.files.iter()) {
        assert_eq!(pa, pb, "文件表顺序漂移");
        assert_eq!(a, b, "{pa} 两次导出应逐字节相同");
    }
    // 模型属性无损往返(HTML 转义解码后逐字节相同)
    let node1 = r1
        .doc
        .nodes
        .get(r1.doc.find_by_sid("aa0001").unwrap())
        .unwrap();
    let node2 = r2
        .doc
        .nodes
        .get(r2.doc.find_by_sid("aa0001").unwrap())
        .unwrap();
    assert_eq!(
        node1.attrs.get("data-vb-appearance"),
        node2.attrs.get("data-vb-appearance"),
        "外观模型属性应无损往返"
    );
    // 条目模型完整还原:3 填充 + 4 效果
    let attrs: Vec<(String, String)> = node2
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let m = decode_model(&node2.kind, &node2.style, &attrs);
    assert_eq!(m.items.len(), 7);
    let bottom_solid = match &m.items[2] {
        AppearanceItem::Fill(f) => match &f.body {
            vb_app::app::appearance::FillBody::Solid { value } => value.clone(),
            other => panic!("第 3 条应为纯色填充:{other:?}"),
        },
        other => panic!("第 3 条应为填充:{other:?}"),
    };
    assert_eq!(bottom_solid, "#102030");
    assert!(
        matches!(&m.items[6], AppearanceItem::Effect(e) if matches!(e.effect, Effect::RoundCorners { radius: 6.0 }))
    );
}

// ────────────────── 门 3:描边全字段 + 任意元素 ──────────────────

/// 门 3a:盒对象描边全字段:内侧 → border 系;外侧 → outline 系;
/// 虚线 → border-style:dashed 近似(降级注记在报告)。
#[test]
fn box_stroke_full_fields_inside_and_outside() {
    let (mut doc, mut stack, sid) = box_doc();
    push_with(&mut stack, &mut doc, |d| add_stroke_cmd(d, &sid));
    let spec = StrokeSpec {
        width: 3.0,
        color: Some("#ff8800".into()),
        cap: StrokeCap::Round,
        join: StrokeJoin::Round,
        miter_limit: 4.0,
        dash: vec![6.0, 3.0],
        align: StrokeAlign::Inside,
        arrow_start: Arrowhead::None,
        arrow_end: Arrowhead::None,
    };
    push_param(&mut stack, &mut doc, |d| {
        set_stroke_spec_cmd(d, &sid, 0, spec)
    });
    assert_eq!(decl(style_of(&doc, &sid), "border-width"), "3px");
    assert_eq!(
        decl(style_of(&doc, &sid), "border-style"),
        "dashed",
        "虚线 → dashed 近似"
    );
    assert_eq!(decl(style_of(&doc, &sid), "border-color"), "#ff8800");

    // 外侧 → outline 系(不占布局,画在 border 之外)
    let spec = StrokeSpec {
        width: 2.0,
        color: Some("#00aaff".into()),
        dash: Vec::new(),
        align: StrokeAlign::Outside,
        ..StrokeSpec::default()
    };
    push_param(&mut stack, &mut doc, |d| {
        set_stroke_spec_cmd(d, &sid, 0, spec)
    });
    assert!(
        style_of(&doc, &sid)
            .iter()
            .all(|d| !d.prop.starts_with("border-")),
        "切换外侧后 border 系应整组移除"
    );
    assert_eq!(decl(style_of(&doc, &sid), "outline-width"), "2px");
    assert_eq!(decl(style_of(&doc, &sid), "outline-style"), "solid");
    assert_eq!(decl(style_of(&doc, &sid), "outline-color"), "#00aaff");
    // undo 逆回:添加一条 + 两次参数会话各一条
    let n = undo_count(&mut stack, &mut doc);
    assert_eq!(n, 3, "加描边 + 内侧 + 外侧 = 3 条 undo");
    assert!(style_of(&doc, &sid).is_empty());
}

/// 门 3b:矢量路径描边全字段 → SVG stroke 系(通用入口,替换
/// 「只有钢笔创建时写死」)。
#[test]
fn vector_stroke_full_fields() {
    let mut doc = Document::new_default();
    let mut path = vb_common::geom::BezPath::new();
    path.move_to(vb_common::geom::Point::new(0.0, 0.0));
    path.line_to(vb_common::geom::Point::new(100.0, 0.0));
    let sid = make_node(&mut doc, NodeKind::Vector { path }, "路径");
    let mut stack = UndoStack::new();
    push_with(&mut stack, &mut doc, |d| add_stroke_cmd(d, &sid));
    let spec = StrokeSpec {
        width: 2.5,
        color: Some("#112233".into()),
        cap: StrokeCap::Square,
        join: StrokeJoin::Miter,
        miter_limit: 8.0,
        dash: vec![4.0, 2.0, 1.0],
        align: StrokeAlign::Center,
        arrow_start: Arrowhead::Arrow,
        arrow_end: Arrowhead::None,
    };
    push_param(&mut stack, &mut doc, |d| {
        set_stroke_spec_cmd(d, &sid, 0, spec)
    });
    let st = style_of(&doc, &sid);
    assert_eq!(decl(st, "stroke"), "#112233");
    assert_eq!(decl(st, "stroke-width"), "2.5px");
    assert_eq!(decl(st, "stroke-linecap"), "square");
    assert_eq!(decl(st, "stroke-linejoin"), "miter");
    assert_eq!(decl(st, "stroke-miterlimit"), "8");
    assert_eq!(decl(st, "stroke-dasharray"), "4px, 2px, 1px");
    // 箭头 = 冻结登记:模型保留,CSS 不落盘
    let m = model_of(&doc, &sid);
    assert!(
        matches!(&m.items[0], AppearanceItem::Stroke(s) if s.spec.arrow_start == Arrowhead::Arrow && s.spec.join == StrokeJoin::Miter && s.spec.cap == StrokeCap::Square)
    );
    assert!(decl(st, "stroke") != "marker", "SVG marker 未建模");
    // undo 逆回(钢笔式写死的原始声明不受影响)
    undo_count(&mut stack, &mut doc);
    assert!(style_of(&doc, &sid).is_empty());
}

/// 门 3c:文字描边 → -webkit-text-stroke-width/color(白名单核验过的落点)。
#[test]
fn text_stroke_webkit_fallback() {
    let mut doc = Document::new_default();
    let sid = make_node(
        &mut doc,
        NodeKind::Text {
            text: "招牌".into(),
            mode: Default::default(),
            segments: Vec::new(),
        },
        "招牌",
    );
    let mut stack = UndoStack::new();
    push_with(&mut stack, &mut doc, |d| add_stroke_cmd(d, &sid));
    let spec = StrokeSpec {
        width: 2.0,
        color: Some("#3366ff".into()),
        ..StrokeSpec::default()
    };
    push_param(&mut stack, &mut doc, |d| {
        set_stroke_spec_cmd(d, &sid, 0, spec)
    });
    assert_eq!(
        decl(style_of(&doc, &sid), "-webkit-text-stroke-width"),
        "2px"
    );
    assert_eq!(
        decl(style_of(&doc, &sid), "-webkit-text-stroke-color"),
        "#3366ff"
    );
    assert_eq!(undo_count(&mut stack, &mut doc), 2);
}

/// 门 3d:多描边 = 主条目落盘 + 其余冻结登记(「主条目写属性 + 其余冻结」)。
#[test]
fn multiple_strokes_primary_only_is_honest() {
    let (mut doc, mut stack, sid) = box_doc();
    push_with(&mut stack, &mut doc, |d| add_stroke_cmd(d, &sid));
    push_with(&mut stack, &mut doc, |d| add_stroke_cmd(d, &sid));
    let m = model_of(&doc, &sid);
    assert_eq!(m.items.len(), 2, "两条描边条目都在模型");
    // 编译产物只有一组 border(主描边),不会出现两组叠加
    assert_eq!(decl(style_of(&doc, &sid), "border-width"), "1px");
    assert!(
        style_of(&doc, &sid)
            .iter()
            .filter(|d| d.prop == "border-width")
            .count()
            == 1
    );
    let _ = stack;
}

/// 门 3e:填充条目级混合模式 → background-blend-mode(文档状态级)。
#[test]
fn fill_blend_doc_state() {
    let (mut doc, mut stack, sid) = box_doc();
    push_with(&mut stack, &mut doc, |d| {
        add_fill_cmd(
            d,
            &sid,
            FillBody::Solid {
                value: "#ff0000".into(),
            },
        )
    });
    push_with(&mut stack, &mut doc, |d| {
        set_item_blend_cmd(d, &sid, 0, Some("multiply".into()))
    });
    // 单填充 → 纯色落 background-color,blend 无层可混(诚实:不产生空层)
    let m = model_of(&doc, &sid);
    assert_eq!(m.items[0].blend().unwrap(), "multiply", "模型保留混合模式");
    // 两条填充时 blend 落 background-blend-mode 逐层
    push_with(&mut stack, &mut doc, |d| {
        add_fill_cmd(
            d,
            &sid,
            FillBody::Solid {
                value: "#00ff00".into(),
            },
        )
    });
    // 条目序:[绿(顶), 红(底)];blend 设在顶层(→ image 层)有 CSS 落点
    push_with(&mut stack, &mut doc, |d| {
        set_item_blend_cmd(d, &sid, 0, Some("screen".into()))
    });
    assert_eq!(
        decl(style_of(&doc, &sid), "background-blend-mode"),
        "screen",
        "blend 列表只覆盖 image 层"
    );
    // 底层纯色的 blend 无 CSS 落点(它就是 background-color):仅模型保留
    let m = model_of(&doc, &sid);
    assert_eq!(
        m.items[1].blend().unwrap(),
        "multiply",
        "底层 blend 仅模型保留"
    );
}

// ────────────────── 门 4:不支持提示清单(05-6) ──────────────────

/// 门 4a:design/06 §七 六项全部在册,文案非空且可按 id 查;
/// 网格/描摹带「计划于 v2」。
#[test]
fn unsupported_list_matches_design06() {
    let ids: Vec<&str> = UNSUPPORTED.iter().map(|f| f.id).collect();
    assert_eq!(
        ids,
        vec![
            "live_paint",
            "mesh_gradient",
            "image_trace",
            "perspective_3d",
            "symbols",
            "variables",
        ],
        "§七 六项缺一不可"
    );
    for f in UNSUPPORTED {
        assert_eq!(unsupported_message(f.id), Some(f.message));
        assert!(!f.message.trim().is_empty());
    }
    assert!(unsupported_message("mesh_gradient")
        .unwrap()
        .contains("计划于 v2"));
    assert!(unsupported_message("image_trace")
        .unwrap()
        .contains("计划于 v2"));
    assert!(unsupported_message("symbols").unwrap().contains("v2"));
    assert!(unsupported_message("variables")
        .unwrap()
        .contains("设计令牌"));
}

/// 门 4b:圆角对非盒对象(路径/文字)在**命令层**拒绝并给提示;
/// 提示文案与属性面板 toast 同源。
#[test]
fn round_corners_rejected_on_non_box_with_message() {
    let mut doc = Document::new_default();
    let mut path = vb_common::geom::BezPath::new();
    path.move_to(vb_common::geom::Point::new(0.0, 0.0));
    let vsid = make_node(&mut doc, NodeKind::Vector { path }, "路径");
    let tsid = make_node(
        &mut doc,
        NodeKind::Text {
            text: "字".into(),
            mode: Default::default(),
            segments: Vec::new(),
        },
        "字",
    );
    let msg = add_effect_cmd(&doc, &vsid, Effect::RoundCorners { radius: 8.0 }).unwrap_err();
    assert_eq!(msg, "圆角仅对盒对象有效(路径/文字对象不支持)");
    assert_eq!(
        add_effect_cmd(&doc, &tsid, Effect::RoundCorners { radius: 8.0 }).unwrap_err(),
        "圆角仅对盒对象有效(路径/文字对象不支持)"
    );
    // 盒对象不受限
    let (doc, sid) = box_only();
    assert!(add_effect_cmd(&doc, &sid, Effect::RoundCorners { radius: 8.0 }).is_ok());
}

/// 门 4c:虚线超限在命令层拒绝(最多 6 组)。
#[test]
fn dash_over_limit_rejected() {
    let (mut doc, mut stack, sid) = box_doc();
    push_with(&mut stack, &mut doc, |d| add_stroke_cmd(d, &sid));
    let spec = StrokeSpec {
        dash: vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0],
        ..StrokeSpec::default()
    };
    let err = set_stroke_spec_cmd(&doc, &sid, 0, spec).unwrap_err();
    assert!(err.contains("6 组"), "超限提示:{err}");
}
