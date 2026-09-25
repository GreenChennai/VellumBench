//! 命令派发 ·「视图/面板显隐/工具切换」段。
//!
//! 06-1 自 `app.rs` 的 `run_command` 巨型 match 按连续段拆出
//! (纯搬移,零行为变化;切分顺序 = 原臂序,见 `dispatch` 文件头)。

use vb_doc::commands::Command;

use crate::shortcuts;

use super::dock_layout;
use super::{align_panel, panel_dock, panels};
use super::{Tool, VellumApp};

impl VellumApp {
    /// 「视图/面板/工具」命令段(原臂序连续段,顺序保持)。
    pub(super) fn dispatch_view_cmd(&mut self, id: &str, _shift: bool, _alt: bool) -> bool {
        let mut matched = true;
        match id {
            // ── 视图(B2:菜单显示的加速键在此真正落地) ──
            "view.zoom_in" | "view.zoom_out" => {
                let f = if id == "view.zoom_in" { 1.25 } else { 0.8 };
                if let Some(r) = self.canvas_rect {
                    self.camera
                        .zoom_at(r.center().x as f64, r.center().y as f64, f);
                }
                self.status = format!("缩放 {}%", (self.camera.zoom * 100.0) as i64);
            }
            "view.fit" => {
                self.fit_view();
                self.status = format!("适合窗口 {}%", (self.camera.zoom * 100.0) as i64);
            }
            "view.actual_size" => {
                self.camera.zoom = 1.0;
                self.status = "实际大小 100%".into();
            }
            "view.outline" => {
                self.outline_mode = !self.outline_mode;
                self.status = if self.outline_mode {
                    "轮廓模式:开(Mod+Y)".into()
                } else {
                    "轮廓模式:关(Mod+Y)".into()
                };
            }
            "view.toggle_grid" => {
                self.grid_on = !self.grid_on;
                self.status = format!("网格:{}", if self.grid_on { "显示" } else { "隐藏" });
            }
            "view.toggle_theme" => {
                self.theme_dark = !self.theme_dark;
                // 阶段 2(02-3-5):主题广播,主页与所有窗口跟随
                if let Some(tx) = &self.shell_tx {
                    let _ = tx.send(crate::shell::ShellRequest::ThemeChanged(self.theme_dark));
                }
                self.status = if self.theme_dark {
                    "主题:深色"
                } else {
                    "主题:浅色"
                }
                .into();
            }
            "view.toggle_smart_guides" => {
                self.smart_guides_on = !self.smart_guides_on;
                self.smart_guides.clear();
                self.status = format!(
                    "智能参考线:{}",
                    if self.smart_guides_on { "开" } else { "关" }
                );
            }
            // 09-E(05-2):像素预览 —— 缩放 ≥ 阈值时画布对齐物理像素网格
            // 渲染并显示像素边界提示(design/06 §六「按 1:1 设备像素光栅显示」)
            "view.pixel_preview" => {
                self.pixel_preview = !self.pixel_preview;
                self.status = if self.pixel_preview {
                    format!(
                        "像素预览:开(缩放 ≥{}× 时对齐物理像素网格并显示边界)",
                        Self::PIXEL_PREVIEW_MIN_ZOOM
                    )
                } else {
                    "像素预览:关".into()
                };
            }
            "view.next_artboard" | "view.prev_artboard" => {
                // 画板循环导航:选中并视图居中(C3)
                if self.doc.artboards.is_empty() {
                    return true;
                }
                let cur = self
                    .selection
                    .first()
                    .and_then(|s| self.doc.find_by_sid(s))
                    .and_then(|id| self.doc.artboards.iter().position(|&a| a == id));
                let n = self.doc.artboards.len();
                let next = match cur {
                    Some(i) => {
                        if id == "view.next_artboard" {
                            (i + 1) % n
                        } else {
                            (i + n - 1) % n
                        }
                    }
                    None => 0,
                };
                let ab = self.doc.artboards[next];
                let sid = self.doc.nodes.get(ab).unwrap().sid.as_str().to_string();
                self.selection = vec![sid];
                self.run_command("view.zoom_to_selection", false, false);
            }
            "view.next_panel_tab" => {
                self.panel_tab = (self.panel_tab + 1) % panels::TAB_COUNT;
            }
            // ── S1-b 面板显隐(F7 / Tab;design/02 §四-面板显隐) ──
            "view.toggle_layers_panel" => {
                let forced = self.last_viewport_width < vb_ui::theme::space::COLLAPSE_BELOW;
                // 「图层可见」= 面板未被 Tab 隐藏、未折叠(窄窗强制折叠不算)且正处图层 Tab
                let layers_visible = !self.panels_hidden
                    && (forced || !self.dock_collapsed)
                    && self.panel_tab == panels::TAB_LAYERS;
                if layers_visible {
                    if !forced {
                        self.dock_collapsed = true;
                    }
                    self.say("图层面板:已折叠(F7 恢复)");
                } else {
                    self.panels_hidden = false;
                    if !forced {
                        self.dock_collapsed = false;
                    }
                    self.panel_tab = panels::TAB_LAYERS;
                    self.say("图层面板:显示(F7 折叠)");
                }
            }
            "view.toggle_all_panels" => {
                self.panels_hidden = !self.panels_hidden;
                self.say(if self.panels_hidden {
                    "已隐藏所有面板(Tab 恢复)"
                } else {
                    "已恢复所有面板(Tab 再隐藏)"
                });
            }
            "view.zoom_to_selection" => {
                self.zoom_to_selection();
            }
            "view.toggle_rulers" => {
                self.rulers_on = !self.rulers_on;
                self.status = format!("标尺:{}", if self.rulers_on { "显示" } else { "隐藏" });
            }
            "view.toggle_guides" => {
                self.guides_visible = !self.guides_visible;
                self.status = format!(
                    "参考线:{}",
                    if self.guides_visible {
                        "显示"
                    } else {
                        "隐藏"
                    }
                );
            }
            "view.lock_guides" => {
                self.guides_locked = !self.guides_locked;
                self.status = format!(
                    "参考线:{}",
                    if self.guides_locked {
                        "已锁定"
                    } else {
                        "未锁定"
                    }
                );
            }
            "view.guides_from_selection" => {
                let mut added = 0;
                for sid in &self.selection {
                    if let Some(nid) = self.doc.find_by_sid(sid) {
                        if let Some(_n) = self.doc.nodes.get(nid) {
                            let bb = vb_tools::abs_bbox_world(&self.doc, nid).unwrap_or_default();
                            for pos in [bb.x0, (bb.x0 + bb.x1) / 2.0, bb.x1] {
                                self.guides.push((false, pos));
                                added += 1;
                            }
                            for pos in [bb.y0, (bb.y0 + bb.y1) / 2.0, bb.y1] {
                                self.guides.push((true, pos));
                                added += 1;
                            }
                        }
                    }
                }
                let key = shortcuts::key_text_for("view.guides_from_selection")
                    .unwrap_or_else(|| "未绑定".into());
                self.status = format!("从选区生成 {added} 条参考线({key})");
            }
            // ── 工具箱(统一经 set_tool:清进行中的钢笔锚点/直接选择顶点) ──
            "tool.select" => self.set_tool(Tool::Select),
            "tool.rect" => self.set_tool(Tool::Rect),
            "tool.ellipse" => self.set_tool(Tool::Ellipse),
            "tool.line" => self.set_tool(Tool::Line),
            "tool.pen" => self.set_tool(Tool::Pen),
            "tool.direct_select" => self.set_tool(Tool::DirectSelect),
            "tool.zoom" => self.set_tool(Tool::Zoom),
            "tool.hand" => self.set_tool(Tool::Hand),
            "tool.text" => self.set_tool(Tool::Text),
            // ── 04 字符/段落面板与文字工具模式 ──
            // 04-2:九面板开/关均聚焦次级坞对应组(打开必须有可见反馈)
            "view.toggle_char_panel" => {
                self.char_panel_open = !self.char_panel_open;
                self.sec_focus(panel_dock::SecPanel::Char);
                self.say(if self.char_panel_open {
                    "字符面板:显示(Ctrl+T 关闭)"
                } else {
                    "字符面板:隐藏(Ctrl+T 显示)"
                });
            }
            "view.toggle_para_panel" => {
                self.para_panel_open = !self.para_panel_open;
                self.sec_focus(panel_dock::SecPanel::Para);
                self.say(if self.para_panel_open {
                    "段落面板:显示(Ctrl+Alt+T 关闭)"
                } else {
                    "段落面板:隐藏(Ctrl+Alt+T 显示)"
                });
            }
            // ── S4 外观/描边面板显隐(⇧F6 / ^F10;design/03 §5.9 / §5.7) ──
            "view.toggle_appearance_panel" => {
                self.appearance_panel_open = !self.appearance_panel_open;
                self.sec_focus(panel_dock::SecPanel::Appearance);
                self.say(if self.appearance_panel_open {
                    "外观面板:显示(⇧F6 关闭)"
                } else {
                    "外观面板:隐藏(⇧F6 显示)"
                });
            }
            "view.toggle_stroke_panel" => {
                self.stroke_panel_open = !self.stroke_panel_open;
                self.sec_focus(panel_dock::SecPanel::Stroke);
                self.say(if self.stroke_panel_open {
                    "描边面板:显示(^F10 关闭)"
                } else {
                    "描边面板:隐藏(^F10 显示)"
                });
            }
            // ── S4-b 渐变/透明度/颜色面板显隐(^F9 / ⇧^F10 / F6) ──
            "view.toggle_gradient_panel" => {
                self.gradient_panel_open = !self.gradient_panel_open;
                self.sec_focus(panel_dock::SecPanel::Gradient);
                self.say(if self.gradient_panel_open {
                    "渐变面板:显示(^F9 关闭)"
                } else {
                    "渐变面板:隐藏(^F9 显示)"
                });
            }
            "view.toggle_opacity_panel" => {
                self.opacity_panel_open = !self.opacity_panel_open;
                self.sec_focus(panel_dock::SecPanel::Opacity);
                self.say(if self.opacity_panel_open {
                    "透明度面板:显示(⇧^F10 关闭)"
                } else {
                    "透明度面板:隐藏(⇧^F10 显示)"
                });
            }
            "view.toggle_color_panel" => {
                self.color_panel_open = !self.color_panel_open;
                self.sec_focus(panel_dock::SecPanel::Color);
                self.say(if self.color_panel_open {
                    "颜色面板:显示(F6 关闭)"
                } else {
                    "颜色面板:隐藏(F6 显示)"
                });
            }
            // ── S4-b 颜色动作(D / X / Shift+X;design/03 §5.4)──
            "color.toggle_target" => self.color_toggle_target(),
            "color.swap_fill_stroke" => self.color_swap(),
            "color.default_fill_stroke" => self.color_default(),
            // ── 副文档 09-3:能力台账(帮助 → 能力台账)──
            "help.capabilities" => {
                self.capabilities_ui.toggle();
                self.say(if self.capabilities_ui.open {
                    "能力台账:显示(再点关闭)"
                } else {
                    "能力台账:隐藏"
                });
            }
            // ── 阶段 2:变换数值面板(副文档 03-2,⇧F8)──
            "view.toggle_transform_panel" => {
                self.transform_panel_open = !self.transform_panel_open;
                self.sec_focus(panel_dock::SecPanel::Transform);
                self.say(if self.transform_panel_open {
                    "变换面板:显示(⇧F8 关闭)"
                } else {
                    "变换面板:隐藏(⇧F8 显示)"
                });
            }
            "view.toggle_align_panel" => {
                self.align_panel_open = !self.align_panel_open;
                self.sec_focus(panel_dock::SecPanel::Align);
                self.say(if self.align_panel_open {
                    "对齐面板:显示(⇧F7 关闭)"
                } else {
                    "对齐面板:隐藏(⇧F7 显示)"
                });
            }
            // ── 04-4:开发者统计与提示条(默认隐藏/显示,状态入 workspace.json)──
            "view.developer_stats" => {
                self.dev_stats = !self.dev_stats;
                self.say(if self.dev_stats {
                    "开发者统计:显示(帧率/帧时间/节点/显卡只在此可见)"
                } else {
                    "开发者统计:隐藏"
                });
            }
            "view.toggle_hints" => {
                self.hints = !self.hints;
                self.say(if self.hints {
                    "提示条:显示"
                } else {
                    "提示条:隐藏"
                });
            }
            // ── 第四轮 H-1:动效总开关(持久化 + 主页广播)──
            "view.toggle_motion" => {
                self.motion_enabled = !self.motion_enabled;
                self.save_workspace();
                if let Some(tx) = &self.shell_tx {
                    let _ = tx.send(crate::shell::ShellRequest::MotionChanged(
                        self.motion_enabled,
                    ));
                }
                self.say(if self.motion_enabled {
                    "界面动效:开(对话框/面板淡入、悬停过渡)"
                } else {
                    "界面动效:关(所有过渡立即到位;视图菜单或首选项可再开)"
                });
            }
            // ── 04-3:UI 缩放档位(完整首选项九分类属阶段 5;最小入口挂视图菜单)──
            "view.ui_scale_up" => self.step_ui_scale(1),
            "view.ui_scale_down" => self.step_ui_scale(-1),
            "view.ui_scale_reset" => {
                self.ui_scale = 1.0;
                self.save_workspace();
                self.say("界面缩放 100%(跟随系统 DPI)");
            }
            // ── 04-6:显示未支持工具(design/06 §二;置灰展示,点击有响应)──
            "edit.toggle_unsupported_tools" => {
                self.show_all_tools = !self.show_all_tools;
                self.save_workspace();
                self.say(if self.show_all_tools {
                    "未支持工具:显示(置灰,点击见计划版本)"
                } else {
                    "未支持工具:隐藏(工具箱保持整洁)"
                });
            }
            // ── 阶段 7(07-A/07-D/07-E:数据安全批次)──
            // 07-A:自动保存间隔档位循环(关/30/60/120/300;落 workspace.json)
            "edit.autosave_interval" => self.autosave_cycle(),
            // 07-D:撤销历史面板(次级坞「变换」组)
            "view.toggle_history_panel" => {
                self.history_open = !self.history_open;
                self.sec_focus(panel_dock::SecPanel::History);
                self.say(if self.history_open {
                    "历史面板:显示(点击历史项可跳转;回退遇重做尾需确认)"
                } else {
                    "历史面板:隐藏"
                });
            }
            // 07-K:资产面板(次级坞「资产」组;assets/ 清单 + 引用关系)
            "view.toggle_assets_panel" => {
                self.assets_open = !self.assets_open;
                self.sec_focus(panel_dock::SecPanel::Assets);
                self.say(if self.assets_open {
                    "资产面板:显示(点击引用可定位图层;支持替换引用)"
                } else {
                    "资产面板:隐藏"
                });
            }
            // ── 05-9 动效时间轴(09-I;ADR-VB-L11)──
            // 时间轴面板(次级坞「时间轴」组;关键帧轨道 + 播放头)
            "view.toggle_timeline_panel" => {
                self.timeline_open = !self.timeline_open;
                self.sec_focus(panel_dock::SecPanel::Timeline);
                self.say(if self.timeline_open {
                    "时间轴面板:显示(选中对象 → 双击轨道加关键帧 → 播放预览)"
                } else {
                    "时间轴面板:隐藏(预览若在播放将继续)"
                });
            }
            // 播放 / 暂停(会话态;文档编辑走 anim.keyframe_* 命令)
            "anim.play_toggle" => self.anim_play_toggle(),
            // 停止并回零
            "anim.stop" => self.anim_stop(),
            // 循环开关
            "anim.loop_toggle" => self.anim_loop_toggle(),
            // 在播放头处加关键帧(值 = 对象静态值;走 SetNodeAnimation 可撤销)
            "anim.keyframe_add" => self.anim_keyframe_add(),
            // 删除时间轴选中的关键帧
            "anim.keyframe_delete" => self.anim_keyframe_delete(),
            // 清除对象动画(@keyframes 块 + animation 声明一并移除)
            "anim.clear" => self.anim_clear(),
            // ── 05-10 插件系统(09-J)──
            // 插件管理窗口(安装/授权/启停/日志/重启)
            "edit.plugins" => {
                self.plugins_mgr_open = true;
                self.say("插件管理:已打开(插件 = 外部进程,默认零权限,首次启用需授权)");
            }
            // 插件坞面板(次级坞「插件」组;Running 插件的注册面板)
            "view.toggle_plugins_panel" => {
                self.plugins_panel_open = !self.plugins_panel_open;
                self.sec_focus(panel_dock::SecPanel::Plugins);
                self.say(if self.plugins_panel_open {
                    "插件面板:显示(Running 插件的注册面板;按钮点击回发插件通知)"
                } else {
                    "插件面板:隐藏"
                });
            }
            // 07-E:项目健康检查(报告窗口;打开即重算;07-N 起含无障碍三查)
            "file.health_check" => {
                if self.project_dir.is_some() {
                    self.health_open = true;
                    self.health_report = None; // 置空 → 窗口打开时现算
                    self.say("项目健康检查:缺失资源 / 失效链接 / 冻结块 / 未使用资产 / 超长文件 / 无障碍");
                } else {
                    self.toast_warn("项目健康检查:当前文档没有项目目录(先保存或打开一个项目)");
                }
            }
            // ── 阶段 2:路径查找器扩展三运算(副文档 03-1-4)──
            "path.merge" => self.path_boolean(vb_tools::boolean::BooleanOp::Merge),
            "path.subtract_back" => self.path_boolean(vb_tools::boolean::BooleanOp::SubtractBack),
            "path.crop" => self.path_boolean(vb_tools::boolean::BooleanOp::Crop),
            // ── 阶段 2:对齐工具族(副文档 03-5)──
            "align.to_selection" => self.set_align_to(align_panel::AlignTo::Selection),
            "align.to_key_object" => self.set_align_to(align_panel::AlignTo::KeyObject),
            "align.to_artboard" => self.set_align_to(align_panel::AlignTo::Artboard),
            "object.distribute_hspace" => self.distribute_space(true),
            "object.distribute_vspace" => self.distribute_space(false),
            // ── 阶段 6:工具箱停靠(副文档 07-4-1)──
            "view.dock_toolbar_top" => {
                self.set_toolbar_dock(dock_layout::DockSide::Top);
            }
            "view.dock_toolbar_left" => {
                self.set_toolbar_dock(dock_layout::DockSide::Left);
            }
            "view.dock_toolbar_right" => {
                self.set_toolbar_dock(dock_layout::DockSide::Right);
            }
            "view.dock_toolbar_bottom" => {
                self.set_toolbar_dock(dock_layout::DockSide::Bottom);
            }
            "view.toolbar_columns_1" => self.set_toolbar_columns(1),
            "view.toolbar_columns_2" => self.set_toolbar_columns(2),
            "tool.text_cycle_mode" => {
                // Shift+T:点 ↔ 区域循环(路径文本 v1.5 冻结登记);选中
                // 文本对象时经 SetTextMode 命令一并转换(可撤销)
                self.text_mode_pending = match self.text_mode_pending {
                    vb_doc::model::TextMode::Point => vb_doc::model::TextMode::Area,
                    vb_doc::model::TextMode::Area => vb_doc::model::TextMode::Point,
                };
                let pending = format!("{:?}", self.text_mode_pending);
                let targets: Vec<String> = self
                    .selection
                    .iter()
                    .filter(|sid| {
                        self.doc
                            .find_by_sid(sid)
                            .and_then(|nid| self.doc.nodes.get(nid))
                            .is_some_and(|n| matches!(n.kind, vb_doc::model::NodeKind::Text { .. }))
                    })
                    .cloned()
                    .collect();
                if !targets.is_empty() {
                    let cmds: Vec<Command> = targets
                        .iter()
                        .map(|sid| Command::SetTextMode {
                            sid: sid.clone(),
                            new: self.text_mode_pending,
                            old: None,
                        })
                        .collect();
                    let n = cmds.len();
                    self.exec(Command::Compound { cmds });
                    self.say(format!(
                        "文字模式 → {pending}(待用 + {n} 个选中文本对象已转换)"
                    ));
                } else {
                    self.say(format!("文字模式 → {pending}(下次新建生效)"));
                }
            }
            _ => matched = false,
        }
        matched
    }
}
