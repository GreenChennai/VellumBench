//! 外部冲突三方对比对话框(阶段 5 / 05-4-A2;台账 09-N)。
//!
//! 「文件 → 对比并合并…」(07-R 未采用印记的处置入口):磁盘版本 /
//! 内存版本 / 自动快照 三方可读 diff(reuse `crate::autosave::line_diff`
//! 的 LCS 行 diff,与 recover.rs 差异视图同一套差异语义),并给两个
//! 决断动作:
//! - **以磁盘为准重载**:弃本地未保存编辑,重载磁盘(与热重载同一条
//!   导入链);印记转为「已重载」;
//! - **以内存为准存回**:本地编辑胜,Ctrl+S 路径写回磁盘;印记转为已消化。
//!
//! 与 07-R 的打通:状态栏「外部已改动(未采用)」印记点击**直接打开**
//! 本对话框(见 `panels::status_bar`);信息窗保留(07-R 原语义)。
//!
//! 打开条件(命令侧校验):存在未采用的印记 + 有项目目录。无冲突时
//! 点命令给中文提示,不弹空窗。

use crate::autosave::{self, DiffKind, DiffRow};

use super::VellumApp;

/// 三方对比的差分对(单选;默认 磁盘 ↔ 内存)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffPair {
    DiskMemory,
    DiskSnapshot,
    SnapshotMemory,
}

impl DiffPair {
    pub const ALL: [DiffPair; 3] = [
        DiffPair::DiskMemory,
        DiffPair::DiskSnapshot,
        DiffPair::SnapshotMemory,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DiffPair::DiskMemory => "磁盘 ↔ 内存",
            DiffPair::DiskSnapshot => "磁盘 ↔ 快照",
            DiffPair::SnapshotMemory => "快照 ↔ 内存",
        }
    }
}

/// 一方的可读摘要(缺失如实标注,不装作有值)。
pub(crate) struct SideSummary {
    pub label: &'static str,
    pub lines: Option<usize>,
}

impl SideSummary {
    fn text(&self) -> String {
        match self.lines {
            Some(n) => format!("{}:{} 行", self.label, n),
            None => format!("{}:不可读/缺失", self.label),
        }
    }
}

// ─────────────────────────── 纯函数(单测直打) ───────────────────────────

/// 三方摘要(磁盘 / 快照 / 内存;快照可缺)。
pub(crate) fn side_summaries(
    disk: Option<&str>,
    snapshot: Option<&str>,
    memory: &str,
) -> [SideSummary; 3] {
    [
        SideSummary {
            label: "磁盘 index.html",
            lines: disk.map(|d| d.lines().count()),
        },
        SideSummary {
            label: "自动快照",
            lines: snapshot.map(|s| s.lines().count()),
        },
        SideSummary {
            label: "内存当前态",
            lines: Some(memory.lines().count()),
        },
    ]
}

/// 指定差分对的行 diff(任一方缺失 → 单行占位,如实标注)。
pub(crate) fn pair_diff(
    pair: DiffPair,
    disk: Option<&str>,
    snapshot: Option<&str>,
    memory: &str,
) -> Vec<DiffRow> {
    let (a, b, la, lb): (Option<&str>, Option<&str>, &str, &str) = match pair {
        DiffPair::DiskMemory => (disk, Some(memory), "磁盘", "内存"),
        DiffPair::DiskSnapshot => (disk, snapshot, "磁盘", "快照"),
        DiffPair::SnapshotMemory => (snapshot, Some(memory), "快照", "内存"),
    };
    match (a, b) {
        (Some(a), Some(b)) => autosave::line_diff(a, b),
        _ => vec![DiffRow {
            kind: DiffKind::Same,
            text: format!(
                "(一方缺失,无法做双方 diff;缺失方:{})",
                if a.is_none() { la } else { lb }
            ),
        }],
    }
}

// ─────────────────────────── 对话框(渲染层) ───────────────────────────

impl VellumApp {
    /// 是否存在待处置的外部冲突(命令入口与印记点击共用的判据)。
    pub(crate) fn has_pending_conflict(&self) -> bool {
        self.project_dir.is_some()
            && self
                .external_change
                .as_ref()
                .map(|e| !e.adopted)
                .unwrap_or(false)
    }

    /// 「文件 → 对比并合并…」窗口(09-N)。
    pub(crate) fn show_conflict_window(&mut self, ui: &mut egui::Ui) {
        if !self.conflict_open {
            return;
        }
        // 现读三方内容(窗口开着时每帧重读,量级为单文件文本,可忽略;
        // 好处:磁盘再被改也能看到最新值)
        let disk = self
            .project_dir
            .as_ref()
            .and_then(|d| std::fs::read_to_string(d.join("index.html")).ok());
        let snap = self
            .project_dir
            .as_ref()
            .and_then(|d| autosave::read_newest(d))
            .map(|(s, _)| s);
        let snap_html: Option<String> = snap
            .as_ref()
            .and_then(|s| s.index_html().map(str::to_string));
        let memory = vb_doc::export::render_project(&self.doc);
        let mem_html = memory
            .files
            .iter()
            .find(|(p, _)| p == "index.html")
            .map(|(_, c)| c.clone())
            .unwrap_or_default();

        let mut open = true;
        let mut action: Option<ConflictAction> = None;
        let mut cancel = false;
        egui::Window::new("外部改动 — 对比并合并")
            .open(&mut open)
            .collapsible(false)
            .default_size([760.0, 500.0])
            .show(ui.ctx(), |ui| {
                ui.colored_label(
                    vb_ui::theme::tokens(ui.ctx()).warn,
                    "磁盘上的 index.html 已被外部修改,而本地有未保存编辑(未自动采用)。",
                );
                // 三方摘要
                ui.horizontal_wrapped(|ui| {
                    for side in side_summaries(disk.as_deref(), snap_html.as_deref(), &mem_html) {
                        ui.label(side.text());
                        ui.separator();
                    }
                });
                // 差分对选择
                let mut pair = self.conflict_diff_pair;
                ui.horizontal(|ui| {
                    ui.label("对比");
                    for p in DiffPair::ALL {
                        if ui.selectable_label(pair == p, p.label()).clicked() {
                            pair = p;
                        }
                    }
                    self.conflict_diff_pair = pair;
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let rows = pair_diff(pair, disk.as_deref(), snap_html.as_deref(), &mem_html);
                    let changed = rows.iter().filter(|r| r.kind != DiffKind::Same).count();
                    ui.label(format!(
                        "统一视图;差异 {changed} 行。「−」= 前者独有,「+」= 后者独有。"
                    ));
                    ui.separator();
                    // 差异红绿走 vb_ui::theme::semantic 的差异令牌(与
                    // recover.rs 差异视图同一口径:深色原值,浅色自动加深)
                    let t = vb_ui::theme::tokens(ui.ctx());
                    for r in &rows {
                        let (color, prefix) = match r.kind {
                            DiffKind::Same => (t.text_2, "  "),
                            DiffKind::Del => (vb_ui::theme::semantic::diff_del(t.dark), "− "),
                            DiffKind::Add => (vb_ui::theme::semantic::diff_add(t.dark), "+ "),
                        };
                        ui.label(
                            egui::RichText::new(format!("{prefix}{}", r.text))
                                .color(color)
                                .monospace(),
                        );
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("以磁盘为准重载(放弃本地编辑)").clicked() {
                        action = Some(ConflictAction::TakeDisk);
                    }
                    if ui.button("以内存为准存回(覆盖磁盘)").clicked() {
                        action = Some(ConflictAction::TakeMemory);
                    }
                    if ui.button("先不动").clicked() {
                        cancel = true;
                    }
                });
                ui.weak("快照列为最近一次自动保存(.vb-autosave/);项目没有快照时该方显示缺失。");
            });
        match action {
            Some(ConflictAction::TakeDisk) => self.conflict_take_disk(),
            Some(ConflictAction::TakeMemory) => self.conflict_take_memory(),
            None => {}
        }
        if cancel {
            open = false;
        }
        self.conflict_open = open && self.has_pending_conflict();
    }

    /// 「以磁盘为准重载」:弃本地编辑,重载磁盘(与热重载同一条导入链);
    /// 撤销栈/画布态清空(文档整体换血),印记转为「已重载」。
    pub(crate) fn conflict_take_disk(&mut self) {
        let Some(dir) = self.project_dir.clone() else {
            return;
        };
        match super::external::import_with_layout(&dir) {
            Ok(r) => {
                let n = r.doc.artboards.len();
                self.doc = r.doc;
                self.undo = vb_doc::undo::UndoStack::new();
                self.selection.clear();
                self.isolate_stack.clear();
                self.pen_points.clear();
                self.ds_vertex = None;
                self.editing_text = None;
                self.drag = super::Drag::None;
                self.layer_drag = None;
                self.editing_layer = None;
                self.set_tool(super::Tool::Select);
                self.saved_rev = self.doc.rev;
                // 印记转「已重载」(07-R 语义保持:同一事件,处置结果更新)
                if let Some(e) = &mut self.external_change {
                    e.adopted = true;
                }
                self.conflict_open = false;
                self.say(format!("已放弃本地编辑,采用磁盘版本(画板 {n})"));
            }
            Err(e) => self.toast_error(format!("重载磁盘版本失败:{e}")),
        }
    }

    /// 「以内存为准存回」:本地编辑写回磁盘(Ctrl+S 同一路径);
    /// 写成功后印记按已消化处理(磁盘 = 内存,冲突解除)。
    pub(crate) fn conflict_take_memory(&mut self) {
        let ok = self.save_project();
        if ok {
            if let Some(e) = &mut self.external_change {
                e.adopted = true;
            }
            self.conflict_open = false;
            self.say("已以内存版本写回磁盘(外部改动被覆盖)");
        }
    }
}

/// 对比窗口的动作(渲染后统一执行)。
enum ConflictAction {
    TakeDisk,
    TakeMemory,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;

    fn ext_fixture(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("vb-conflict-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.html"),
            "<html><body><section class=\"vb-artboard\"><p>a</p></section></body></html>",
        )
        .unwrap();
        dir
    }

    /// 三方摘要与差分对:缺失方如实标注;三对差分都有产出。
    #[test]
    fn summaries_and_pair_diffs_handle_missing_sides() {
        let disk = "a\nb\nc\n";
        let snap = "a\nx\nc\n";
        let mem = "a\nb2\nc\nY\n";
        let s = side_summaries(Some(disk), None, mem);
        assert_eq!(s[0].lines, Some(3));
        assert_eq!(s[1].lines, None, "快照缺失必须如实标注");
        assert_eq!(s[2].lines, Some(4));
        assert!(s[1].text().contains("不可读"));

        // 磁盘 ↔ 内存:删 b 加 b2 加 Y
        let rows = pair_diff(DiffPair::DiskMemory, Some(disk), None, mem);
        assert!(rows
            .iter()
            .any(|r| r.kind == DiffKind::Del && r.text == "b"));
        assert!(rows
            .iter()
            .any(|r| r.kind == DiffKind::Add && r.text == "b2"));
        // 快照缺失 → 占位行,不 panic
        let rows = pair_diff(DiffPair::DiskSnapshot, Some(disk), None, mem);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].text.contains("快照"));
        // 快照 ↔ 内存正常
        let rows = pair_diff(DiffPair::SnapshotMemory, Some(disk), Some(snap), mem);
        assert!(rows
            .iter()
            .any(|r| r.kind == DiffKind::Del && r.text == "x"));
    }

    /// 09-N 端到端:构造「外部改动 + 脏文档」→ 印记未采用 →
    /// 以磁盘为准重载 = 磁盘内容进内存且印记转已重载;
    /// 再构造一次 → 以内存为准存回 = 磁盘被覆盖且印记转已消化。
    #[test]
    fn conflict_actions_resolve_both_ways() {
        let _env = crate::ENV_LOCK.lock();
        let dir = ext_fixture("e2e");
        let mut app = app_fresh(Some(dir.clone()));
        // 本地未保存编辑(脏)
        app.exec(vb_doc::commands::Command::SetMetaTitle {
            new: "本地编辑".into(),
            old: None,
        });
        assert!(app.is_dirty());
        // 外部改盘 → 印记「未采用」
        std::fs::write(
            dir.join("index.html"),
            "<html><body><section class=\"vb-artboard\"><p>磁盘版</p></section></body></html>",
        )
        .unwrap();
        app.handle_external_event(vec![dir.join("index.html")]);
        assert!(!app.external_change.as_ref().unwrap().adopted);
        assert!(app.has_pending_conflict(), "未采用 + 有项目 = 待处置冲突");

        // ① 以磁盘为准:内存变成磁盘版,脏态解除
        app.conflict_take_disk();
        assert!(app.external_change.as_ref().unwrap().adopted);
        assert!(!app.is_dirty(), "重载后 = 磁盘基线");
        let mem = vb_doc::export::render_project(&app.doc);
        assert!(mem.files[0].1.contains("磁盘版"), "内存必须是磁盘内容");
        assert!(!app.has_pending_conflict(), "已重载不再是待处置冲突");

        // ② 以内存为准:本地先有编辑(脏)→ 再改盘 → 未采用印记 →
        // 内存编辑存回覆盖磁盘
        app.exec(vb_doc::commands::Command::SetMetaTitle {
            new: "内存胜出".into(),
            old: None,
        });
        std::fs::write(
            dir.join("index.html"),
            "<html><body><section class=\"vb-artboard\"><p>又改了</p></section></body></html>",
        )
        .unwrap();
        app.handle_external_event(vec![dir.join("index.html")]);
        assert!(!app.external_change.as_ref().unwrap().adopted);
        app.conflict_take_memory();
        let on_disk = std::fs::read_to_string(dir.join("index.html")).unwrap();
        assert!(on_disk.contains("内存胜出"), "磁盘必须被内存版本覆盖");
        assert!(app.external_change.as_ref().unwrap().adopted);
        assert!(!app.has_pending_conflict());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
