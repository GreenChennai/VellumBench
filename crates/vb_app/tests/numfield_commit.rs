//! NumField 提交会话的 undo 合并测试(02-6-2 验收门 1)。
//!
//! 「文档状态级」:走 `vb_app` 面板与数值框共用的同一条命令路径
//! (`UndoStack::push(SetGeom…)`),用真实时钟拉开 >500ms 的间隔,
//! 断言 **UndoStack 条数**(逐条 `undo()` 数出来,顺带验证每步可逆)。
//!
//! 会话开合语义 = `vb_app` 面板层 `num_commit` 助手对 NumField 响应
//! (`scrub_started/scrub_ended/focus_lost`)的折算:
//! `scrub_started 或首次 changed → begin_session`;
//! `scrub_ended 或 focus_lost → end_session`。

use std::time::Duration;

use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom};
use vb_doc::undo::UndoStack;

/// 合并窗口:与 `vb_doc::undo` 内部常量一致(用 >窗口 的间隔做验证)。
const MERGE_WINDOW: Duration = Duration::from_millis(500);

fn geom(x: f64) -> Geom {
    Geom {
        x,
        y: 10.0,
        w: 50.0,
        h: 60.0,
    }
}

fn set_geom(sid: &str, x: f64) -> Command {
    Command::SetGeom {
        sid: sid.to_string(),
        new: geom(x),
        old: None,
        old_declared: None,
    }
}

/// 在默认画板上放一个盒子,返回 sid(与 commands_tests 同款插入方式)。
fn make_box(doc: &mut Document) -> String {
    let ab = doc.artboards.first().copied().expect("默认文档有一块画板");
    let sid = doc.alloc_sid();
    let mut n = vb_doc::model::Node::new(vb_doc::model::NodeKind::Box, "盒子", sid.clone());
    n.geom = geom(10.0);
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(ab);
    doc.nodes.get_mut(ab).unwrap().children.push(id);
    sid.as_str().to_string()
}

/// 文档状态级数栈:逐条 undo 直到空,返回条数(每条都必须成功回退)。
fn undo_entries(stack: &mut UndoStack, doc: &mut Document) -> usize {
    let mut n = 0;
    while stack.can_undo() {
        // undo() 返回回退后新栈顶的「会撤销什么」;弹空时为 None,属正常
        stack.undo(doc).expect("undo 不应失败");
        n += 1;
    }
    n
}

/// 连续 scrubby(每帧一条 SetGeom,帧间隔超过 500ms 窗口)在
/// **提交会话内**必须合并为一条 undo;松手(end_session)后的下一次
/// 拖拽是新的一条。
#[test]
fn scrub_burst_within_session_is_one_undo_entry() {
    let mut doc = Document::new_default();
    let sid = make_box(&mut doc);
    let mut stack = UndoStack::new();

    // 拖拽开始(scrub_started → begin_session)
    stack.begin_session();
    assert!(stack.session_active());
    for x in [12.0, 24.0, 48.0] {
        std::thread::sleep(MERGE_WINDOW + Duration::from_millis(50));
        stack.push(&mut doc, set_geom(&sid, x)).expect("push ok");
    }
    assert_eq!(
        doc.nodes
            .get(doc.find_by_sid(&sid).unwrap())
            .unwrap()
            .geom
            .x,
        48.0
    );

    // 松手(scrub_ended → end_session)
    stack.end_session();
    assert!(!stack.session_active());

    // 第二次独立拖拽(先 begin 后 push;间隔再大也只有这一条)
    std::thread::sleep(MERGE_WINDOW + Duration::from_millis(50));
    stack.begin_session();
    stack.push(&mut doc, set_geom(&sid, 96.0)).expect("push ok");
    stack.end_session();

    assert_eq!(
        undo_entries(&mut stack, &mut doc),
        2,
        "两次拖拽各合并为一条 undo(慢速拖动不得刷栈)"
    );
    // 最终文档回到初始位置
    assert_eq!(
        doc.nodes
            .get(doc.find_by_sid(&sid).unwrap())
            .unwrap()
            .geom
            .x,
        10.0
    );
}

/// 键盘连续步进/连续表达式提交(每次一条命令,间隔超过 500ms)
/// 在同一会话(字段保持焦点)内合并为一条;失焦结束会话后
/// 相邻的下一次编辑**不**并进旧条目。
#[test]
fn keyboard_burst_merges_but_stops_after_focus_loss() {
    let mut doc = Document::new_default();
    let sid = make_box(&mut doc);
    let mut stack = UndoStack::new();

    // 字段聚焦期间的连续步进(↑ ×3,每次间隔超窗口)
    stack.begin_session();
    for x in [11.0, 12.0, 13.0] {
        std::thread::sleep(MERGE_WINDOW + Duration::from_millis(30));
        stack.push(&mut doc, set_geom(&sid, x)).expect("push ok");
    }
    // 失焦(focus_lost → end_session)
    stack.end_session();

    // 点击画布后又拖了一次对象(无会话;>500ms 间隔 → 独立一条)
    std::thread::sleep(MERGE_WINDOW + Duration::from_millis(30));
    stack.push(&mut doc, set_geom(&sid, 99.0)).expect("push ok");

    assert_eq!(
        undo_entries(&mut stack, &mut doc),
        2,
        "会话内 3 次步进 = 1 条;失焦后下一次编辑 = 另 1 条"
    );
}

/// 对照组:**无会话**时超过 500ms 的两次 push 必须拆成两条
/// (会话是合并的必要条件,不是无条件吞并)。
#[test]
fn without_session_slow_edits_stay_separate() {
    let mut doc = Document::new_default();
    let sid = make_box(&mut doc);
    let mut stack = UndoStack::new();

    stack.push(&mut doc, set_geom(&sid, 20.0)).expect("push ok");
    std::thread::sleep(MERGE_WINDOW + Duration::from_millis(50));
    stack.push(&mut doc, set_geom(&sid, 30.0)).expect("push ok");

    assert_eq!(undo_entries(&mut stack, &mut doc), 2, "无会话不合并");
}

/// 会话期间**换目标**(同字段会话中改了另一个对象)不误并:
/// merge_target 不同照样拆条(会话只放宽时间窗,不放宽目标匹配)。
#[test]
fn session_does_not_merge_across_targets() {
    let mut doc = Document::new_default();
    let a = make_box(&mut doc);
    let b = make_box(&mut doc);
    let mut stack = UndoStack::new();

    stack.begin_session();
    stack.push(&mut doc, set_geom(&a, 5.0)).expect("push ok");
    std::thread::sleep(MERGE_WINDOW + Duration::from_millis(30));
    stack.push(&mut doc, set_geom(&b, 7.0)).expect("push ok");
    stack.end_session();

    assert_eq!(undo_entries(&mut stack, &mut doc), 2, "换目标必拆条");
}
