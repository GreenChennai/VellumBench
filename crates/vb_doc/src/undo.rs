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
        cmd.revert(doc)?;
        self.redo.push(cmd);
        self.last_merge = None;
        doc.rev += 1;
        Ok(self.undo.last().map(|c| c.label().to_string()))
    }

    pub fn redo(&mut self, doc: &mut Document) -> Result<Option<String>> {
        let Some(mut cmd) = self.redo.pop() else {
            return Ok(None);
        };
        cmd.apply(doc)?;
        self.undo.push(cmd);
        self.last_merge = None;
        doc.rev += 1;
        Ok(self.undo.last().map(|c| c.label().to_string()))
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
fn replace_new(top: &mut Command, src: &Command) {
    use Command::*;
    match (top, src) {
        (SetGeom { new, .. }, SetGeom { new: n2, .. }) => *new = *n2,
        (SetStyle { new, .. }, SetStyle { new: n2, .. }) => *new = n2.clone(),
        (SetText { new, .. }, SetText { new: n2, .. }) => *new = n2.clone(),
        (Rename { new, .. }, Rename { new: n2, .. }) => *new = n2.clone(),
        _ => {}
    }
}
