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

/// `Default` 为手写实现(见下):derive 版会把 `cap_bytes` 置 0,
/// 直接废掉内存上限语义 —— 以手写版为准。
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
    /// 内存软上限实际生效值(默认 [`MAX_UNDO_BYTES`];测试注入小上限
    /// 验证 DOC-02 场景,免 200MB 级测试载荷)。
    cap_bytes: usize,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoStack {
    pub fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            last_merge: None,
            merging_enabled: true,
            session_merge: false,
            cap_bytes: MAX_UNDO_BYTES,
        }
    }

    /// 测试注入:小上限复现「单条超大命令清栈」路径(DOC-02)。
    #[cfg(test)]
    fn with_cap(cap_bytes: usize) -> Self {
        let mut st = Self::new();
        st.cap_bytes = cap_bytes;
        st
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
            // DOC-02:栈可能刚被内存上限回收清空(enforce_memory_cap),此时
            // **不得 expect**(旧实现 panic 中断整个编辑会话),回退非合并路径
            // 正常入栈 —— undo 语义等价(空栈上的「合并」本来就是新条目)。
            match self.undo.last_mut() {
                Some(top) => {
                    replace_new(top, &cmd);
                    cmd.apply(doc)? // 正常 apply(old 已被首条捕获时不会覆盖;此处 cmd 是新命令)
                }
                None => {
                    let cs = cmd.apply(doc)?;
                    self.undo.push(cmd);
                    self.enforce_memory_cap();
                    cs
                }
            }
        } else {
            let cs = cmd.apply(doc)?;
            self.undo.push(cmd);
            self.enforce_memory_cap();
            cs
        };
        // DOC-02:合并标记**统一在内存回收之后**收口 —— 仅当栈顶存活时
        // 才指向它;单条超限把整栈清空时保持 `None`。旧实现两处无条件
        // `last_merge = Some(...)`(先于/无视回收),把标记指向已被回收
        // 的空栈 —— 下一条可合并命令带着 Some 进 merge 分支,对空栈
        // `expect` panic。标记与栈态的一致性是本函数的唯一不变量。
        self.last_merge = if self.undo.is_empty() {
            None
        } else {
            key.map(|(k, t)| (k, t, Instant::now()))
        };
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

    /// 内存上限执行:超上限时从栈底(最旧)丢弃。
    /// 只丢 undo 不动 redo——新操作压栈必然清 redo,redo 的存量在下次
    /// push 前仍可重做,属活跃数据;丢弃不计通知(历史面板自然变短)。
    fn enforce_memory_cap(&mut self) {
        let mut total: usize = self.undo.iter().map(command_bytes).sum();
        if total <= self.cap_bytes {
            return;
        }
        let mut drop_from = 0usize;
        for (i, cmd) in self.undo.iter().enumerate() {
            drop_from = i + 1; // 至少保留当前条目之后的新历史
            total -= command_bytes(cmd);
            if total <= self.cap_bytes {
                break;
            }
        }
        self.undo.drain(..drop_from);
        // DOC-02:整栈清空(单条命令即超限)时合并标记必须同步失效,
        // 否则下一条可合并命令会带着 Some(last_merge) 进 merge 分支,
        // 对空栈取栈顶(旧实现 expect panic)。
        if self.undo.is_empty() {
            self.last_merge = None;
        }
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

/// NodeTree 快照字节(节点文本/attrs 按长度,树骨架按节点数 × 96B)。
/// 显式栈迭代(RB-02):快照可来自任意深的文档,递归版有栈溢出面。
fn tree_bytes(tree: &crate::model::NodeTree) -> usize {
    let mut acc = 0usize;
    let mut stack = vec![tree];
    while let Some(t) = stack.pop() {
        acc += std::mem::size_of::<crate::model::NodeTree>() + 96;
        acc += t.node.name.len() + t.node.text().map(str::len).unwrap_or(0);
        for c in &t.children {
            stack.push(c);
        }
    }
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

    /// DOC-02(验收):单条超大命令把栈清空后,后续**可合并**命令进入
    /// merge 判定 —— 旧实现 `expect("merge 需要栈顶")` panic 中断整个
    /// 编辑会话;现在必须不 panic 且 undo 语义正确。
    #[test]
    fn oversized_command_then_mergeable_command_does_not_panic() {
        let mut doc = Document::new_default();
        let root_sid = doc.nodes.get(doc.root).unwrap().sid.as_str().to_string();
        // 上限相对 size_of::<Command>() 取值:command_bytes 的常数项就是
        // 枚举本身的大小(Delete 携带内联子树捕获,可达数百字节)——
        // 常数取值会让「小命令」也触发整栈回收,淹没被测语义。这里
        // cap = 4 × 常数项,大命令载荷 = 16 × 常数项。
        let base = std::mem::size_of::<Command>();
        let mut st = UndoStack::with_cap(base * 4);

        // ① 单条超大命令:入栈即触发回收,整栈清空,合并标记同步失效
        let big = Command::Rename {
            sid: root_sid.clone(),
            new: "x".repeat(base * 16),
            old: None,
        };
        st.push(&mut doc, big).unwrap();
        assert!(!st.can_undo(), "单条即超限必须被整体回收");
        assert!(st.undo_label().is_none());

        // ② 后续可合并命令(同 kind + 同 target):不得 panic(旧实现
        // 在此 `expect` 崩溃)。该命令 apply 时捕获 ① 写入的超大 old →
        // 自身也超限:效果落盘、历史被回收 —— 上限的既有语义(超出部分
        // 的历史不可再撤销),关键回归点是**不 panic + 标记不指向空栈**。
        let small = Command::Rename {
            sid: root_sid.clone(),
            new: "改名甲".to_string(),
            old: None,
        };
        st.push(&mut doc, small).unwrap();
        assert_eq!(
            doc.node(doc.root).unwrap().name,
            "改名甲",
            "超限回收只影响历史,不影响命令效果"
        );

        // ③ 再来一条常规命令:old 已回落(= ② 的落盘值),正常入栈
        st.push(
            &mut doc,
            Command::Rename {
                sid: root_sid.clone(),
                new: "改名乙".to_string(),
                old: None,
            },
        )
        .unwrap();
        assert!(st.can_undo(), "常规命令必须正常入栈");
        assert_eq!(st.undo_len(), 1);
        assert_eq!(
            doc.node(doc.root).unwrap().name,
            "改名乙",
            "文档值 = 最新命令"
        );

        // ④ undo 语义正确:一次撤销回到 ③ 捕获的 old(= ② 的落盘值)
        st.undo(&mut doc).unwrap();
        assert!(!st.can_undo());
        assert_eq!(
            doc.node(doc.root).unwrap().name,
            "改名甲",
            "撤销还原到栈内命令捕获的 old"
        );
    }

    /// DOC-02:部分回收(超限但栈未清空)时合并标记必须仍然有效 ——
    /// 栈顶存活、last_merge 保留,窗口内同目标命令照常合并。
    #[test]
    fn partial_cap_drop_keeps_top_mergeable() {
        let mut doc = Document::new_default();
        let root_sid = doc.nodes.get(doc.root).unwrap().sid.as_str().to_string();
        let mut st = UndoStack::with_cap(1024);
        for i in 0..8 {
            let cmd = Command::Rename {
                sid: root_sid.clone(),
                new: format!("n{i}-{}", "y".repeat(96)),
                old: None,
            };
            st.push(&mut doc, cmd).unwrap();
        }
        assert!(
            st.bytes_estimated() <= 1024,
            "预置条件:必须触发过回收:{}",
            st.bytes_estimated()
        );
        assert!(st.can_undo(), "部分回收必须保留近期条目");
        let before = st.undo_len();
        st.push(
            &mut doc,
            Command::Rename {
                sid: root_sid.clone(),
                new: "latest".to_string(),
                old: None,
            },
        )
        .unwrap();
        assert_eq!(st.undo_len(), before, "栈顶存活的合并不得新增条目");
    }
}
