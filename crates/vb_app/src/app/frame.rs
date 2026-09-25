//! 每帧装配(eframe::App::ui):主题同步、UI 缩放、画布/工具条/面板坞
//! 与状态栏的逐帧编排。
//!
//! 06-1 自 `app.rs` 拆出(纯搬移,零行为变化);应用状态与生命周期在
//! `app.rs`,启动装配在 `assemble.rs`。

use vb_ui::theme;

use super::dock_layout;
use super::VellumApp;

impl eframe::App for VellumApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // 主题单一真相(B4):theme_dark 是唯一来源,变化时同步进
        // egui 偏好 —— 否则 tokens(ctx)(读 ctx.theme())在系统浅色
        // 模式下取到浅色令牌,深色界面对比度塌掉
        if self.theme_synced != Some(self.theme_dark) {
            ui.ctx().set_theme(if self.theme_dark {
                egui::ThemePreference::Dark
            } else {
                egui::ThemePreference::Light
            });
            self.theme_synced = Some(self.theme_dark);
        }
        // 主题逐帧应用(幂等;P2.7 支持 深/浅 切换)。H-1:带动效总开关
        // —— 关闭时 egui animation_time 归零,组件侧动画同步冻结。
        theme::apply_ex(ui.ctx(), self.theme_dark, 1.0, self.motion_enabled);
        // 04-3:UI 缩放因子。egui 的 pixels_per_point = zoom_factor × 系统 DPI,
        // 所以把缩放表达成 zoom_factor:随系统 DPI(150% 屏自动 1.5×),
        // 手调档位(视图 → 界面缩放)与系统缩放正交;egui-winit 在显示器
        // 变更时只改 native 侧,zoom_factor 保持用户选择。行高/字号全部
        // 从此处派生(见 vb_ui::theme::row_height),无第二套缩放。
        {
            let want = self.ui_scale;
            if (ui.ctx().zoom_factor() - want).abs() > f32::EPSILON {
                ui.ctx().set_zoom_factor(want);
            }
        }
        // FPS 统计(04-4-3:只在开发者统计可见时采样 —— 显示不造成
        // 隐性常驻计算,给阶段 6 idle 节流留路)
        if self.dev_stats {
            let dt = ui.ctx().input(|i| i.stable_dt);
            if dt > 0.0 {
                self.frame_times.push_back(dt);
                if self.frame_times.len() > 60 {
                    self.frame_times.pop_front();
                }
            }
        }
        // 06-3-2:idle 帧率实测钩子(VB_FPS_LOG=1;与 VB_SMOKE_COMMAND 同族的
        // 冒烟钩子,不进 UI 面):帧发生时按墙钟 ≥5s 汇一次 stderr ——
        // 持续满帧时每 5s 一条(帧数 ≈ 帧率×5),空闲休眠时完全静默
        // (无输入无动画无播放 = 无帧无代码;**静默即 idle 节流生效的证据**)。
        // tools/bench.ps1 -Boot 用它的首行做「冷启动到主页首帧」计时。
        if let Some(t0) = self.fps_log_at {
            self.fps_log_frames += 1;
            if self.fps_log_frames == 1 {
                eprintln!(
                    "[fps-log] project first-frame {} ms",
                    t0.elapsed().as_millis()
                );
            }
            let win = t0.elapsed().as_secs_f32();
            if win >= 5.0 {
                let n = self.fps_log_frames;
                self.fps_log_frames = 0;
                self.fps_log_at = Some(std::time::Instant::now());
                eprintln!(
                    "[fps-log] project 窗口 {win:.1}s 内 {n} 帧(均 {:.1} fps)",
                    n as f32 / win.max(0.001)
                );
            }
        }

        self.poll_watcher();
        // 09-J:插件逐帧 poll(泵请求/通知 + 崩溃检测;零插件零开销)
        self.plugin_poll();
        // 05-9(09-I):动画预览节拍(播放中按真实 dt 推进播放头;画布
        // 随后按 anim::apply_frame_state 呈现,与导出同一求值入口)
        self.tick_anim_preview(ui.ctx());
        // 07-A:自动保存节拍(有未保存改动才写 .vb-autosave/,不覆盖 index.html;
        // 响应式渲染下由节拍自身挂重绘请求保底,见 recover.rs)
        self.tick_autosave(ui.ctx());
        self.handle_shortcuts(ui.ctx());
        // 工作区布局写通(阶段 6 / 07-2):任何停靠/Tab/折叠变化在下一次存活帧落盘
        if self.workspace_dirty() {
            self.save_workspace();
        }
        // 缓存视口宽度(run_command 里做折叠判定用)
        self.last_viewport_width = ui.ctx().viewport_rect().width();
        self.top_menu(ui);
        // 控制面板(S1-c 02-2):菜单栏下的随工具上下文条;随 Tab 一起隐藏
        if !self.panels_hidden {
            self.control_bar(ui);
        }
        // Tab(02-6-5):隐藏所有面板 —— 右侧坞/状态栏/浮动工具条都不画,
        // 画布吃满窗口;再按 Tab 恢复。顶部菜单保留(可发现性)。
        // 工具箱停靠(阶段 6 / 07-1)。**装配次序 = 层级规矩**(07 §6 风险):
        // egui 的底/顶面板"先装者更靠外",故 底向工具栏必须装在**状态栏之后**,
        // 状态栏才能永远贴底;顶/左/右三向则在状态栏之前装配。
        if !self.panels_hidden && self.toolbar_dock != dock_layout::DockSide::Bottom {
            self.docked_toolbar(ui);
        }
        if !self.panels_hidden {
            self.right_panel(ui);
            // 04-2:九面板统一入口 —— 停靠次级坞 / 受控浮窗(默认停靠)
            self.show_secondary_panels(ui);
            self.status_bar(ui, frame);
            // 04-4:提示条(教学/操作提示独立层;`视图 → 提示` 可关)
            if self.hints {
                self.hint_bar(ui);
            }
            if self.toolbar_dock == dock_layout::DockSide::Bottom {
                self.docked_toolbar(ui);
            }
        }
        self.canvas(ui, frame);
        // U-4:工具长按同族弹层(独立 Area,最上层;无弹层时零开销)
        self.family_popup_ui(ui);
        // 04-5-1:打开/新建项目后自动「适合窗口」。fit 依赖画布矩形,
        // 首帧 canvas_rect 尚未就绪 → 挂到就绪后的第一帧执行。
        if self.fit_pending && self.canvas_rect.is_some() {
            self.fit_pending = false;
            self.fit_view();
            self.status = format!(
                "已适合窗口:{}%(打开项目自动适配)",
                (self.camera.zoom * 100.0) as i64
            );
        }

        // 对话框与浮窗(拆分至 dialogs.rs,窗口内容逐字未动)
        self.show_about_window(ui);
        // 04-4:开发者统计浮层(默认隐藏;FPS/帧时间/节点/显卡只在此可见)
        self.show_dev_stats(ui, frame);
        // 能力台账(副文档 09-3;帮助 → 能力台账)
        self.show_capabilities_window(ui);
        self.show_text_edit_window(ui);
        self.show_export_window(ui);
        self.show_command_palette(ui);
        // 阶段 2:「新建项目 / 从模板新建」对话框(02-4-1;确认后外壳开新窗口)
        self.show_new_project_window(ui);
        // 阶段 7:崩溃恢复对话框 + 只读差异视图(07-B)
        self.show_recover_window(ui);
        self.show_recover_diff_window(ui);
        // 阶段 7:历史跳转丢弃确认(07-D)与项目健康检查报告(07-E)
        self.show_history_jump_confirm(ui);
        self.show_health_window(ui);
        // 阶段 7b:最近外部改动信息窗(07-R;点状态栏印记开合)
        self.show_external_change_window(ui);
        // ── 阶段 5(05-4-A2:对话框族)──
        self.show_preferences_window(ui); // 09-L 首选项九分类
        self.show_keymap_window(ui); // 09-L 键位方案编辑器
        self.show_doc_settings_window(ui); // 09-M 文档设置
        self.show_conflict_window(ui); // 09-N 外部冲突三方对比
        self.show_font_window(ui); // 09-O 字体缺失专项
        self.show_workspace_window(ui); // X-7 新建工作区
                                        // 09-J:插件管理窗口 + 首次启用授权弹窗(05-10-3/6)
        self.show_plugins_window(ui);
        self.show_plugin_auth_window(ui);
        // toast 通知(02-6-6:错误/告警,右下角可堆叠;面板隐藏时也在)
        self.toasts.show(ui.ctx());
        // 03-4:浏览器校对面板(读回在画布渲染之后,纹理才是当帧的)
        // 冒烟/脚本钩子:VB_PROOFREAD_AUTOSTART=1 自动开始一次校对
        // (VB_PROOFREAD_ARTBOARD=<序号> 选画板,缺省 0;与 KILN_DUMP_RECTS
        // 等 env 钩子同款,不进 UI 面)
        if !self.proofread_autostart_done && std::env::var("VB_PROOFREAD_AUTOSTART").is_ok() {
            self.proofread_autostart_done = true;
            self.proofread_open = true;
            let idx = std::env::var("VB_PROOFREAD_ARTBOARD")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            // 画布矩形就绪后再开跑(首帧 canvas_rect 还是空)
            if self.canvas_rect.is_some() {
                self.proofread_begin(idx);
            }
        }
        self.tick_proofread(ui.ctx(), frame);
        self.show_proofread_window(ui.ctx());
        // 冒烟钩子(不进 UI 面):VB_SMOKE_COMMAND=<命令 id> 在装配完成后的
        // 第一帧派发一次(供脚本截图:如 view.developer_stats)。04-7 起支持
        // 「+」分隔的多条命令(如七面板全开的截图场景),按序逐条派发;
        // 未设置时零开销。
        if !self.smoke_cmd_done {
            self.smoke_cmd_done = true;
            if let Ok(cmds) = std::env::var("VB_SMOKE_COMMAND") {
                for cmd in cmds.split('+').map(str::trim).filter(|s| !s.is_empty()) {
                    if crate::shortcuts::is_implemented(cmd) {
                        self.run_command(cmd, false, false);
                    } else {
                        eprintln!("VB_SMOKE_COMMAND:未注册命令 {cmd}(已跳过)");
                    }
                }
            }
            // 05-4-A2 冒烟夹具:VB_PREFS_TAB=<0..8> 指定首选项对话框打开时
            // 的分类页(配合 VB_SMOKE_COMMAND=edit.preferences 截图;
            // 与 VB_PROOFREAD_ARTBOARD 同族,不进 UI 面)
            if self.prefs_open {
                if let Ok(tab) = std::env::var("VB_PREFS_TAB") {
                    if let Ok(n) = tab.trim().parse::<usize>() {
                        if n < 9 {
                            self.prefs_tab = n;
                        }
                    }
                }
            }
            // 09-J 冒烟夹具:VB_PLUGIN_LOAD / VB_PLUGIN_SMOKE(05-10-6
            // 冒烟链:装载 → 授权 → 启动 → 管理窗与插件面板可见)
            self.plugin_smoke_fixture();
        }
        // 03-3:画布出图状态机(隐藏 --canvas-shot;未启用时零开销直返)
        self.tick_canvas_shot(ui.ctx(), frame);
    }
}
