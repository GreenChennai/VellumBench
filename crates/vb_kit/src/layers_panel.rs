//! VellumBench 文档图层投影面板。
//!
//! 图层数据只从 `vb_session::CanvasSnapshot` 读取,选择操作仍回到共享
//! `ProjectSession`;此面板不构造 Sable Scene 或第二份可编辑文档。

use sable::gpui::{
    div, px, uniform_list, App, AppContext as _, Context, ElementId, Entity, FocusHandle,
    Focusable, Hsla, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement as _, Render, Styled as _, Window,
};
use sable::widgets::controls::button::{Button, ButtonSize, ButtonVariant};
use sable::widgets::interact::{self, list_nav, ListNavIntent, Semantic, SemanticRole};
use sable::widgets::prelude::{h_flex, v_flex};
use sable::widgets::theme::theme;
use sable::widgets::tokens::FONT_SIZE_BODY;
use vb_session::command::CommandOutcome;
use vb_session::i18n::t;
use vb_session::project::{LayerSnapshot, ProjectSession};
use vb_session::selection::ModifierState;

use crate::tokens::{radius, space};

/// 共享会话的图层面板;只投影稳定 SID、名称与树深度。
pub struct ProjectLayersPanel {
    focus_handle: FocusHandle,
    session: Option<Entity<ProjectSession>>,
    error: Option<String>,
    action_status: Option<String>,
    layers: Vec<LayerSnapshot>,
    selected_sids: Vec<String>,
    select_all_button: Option<Entity<Button>>,
    actions: Option<LayerActions>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LayerActionState {
    selected_count: usize,
    all_hidden: bool,
    all_locked: bool,
    can_group: bool,
    can_ungroup: bool,
}

fn layer_command_for_key(
    key: &str,
    platform_modifier: bool,
    shift: bool,
    alt: bool,
) -> Option<&'static str> {
    if !platform_modifier || alt {
        return None;
    }
    match (key.to_ascii_lowercase().as_str(), shift) {
        ("a", false) => Some("edit.select_all"),
        ("g", false) => Some("object.group"),
        ("g", true) => Some("object.ungroup"),
        _ => None,
    }
}

struct LayerActions {
    state: LayerActionState,
    visibility: Entity<Button>,
    lock: Entity<Button>,
    group: Entity<Button>,
    ungroup: Entity<Button>,
}

impl ProjectLayersPanel {
    /// 构造图层面板并订阅同一 `ProjectSession` 的变更。
    pub fn new(
        session: Option<Entity<ProjectSession>>,
        error: Option<String>,
        cx: &mut App,
    ) -> Entity<Self> {
        let has_session = session.is_some();
        let panel = cx.new(|cx| {
            let mut panel = Self {
                focus_handle: cx.focus_handle(),
                session: session.clone(),
                error,
                action_status: None,
                layers: Vec::new(),
                selected_sids: Vec::new(),
                select_all_button: None,
                actions: None,
            };
            panel.refresh_projection(cx);
            panel
        });
        if let Some(session) = session {
            panel.update(cx, |_, cx| {
                cx.observe(&session, |panel, _, cx| {
                    panel.refresh_projection(cx);
                    cx.notify();
                })
                .detach();
            });
        }
        let weak_panel = panel.downgrade();
        let select_all_button = cx.new(|_| {
            Button::new("vb-layer-select-all", t("ui-layer-select-all"))
                .size(ButtonSize::Compact)
                .variant(ButtonVariant::Secondary)
                .disabled(!has_session)
                .on_press(move |_, _, cx| {
                    let _ = weak_panel.update(cx, |panel, cx| {
                        panel.dispatch_catalog("edit.select_all", cx);
                    });
                })
        });
        panel.update(cx, |panel, _| {
            panel.select_all_button = Some(select_all_button);
        });
        panel
    }

    fn refresh_projection(&mut self, cx: &App) {
        let Some(session) = self.session.as_ref() else {
            self.layers.clear();
            self.selected_sids.clear();
            return;
        };
        match session.read(cx).active_canvas_snapshot() {
            Ok(snapshot) => {
                self.layers = snapshot.layers;
                self.selected_sids = snapshot.selected_sids;
                self.error = None;
            }
            Err(error) => {
                self.layers.clear();
                self.selected_sids.clear();
                self.error = Some(error.to_string());
            }
        }
    }

    fn selected_action_state(&self, cx: &App) -> Option<LayerActionState> {
        let selected = self
            .layers
            .iter()
            .filter(|layer| self.selected_sids.contains(&layer.sid))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return None;
        }
        let (can_group, can_ungroup) = self
            .session
            .as_ref()
            .map(|session| {
                let session = session.read(cx);
                (
                    session.can_group_selection(),
                    session.can_ungroup_selection(),
                )
            })
            .unwrap_or((false, false));
        Some(LayerActionState {
            selected_count: selected.len(),
            all_hidden: selected.iter().all(|layer| layer.hidden),
            all_locked: selected.iter().all(|layer| layer.locked),
            can_group,
            can_ungroup,
        })
    }

    fn ensure_actions(&mut self, cx: &mut Context<Self>) {
        let state = self.selected_action_state(cx);
        if self.actions.as_ref().map(|actions| actions.state) == state {
            return;
        }
        self.actions = None;
        let Some(state) = state else {
            return;
        };
        let panel = cx.entity().downgrade();
        let visibility_label = if state.all_hidden {
            t("ui-layer-show-selected")
        } else {
            t("ui-layer-hide-selected")
        };
        let visibility_panel = panel.clone();
        let visibility = cx.new(|_| {
            Button::new("vb-layer-visibility", visibility_label)
                .size(ButtonSize::Compact)
                .variant(ButtonVariant::Secondary)
                .on_press(move |_, _, cx| {
                    let _ = visibility_panel.update(cx, |panel, cx| {
                        panel.set_selection_flags(Some(!state.all_hidden), None, cx);
                    });
                })
        });
        let lock_label = if state.all_locked {
            t("ui-layer-unlock-selected")
        } else {
            t("ui-layer-lock-selected")
        };
        let lock_panel = panel.clone();
        let lock = cx.new(|_| {
            Button::new("vb-layer-lock", lock_label)
                .size(ButtonSize::Compact)
                .variant(ButtonVariant::Secondary)
                .on_press(move |_, _, cx| {
                    let _ = lock_panel.update(cx, |panel, cx| {
                        panel.set_selection_flags(None, Some(!state.all_locked), cx);
                    });
                })
        });
        let group_panel = panel.clone();
        let group = cx.new(|_| {
            Button::new("vb-layer-group", t("ui-layer-group-selected"))
                .size(ButtonSize::Compact)
                .variant(ButtonVariant::Secondary)
                .disabled(!state.can_group)
                .on_press(move |_, _, cx| {
                    let _ = group_panel.update(cx, |panel, cx| {
                        panel.dispatch_catalog("object.group", cx);
                    });
                })
        });
        let ungroup = cx.new(|_| {
            Button::new("vb-layer-ungroup", t("ui-layer-ungroup-selected"))
                .size(ButtonSize::Compact)
                .variant(ButtonVariant::Secondary)
                .disabled(!state.can_ungroup)
                .on_press(move |_, _, cx| {
                    let _ = panel.update(cx, |panel, cx| {
                        panel.dispatch_catalog("object.ungroup", cx);
                    });
                })
        });
        self.actions = Some(LayerActions {
            state,
            visibility,
            lock,
            group,
            ungroup,
        });
    }

    fn set_selection_flags(
        &mut self,
        hidden: Option<bool>,
        locked: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.clone() else {
            return;
        };
        self.action_status = session
            .update(cx, |session, session_cx| {
                let result = session.set_selected_flags(hidden, locked);
                session_cx.notify();
                result
            })
            .err()
            .map(|error| error.to_string());
        cx.notify();
    }

    fn dispatch_catalog(&mut self, catalog_id: &str, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let catalog_id = catalog_id.to_string();
        let outcome = session.update(cx, |session, session_cx| {
            let outcome = session.dispatch_catalog(&catalog_id);
            session_cx.notify();
            outcome
        });
        self.action_status = match outcome {
            CommandOutcome::Applied => None,
            CommandOutcome::NotRun { reason } => Some(reason),
        };
        cx.notify();
    }

    fn select_sid(&mut self, sid: &str, modifiers: ModifierState, cx: &mut Context<Self>) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let sid = sid.to_string();
        session.update(cx, |session, session_cx| {
            session.select_sid(Some(&sid), modifiers);
            session_cx.notify();
        });
    }

    fn on_list_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if event.is_held || self.layers.is_empty() {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if let Some(command) = layer_command_for_key(
            event.keystroke.key.as_str(),
            modifiers.platform,
            modifiers.shift,
            modifiers.alt,
        ) {
            self.dispatch_catalog(command, cx);
            return;
        }
        let current = self
            .selected_sids
            .first()
            .and_then(|sid| self.layers.iter().position(|layer| &layer.sid == sid))
            .unwrap_or(0);
        match list_nav(current, self.layers.len(), event.keystroke.key.as_str()) {
            Some(ListNavIntent::Move(index)) => {
                if let Some(sid) = self.layers.get(index).map(|layer| layer.sid.clone()) {
                    self.select_sid(&sid, modifiers_from_key(event), cx);
                }
            }
            Some(ListNavIntent::Activate) => {
                if let Some(sid) = self.layers.get(current).map(|layer| layer.sid.clone()) {
                    self.select_sid(&sid, ModifierState::default(), cx);
                }
            }
            Some(ListNavIntent::Escape) | None => {}
        }
    }
}

impl Focusable for ProjectLayersPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ProjectLayersPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        self.ensure_actions(cx);
        let layers = self.layers.clone();
        let selected_sids = self.selected_sids.clone();
        let weak = cx.entity().downgrade();
        let list = uniform_list(
            "vb-project-layers",
            layers.len(),
            move |range, _window, _cx| {
                let mut rows = Vec::with_capacity(range.end.saturating_sub(range.start));
                for index in range {
                    let Some(layer) = layers.get(index) else {
                        continue;
                    };
                    let sid = layer.sid.clone();
                    let name = if layer.name.trim().is_empty() {
                        layer.kind.to_string()
                    } else {
                        layer.name.clone()
                    };
                    let depth = layer.depth.min(12) as f32;
                    let selected = selected_sids.iter().any(|selected| selected == &sid);
                    let row_colors = colors;
                    let row_id = format!("vb-layer-{sid}");
                    let selected_sid = sid.clone();
                    let panel = weak.clone();
                    let on_select =
                        move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                            let modifiers = ModifierState {
                                shift: event.modifiers.shift,
                                control: event.modifiers.control,
                                alt: event.modifiers.alt,
                                platform: event.modifiers.platform,
                            };
                            let _ = panel.update(cx, |panel, cx| {
                                panel.focus_handle.focus(window);
                                panel.select_sid(&selected_sid, modifiers, cx);
                            });
                        };
                    let semantic = Semantic::new()
                        .with_role(SemanticRole::ListItem)
                        .with_label(name.clone());
                    let row = h_flex()
                        .id(ElementId::Name(row_id.into()))
                        .w_full()
                        .h(px(32.0))
                        .gap(px(space::S2))
                        .px(px(space::S2 + depth * 12.0))
                        .items_center()
                        .rounded(px(radius::SM))
                        .border_1()
                        .border_color(if selected {
                            row_colors.accent
                        } else {
                            Hsla::transparent_black()
                        })
                        .bg(if selected {
                            row_colors.accent_muted
                        } else {
                            row_colors.surface_1
                        })
                        .child(
                            div()
                                .w(px(18.0))
                                .text_size(px(FONT_SIZE_BODY))
                                .text_color(if selected {
                                    row_colors.accent
                                } else {
                                    row_colors.text_disabled
                                })
                                .child(layer_glyph(layer.kind)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(FONT_SIZE_BODY))
                                .text_color(if selected {
                                    row_colors.text_primary
                                } else {
                                    row_colors.text_secondary
                                })
                                .child(name),
                        )
                        .on_mouse_down(MouseButton::Left, on_select);
                    rows.push(interact::attach_semantics(row, &semantic));
                }
                rows
            },
        )
        .flex_1()
        .min_h_0()
        .bg(colors.surface_1)
        .track_focus(&self.focus_handle)
        .on_key_down(cx.listener(Self::on_list_key));

        let mut list_area = v_flex().flex_1().min_h_0().relative().child(list);
        if self.focus_handle.is_focused(window) {
            list_area = list_area.children(interact::focus_ring(colors.accent, radius::SM));
        }

        let mut header = h_flex()
            .items_center()
            .gap(px(space::S2))
            .px(px(space::S2))
            .py(px(space::S2))
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .flex_1()
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.text_primary)
                    .child(t("ui-shell-panel-layers")),
            );
        if let Some(select_all_button) = self.select_all_button.clone() {
            header = header.child(select_all_button);
        }
        header = header.child(
            div()
                .text_size(px(FONT_SIZE_BODY))
                .text_color(colors.text_disabled)
                .child(self.layers.len().to_string()),
        );

        let mut body = v_flex()
            .id("vb-project-layers-panel")
            .size_full()
            .min_h_0()
            .gap(px(space::S1))
            .p(px(space::S2))
            .bg(colors.surface_1)
            .child(header);
        if let Some(actions) = self.actions.as_ref() {
            body = body
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(space::S1))
                        .child(actions.visibility.clone())
                        .child(actions.lock.clone()),
                )
                .child(
                    h_flex()
                        .w_full()
                        .gap(px(space::S1))
                        .child(actions.group.clone())
                        .child(actions.ungroup.clone()),
                );
        }
        if let Some(status) = self.action_status.clone() {
            body = body.child(
                div()
                    .px(px(space::S2))
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.warning)
                    .child(status),
            );
        }
        if let Some(error) = self.error.clone() {
            body = body.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(space::S2))
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.warning)
                    .child(error),
            );
        } else if self.layers.is_empty() {
            body = body.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .p(px(space::S2))
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.text_secondary)
                    .child(t("ui-canvas-layers-empty")),
            );
        } else {
            body = body.child(list_area);
        }
        body
    }
}

fn modifiers_from_key(event: &KeyDownEvent) -> ModifierState {
    ModifierState {
        shift: event.keystroke.modifiers.shift,
        control: event.keystroke.modifiers.control,
        alt: event.keystroke.modifiers.alt,
        platform: event.keystroke.modifiers.platform,
    }
}

fn layer_glyph(kind: &str) -> &'static str {
    match kind {
        "artboard" => "▣",
        "group" => "▸",
        "text" => "T",
        "box" => "▭",
        "image" => "▧",
        "path" | "vector" => "◇",
        _ => "•",
    }
}

#[cfg(test)]
mod tests {
    use super::layer_command_for_key;

    const COMMANDS_YAML: &str = include_str!("../../../commands.yaml");

    fn windows_shortcut(command_id: &str) -> Option<&'static str> {
        let section_header = format!("- id: {command_id}");
        let mut in_section = false;
        for line in COMMANDS_YAML.lines() {
            if line.starts_with("- id: ") {
                in_section = line == section_header;
                continue;
            }
            if in_section {
                if let Some(value) = line.trim().strip_prefix("keys: { win: \"") {
                    return value.strip_suffix("\" }");
                }
            }
        }
        None
    }

    #[test]
    fn select_all_and_group_focus_shortcuts_match_catalog() {
        assert_eq!(windows_shortcut("edit.select_all"), Some("Ctrl+A"));
        assert_eq!(windows_shortcut("object.group"), Some("Ctrl+G"));
        assert_eq!(windows_shortcut("object.ungroup"), Some("Ctrl+Shift+G"));
        assert_eq!(
            layer_command_for_key("a", true, false, false),
            Some("edit.select_all")
        );
        assert_eq!(
            layer_command_for_key("g", true, false, false),
            Some("object.group")
        );
        assert_eq!(
            layer_command_for_key("g", true, true, false),
            Some("object.ungroup")
        );
        assert_eq!(layer_command_for_key("a", true, false, true), None);
    }
}
