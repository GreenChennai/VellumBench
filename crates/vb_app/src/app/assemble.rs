//! 应用构造与启动装配:窗口构建、项目打开/新建对话框装配、`construct`
//! 状态初始化(生命周期入口)。
//!
//! 06-1 自 `app.rs` 按「生命周期 + 装配」拆出(纯搬移,零行为变化):
//! 结构体定义与每帧主循环仍在 `app.rs` / `frame.rs`;测试夹具 `app_fresh`
//! 也在此(`pub(crate)`,供兄弟模块的派发/外部监听回归测试复用)。

use std::path::PathBuf;

use vb_doc::model::Document;
use vb_doc::undo::UndoStack;
use vb_tools::Camera;
use vb_ui::fonts as vb_fonts;

// 06-1 拆分后的私有转发:自由函数在 `external`,本文件与兄弟子模块
// (commands / canvas_input)仍经 `super::` 路径调用,调用点不动。
use super::external::{import_with_layout, start_watcher};

use super::{
    align_panel, conflict_dialog, font_dialog, keymap_dialog, panel_dock, transform_panel,
};
use super::{dock_layout, panels, VellumApp};
use super::{Drag, Tool};

impl VellumApp {
    pub fn new(cc: &eframe::CreationContext<'_>, project: Option<PathBuf>) -> Self {
        Self::build(&cc.egui_ctx, project)
    }

    /// 阶段 2 新入口:子视口没有 `CreationContext`,外壳直接传全局 `Context`。
    /// (字体安装由外壳统一做一次;这里不再重复装。)
    pub fn build(ctx: &egui::Context, project: Option<PathBuf>) -> Self {
        let fonts_report = vb_fonts::install(ctx);
        log::info!("字体安装: {}", fonts_report.summary());

        // 工作区布局(阶段 6 / 07-2):启动还原;损坏/版本不符 → 回退默认 + 告警
        let (ws, ws_warn) = dock_layout::load();
        let ws_panel_order: [usize; 4] = {
            let mut a = [0, 1, 2, 3];
            for (i, v) in ws.panel_order.iter().take(4).enumerate() {
                a[i] = *v;
            }
            a
        };
        if let Some(w) = &ws_warn {
            log::warn!("{w}");
        }

        let (doc, project_dir) = match &project {
            Some(p) => match import_with_layout(p) {
                Ok(r) => {
                    println!(
                        "已打开:{}(画板 {})",
                        r.project_dir.display(),
                        r.doc.artboards.len()
                    );
                    (r.doc, Some(r.project_dir))
                }
                Err(e) => {
                    eprintln!("打开失败:{e};使用新建文档");
                    (Document::new_default(), None)
                }
            },
            None => (Document::new_default(), None),
        };

        Self::construct(ctx, ws, ws_panel_order, ws_warn, doc, project_dir)
    }

    /// 阶段 2 外壳入口:打开失败返回 `Err`(主页/toast 显式告知),**不静默回退**。
    pub fn try_open_project(ctx: &egui::Context, dir: &std::path::Path) -> Result<Self, String> {
        if !dir.is_dir() {
            return Err(format!("路径不存在:{}", dir.display()));
        }
        let r = import_with_layout(dir).map_err(|e| e.to_string())?;
        let (ws, ws_warn) = dock_layout::load();
        let ws_panel_order: [usize; 4] = {
            let mut a = [0, 1, 2, 3];
            for (i, v) in ws.panel_order.iter().take(4).enumerate() {
                a[i] = *v;
            }
            a
        };
        let app = Self::construct(ctx, ws, ws_panel_order, ws_warn, r.doc, Some(r.project_dir));
        Ok(app)
    }

    /// 共同构造体(路径收敛:build / try_open_project 都走这里)。
    pub(super) fn construct(
        _ctx: &egui::Context,
        ws: dock_layout::WorkspaceConfig,
        ws_panel_order: [usize; 4],
        ws_warn: Option<String>,
        doc: Document,
        project_dir: Option<PathBuf>,
    ) -> Self {
        // 04-5-1:打开项目 → 挂起「适合窗口」(画布矩形就绪后的第一帧落)
        let fit_pending = project_dir.is_some();
        let mut app = Self {
            doc,
            undo: UndoStack::new(),
            camera: Camera::default(),
            tool: Tool::Select,
            selection: Vec::new(),
            project_dir,
            drag: Drag::None,
            space_down: false,
            cursor_world: (0.0, 0.0),
            grid_on: true,
            smart_guides_on: true,
            outline_mode: false,
            image_cache: std::collections::HashMap::new(),
            drag_edited: false,
            theme_synced: None,
            theme_dark: true,
            // 面板坞状态由工作区配置还原(阶段 6 / 07-2-1)
            panel_tab: ws.panel_tab.min(panels::TAB_COUNT - 1),
            dock_collapsed: ws.dock_collapsed,
            panel_order: ws_panel_order,
            panels_hidden: ws.panels_hidden,
            num_commit_open: false,
            prop_groups_open: [true; 7],
            layer_search: String::new(),
            layer_expanded: std::collections::HashSet::new(),
            layer_drag: None,
            editing_layer: None,
            arrange_gap: 80.0,
            toasts: vb_ui::toast::ToastHost::default(),
            last_viewport_width: 1280.0,
            smart_guides: Vec::new(),
            editing_text: None,
            char_panel_open: false,
            para_panel_open: false,
            appearance_panel_open: false,
            stroke_panel_open: false,
            appearance_sel: None,
            gradient_panel_open: false,
            gradient_sel: None,
            gradient_annot: None,
            opacity_panel_open: false,
            color_panel_open: false,
            color_tab: 0,
            color_target_stroke: false,
            last_effect: None,
            toolbar_dock: ws.toolbar_dock,
            toolbar_columns: ws.toolbar_columns,
            toolbar_dragging: false,
            toolbar_dock_preview: None,
            // ── 第四轮 U-2/U-4/H-1 ──
            family_popup: None,
            family_popup_arm_release: false,
            dock_width: ws.dock_width,
            sec_dock_width: ws.sec_dock_width,
            sec_dock_collapsed: ws.sec_dock_collapsed,
            motion_enabled: ws.motion_enabled,
            workspace_saved: ws.clone(),
            transform_panel_open: false,
            transform_ref: transform_panel::RefPoint::MC,
            transform_lock_ratio: false,
            transform_scale_stroke: false,
            transform_snap_pixel: false,
            last_transform: None,
            align_panel_open: false,
            align_to: align_panel::AlignTo::default(),
            capabilities_ui: crate::capabilities::CapabilityUi::default(),
            text_mode_pending: vb_doc::model::TextMode::Point,
            text_default: vb_doc::model::SegStyle {
                font_size: Some(24.0),
                color: Some("#1a1a1a".into()), // vb-token-ok: 新建文本默认字色(文档内容,非 UI 皮肤)
                ..vb_doc::model::SegStyle::default()
            },
            text_discard_arm: None,
            status: match &ws_warn {
                Some(w) => format!("{w} — 已使用默认布局"),
                None => "就绪 — V 选择 · A 直接选择 · M 矩形 · Alt+拖动 复制 · Shift 约束 · Space 平移 · Ctrl+0 适合".into(),
            },
            last_move_delta: None,
            canvas_rect: None,
            gpu: None,
            frame_times: std::collections::VecDeque::new(),
            // 06-3:VB_FPS_LOG=1 冒烟钩子(idle 帧率取证 / bench.ps1 -Boot 冷启动
            // 计时用;未设置时零开销,不进 UI 面)
            fps_log_at: std::env::var("VB_FPS_LOG").is_ok().then(std::time::Instant::now),
            fps_log_frames: 0,
            show_about: false,
            show_export: false,
            export_format: 0,
            export_scale: 2,
            export_transparent: false,
            saved_rev: 0,
            watcher_rx: None,
            last_self_write: None,
            clipboard: Vec::new(),
            paste_offset: 0,
            palette_open: false,
            palette_query: String::new(),
            palette_sel: 0,
            rulers_on: true,
            guides_visible: true,
            guides_locked: false,
            guides: Vec::new(),
            isolate_stack: Vec::new(),
            pen_points: Vec::new(),
            ds_vertex: None,
            // 阶段 2:外壳协作字段(默认无外壳;由 ShellApp 装配后注入)
            shell_tx: None,
            viewport_id: egui::ViewportId::ROOT,
            new_dialog: None,
            canvas_shot: None,
            proofread_open: false,
            proofread_target: 0,
            proofread_mode: 0,
            proofread_slider: 0.5,
            proofread_show_heat: true,
            proofread: None,
            proofread_autostart_done: false,
            // 04-2:次级面板坞状态由工作区配置还原(越界自动夹回)
            sec: panel_dock::SecDockState::from_config(&ws),
            // 04-4:调试默认隐藏、提示默认显示(均入 workspace.json)
            dev_stats: ws.dev_stats,
            hints: ws.hints,
            // 04-3:UI 缩放因子:工作区配置优先,环境变量 VB_UI_SCALE 覆盖
            // (多 DPI 截图基线夹具用;normalize 已把配置值夹进 0.5..=3.0)
            ui_scale: std::env::var("VB_UI_SCALE")
                .ok()
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|v| (0.5..=3.0).contains(v) && *v > 0.0)
                .unwrap_or(ws.ui_scale),
            // 04-6:未支持工具默认隐藏(design/06 §二)
            show_all_tools: ws.show_all_tools,
            // 04-5:打开项目 → 画布矩形就绪后自动「适合窗口」
            fit_pending,
            smoke_cmd_done: false,
            // ── 阶段 7(07-A ~ 07-E)──
            autosave_interval_secs: ws.autosave_interval_secs,
            autosave_last: None,
            autosave_at: None,
            recover: None,
            recover_diff_open: false,
            recover_auto_done: false,
            history_open: false,
            jump_confirm: None,
            health_open: false,
            health_report: None,
            // ── 阶段 7b(07-K / 07-R)──
            assets_open: false,
            asset_thumbs: std::collections::HashMap::new(),
            asset_replace: None,
            assets_cache: None,
            external_change: None,
            external_info_open: false,
            // ── 阶段 5(05-2:交互完整性六件)──
            xf_center: None,
            pencil_fidelity: ws.pencil_fidelity,
            pixel_preview: false,
            measure_result: None,
            measure_anchor: None,
            // ── 阶段 5(05-4-A2:对话框族)──
            prefs_open: false,
            prefs_tab: 0,
            keymap_open: false,
            keymap_editor: keymap_dialog::KeymapEditor::default(),
            keymap: crate::keymap::KeymapStore::default(),
            keymap_live: crate::keymap::default_live(),
            doc_settings_open: false,
            doc_settings_state: None,
            conflict_open: false,
            conflict_diff_pair: conflict_dialog::DiffPair::DiskMemory,
            font_dialog: None,
            workspace_dialog_open: false,
            workspace_dialog_name: String::new(),
            // 首选项九分类(参考线与网格 / 数据 / 画板)
            grid_size: ws.grid_size,
            autosave_keep: ws.autosave_keep,
            artboard_preset: ws.artboard_preset,
            // ── 05-5:断点与伪类(会话态,不持久化)──
            active_breakpoint: None,
            style_state: 0,
            // ── 05-9:动效时间轴(09-I;播放会话态,不持久化)──
            timeline_open: false,
            anim_playing: false,
            anim_time: 0.0,
            anim_loop: true,
            anim_last_clock: None,
            anim_cache: None,
            anim_sel: None,
            anim_drag_open: false,
            // ── 05-10:插件系统(09-J;授权持久化在 plugins.json)──
            plugin_host: vb_plugin::host::PluginHost::default(),
            plugins_mgr_open: false,
            plugins_panel_open: false,
            plugin_auth: None,
            plugin_logs_open: std::collections::HashSet::new(),
            plugin_panel_sel: 0,
            plugin_input_buf: std::collections::HashMap::new(),
        };
        // 首选项九分类:主题 / 默认显隐 / 新建文本默认样式由工作区配置还原
        // (外观页收编既有散落项的持久化;无壳独立构造同样生效)
        app.theme_dark = ws.theme_dark;
        app.rulers_on = ws.rulers_default;
        app.grid_on = ws.grid_default;
        app.guides_visible = ws.guides_default;
        app.smart_guides_on = ws.smart_guides_default;
        app.text_default = vb_doc::model::SegStyle {
            font_size: Some(ws.text_default_size),
            color: Some(ws.text_default_color.clone()),
            font_family: if ws.text_default_family.is_empty() {
                None
            } else {
                Some(ws.text_default_family.clone())
            },
            ..vb_doc::model::SegStyle::default()
        };
        // 05-7 i18n:界面语言由工作区配置还原(首选项「常规」页可切换)
        crate::i18n::set_lang(crate::i18n::Lang::from_code(&ws.ui_lang));
        // 09-L:用户键位方案(keymap.json)叠加在静态绑定表之上;
        // 损坏/版本不符回退默认 + 告警(与 recent.json 同范式),不静默。
        {
            let (store, warn) = crate::keymap::load();
            if let Some(w) = &warn {
                log::warn!("{w}");
                app.toast_warn(w.clone());
            }
            let (live, warns) = crate::keymap::build_live(&store);
            for w in warns {
                log::warn!("{w}");
                app.toast_warn(w);
            }
            app.keymap = store;
            app.keymap_live = live;
        }
        // 打开即"已保存基线"(rev 对齐;修复旧实现打开项目后误标脏的问题)
        app.saved_rev = app.doc.rev;
        // 文件监听 per-window(02-5-7:多窗口按项目隔离,互不串扰)
        app.watcher_rx = start_watcher(app.project_dir.as_deref());
        // 07-B 崩溃恢复检测:打开项目的全部路径(--project 直开 / 主页打开 /
        // 会话恢复 / 就地打开兜底)都汇经 `construct` —— 在这里检出快照残留,
        // 对话框渲染在 `recover.rs`(出图/脚本模式在渲染层豁免)。
        if let Some(dir) = app.project_dir.clone() {
            if let Some((snapshot, path)) = crate::autosave::read_newest(&dir) {
                log::info!("检出自动快照残留:{}", path.display());
                app.recover = Some(crate::autosave::RecoverPrompt { snapshot, path });
            }
        }
        // 05-4-A2 冒烟夹具:VB_CONFLICT_DEMO=1 构造「磁盘被外部修改 + 本地有
        // 未保存编辑」的未采用印记状态(供脚本截图三方对比对话框;与本文件
        // 其余环境钩子同族,不进 UI 面;门禁出图模式不构造)。
        if app.canvas_shot.is_none()
            && std::env::var("VB_CONFLICT_DEMO").is_ok()
            && app.project_dir.is_some()
        {
            let _ = app.undo.push(
                &mut app.doc,
                vb_doc::commands::Command::SetMetaTitle {
                    new: "本地未保存的标题编辑".into(),
                    old: None,
                },
            );
            app.external_change = Some(super::ExternalChange {
                at_unix: crate::recent::now_secs(),
                at: std::time::Instant::now(),
                files: vec!["index.html".into()],
                adopted: false,
            });
            log::info!("VB_CONFLICT_DEMO:已构造未采用外部改动印记(冒烟夹具)");
        }
        // 09-O:打开文档时检测字体缺失(出图/脚本模式不弹;对话框在 font_dialog)。
        if app.canvas_shot.is_none() {
            let missing = font_dialog::scan_missing_fonts(&app.doc);
            if !missing.is_empty() {
                log::warn!(
                    "字体缺失 {} 项:{}",
                    missing.len(),
                    missing
                        .iter()
                        .map(|m| m.family.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                app.font_dialog = Some(font_dialog::FontDialogState::new(missing));
            }
        }
        // 09-J:重载已登记插件(plugins.json 的 installed 清单;损坏条目
        // 只告警不阻断,管理窗口可见)。授权态随 plugins.json 还原。
        app.plugin_load_installed();
        app
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::app::dock_layout::WorkspaceConfig;

    /// 确定性构造:绕过机器上的真实 workspace.json(单测与运行环境解耦)。
    /// `VB_WORKSPACE` 指到临时文件,避免 `save_workspace` 写到真实配置目录。
    /// 确定性构造:绕过机器上的真实 workspace.json(单测与运行环境解耦)。
    /// `VB_WORKSPACE` 指到临时文件,避免 `save_workspace` 写到真实配置目录。
    pub(crate) fn app_fresh(project: Option<PathBuf>) -> VellumApp {
        let file = std::env::temp_dir().join(format!(
            "vb-app-gate-{}-{}.json",
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
            Document::new_default(),
            project,
        )
    }
    /// 04-5-2(P1-⑤):默认工具必须是「选择」—— 打开/新建都不会以
    /// 矩形工具起步,点画布不会误建形状。
    #[test]
    fn default_tool_is_select() {
        let _env = crate::ENV_LOCK.lock();
        let app = app_fresh(None);
        assert_eq!(app.tool, Tool::Select, "默认工具必须是选择(V)");
    }
    /// 04-5-1:构造时带项目目录 → 挂起「适合窗口」,画布矩形就绪后落。
    #[test]
    fn project_open_defers_fit() {
        let _env = crate::ENV_LOCK.lock();
        let app = app_fresh(None);
        assert!(!app.fit_pending, "无项目不挂起 fit");
        let app = app_fresh(Some(PathBuf::from("examples/landing")));
        assert!(app.fit_pending, "打开项目必须挂起自动适合窗口");
    }
    /// 04-3:UI 缩放档位 —— ±档位按表步进并持久化,复位回 1.0。
    #[test]
    fn ui_scale_steps_and_persist() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        assert_eq!(app.ui_scale, 1.0, "默认缩放 = 跟随系统");
        app.run_command("view.ui_scale_up", false, false);
        assert!((app.ui_scale - 1.1).abs() < 1e-3, "升一档 → 110%");
        app.run_command("view.ui_scale_down", false, false);
        app.run_command("view.ui_scale_down", false, false);
        assert!((app.ui_scale - 0.9).abs() < 1e-3, "降两档 → 90%");
        app.run_command("view.ui_scale_reset", false, false);
        assert!((app.ui_scale - 1.0).abs() < 1e-3, "复位 → 100%");
        // 持久化:工作区快照与新值一致(脏检查不误报)
        assert!((app.workspace_saved.ui_scale - app.ui_scale).abs() < 1e-3);
    }
    /// 04-6:「显示未支持工具」开关持久化(工具箱数据面测试在 toolbar.rs)。
    #[test]
    fn show_unsupported_tools_toggles_and_persists() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        assert!(!app.show_all_tools, "默认隐藏未支持工具(design/06 §二)");
        app.run_command("edit.toggle_unsupported_tools", false, false);
        assert!(app.show_all_tools);
        assert!(
            app.workspace_saved.show_all_tools,
            "开关必须落 workspace.json"
        );
    }
}
