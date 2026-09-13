//! Undo/Redo 栈:合并策略 + 无限深度(设计文档 02 篇 §六:无限撤销,命令模式)。
//!
//! v0.1 内存栈,无溢出写盘(200MB 上限 + `.vbdoc/history/` 溢出为 v0.2 项,ADR-0018)。

use std::time::{Duration, Instant};

use crate::commands::{ChangeSet, Command};
use crate::model::Document;
use crate::Result;

/// 合并窗口:同 kind + 同 target 且间隔 ≤500ms → 合并(不新增 undo 条目)。
const MERGE_WINDOW: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct UndoStack {
    undo: Vec<Command>,
    redo: Vec<Command>,
    last_merge: Option<(crate::commands::CmdKind, String, Instant)>,
    /// 压栈时是否允许合并(拖动中为 true,松手后首次压栈为 false 由调用方控制)。
    pub merging_enabled: bool,
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            last_merge: None,
            merging_enabled: true,
        }
    }

    /// 应用并入栈。返回是否实际应用。
    pub fn push(&mut self, doc: &mut Document, mut cmd: Command) -> Result<ChangeSet> {
        let key = if self.merging_enabled {
            cmd.merge_target()
        } else {
            None
        };
        let mergeable = match (&key, &self.last_merge) {
            (Some((k, t)), Some((lk, lt, at))) => {
                k == lk && t == lt && at.elapsed() <= MERGE_WINDOW
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

    /// 事务批量应用(一次 patch = 一条 undo,设计文档 08 篇 §五)。
    pub fn push_compound(&mut self, doc: &mut Document, cmds: Vec<Command>) -> Result<ChangeSet> {
        self.push(doc, Command::Compound { cmds })
    }
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
        (Rename { new, .. }, Rename { new: n2, .. }) => *new = n2.clone(),
        (SetTag { new, .. }, SetTag { new: n2, .. }) => *new = n2.clone(),
        (SetVector { new, .. }, SetVector { new: n2, .. }) => *new = n2.clone(),
        // 多目标 SetStyle Compound(渐变拖拽):按 sid 配对更新 new,
        // 栈顶首帧捕获的 old 不动(可合并性由 merge_target 校验)
        (Compound { cmds: tcmds, .. }, Compound { cmds: scmds, .. }) => {
            for t in tcmds.iter_mut() {
                let (sid, new) = match t {
                    SetStyle { sid, new, .. } => (sid, new),
                    _ => continue,
                };
                if let Some(SetStyle { new: n2, .. }) = scmds
                    .iter()
                    .find(|c| matches!(c, SetStyle { sid: s2, .. } if s2 == sid))
                {
                    *new = n2.clone();
                }
            }
        }
        _ => {}
    }
}
