//! 图层 Tab 门禁测试(文档状态级:拖放语义 / 标记 / 复合命令可逆)。
//! 06-1 自 `panels/layers.rs` 拆出(纯搬移,零行为变化)。

use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom, Node, NodeKind};

use super::model::*;
use vb_doc::undo::UndoStack;

fn add_box(doc: &mut Document, parent: &str, name: &str, x: f64, y: f64) -> String {
    let sid = doc.alloc_sid();
    let mut n = Node::new(NodeKind::Box, name, sid.clone());
    n.geom = Geom {
        x,
        y,
        w: 100.0,
        h: 50.0,
    };
    let pid = doc.find_by_sid(parent).unwrap();
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(pid);
    doc.nodes.get_mut(pid).unwrap().children.push(id);
    sid.as_str().to_string()
}

fn add_group(doc: &mut Document, parent: &str, name: &str) -> String {
    let sid = doc.alloc_sid();
    let n = Node::new(NodeKind::Group, name, sid.clone());
    let pid = doc.find_by_sid(parent).unwrap();
    let id = doc.nodes.insert(n);
    doc.nodes.get_mut(id).unwrap().parent = Some(pid);
    doc.nodes.get_mut(pid).unwrap().children.push(id);
    sid.as_str().to_string()
}

/// 夹具:画板 AB1(组 G 内含 g1;同级 a、b)+ 画板 AB2(原点 y=1000,盒 c)。
struct Fx {
    doc: Document,
    ab1: String,
    ab2: String,
    g: String,
    g1: String,
    a: String,
    b: String,
    c: String,
}

fn fx() -> Fx {
    let mut doc = Document::new_default();
    let ab1 = doc
        .nodes
        .get(doc.artboards[0])
        .unwrap()
        .sid
        .as_str()
        .to_string();
    let ab2 = doc.new_artboard("画板 2", 800.0, 600.0);
    doc.nodes.get_mut(ab2).unwrap().geom.y = 1000.0;
    let ab2 = doc.nodes.get(ab2).unwrap().sid.as_str().to_string();
    let g = add_group(&mut doc, &ab1, "组");
    let g1 = add_box(&mut doc, &g, "g1", 10.0, 10.0);
    let a = add_box(&mut doc, &ab1, "盒A", 5.0, 200.0);
    let b = add_box(&mut doc, &ab1, "盒B", 300.0, 200.0);
    let c = add_box(&mut doc, &ab2, "盒C", 50.0, 60.0);
    Fx {
        doc,
        ab1,
        ab2,
        g,
        g1,
        a,
        b,
        c,
    }
}

fn children_of(doc: &Document, sid: &str) -> Vec<String> {
    doc.find_by_sid(sid)
        .map(|id| {
            doc.nodes
                .get(id)
                .unwrap()
                .children
                .iter()
                .map(|&c| doc.nodes.get(c).unwrap().sid.as_str().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn parent_of(doc: &Document, sid: &str) -> Option<String> {
    doc.find_by_sid(sid)
        .and_then(|id| doc.nodes.get(id))
        .and_then(|n| n.parent)
        .map(|p| doc.nodes.get(p).unwrap().sid.as_str().to_string())
}

fn geom_of(doc: &Document, sid: &str) -> Geom {
    doc.nodes.get(doc.find_by_sid(sid).unwrap()).unwrap().geom
}

// ── 02-4-1 拖拽重排(命令层等价路径) ──

/// 同父重排:落点 = 末行下缘(index=末位)→ 单条 Move;
/// 层序变化,undo 精确逆回。
#[test]
fn drop_edge_reorders_within_parent_and_reverts() {
    let mut f = fx();
    // ab1 子序 [g, a, b];把 a 落到 b 下缘 → index=3 → [g, b, a]
    let cmds = move_cmds(&f.doc, &f.a, &f.ab1, 3);
    assert_eq!(cmds.len(), 1, "同父重排只需一条 Move");
    assert!(matches!(cmds[0], Command::Move { .. }));
    let mut stack = UndoStack::new();
    for c in cmds {
        stack.push(&mut f.doc, c).unwrap();
    }
    assert_eq!(
        children_of(&f.doc, &f.ab1),
        vec![f.g.clone(), f.b.clone(), f.a.clone()]
    );
    stack.undo(&mut f.doc).unwrap();
    assert_eq!(
        children_of(&f.doc, &f.ab1),
        vec![f.g.clone(), f.a.clone(), f.b.clone()],
        "undo 逆回层序"
    );
}

/// 跨父拖进编组:落点 = 组中带 → Move 进组 + SetGeom 重定基;
/// 世界位置保持;undo 精确逆回(结构与几何)。
#[test]
fn drop_into_group_moves_and_keeps_world_pos() {
    let f = fx();
    let mut doc = f.doc;
    // 组有偏移 (100, 80):组内本地系 = 画板系 - (100, 80)
    let gid = doc.find_by_sid(&f.g).unwrap();
    doc.nodes.get_mut(gid).unwrap().geom = Geom {
        x: 100.0,
        y: 80.0,
        w: 400.0,
        h: 300.0,
    };
    // 落点 = 组中带(Into)→ 索引 = 组子级末尾
    let rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(200.0, 100.0));
    let zones = vec![DropZone {
        rect,
        sid: f.g.clone(),
        parent_sid: f.ab1.clone(),
        container: true,
        index: 0,
    }];
    let target = drop_slot(&doc, &zones, Some(egui::Pos2::new(100.0, 50.0)), &f.a).unwrap();
    let (parent, index) = match &target {
        DropTarget::Into { parent_sid, .. } => {
            let len = doc
                .find_by_sid(parent_sid)
                .map(|p| doc.nodes.get(p).unwrap().children.len())
                .unwrap_or(0);
            (parent_sid.clone(), len)
        }
        _ => panic!("中带应为 Into"),
    };
    let cmds = move_cmds(&doc, &f.a, &parent, index);
    assert_eq!(cmds.len(), 2, "跨父 = Move + SetGeom 重定基");
    // 与面板落下路径一致:整包 Compound(一次 undo 全量逆回)
    let mut stack = UndoStack::new();
    stack.push(&mut doc, Command::Compound { cmds }).unwrap();
    assert_eq!(parent_of(&doc, &f.a).as_deref(), Some(f.g.as_str()));
    let a = geom_of(&doc, &f.a);
    assert_eq!(a.x, 5.0 - 100.0, "世界 X 保持(本地重定基)");
    assert_eq!(a.y, 200.0 - 80.0, "世界 Y 保持(本地重定基)");
    stack.undo(&mut doc).unwrap();
    assert_eq!(parent_of(&doc, &f.a).as_deref(), Some(f.ab1.as_str()));
    let a = geom_of(&doc, &f.a);
    assert_eq!((a.x, a.y), (5.0, 200.0), "undo 逆回几何");
}

/// 跨画板拖拽:盒 C(画板2 原点 y=1000,本地 50,60)拖进画板 1
/// → 世界位置 (50, 1060) 保持;undo 逆回父级。
#[test]
fn drop_across_artboards_keeps_world_pos() {
    let mut f = fx();
    let cmds = move_cmds(&f.doc, &f.c, &f.ab1, usize::MAX);
    assert_eq!(cmds.len(), 2);
    // 与面板落下路径一致:整包 Compound(一次 undo 全量逆回)
    let mut stack = UndoStack::new();
    stack.push(&mut f.doc, Command::Compound { cmds }).unwrap();
    let c = geom_of(&f.doc, &f.c);
    assert_eq!((c.x, c.y), (50.0, 1060.0), "世界位置保持");
    stack.undo(&mut f.doc).unwrap();
    assert_eq!(parent_of(&f.doc, &f.c).as_deref(), Some(f.ab2.as_str()));
}

/// 落点守卫:放进自身行 / 放进自身子树 → None(不下命令)。
#[test]
fn drop_slot_rejects_self_and_own_subtree() {
    let f = fx();
    let r = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(100.0));
    let zones = vec![
        DropZone {
            rect: r,
            sid: f.g.clone(),
            parent_sid: f.ab1.clone(),
            container: true,
            index: 0,
        },
        DropZone {
            rect: r,
            sid: f.g1.clone(),
            parent_sid: f.g.clone(),
            container: false,
            index: 0,
        },
    ];
    let pos = egui::Pos2::new(50.0, 50.0);
    // 拖组放组自身行上 = None
    assert!(drop_slot(&f.doc, &zones, Some(pos), &f.g).is_none());
    // 拖组放到自身子树成员(g1)的行上 = None
    assert!(
        drop_slot(&f.doc, &zones, Some(pos), &f.a).is_some(),
        "无关对象可正常落点"
    );
    // 拖 a 到 g1(不是 a 的子树)→ 命中 g1 行,守卫不拦
    assert!(
        drop_slot(&f.doc, &zones, Some(pos), &f.g).is_none(),
        "组进入自身子树被守卫"
    );
    // 指针在区外 / 无指针 → None
    assert!(drop_slot(&f.doc, &zones, Some(egui::Pos2::new(999.0, 999.0)), &f.a).is_none());
    assert!(drop_slot(&f.doc, &zones, None, &f.a).is_none());
}

/// 落点语义:容器中带 = Into;上/下缘 = Edge(索引与指示线随缘)。
#[test]
fn drop_slot_band_semantics() {
    let f = fx();
    let r = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(200.0, 100.0));
    let zones = vec![DropZone {
        rect: r,
        sid: f.g.clone(),
        parent_sid: f.ab1.clone(),
        container: true,
        index: 0,
    }];
    // 中带(25%~75%)→ Into 组
    let t = drop_slot(&f.doc, &zones, Some(egui::Pos2::new(100.0, 50.0)), &f.a).unwrap();
    assert!(matches!(t, DropTarget::Into { ref parent_sid, .. } if parent_sid == &f.g));
    // 上缘 → Edge 插到组之前(index = 组当前索引 0;指示线在行顶)
    let t = drop_slot(&f.doc, &zones, Some(egui::Pos2::new(100.0, 5.0)), &f.a).unwrap();
    match t {
        DropTarget::Edge { index, line_y, .. } => {
            assert_eq!(index, 0);
            assert_eq!(line_y, 0.0);
        }
        _ => panic!("上缘应为 Edge"),
    }
    // 下缘 → Edge 插到组之后(index = 1;指示线在行底)
    let t = drop_slot(&f.doc, &zones, Some(egui::Pos2::new(100.0, 95.0)), &f.a).unwrap();
    match t {
        DropTarget::Edge { index, line_y, .. } => {
            assert_eq!(index, 1);
            assert_eq!(line_y, 100.0);
        }
        _ => panic!("下缘应为 Edge"),
    }
}

/// 画板拖拽守卫:画板落到其他画板行中带**不产生 Into**
/// (画板嵌画板会脱离导出序;只能 Edge 重排)。
#[test]
fn drop_slot_never_puts_artboard_inside_container() {
    let f = fx();
    let rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(200.0, 100.0));
    let zones = vec![DropZone {
        rect,
        sid: f.ab1.clone(),
        parent_sid: String::new(),
        container: true,
        index: 0,
    }];
    let mid = egui::Pos2::new(100.0, 50.0);
    // 普通对象 → Into 画板(跨画板移动合法)
    let t = drop_slot(&f.doc, &zones, Some(mid), &f.a).unwrap();
    assert!(matches!(t, DropTarget::Into { .. }));
    // 画板 → 中带退化为 Edge(root 内重排),绝不入组
    let t = drop_slot(&f.doc, &zones, Some(mid), &f.ab2).unwrap();
    assert!(matches!(t, DropTarget::Edge { .. }), "画板不得进入容器");
}

// ── 02-4-2 Alt 复制(命令层等价路径) ──

/// Alt 复制:克隆 A 进组 → 新 sid、世界位置保持;undo 后消失。
#[test]
fn alt_dup_inserts_copy_into_group_and_reverts() {
    let mut f = fx();
    let gid = f.doc.find_by_sid(&f.g).unwrap();
    f.doc.nodes.get_mut(gid).unwrap().geom = Geom {
        x: 100.0,
        y: 80.0,
        w: 400.0,
        h: 300.0,
    };
    let (insert, new_sid) = dup_insert_cmd(&mut f.doc, &f.a, &f.g, usize::MAX).unwrap();
    assert_ne!(new_sid, f.a, "复制体必须有自己的稳定 id");
    let mut cmds = vec![insert];
    cmds.extend(rebase_for_new_parent(&f.doc, &f.a, &new_sid, &f.g));
    assert_eq!(cmds.len(), 2, "跨父复制 = Insert + SetGeom 重定基");
    // 与面板落下路径一致:整包 Compound(一次 undo 全量逆回)
    let mut stack = UndoStack::new();
    stack.push(&mut f.doc, Command::Compound { cmds }).unwrap();
    assert!(f.doc.find_by_sid(&new_sid).is_some(), "复制体落盘");
    assert_eq!(
        children_of(&f.doc, &f.g),
        vec![f.g1.clone(), new_sid.clone()]
    );
    let copy = geom_of(&f.doc, &new_sid);
    assert_eq!((copy.x, copy.y), (-95.0, 120.0), "复制体世界位置与源一致");
    stack.undo(&mut f.doc).unwrap();
    assert!(f.doc.find_by_sid(&new_sid).is_none(), "undo 后复制体消失");
}

// ── 07-L:右键菜单「复制 / 删除」的命令路径(文档状态级) ──

/// 菜单「复制」:同父克隆插到自身之后(index+1);undo 后副本消失。
#[test]
fn menu_duplicate_inserts_after_self_in_same_parent() {
    let mut f = fx();
    let a_index = f
        .doc
        .find_by_sid(&f.a)
        .map(|id| {
            let p = f.doc.nodes.get(id).unwrap().parent.unwrap();
            f.doc
                .nodes
                .get(p)
                .unwrap()
                .children
                .iter()
                .position(|&c| c == id)
                .unwrap()
        })
        .unwrap();
    // 与菜单路径一致:dup_insert_cmd(同父)→ Compound(Insert;同父无重定基)
    let (insert, new_sid) = dup_insert_cmd(&mut f.doc, &f.a, &f.ab1, a_index + 1).unwrap();
    let mut stack = UndoStack::new();
    stack
        .push(&mut f.doc, Command::Compound { cmds: vec![insert] })
        .unwrap();
    assert_eq!(
        children_of(&f.doc, &f.ab1),
        vec![f.g.clone(), f.a.clone(), new_sid.clone(), f.b.clone()],
        "副本必须插到原对象之后"
    );
    let copy = geom_of(&f.doc, &new_sid);
    assert_eq!(
        (copy.x, copy.y),
        (5.0, 200.0),
        "同父复制位置不变(本地系一致)"
    );
    stack.undo(&mut f.doc).unwrap();
    assert!(f.doc.find_by_sid(&new_sid).is_none(), "undo 后副本消失");
}

/// 菜单「删除」:Delete 命令移除节点;undo 完整恢复(含槽位)。
#[test]
fn menu_delete_removes_and_undo_restores() {
    let mut f = fx();
    let before = children_of(&f.doc, &f.ab1);
    let mut stack = UndoStack::new();
    stack
        .push(
            &mut f.doc,
            Command::Delete {
                target_sid: f.b.clone(),
                captured: None,
            },
        )
        .unwrap();
    assert!(
        !children_of(&f.doc, &f.ab1).contains(&f.b),
        "删除后从父级消失"
    );
    stack.undo(&mut f.doc).unwrap();
    assert_eq!(
        children_of(&f.doc, &f.ab1),
        before,
        "undo 必须恢复原槽位与层序"
    );
}

// ── 02-4-3 颜色标记(入文档 + 往返) ──

/// 标记循环:None → 首色 → … → 末色 → None;写回 SetAttrs 后
/// 落盘为 data-vb-mark,undo 清除,导出产物含该属性(合法属性往返)。
#[test]
fn mark_cycles_persists_and_roundtrips() {
    let mut f = fx();
    // 循环语义
    assert_eq!(cycle_mark(None), Some(MARK_COLORS[0]));
    let last = *MARK_COLORS.last().unwrap();
    assert_eq!(cycle_mark(Some(last)), None, "末色再点 = 清除");

    let cmd = mark_cmd(&f.doc, &f.a, Some(MARK_COLORS[2].to_string()));
    let mut stack = UndoStack::new();
    stack.push(&mut f.doc, cmd).unwrap();
    let a = f.doc.nodes.get(f.doc.find_by_sid(&f.a).unwrap()).unwrap();
    assert_eq!(
        a.attrs.get(MARK_ATTR).map(String::as_str),
        Some(MARK_COLORS[2]),
        "标记落盘为 data-vb-mark 属性"
    );
    // 导出落盘:HTML 产物含 data-vb-mark(split_attrs 导入器原样保留)
    let files = vb_doc::export::render_project(&f.doc).files;
    assert!(
        files.iter().any(|f| f.1.contains("data-vb-mark")),
        "导出 HTML 应携带 data-vb-mark"
    );
    // undo 清除
    stack.undo(&mut f.doc).unwrap();
    let a = f.doc.nodes.get(f.doc.find_by_sid(&f.a).unwrap()).unwrap();
    assert!(!a.attrs.contains_key(MARK_ATTR), "undo 后标记消失");
}

// ── 02-4-4 右键菜单的命令路径 ──

/// 隐藏其他:除自身/祖先/后代外全部 hidden;undo 全量逆回。
#[test]
fn hide_others_compound_and_revert() {
    let mut f = fx();
    let others = others_of(&f.doc, &f.a);
    // 组 g 与其子 g1 是 a 的兄弟子树(不是祖先);画板节点已排除
    assert_eq!(others.len(), 4, "g、g1、b、c");
    assert!(!others.contains(&f.a), "自身除外");
    for s in [&f.g, &f.g1, &f.b, &f.c] {
        assert!(others.contains(s), "其他应含 {s}");
    }
    let cmds: Vec<Command> = others
        .iter()
        .map(|sid| Command::SetFlags {
            sid: sid.clone(),
            hidden: Some(true),
            locked: None,
            old: None,
        })
        .collect();
    let mut stack = UndoStack::new();
    stack.push(&mut f.doc, Command::Compound { cmds }).unwrap();
    for sid in [&f.g, &f.g1, &f.b, &f.c] {
        let n = f.doc.nodes.get(f.doc.find_by_sid(sid).unwrap()).unwrap();
        assert!(n.hidden, "{sid} 应被隐藏");
    }
    assert!(
        !f.doc
            .nodes
            .get(f.doc.find_by_sid(&f.a).unwrap())
            .unwrap()
            .hidden,
        "目标自身保持可见"
    );
    stack.undo(&mut f.doc).unwrap();
    for sid in [&f.g, &f.g1, &f.b, &f.c] {
        let n = f.doc.nodes.get(f.doc.find_by_sid(sid).unwrap()).unwrap();
        assert!(!n.hidden, "undo 后 {sid} 应恢复可见");
    }
}

/// 锁定其他:作用域同隐藏其他(g1 的祖先链除外 → a、b、c);undo 逆回。
#[test]
fn lock_others_compound_and_revert() {
    let mut f = fx();
    let others = others_of(&f.doc, &f.g1);
    assert_eq!(others.len(), 3);
    assert!(!others.contains(&f.g) && !others.contains(&f.g1));
    let cmds: Vec<Command> = others
        .iter()
        .map(|sid| Command::SetFlags {
            sid: sid.clone(),
            hidden: None,
            locked: Some(true),
            old: None,
        })
        .collect();
    let mut stack = UndoStack::new();
    stack.push(&mut f.doc, Command::Compound { cmds }).unwrap();
    assert!(
        f.doc
            .nodes
            .get(f.doc.find_by_sid(&f.a).unwrap())
            .unwrap()
            .locked
    );
    assert!(
        f.doc
            .nodes
            .get(f.doc.find_by_sid(&f.c).unwrap())
            .unwrap()
            .locked
    );
    stack.undo(&mut f.doc).unwrap();
    assert!(
        !f.doc
            .nodes
            .get(f.doc.find_by_sid(&f.a).unwrap())
            .unwrap()
            .locked
    );
}

/// 选择同类:所有 Box 节点(a、b、c、g1;g 是 Group 不算)。
#[test]
fn select_same_kind_scope() {
    let f = fx();
    let sids = same_kind_sids(&f.doc, &f.a);
    assert_eq!(sids.len(), 4);
    for s in [&f.a, &f.b, &f.c, &f.g1] {
        assert!(sids.contains(s), "同类应含 {s}");
    }
    let groups = same_kind_sids(&f.doc, &f.g);
    assert_eq!(groups, vec![f.g.clone()]);
}

/// 转换为编组:单成员 Group 包裹;成员进组、组顶替原槽位;
/// undo 完整逆回。画板被拒(菜单也不出现该项)。
#[test]
fn wrap_in_group_uses_existing_group_command() {
    let mut f = fx();
    let cmd = wrap_in_group_cmd(&mut f.doc, &f.a).unwrap();
    let gsid = match &cmd {
        Command::Group { group_sid, .. } => group_sid.clone(),
        _ => panic!("应为 Group 命令"),
    };
    let mut stack = UndoStack::new();
    stack.push(&mut f.doc, cmd).unwrap();
    assert_eq!(parent_of(&f.doc, &f.a).as_deref(), Some(gsid.as_str()));
    assert_eq!(parent_of(&f.doc, &gsid).as_deref(), Some(f.ab1.as_str()));
    assert_eq!(children_of(&f.doc, &gsid), vec![f.a.clone()]);
    stack.undo(&mut f.doc).unwrap();
    assert_eq!(parent_of(&f.doc, &f.a).as_deref(), Some(f.ab1.as_str()));
    assert!(f.doc.find_by_sid(&gsid).is_none());
    // 画板不支持
    assert!(wrap_in_group_cmd(&mut f.doc, &f.ab1).is_none());
}

// ── 02-4-7 搜索过滤 ──

/// 命中子节点的祖先行保持可见;无关节点被过滤;大小写不敏感。
#[test]
fn search_keeps_ancestors_of_matches() {
    let f = fx();
    let gid = f.doc.find_by_sid(&f.g).unwrap();
    let aid = f.doc.find_by_sid(&f.a).unwrap();
    // "g1" 命中:组(祖先)+ g1;盒 A 不命中
    assert!(matches_query(&f.doc, gid, "g1"));
    assert!(!matches_query(&f.doc, aid, "g1"));
    // 名字直接命中 + 大小写不敏感
    assert!(matches_query(&f.doc, gid, "组"));
    assert!(matches_query(&f.doc, aid, "盒a"));
    // 空词全过
    assert!(matches_query(&f.doc, aid, "  "));
}
