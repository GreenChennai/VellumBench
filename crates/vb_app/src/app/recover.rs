//! 自动保存节拍 + 崩溃恢复 GUI(阶段 7 / 07-A·07-B;副文档 07-1)。
//!
//! 纯文件层(快照信封、滚动、损坏回退、LCS diff)在 [`crate::autosave`];
//! 本模块是它的 GUI/会话投影 —— 要摸 `VellumApp` 的私有会话字段,
//! 所以作为 `app` 的子模块存在(与 `panel_dock` 同款分层)。
//!
//! 交互契约(副文档 07 §2.1 + ADR-VB-L08 Z1):
//! - 自动保存只在**有未保存改动**时写,落 `项目/.vb-autosave/`,
//!   绝不覆盖 `index.html`;间隔可配置(编辑 → 设置入口,默认 60s);
//! - 打开项目(含 --project 直开 / 主页打开 / 会话恢复)检测到快照残留
//!   → 弹「恢复快照 / 查看差异 / 丢弃 / 暂不处理」;
//! - 「查看差异」= 磁盘 index.html vs 快照序列化 vs 内存当前态 的只读对比
//!   (内存方不可比时如实标注);
//! - 「恢复」载入内存、清空撤销栈;「丢弃」删快照;「暂不处理」保留快照。

use crate::autosave::{self, DiffKind, DiffRow, DEFAULT_INTERVAL_SECS, INTERVAL_STEPS};

use super::VellumApp;

impl VellumApp {
    /// 自动保存节拍(每帧调用;未启用 / 无项目 / 不脏 / 出图模式 → 零开销直返)。
    ///
    /// 「有未保存改动时才写」:以 [`VellumApp::is_dirty`] 为闸;写成功刷新
    /// 状态栏印记,写失败 toast 一次并同样推后节拍(不刷屏)。
    ///
    /// **响应式渲染保底**:egui 无输入事件可以不产帧 —— 用户改完放着不动,
    /// 节拍就永远到不了点。故只要"脏 + 计时中",挂一个 ≤500ms 的重绘请求,
    /// 保证墙钟节拍(干净态零开销,不给阶段 6 的 idle 节流添负担)。
    pub(crate) fn tick_autosave(&mut self, ctx: &egui::Context) {
        // 门禁出图 / 脚本通道不做自动保存(确定性优先)
        if self.canvas_shot.is_some() {
            return;
        }
        if self.autosave_interval_secs == 0 {
            return;
        }
        let Some(dir) = self.project_dir.clone() else {
            return;
        };
        if !self.is_dirty() {
            self.autosave_last = None; // 干净态重置节拍(下次变脏重新计时)
            return;
        }
        let due = match self.autosave_last {
            None => {
                // 首次见脏:只起表,不立即写(打开即脏的项目先给用户 1 个间隔)
                self.autosave_last = Some(std::time::Instant::now());
                ctx.request_repaint_after(std::time::Duration::from_millis(
                    (self.autosave_interval_secs as u64 * 1000).min(500),
                ));
                return;
            }
            Some(t) => {
                t.elapsed() >= std::time::Duration::from_secs(self.autosave_interval_secs as u64)
            }
        };
        if !due {
            // 到点前:500ms 一帧的节拍(仅脏态; 到点即写,多花的帧可忽略)
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            return;
        }
        self.autosave_last = Some(std::time::Instant::now());
        match autosave::write_snapshot(&dir, &self.doc) {
            Ok(at) => {
                self.autosave_at = Some((at, std::time::Instant::now()));
                log::info!(
                    "自动保存:.vb-autosave/(间隔 {}s)",
                    self.autosave_interval_secs
                );
            }
            Err(e) => self.toast_warn(format!("自动保存失败:{e}")),
        }
    }

    /// 状态栏印记文案(`None` = 本会话还没自动保存过)。
    pub(crate) fn autosave_stamp_text(&self) -> Option<String> {
        let (_, at) = self.autosave_at?;
        let mins = at.elapsed().as_secs() / 60;
        Some(match mins {
            0 => "已自动保存(刚刚)".into(),
            m if m < 60 => format!("已自动保存 {m} 分钟前"),
            m => format!("已自动保存 {} 小时前", m / 60),
        })
    }

    /// 「设置 → 自动保存间隔」:档位循环并落 workspace.json(07-A 最小入口;
    /// 首选项九分类属阶段 5)。
    pub(crate) fn autosave_cycle(&mut self) {
        let cur = self.autosave_interval_secs;
        let pos = INTERVAL_STEPS
            .iter()
            .position(|&s| s == cur)
            .unwrap_or_else(|| {
                INTERVAL_STEPS
                    .iter()
                    .position(|&s| s >= DEFAULT_INTERVAL_SECS)
                    .unwrap_or(2)
            });
        let next = INTERVAL_STEPS[(pos + 1) % INTERVAL_STEPS.len()];
        self.autosave_interval_secs = next;
        self.save_workspace();
        self.say(match next {
            0 => "自动保存:关闭(请常按 Ctrl+S)".into(),
            s => format!("自动保存:每 {s} 秒(快照写入项目 .vb-autosave/,不覆盖 index.html)"),
        });
    }

    /// 崩溃恢复对话框(07-B;仅在 `recover` 有值时渲染)。
    ///
    /// `VB_RECOVER_AUTO=1` 冒烟钩子:检测到快照即自动恢复(强杀 e2e 用,
    /// 与 `VB_PROOFREAD_AUTOSTART` / `VB_SMOKE_COMMAND` 同款,不进 UI 面)。
    pub(crate) fn show_recover_window(&mut self, ui: &mut egui::Ui) {
        // 门禁出图模式不弹恢复(确定性;脚本通道不掺用户态对话框)
        if self.canvas_shot.is_some() || self.recover.is_none() {
            return;
        }
        // 冒烟钩子:自动走「恢复快照」路径
        if !self.recover_auto_done && std::env::var("VB_RECOVER_AUTO").is_ok() {
            self.recover_auto_done = true;
            self.recover_restore();
            return;
        }
        let Some(rec) = self.recover.clone() else {
            return;
        };
        let mut action = 0u8; // 1=恢复 2=差异 3=丢弃 4=暂不
        egui::Window::new("发现未保存的自动快照")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 80.0])
            .show(ui.ctx(), |ui| {
                let name = self.display_name();
                let when = crate::recent::relative_time(
                    rec.snapshot.saved_at_unix,
                    crate::recent::now_secs(),
                );
                ui.label(format!(
                    "「{name}」检测到上次异常退出留下的自动快照({when}写入)。"
                ));
                ui.colored_label(
                    // 提示级走主题 warn 令牌(07-I 口径,浅色自动加深保对比)
                    vb_ui::theme::tokens(ui.ctx()).warn,
                    "磁盘上的 index.html 未被快照覆盖;恢复只载入内存,何时写回由你决定。",
                );
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("恢复快照").clicked() {
                        action = 1;
                    }
                    if ui.button("查看差异").clicked() {
                        action = 2;
                    }
                    if ui.button("丢弃").clicked() {
                        action = 3;
                    }
                    if ui.button("暂不处理").clicked() {
                        action = 4;
                    }
                });
                ui.label(
                    egui::RichText::new("快照保留期间会随编辑继续滚动;选择后本窗口关闭。")
                        .size(12.0),
                );
            });
        match action {
            1 => self.recover_restore(),
            2 => self.recover_diff_open = true,
            3 => {
                if let Some(dir) = self.project_dir.clone() {
                    autosave::clear(&dir);
                }
                self.recover = None;
                self.say("已丢弃自动快照(继续使用磁盘版本)");
            }
            4 => {
                self.recover = None;
                self.say("快照已保留;可继续编辑,下次打开仍会提示");
            }
            _ => {}
        }
    }

    /// 「恢复快照」:快照 → 当前文档;撤销栈清空并记「从快照恢复」反馈;
    /// 快照清点(恢复完成后残留无意义,防下次打开误报)。
    ///
    /// 磁盘文件不动;`saved_rev` 刻意与 `doc.rev` 错开 → 文档按"脏"处理,
    /// 关闭确认 / 标题 `*` / 后续自动保存全部按未保存语义工作。
    pub(crate) fn recover_restore(&mut self) {
        let Some(rec) = self.recover.clone() else {
            return;
        };
        let Some(dir) = self.project_dir.clone() else {
            return;
        };
        match autosave::restore_doc(&dir, &rec.snapshot) {
            Ok(doc) => {
                self.doc = doc;
                // 撤销栈清空(新 arena,旧命令全部悬空);「从快照恢复」记入
                // 状态栏 + 日志(撤销条目需要 vb_doc 新命令变体,本批次不动
                // 核心 crate —— 状态语义等价:栈空,第一步就是当前快照态)
                self.undo = vb_doc::undo::UndoStack::new();
                self.selection.clear();
                self.isolate_stack.clear();
                self.pen_points.clear();
                self.ds_vertex = None;
                self.editing_text = None;
                self.drag = Drag::None;
                self.layer_drag = None;
                self.editing_layer = None;
                self.image_cache.clear();
                self.set_tool(Tool::Select);
                self.fit_pending = true;
                // 刻意脏:恢复态 ≠ 磁盘态(见函数注释)
                self.saved_rev = self.doc.rev.wrapping_sub(1);
                autosave::clear(&dir);
                self.recover = None;
                self.recover_diff_open = false;
                self.autosave_last = None;
                self.say("从快照恢复:磁盘文件未动,Ctrl+S 写回(撤销栈已清空)");
                log::info!("崩溃恢复:已载入快照 {}", rec.path.display());
            }
            Err(e) => self.toast_error(format!("恢复快照失败:{e}")),
        }
    }

    /// 「查看差异」:磁盘 index.html / 快照 / 内存当前态 的只读对比视图。
    ///
    /// 主对比面 =「快照 vs 磁盘」的 LCS 行 diff;内存方以行数与一致性摘要
    /// 呈现(刚打开时内存 = 磁盘导入,如实标注;磁盘缺失时快照内容直出)。
    pub(crate) fn show_recover_diff_window(&mut self, ui: &mut egui::Ui) {
        if !self.recover_diff_open || self.recover.is_none() {
            return;
        }
        let Some(rec) = self.recover.clone() else {
            return;
        };
        let mut open = true;
        egui::Window::new("快照差异(只读)")
            .open(&mut open)
            .collapsible(false)
            .default_size([760.0, 480.0])
            .show(ui.ctx(), |ui| {
                let disk = self
                    .project_dir
                    .as_ref()
                    .and_then(|d| std::fs::read_to_string(d.join("index.html")).ok());
                let snap_html = rec.snapshot.index_html().unwrap_or("").to_string();
                let mem = vb_doc::export::render_project(&self.doc);
                let mem_html = mem
                    .files
                    .iter()
                    .find(|(p, _)| p == "index.html")
                    .map(|(_, c)| c.clone())
                    .unwrap_or_default();

                // 三方可读性摘要(缺失如实标注,不装作有值)
                ui.horizontal_wrapped(|ui| {
                    match &disk {
                        Some(d) => {
                            ui.label(format!("磁盘 index.html:{} 行", d.lines().count()))
                        }
                        None => ui.colored_label(
                            // 提示级走主题 warn 令牌(07-I 口径,两主题可读)
                            vb_ui::theme::tokens(ui.ctx()).warn,
                            "磁盘 index.html:不可读(文件缺失?)",
                        ),
                    };
                    ui.separator();
                    ui.label(format!("快照:{} 行", snap_html.lines().count()));
                    ui.separator();
                    let mem_lines = mem_html.lines().count();
                    match &disk {
                        Some(d) => {
                            let same = mem_html == *d;
                            ui.label(format!(
                                "内存当前态:{mem_lines} 行({})",
                                if same { "与磁盘一致" } else { "与磁盘不同" }
                            ))
                        }
                        None => ui.label(format!(
                            "内存当前态:{mem_lines} 行(磁盘方缺失,无从对比)"
                        )),
                    };
                });
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let rows = match &disk {
                        Some(d) => autosave::line_diff(d, &snap_html),
                        None => std::iter::once(DiffRow {
                            kind: DiffKind::Same,
                            text: "(磁盘 index.html 不可读,无法做双方 diff;快照内容见下)"
                                .into(),
                        })
                        .chain(snap_html.lines().map(|l| DiffRow {
                            kind: DiffKind::Add,
                            text: l.to_string(),
                        }))
                        .collect::<Vec<_>>(),
                    };
                    let changed = rows.iter().filter(|r| r.kind != DiffKind::Same).count();
                    ui.label(format!(
                        "「磁盘 −」/「快照 +」统一视图;差异 {changed} 行。删除行=磁盘独有,新增行=快照独有。"
                    ));
                    ui.separator();
                    // 差异红绿是固定语义色(删=红/增=绿),走 vb_ui::theme::semantic
                    // 的差异令牌:深色沿用原设计值,浅色自动加深保对比,不再写字面量。
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
            });
        self.recover_diff_open = open && self.recover.is_some();
    }
}

// `Drag` / `Tool` 是 `app` 模块的私有类型,子模块直接引用即可。
use super::{Drag, Tool};

// ─────────────────────── 单测(节拍闸门 / 档位循环) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::dock_layout::WorkspaceConfig;

    /// 确定性构造(与 app.rs 门禁同款;VB_WORKSPACE 指临时文件)。
    fn app_fresh(project: Option<std::path::PathBuf>) -> VellumApp {
        let file = std::env::temp_dir().join(format!(
            "vb-recover-gate-{}-{}.json",
            std::process::id(),
            project.is_some() as u8
        ));
        unsafe {
            std::env::set_var("VB_WORKSPACE", &file);
        }
        let ctx = egui::Context::default();
        VellumApp::construct(
            &ctx,
            WorkspaceConfig::default(),
            [0, 1, 2, 3],
            None,
            vb_doc::model::Document::new_default(),
            project,
        )
    }

    /// 07-A 闸门:干净态不写;关档不写;出图模式不写。
    #[test]
    fn autosave_tick_is_gated() {
        let _env = crate::ENV_LOCK.lock();
        let proj = std::env::temp_dir().join(format!("vb-autosave-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&proj);
        std::fs::create_dir_all(&proj).unwrap();
        let mut app = app_fresh(Some(proj.clone()));
        // 干净态:即使到点也不写
        app.autosave_interval_secs = 1;
        app.autosave_last = Some(std::time::Instant::now() - std::time::Duration::from_secs(10));
        let ctx = egui::Context::default();
        app.tick_autosave(&ctx);
        assert!(
            !crate::autosave::autosave_dir(&proj)
                .join("doc.json")
                .exists(),
            "干净文档不得触发自动保存"
        );
        // 关档(0 秒)不写
        app.saved_rev = app.doc.rev.wrapping_sub(1); // 刻意脏
        app.autosave_interval_secs = 0;
        let ctx = egui::Context::default();
        app.tick_autosave(&ctx);
        assert!(!crate::autosave::autosave_dir(&proj)
            .join("doc.json")
            .exists());
        // 出图模式不写
        app.autosave_interval_secs = 60;
        app.canvas_shot = Some(crate::canvas_shot::CanvasShotCfg::new(
            proj.join("x.png"),
            "0".into(),
        ));
        let ctx = egui::Context::default();
        app.tick_autosave(&ctx);
        assert!(!crate::autosave::autosave_dir(&proj)
            .join("doc.json")
            .exists());
        // 脏 + 到点:写盘且印记出现
        app.canvas_shot = None;
        app.autosave_last = Some(std::time::Instant::now() - std::time::Duration::from_secs(120));
        let ctx = egui::Context::default();
        app.tick_autosave(&ctx);
        assert!(
            crate::autosave::autosave_dir(&proj)
                .join("doc.json")
                .exists(),
            "脏 + 到点必须写快照"
        );
        assert!(app.autosave_stamp_text().is_some(), "写后必须有状态印记");
        assert!(app.autosave_stamp_text().unwrap().starts_with("已自动保存"));
        crate::autosave::clear(&proj);
        let _ = std::fs::remove_dir_all(&proj);
    }

    /// 07-A 档位循环:关 ↔ 30 ↔ 60 ↔ 120 ↔ 300 循环,且每次都落 workspace。
    #[test]
    fn autosave_interval_cycles_and_persists() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.autosave_interval_secs = 60;
        app.autosave_cycle();
        assert_eq!(app.autosave_interval_secs, 120);
        app.autosave_cycle();
        assert_eq!(app.autosave_interval_secs, 300);
        app.autosave_cycle();
        assert_eq!(app.autosave_interval_secs, 0, "300 之后是关");
        app.autosave_cycle();
        assert_eq!(app.autosave_interval_secs, 30);
        // 非法当前值(手改 workspace)也能回到档位表
        app.autosave_interval_secs = 45;
        app.autosave_cycle();
        assert!(
            INTERVAL_STEPS.contains(&app.autosave_interval_secs),
            "循环后必须落在档位表内"
        );
    }
}
