//! 05-8 符号 / 组件命令的命令构建与派发(台账 09-H,ADR-VB-L10)。
//!
//! 五个命令全部走命令层(可撤销;select_instances 为纯选区,不产生 undo):
//! - `object.symbol_create`(选中元素提升为主件 + 当前变为首个实例)
//! - `object.symbol_detach`(实例转普通元素,去标记)
//! - `object.symbol_reset_overrides`(还原为主件当前内容)
//! - `object.symbol_swap_main`(以实例内容替换主件定义,同步其余实例)
//! - `object.symbol_select_instances`(选中同主件全部实例)
//!
//! 事务构建逻辑沉在 `vb_doc::symbol`(纯函数,GUI / Agent / 测试共用);
//! 本文件只负责选区语义、状态提示与派发。跨文档复用(05-8-5 可选项)
//! 本轮未做,台账以 Partial 如实记录,不做假入口。

use vb_doc::symbol as sym;

use super::VellumApp;

/// 从选区收集涉及的实例根(实例内部节点归并到其根)。
fn instance_roots_in_selection(app: &VellumApp) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for sid in &app.selection {
        if let Some(id) = app.doc.find_by_sid(sid) {
            if let Some(root) = vb_doc::symbol::instance_root_of(&app.doc, id) {
                let rs = app.doc.nodes.get(root).unwrap().sid.as_str().to_string();
                if !roots.contains(&rs) {
                    roots.push(rs);
                }
            }
        }
    }
    roots
}

impl VellumApp {
    /// 「创建组件」:选区唯一且不是画板 / 主件 / 实例时有效。
    pub(crate) fn symbol_create(&mut self) {
        if self.selection.len() != 1 {
            self.toast_warn("创建组件:请先选中一个对象(多选不支持)");
            return;
        }
        let sid = self.selection[0].clone();
        let name = self
            .doc
            .find_by_sid(&sid)
            .map(|id| self.doc.nodes.get(id).unwrap().name.clone())
            .unwrap_or_default();
        match sym::symbol_create_commands(&mut self.doc, &sid, &name) {
            Ok((cmd, inst_sid)) => {
                self.exec(cmd);
                self.selection = vec![inst_sid];
                self.say(format!(
                    "已创建组件「{name}」(当前位置成为首个实例;编辑主件将同步全部实例)"
                ));
            }
            Err(e) => self.toast_error(format!("创建组件:{e}")),
        }
    }

    /// 「分离实例」:选区内的全部实例根转普通元素(内容原样保留)。
    pub(crate) fn symbol_detach(&mut self) {
        let roots = instance_roots_in_selection(self);
        if roots.is_empty() {
            self.toast_warn("分离实例:选中对象里没有组件实例");
            return;
        }
        for r in &roots {
            match sym::symbol_detach_commands(&self.doc, r) {
                Ok(cmd) => self.exec(cmd),
                Err(e) => self.toast_error(format!("分离实例:{e}")),
            }
        }
        self.selection = roots;
        self.say("已分离为普通元素(不再随主件同步)");
    }

    /// 「重置覆盖」:选区内全部实例还原为主件当前内容。
    pub(crate) fn symbol_reset_overrides(&mut self) {
        let roots = instance_roots_in_selection(self);
        if roots.is_empty() {
            self.toast_warn("重置覆盖:选中对象里没有组件实例");
            return;
        }
        let mut n = 0usize;
        for r in &roots {
            match sym::symbol_reset_overrides_commands(&mut self.doc, r) {
                Ok(cmd) => {
                    self.exec(cmd);
                    n += 1;
                }
                Err(e) => self.toast_error(format!("重置覆盖:{e}")),
            }
        }
        self.selection = roots;
        self.say(format!("已还原 {n} 个实例为主件当前内容(覆盖已清除)"));
    }

    /// 「替换主件定义」:选中一个实例,其当前内容成为新定义并同步其余实例。
    pub(crate) fn symbol_swap_main(&mut self) {
        if self.selection.len() != 1 {
            self.toast_warn("替换主件定义:请选中一个组件实例");
            return;
        }
        let sid = self.selection[0].clone();
        match sym::symbol_swap_main_commands(&mut self.doc, &sid) {
            Ok(cmd) => {
                self.exec(cmd);
                self.say("已用该实例内容替换主件定义,并同步其余实例(各自的覆盖仍保留)");
            }
            Err(e) => self.toast_error(format!("替换主件定义:{e}")),
        }
    }

    /// 「选择所有实例」:种子 = 实例 / 实例内部节点 / 主件定义区节点。
    pub(crate) fn symbol_select_instances(&mut self) {
        let Some(seed) = self.selection.last().cloned() else {
            self.toast_warn("选择所有实例:先选中一个实例或主件");
            return;
        };
        let hits = sym::select_instances_of(&self.doc, &seed);
        if hits.is_empty() {
            self.toast_warn("选择所有实例:选中对象不属于任何组件");
        } else {
            let n = hits.len();
            self.selection = hits;
            self.say(format!("已选中同主件全部实例({n} 个)"));
        }
    }
}

// ─────────────────────────── 05-8 应用级测试 ───────────────────────────

#[cfg(test)]
mod symbol_tests {
    use super::instance_roots_in_selection;
    use crate::app::assemble::tests::app_fresh;
    use crate::app::external::re_sid_tree;
    use vb_doc::model::{Geom, Node, NodeKind, NodeTree, TextMode};
    use vb_doc::symbol::{ATTR_OVERRIDES, ATTR_SYMBOL, ATTR_SYMBOL_REF};

    use super::super::VellumApp;

    /// 造一张"卡片"(容器 + 标题文本),返回根 sid。
    fn add_card(app: &mut VellumApp, name: &str, x: f64, title: &str) -> String {
        let root_sid = app.doc.alloc_sid();
        let mut root = Node::new(NodeKind::Box, name, root_sid.clone());
        root.geom = Geom {
            x,
            y: 40.0,
            w: 220.0,
            h: 90.0,
        };
        root.style.push(vb_css::Decl {
            prop: "background-color".into(),
            value: "#ffffff".into(), // vb-token-ok: 文档内容色
            important: false,
        });
        let root_id = app.doc.nodes.insert(root);
        let ab = app.doc.artboards[0];
        app.doc.nodes.get_mut(root_id).unwrap().parent = Some(ab);
        app.doc.nodes.get_mut(ab).unwrap().children.push(root_id);

        let t_sid = app.doc.alloc_sid();
        let t = Node::new(
            NodeKind::Text {
                text: title.to_string(),
                mode: TextMode::Point,
                segments: Vec::new(),
            },
            "标题",
            t_sid,
        );
        let t_id = app.doc.nodes.insert(t);
        app.doc.nodes.get_mut(t_id).unwrap().parent = Some(root_id);
        app.doc.nodes.get_mut(root_id).unwrap().children.push(t_id);
        root_sid.as_str().to_string()
    }

    /// 直接对实例做"复制"(带标记的全新 sid 副本)造更多实例,返回新根 sid。
    fn duplicate_instance(app: &mut VellumApp, inst_sid: &str) -> String {
        let id = app.doc.find_by_sid(inst_sid).unwrap();
        let parent_sid = {
            let pid = app.doc.nodes.get(id).unwrap().parent.unwrap();
            app.doc.nodes.get(pid).unwrap().sid.as_str().to_string()
        };
        let mut t = NodeTree::from_document(&app.doc, id).unwrap();
        re_sid_tree(&mut t, &mut app.doc);
        app.exec(vb_doc::commands::Command::Insert {
            parent_sid,
            index: usize::MAX,
            tree: t,
        });
        let id2 = app.doc.find_by_sid(inst_sid).unwrap();
        let pid2 = app.doc.nodes.get(id2).unwrap().parent.unwrap();
        let new_root = app
            .doc
            .nodes
            .get(pid2)
            .unwrap()
            .children
            .last()
            .copied()
            .unwrap();
        app.doc
            .nodes
            .get(new_root)
            .unwrap()
            .sid
            .as_str()
            .to_string()
    }

    /// 主件原型根 sid(defs_root → 容器 → 原型)。
    fn proto_root(app: &VellumApp) -> String {
        let r = app.doc.nodes.get(app.doc.defs_root).unwrap();
        let c = r.children[0];
        let p = app.doc.nodes.get(c).unwrap().children[0];
        app.doc.nodes.get(p).unwrap().sid.as_str().to_string()
    }

    /// 实例根的第 0 个子节点 sid。
    fn first_child(app: &VellumApp, inst_sid: &str) -> String {
        let id = app.doc.find_by_sid(inst_sid).unwrap();
        let tid = app.doc.nodes.get(id).unwrap().children[0];
        app.doc.nodes.get(tid).unwrap().sid.as_str().to_string()
    }

    /// 实例根第 0 个子节点的文本。
    fn child_text(app: &VellumApp, inst_sid: &str) -> String {
        let id = app.doc.find_by_sid(inst_sid).unwrap();
        let tid = app.doc.nodes.get(id).unwrap().children[0];
        match &app.doc.nodes.get(tid).unwrap().kind {
            NodeKind::Text { text, .. } => text.clone(),
            k => panic!("实例子节点应为文本,实际 {k:?}"),
        }
    }

    /// 门禁①:3 实例改主件标题同步一致 + 门禁②:覆盖保留 + 撤销/重做。
    #[test]
    fn main_edit_syncs_all_instances_and_keeps_overrides() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        let card = add_card(&mut app, "卡片", 40.0, "标题 A");
        app.selection = vec![card];
        app.run_command("object.symbol_create", false, false);
        assert!(app.status.contains("已创建组件"), "创建失败:{}", app.status);
        let inst0 = app.selection[0].clone();
        let inst1 = duplicate_instance(&mut app, &inst0);
        let inst2 = duplicate_instance(&mut app, &inst0);
        let insts = [inst0, inst1, inst2];

        // 三实例标记一致,且与主件容器 ref 相同
        for s in &insts {
            let id = app.doc.find_by_sid(s).unwrap();
            let n = app.doc.nodes.get(id).unwrap();
            assert!(n.attrs.contains_key(ATTR_SYMBOL));
            assert!(n.attrs.contains_key(ATTR_SYMBOL_REF));
        }

        // 改主件标题(push 收口自动同步)
        let title_main = {
            let pr = app.doc.find_by_sid(&proto_root(&app)).unwrap();
            app.doc.nodes.get(pr).unwrap().children[0]
        };
        let title_main_sid = app
            .doc
            .nodes
            .get(title_main)
            .unwrap()
            .sid
            .as_str()
            .to_string();
        app.exec(vb_doc::commands::Command::SetText {
            sid: title_main_sid.clone(),
            new: "新标题".into(),
            old: None,
        });
        for s in &insts {
            assert_eq!(child_text(&app, s), "新标题", "实例 {s} 未同步");
        }

        // 门禁②:实例 0 改自己的文本(登记覆盖),主件再改
        let t0 = first_child(&app, &insts[0]);
        app.exec(vb_doc::commands::Command::SetText {
            sid: t0,
            new: "我的特例".into(),
            old: None,
        });
        {
            let id = app.doc.find_by_sid(&insts[0]).unwrap();
            let ov = app
                .doc
                .nodes
                .get(id)
                .unwrap()
                .attrs
                .get("data-vb-symbol-overrides")
                .cloned()
                .unwrap_or_default();
            assert!(ov.contains("0:text"), "覆盖键未登记:{ov}");
        }
        app.exec(vb_doc::commands::Command::SetText {
            sid: title_main_sid,
            new: "标题 B".into(),
            old: None,
        });
        assert_eq!(child_text(&app, &insts[0]), "我的特例", "覆盖被同步冲掉");
        assert_eq!(child_text(&app, &insts[1]), "标题 B", "未同步");
        assert_eq!(child_text(&app, &insts[2]), "标题 B", "未同步");

        // 撤销整条(编辑 + 同步)→ 回到上一态;重做 → 再次一致
        app.run_command("edit.undo", false, false);
        assert_eq!(
            child_text(&app, &insts[1]),
            "新标题",
            "撤销应回到上次同步结果"
        );
        app.run_command("edit.redo", false, false);
        assert_eq!(child_text(&app, &insts[1]), "标题 B", "重做后重新同步");
    }

    /// 门禁⑤:Detach 后无残留标记;undo 恢复标记;实例子树编辑登记 geom 覆盖。
    #[test]
    fn detach_removes_markers_and_undo_restores() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        let card = add_card(&mut app, "卡片", 0.0, "T");
        app.selection = vec![card];
        app.run_command("object.symbol_create", false, false);
        let inst = app.selection[0].clone();

        app.run_command("object.symbol_detach", false, false);
        let id = app.doc.find_by_sid(&inst).unwrap();
        let n = app.doc.nodes.get(id).unwrap();
        assert!(
            !n.attrs.contains_key(ATTR_SYMBOL),
            "detach 后残留主件名标记"
        );
        assert!(
            !n.attrs.contains_key(ATTR_SYMBOL_REF),
            "detach 后残留引用标记"
        );
        assert!(!n.attrs.contains_key(ATTR_OVERRIDES));
        assert!(
            instance_roots_in_selection(&app).is_empty(),
            "分离后不再是实例"
        );

        app.run_command("edit.undo", false, false);
        let id = app.doc.find_by_sid(&inst).unwrap();
        assert!(app
            .doc
            .nodes
            .get(id)
            .unwrap()
            .attrs
            .contains_key(ATTR_SYMBOL));
    }

    /// 重置覆盖 / 替换主件定义 / 选择所有实例的命令路径。
    #[test]
    fn reset_swap_select_commands() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        let card = add_card(&mut app, "卡片", 0.0, "T");
        app.selection = vec![card];
        app.run_command("object.symbol_create", false, false);
        let inst0 = app.selection[0].clone();
        let inst1 = duplicate_instance(&mut app, &inst0);

        // 实例 0 覆盖标题颜色(style 覆盖登记)
        let t0 = first_child(&app, &inst0);
        let mut style = {
            let id = app.doc.find_by_sid(&t0).unwrap();
            app.doc.nodes.get(id).unwrap().style.clone()
        };
        style.push(vb_css::Decl {
            prop: "color".into(),
            value: "#ff0000".into(), // vb-token-ok: 文档内容色
            important: false,
        });
        app.exec(vb_doc::commands::Command::SetStyle {
            sid: t0,
            new: style,
            old: None,
        });
        {
            let id = app.doc.find_by_sid(&inst0).unwrap();
            let ov = app
                .doc
                .nodes
                .get(id)
                .unwrap()
                .attrs
                .get(ATTR_OVERRIDES)
                .cloned()
                .unwrap_or_default();
            assert!(ov.contains("0:style:color"), "覆盖键未登记:{ov}");
        }

        // 主件标题改值(实例 1 同步;实例 0 文本同值但颜色覆盖仍在)
        let title_main_sid = {
            let pr = app.doc.find_by_sid(&proto_root(&app)).unwrap();
            let t = app.doc.nodes.get(pr).unwrap().children[0];
            app.doc.nodes.get(t).unwrap().sid.as_str().to_string()
        };
        app.exec(vb_doc::commands::Command::SetText {
            sid: title_main_sid,
            new: "主件新值".into(),
            old: None,
        });
        assert_eq!(child_text(&app, &inst1), "主件新值");

        // 重置覆盖:实例 0 回主件内容,覆盖列表清空
        app.selection = vec![inst0.clone()];
        app.run_command("object.symbol_reset_overrides", false, false);
        {
            let id = app.doc.find_by_sid(&inst0).unwrap();
            let n = app.doc.nodes.get(id).unwrap();
            assert!(!n.attrs.contains_key(ATTR_OVERRIDES), "覆盖列表未清空");
            let tid = n.children[0];
            let color = app.doc.nodes.get(tid).unwrap().style_get("color");
            assert!(color.is_none(), "样式覆盖未还原");
        }

        // 替换主件定义(以实例 0 当前内容为定义)
        app.selection = vec![inst0.clone()];
        app.run_command("object.symbol_swap_main", false, false);
        assert!(app.status.contains("替换主件定义"), "{}", app.status);
        {
            let id = app.doc.find_by_sid(&inst0).unwrap();
            assert!(
                !app.doc
                    .nodes
                    .get(id)
                    .unwrap()
                    .attrs
                    .contains_key(ATTR_OVERRIDES),
                "成为定义源后覆盖应被吸收清空"
            );
        }
        // 实例 1 仍是实例且引用同一容器
        {
            let id1 = app.doc.find_by_sid(&inst1).unwrap();
            assert!(app
                .doc
                .nodes
                .get(id1)
                .unwrap()
                .attrs
                .contains_key(ATTR_SYMBOL));
        }

        // 选择所有实例:以实例 1 为种子
        app.selection = vec![inst1];
        app.run_command("object.symbol_select_instances", false, false);
        assert_eq!(app.selection.len(), 2, "应选中全部实例");

        // 错误路径:非实例调分离给可读提示,不入撤销栈
        app.selection = vec![];
        app.run_command("object.symbol_detach", false, false);
        assert!(app.status.contains("没有组件实例"));
    }

    /// 实例内部节点的连续拖拽(scrubby 等价:逐帧 SetGeom)经覆盖登记
    /// 包装后**仍合并为一条 undo**(merge_target 的 sy 形状签名)。
    #[test]
    fn instance_inner_drag_merges_into_one_undo() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        let card = add_card(&mut app, "卡片", 0.0, "T");
        app.selection = vec![card];
        app.run_command("object.symbol_create", false, false);
        let inst = app.selection[0].clone();
        let t0 = first_child(&app, &inst);
        let depth_before = app.undo.undo_len();

        // 模拟拖拽:三帧 SetGeom(同一目标,位置渐变)
        for dx in [10.0f64, 20.0, 30.0] {
            let id = app.doc.find_by_sid(&t0).unwrap();
            let mut g = app.doc.nodes.get(id).unwrap().geom;
            g.x = dx;
            app.exec(vb_doc::commands::Command::SetGeom {
                sid: t0.clone(),
                new: g,
                old: None,
                old_declared: None,
            });
        }
        assert_eq!(
            app.undo.undo_len(),
            depth_before + 1,
            "三帧拖拽应合并为一条 undo(实际 {} 条)",
            app.undo.undo_len() - depth_before
        );
        // 覆盖键已登记 geom
        let iid = app.doc.find_by_sid(&inst).unwrap();
        let ov = app
            .doc
            .nodes
            .get(iid)
            .unwrap()
            .attrs
            .get(ATTR_OVERRIDES)
            .cloned()
            .unwrap_or_default();
        assert!(ov.contains("0:geom"), "geom 覆盖键未登记:{ov}");
    }
}
