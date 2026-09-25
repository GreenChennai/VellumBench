//! 05-9 动效时间轴一致性门禁(台账 09-I;ADR-VB-L11;副文档 05 §4.9)。
//!
//! 门禁②(预览 vs 导出逐帧抽样):**同一时刻 t,预览求值结果 == 导出帧
//! 求值结果**。预览侧 = 画布路径(`encode_artboard` → `anim::apply_frame_state`
//! → `rasterize_rgba`),导出侧 = `ExportContext::build` 的 GIF 逐帧光栅 ——
//! 两侧共用同一求值入口 [`vb_kiln::anim::apply_frame_state`],此处以**像素级
//! 相等**断言锁死(同一确定性 CPU 栅格,同路径必等;若有分叉立即红)。
//!
//! 门禁③(hold 定格语义保持):fill both 下动画结束后帧**保持终值**不回
//! 静态 —— 对应 WPI「有限动画 finish 定格」在自研逐帧侧的语义,时间轴
//! 产物不得破坏(求值级断言见 `anim.rs::tests::fill_both_holds_final_value_after_end`)。

use std::collections::HashMap;

use vb_css::Decl;
use vb_doc::model::{Document, Node, NodeKind, NodeTree};
use vb_kiln::anim::{apply_frame_state, parse_keyframes, resolve_node_anim};
use vb_kiln::{ExportContext, ExportRequest, Format};

/// 构造:文档 + 400×300 画板 + 一个 200×100 盒,带时间轴命名动画
/// (translateX 0→100px + opacity 1→0,1s,fill both)。
fn doc_with_timeline_anim() -> (Document, String, vb_doc::model::NodeId) {
    let mut doc = Document::new_default();
    let ab = doc.artboards[0];
    let sid = doc.alloc_sid();
    let parent_sid = doc.nodes.get(ab).unwrap().sid.as_str().to_string();
    let mut node = Node::new(NodeKind::Box, "动盒", sid.clone());
    node.geom = vb_doc::model::Geom {
        x: 40.0,
        y: 40.0,
        w: 200.0,
        h: 100.0,
    };
    node.style = vec![
        Decl {
            prop: "background-color".into(),
            value: "#334455".into(),
            important: false,
        },
        Decl {
            prop: "animation".into(),
            value: format!("vb-anim-{} 1000ms linear 0ms 1 both", sid.as_str()),
            important: false,
        },
    ];
    let tree = NodeTree {
        node,
        children: vec![],
    };
    doc.insert_tree_at(&tree, &parent_sid, 0).expect("插盒成功");
    doc.sync_artboards();
    doc.raw_css.push(format!(
        "@keyframes vb-anim-{} {{\n  \
         0% {{ transform: translateX(0px) translateY(0px); opacity: 1; animation-timing-function: linear; }}\n  \
         100% {{ transform: translateX(100px) translateY(0px); opacity: 0; animation-timing-function: linear; }}\n\
         }}",
        sid.as_str()
    ));
    let ab_id = doc.artboards[0];
    (doc, sid.as_str().to_string(), ab_id)
}

/// 预览路径:画布同款(编码 → 应用 t 时刻动画 → 光栅)。
fn preview_frame(
    doc: &Document,
    ab: vb_doc::model::NodeId,
    anims: &HashMap<String, vb_kiln::anim::NodeAnim>,
    t: f64,
    scale: u32,
) -> Vec<u8> {
    let mut list = vb_render::encode::encode_artboard(doc, ab).expect("编码成功");
    apply_frame_state(anims, &mut list, t);
    vb_kiln::raster::rasterize_rgba(&list, scale as f64).expect("光栅成功")
}

/// 门禁②:同一时刻 t,预览求值 == 导出帧(像素级相等)。
#[test]
fn preview_frames_equal_export_frames_pixelwise() {
    let (doc, sid, ab) = doc_with_timeline_anim();

    // 导出侧:GIF 逐帧(fps=10,时长 1s = 动画时长;scale 1 控制体积)
    let req = ExportRequest {
        format: Format::Gif,
        scale: 1,
        fps: 10,
        duration_s: 1.0,
        ..ExportRequest::default()
    };
    let ctx = ExportContext::build(&doc, ab, &req, None).expect("导出上下文构建成功");
    assert_eq!(
        ctx.frames.len(),
        10,
        "1s @10fps 必须 10 帧(动画实例存在,逐帧路径)"
    );

    // 预览侧:与 ExportContext::build 同款实例表(parse + resolve,逐画板)
    let kf = parse_keyframes(&doc.raw_css);
    let nid = doc.find_by_sid(&sid).expect("节点存在");
    let na = resolve_node_anim(&doc, nid, &kf).expect("动画实例可解析");
    let mut anims = HashMap::new();
    anims.insert(sid.clone(), na);

    // 逐帧抽样比对:同一 t,像素必须完全相等(同求值入口 + 同栅格)
    for (k, frame) in ctx.frames.iter().enumerate() {
        let t = k as f64 / 10.0;
        let preview = preview_frame(&doc, ab, &anims, t, 1);
        assert_eq!(
            preview.len(),
            frame.rgba.len(),
            "帧 {k} 尺寸不一致:预览 {} vs 导出 {}",
            preview.len(),
            frame.rgba.len()
        );
        let diff = preview
            .iter()
            .zip(frame.rgba.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            diff, 0,
            "帧 {k}(t={t}s)预览与导出像素不一致({} 字节不同)",
            diff
        );
    }
}

/// 门禁②(求值级):动画中途帧 != 静态帧(动画确实生效,防止
/// 「两边都恒等于静态」的假一致)。
#[test]
fn animation_actually_changes_over_time() {
    let (doc, sid, ab) = doc_with_timeline_anim();
    let kf = parse_keyframes(&doc.raw_css);
    let nid = doc.find_by_sid(&sid).expect("节点存在");
    let na = resolve_node_anim(&doc, nid, &kf).expect("动画实例可解析");
    let mut anims = HashMap::new();
    anims.insert(sid, na);
    let t0 = preview_frame(&doc, ab, &anims, 0.0, 1);
    let t_half = preview_frame(&doc, ab, &anims, 0.5, 1);
    let t_end = preview_frame(&doc, ab, &anims, 1.0, 1);
    assert_ne!(t0, t_half, "中途帧必须偏离起点帧(动画生效)");
    assert_ne!(t_half, t_end, "中途帧必须偏离终点帧(动画生效)");
}

/// 门禁③:hold 定格 —— 导出时长(2s)> 动画时长(1s)且 fill both 时,
/// 1s 后的帧保持**动画终值帧**(不回静态、不继续变化)。
#[test]
fn export_holds_final_frame_after_animation_ends() {
    let (doc, _sid, ab) = doc_with_timeline_anim();
    let req = ExportRequest {
        format: Format::Gif,
        scale: 1,
        fps: 10,
        duration_s: 2.0,
        ..ExportRequest::default()
    };
    let ctx = ExportContext::build(&doc, ab, &req, None).expect("导出上下文构建成功");
    assert_eq!(ctx.frames.len(), 20);
    // 终值帧 = 第 10 帧(t=1.0s,动画结束点);其后 9 帧(hold 段)必须逐像素相同
    let last = &ctx.frames[10].rgba;
    for k in 11..20 {
        assert_eq!(
            ctx.frames[k].rgba, *last,
            "帧 {k}(t>动画时长)必须 hold 终值帧(fill both 定格语义)"
        );
    }
    // 且终值帧 ≠ 首帧(动画确实走过,而非全程静态)
    assert_ne!(ctx.frames[0].rgba, *last, "终值帧必须偏离首帧");
}
