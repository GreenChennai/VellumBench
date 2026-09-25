//! 图层 Tab · 渲染:Tab 装配 / 行收集 / 行 UI / 图标命中。
//!
//! 06-1 自 `panels/layers.rs` 按职责拆出(纯搬移,零行为变化):
//! 拖拽高亮、右键菜单与落下执行在 `dragdrop`(同属 `impl VellumApp`)。

use egui::{Color32, Pos2, Rect, RichText, Sense};
use vb_doc::commands::Command;
use vb_doc::model::{NodeId, NodeKind};
use vb_ui::components::{caption, icon_button};
use vb_ui::icons::{self, Name};
use vb_ui::theme;

use super::model::*;
use crate::app::VellumApp;

impl VellumApp {
    /// H-3:图层面板空白双击 → 新建图层(加到当前画板末尾)。
    ///
    /// 走既有 [`Command::Insert`] 命令路径(可撤销),不造新命令;
    /// 没有画板时给中文 toast 指路(绝不「点了没反应」)。
    fn layers_new_layer_in_active_artboard(&mut self) {
        let Some(ab) = self.active_artboard() else {
            self.toast_warn("没有画板:先用画板工具(Shift+O)在画布上创建");
            return;
        };
        let (ab_sid, ab_w, ab_h) = {
            let n = self.doc.nodes.get(ab).unwrap();
            (
                n.sid.as_str().to_string(),
                n.geom.w.max(240.0),
                n.geom.h.max(160.0),
            )
        };
        let mut node = vb_doc::model::Node::new(NodeKind::Layer, "新图层", self.doc.alloc_sid());
        // 默认尺寸 = 画板的 1/4(上限 240×160),不出界、可拖可改
        node.geom = vb_doc::model::Geom {
            x: 0.0,
            y: 0.0,
            w: (ab_w * 0.5).min(240.0),
            h: (ab_h * 0.25).min(160.0),
        };
        let sid = node.sid.as_str().to_string();
        let tree = vb_doc::model::NodeTree {
            node,
            children: vec![],
        };
        self.exec(Command::Insert {
            parent_sid: ab_sid,
            index: usize::MAX,
            tree,
        });
        self.selection = vec![sid.clone()];
        self.say(format!("已新建图层 {sid}(画板末尾;Ctrl+Z 可撤销)"));
    }

    pub(crate) fn layers_tab(&mut self, ui: &mut egui::Ui) {
        let t = theme::tokens(ui.ctx());
        ui.heading("图层");
        ui.separator();

        // --- 搜索框(02-4-7;纯前端过滤) ---
        ui.horizontal(|ui| {
            ui.monospace("🔍");
            ui.add_sized(
                [ui.available_width(), vb_ui::theme::row_height(ui.ctx())],
                egui::TextEdit::singleline(&mut self.layer_search).hint_text("搜索图层名"),
            );
        });
        let query = self.layer_search.clone();

        // --- 收集行描述(先收集后渲染,免借用冲突) ---
        let rows = self.collect_rows(&query);
        let mut zones: Vec<DropZone> = Vec::new();

        egui::ScrollArea::vertical().show(ui, |ui| {
            for r in &rows {
                self.layer_row_ui(ui, r, &mut zones);
            }
            if rows.is_empty() {
                if query.trim().is_empty() {
                    // U-5:图层空态 = 统一「图标 + 一句短话 + 动作按钮」
                    ui.add_space(theme::space::S3);
                    ui.horizontal(|ui| {
                        ui.add_space(theme::space::S2);
                        ui.label(icons::rich(Name::KindLayer, 18.0).color(t.text_3));
                        ui.label(caption(
                            ui,
                            "画板下还没有对象 —— 用工具创建,或双击下方空白新建图层。",
                        ));
                    });
                    ui.add_space(theme::space::S2);
                    ui.horizontal_wrapped(|ui| {
                        for (id, icon, tip) in [
                            ("tool.rect", Name::ToolRect, "矩形工具(M):拖框新建"),
                            ("tool.text", Name::ToolText, "文字工具(T):单击点文本"),
                        ] {
                            if icon_button(ui, icon, tip).clicked() {
                                self.run_command(id, false, false);
                            }
                        }
                    });
                } else {
                    ui.label(caption(ui, "没有匹配的图层(清空搜索框恢复)"));
                }
            }
            self.layer_drag_overlay(ui, &zones);
        });

        // --- H-3:图层面板空白双击 = 新建图层(画板末尾;命令层可撤销)---
        // 空白带吃满剩余高度;拖拽进行中不占位判定(避免与落下冲突)。
        if self.layer_drag.is_none() {
            let t = theme::tokens(ui.ctx());
            let (blank, blank_resp) = ui.allocate_exact_size(
                egui::Vec2::new(ui.available_width(), ui.available_height().max(48.0)),
                egui::Sense::click(),
            );
            if blank_resp.hovered() {
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                ui.painter().rect_stroke(
                    blank,
                    theme::radius::sm(),
                    egui::Stroke::new(theme::stroke::HAIRLINE, t.border),
                    egui::StrokeKind::Inside,
                );
            }
            if blank_resp.double_clicked() {
                self.layers_new_layer_in_active_artboard();
            }
            let _ = blank_resp.on_hover_text("双击:新建图层(加到当前画板末尾)");
        }

        // --- 底部操作行(02-4-5) ---
        // 剪切蒙版 / 单节点导出:文档模型暂无命令支撑,不放假按钮
        // (登记遗留项,见 02d 报告)。
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("⌖ 定位对象").clicked() {
                if self.selection.is_empty() {
                    self.say("定位对象:先在图层树选中一个节点");
                } else {
                    self.run_command("view.zoom_to_selection", false, false);
                }
            }
        });
    }

    /// 收集全部画板子树的行描述(搜索过滤 + 展开/收起)。
    fn collect_rows(&self, query: &str) -> Vec<RowDesc> {
        let filtering = !query.trim().is_empty();
        let mut out = Vec::new();
        for &ab in &self.doc.artboards {
            self.push_rows(ab, 0, query, filtering, &mut out);
        }
        out
    }

    fn push_rows(
        &self,
        id: NodeId,
        depth: u8,
        query: &str,
        filtering: bool,
        out: &mut Vec<RowDesc>,
    ) {
        let Some(n) = self.doc.nodes.get(id) else {
            return;
        };
        let parent_sid = n
            .parent
            .and_then(|p| self.doc.nodes.get(p))
            .map(|p| p.sid.as_str().to_string());
        let index = n
            .parent
            .and_then(|p| self.doc.nodes.get(p))
            .map(|p| p.children.iter().position(|&c| c == id).unwrap_or(0))
            .unwrap_or(0);
        if !filtering || matches_query(&self.doc, id, query) {
            let kind = &n.kind;
            out.push(RowDesc {
                sid: n.sid.as_str().to_string(),
                name: n.name.clone(),
                icon: kind_icon(kind),
                frozen: matches!(kind, NodeKind::Frozen { .. }),
                container: kind.is_container(),
                is_artboard: matches!(kind, NodeKind::Artboard),
                hidden: n.hidden,
                locked: n.locked,
                has_children: !n.children.is_empty(),
                expanded: filtering || !self.layer_expanded.contains(n.sid.as_str()),
                depth,
                parent_sid,
                index,
                mark: n.attrs.get(MARK_ATTR).cloned(),
            });
        }
        // 收起态不递归(搜索态强制展开命中路径)
        let collapsed = !filtering && self.layer_expanded.contains(n.sid.as_str());
        if !n.children.is_empty() && !collapsed {
            for &c in &n.children {
                self.push_rows(c, depth + 1, query, filtering, out);
            }
        }
    }

    /// 渲染一行 + 收集命中区 + 处理交互(选中/改名/显隐/锁定/层序/
    /// 拖拽起手/颜色标记/右键菜单)。
    fn layer_row_ui(&mut self, ui: &mut egui::Ui, r: &RowDesc, zones: &mut Vec<DropZone>) {
        let t = theme::tokens(ui.ctx());
        let selected = self.selection.last().is_some_and(|s| *s == r.sid);
        let editing = self.editing_layer.as_deref() == Some(r.sid.as_str());
        let indent = theme::space::S2 + r.depth as f32 * theme::space::S6;
        let (rect, resp) = ui.allocate_exact_size(
            egui::Vec2::new(ui.available_width(), theme::space::ROW_HEIGHT),
            Sense::click_and_drag(),
        );
        zones.push(DropZone {
            rect,
            sid: r.sid.clone(),
            parent_sid: r.parent_sid.clone().unwrap_or_default(),
            container: r.container,
            index: r.index,
        });

        // 行底:选中 > 冻结淡底 > 悬停
        let hover_t = ui.ctx().animate_bool_with_time(
            ui.id().with(("vblayer", &r.sid)),
            resp.hovered() && !selected,
            theme::motion::HOVER,
        );
        let fill = if selected {
            t.accent_dim
        } else if r.frozen {
            theme::semantic::frozen_fill(self.theme_dark)
        } else {
            blend(Color32::TRANSPARENT, t.bg_hover, hover_t)
        };
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(theme::space::S1, 1.0)),
            theme::radius::sm(),
            fill,
        );

        // 布局:缩进 | 展开箭头 | 类型图标 | 标记点 | 名称 | ↑ ↓ 👁 🔒
        let center = rect.center().y;
        let right_edge = rect.right() - theme::space::S2;
        let btn_w = 18.0;
        let lock_x = right_edge - btn_w * 0.5;
        let eye_x = lock_x - btn_w;
        let down_x = eye_x - btn_w;
        let up_x = down_x - btn_w;
        let mut x = rect.left() + indent;

        // 展开箭头(有子级才画;点击切换)
        if r.has_children {
            let chev = if r.expanded {
                Name::Expanded
            } else {
                Name::Collapsed
            };
            let crect = Rect::from_min_size(Pos2::new(x, rect.top()), egui::Vec2::splat(14.0));
            ui.painter().text(
                crect.center(),
                egui::Align2::CENTER_CENTER,
                chev.glyph().to_string(),
                icons::font(11.0),
                t.text_3,
            );
            let crep = ui.interact(crect, ui.id().with(("vbchev", &r.sid)), Sense::click());
            if crep.clicked() {
                if r.expanded {
                    self.layer_expanded.insert(r.sid.clone());
                } else {
                    self.layer_expanded.remove(&r.sid);
                }
            }
            crep.on_hover_text(if r.expanded { "收起" } else { "展开" });
        }
        x += 15.0;

        // 类型图标(冻结块 = ❄)
        ui.painter().text(
            Pos2::new(x + 7.0, center),
            egui::Align2::CENTER_CENTER,
            r.icon.glyph().to_string(),
            icons::font(12.0),
            if r.frozen { t.text_3 } else { t.text_2 },
        );
        x += 17.0;

        // 颜色标记点(02-4-3):点击循环取色,入文档(data-vb-mark)
        let drect = Rect::from_center_size(Pos2::new(x + 4.0, center), egui::Vec2::splat(10.0));
        let mark_color = r
            .mark
            .as_deref()
            .and_then(vb_common::color::parse_color)
            .map(|c| Color32::from_rgb(c.r, c.g, c.b));
        let _ = ui.painter().circle_filled(
            drect.center(),
            3.5,
            mark_color.unwrap_or(Color32::TRANSPARENT),
        );
        if mark_color.is_none() {
            ui.painter()
                .circle_stroke(drect.center(), 3.5, egui::Stroke::new(1.0, t.text_3));
        }
        let drep = ui.interact(drect, ui.id().with(("vbmark", &r.sid)), Sense::click());
        if drep.clicked() {
            let next = cycle_mark(r.mark.as_deref()).map(str::to_string);
            let said = match &next {
                Some(c) => format!("颜色标记 → {c}(data-vb-mark,已入文档)"),
                None => "颜色标记已清除".to_string(),
            };
            let cmd = mark_cmd(&self.doc, &r.sid, next);
            self.exec(cmd);
            self.say(said);
        }
        drep.on_hover_text("图层颜色标记(点击循环;颜色随文档保存)");
        x += 12.0;

        // 名称(冻结块 = 灰色斜体;双击行内改名)
        let name_rect = Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new(up_x - btn_w * 0.6, rect.bottom()),
        );
        if editing {
            let mut buf = r.name.clone();
            let edit = ui
                .new_child(egui::UiBuilder::new().max_rect(name_rect))
                .add(egui::TextEdit::singleline(&mut buf).desired_width(name_rect.width()));
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            if edit.lost_focus() || enter {
                self.editing_layer = None;
                let trimmed = buf.trim().to_string();
                if !trimmed.is_empty() && trimmed != r.name {
                    self.exec(Command::Rename {
                        sid: r.sid.clone(),
                        new: trimmed,
                        old: None,
                    });
                    self.say("已重命名(data-vb-name 同步)");
                }
            }
        } else {
            let mut text = RichText::new(&r.name).font(egui::FontId::proportional(12.5));
            text = if r.frozen {
                text.italics().color(t.text_3)
            } else if r.hidden {
                text.color(t.text_3)
            } else {
                text.color(t.text)
            };
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(name_rect));
            child.add(egui::Label::new(text).truncate().selectable(false));
        }

        // 右侧开关:↑ ↓ 👁 🔒(既有行为保持)
        if self.icon_hit(
            ui,
            up_x,
            center,
            btn_w,
            &r.sid,
            "vbup",
            Name::MoveUp,
            t.text_3,
            "前移一层",
        ) {
            self.reorder(&r.sid, 1);
        }
        if self.icon_hit(
            ui,
            down_x,
            center,
            btn_w,
            &r.sid,
            "vbdown",
            Name::MoveDown,
            t.text_3,
            "后移一层",
        ) {
            self.reorder(&r.sid, -1);
        }
        let eye_icon = if r.hidden {
            Name::Hidden
        } else {
            Name::Visible
        };
        let eye_color = if r.hidden { t.warn } else { t.text_3 };
        let eye_tip = if r.hidden {
            "显示(取消隐藏)"
        } else {
            "隐藏"
        };
        if self.icon_hit(
            ui, eye_x, center, btn_w, &r.sid, "vbeye", eye_icon, eye_color, eye_tip,
        ) {
            self.exec(Command::SetFlags {
                sid: r.sid.clone(),
                hidden: Some(!r.hidden),
                locked: None,
                old: None,
            });
        }
        let lock_icon = if r.locked {
            Name::Locked
        } else {
            Name::Unlocked
        };
        let lock_color = if r.locked { t.warn } else { t.text_3 };
        let lock_tip = if r.locked { "解锁" } else { "锁定" };
        if self.icon_hit(
            ui, lock_x, center, btn_w, &r.sid, "vblock", lock_icon, lock_color, lock_tip,
        ) {
            self.exec(Command::SetFlags {
                sid: r.sid.clone(),
                hidden: None,
                locked: Some(!r.locked),
                old: None,
            });
        }

        // 行交互:点击选中 / 双击改名 / 拖拽起手 / 右键菜单
        if resp.clicked() {
            self.selection = vec![r.sid.clone()];
        }
        if resp.double_clicked() {
            self.editing_layer = Some(r.sid.clone());
        }
        if resp.drag_started() {
            let dup = ui.input(|i| i.modifiers.alt);
            self.layer_drag = Some(LayerDrag {
                sid: r.sid.clone(),
                dup,
            });
            if dup {
                self.say("Alt+拖拽:复制到目标位置");
            }
        }
        if r.frozen {
            resp.clone()
                .on_hover_text("冻结块:含不支持的 CSS,原样保留(❄)");
        }
        self.layer_context_menu(ui, &resp, r);
    }

    /// 行内右侧小图标命中区(手绘 + 点击;与行主体点击互不干扰)。
    #[allow(clippy::too_many_arguments)]
    fn icon_hit(
        &mut self,
        ui: &mut egui::Ui,
        cx: f32,
        cy: f32,
        size: f32,
        sid: &str,
        key: &'static str,
        icon: Name,
        color: Color32,
        tip: &str,
    ) -> bool {
        let r = Rect::from_center_size(Pos2::new(cx, cy), egui::Vec2::splat(size));
        let resp = ui.interact(r, ui.id().with((key, sid)), Sense::click());
        ui.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            icon.glyph().to_string(),
            icons::font(13.0),
            color,
        );
        let clicked = resp.clicked();
        resp.on_hover_text(tip);
        clicked
    }
}

/// 本地面板用两色插值(悬停过渡;与 vb_ui::components 内部实现同式)。
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

fn kind_icon(kind: &NodeKind) -> Name {
    match kind {
        NodeKind::Artboard => Name::KindArtboard,
        NodeKind::Layer => Name::KindLayer,
        NodeKind::Group => Name::KindGroup,
        NodeKind::Box => Name::KindBox,
        NodeKind::Text { .. } => Name::KindText,
        NodeKind::Image { .. } => Name::KindImage,
        NodeKind::Vector { .. } => Name::KindVector,
        NodeKind::Slice => Name::KindSlice,
        NodeKind::Frozen { .. } => Name::KindFrozen,
    }
}
