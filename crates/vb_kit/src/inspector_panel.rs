//! VellumBench 属性检查器：只通过会话投影读取和写回文档几何。
//!
//! 受控数值框复用 Sable NumberField/Binding；文档仍只由 ProjectSession 持有。

use sable::gpui::{
    div, px, App, AppContext as _, Context, ElementId, Entity, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, WeakEntity, Window,
};
use sable::gpui_component::scroll::ScrollableElement as _;
use sable::widgets::controls::button::{Button, ButtonSize, ButtonVariant};
use sable::widgets::controls::text_field::TextField;
use sable::widgets::prelude::{h_flex, section, v_flex, Binding, NumberField, PropertyRow};
use sable::widgets::theme::theme;
use sable::widgets::tokens::FONT_SIZE_BODY;
use vb_session::command::CommandOutcome;
use vb_session::i18n::t;
use vb_session::project::{CanvasRect, InspectorSnapshot, ProjectSession};

use crate::tokens::{radius, space};

const MAX_GEOMETRY: f64 = 1_000_000.0;
const ALIGNMENT_COMMANDS: &[(&str, &str, bool)] = &[
    ("align.to_selection", "cmd-align-to-selection", true),
    ("align.to_key_object", "cmd-align-to-key-object", true),
    ("align.to_artboard", "cmd-align-to-artboard", true),
    ("align.left", "cmd-align-left", false),
    ("align.hcenter", "cmd-align-hcenter", false),
    ("align.right", "cmd-align-right", false),
    ("align.top", "cmd-align-top", false),
    ("align.vcenter", "cmd-align-vcenter", false),
    ("align.bottom", "cmd-align-bottom", false),
];

/// 右侧属性面板；单选字段经可撤销命令写回同一个 ProjectSession。
pub struct ProjectInspectorPanel {
    session: Option<Entity<ProjectSession>>,
    error: Option<String>,
    snapshot: InspectorSnapshot,
    field_key: Option<(String, bool)>,
    fields: Option<[Entity<NumberField>; 4]>,
    name_field_key: Option<(String, String, bool)>,
    name_field: Option<Entity<TextField>>,
    apply_name_button: Option<Entity<Button>>,
    text_field_key: Option<(String, String, bool, bool)>,
    text_field: Option<Entity<TextField>>,
    apply_text_button: Option<Entity<Button>>,
    fill_field_key: Option<(String, String, bool, bool)>,
    fill_field: Option<Entity<TextField>>,
    apply_fill_button: Option<Entity<Button>>,
    alignment_target_key: Option<String>,
    alignment_buttons: Vec<(String, Entity<Button>)>,
    status: Option<String>,
}

impl ProjectInspectorPanel {
    /// 构造并订阅共享会话的单选属性投影。
    pub fn new(
        session: Option<Entity<ProjectSession>>,
        error: Option<String>,
        cx: &mut App,
    ) -> Entity<Self> {
        let panel = cx.new(|_| Self {
            session: session.clone(),
            error,
            snapshot: InspectorSnapshot::default(),
            field_key: None,
            fields: None,
            name_field_key: None,
            name_field: None,
            apply_name_button: None,
            text_field_key: None,
            text_field: None,
            apply_text_button: None,
            fill_field_key: None,
            fill_field: None,
            apply_fill_button: None,
            alignment_target_key: None,
            alignment_buttons: Vec::new(),
            status: None,
        });
        panel.update(cx, |panel, cx| panel.refresh_snapshot(cx));
        if let Some(session) = session {
            panel.update(cx, |_, cx| {
                cx.observe(&session, |panel, _, cx| {
                    panel.refresh_snapshot(cx);
                    cx.notify();
                })
                .detach();
            });
        }
        panel
    }

    fn refresh_snapshot(&mut self, cx: &App) {
        if let Some(session) = self.session.as_ref() {
            self.snapshot = session.read(cx).inspector_snapshot();
        } else {
            self.snapshot = InspectorSnapshot::default();
        }
    }

    fn ensure_fields(&mut self, cx: &mut Context<Self>) {
        let key = self
            .snapshot
            .selected
            .as_ref()
            .map(|selected| (selected.sid.clone(), selected.locked));
        if key == self.field_key {
            return;
        }
        self.field_key = key.clone();
        self.fields = None;
        self.status = None;
        let (Some((sid, locked)), Some(session)) = (key, self.session.clone()) else {
            return;
        };
        let panel = cx.entity().downgrade();
        let x = number_field(
            session.clone(),
            panel.clone(),
            sid.clone(),
            GeometryAxis::X,
            locked,
            cx,
        );
        let y = number_field(
            session.clone(),
            panel.clone(),
            sid.clone(),
            GeometryAxis::Y,
            locked,
            cx,
        );
        let width = number_field(
            session.clone(),
            panel.clone(),
            sid.clone(),
            GeometryAxis::Width,
            locked,
            cx,
        );
        let height = number_field(session, panel, sid, GeometryAxis::Height, locked, cx);
        self.fields = Some([x, y, width, height]);
    }

    fn ensure_name_field(&mut self, cx: &mut Context<Self>) {
        let key = self
            .snapshot
            .selected
            .as_ref()
            .map(|selected| (selected.sid.clone(), selected.name.clone(), selected.locked));
        if key == self.name_field_key {
            return;
        }
        self.name_field_key = key.clone();
        self.name_field = None;
        self.apply_name_button = None;
        let (Some((sid, initial, locked)), Some(session)) = (key, self.session.clone()) else {
            return;
        };
        let submitted_sid = sid.clone();
        let panel = cx.entity().downgrade();
        let name_field = cx.new(|_| {
            TextField::new(
                ElementId::Name(format!("vb-inspector-name-{sid}").into()),
                initial,
            )
            .placeholder(t("ui-inspector-name-placeholder"))
            .disabled(locked)
            .on_submit(move |value, _window, app| {
                let result = session.update(app, |session, session_cx| {
                    let result = session.set_selected_name(&submitted_sid, value);
                    if result.is_ok() {
                        session_cx.notify();
                    }
                    result
                });
                let status = result.err().map(|error| error.to_string());
                let _ = panel.update(app, |panel, cx| {
                    panel.status = status;
                    cx.notify();
                });
            })
        });
        if !locked {
            let weak_name_field = name_field.downgrade();
            self.apply_name_button = Some(cx.new(|_| {
                Button::new("inspector-name-apply", t("ui-inspector-name-apply"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Compact)
                    .on_press(move |_, window, app| {
                        if let Some(name_field) = weak_name_field.upgrade() {
                            name_field.update(app, |field, cx| field.submit(window, cx));
                        }
                    })
            }));
        }
        self.name_field = Some(name_field);
    }

    fn ensure_text_field(&mut self, cx: &mut Context<Self>) {
        let key = self.snapshot.selected.as_ref().and_then(|selected| {
            selected.text.as_ref().map(|text| {
                (
                    selected.sid.clone(),
                    text.clone(),
                    selected.locked,
                    selected.text_editable,
                )
            })
        });
        if key == self.text_field_key {
            return;
        }
        self.text_field_key = key.clone();
        self.text_field = None;
        self.apply_text_button = None;
        let (Some((sid, initial, locked, editable)), Some(session)) = (key, self.session.clone())
        else {
            return;
        };
        let submitted_sid = sid.clone();
        let panel = cx.entity().downgrade();
        let text_field = cx.new(|_| {
            TextField::new(
                ElementId::Name(format!("vb-inspector-text-{sid}").into()),
                initial,
            )
            .placeholder(t("ui-inspector-text-placeholder"))
            .disabled(locked || !editable)
            .on_submit(move |value, _window, app| {
                let result = session.update(app, |session, session_cx| {
                    let result = session.set_selected_text(&submitted_sid, value);
                    if result.is_ok() {
                        session_cx.notify();
                    }
                    result
                });
                let status = result.err().map(|error| error.to_string());
                let _ = panel.update(app, |panel, cx| {
                    panel.status = status;
                    cx.notify();
                });
            })
        });
        if !locked && editable {
            let weak_text_field = text_field.downgrade();
            self.apply_text_button = Some(cx.new(|_| {
                Button::new("inspector-text-apply", t("ui-inspector-text-apply"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Compact)
                    .on_press(move |_, window, app| {
                        if let Some(text_field) = weak_text_field.upgrade() {
                            text_field.update(app, |field, cx| field.submit(window, cx));
                        }
                    })
            }));
        }
        self.text_field = Some(text_field);
    }

    fn ensure_fill_field(&mut self, cx: &mut Context<Self>) {
        let key = self.snapshot.selected.as_ref().and_then(|selected| {
            matches!(selected.kind, "box" | "text").then(|| {
                (
                    selected.sid.clone(),
                    selected.fill.clone().unwrap_or_default(),
                    selected.locked,
                    selected.fill_editable,
                )
            })
        });
        if key == self.fill_field_key {
            return;
        }
        self.fill_field_key = key.clone();
        self.fill_field = None;
        self.apply_fill_button = None;
        let (Some((sid, initial, locked, editable)), Some(session)) = (key, self.session.clone())
        else {
            return;
        };
        let submitted_sid = sid.clone();
        let panel = cx.entity().downgrade();
        let fill_field = cx.new(|_| {
            TextField::new(
                ElementId::Name(format!("vb-inspector-fill-{sid}").into()),
                initial,
            )
            .placeholder(t("ui-inspector-fill-placeholder"))
            .disabled(locked || !editable)
            .on_submit(move |value, _window, app| {
                let result = session.update(app, |session, session_cx| {
                    let result = session.set_selected_fill(&submitted_sid, value);
                    if result.is_ok() {
                        session_cx.notify();
                    }
                    result
                });
                let status = result.err().map(|error| error.to_string());
                let _ = panel.update(app, |panel, cx| {
                    panel.status = status;
                    cx.notify();
                });
            })
        });
        if !locked && editable {
            let weak_fill_field = fill_field.downgrade();
            self.apply_fill_button = Some(cx.new(|_| {
                Button::new("inspector-fill-apply", t("ui-inspector-fill-apply"))
                    .variant(ButtonVariant::Secondary)
                    .size(ButtonSize::Compact)
                    .on_press(move |_, window, app| {
                        if let Some(fill_field) = weak_fill_field.upgrade() {
                            fill_field.update(app, |field, cx| field.submit(window, cx));
                        }
                    })
            }));
        }
        self.fill_field = Some(fill_field);
    }

    fn ensure_alignment_buttons(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            self.alignment_target_key = None;
            self.alignment_buttons.clear();
            return;
        };
        let active_target = session.read(cx).align_target_catalog_id().to_string();
        if self.alignment_target_key.as_deref() == Some(active_target.as_str())
            && self.alignment_buttons.len() == ALIGNMENT_COMMANDS.len()
        {
            return;
        }
        self.alignment_target_key = Some(active_target.clone());
        let panel = cx.entity().downgrade();
        let mut buttons = Vec::with_capacity(ALIGNMENT_COMMANDS.len());
        for (index, &(command_id, label_id, is_target)) in ALIGNMENT_COMMANDS.iter().enumerate() {
            let command = command_id.to_string();
            let session = session.clone();
            let panel = panel.clone();
            let variant = if is_target && command_id == active_target {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Secondary
            };
            let button = cx.new(|_| {
                Button::new(("inspector-align", index as u32), t(label_id))
                    .variant(variant)
                    .size(ButtonSize::Compact)
                    .on_press(move |_, _, app| {
                        let outcome = session.update(app, |session, session_cx| {
                            let outcome = session.dispatch_catalog(&command);
                            if outcome.is_applied() {
                                session_cx.notify();
                            }
                            outcome
                        });
                        let status = match outcome {
                            CommandOutcome::Applied => None,
                            CommandOutcome::NotRun { reason } => Some(reason),
                        };
                        let _ = panel.update(app, |panel, cx| {
                            panel.status = status;
                            cx.notify();
                        });
                    })
            });
            buttons.push((command_id.to_string(), button));
        }
        self.alignment_buttons = buttons;
    }
}

impl Render for ProjectInspectorPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_fields(cx);
        self.ensure_name_field(cx);
        self.ensure_text_field(cx);
        self.ensure_fill_field(cx);
        self.ensure_alignment_buttons(cx);
        let colors = theme(cx).colors;
        let header = h_flex()
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
                    .child(t("ui-shell-panel-inspector")),
            )
            .child(
                div()
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.text_disabled)
                    .child(self.snapshot.selected_count.to_string()),
            );

        let body = if let Some(error) = self.error.clone() {
            v_flex()
                .flex_1()
                .min_h_0()
                .p(px(space::S2))
                .text_size(px(FONT_SIZE_BODY))
                .text_color(colors.warning)
                .child(error)
                .into_any_element()
        } else if self.snapshot.selected_count == 0 {
            empty_message(t("ui-inspector-empty"), colors.text_secondary)
        } else if self.snapshot.selected_count > 1 {
            empty_message(t("ui-inspector-multi-selection"), colors.text_secondary)
        } else if let Some(selected) = self.snapshot.selected.clone() {
            self.render_single_selection(selected, colors)
        } else {
            empty_message(t("ui-inspector-empty"), colors.text_secondary)
        };

        let mut panel = v_flex()
            .id("vb-project-inspector")
            .size_full()
            .min_h_0()
            .gap(px(space::S1))
            .p(px(space::S2))
            .rounded(px(radius::SM))
            .bg(colors.surface_1)
            .child(header)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap(px(space::S1))
                    .overflow_y_scrollbar()
                    .child(self.render_alignment_controls())
                    .child(body),
            );
        if let Some(status) = self.status.clone() {
            panel = panel.child(
                div()
                    .px(px(space::S2))
                    .py(px(space::S1))
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.warning)
                    .child(status),
            );
        }
        panel
    }
}

impl ProjectInspectorPanel {
    fn render_alignment_controls(&self) -> sable::gpui::AnyElement {
        if self.alignment_buttons.is_empty() {
            return div().into_any_element();
        }
        let target_controls = v_flex().gap(px(space::S1)).children(
            self.alignment_buttons
                .iter()
                .take(3)
                .map(|(_, button)| button.clone()),
        );
        let action_controls = v_flex().gap(px(space::S1)).children(
            self.alignment_buttons
                .iter()
                .skip(3)
                .map(|(_, button)| button.clone()),
        );
        v_flex()
            .gap(px(space::S2))
            .child(section(t("ui-inspector-alignment-target"), target_controls))
            .child(section(t("ui-inspector-alignment"), action_controls))
            .into_any_element()
    }

    fn render_single_selection(
        &self,
        selected: vb_session::project::SelectedNodeSnapshot,
        colors: sable::widgets::tokens::ColorTokens,
    ) -> sable::gpui::AnyElement {
        let Some(fields) = self.fields.as_ref() else {
            return empty_message(t("ui-inspector-empty"), colors.text_secondary);
        };
        let transform = v_flex()
            .gap(px(space::S1))
            .child(PropertyRow::new(t("ui-inspector-x")).control(fields[0].clone()))
            .child(PropertyRow::new(t("ui-inspector-y")).control(fields[1].clone()))
            .child(PropertyRow::new(t("ui-inspector-width")).control(fields[2].clone()))
            .child(PropertyRow::new(t("ui-inspector-height")).control(fields[3].clone()));
        let mut name_control = h_flex().items_center().gap(px(space::S1));
        if let Some(name_field) = self.name_field.clone() {
            name_control = name_control.child(name_field);
        } else {
            name_control =
                name_control.child(static_value(selected.name.clone(), colors.text_primary));
        }
        if let Some(button) = self.apply_name_button.clone() {
            name_control = name_control.child(button);
        }
        let mut fill_control = h_flex().items_center().gap(px(space::S1));
        if let Some(fill_field) = self.fill_field.clone() {
            fill_control = fill_control.child(fill_field);
        } else {
            fill_control = fill_control.child(static_value(
                selected.fill.clone().unwrap_or_else(|| "—".to_string()),
                colors.text_primary,
            ));
        }
        if let Some(button) = self.apply_fill_button.clone() {
            fill_control = fill_control.child(button);
        }
        let fill_readonly = selected.fill.is_some() && !selected.fill_editable && !selected.locked;
        let mut appearance = v_flex()
            .gap(px(space::S1))
            .child(PropertyRow::new(t("ui-inspector-name")).control(name_control))
            .child(
                PropertyRow::new(t("ui-inspector-type"))
                    .control(static_value(selected.kind, colors.text_secondary)),
            )
            .child(
                PropertyRow::new(if selected.kind == "text" {
                    t("ui-inspector-text-color")
                } else {
                    t("ui-inspector-fill")
                })
                .control(fill_control),
            );
        if fill_readonly {
            appearance = appearance.child(static_value(
                t("ui-inspector-fill-readonly"),
                colors.text_disabled,
            ));
        }
        let mut content = v_flex()
            .flex_1()
            .min_h_0()
            .gap(px(space::S3))
            .child(section(t("ui-inspector-transform"), transform))
            .child(section(t("ui-inspector-appearance"), appearance));
        if selected.text.is_some() {
            let mut text_content = v_flex().gap(px(space::S1));
            if let Some(text_field) = self.text_field.clone() {
                text_content = text_content.child(text_field);
            }
            if let Some(button) = self.apply_text_button.clone() {
                text_content = text_content.child(button);
            }
            if !selected.text_editable {
                text_content = text_content.child(static_value(
                    t("ui-inspector-rich-text-readonly"),
                    colors.text_disabled,
                ));
            }
            content = content.child(section(t("ui-inspector-content"), text_content));
        }
        if selected.locked {
            content = content.child(
                div()
                    .text_size(px(FONT_SIZE_BODY))
                    .text_color(colors.text_disabled)
                    .child(t("ui-inspector-locked")),
            );
        }
        content.into_any_element()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeometryAxis {
    X,
    Y,
    Width,
    Height,
}

impl GeometryAxis {
    fn key(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Width => "width",
            Self::Height => "height",
        }
    }

    fn range(self) -> (f64, f64) {
        match self {
            Self::X | Self::Y => (-MAX_GEOMETRY, MAX_GEOMETRY),
            Self::Width | Self::Height => (0.01, MAX_GEOMETRY),
        }
    }

    fn get(self, rect: CanvasRect) -> f64 {
        match self {
            Self::X => rect.x,
            Self::Y => rect.y,
            Self::Width => rect.width,
            Self::Height => rect.height,
        }
    }

    fn set(self, rect: &mut CanvasRect, value: f64) {
        match self {
            Self::X => rect.x = value,
            Self::Y => rect.y = value,
            Self::Width => rect.width = value,
            Self::Height => rect.height = value,
        }
    }
}

fn number_field(
    session: Entity<ProjectSession>,
    panel: WeakEntity<ProjectInspectorPanel>,
    sid: String,
    axis: GeometryAxis,
    locked: bool,
    cx: &mut Context<ProjectInspectorPanel>,
) -> Entity<NumberField> {
    let read_session = session.clone();
    let read_sid = sid.clone();
    let write_sid = sid.clone();
    let binding = Binding::new(
        move |app: &App| {
            read_session
                .read(app)
                .inspector_snapshot()
                .selected
                .filter(|selected| selected.sid == read_sid)
                .map(|selected| axis.get(selected.geometry))
                .unwrap_or_default()
        },
        move |value: f64, app: &mut App| {
            let result = session.update(app, |session, session_cx| {
                let Some(selected) = session
                    .inspector_snapshot()
                    .selected
                    .filter(|selected| selected.sid == write_sid)
                else {
                    return Ok(false);
                };
                let mut geometry = selected.geometry;
                axis.set(&mut geometry, value);
                let result = session.set_selected_geometry(geometry);
                if result.is_ok() {
                    session_cx.notify();
                }
                result
            });
            let status = match result {
                Ok(_) => None,
                Err(error) => Some(error.to_string()),
            };
            let _ = panel.update(app, |panel, cx| {
                panel.status = status;
                cx.notify();
            });
        },
    );
    let (min, max) = axis.range();
    cx.new(|_| {
        NumberField::new(binding)
            .range(min, max)
            .step(1.0)
            .unit("px")
            .disabled(locked)
            .element_id(ElementId::Name(
                format!("vb-inspector-{}-{sid}", axis.key()).into(),
            ))
    })
}

fn static_value(
    value: impl Into<sable::gpui::SharedString>,
    color: sable::gpui::Hsla,
) -> impl IntoElement {
    div()
        .min_w_0()
        .text_size(px(FONT_SIZE_BODY))
        .text_color(color)
        .child(value.into())
}

fn empty_message(
    value: impl Into<sable::gpui::SharedString>,
    color: sable::gpui::Hsla,
) -> sable::gpui::AnyElement {
    v_flex()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .p(px(space::S3))
        .text_size(px(FONT_SIZE_BODY))
        .text_color(color)
        .child(value.into())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{CanvasRect, GeometryAxis, ALIGNMENT_COMMANDS};

    #[test]
    fn inspector_alignment_buttons_match_catalog_and_localizations() {
        let catalog = include_str!("../../../commands.yaml");
        let english = include_str!("../../../i18n/en.ftl");
        let chinese = include_str!("../../../i18n/zh.ftl");
        assert_eq!(ALIGNMENT_COMMANDS.len(), 9);
        for (command_id, label_id, _) in ALIGNMENT_COMMANDS {
            assert!(
                catalog.contains(&format!("- id: {command_id}")),
                "alignment button command is missing from commands.yaml: {command_id}"
            );
            let label = format!("{label_id} =");
            assert!(
                english.contains(&label) && chinese.contains(&label),
                "alignment button translation is missing: {label_id}"
            );
        }
    }

    #[test]
    fn inspector_size_fields_disallow_nonpositive_geometry() {
        assert_eq!(GeometryAxis::Width.range(), (0.01, 1_000_000.0));
        assert_eq!(GeometryAxis::Height.range(), (0.01, 1_000_000.0));
        assert_eq!(GeometryAxis::X.range(), (-1_000_000.0, 1_000_000.0));
        assert_eq!(GeometryAxis::Y.range(), (-1_000_000.0, 1_000_000.0));
    }

    #[test]
    fn axis_binding_updates_only_the_requested_geometry_value() {
        let mut rect = CanvasRect {
            x: 2.0,
            y: 3.0,
            width: 40.0,
            height: 50.0,
        };
        GeometryAxis::Y.set(&mut rect, 12.0);
        assert_eq!(
            rect,
            CanvasRect {
                x: 2.0,
                y: 12.0,
                width: 40.0,
                height: 50.0,
            }
        );
    }
}
