//! Undo/Redo 栈:合并策略 + 无限深度(设计文档 02 篇 §六:无限撤销,命令模式)。
//!
//! 内存上限:**200MB 软上限**(`MAX_UNDO_BYTES`,按 [`command_bytes`] 估算记账,
//! 超限从栈底丢弃最旧条目 —— 大文档长会话不再无界吃内存)。ADR-0018 的
//! 溢出**写盘**(`.vbdoc/history/`)仍是 v0.2 项;丢弃语义 = 超出部分的
//! 历史不可再撤销,与主流设计工具一致。

use std::time::{Duration, Instant};

use crate::commands::{ChangeSet, Command};
use crate::model::Document;
use crate::Result;

/// 合并窗口:同 kind + 同 target 且间隔 ≤500ms → 合并(不新增 undo 条目)。
const MERGE_WINDOW: Duration = Duration::from_millis(500);

/// undo 栈内存软上限(字节)。超过即从栈底丢弃,保证新操作永远可撤销。
pub const MAX_UNDO_BYTES: usize = 200 * 1024 * 1024;

#[derive(Default)]
pub struct UndoStack {
    undo: Vec<Command>,
    redo: Vec<Command>,
    last_merge: Option<(crate::commands::CmdKind, String, Instant)>,
    /// 压栈时是否允许合并(拖动中为 true,松手后首次压栈为 false 由调用方控制)。
    pub merging_enabled: bool,
    /// 提交会话(S1-b 02-6-2:NumField 连续 scrubby/键入合并为一次 undo)。
    /// 会话期间合并不看 500ms 窗口 —— 慢速拖动/逐字键入拆条才是 bug;
    /// `end_session` 时清 `last_merge`,会话外的相邻编辑不误并。
    session_merge: bool,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            last_merge: None,
            merging_enabled: true,
            session_merge: false,
        }
    }

    /// 应用并入栈。返回是否实际应用。
    pub fn push(&mut self, doc: &mut Document, mut cmd: Command) -> Result<ChangeSet> {
        // 05-8 符号语义收口(所有命令路径的唯一入口):实例编辑自动登记
        // 覆盖字段,主件编辑自动追加懒构建的 SymbolSync(同一条 undo)。
        cmd = crate::symbol::wrap_symbol_effects(doc, cmd);
        let key = if self.merging_enabled {
            cmd.merge_target()
        } else {
            None
        };
        let mergeable = match (&key, &self.last_merge) {
            (Some((k, t)), Some((lk, lt, at))) => {
                k == lk && t == lt && (self.session_merge || at.elapsed() <= MERGE_WINDOW)
            }
            _ => false,
        };
        let cs = if mergeable {
            // 合并:只更新栈顶的 new 值(old 保留最初状态),不重复 apply 到文档外对象——
            // 文档已经处于中间态,直接按新值改写即可。
            let top = self.undo.last_mut().expect("merge 需要栈顶");
            replace_new(top, &cmd);
            cmd.apply(doc)? // 正常 apply(old 已被首条捕获时不会覆盖;此处 cmd 是新命令)
        } else {
            let cs = cmd.apply(doc)?;
            self.undo.push(cmd);
            self.enforce_memory_cap();
            self.last_merge = key.clone().map(|(k, t)| (k, t, Instant::now()));
            cs
        };
        if mergeable {
            if let Some((k, t)) = &key {
                self.last_merge = Some((*k, t.clone(), Instant::now()));
            }
        }
        self.redo.clear();
        doc.rev += 1;
        Ok(cs)
    }

    pub fn undo(&mut self, doc: &mut Document) -> Result<Option<String>> {
        let Some(mut cmd) = self.undo.pop() else {
            return Ok(None);
        };
        match cmd.revert(doc) {
            Ok(_) => {
                self.redo.push(cmd);
                self.last_merge = None;
                doc.rev += 1;
                Ok(self.undo.last().map(|c| c.label().to_string()))
            }
            // 失败时命令推回原栈:丢弃会让该步操作既不能重试也不能重做
            Err(e) => {
                self.undo.push(cmd);
                Err(e)
            }
        }
    }

    /// 弹出栈顶命令并直接 revert,**不进入 redo 栈**(Esc 取消语义:
    /// 本次拖拽整体作废,不可重做)。栈空或 revert 失败(命令推回)时
    /// 文档保持原状。
    pub fn cancel_top(&mut self, doc: &mut Document) {
        if let Some(mut cmd) = self.undo.pop() {
            match cmd.revert(doc) {
                Ok(_) => doc.rev += 1,
                Err(_) => self.undo.push(cmd),
            }
        }
        self.last_merge = None;
    }

    pub fn redo(&mut self, doc: &mut Document) -> Result<Option<String>> {
        let Some(mut cmd) = self.redo.pop() else {
            return Ok(None);
        };
        match cmd.apply(doc) {
            Ok(_) => {
                self.undo.push(cmd);
                self.last_merge = None;
                doc.rev += 1;
                Ok(self.undo.last().map(|c| c.label().to_string()))
            }
            Err(e) => {
                self.redo.push(cmd);
                Err(e)
            }
        }
    }

    /// 栈顶命令窥视(宿主在 undo/redo 后据此恢复选区;不移除)。
    pub fn top(&self) -> Option<&Command> {
        self.undo.last()
    }

    /// redo 栈顶命令窥视。
    pub fn top_redo(&self) -> Option<&Command> {
        self.redo.last()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|c| c.label())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|c| c.label())
    }

    // ── 历史面板只读访问器(vb_app 07-D;不暴露可变性,栈语义不受影响)──

    /// 撤销栈全部命令(保存序 = 旧 → 新;历史列表展示用)。
    pub fn undo_slice(&self) -> &[Command] {
        &self.undo
    }

    /// 重做栈全部命令(保存序 = 下一个重做在末尾)。
    pub fn redo_slice(&self) -> &[Command] {
        &self.redo
    }

    /// 撤销深度(历史"当前位置"= 该值)。
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// 重做深度。
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// 清空重做尾(历史跳转的"丢弃其后步骤"语义;不影响文档)。
    pub fn clear_redo(&mut self) {
        self.redo.clear();
        self.last_merge = None;
    }

    /// 事务批量应用(一次 patch = 一条 undo,设计文档 08 篇 §五)。
    pub fn push_compound(&mut self, doc: &mut Document, cmds: Vec<Command>) -> Result<ChangeSet> {
        self.push(doc, Command::Compound { cmds })
    }

    /// 开始提交会话(S1-b 02-6-2)。会话期间,同目标的可合并命令
    /// (Set*/同类 Compound,见 `Command::merge_target`)**不受 500ms
    /// 窗口限制**地并入栈顶条目 —— NumField 的连续 scrubby/键盘步进/
    /// 连续表达式提交,无论多慢都只算一次 undo。
    ///
    /// 嵌套调用安全:重复 `begin_session` 只置位;会话由第一个
    /// `end_session` 结束(宿主保证一一配对,见 `vb_app` 的 `num_commit`)。
    pub fn begin_session(&mut self) {
        self.session_merge = true;
    }

    /// 结束提交会话。同时清 `last_merge`:会话结束后的第一条同目标
    /// 命令是**新的一次编辑**,不该并进会话条目。
    pub fn end_session(&mut self) {
        self.session_merge = false;
        self.last_merge = None;
    }

    /// 会话是否进行中(宿主断言配对用)。
    pub fn session_active(&self) -> bool {
        self.session_merge
    }

    /// undo 栈估算字节(历史面板/诊断展示用)。
    pub fn bytes_estimated(&self) -> usize {
        self.undo.iter().map(command_bytes).sum()
    }

    /// 内存上限执行:超 [`MAX_UNDO_BYTES`] 时从栈底(最旧)丢弃。
    /// 只丢 undo 不动 redo——新操作压栈必然清 redo,redo 的存量在下次
    /// push 前仍可重做,属活跃数据;丢弃不计通知(历史面板自然变短)。
    fn enforce_memory_cap(&mut self) {
        let mut total: usize = self.undo.iter().map(command_bytes).sum();
        if total <= MAX_UNDO_BYTES {
            return;
        }
        let mut drop_from = 0usize;
        for (i, cmd) in self.undo.iter().enumerate() {
            drop_from = i + 1; // 至少保留当前条目之后的新历史
            total -= command_bytes(cmd);
            if total <= MAX_UNDO_BYTES {
                break;
            }
        }
        self.undo.drain(..drop_from);
    }
}

/// 单条命令的内存占用估算(字节)。记账口径:结构体按 `size_of` 底价,
/// 堆载体(String/Vec/BezPath)按当前长度计 —— 不求精确,求不漏大件。
fn command_bytes(cmd: &Command) -> usize {
    use Command::*;
    std::mem::size_of::<Command>()
        + match cmd {
            Insert {
                parent_sid, tree, ..
            } => parent_sid.len() + tree_bytes(tree),
            Delete { captured, .. } => captured
                .as_ref()
                .map(|c| {
                    tree_bytes(&c.tree) + c.anim_blocks.iter().map(|(_, s)| s.len()).sum::<usize>()
                })
                .unwrap_or(0),
            Move { new_parent_sid, .. } => new_parent_sid.len(),
            SetGeom { .. } | SetFlags { .. } | SetTextMode { .. } => 0,
            SetStyle { new, old, .. } => {
                decls_bytes(new) + old.as_ref().map(|o| decls_bytes(o)).unwrap_or(0)
            }
            SetAttrs { new, old, .. } => {
                attrs_bytes(new) + old.as_ref().map(|o| attrs_bytes(o)).unwrap_or(0)
            }
            SetText { new, old, .. } => {
                new.len()
                    + old
                        .as_ref()
                        .map(|s| s.text.len() + s.segs.len() * 64)
                        .unwrap_or(0)
            }
            SetSegs { new, old, .. } => (new.len() + old.as_ref().map(Vec::len).unwrap_or(0)) * 64,
            Rename { new, old, .. } => new.len() + old.as_ref().map(String::len).unwrap_or(0),
            SetTag { new, old, .. } => new.len() + old.as_ref().map(String::len).unwrap_or(0),
            SetVector { new, old, .. } => {
                // BezPath 元素按 32B/段保守计
                (new.elements().len() + old.as_ref().map(|p| p.elements().len()).unwrap_or(0)) * 32
            }
            Group {
                member_sids, name, ..
            } => name.len() + member_sids.iter().map(String::len).sum::<usize>(),
            Ungroup { captured, .. } => captured.as_ref().map(|(_, t)| tree_bytes(t)).unwrap_or(0),
            Compound { cmds } => cmds.iter().map(command_bytes).sum(),
            SetToken { name, new, old, .. } => {
                name.len()
                    + new.len()
                    + old
                        .as_ref()
                        .map(|o| o.as_ref().map(|(_, s)| s.len()).unwrap_or(0))
                        .unwrap_or(0)
            }
            SetNodeAnimation { .. } => 256, // 关键帧/声明快照,量级小,常数计
            PathBoolean {
                new_path, captured, ..
            } => {
                new_path.elements().len() * 32
                    + captured
                        .as_ref()
                        .map(|(lhs, _, _, tree)| {
                            lhs.as_ref().map(|p| p.elements().len() * 32).unwrap_or(0)
                                + tree_bytes(tree)
                        })
                        .unwrap_or(0)
            }
            // 兜底:其余变体新增时默认零堆载体,审查点在 PR
            _ => 0,
        }
}

/// NodeTree 快照字节(递归;节点文本/attrs 按长度,树骨架按节点数 × 96B)。
fn tree_bytes(tree: &crate::model::NodeTree) -> usize {
    fn rec(t: &crate::model::NodeTree, acc: &mut usize) {
        *acc += std::mem::size_of::<crate::model::NodeTree>() + 96;
        *acc += t.node.name.len() + t.node.text().map(str::len).unwrap_or(0);
        for c in &t.children {
            rec(c, acc);
        }
    }
    let mut acc = 0;
    rec(tree, &mut acc);
    acc
}

fn decls_bytes(decls: &[vb_css::Decl]) -> usize {
    decls
        .iter()
        .map(|d| d.prop.len() + d.value.len() + 48)
        .sum()
}

fn attrs_bytes(attrs: &[(String, String)]) -> usize {
    attrs.iter().map(|(k, v)| k.len() + v.len() + 48).sum()
}

/// 用 `src` 的 new 值覆盖 `top` 的 new 值(合并时保持最初 old)。
/// 必须与 `Command::merge_target` 的可合并集合保持一致 —— 漏一个 arm,
/// Redo 就会把中间值写回文档,吞掉最后一次编辑。
fn replace_new(top: &mut Command, src: &Command) {
    use Command::*;
    match (top, src) {
        (SetGeom { new, .. }, SetGeom { new: n2, .. }) => *new = *n2,
        (SetStyle { new, .. }, SetStyle { new: n2, .. }) => *new = n2.clone(),
        (SetText { new, .. }, SetText { new: n2, .. }) => *new = n2.clone(),
        (SetSegs { new, .. }, SetSegs { new: n2, .. }) => *new = n2.clone(),
        (Rename { new, .. }, Rename { new: n2, .. }) => *new = n2.clone(),
        (SetTag { new, .. }, SetTag { new: n2, .. }) => *new = n2.clone(),
        (SetVector { new, .. }, SetVector { new: n2, .. }) => *new = n2.clone(),
        // 05-9:时间轴连续编辑(拖拽关键帧/改值)同对象合并,只更新
        // new 值;old 保留首帧捕获的原状(merge_target 已限定同 sid)
        (
            SetNodeAnimation {
                new_keyframes,
                new_animation,
                ..
            },
            SetNodeAnimation {
                new_keyframes: k2,
                new_animation: a2,
                ..
            },
        ) => {
            *new_keyframes = k2.clone();
            *new_animation = a2.clone();
        }
        // 多目标 SetStyle/SetGeom Compound(渐变拖拽/多选拖拽):按 sid
        // 配对更新 new,栈顶首帧捕获的 old 不动(可合并性由 merge_target 校验)
        (Compound { cmds: tcmds, .. }, Compound { cmds: scmds, .. }) => {
            for t in tcmds.iter_mut() {
                match t {
                    SetStyle { sid, new, .. } => {
                        if let Some(SetStyle { new: n2, .. }) = scmds
                            .iter()
                            .find(|c| matches!(c, SetStyle { sid: s2, .. } if s2 == sid))
                        {
                            *new = n2.clone();
                        }
                    }
                    // S4 外观条目写回(merge_target "mas"):SetAttrs 与
                    // SetStyle 一同按 sid 配对更新,模型 JSON 不丢最后一帧
                    SetAttrs { sid, new, .. } => {
                        if let Some(SetAttrs { new: n2, .. }) = scmds
                            .iter()
                            .find(|c| matches!(c, SetAttrs { sid: s2, .. } if s2 == sid))
                        {
                            *new = n2.clone();
                        }
                    }
                    SetGeom { sid, new, .. } => {
                        if let Some(SetGeom { new: n2, .. }) = scmds
                            .iter()
                            .find(|c| matches!(c, SetGeom { sid: s2, .. } if s2 == sid))
                        {
                            *new = *n2;
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod memory_cap_tests {
    use super::*;
    use crate::commands::Command;
    use crate::model::Document;

    /// 大载荷命令:SetToken 携带 n 字节的 new 值,直接应用到 tokens 表,
    /// 不需要节点配合。
    fn big_cmd(n: usize, tag: &str) -> Command {
        Command::SetToken {
            name: format!("tk-{tag}"),
            new: "x".repeat(n),
            old: None,
        }
    }

    #[test]
    fn cap_drops_oldest_keeps_recent_undoable() {
        let mut doc = Document::new_default();
        let mut st = UndoStack::new();
        // 20 条 16MB ≈ 320MB > 200MB 上限 → 栈底被丢
        for i in 0..20 {
            let r = st.push(&mut doc, big_cmd(16 * 1024 * 1024, &i.to_string()));
            assert!(r.is_ok(), "push {i} 失败:{r:?}");
        }
        assert!(
            st.bytes_estimated() <= MAX_UNDO_BYTES,
            "超限未回收:{}",
            st.bytes_estimated()
        );
        // 近期历史仍可撤销:连续 undo 到栈空,不应有失败
        let mut undos = 0;
        while st.can_undo() {
            assert!(st.undo(&mut doc).is_ok());
            undos += 1;
        }
        assert!(undos > 0, "上限回收后一条都撤不了");
        assert!(undos < 20, "上限未生效(全部 20 条都在)");
    }

    #[test]
    fn under_cap_nothing_dropped() {
        let mut doc = Document::new_default();
        let mut st = UndoStack::new();
        for i in 0..8 {
            let _ = st.push(&mut doc, big_cmd(1024, &i.to_string()));
        }
        assert_eq!(st.undo_len(), 8);
        assert!(st.bytes_estimated() < MAX_UNDO_BYTES);
    }

    #[test]
    fn estimator_counts_heap_carriers() {
        let cmd = big_cmd(10_000, "est");
        assert!(
            command_bytes(&cmd) > 10_000,
            "估算必须计入 String 堆载体:{}",
            command_bytes(&cmd)
        );
    }
}
