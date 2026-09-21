//! S4-b 渐变 / 透明度 / 颜色面板的**文档状态级**验收门(副文档 05 §4 步骤单)。
//!
//! 「文档状态级」= 走 UI 与 Agent 共用的同一条命令路径:纯函数构建器
//! (`gradient_panel` / `opacity_panel` / `color_panel`)产出命令 →
//! `UndoStack::push` → 断言 CSS 声明 / 外观模型属性 / 文档令牌 →
//! **导出 → 重导入 → 再导出逐字节相同**(L1 幂等,判据 6)。

use std::path::PathBuf;

use vb_app::app::appearance::{add_fill_cmd, decode_model, AppearanceItem, FillBody};
use vb_app::app::color_panel::{
    add_global_cmd, delete_global_cmd, global_colors, read_target_color, set_global_cmd, swap_cmd,
    write_color_cmd,
};
use vb_app::app::gradient_panel::{project, write_cmd, GradSink};
use vb_app::app::opacity_panel;
use vb_css::Decl;
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, Node, NodeKind};
use vb_doc::undo::UndoStack;
use vb_ui::gradient::{self, GradKind};

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
    let mut doc = Document::new_default();
    let sid = make_node(&mut doc, NodeKind::Box, "盒子");
    (doc, UndoStack::new(), sid)
}

/// 两段式推送:先构建命令(不可变借用),再入栈(可变借用)——规避借用冲突。
/// 离散操作纪律:禁用合并(与 UI `exec_*` 一致,一次点击 = 一条 undo)。
fn push<F>(stack: &mut UndoStack, doc: &mut Document, build: F)
where
    F: FnOnce(&Document) -> Command,
{
    let cmd = build(doc);
    stack.merging_enabled = false;
    stack.push(doc, cmd).expect("命令应成功应用");
    stack.merging_enabled = true;
}

fn style_of<'a>(doc: &'a Document, sid: &str) -> &'a Vec<Decl> {
    &doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().style
}

fn decl<'a>(style: &'a [Decl], prop: &str) -> Option<&'a str> {
    style
        .iter()
        .find(|d| d.prop == prop)
        .map(|d| d.value.as_str().trim())
}

/// 一轮导出的文件表 `(相对路径, 内容)`。
type Files = Vec<(String, String)>;

/// 导出 → 重导入 → 再导出,返回两轮文件表(判据 6 的 L1 门)。
fn l1_roundtrip(doc: &Document, tag: &str) -> (Files, Files) {
    let dir: PathBuf = std::env::temp_dir().join(format!("vb-s4b-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out1 = vb_doc::export::render_project(doc);
    for (rel, content) in &out1.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let r2 = vb_doc::import::import_project(&dir).unwrap();
    let out2 = vb_doc::export::render_project(&r2.doc);
    let _ = std::fs::remove_dir_all(&dir);
    (out1.files, out2.files)
}

fn assert_l1(files: &[(String, String)], files2: &[(String, String)]) {
    for (a, b) in files.iter().zip(files2.iter()) {
        assert_eq!(a.1, b.1, "{} 两轮导出字节不同(L1 幂等破损)", a.0);
    }
}

fn css_of(files: &[(String, String)]) -> String {
    files
        .iter()
        .find(|(p, _)| p == "styles/main.css")
        .map(|(_, c)| c.clone())
        .unwrap_or_default()
}

// ────────────────── 门 1:渐变写回(层保真 / 外观模型同源) ──────────────────

/// 多层背景里改渐变:**只替换渐变层**,`url()` 层不丢,且往返仍幂等。
#[test]
fn gradient_edit_preserves_other_background_layers() {
    let (mut doc, mut stack, sid) = box_doc();
    let style = vec![Decl::parse(
        "background-image: url(a.png), linear-gradient(90deg, #000 0%, #fff 100%)",
    )
    .unwrap()];
    doc.nodes
        .get_mut(doc.find_by_sid(&sid).unwrap())
        .unwrap()
        .style = style;

    // 外观模型已认领背景层 → 走 Fill 条目(渐变那条),`url()` 那条保持不动
    let p = project(&doc, &sid).unwrap();
    assert!(
        matches!(p.sink, GradSink::FillItem { .. }),
        "多层背景应落在外观填充条目"
    );
    assert!(
        p.gradient().is_some(),
        "应能从填充条目里解析出渐变:{:?}",
        p.layers
    );

    push(&mut stack, &mut doc, |d| {
        let p = project(d, &sid).unwrap();
        let mut g = p.gradient().expect("应解析出渐变");
        g.angle = 45.0;
        g.angle_explicit = true;
        let layers = p.with_layer(&g);
        write_cmd(d, &p, &layers).unwrap().unwrap()
    });

    let (f1, f2) = l1_roundtrip(&doc, "layers");
    assert_l1(&f1, &f2);
    let css = css_of(&f1);
    assert!(css.contains("url(a.png)"), "url 层必须保留:{css}");
    assert!(css.contains("linear-gradient(45deg"), "角度应已更新:{css}");
}

/// 有外观填充条目时,渐变写回**外观模型**(与外观面板同源,不打架)。
#[test]
fn gradient_edit_goes_through_appearance_model() {
    let (mut doc, mut stack, sid) = box_doc();
    push(&mut stack, &mut doc, |d| {
        add_fill_cmd(
            d,
            &sid,
            FillBody::Solid {
                value: "#ff0000".into(),
            },
        )
        .unwrap()
        .unwrap()
    });

    let p = project(&doc, &sid).unwrap();
    assert!(
        matches!(p.sink, GradSink::FillItem { .. }),
        "应落到外观填充条目"
    );
    push(&mut stack, &mut doc, |d| {
        let p = project(d, &sid).unwrap();
        let g = gradient::parse("linear-gradient(120deg, #f00 0%, #00f 100%)").unwrap();
        let layers = p.with_layer(&g);
        write_cmd(d, &p, &layers).unwrap().unwrap()
    });

    let n = doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap();
    let attrs: Vec<(String, String)> = n
        .attrs
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let model = decode_model(&n.kind, &n.style, &attrs);
    let body = model.items.iter().find_map(|it| match it {
        AppearanceItem::Fill(f) => Some(&f.body),
        _ => None,
    });
    match body {
        Some(FillBody::Gradient { value }) => {
            // 面板写入的是 canonical 形式(#fff→#f00 已最短;`0%`→`0`)
            assert_eq!(value, "linear-gradient(120deg, #f00 0, #00f 100%)");
        }
        other => panic!("填充条目应变为渐变:{other:?}"),
    }
}

/// 反向 / 类型切换 / 增删色标在结构化模型上往返自洽(纯模型级)。
#[test]
fn gradient_model_ops_are_consistent() {
    let src = "linear-gradient(45deg, #000 0%, #ff0 50%, #fff 100%)";
    let mut g = gradient::parse(src).unwrap();
    assert_eq!(
        g.to_css(),
        "linear-gradient(45deg, #000 0, #ff0 50%, #fff 100%)"
    );
    g.reverse();
    assert_eq!(
        g.to_css(),
        "linear-gradient(225deg, #000 0, #ff0 50%, #fff 100%)"
    );
    g.kind = GradKind::Radial;
    g.head = Some("circle at 50% 50%".into());
    assert!(g
        .to_css()
        .starts_with("radial-gradient(circle at 50% 50%, "));
    g.stops.push(vb_ui::gradient::Stop::new(0.75, "#0f0"));
    g.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    let css = g.to_css();
    assert!(css.contains("#0f0 75%"), "{css}");
    // 中点提示往返
    let with_hint = gradient::parse("linear-gradient(90deg, #000 0%, 30%, #fff 100%)").unwrap();
    assert_eq!(with_hint.hints, vec![(0, 0.3)]);
    assert_eq!(
        with_hint.to_css(),
        "linear-gradient(90deg, #000 0, 30%, #fff 100%)"
    );
}

/// 渐变编辑后 **L1 字节幂等**(判据 6)。
#[test]
fn gradient_edit_survives_roundtrip_byte_identical() {
    let (mut doc, mut stack, sid) = box_doc();
    push(&mut stack, &mut doc, |d| {
        let p = project(d, &sid).unwrap();
        let g = gradient::parse("linear-gradient(60deg, #123456 0%, #ffffff 100%)").unwrap();
        let layers = p.with_layer(&g);
        write_cmd(d, &p, &layers).unwrap().unwrap()
    });

    let (f1, f2) = l1_roundtrip(&doc, "grad");
    assert_l1(&f1, &f2);
    assert!(css_of(&f1).contains("linear-gradient(60deg"));
}

// ────────────────── 门 2:透明度面板命令(合法 CSS) ──────────────────

#[test]
fn opacity_panel_props_are_written_and_legal() {
    let (mut doc, mut stack, sid) = box_doc();
    push(&mut stack, &mut doc, |d| {
        opacity_panel::set_prop_cmd(d, &sid, "opacity", Some("0.42")).unwrap()
    });
    push(&mut stack, &mut doc, |d| {
        opacity_panel::set_prop_cmd(d, &sid, "mix-blend-mode", Some("multiply")).unwrap()
    });
    push(&mut stack, &mut doc, |d| {
        opacity_panel::knockout_cmd(d, &sid, true).unwrap()
    });
    push(&mut stack, &mut doc, |d| {
        opacity_panel::set_prop_cmd(
            d,
            &sid,
            "mask-image",
            Some("linear-gradient(#000 0, #0000 100%)"),
        )
        .unwrap()
    });

    {
        let s = style_of(&doc, &sid);
        assert_eq!(decl(s, "opacity"), Some("0.42"));
        assert_eq!(decl(s, "mix-blend-mode"), Some("multiply"));
        assert_eq!(decl(s, "isolation"), Some("isolate"));
        assert!(decl(s, "mask-image")
            .unwrap()
            .contains("linear-gradient(#000 0"));
    }

    // 全部属性都必须在白名单内(否则导出会落到"未知属性"通道)
    for prop in ["opacity", "mix-blend-mode", "isolation", "mask-image"] {
        assert!(vb_css::is_known_prop(prop), "{prop} 不在 L1 白名单");
    }

    // 制作 → 反转 → 释放
    assert!(opacity_panel::invert_mask_cmd(&doc, &sid).is_ok());
    push(&mut stack, &mut doc, |d| {
        opacity_panel::invert_mask_cmd(d, &sid).unwrap().unwrap()
    });
    let v = decl(style_of(&doc, &sid), "mask-image")
        .unwrap()
        .to_string();
    assert!(v.contains("0deg"), "反转后方向应显式翻转:{v}");
    push(&mut stack, &mut doc, |d| {
        opacity_panel::set_prop_cmd(d, &sid, "mask-image", None).unwrap()
    });
    assert!(
        decl(style_of(&doc, &sid), "mask-image").is_none(),
        "释放应删除声明"
    );

    let (f1, f2) = l1_roundtrip(&doc, "opacity");
    assert_l1(&f1, &f2);
}

/// 挖空组关闭 = 移除 `isolation`(不留空值声明)。
#[test]
fn knockout_off_removes_declaration() {
    let (mut doc, mut stack, sid) = box_doc();
    push(&mut stack, &mut doc, |d| {
        opacity_panel::knockout_cmd(d, &sid, true).unwrap()
    });
    push(&mut stack, &mut doc, |d| {
        opacity_panel::knockout_cmd(d, &sid, false).unwrap()
    });
    assert!(decl(style_of(&doc, &sid), "isolation").is_none());
}

/// 无蒙版时反转必须给提示而非静默。
#[test]
fn invert_mask_without_mask_reports() {
    let (doc, _, sid) = box_doc();
    let e = opacity_panel::invert_mask_cmd(&doc, &sid).unwrap_err();
    assert!(e.contains("还没有蒙版"), "{e}");
}

// ────────────────── 门 3:颜色面板(填充/描边/交换/默认/全局色) ──────────────────

#[test]
fn color_target_read_write_swap_default() {
    let (mut doc, mut stack, sid) = box_doc();
    push(&mut stack, &mut doc, |d| {
        write_color_cmd(d, &sid, false, "#ff0000").unwrap().unwrap()
    });
    push(&mut stack, &mut doc, |d| {
        write_color_cmd(d, &sid, true, "#0000ff").unwrap().unwrap()
    });
    assert_eq!(read_target_color(&doc, &sid, false).unwrap(), "#ff0000");
    assert_eq!(read_target_color(&doc, &sid, true).unwrap(), "#0000ff");

    // Shift+X 交换
    push(&mut stack, &mut doc, |d| {
        swap_cmd(d, &sid).unwrap().unwrap()
    });
    assert_eq!(read_target_color(&doc, &sid, false).unwrap(), "#0000ff");
    assert_eq!(read_target_color(&doc, &sid, true).unwrap(), "#ff0000");

    // D 默认
    push(&mut stack, &mut doc, |d| {
        write_color_cmd(d, &sid, false, "#ffffff").unwrap().unwrap()
    });
    assert_eq!(read_target_color(&doc, &sid, false).unwrap(), "#ffffff");
}

/// 全局色 = CSS 变量:建令牌 → 写 `var(--x)` → 改令牌值 → `:root` 变、用量不变(全站生效)。
#[test]
fn global_color_token_drives_root_and_usages() {
    let (mut doc, mut stack, sid) = box_doc();
    push(&mut stack, &mut doc, |d| add_global_cmd(d, "#123456"));
    let g = global_colors(&doc);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].0, "vb-color-1");

    push(&mut stack, &mut doc, |d| {
        write_color_cmd(d, &sid, false, "var(--vb-color-1)")
            .unwrap()
            .unwrap()
    });
    let (f1, _) = l1_roundtrip(&doc, "var");
    let css = css_of(&f1);
    assert!(css.contains("--vb-color-1: #123456"), "{css}");
    assert!(css.contains("background-color: var(--vb-color-1)"), "{css}");

    // 改令牌值 → 写用点仍是 var(),`:root` 值变(全站生效机制)
    push(&mut stack, &mut doc, |_| {
        set_global_cmd("vb-color-1", "#00ff00")
    });
    let (f2, _) = l1_roundtrip(&doc, "var2");
    let css2 = css_of(&f2);
    assert!(css2.contains("--vb-color-1: #00ff00"), "{css2}");
    assert!(
        css2.contains("background-color: var(--vb-color-1)"),
        "{css2}"
    );

    // 删除令牌
    push(&mut stack, &mut doc, |_| delete_global_cmd("vb-color-1"));
    assert!(global_colors(&doc).is_empty());
}

/// 冻结对象改色必须报错(不静默)。
#[test]
fn frozen_node_color_change_is_rejected() {
    let mut doc = Document::new_default();
    let sid = make_node(
        &mut doc,
        NodeKind::Frozen {
            html: "<svg/>".into(),
        },
        "冻结",
    );
    let e = write_color_cmd(&doc, &sid, false, "#ff0000").unwrap_err();
    assert!(e.contains("冻结"), "{e}");
}

/// 渐变面板对文字/矢量/冻结对象给出**说明性提示**,不静默。
#[test]
fn gradient_rejects_unsupported_targets_with_message() {
    let mut doc = Document::new_default();
    let text = make_node(
        &mut doc,
        NodeKind::Text {
            text: "甲".into(),
            segments: Vec::new(),
            mode: vb_doc::model::TextMode::Point,
        },
        "文字",
    );
    let p = project(&doc, &text).unwrap();
    let e = write_cmd(
        &doc,
        &p,
        &["linear-gradient(0deg, #000 0%, #fff 100%)".into()],
    )
    .unwrap_err();
    assert!(e.contains("v2"), "文字渐变应给 v2 提示:{e}");

    let frozen = make_node(
        &mut doc,
        NodeKind::Frozen {
            html: "<pre/>".into(),
        },
        "冻结",
    );
    let p2 = project(&doc, &frozen).unwrap();
    let e2 = write_cmd(
        &doc,
        &p2,
        &["linear-gradient(0deg, #000 0%, #fff 100%)".into()],
    )
    .unwrap_err();
    assert!(e2.contains("冻结"), "{e2}");
}
