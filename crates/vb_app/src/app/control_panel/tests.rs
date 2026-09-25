//! 控制面板门禁测试(门 1 字段映射表 / 门 2 写回构建器文档状态级)。
//! 06-1 自 `control_panel.rs` 拆出(纯搬移,零行为变化)。

use vb_doc::model::{Document, Geom, Node, NodeKind, NodeTree};
use vb_doc::undo::UndoStack;

use super::*;
use crate::app::Tool;
use vb_css::Decl;
use vb_doc::commands::Command;

// ───────────── 门 1:八工具态字段映射表(design/03 §三) ─────────────

/// 工具态 → 字段清单映射表(门 1 断言的真相表,与 02c 报告同源)。
fn expect_table() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        (
            // 选择·无选区 → 画板选项:预设/取向/尺寸/背景/画板数(+画板)
            // 04-5-3:画板数(ab.count,只读计数)按 design/03 §三 补齐
            "select.artboard",
            vec![
                "ab.preset",
                "ab.orient",
                "ab.w",
                "ab.h",
                "ab.bg",
                "ab.count",
                "ab.add",
            ],
        ),
        (
            // 选择·有选区 → 变换:X/Y/W/H/∠ + 填充/不透明 + 对齐画板 + 编组/排列
            "select.transform",
            vec![
                "x", "y", "w", "h", "rot", "fill", "opacity", "align.h", "align.v", "group", "fwd",
                "bwd",
            ],
        ),
        (
            // 直接选择 → 锚点坐标
            "direct.anchor",
            vec!["ax", "ay", "anchor.hint"],
        ),
        (
            // 钢笔 → 填充/描边/粗细 + 自动闭合提示
            "pen",
            vec!["pen.fill", "pen.stroke", "pen.sw", "pen.hint"],
        ),
        (
            // 矩形 → 填充/描边/粗细/圆角(起始角 → 遗留项)
            "rect",
            vec!["sh.fill", "sh.stroke", "sh.sw", "sh.radius"],
        ),
        (
            // 椭圆 → 填充/描边/粗细(圆角是矩形的)
            "ellipse",
            vec!["sh.fill", "sh.stroke", "sh.sw"],
        ),
        (
            // 文字 → 字号/对齐/颜色(字体族 → 阶段 3)
            "text",
            vec!["t.size", "t.align", "t.color", "text.hint"],
        ),
        (
            // 渐变 → 类型/角度/反向(色标编辑 → 阶段 4)
            "gradient",
            vec!["g.kind", "g.angle", "g.reverse", "g.hint"],
        ),
        (
            // 画板工具 → 预设/名称/尺寸/位置(适配内容 → 阶段 2)
            "artboard",
            vec![
                "ab.preset",
                "ab.name",
                "ab.x",
                "ab.y",
                "ab.w",
                "ab.h",
                "ab.hint",
            ],
        ),
    ]
}

fn ctx(tool: Tool, has_selection: bool) -> CtlCtx {
    CtlCtx {
        tool,
        has_selection,
    }
}

/// 门 1(逐态):spec_for 的输出与 design/03 §三 字段表一一对应。
#[test]
fn eight_tool_states_match_design03_field_table() {
    let cases = vec![
        ctx(Tool::Select, false),
        ctx(Tool::Select, true),
        ctx(Tool::DirectSelect, true),
        ctx(Tool::Pen, false),
        ctx(Tool::Rect, false),
        ctx(Tool::Ellipse, false),
        ctx(Tool::Text, false),
        ctx(Tool::Gradient, true),
        ctx(Tool::Artboard, false),
    ];
    for c in &cases {
        let spec = spec_for(c);
        let expect = expect_table()
            .into_iter()
            .find(|(s, _)| *s == spec.state)
            .unwrap_or_else(|| panic!("态 {} 不在映射表", spec.state));
        assert_eq!(
            spec.ids(),
            expect.1,
            "工具态 {} 的字段集合与 design/03 §三 映射表不一致",
            spec.state
        );
    }
}

/// 门 1(无遗漏):映射表里的每个态都由 spec_for 实际产出。
#[test]
fn mapping_table_states_are_all_reachable() {
    let produced: Vec<&str> = [
        ctx(Tool::Select, false),
        ctx(Tool::Select, true),
        ctx(Tool::DirectSelect, true),
        ctx(Tool::Pen, false),
        ctx(Tool::Rect, false),
        ctx(Tool::Ellipse, false),
        ctx(Tool::Text, false),
        ctx(Tool::Gradient, true),
        ctx(Tool::Artboard, false),
    ]
    .iter()
    .map(spec_for)
    .map(|s| s.state)
    .collect();
    for (state, _) in expect_table() {
        assert!(
            produced.contains(&state),
            "映射表态 {state} 未被任何工具态产出"
        );
    }
}

/// 门 1(回落与提示态):直线 = 椭圆态字段;编组选择 = 变换态;
/// 抓手/缩放/吸管/剪刀只给提示,无假控件。
#[test]
fn fallback_states_reuse_nearest_defined_spec() {
    assert_eq!(spec_for(&ctx(Tool::Line, false)).state, "ellipse");
    assert_eq!(
        spec_for(&ctx(Tool::GroupSelect, true)).state,
        "select.transform"
    );
    for t in [Tool::Hand, Tool::Zoom, Tool::Eyedropper, Tool::Scissors] {
        let spec = spec_for(&ctx(t, false));
        assert!(
            spec.fields.iter().all(|x| x.kind == CtlKind::Hint),
            "{} 态只允许提示字段",
            spec.state
        );
    }
}

/// 固定右侧区:文档标题 / 画板切换 / 缩放 三字段恒在(设计 §三 尾注)。
#[test]
fn fixed_zone_has_title_artboard_switch_zoom() {
    let ids: Vec<&str> = FIXED_FIELDS.iter().map(|x| x.id).collect();
    assert_eq!(ids, vec!["doc.title", "ab.switch", "zoom"]);
    assert_eq!(FIXED_FIELDS[0].write, CtlWrite::Doc("SetMetaTitle"));
    assert_eq!(
        FIXED_FIELDS[1].write,
        CtlWrite::App("view.zoom_to_selection")
    );
}

/// 渐变解析/拼回:线性、径向、rgb() 色标的顶层逗号拆分。
#[test]
fn gradient_parse_build_roundtrip() {
    let (k, a, stops) = parse_gradient("linear-gradient(45deg, #ff5a1f 0%, #ffffff 100%)").unwrap();
    assert_eq!(k, GradKind::Linear);
    assert!((a - 45.0).abs() < 1e-9);
    assert_eq!(stops, vec!["#ff5a1f 0%", "#ffffff 100%"]);
    assert_eq!(
        build_gradient(k, a, &stops),
        "linear-gradient(45deg, #ff5a1f 0%, #ffffff 100%)"
    );
    let (k, _, stops) =
        parse_gradient("radial-gradient(circle at 50% 50%, rgb(255, 0, 0) 0%, #00ff00 100%)")
            .unwrap();
    assert_eq!(k, GradKind::Radial);
    assert_eq!(stops.len(), 3, "锚点段 + 2 色标");
    assert_eq!(stops[0], "circle at 50% 50%", "径向锚点段保留在 stops[0]");
    assert_eq!(stops[1], "rgb(255, 0, 0) 0%", "rgb() 内逗号不得拆分色标");
    assert!(parse_gradient("solid #fff").is_none());
    // 径向 → 线性:锚点段剥掉,色标保留
    let (_, a, stops) =
        parse_gradient("radial-gradient(circle at 35% 35%, #ff0000 0%, #00ff00 100%)").unwrap();
    let mut from_radial = stops.clone();
    from_radial.remove(0);
    assert_eq!(
        build_gradient(GradKind::Linear, 90.0, &from_radial),
        "linear-gradient(90deg, #ff0000 0%, #00ff00 100%)"
    );
    let _ = a;
}

// ───────────── 门 2:文档状态级(经命令路径) ─────────────

/// 夹具:默认文档 + 一个盒子;返回 (doc, sid)。
fn box_doc() -> (Document, String) {
    let mut doc = Document::new_default();
    let ab = doc.artboards.first().copied().unwrap();
    let sid = doc.alloc_sid();
    let mut n = Node::new(NodeKind::Box, "盒子", sid.clone());
    n.geom = Geom {
        x: 10.0,
        y: 20.0,
        w: 100.0,
        h: 50.0,
    };
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    doc.nodes.get_mut(ab).unwrap().children.push(id);
    (doc, sid.as_str().to_string())
}

fn push(stack: &mut UndoStack, doc: &mut Document, cmd: Command) {
    stack.push(doc, cmd).expect("命令应成功应用");
}

fn style_of<'a>(doc: &'a Document, sid: &str) -> &'a Vec<Decl> {
    &doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().style
}

fn decl<'a>(style: &'a [Decl], prop: &str) -> &'a str {
    &style.iter().find(|d| d.prop == prop).unwrap().value
}

/// 门 2(变换):X/Y/W/H 逐分量写回只动该分量;undo 精确逆回。
#[test]
fn geom_field_writes_single_axis_and_reverts() {
    let (mut doc, sid) = box_doc();
    let mut stack = UndoStack::new();
    let sids = vec![sid.clone()];
    for (axis, v) in [
        (GeomAxis::X, 42.0),
        (GeomAxis::Y, 43.0),
        (GeomAxis::W, 200.0),
        (GeomAxis::H, 80.0),
    ] {
        for cmd in geom_field_cmds(&doc, &sids, axis, v) {
            push(&mut stack, &mut doc, cmd);
        }
    }
    let g = &doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap().geom;
    assert_eq!((g.x, g.y, g.w, g.h), (42.0, 43.0, 200.0, 80.0));
    while stack.undo(&mut doc).unwrap().is_some() {}
    let g = &doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap().geom;
    assert_eq!(
        (g.x, g.y, g.w, g.h),
        (10.0, 20.0, 100.0, 50.0),
        "undo 应逆回"
    );
}

/// 门 2(变换 ∠):旋转数值化写 transform: rotate(Ndeg);undo 还原。
#[test]
fn rotation_numeric_write_and_revert() {
    let (mut doc, sid) = box_doc();
    let mut stack = UndoStack::new();
    let sids = vec![sid.clone()];
    for cmd in rotation_cmds(&doc, &sids, 37.5) {
        push(&mut stack, &mut doc, cmd);
    }
    assert_eq!(decl(style_of(&doc, &sid), "transform"), "rotate(37.5deg)");
    assert!((rotation_deg_of(style_of(&doc, &sid)) - 37.5).abs() < 1e-9);
    stack.undo(&mut doc).unwrap();
    assert!(
        style_of(&doc, &sid).iter().all(|d| d.prop != "transform"),
        "undo 后 transform 声明应消失"
    );
}

/// 门 2(外观):填充/不透明度写 CSS;清除 = 移除声明;
/// 多选 Compound 一条 undo 同时逆回两个对象(拉开合并窗口使
/// 「填充」「不透明度」为独立条目,断言不受合并语义干扰)。
#[test]
fn appearance_fill_opacity_and_multi_select_compound() {
    let (mut doc, sid) = box_doc();
    // 第二个盒子(多选)
    let ab = doc.artboards.first().copied().unwrap();
    let sid2 = doc.alloc_sid();
    let n = Node::new(NodeKind::Box, "盒子2", sid2.clone());
    let id2 = doc.nodes.insert(n);
    doc.nodes.get_mut(id2).unwrap().parent = Some(ab);
    doc.nodes.get_mut(ab).unwrap().children.push(id2);
    let sids = vec![sid.clone(), sid2.as_str().to_string()];
    let mut stack = UndoStack::new();

    // vb-token-ok: 文档内容测试数据(非 UI 皮肤色)
    let cmd = combine(style_prop_cmds(&doc, &sids, "background-color", "#ff5a1f")).unwrap();
    push(&mut stack, &mut doc, cmd);
    // 拉开 >500ms 合并窗口:使「不透明度」成为独立 undo 条目
    std::thread::sleep(std::time::Duration::from_millis(600));
    let cmd = combine(style_prop_cmds(&doc, &sids, "opacity", "0.5")).unwrap();
    push(&mut stack, &mut doc, cmd);
    // vb-token-ok: 文档内容测试数据
    assert_eq!(decl(style_of(&doc, &sid), "background-color"), "#ff5a1f");
    assert_eq!(decl(style_of(&doc, &sid), "opacity"), "0.5");
    assert_eq!(decl(style_of(&doc, sid2.as_str()), "opacity"), "0.5");

    // 多选一条 undo:两个对象的 opacity 同步逆回(填充不动)
    stack.undo(&mut doc).unwrap();
    assert!(
        style_of(&doc, &sid).iter().all(|d| d.prop != "opacity"),
        "一条 undo 应逆回主选中"
    );
    assert!(
        style_of(&doc, sid2.as_str())
            .iter()
            .all(|d| d.prop != "opacity"),
        "一条 undo 应同步逆回第二条目标"
    );
    // vb-token-ok: 文档内容测试数据
    assert_eq!(decl(style_of(&doc, &sid), "background-color"), "#ff5a1f");

    // 清除:声明整条移除;重复清除不再产生命令(不刷 undo 栈)
    let cmd = combine(style_prop_cmds(&doc, &sids, "opacity", "0.7")).unwrap();
    push(&mut stack, &mut doc, cmd);
    let cmds = style_prop_remove_cmds(&doc, &sids, "opacity");
    assert_eq!(cmds.len(), 2, "两个目标各一条");
    let cmd = combine(cmds).unwrap();
    push(&mut stack, &mut doc, cmd);
    assert!(style_of(&doc, &sid).iter().all(|d| d.prop != "opacity"));
    assert!(style_prop_remove_cmds(&doc, &sids, "opacity").is_empty());
}

/// 门 2(交互/无障碍):href、target、alt、aria-label 写 attrs;
/// 空值 = 删除;导出 HTML 里可见。
#[test]
fn interaction_and_a11y_attrs_written_to_document() {
    let (mut doc, sid) = box_doc();
    let mut stack = UndoStack::new();
    let sids = vec![sid.clone()];
    for (k, v) in [
        ("href", "https://example.com"),
        ("target", "_blank"),
        ("alt", "封面图"),
        ("aria-label", "主标题"),
    ] {
        for cmd in attr_cmds(&doc, &sids, k, v) {
            push(&mut stack, &mut doc, cmd);
        }
    }
    {
        let n = doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap();
        for (k, v) in [
            ("href", "https://example.com"),
            ("target", "_blank"),
            ("alt", "封面图"),
            ("aria-label", "主标题"),
        ] {
            assert_eq!(n.attrs.get(k).map(String::as_str), Some(v), "attr {k}");
        }
    }
    // 删除语义:空值移除且已删后不再产生命令
    let cmds = attr_cmds(&doc, &sids, "target", "");
    assert_eq!(cmds.len(), 1);
    for cmd in cmds {
        push(&mut stack, &mut doc, cmd);
    }
    assert!(attr_cmds(&doc, &sids, "target", "").is_empty());
    let html = vb_doc::export::render_project(&doc).files;
    assert!(
        html.iter()
            .any(|(_, c)| c.contains("aria-label=\"主标题\"")),
        "导出 HTML 应携带无障碍属性"
    );
}

/// 门 2(导出组):图层名改名 → data-vb-name;文档标题 → `<title>`;
/// 两者均可撤销。
#[test]
fn export_group_name_and_doc_title_are_real_document_state() {
    let (mut doc, sid) = box_doc();
    let mut stack = UndoStack::new();
    let sids = vec![sid.clone()];
    let cmd = combine(rename_cmds(&doc, &sids, "英雄区")).unwrap();
    push(&mut stack, &mut doc, cmd);
    push(
        &mut stack,
        &mut doc,
        Command::SetMetaTitle {
            new: "产品官网".into(),
            old: None,
        },
    );
    assert_eq!(
        doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap().name,
        "英雄区"
    );
    let html = vb_doc::export::render_project(&doc).files;
    assert!(html
        .iter()
        .any(|(_, c)| c.contains("data-vb-name=\"英雄区\"")));
    assert!(html
        .iter()
        .any(|(_, c)| c.contains("<title>产品官网</title>")));
    // 撤销文档标题
    stack.undo(&mut doc).unwrap();
    assert_eq!(doc.meta.title, "未命名", "SetMetaTitle 应可撤销");
}

/// 门 2(渐变):角度数值化改写既有渐变;无渐变回退生成;反向 = +180°;
/// 类型切换保留色标。
#[test]
fn gradient_numeric_angle_and_reverse() {
    let (mut doc, sid) = box_doc();
    let mut stack = UndoStack::new();
    let sids = vec![sid.clone()];
    // 无渐变 → 角度提交回退生成「现填充 → 白」
    for cmd in gradient_angle_cmds(&doc, &sids, 30.0) {
        push(&mut stack, &mut doc, cmd);
    }
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "linear-gradient(30deg, #d4d4d4 0%, #ffffff 100%)"
    );
    // 再改角度:只改角度,色标不动
    for cmd in gradient_angle_cmds(&doc, &sids, 120.0) {
        push(&mut stack, &mut doc, cmd);
    }
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "linear-gradient(120deg, #d4d4d4 0%, #ffffff 100%)"
    );
    // 反向:+180°
    for cmd in gradient_reverse_cmds(&doc, &sids) {
        push(&mut stack, &mut doc, cmd);
    }
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "linear-gradient(300deg, #d4d4d4 0%, #ffffff 100%)"
    );
    // 类型切换:线性 → 径向(保留色标)
    for cmd in gradient_kind_cmds(&doc, &sids, GradKind::Radial) {
        push(&mut stack, &mut doc, cmd);
    }
    assert_eq!(
        decl(style_of(&doc, &sid), "background-image"),
        "radial-gradient(circle at 50% 50%, #d4d4d4 0%, #ffffff 100%)"
    );
}

/// 画板工具态字段:预设/取向/尺寸写活动画板 geom;名称走 Rename。
#[test]
fn artboard_state_fields_write_active_artboard() {
    let (mut doc, _sid) = box_doc();
    let ab_sid = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let mut stack = UndoStack::new();
    let sids = vec![ab_sid.clone()];
    // 预设:W+H 一条 SetGeom(逐分量两条会以同一基准互相覆盖)
    let cmd = geom_axes_cmd(
        &doc,
        &ab_sid,
        &[(GeomAxis::W, 1920.0), (GeomAxis::H, 1080.0)],
    )
    .unwrap();
    assert!(
        matches!(cmd, Command::SetGeom { .. }),
        "W+H 应合并为一条 SetGeom"
    );
    push(&mut stack, &mut doc, cmd);
    let g = &doc.nodes.get(doc.artboards[0]).unwrap().geom;
    assert_eq!((g.w, g.h), (1920.0, 1080.0));
    // 取向互换(横 ↔ 竖)
    let cmd = geom_axes_cmd(
        &doc,
        &ab_sid,
        &[(GeomAxis::W, 1080.0), (GeomAxis::H, 1920.0)],
    )
    .unwrap();
    push(&mut stack, &mut doc, cmd);
    let g = &doc.nodes.get(doc.artboards[0]).unwrap().geom;
    assert_eq!((g.w, g.h), (1080.0, 1920.0));
    // 名称走 Rename
    let cmd = combine(rename_cmds(&doc, &sids, "首页")).unwrap();
    push(&mut stack, &mut doc, cmd);
    assert_eq!(doc.nodes.get(doc.artboards[0]).unwrap().name, "首页");
    // 三次 undo 逐步逆回
    stack.undo(&mut doc).unwrap();
    stack.undo(&mut doc).unwrap();
    stack.undo(&mut doc).unwrap();
    let g = &doc.nodes.get(doc.artboards[0]).unwrap().geom;
    assert_eq!((g.w, g.h), (1440.0, 900.0), "三次 undo 应回到默认画板尺寸");
}

/// combine 收敛规则:单条不包 Compound,空集为 None。
#[test]
fn combine_of_single_is_not_wrapped() {
    let (doc, sid) = box_doc();
    let cmds = rename_cmds(&doc, &[sid], "只改一个");
    assert_eq!(cmds.len(), 1);
    assert!(matches!(combine(cmds), Some(Command::Rename { .. })));
    assert!(combine(Vec::new()).is_none());
}

/// 锚点写回:SetVector 只动目标元素(直接选择数值化共用路径)。
#[test]
fn anchor_numeric_edit_moves_only_target_element() {
    use vb_common::geom::{BezPath, PathEl, Point};
    let (mut doc, _sid) = box_doc();
    let ab = doc.artboards.first().copied().unwrap();
    let vsid = doc.alloc_sid();
    let mut path = BezPath::new();
    path.move_to(Point::new(0.0, 0.0));
    path.line_to(Point::new(100.0, 0.0));
    let mut n = Node::new(NodeKind::Vector { path }, "路径", vsid.clone());
    n.parent = Some(ab);
    let vid = doc.nodes.insert(n);
    doc.nodes.get_mut(ab).unwrap().children.push(vid);
    let vsid_s = vsid.as_str().to_string();

    // 与控制面板锚点字段同款写法:重写第 1 个元素的终点
    let nid = doc.find_by_sid(&vsid_s).unwrap();
    let mut els: Vec<PathEl> = match &doc.nodes.get(nid).unwrap().kind {
        NodeKind::Vector { path } => path.elements().to_vec(),
        _ => unreachable!(),
    };
    els[1] = PathEl::LineTo(Point::new(150.0, 25.0));
    let mut np = BezPath::new();
    for el in els {
        np.push(el);
    }
    let mut stack = UndoStack::new();
    push(
        &mut stack,
        &mut doc,
        Command::SetVector {
            sid: vsid_s.clone(),
            new: np,
            old: None,
        },
    );
    let nid = doc.find_by_sid(&vsid_s).unwrap();
    match &doc.nodes.get(nid).unwrap().kind {
        NodeKind::Vector { path } => {
            let els = path.elements().to_vec();
            assert_eq!(els[0], PathEl::MoveTo(Point::new(0.0, 0.0)), "其余元素不动");
            assert_eq!(els[1], PathEl::LineTo(Point::new(150.0, 25.0)));
        }
        _ => unreachable!(),
    }
}

/// NodeTree 克隆辅助与命令层同构(导入健康检查)。
#[test]
fn node_tree_import_still_valid() {
    let (doc, sid) = box_doc();
    let nid = doc.find_by_sid(&sid).unwrap();
    let tree = NodeTree::from_document(&doc, nid).expect("可克隆子树");
    assert_eq!(tree.node.sid.as_str(), sid);
}
