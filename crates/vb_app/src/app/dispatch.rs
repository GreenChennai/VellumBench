//! 命令派发单一入口:输入上下文投影、快捷键处理、`run_command` 分派。
//!
//! 06-1 自 `app.rs` 拆出(纯搬移,零行为变化)。原单条巨型 `match`
//! 按**连续段**机械切为三份,顺序保持(字面量臂互斥,行为等价):
//! 本文件放「文件/编辑/对象」段,「视图/面板/工具」段在
//! `dispatch_view`,「剪贴板/布尔/画布/应用」段在 `dispatch_canvas`;
//! 未匹配命令仍走 `run_menu_command` 兜底(原 `_ =>` 分支)。

use egui::Key;
use vb_doc::commands::Command;
use vb_doc::model::Document;
use vb_doc::undo::UndoStack;
use vb_ui::cursor as vbcursor;

use crate::shortcuts::{self, InputContext};

use super::{Drag, Tool, VellumApp};

impl VellumApp {
    pub(super) fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // 输入上下文栈(02 篇 §一):只有栈顶上下文消费按键。
        // 文本编辑 / 输入框聚焦 → TextEdit,工具键与 Delete 一律不生效(B1 修复)。
        let top = self.input_context(ctx);
        self.space_down = top != InputContext::TextEdit && ctx.input(|i| i.key_down(Key::Space));
        if self.space_down && ctx.input(|i| i.key_pressed(Key::Space)) {
            ctx.set_cursor_icon(vbcursor::PAN);
        }

        // 本帧按下的键(含修饰键)。遍历**全部**事件(旧实现只看第一个,方向键连按会丢)。
        let pressed: Vec<(Key, bool, bool, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((
                        *key,
                        modifiers.ctrl || modifiers.command,
                        modifiers.shift,
                        modifiers.alt,
                    )),
                    _ => None,
                })
                .collect()
        });

        for (key, ctrl, shift, alt) in pressed {
            // 05-4-A2:有效键位集解析 —— 用户键位方案(keymap.json)优先,
            // 未被覆盖的静态绑定兜底;返回 (命令, 生效上下文)。
            let Some((id, ctx)) = crate::keymap::resolve_effective(
                &self.keymap,
                &self.keymap_live,
                key,
                ctrl,
                shift,
                alt,
            ) else {
                continue;
            };
            // 上下文守卫:栈顶不允许 → 不消费(交给输入框 / 面板自行处理)
            if !ctx.contains(top) {
                continue;
            }
            self.run_command(&id, shift, alt);
        }
    }

    /// 当前输入上下文(栈顶)。02 篇 §一 / 14 篇 §4.1。
    /// 判定单一真相在 `shortcuts::top_input_context` 纯函数
    /// (B1 派发层回归测试直接打它;此处只做 GUI 状态投影)。
    ///
    /// 04-1(P0-⑤ 根因):旧实现喂的是 `egui_wants_keyboard_input()` ——
    /// egui 0.35 里它等于「**任何**控件持有焦点」(不只是文本框),而
    /// NumField 回车提交后会**保留焦点**(`vb_ui::components` 02-6-2)。
    /// 结果:点过一次数值框,整个快捷键层被永久顶到 TextEdit 态,
    /// Ctrl+K 命令面板 / Ctrl+0 适合窗口(CTX_NO_TEXT)全部吞键。
    /// 改喂 `text_edit_focused()`:只有真正的文本编辑框聚焦才进 TextEdit 态。
    fn input_context(&self, ctx: &egui::Context) -> InputContext {
        shortcuts::top_input_context(
            self.editing_text.is_some(),
            self.palette_open,
            ctx.text_edit_focused(),
            !matches!(self.drag, Drag::None),
        )
    }

    /// 命令派发单一入口(快捷键 / 菜单 / 未来的命令面板共用)。
    ///
    /// `id` 必须出现在 `shortcuts::IMPLEMENTED_IDS` 中(有测试把关)。
    /// `_alt` 为修饰键上下文预留(当前无命令依赖 Alt 分支:Alt 语义由
    /// 画布拖动层直接处理,见 02 篇 §二 四大灵魂手势)。
    pub(super) fn run_command(&mut self, id: &str, shift: bool, _alt: bool) {
        debug_assert!(
            shortcuts::is_implemented(id),
            "命令 {id} 未在 shortcuts::IMPLEMENTED_IDS 中声明"
        );
        // NumField 提交会话兜底收口:任何快捷键/菜单命令都意味着
        // 用户离开了数值框编辑(键盘输入在 TextEdit 上下文不会派发到这),
        // 会话不该跨过一次显式命令继续合并。
        if self.num_commit_open {
            self.num_commit_open = false;
            self.undo.end_session();
        }
        // 拖拽进行中收敛可派发集合:删除正在拖的对象会让后续每帧 SetGeom
        // 打到死 sid 上刷屏报错;切工具会让 drag 状态与新工具错位
        if matches!(
            self.drag,
            Drag::MoveObj { .. }
                | Drag::Resize { .. }
                | Drag::Rotate { .. }
                | Drag::Marquee { .. }
                | Drag::Create { .. }
                | Drag::ZoomRegion { .. }
                // 05-2:X-4 工具族 / 铅笔 / 度量的拖拽态同样收敛可派发集合
                | Drag::ToolRotate { .. }
                | Drag::ToolMirror { .. }
                | Drag::ToolScale { .. }
                | Drag::FreeTransform { .. }
                | Drag::PencilStroke { .. }
                | Drag::Measure { .. }
        ) && !(id == "canvas.cancel"
            || id.starts_with("app.")
            || id.starts_with("file.")
            || id.starts_with("view."))
        {
            self.toast_warn("拖拽进行中:先松手或 Esc 取消");
            return;
        }
        // 06-1:按连续段分派(顺序 = 原臂序;行为等价见文件头说明)
        if self.dispatch_file_edit_cmd(id, shift, _alt) {
            return;
        }
        if self.dispatch_view_cmd(id, shift, _alt) {
            return;
        }
        if self.dispatch_canvas_cmd(id, shift, _alt) {
            return;
        }
        // 原 `_ =>` 分支:AI 规范 9 项菜单新增命令(实现见 `menu_commands.rs`)
        if !self.run_menu_command(id) {
            debug_assert!(false, "命令 {id} 未在 run_command 中实现");
            log::warn!("未实现的命令:{id}");
            self.toast_error(format!("命令未实现:{id}"));
        }
    }

    /// 「文件/编辑/对象」命令段(原臂序连续段,顺序保持)。
    fn dispatch_file_edit_cmd(&mut self, id: &str, _shift: bool, _alt: bool) -> bool {
        let mut matched = true;
        match id {
            // ── 文件 ──
            // 阶段 2(02-4-3):新建走对话框 → 新窗口;无外壳的独立构造保留旧行为
            "file.new" => {
                if self.shell_tx.is_some() {
                    self.new_dialog = Some(crate::new_project::NewProjectDialog::new());
                } else {
                    self.doc = Document::new_default();
                    self.undo = UndoStack::new();
                    self.selection.clear();
                    self.project_dir = None;
                    // 旧 doc 的 NodeId 全部失效,进行中的状态一并作废
                    self.isolate_stack.clear();
                    self.pen_points.clear();
                    self.ds_vertex = None;
                    self.editing_text = None;
                    self.drag = Drag::None;
                    self.layer_drag = None;
                    self.editing_layer = None;
                    // 04-5-2:新建项目后强制回选择工具(避免残留矩形工具态误建形状);
                    // 04-5-1:新建后自动适合窗口(画布矩形就绪后的第一帧落)
                    self.set_tool(Tool::Select);
                    self.fit_pending = true;
                    self.status = "新建文档(1440×900)".into();
                }
            }
            // 阶段 2(02-5-5):打开项目经外壳 → 新窗口 / 聚焦已有窗口
            "file.open" => {
                if let Some(tx) = &self.shell_tx {
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title("打开项目目录(含 index.html)")
                        .pick_folder()
                    {
                        let _ = tx.send(crate::shell::ShellRequest::OpenProject(dir));
                    }
                } else {
                    self.open_project_inplace();
                }
            }
            "file.save" => {
                self.save_project();
            }
            // 阶段 2(02-5):关闭当前窗口(有未保存改动由外壳弹确认)
            "file.close" => {
                if let Some(tx) = &self.shell_tx {
                    let _ = tx.send(crate::shell::ShellRequest::CloseWindow(self.viewport_id));
                    self.say("关闭窗口…");
                } else {
                    self.toast_warn("当前不是多窗口模式,无法关闭窗口");
                }
            }
            // 阶段 2(02-1):回到主页(--project 启动时主页是子视口,可从这里唤出)
            "file.home" => {
                if let Some(tx) = &self.shell_tx {
                    let _ = tx.send(crate::shell::ShellRequest::ShowHome);
                    self.say("已打开主页");
                } else {
                    self.toast_warn("当前不是多窗口模式,无主页");
                }
            }
            "file.export_dialog" => self.show_export = true,
            "file.export_repeat" => self.export_current_artboard_png(),
            // 09-D(05-2):文件 → 置入图像(选文件 → assets/ → 选中设 src 或新建 img)
            "file.place_image" => self.place_image_via_dialog(),
            // ── 05-4-A2 对话框族 ──
            // X-7:打印 = 当前画板 → Kiln 临时 PDF → 系统默认程序打开
            "file.print" => self.print_current_artboard(),
            // ── 05-5 断点与伪类(09-F)──
            // 断点循环切换(状态栏切换器同一实现;可用断点为空给诚实提示)
            "view.breakpoint_cycle" => self.cycle_breakpoint(),
            // 属性面板样式状态:正常 ↔ hover(落 selector:hover 规则)
            "style.state_toggle" => self.toggle_style_state(),
            // 09-M:文档设置(项目名/画板预设/输出模式/网格与参考线)
            "file.doc_settings" => {
                self.doc_settings_open = true;
                self.say("文档设置:项目名 / 输出模式 / 网格与参考线(应用后生效)");
            }
            // 09-N:外部冲突三方对比(仅未采用印记存在时打开;否则提示)
            "file.resolve_conflict" => {
                if self.has_pending_conflict() {
                    self.conflict_open = true;
                    self.say("外部冲突对比:磁盘 / 内存 / 自动快照 三方差异与处置动作");
                } else {
                    self.say("当前没有待处理的外部冲突(磁盘与内存一致,或本会话无外部改动)");
                }
            }
            // ── 编辑 ──
            "edit.undo" | "edit.redo" => {
                let redo = id == "edit.redo";
                // 恢复选区策略(AI 行为):撤销删除→重选被删对象;撤销编组→重选成员;
                // 撤销解组→重选编组;重做反向(05-3 增:多结果事务撤销重选 N 源 /
                // 重做选 M 结果)。其余情形清掉悬空 sid。
                let top = if redo {
                    self.undo.top_redo().cloned()
                } else {
                    self.undo.top().cloned()
                };
                // 是否真的发生了撤销/重做:undo()/redo() 只在栈中**还有余量**
                // 时返回 Some(label),动到最后一条也返回 None —— 以深度变化为
                // 准,否则「撤销第一步」的选区恢复会被跳过(05-3 修的隐性缺口)。
                let depth_before = if redo {
                    self.undo.redo_len()
                } else {
                    self.undo.undo_len()
                };
                let label = if redo {
                    self.undo.redo(&mut self.doc).ok().flatten()
                } else {
                    self.undo.undo(&mut self.doc).ok().flatten()
                };
                let depth_after = if redo {
                    self.undo.redo_len()
                } else {
                    self.undo.undo_len()
                };
                let acted = depth_before > depth_after;
                if acted {
                    self.isolate_stack
                        .retain(|id| self.doc.nodes.get(*id).is_some());
                    self.selection = match (&top, redo) {
                        (Some(Command::Delete { target_sid, .. }), false) => {
                            vec![target_sid.clone()]
                        }
                        (Some(Command::Group { member_sids, .. }), false) => member_sids.clone(),
                        (Some(Command::Ungroup { group_sid, .. }), false) => {
                            vec![group_sid.clone()]
                        }
                        (Some(Command::Insert { tree, .. }), true) => {
                            vec![tree.root_sid().to_string()]
                        }
                        // 05-3 多结果事务:撤销重选 N 个源;重做选中 M 个结果
                        (Some(Command::MultiResult { src_sids, .. }), false) => src_sids.clone(),
                        (Some(Command::MultiResult { results, .. }), true) => {
                            results.iter().map(|t| t.root_sid().to_string()).collect()
                        }
                        (Some(Command::Delete { target_sid, .. }), true) => self
                            .selection
                            .iter()
                            .filter(|s| *s != target_sid)
                            .cloned()
                            .collect(),
                        (Some(Command::Ungroup { captured, .. }), true) => captured
                            .as_ref()
                            .map(|(_, tree)| {
                                tree.children
                                    .iter()
                                    .map(|c| c.node.sid.as_str().to_string())
                                    .collect()
                            })
                            .unwrap_or_default(),
                        _ => std::mem::take(&mut self.selection),
                    };
                    self.selection.retain(|s| self.doc.find_by_sid(s).is_some());
                }
                self.status = match label {
                    Some(l) => format!("{}:{l}", if redo { "重做" } else { "撤销" }),
                    None => "没有可撤销/重做的操作".into(),
                };
            }
            "edit.select_all" => {
                // 当前画板(含选区推断),不是硬编码第一块
                if let Some(ab) = self.active_artboard() {
                    let kids = self.doc.nodes.get(ab).unwrap().children.clone();
                    self.selection = kids
                        .into_iter()
                        .filter(|id| {
                            self.doc
                                .nodes
                                .get(*id)
                                .map(|n| !n.locked && !n.hidden)
                                .unwrap_or(false)
                        })
                        .map(|id| self.doc.nodes.get(id).unwrap().sid.as_str().to_string())
                        .collect();
                    self.status = format!("已全选 {} 个对象", self.selection.len());
                }
            }
            // 05-2(X-5):铅笔保真度档位循环(持久化到 workspace.json)
            "edit.pencil_fidelity" => self.step_pencil_fidelity(),
            // ── 05-4-A2 对话框族(编辑段)──
            // 09-L:首选项九分类
            "edit.preferences" => {
                self.prefs_open = true;
                self.say("首选项:常规 / 文字 / 单位与标尺 / 参考线与网格 / 智能参考线 / 画板 / 性能 / 外观 / 数据");
            }
            // 09-L:键位方案编辑器
            "edit.keyboard_shortcuts" => {
                self.keymap_open = true;
                self.say("键盘快捷键:命令列表 + 冲突检测 + 录制新键(方案存 keymap.json)");
            }
            // ── 对象 ──
            "object.group" => self.group_selection(),
            "object.ungroup" => self.ungroup_selection(),
            "object.transform_again" => self.transform_again(),
            "object.bring_forward" => self.reorder_selection(1),
            "object.bring_to_front" => self.reorder_selection(10001),
            "object.send_backward" => self.reorder_selection(-1),
            "object.send_to_back" => self.reorder_selection(-10001),
            "object.delete" => self.delete_selection(),
            _ => matched = false,
        }
        matched
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;

    /// 04-5-4:Esc(canvas.cancel)从任意工具回选择。
    #[test]
    fn esc_returns_to_select_from_any_tool() {
        let _env = crate::ENV_LOCK.lock();
        for tool in [
            Tool::Rect,
            Tool::Ellipse,
            Tool::Pen,
            Tool::Artboard,
            Tool::Zoom,
            Tool::Hand,
        ] {
            let mut app = app_fresh(None);
            app.run_command(
                match tool {
                    Tool::Rect => "tool.rect",
                    Tool::Ellipse => "tool.ellipse",
                    Tool::Pen => "tool.pen",
                    Tool::Artboard => "tool.artboard",
                    Tool::Zoom => "tool.zoom",
                    _ => "tool.hand",
                },
                false,
                false,
            );
            assert_eq!(app.tool, tool, "先切入目标工具");
            app.run_command("canvas.cancel", false, false);
            assert_eq!(app.tool, Tool::Select, "Esc 必须从 {tool:?} 回选择");
        }
    }

    /// 07-K:资产面板开关命令(真值布尔 + 次级坞聚焦「资产」组)。
    #[test]
    fn assets_panel_toggle_focuses_assets_group() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        assert!(!app.assets_open, "默认关");
        app.run_command("view.toggle_assets_panel", false, false);
        assert!(app.assets_open);
        assert_eq!(
            app.sec.active_group,
            crate::app::panel_dock::SecPanel::Assets.group().index(),
            "打开必须聚焦到「资产」组(可见反馈)"
        );
        app.run_command("view.toggle_assets_panel", false, false);
        assert!(!app.assets_open);
    }
}
