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
                self.status = vb_session::i18n::t_args(
                    "ui-app-canvas-input-view-001",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            ((self.camera.zoom * 100.0) as i64).to_string(),
                        ),
                    )],
                );
            }
            "view.fit" => {
                self.fit_view();
                self.status = vb_session::i18n::t_args(
                    "ui-app-dispatch-view-001",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            ((self.camera.zoom * 100.0) as i64).to_string(),
                        ),
                    )],
                );
            }
            "view.actual_size" => {
                self.camera.zoom = 1.0;
                self.status = vb_session::i18n::t("ui-app-dispatch-view-002");
            }
            "view.outline" => {
                self.outline_mode = !self.outline_mode;
                self.status = if self.outline_mode {
                    vb_session::i18n::t("ui-app-dispatch-view-003")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-004")
                };
            }
            "view.toggle_grid" => {
                self.grid_on = !self.grid_on;
                self.status = vb_session::i18n::t_args(
                    "ui-app-dispatch-view-005",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            (if self.grid_on {
                                vb_session::i18n::t("ui-common-show")
                            } else {
                                vb_session::i18n::t("ui-common-hide")
                            })
                            .to_string(),
                        ),
                    )],
                );
            }
            "view.toggle_theme" => {
                self.theme_dark = !self.theme_dark;
                // 阶段 2(02-3-5):主题广播,主页与所有窗口跟随
                if let Some(tx) = &self.shell_tx {
                    let _ = tx.send(crate::shell::ShellRequest::ThemeChanged(self.theme_dark));
                }
                self.status = if self.theme_dark {
                    vb_session::i18n::t("ui-app-dispatch-view-006")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-007")
                };
            }
            "view.toggle_smart_guides" => {
                self.smart_guides_on = !self.smart_guides_on;
                self.smart_guides.clear();
                self.status = vb_session::i18n::t_args(
                    "ui-app-dispatch-view-008",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            (if self.smart_guides_on {
                                vb_session::i18n::t("ui-common-on")
                            } else {
                                vb_session::i18n::t("ui-common-off")
                            })
                            .to_string(),
                        ),
                    )],
                );
            }
            // 09-E(05-2):像素预览 —— 缩放 ≥ 阈值时画布对齐物理像素网格
            // 渲染并显示像素边界提示(design/06 §六「按 1:1 设备像素光栅显示」)
            "view.pixel_preview" => {
                self.pixel_preview = !self.pixel_preview;
                self.status = if self.pixel_preview {
                    vb_session::i18n::t_args(
                        "ui-app-dispatch-view-009",
                        &[(
                            "a1",
                            vb_session::i18n::FluentValue::from(
                                (Self::PIXEL_PREVIEW_MIN_ZOOM).to_string(),
                            ),
                        )],
                    )
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-010")
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
                self.selection = vec![sid].into();
                self.run_command("view.zoom_to_selection", false, false);
            }
            "view.next_panel_tab" => {
                self.panel_tab = (self.panel_tab + 1) % panels::TAB_COUNT;
            }
            // ── S5 清单 ②(§8.10.2):面板区焦点循环 ──
            // 区状态机与循环顺序见 [`crate::app::next_focus_zone`]。
            // 区跳转 = 可见性抬升:反面板隐藏、反用户折叠(窄窗强制折叠
            // 不受影响,那是布局函数不是偏好)、保持用户排的 panel_order。
            "view.focus_next_panel" | "view.focus_prev_panel" => {
                let forward = id == "view.focus_next_panel";
                let sec_available = !self.panels_hidden
                    && panel_dock::SecPanel::ALL
                        .iter()
                        .copied()
                        .any(|p| self.sec_is_open(p));
                self.focus_zone = super::next_focus_zone(self.focus_zone, sec_available, forward);
                match self.focus_zone {
                    super::FocusZone::RightDock => {
                        self.panels_hidden = false;
                        self.dock_collapsed = false;
                        self.status = vb_session::i18n::t_args(
                            "ui-app-dispatch-view-063",
                            &[(
                                "a1",
                                vb_session::i18n::FluentValue::from(
                                    (panels::TAB_LABELS[self.panel_tab]).to_string(),
                                ),
                            )],
                        );
                    }
                    super::FocusZone::SecDock => {
                        self.panels_hidden = false;
                        self.sec_dock_collapsed = false;
                        // 区可用性在循环函数已保证;此处再兜一次组内选中
                        // (面板全浮窗/组切换后 effective 可能落空)。
                        if self.sec.active.is_none_or(|p| !self.sec_is_open(p)) {
                            if let Some(p) = panel_dock::SecPanel::ALL
                                .iter()
                                .copied()
                                .find(|&p| self.sec_is_open(p))
                            {
                                self.sec_focus(p);
                            }
                        }
                        let group = panel_dock::SecGroup::from_index(self.sec.active_group);
                        self.status = vb_session::i18n::t_args(
                            "ui-app-dispatch-view-064",
                            &[(
                                "a1",
                                vb_session::i18n::FluentValue::from((group.label()).to_string()),
                            )],
                        );
                    }
                    super::FocusZone::Canvas => {
                        self.status = vb_session::i18n::t("ui-app-dispatch-view-065");
                    }
                }
            }
            // ── S5 清单 ②:逐级退出(浮层→面板→画布)的「面板级」原语。
            // 浮层级由 Esc 回退链的 esc_dialog_top 承担、画布级由既有
            // canvas.cancel 承担;本命令独立可用(命令面板/Agent),
            // Esc 键位仍归 canvas.cancel(见 binds.rs 注释)。
            "view.escape_overlay" => {
                if self.esc_dialog_top().is_some() {
                    self.close_esc_dialog_top();
                } else if self.focus_zone != super::FocusZone::Canvas {
                    self.focus_zone = super::FocusZone::Canvas;
                    self.status = vb_session::i18n::t("ui-app-dispatch-view-065");
                }
                // 浮层与面板区都没有 → 画布级交还 canvas.cancel 语义
                // (本命令不重复取消选择;Esc 键路径会继续走它)。
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
                    self.say(vb_session::i18n::t("ui-app-dispatch-view-011"));
                } else {
                    self.panels_hidden = false;
                    if !forced {
                        self.dock_collapsed = false;
                    }
                    self.panel_tab = panels::TAB_LAYERS;
                    self.say(vb_session::i18n::t("ui-app-dispatch-view-012"));
                }
            }
            "view.toggle_all_panels" => {
                self.panels_hidden = !self.panels_hidden;
                self.say(if self.panels_hidden {
                    vb_session::i18n::t("ui-app-dispatch-view-013")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-014")
                });
            }
            "view.zoom_to_selection" => {
                self.zoom_to_selection();
            }
            "view.toggle_rulers" => {
                self.rulers_on = !self.rulers_on;
                self.status = vb_session::i18n::t_args(
                    "ui-app-dispatch-view-015",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            (if self.rulers_on {
                                vb_session::i18n::t("ui-common-show")
                            } else {
                                vb_session::i18n::t("ui-common-hide")
                            })
                            .to_string(),
                        ),
                    )],
                );
            }
            "view.toggle_guides" => {
                self.guides_visible = !self.guides_visible;
                self.status = vb_session::i18n::t_args(
                    "ui-app-dispatch-view-016",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            (if self.guides_visible {
                                vb_session::i18n::t("ui-common-show")
                            } else {
                                vb_session::i18n::t("ui-common-hide")
                            })
                            .to_string(),
                        ),
                    )],
                );
            }
            "view.lock_guides" => {
                self.guides_locked = !self.guides_locked;
                self.status = vb_session::i18n::t_args(
                    "ui-app-dispatch-view-016",
                    &[(
                        "a1",
                        vb_session::i18n::FluentValue::from(
                            (if self.guides_locked {
                                vb_session::i18n::t("ui-app-dispatch-view-017")
                            } else {
                                vb_session::i18n::t("ui-app-dispatch-view-018")
                            })
                            .to_string(),
                        ),
                    )],
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
                    .unwrap_or_else(|| vb_session::i18n::t("ui-app-dispatch-view-019"));
                self.status = vb_session::i18n::t_args(
                    "ui-app-dispatch-view-020",
                    &[
                        (
                            "added",
                            vb_session::i18n::FluentValue::from((added).to_string()),
                        ),
                        (
                            "key",
                            vb_session::i18n::FluentValue::from((key).to_string()),
                        ),
                    ],
                );
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
                    vb_session::i18n::t("ui-app-dispatch-view-021")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-022")
                });
            }
            "view.toggle_para_panel" => {
                self.para_panel_open = !self.para_panel_open;
                self.sec_focus(panel_dock::SecPanel::Para);
                self.say(if self.para_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-023")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-024")
                });
            }
            // ── S4 外观/描边面板显隐(⇧F6 / ^F10;design/03 §5.9 / §5.7) ──
            "view.toggle_appearance_panel" => {
                self.appearance_panel_open = !self.appearance_panel_open;
                self.sec_focus(panel_dock::SecPanel::Appearance);
                self.say(if self.appearance_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-025")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-026")
                });
            }
            "view.toggle_stroke_panel" => {
                self.stroke_panel_open = !self.stroke_panel_open;
                self.sec_focus(panel_dock::SecPanel::Stroke);
                self.say(if self.stroke_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-027")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-028")
                });
            }
            // ── S4-b 渐变/透明度/颜色面板显隐(^F9 / ⇧^F10 / F6) ──
            "view.toggle_gradient_panel" => {
                self.gradient_panel_open = !self.gradient_panel_open;
                self.sec_focus(panel_dock::SecPanel::Gradient);
                self.say(if self.gradient_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-029")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-030")
                });
            }
            "view.toggle_opacity_panel" => {
                self.opacity_panel_open = !self.opacity_panel_open;
                self.sec_focus(panel_dock::SecPanel::Opacity);
                self.say(if self.opacity_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-031")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-032")
                });
            }
            "view.toggle_color_panel" => {
                self.color_panel_open = !self.color_panel_open;
                self.sec_focus(panel_dock::SecPanel::Color);
                self.say(if self.color_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-033")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-034")
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
                    vb_session::i18n::t("ui-app-dispatch-view-035")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-036")
                });
            }
            // ── 阶段 2:变换数值面板(副文档 03-2,⇧F8)──
            "view.toggle_transform_panel" => {
                self.transform_panel_open = !self.transform_panel_open;
                self.sec_focus(panel_dock::SecPanel::Transform);
                self.say(if self.transform_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-037")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-038")
                });
            }
            "view.toggle_align_panel" => {
                self.align_panel_open = !self.align_panel_open;
                self.sec_focus(panel_dock::SecPanel::Align);
                self.say(if self.align_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-039")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-040")
                });
            }
            // ── 04-4:开发者统计与提示条(默认隐藏/显示,状态入 workspace.json)──
            "view.developer_stats" => {
                self.dev_stats = !self.dev_stats;
                self.say(if self.dev_stats {
                    vb_session::i18n::t("ui-app-dispatch-view-041")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-042")
                });
            }
            "view.toggle_hints" => {
                self.hints = !self.hints;
                self.say(if self.hints {
                    vb_session::i18n::t("ui-app-dispatch-view-043")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-044")
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
                    vb_session::i18n::t("ui-app-dispatch-view-045")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-046")
                });
            }
            // ── 04-3:UI 缩放档位(完整首选项九分类属阶段 5;最小入口挂视图菜单)──
            "view.ui_scale_up" => self.step_ui_scale(1),
            "view.ui_scale_down" => self.step_ui_scale(-1),
            "view.ui_scale_reset" => {
                self.ui_scale = 1.0;
                self.save_workspace();
                self.say(vb_session::i18n::t("ui-app-dispatch-view-047"));
            }
            // ── 04-6:显示未支持工具(design/06 §二;置灰展示,点击有响应)──
            "edit.toggle_unsupported_tools" => {
                self.show_all_tools = !self.show_all_tools;
                self.save_workspace();
                self.say(if self.show_all_tools {
                    vb_session::i18n::t("ui-app-dispatch-view-048")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-049")
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
                    vb_session::i18n::t("ui-app-dispatch-view-050")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-051")
                });
            }
            // 07-K:资产面板(次级坞「资产」组;assets/ 清单 + 引用关系)
            "view.toggle_assets_panel" => {
                self.assets_open = !self.assets_open;
                self.sec_focus(panel_dock::SecPanel::Assets);
                self.say(if self.assets_open {
                    vb_session::i18n::t("ui-app-dispatch-view-052")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-053")
                });
            }
            // ── 05-9 动效时间轴(09-I;ADR-VB-L11)──
            // 时间轴面板(次级坞「时间轴」组;关键帧轨道 + 播放头)
            "view.toggle_timeline_panel" => {
                self.timeline_open = !self.timeline_open;
                self.sec_focus(panel_dock::SecPanel::Timeline);
                self.say(if self.timeline_open {
                    vb_session::i18n::t("ui-app-dispatch-view-054")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-055")
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
                self.say(vb_session::i18n::t("ui-app-dispatch-view-056"));
            }
            // 插件坞面板(次级坞「插件」组;Running 插件的注册面板)
            "view.toggle_plugins_panel" => {
                self.plugins_panel_open = !self.plugins_panel_open;
                self.sec_focus(panel_dock::SecPanel::Plugins);
                self.say(if self.plugins_panel_open {
                    vb_session::i18n::t("ui-app-dispatch-view-057")
                } else {
                    vb_session::i18n::t("ui-app-dispatch-view-058")
                });
            }
            // 07-E:项目健康检查(报告窗口;打开即重算;07-N 起含无障碍三查)
            "file.health_check" => {
                if self.project_dir.is_some() {
                    self.health_open = true;
                    self.health_report = None; // 置空 → 窗口打开时现算
                    self.say(vb_session::i18n::t("ui-app-dispatch-view-059"));
                } else {
                    self.toast_warn(vb_session::i18n::t("ui-app-dispatch-view-060"));
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
                    self.say(vb_session::i18n::t_args(
                        "ui-app-dispatch-view-061",
                        &[
                            (
                                "pending",
                                vb_session::i18n::FluentValue::from((pending).to_string()),
                            ),
                            ("n", vb_session::i18n::FluentValue::from((n).to_string())),
                        ],
                    ));
                } else {
                    self.say(vb_session::i18n::t_args(
                        "ui-app-dispatch-view-062",
                        &[(
                            "pending",
                            vb_session::i18n::FluentValue::from((pending).to_string()),
                        )],
                    ));
                }
            }
            _ => matched = false,
        }
        matched
    }
}

// ─────────────────────── S5 清单 ②门禁(单测) ───────────────────────

#[cfg(test)]
mod panel_zone_tests {
    use super::super::FocusZone;
    use crate::app::assemble::tests::app_fresh;
    use crate::app::panel_dock::SecPanel;
    use crate::app::panels::TAB_LAYERS;
    use crate::app::Tool;

    /// 循环命令:区状态机按「画布 → 右坞 →(次级坞)→ 画布」推进;
    /// 次级坞无开面板时被剔除;跳右坞/次级坞必须反隐藏、反折叠
    /// (区跳转要有可见反馈,否则对键盘用户是空操作)。
    #[test]
    fn panel_focus_cycle_advances_regions() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        // 次级坞全关:二区环
        app.run_command("view.focus_next_panel", false, false);
        assert_eq!(app.focus_zone, FocusZone::RightDock);
        assert!(!app.panels_hidden, "区跳转必须反面板隐藏");
        assert!(!app.dock_collapsed, "区跳转必须反用户折叠");
        app.run_command("view.focus_next_panel", false, false);
        assert_eq!(
            app.focus_zone,
            FocusZone::Canvas,
            "次级坞不可用 → 直接回画布"
        );
        // 打开一个次级面板:三区环
        app.sec_set_open(SecPanel::Char, true);
        app.focus_zone = FocusZone::Canvas;
        app.run_command("view.focus_next_panel", false, false);
        app.run_command("view.focus_next_panel", false, false);
        assert_eq!(app.focus_zone, FocusZone::SecDock);
        assert!(!app.sec_dock_collapsed, "次级坞区跳转必须反折叠");
        app.run_command("view.focus_next_panel", false, false);
        assert_eq!(app.focus_zone, FocusZone::Canvas, "循环回卷");
        // 反向:画布 →(次级坞)→ 右坞
        app.run_command("view.focus_prev_panel", false, false);
        assert_eq!(app.focus_zone, FocusZone::SecDock);
        app.run_command("view.focus_prev_panel", false, false);
        assert_eq!(app.focus_zone, FocusZone::RightDock);
        app.run_command("view.focus_prev_panel", false, false);
        assert_eq!(app.focus_zone, FocusZone::Canvas);
    }

    /// Esc 层级(浮层→面板→画布):面板区持焦时第一下 Esc 只退回画布
    /// (不消费画布级取消);第二下才走画布级(工具回选择)。
    #[test]
    fn escape_steps_out_panel_region_before_canvas_cancel() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.sec_set_open(SecPanel::Char, true);
        app.run_command("tool.rect", false, false);
        app.run_command("view.focus_next_panel", false, false);
        assert_eq!(app.focus_zone, FocusZone::RightDock);
        // Esc(= canvas.cancel):先退面板级,工具保持
        app.run_command("canvas.cancel", false, false);
        assert_eq!(app.focus_zone, FocusZone::Canvas, "第一下 Esc 退面板区");
        assert_eq!(app.tool, Tool::Rect, "面板级消费,画布级取消不动工具");
        // 第二下 Esc:画布级(工具回选择)
        app.run_command("canvas.cancel", false, false);
        assert_eq!(app.tool, Tool::Select, "第二下 Esc 走画布级取消");
        // 独立命令:浮层/面板都没有时是良性空操作(不炸不误清选区)
        app.run_command("view.escape_overlay", false, false);
        assert_eq!(app.focus_zone, FocusZone::Canvas);
    }

    /// `view.escape_overlay` 独立路径:面板区持焦 → 退回画布;
    /// 已在画布 → 幂等。与 Esc 键路径(canvas.cancel 内联)同语义。
    #[test]
    fn escape_overlay_command_is_the_panel_level_primitive() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        app.sec_set_open(SecPanel::Char, true);
        app.run_command("view.focus_prev_panel", false, false);
        assert_eq!(app.focus_zone, FocusZone::SecDock, "反向第一跳 = 次级坞");
        app.run_command("view.escape_overlay", false, false);
        assert_eq!(app.focus_zone, FocusZone::Canvas);
        app.run_command("view.escape_overlay", false, false);
        assert_eq!(app.focus_zone, FocusZone::Canvas, "幂等");
        // Tab 跳转命令不受影响(同族回归钉):F4 循环右坞 Tab
        app.panel_tab = TAB_LAYERS;
        app.run_command("view.next_panel_tab", false, false);
        assert_eq!(app.panel_tab, TAB_LAYERS + 1);
    }
}
