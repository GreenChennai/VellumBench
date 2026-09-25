//! 打印 + 新建工作区(阶段 5 / 05-4-A2;台账 X-7)。
//!
//! **打印**:「文件 → 打印…」= 当前画板经 **Kiln PDF 导出链**
//! (`vb_kiln::export_artboard`,`Format::Pdf`,与导出对话框同一引擎,
//! 不新写 PDF 引擎)导出到临时目录,再交系统默认程序打开(Windows
//! `cmd /c start`;用户在 PDF 查看器里按打印)。导出失败给中文 toast,
//! 不静默。
//!
//! **新建工作区**:「窗口 → 工作区 → 新建工作区…」= 把当前面板/工具栏/
//! 次级坞布局存为命名预设(`workspace.json` 的 `workspace_presets`,
//! 与既有「工作区预设同步」机制合流:布局字段与内置三档同源,最后
//! 写入胜 + 原子替换);对话框内列出全部用户预设,可**切换 / 删除**;
//! 「窗口」菜单在静态项之后动态列出同名入口(见 `menus.rs`)。

use vb_kiln::Format;

use super::VellumApp;

// ─────────────────────────── 打印(纯函数 + 动作) ───────────────────────────

/// 打印用临时 PDF 文件名(`vellumbench-print-<标题>.pdf`;标题做文件名
/// 非法字符清洗,时间戳防同名覆盖)。
pub fn print_pdf_name(title: &str) -> String {
    let cleaned: String = title
        .trim()
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    let safe = if cleaned.is_empty() {
        "未命名".to_string()
    } else {
        cleaned
    };
    format!("vellumbench-print-{safe}-{}.pdf", crate::recent::now_secs())
}

/// 用系统默认程序打开文件(Windows `cmd /c start ""`;其它平台
/// `xdg-open`/`open`)。返回中文错误说明(不静默)。
pub fn open_with_default_app(path: &std::path::Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("文件不存在:{}", path.display()));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", ""])
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("调用系统默认程序失败:{e}"))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("调用系统默认程序失败:{e}"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("调用系统默认程序失败:{e}"))
    }
}

impl VellumApp {
    /// 「文件 → 打印…」:当前画板 → 临时 PDF(Kiln)→ 系统默认程序打开。
    pub(crate) fn print_current_artboard(&mut self) {
        let Some(ab) = self.active_artboard() else {
            self.toast_warn("打印:当前没有画板");
            return;
        };
        let name = self.doc.nodes.get(ab).unwrap().name.clone();
        // 资产解析基准:有项目用项目根;无项目退临时目录(纯矢量文档可打印)
        let base = self.project_dir.clone().unwrap_or_else(std::env::temp_dir);
        let out = std::env::temp_dir().join(print_pdf_name(&self.doc.meta.title));
        let req = vb_kiln::ExportRequest {
            format: Format::Pdf,
            scale: 1,
            transparent: false,
            ..Default::default()
        };
        match vb_kiln::export_artboard(&self.doc, ab, &req, Some(&base)) {
            Ok((bytes, report)) => match std::fs::write(&out, &bytes) {
                Ok(()) => match open_with_default_app(&out) {
                    Ok(()) => {
                        self.status = format!(
                            "打印:画板「{name}」已导出临时 PDF({} KB,{})并交系统打开({});在 PDF 程序中执行打印",
                            bytes.len() / 1024,
                            report.summary(),
                            out.display()
                        );
                    }
                    Err(e) => {
                        self.toast_error(format!("PDF 已生成({}),但打开失败:{e}", out.display()))
                    }
                },
                Err(e) => self.toast_error(format!("写临时 PDF 失败:{e}")),
            },
            Err(e) => self.toast_error(format!("Kiln PDF 导出失败:{e}")),
        }
    }
}

// ─────────────────────────── 新建工作区(对话框) ───────────────────────────

impl VellumApp {
    /// 「窗口 → 工作区 → 新建工作区…」窗口(X-7):保存当前布局为
    /// 命名预设 + 列表切换/删除。
    pub(crate) fn show_workspace_window(&mut self, ui: &mut egui::Ui) {
        if !self.workspace_dialog_open {
            return;
        }
        let mut open = true;
        let mut action: Option<(WorkspaceAction, String)> = None;
        egui::Window::new("工作区")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                // 保存当前布局
                ui.label("把当前布局(工具箱停靠 / 面板坞 / 次级面板摆放)存为预设:");
                ui.horizontal(|ui| {
                    let resp = ui.add_sized(
                        [220.0, vb_ui::theme::row_height(ui.ctx())],
                        egui::TextEdit::singleline(&mut self.workspace_dialog_name)
                            .hint_text("工作区名…"),
                    );
                    let can_save = !self.workspace_dialog_name.trim().is_empty();
                    let clicked = ui
                        .add_enabled(can_save, egui::Button::new("保存为预设"))
                        .clicked();
                    let enter = resp.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && can_save;
                    if clicked || enter {
                        let n = self.workspace_dialog_name.trim().to_string();
                        self.workspace_dialog_name.clear();
                        self.workspace_preset_save(&n);
                    }
                });
                ui.separator();
                // 内置三档(既有工作区命令,入口仍在「窗口」菜单)
                ui.weak("内置:基本功能 / 排版 / 导出(见「窗口」菜单);下面是自定义预设:");
                let presets = self.workspace_presets();
                if presets.is_empty() {
                    ui.weak("(暂无自定义工作区)");
                } else {
                    let names: Vec<String> = presets.iter().map(|p| p.name.clone()).collect();
                    for name in &names {
                        ui.horizontal(|ui| {
                            ui.label(format!("◦ {name}"));
                            let n = name.clone();
                            if ui.small_button("切换").clicked() {
                                action = Some((WorkspaceAction::Apply, n));
                            }
                            let n = name.clone();
                            if ui.small_button("删除").clicked() {
                                action = Some((WorkspaceAction::Delete, n));
                            }
                        });
                    }
                }
            });
        match action {
            Some((WorkspaceAction::Apply, n)) => self.workspace_preset_apply(&n),
            Some((WorkspaceAction::Delete, n)) => self.workspace_preset_delete(&n),
            None => {}
        }
        self.workspace_dialog_open = open;
    }
}

/// 工作区列表动作(渲染后统一执行)。
enum WorkspaceAction {
    Apply,
    Delete,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;

    /// 打印文件名:非法字符清洗、空标题兜底、含时间戳(防覆盖)。
    #[test]
    fn print_name_sanitizes_and_uniquifies() {
        let a = print_pdf_name("a<b>:c");
        assert!(a.starts_with("vellumbench-print-a-b--c-"), "{a}");
        assert!(a.ends_with(".pdf"));
        assert!(
            print_pdf_name("  . ").starts_with("vellumbench-print-未命名-"),
            "空标题兜底"
        );
        let b = print_pdf_name("x");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_ne!(print_pdf_name("x"), b, "时间戳防同名覆盖");
    }

    /// 工作区预设:保存 → 列表出现 → 应用换血布局 → 删除;
    /// 全程 VB_WORKSPACE 指临时文件(不落真实配置目录)。
    #[test]
    fn workspace_preset_save_apply_delete() {
        let _env = crate::ENV_LOCK.lock();
        let file = std::env::temp_dir().join(format!(
            "vb-ws-preset-app-{}-{}.json",
            std::process::id(),
            line!()
        ));
        unsafe {
            std::env::set_var("VB_WORKSPACE", &file);
        }
        let _ = std::fs::remove_file(&file);
        let mut app = app_fresh(None);
        // app_fresh 内部会把 VB_WORKSPACE 指回 gate 文件,构造完再指回本测试文件
        unsafe {
            std::env::set_var("VB_WORKSPACE", &file);
        }
        // 保存:当前布局(左停靠 + 双列)存为「测试工作区」
        app.toolbar_dock = crate::app::dock_layout::DockSide::Left;
        app.toolbar_columns = 2;
        app.workspace_preset_save("测试工作区");
        assert_eq!(app.workspace_presets().len(), 1);
        assert_eq!(app.workspace_presets()[0].name, "测试工作区");
        assert_eq!(
            app.workspace_presets()[0].layout.toolbar_dock,
            crate::app::dock_layout::DockSide::Left
        );
        assert!(file.exists(), "预设必须落盘");
        // 同名覆盖(不重复)
        app.workspace_preset_save("测试工作区");
        assert_eq!(app.workspace_presets().len(), 1);
        // 切换:改布局 → 应用预设 → 布局回到快照
        app.toolbar_dock = crate::app::dock_layout::DockSide::Top;
        app.toolbar_columns = 1;
        app.workspace_preset_apply("测试工作区");
        assert_eq!(app.toolbar_dock, crate::app::dock_layout::DockSide::Left);
        assert_eq!(app.toolbar_columns, 2, "应用预设必须还原列数");
        // 删除
        app.workspace_preset_delete("测试工作区");
        assert!(app.workspace_presets().is_empty());
        let (back, _) = crate::app::dock_layout::load_from(&file);
        assert!(back.workspace_presets.is_empty(), "删除必须落盘");
        let _ = std::fs::remove_file(&file);
        unsafe { std::env::remove_var("VB_WORKSPACE") };
    }

    /// 预设合流:预设保存/删除以**磁盘最新列表**为操作基线 ——
    /// 窗口 B(内存快照为空)保存自己的预设时,不得覆盖窗口 A 已落盘的预设。
    #[test]
    fn preset_ops_merge_from_disk_list() {
        let _env = crate::ENV_LOCK.lock();
        let file =
            std::env::temp_dir().join(format!("vb-ws-preset-merge-{}.json", std::process::id()));
        unsafe {
            std::env::set_var("VB_WORKSPACE", &file);
        }
        let _ = std::fs::remove_file(&file);
        let mut app = app_fresh(None);
        unsafe {
            std::env::set_var("VB_WORKSPACE", &file);
        }
        app.workspace_preset_save("窗口A的预设");
        assert_eq!(app.workspace_presets().len(), 1);
        // 模拟窗口 B:独立构造(内存快照为空),共享同一磁盘文件
        let mut b = app_fresh(None);
        unsafe {
            std::env::set_var("VB_WORKSPACE", &file);
        }
        assert!(b.workspace_presets().is_empty(), "B 的内存快照初始为空");
        b.workspace_preset_save("窗口B的预设");
        assert_eq!(
            b.workspace_presets().len(),
            2,
            "预设保存以磁盘列表为基:A 的预设必须被保住"
        );
        // A 窗口整存(save_workspace)时同样从磁盘回读合流,保住 B 的预设
        app.toolbar_dock = crate::app::dock_layout::DockSide::Right;
        app.save_workspace();
        assert_eq!(app.workspace_presets().len(), 2, "两窗预设都在");
        let names: Vec<&str> = app
            .workspace_presets()
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert!(names.contains(&"窗口A的预设") && names.contains(&"窗口B的预设"));
        let _ = std::fs::remove_file(&file);
        unsafe { std::env::remove_var("VB_WORKSPACE") };
    }
}
