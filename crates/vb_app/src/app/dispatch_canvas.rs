//! 命令派发 ·「对齐/停靠/文字模式/剪贴板/布尔/画布/应用」段。
//!
//! 06-1 自 `app.rs` 的 `run_command` 巨型 match 按连续段拆出
//! (纯搬移,零行为变化;切分顺序 = 原臂序,见 `dispatch` 文件头)。

use egui::Key;
use vb_doc::commands::Command;

use super::{Drag, Tool, VellumApp};

impl VellumApp {
    /// 「剪贴板/布尔/画布/应用」命令段(原臂序连续段,顺序保持)。
    pub(super) fn dispatch_canvas_cmd(&mut self, id: &str, shift: bool, _alt: bool) -> bool {
        let mut matched = true;
        match id {
            "tool.eyedropper" => self.set_tool(Tool::Eyedropper),
            "tool.artboard" => self.set_tool(Tool::Artboard),
            "tool.gradient" => self.set_tool(Tool::Gradient),
            "tool.scissors" => self.set_tool(Tool::Scissors),
            "tool.group_select" => self.set_tool(Tool::GroupSelect),
            // ── 阶段 5(05-2):X-4 变换工具族 / X-5 曲线 / 09-C 切片 / 09-E 度量 ──
            "tool.rotate" => self.set_tool(Tool::Rotate),
            "tool.mirror" => self.set_tool(Tool::Mirror),
            "tool.scale" => self.set_tool(Tool::Scale),
            "tool.free_transform" => self.set_tool(Tool::FreeTransform),
            "tool.pencil" => self.set_tool(Tool::Pencil),
            "tool.curvature" => self.set_tool(Tool::Curvature),
            "tool.slice" => self.set_tool(Tool::Slice),
            "tool.measure" => self.set_tool(Tool::Measure),
            // 09-C:对象 → 切片 → 建立(从选区包围盒)
            "object.slice_from_selection" => self.slice_from_selection(),
            // ── 09-B 剪切蒙版(Mod+7 / Mod+Alt+7)──
            "object.clip_mask" => self.apply_clip_mask(),
            "object.release_clip_mask" => self.apply_release_clip_mask(),
            // ── 09-D 图像替换(右键/菜单共用;置入在 dispatch.rs 的 file 段)──
            "object.replace_image" => self.replace_image_via_dialog(),
            // ── 05-8 符号 / 组件(09-H;ADR-VB-L10)──
            "object.symbol_create" => self.symbol_create(),
            "object.symbol_detach" => self.symbol_detach(),
            "object.symbol_reset_overrides" => self.symbol_reset_overrides(),
            "object.symbol_swap_main" => self.symbol_swap_main(),
            "object.symbol_select_instances" => self.symbol_select_instances(),
            // ── P3.8 分布(≥3 选中) ──
            "object.distribute_h" => self.distribute_selection(true),
            "object.distribute_v" => self.distribute_selection(false),
            // ── P3.3 剪贴板 ──
            "edit.copy" => self.clipboard_copy(),
            "edit.cut" => {
                self.clipboard_copy();
                self.delete_selection();
            }
            "edit.paste" => self.clipboard_paste(false),
            "edit.paste_in_place" => self.clipboard_paste(true),
            // ── P3.8 对齐 ──
            // ── C1 路径查找器(四基本运算;两两矢量路径) ──
            "path.union" => self.path_boolean(vb_tools::boolean::BooleanOp::Union),
            "path.subtract" => self.path_boolean(vb_tools::boolean::BooleanOp::Subtract),
            "path.intersect" => self.path_boolean(vb_tools::boolean::BooleanOp::Intersect),
            "path.xor" => self.path_boolean(vb_tools::boolean::BooleanOp::Xor),
            // ── 05-3 / X-1:多结果三运算(分割/修边/轮廓,MultiResult 事务)──
            "path.divide" => self.path_boolean_multi(vb_tools::pathfinder::MultiOp::Divide),
            "path.trim" => self.path_boolean_multi(vb_tools::pathfinder::MultiOp::Trim),
            "path.outline" => self.path_boolean_multi(vb_tools::pathfinder::MultiOp::Outline),
            "align.left" => self.align_selection("left"),
            "align.hcenter" => self.align_selection("hcenter"),
            "align.right" => self.align_selection("right"),
            "align.top" => self.align_selection("top"),
            "align.vcenter" => self.align_selection("vcenter"),
            "align.bottom" => self.align_selection("bottom"),
            // ── P3.9 锁定 / 隐藏 ──
            "object.lock" => {
                // 多选合成一条 Compound(N 条独立 undo → 一条,B4)
                let cmds: Vec<Command> = self
                    .selection
                    .clone()
                    .into_iter()
                    .map(|sid| Command::SetFlags {
                        sid,
                        hidden: None,
                        locked: Some(true),
                        old: None,
                    })
                    .collect();
                if !cmds.is_empty() {
                    self.exec(Command::Compound { cmds });
                }
                self.status = "已锁定所选".into();
            }
            "object.unlock_all" => {
                // 走 SetFlags 复合命令入 undo 栈(此前裸改 arena 不可撤销)
                let mut ids = Vec::new();
                for &ab in &self.doc.artboards {
                    self.doc.subtree(ab, &mut ids);
                }
                let cmds: Vec<Command> = ids
                    .into_iter()
                    .filter_map(|id| {
                        let n = self.doc.nodes.get(id)?;
                        if !n.locked {
                            return None;
                        }
                        Some(Command::SetFlags {
                            sid: n.sid.as_str().to_string(),
                            hidden: None,
                            locked: Some(false),
                            old: None,
                        })
                    })
                    .collect();
                if cmds.is_empty() {
                    self.status = "没有已锁定的对象".into();
                } else {
                    self.exec(Command::Compound { cmds });
                    self.status = "已解锁全部".into();
                }
            }
            "object.hide" => {
                let cmds: Vec<Command> = self
                    .selection
                    .clone()
                    .into_iter()
                    .map(|sid| Command::SetFlags {
                        sid,
                        hidden: Some(true),
                        locked: None,
                        old: None,
                    })
                    .collect();
                if !cmds.is_empty() {
                    self.exec(Command::Compound { cmds });
                }
                self.status = "已隐藏所选".into();
            }
            "object.show_all" => {
                let mut ids = Vec::new();
                for &ab in &self.doc.artboards {
                    self.doc.subtree(ab, &mut ids);
                }
                let cmds: Vec<Command> = ids
                    .into_iter()
                    .filter_map(|id| {
                        let n = self.doc.nodes.get(id)?;
                        if !n.hidden {
                            return None;
                        }
                        Some(Command::SetFlags {
                            sid: n.sid.as_str().to_string(),
                            hidden: Some(false),
                            locked: None,
                            old: None,
                        })
                    })
                    .collect();
                if cmds.is_empty() {
                    self.status = "没有已隐藏的对象".into();
                } else {
                    self.exec(Command::Compound { cmds });
                    self.status = "已显示全部".into();
                }
            }
            // ── P3.2 命令面板 ──
            "app.command_palette" => {
                // 切换语义:面板开着再按 Ctrl+K 关闭(此前只能点 X)
                self.palette_open = !self.palette_open;
                self.palette_query.clear();
                self.palette_sel = 0; // 04-1:重开回到首项
            }
            // ── 画布 ──
            "canvas.nudge_left" | "canvas.nudge_right" | "canvas.nudge_up"
            | "canvas.nudge_down" => {
                let key = match id {
                    "canvas.nudge_left" => Key::ArrowLeft,
                    "canvas.nudge_right" => Key::ArrowRight,
                    "canvas.nudge_up" => Key::ArrowUp,
                    _ => Key::ArrowDown,
                };
                self.arrow_nudge(key, false, shift);
            }
            "canvas.cancel" => {
                // H-6:Esc 回退链第 1 层 —— 有开着的对话框先关对话框并消费
                // (命令面板/文本编辑在 TextEdit 上下文,本命令根本不派发,
                // 它们各自处理;见 dialogs.rs esc_dialog_top 的链路说明)
                if self.esc_dialog_top().is_some() {
                    self.close_esc_dialog_top();
                    return true;
                }
                // 04-3(2):文本「二次 Esc = 放弃」—— Esc 提交后短时武装,
                // 再次 Esc 作废刚提交的 SetText(不进 redo 栈;放弃不可重做)
                if let Some((sid, at)) = self.text_discard_arm.clone() {
                    let armed = at.elapsed() < std::time::Duration::from_secs(5)
                        && self.undo.top().is_some_and(
                            |c| matches!(c, Command::SetText { sid: s, .. } if *s == sid),
                        );
                    self.text_discard_arm = None;
                    if armed {
                        self.undo.cancel_top(&mut self.doc);
                        self.say("已放弃文本修改(未入撤销栈)");
                        return true;
                    }
                }
                // 钢笔进行中:Esc = 结束开放路径(02 篇 §5.3)
                if self.tool == Tool::Pen && !self.pen_points.is_empty() {
                    self.finish_pen(false);
                    self.status = "钢笔:路径已结束(开放)".into();
                    return true;
                }
                // P4.3 隔离模式:Esc 逐层弹出进入栈(面包屑回退)
                if let Some(popped) = self.isolate_stack.pop() {
                    self.selection.clear();
                    let name = self
                        .doc
                        .nodes
                        .get(popped)
                        .map(|n| n.name.clone())
                        .unwrap_or_default();
                    self.status = match self.isolate_top() {
                        Some(up) => format!(
                            "退出 {name} → {}",
                            self.doc
                                .nodes
                                .get(up)
                                .map(|n| n.name.clone())
                                .unwrap_or_default()
                        ),
                        None => format!("退出隔离模式({name})"),
                    };
                    return true;
                }
                self.selection.clear();
                if !matches!(self.drag, Drag::None) {
                    // Esc 取消语义:拖拽产生的合并条目从 undo 栈整体作废
                    // (不进 redo 栈 —— 取消的动作不可重做),文档直接回到
                    // 拖拽前。此前走 exec(还原)会留下一条
                    // 「Ctrl+Z 跳回被取消位置」的 undo 步(B4)
                    match std::mem::replace(&mut self.drag, Drag::None) {
                        Drag::MoveObj { .. }
                        | Drag::Resize { .. }
                        | Drag::Rotate { .. }
                        | Drag::GradientAnnotate { .. }
                        // 05-2:X-4 工具族拖拽同样可能已逐帧落命令
                        | Drag::ToolRotate { .. }
                        | Drag::ToolMirror { .. }
                        | Drag::ToolScale { .. }
                        | Drag::FreeTransform { .. } => {
                            if self.drag_edited {
                                self.undo.cancel_top(&mut self.doc);
                                self.status = "已取消(未入撤销栈)".into();
                            } else {
                                self.status = "已取消".into();
                            }
                            self.drag_edited = false;
                            self.last_move_delta = None;
                        }
                        _ => {
                            self.status = "已取消".into();
                        }
                    }
                    self.smart_guides.clear();
                }
                // 04-5-4(P1-⑤):Esc 从任意工具回选择。取消语义处理完后,
                // 创建类工具(矩形/钢笔/画板…)不再驻留 —— 点画布不会误建形状。
                if self.tool != Tool::Select {
                    self.set_tool(Tool::Select);
                    self.status = "已回到选择工具(Esc 退出工具态)".into();
                }
            }
            // ── P4 钢笔:Enter 结束路径 ──
            "canvas.pen_finish" => {
                if self.tool == Tool::Pen && !self.pen_points.is_empty() {
                    self.finish_pen(false);
                    self.status = "钢笔:路径已结束".into();
                }
            }
            // ── 应用级(无键位,仅菜单) ──
            "app.about" => self.show_about = true,
            // 03-4 / X-6:浏览器校对面板(画布 vs 系统浏览器)
            "view.browser_proof" => self.toggle_proofread(),
            // 阶段 2:退出先同步会话(recent.json 的 session 字段)再自然退出
            "app.quit" => {
                if let Some(tx) = &self.shell_tx {
                    let _ = tx.send(crate::shell::ShellRequest::QuitAll);
                    self.say("正在退出…");
                } else {
                    std::process::exit(0);
                }
            }
            _ => matched = false,
        }
        matched
    }
}
