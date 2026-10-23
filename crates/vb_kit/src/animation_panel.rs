//! R4 animation controls projected from the shared ProjectSession.
//!
//! The panel owns no document model: commands and playhead state live in the
//! session, while the canvas observes `active_canvas_snapshot_at`.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use sable::gpui::{
    div, px, AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    WeakEntity, Window,
};
use sable::gpui_component::menu::{ContextMenuExt, PopupMenuItem};
use sable::widgets::controls::button::{Button, ButtonSize, ButtonVariant};
use sable::widgets::controls::slider::Slider;
use sable::widgets::controls::text_field::TextField;
use sable::widgets::prelude::{h_flex, v_flex, Binding, NumberField};
use sable::widgets::theme::theme;
use sable::widgets::tokens::ColorTokens;
use vb_animation::preset::AnimationPresetRef;
use vb_animation::timeline::{keyframe_offset_permille, AnimModel, AnimSource, Easing, TrackProp};
use vb_session::command::CommandOutcome;
use vb_session::i18n::{t, t_args, FluentValue};
use vb_session::project::{
    AnimationFrameRate, AnimationKeyframeSelection, AnimationSnapshot, KeyframeOffsetDelta,
    ProjectSession,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKind {
    None,
    Timeline,
    Foreign,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ControlState {
    source: SourceKind,
    selected: bool,
    locked: bool,
    playing: bool,
    looping: bool,
    can_stop: bool,
    selected_keyframe_count: usize,
}

impl ControlState {
    fn from_snapshot(snapshot: &AnimationSnapshot) -> Self {
        let selected = snapshot.selected.as_ref();
        let source = match selected.map(|item| &item.source) {
            Some(AnimSource::Timeline(_)) => SourceKind::Timeline,
            Some(AnimSource::Foreign(_)) => SourceKind::Foreign,
            Some(AnimSource::None) | None => SourceKind::None,
        };
        Self {
            source,
            selected: selected.is_some() && snapshot.selected_count == 1,
            locked: selected.is_some_and(|item| item.locked),
            playing: snapshot.playing,
            looping: snapshot.looping,
            can_stop: snapshot.playing || snapshot.playhead_seconds > 0.0,
            selected_keyframe_count: snapshot.selected_keyframes.len(),
        }
    }
}

struct AnimationButtons {
    state: ControlState,
    create: Entity<Button>,
    play: Entity<Button>,
    stop: Entity<Button>,
    loop_toggle: Entity<Button>,
    clear: Entity<Button>,
    keyframe_add: Entity<Button>,
    keyframe_delete: Entity<Button>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameSnapSignature {
    enabled: bool,
    frame_rate: AnimationFrameRate,
    controls_enabled: bool,
}

struct FrameSnapButtons {
    signature: FrameSnapSignature,
    toggle: Entity<Button>,
    rates: Vec<Entity<Button>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct KeyframeButtonState {
    sid: String,
    track: TrackProp,
    offset_permille: u16,
    selected: bool,
    disabled: bool,
}

struct KeyframeButton {
    state: KeyframeButtonState,
    button: Entity<Button>,
}

#[derive(Clone, Debug)]
struct KeyframeDragState {
    marker: AnimationKeyframeSelection,
    start_x: f64,
    additive: bool,
    copy: bool,
    active: bool,
    delta: Option<KeyframeOffsetDelta>,
    source_markers: Vec<AnimationKeyframeSelection>,
    preview_markers: Vec<AnimationKeyframeSelection>,
    preview_copy_markers: Vec<AnimationKeyframeSelection>,
}

#[derive(Clone, Debug, PartialEq)]
struct EasingButtonSignature {
    selected_keys: Vec<AnimationKeyframeSelection>,
    active: Option<Easing>,
    enabled: bool,
}

struct EasingButtons {
    signature: EasingButtonSignature,
    buttons: Vec<Entity<Button>>,
}

#[derive(Clone, Debug, PartialEq)]
struct BezierSignature {
    selected_keys: Vec<AnimationKeyframeSelection>,
    active: Option<Easing>,
    enabled: bool,
}

struct BezierControls {
    signature: BezierSignature,
    draft: Rc<RefCell<[f64; 4]>>,
    fields: Vec<Entity<NumberField>>,
    apply: Entity<Button>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PresetControlSignature {
    selected_sid: Option<String>,
    can_save: bool,
    can_apply: bool,
    preset_refs: Vec<AnimationPresetRef>,
}

struct PresetRowButtons {
    reference: AnimationPresetRef,
    apply: Entity<Button>,
    delete: Entity<Button>,
}

struct PresetControls {
    signature: PresetControlSignature,
    name_field: Entity<TextField>,
    save: Entity<Button>,
    rows: Vec<PresetRowButtons>,
}

#[derive(Clone, Copy, Debug)]
struct BezierHandleDrag {
    handle: usize,
    pointer: [f64; 2],
    values: [f64; 4],
    y_range: (f64, f64),
}

#[derive(Clone, Copy)]
enum KeyframeContextAction {
    Delete,
    SetEasing(Easing),
    NudgeCatalog(&'static str),
}

/// Bottom-dock timeline controls; every edit routes through ProjectSession.
pub struct ProjectAnimationPanel {
    session: Option<Entity<ProjectSession>>,
    error: Option<String>,
    slider: Option<Entity<Slider>>,
    buttons: Option<AnimationButtons>,
    frame_snap_buttons: Option<FrameSnapButtons>,
    keyframe_buttons: Vec<KeyframeButton>,
    keyframe_drag: Option<KeyframeDragState>,
    suppressed_marker_click: Option<AnimationKeyframeSelection>,
    easing_buttons: Option<EasingButtons>,
    bezier_controls: Option<BezierControls>,
    preset_controls: Option<PresetControls>,
    bezier_drag: Option<BezierHandleDrag>,
    status: Option<String>,
    pumping: bool,
}

impl ProjectAnimationPanel {
    /// Create the panel and observe the shared document session.
    pub fn new(
        session: Option<Entity<ProjectSession>>,
        error: Option<String>,
        cx: &mut App,
    ) -> Entity<Self> {
        let panel = cx.new(|_| Self {
            session: session.clone(),
            error,
            slider: None,
            buttons: None,
            frame_snap_buttons: None,
            keyframe_buttons: Vec::new(),
            keyframe_drag: None,
            suppressed_marker_click: None,
            easing_buttons: None,
            bezier_controls: None,
            preset_controls: None,
            bezier_drag: None,
            status: None,
            pumping: false,
        });

        let panel_weak = panel.downgrade();
        let slider_session = session.clone();
        let slider = cx.new(|_| {
            Slider::new("animation-playhead", 0.0)
                .range(0.0, 100.0)
                .step(0.1)
                .label_slot(t("ui-animation-playhead"))
                .on_change(move |percent, app| {
                    let Some(session) = slider_session.as_ref() else {
                        return;
                    };
                    let result = session.update(app, |session, session_cx| {
                        let Some(duration) = session.animation_snapshot().duration_seconds else {
                            return (false, None);
                        };
                        match session.set_animation_playhead_seconds(duration * percent / 100.0) {
                            Ok(changed) => {
                                if changed {
                                    session_cx.notify();
                                }
                                (changed, None)
                            }
                            Err(error) => (false, Some(error.to_string())),
                        }
                    });
                    let (_, status) = result;
                    if let Some(status) = status {
                        let _ = panel_weak.update(app, |panel, panel_cx| {
                            panel.status = Some(status);
                            panel_cx.notify();
                        });
                    }
                })
        });
        panel.update(cx, |panel, _| panel.slider = Some(slider));

        if let Some(session) = session {
            panel.update(cx, |_, panel_cx| {
                panel_cx
                    .observe(&session, |panel, session, panel_cx| {
                        let (snapshot, frame_snap_enabled, frame_rate) = {
                            let session = session.read(panel_cx);
                            (
                                session.animation_snapshot(),
                                session.animation_frame_snap_enabled(),
                                session.animation_frame_rate(),
                            )
                        };
                        panel.sync_slider(&snapshot, panel_cx);
                        panel.ensure_buttons(&snapshot, panel_cx);
                        panel.ensure_frame_snap_buttons(
                            &snapshot,
                            frame_snap_enabled,
                            frame_rate,
                            panel_cx,
                        );
                        if snapshot.playing {
                            panel.start_playback_pump(panel_cx);
                        }
                        panel_cx.notify();
                    })
                    .detach();
            });
        }
        panel
    }

    fn command_button(
        &self,
        command_id: &'static str,
        label: String,
        button_id: &'static str,
        variant: ButtonVariant,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Entity<Button> {
        let session = self.session.clone();
        let panel = cx.entity().downgrade();
        cx.new(|_| {
            Button::new(button_id, label)
                .size(ButtonSize::Compact)
                .variant(variant)
                .disabled(disabled)
                .on_press(move |_, _, app| {
                    let Some(session) = session.as_ref() else {
                        return;
                    };
                    let outcome = session.update(app, |session, session_cx| {
                        let outcome = session.dispatch_catalog(command_id);
                        session_cx.notify();
                        outcome
                    });
                    let (snapshot, frame_snap_enabled, frame_rate) = {
                        let current = session.read(app);
                        (
                            current.animation_snapshot(),
                            current.animation_frame_snap_enabled(),
                            current.animation_frame_rate(),
                        )
                    };
                    let _panel_update = panel.update(app, |panel, panel_cx| {
                        panel.status = match outcome {
                            CommandOutcome::Applied => Some(t("ui-animation-command-applied")),
                            CommandOutcome::NotRun { reason } => Some(reason),
                        };
                        panel.sync_slider(&snapshot, panel_cx);
                        panel.ensure_buttons(&snapshot, panel_cx);
                        panel.ensure_frame_snap_buttons(
                            &snapshot,
                            frame_snap_enabled,
                            frame_rate,
                            panel_cx,
                        );
                        if snapshot.playing {
                            panel.start_playback_pump(panel_cx);
                        }
                        panel_cx.notify();
                    });
                })
        })
    }

    fn ensure_buttons(&mut self, snapshot: &AnimationSnapshot, cx: &mut Context<Self>) {
        let state = ControlState::from_snapshot(snapshot);
        if self
            .buttons
            .as_ref()
            .is_some_and(|buttons| buttons.state == state)
        {
            return;
        }
        let can_create = state.selected && !state.locked && state.source == SourceKind::None;
        let can_play = state.selected && !state.locked && state.source == SourceKind::Timeline;
        let can_clear = can_play;
        let can_stop = can_play && state.can_stop;
        let can_add_keyframe = can_play;
        let can_delete_keyframe = can_play && state.selected_keyframe_count > 0;
        let buttons = AnimationButtons {
            state,
            create: self.command_button(
                "anim.create",
                t("ui-animation-create-preset"),
                "animation-create",
                ButtonVariant::Secondary,
                !can_create,
                cx,
            ),
            play: self.command_button(
                "anim.play_toggle",
                if state.playing {
                    t("ui-animation-pause")
                } else {
                    t("ui-animation-play")
                },
                "animation-play-toggle",
                if state.playing {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Secondary
                },
                !can_play,
                cx,
            ),
            stop: self.command_button(
                "anim.stop",
                t("cmd-anim-stop"),
                "animation-stop",
                ButtonVariant::Ghost,
                !can_stop,
                cx,
            ),
            loop_toggle: self.command_button(
                "anim.loop_toggle",
                t("cmd-anim-loop-toggle"),
                "animation-loop-toggle",
                if state.looping {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                },
                !can_play,
                cx,
            ),
            clear: self.command_button(
                "anim.clear",
                t("cmd-anim-clear"),
                "animation-clear",
                ButtonVariant::Danger,
                !can_clear,
                cx,
            ),
            keyframe_add: self.command_button(
                "anim.keyframe_add",
                t("cmd-anim-keyframe-add"),
                "animation-keyframe-add",
                ButtonVariant::Secondary,
                !can_add_keyframe,
                cx,
            ),
            keyframe_delete: self.command_button(
                "anim.keyframe_delete",
                t("cmd-anim-keyframe-delete"),
                "animation-keyframe-delete",
                ButtonVariant::Danger,
                !can_delete_keyframe,
                cx,
            ),
        };
        self.buttons = Some(buttons);
    }

    fn ensure_frame_snap_buttons(
        &mut self,
        snapshot: &AnimationSnapshot,
        enabled: bool,
        frame_rate: AnimationFrameRate,
        cx: &mut Context<Self>,
    ) {
        let controls_enabled = snapshot.selected.as_ref().is_some_and(|selected| {
            snapshot.selected_count == 1
                && !selected.locked
                && matches!(&selected.source, AnimSource::Timeline(_))
        });
        let signature = FrameSnapSignature {
            enabled,
            frame_rate,
            controls_enabled,
        };
        if self
            .frame_snap_buttons
            .as_ref()
            .is_some_and(|buttons| buttons.signature == signature)
        {
            return;
        }
        let Some(session) = self.session.clone() else {
            self.frame_snap_buttons = None;
            return;
        };
        let snap_session = session.clone();
        let snap_button = cx.new(|_| {
            Button::new("animation-frame-snap", t("ui-animation-snap-to-frames"))
                .size(ButtonSize::Compact)
                .variant(if enabled {
                    ButtonVariant::Primary
                } else {
                    ButtonVariant::Ghost
                })
                .disabled(!controls_enabled)
                .on_press(move |_, _, app| {
                    snap_session.update(app, |session, session_cx| {
                        if session.set_animation_frame_snap_enabled(!enabled) {
                            session_cx.notify();
                        }
                    });
                })
        });
        let rates = AnimationFrameRate::ALL
            .into_iter()
            .map(|rate| {
                let rate_session = session.clone();
                cx.new(|_| {
                    Button::new(
                        ("animation-frame-rate", u32::from(rate.fps())),
                        format!("{} fps", rate.fps()),
                    )
                    .size(ButtonSize::Compact)
                    .variant(if rate == frame_rate {
                        ButtonVariant::Primary
                    } else {
                        ButtonVariant::Secondary
                    })
                    .disabled(!controls_enabled)
                    .on_press(move |_, _, app| {
                        rate_session.update(app, |session, session_cx| {
                            if session.set_animation_frame_rate(rate) {
                                session_cx.notify();
                            }
                        });
                    })
                })
            })
            .collect();
        self.frame_snap_buttons = Some(FrameSnapButtons {
            signature,
            toggle: snap_button,
            rates,
        });
    }

    fn ensure_keyframe_buttons(&mut self, snapshot: &AnimationSnapshot, cx: &mut Context<Self>) {
        let states = keyframe_button_states(snapshot);
        if self
            .keyframe_buttons
            .iter()
            .map(|button| button.state.clone())
            .eq(states.iter().cloned())
        {
            return;
        }
        let Some(session) = self.session.clone() else {
            self.keyframe_buttons.clear();
            return;
        };
        let panel = cx.entity().downgrade();
        self.keyframe_buttons = states
            .into_iter()
            .map(|state| {
                let sid = state.sid.clone();
                let track = state.track;
                let offset_permille = state.offset_permille;
                let marker = AnimationKeyframeSelection {
                    sid: sid.clone(),
                    track,
                    offset_permille,
                };
                let button_id = ElementId::Name(
                    format!(
                        "animation-keyframe-{sid}-{}-{offset_permille}",
                        track_key(track)
                    )
                    .into(),
                );
                let label = format!("{:.1}%", f64::from(offset_permille) / 10.0);
                let button_session = session.clone();
                let button_panel = panel.clone();
                let selected = state.selected;
                let disabled = state.disabled;
                let button = cx.new(|_| {
                    Button::new(button_id, label)
                        .size(ButtonSize::Compact)
                        .variant(if selected {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .disabled(disabled)
                        .on_press(move |event, _, app| {
                            let mut suppress = false;
                            let _ = button_panel.update(app, |panel, _| {
                                if panel.suppressed_marker_click.as_ref() == Some(&marker) {
                                    panel.suppressed_marker_click = None;
                                    suppress = true;
                                } else if panel
                                    .keyframe_drag
                                    .as_ref()
                                    .is_some_and(|drag| drag.active && drag.marker == marker)
                                {
                                    suppress = true;
                                }
                            });
                            if suppress {
                                return;
                            }
                            let modifiers = event.modifiers();
                            let additive = modifiers.shift || modifiers.platform;
                            let result = button_session.update(app, |session, session_cx| {
                                let result = session.select_animation_keyframe(
                                    &marker.sid,
                                    marker.track,
                                    marker.offset_permille,
                                    additive,
                                );
                                if result.as_ref().is_ok_and(|changed| *changed) {
                                    session_cx.notify();
                                }
                                result
                            });
                            if let Err(error) = result {
                                let _ = button_panel.update(app, |panel, panel_cx| {
                                    panel.status = Some(error.to_string());
                                    panel_cx.notify();
                                });
                            }
                        })
                });
                KeyframeButton { state, button }
            })
            .collect();
    }

    fn begin_keyframe_drag(
        &mut self,
        marker: AnimationKeyframeSelection,
        start_x: f64,
        additive: bool,
        copy: bool,
    ) {
        self.suppressed_marker_click = None;
        self.keyframe_drag = Some(KeyframeDragState {
            marker,
            start_x,
            additive,
            copy,
            active: false,
            delta: None,
            source_markers: Vec::new(),
            preview_markers: Vec::new(),
            preview_copy_markers: Vec::new(),
        });
    }

    fn handle_keyframe_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut drag) = self.keyframe_drag.clone() else {
            return;
        };
        let pixel_delta = f64::from(event.position.x) - drag.start_x;
        if !pixel_delta.is_finite() {
            return;
        }
        if !drag.active && pixel_delta.abs() < 3.0 {
            return;
        }
        let Some(session) = self.session.clone() else {
            self.keyframe_drag = None;
            return;
        };
        if !drag.active {
            let selected = session.read(cx).animation_snapshot().selected_keyframes;
            if !selected.contains(&drag.marker) {
                let result = session.update(cx, |session, session_cx| {
                    let result = session.select_animation_keyframe(
                        &drag.marker.sid,
                        drag.marker.track,
                        drag.marker.offset_permille,
                        drag.additive,
                    );
                    if result.as_ref().is_ok_and(|changed| *changed) {
                        session_cx.notify();
                    }
                    result
                });
                if let Err(error) = result {
                    self.status = Some(error.to_string());
                    self.keyframe_drag = None;
                    cx.notify();
                    return;
                }
            }
            drag.source_markers = session.read(cx).animation_snapshot().selected_keyframes;
            if drag.source_markers.is_empty() {
                self.keyframe_drag = None;
                return;
            }
            drag.preview_markers = drag.source_markers.clone();
            drag.active = true;
            self.suppressed_marker_click = Some(drag.marker.clone());
        }

        match keyframe_drag_delta(pixel_delta) {
            Ok(Some(delta)) => {
                let preview = {
                    let session = session.read(cx);
                    session
                        .snap_selected_keyframe_delta(delta)
                        .and_then(|delta| {
                            let preview = if drag.copy {
                                session
                                    .preview_selected_keyframe_copy(delta)
                                    .map(|copies| (drag.source_markers.clone(), copies))
                            } else {
                                session
                                    .preview_selected_keyframe_move(delta)
                                    .map(|moved| (moved, Vec::new()))
                            };
                            preview.map(|(markers, copies)| (delta, markers, copies))
                        })
                };
                match preview {
                    Ok((delta, markers, copies)) => {
                        drag.delta = Some(delta);
                        drag.preview_markers = markers;
                        drag.preview_copy_markers = copies;
                        self.status = None;
                    }
                    Err(error) => {
                        drag.delta = None;
                        drag.preview_markers = drag.source_markers.clone();
                        drag.preview_copy_markers.clear();
                        self.status = Some(error.to_string());
                    }
                }
            }
            Ok(None) => {
                drag.delta = None;
                drag.preview_markers = drag.source_markers.clone();
                drag.preview_copy_markers.clear();
                self.status = None;
            }
            Err(error) => {
                drag.delta = None;
                drag.preview_markers = drag.source_markers.clone();
                drag.preview_copy_markers.clear();
                self.status = Some(error);
            }
        }
        self.keyframe_drag = Some(drag);
        cx.notify();
    }

    fn finish_keyframe_drag(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.keyframe_drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        self.suppressed_marker_click = Some(drag.marker);
        let Some(delta) = drag.delta else {
            cx.notify();
            return;
        };
        let Some(session) = self.session.clone() else {
            cx.notify();
            return;
        };
        let copy = drag.copy;
        let result: Result<usize, String> = session.update(cx, |session, session_cx| {
            let moved = if copy {
                session.duplicate_selected_keyframes_by(delta)
            } else {
                session.move_selected_keyframes_by(delta)
            }
            .map_err(|error| error.to_string())?;
            if moved > 0 {
                session_cx.notify();
            }
            Ok(moved)
        });
        self.status = match result {
            Ok(count) if count > 0 => Some(t("ui-animation-command-applied")),
            Ok(_) => None,
            Err(error) => Some(error.to_string()),
        };
        cx.notify();
    }

    fn ensure_easing_buttons(&mut self, snapshot: &AnimationSnapshot, cx: &mut Context<Self>) {
        let signature = EasingButtonSignature {
            selected_keys: snapshot.selected_keyframes.clone(),
            active: selected_easing(snapshot),
            enabled: easing_controls_enabled(snapshot),
        };
        if self
            .easing_buttons
            .as_ref()
            .is_some_and(|buttons| buttons.signature == signature)
        {
            return;
        }
        let Some(session) = self.session.clone() else {
            self.easing_buttons = None;
            return;
        };
        let panel = cx.entity().downgrade();
        let buttons = Easing::PRESETS
            .into_iter()
            .enumerate()
            .map(|(index, easing)| {
                let button_session = session.clone();
                let button_panel = panel.clone();
                let selected = signature.active == Some(easing);
                let disabled = !signature.enabled;
                cx.new(|_| {
                    Button::new(("animation-easing", index as u32), easing_label(easing))
                        .size(ButtonSize::Compact)
                        .variant(if selected {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Secondary
                        })
                        .disabled(disabled)
                        .on_press(move |_, _, app| {
                            let result = button_session.update(app, |session, session_cx| {
                                let result = session.set_selected_keyframe_easing(easing);
                                if result.as_ref().is_ok_and(|changed| *changed > 0) {
                                    session_cx.notify();
                                }
                                result
                            });
                            if let Err(error) = result {
                                let message = error.to_string();
                                let _ = button_panel.update(app, |panel, panel_cx| {
                                    panel.status = Some(message);
                                    panel_cx.notify();
                                });
                            }
                        })
                })
            })
            .collect();
        self.easing_buttons = Some(EasingButtons { signature, buttons });
    }

    fn ensure_bezier_controls(&mut self, snapshot: &AnimationSnapshot, cx: &mut Context<Self>) {
        let signature = BezierSignature {
            selected_keys: snapshot.selected_keyframes.clone(),
            active: selected_easing(snapshot),
            enabled: easing_controls_enabled(snapshot) && !snapshot.selected_keyframes.is_empty(),
        };
        if !signature.enabled {
            self.bezier_controls = None;
            return;
        }
        if self
            .bezier_controls
            .as_ref()
            .is_some_and(|controls| controls.signature == signature)
        {
            return;
        }
        let Some(session) = self.session.clone() else {
            self.bezier_controls = None;
            return;
        };

        let draft = Rc::new(RefCell::new(bezier_control_points(signature.active)));
        let panel = cx.entity().downgrade();
        let fields = (0..4)
            .map(|index| {
                let (min, max) = bezier_control_range(index);
                let read_draft = Rc::clone(&draft);
                let write_draft = Rc::clone(&draft);
                let weak_panel = panel.clone();
                let binding = Binding::new(
                    move |_| read_draft.borrow()[index],
                    move |value: f64, app: &mut App| {
                        if !value.is_finite() {
                            return;
                        }
                        write_draft.borrow_mut()[index] = value.clamp(min, max);
                        if let Some(panel) = weak_panel.upgrade() {
                            panel.update(app, |_, cx| cx.notify());
                        }
                    },
                );
                cx.new(|_| {
                    NumberField::new(binding)
                        .range(min, max)
                        .step(0.01)
                        .unit("")
                        .element_id(ElementId::Name(
                            format!("animation-bezier-control-{index}").into(),
                        ))
                })
            })
            .collect();

        let apply_draft = Rc::clone(&draft);
        let apply_session = session;
        let apply_panel = panel;
        let apply = cx.new(|_| {
            Button::new("animation-bezier-apply", t("ui-animation-apply-bezier"))
                .size(ButtonSize::Compact)
                .variant(ButtonVariant::Primary)
                .on_press(move |_, _, app| {
                    let values = *apply_draft.borrow();
                    let Some(easing) = bezier_easing(values) else {
                        if let Some(panel) = apply_panel.upgrade() {
                            panel.update(app, |panel, cx| {
                                panel.status = Some(t("ui-animation-bezier-invalid"));
                                cx.notify();
                            });
                        }
                        return;
                    };
                    let result = apply_session.update(app, |session, session_cx| {
                        let result = session.set_selected_keyframe_easing(easing);
                        if result.as_ref().is_ok_and(|count| *count > 0) {
                            session_cx.notify();
                        }
                        result
                    });
                    let status = match result {
                        Ok(count) if count > 0 => Some(t("ui-animation-command-applied")),
                        Ok(_) => None,
                        Err(error) => Some(error.to_string()),
                    };
                    if let Some(panel) = apply_panel.upgrade() {
                        panel.update(app, |panel, cx| {
                            panel.status = status;
                            cx.notify();
                        });
                    }
                })
        });
        self.bezier_controls = Some(BezierControls {
            signature,
            draft,
            fields,
            apply,
        });
    }

    fn bezier_curve_editor(
        &self,
        values: [f64; 4],
        colors: ColorTokens,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let handles = (0..2).filter_map(|handle| {
            let (x, y) = bezier_handle_position(values, handle)?;
            Some(
                div()
                    .id(ElementId::Name(format!("bezier-handle-{handle}").into()))
                    .absolute()
                    .left(px(x - 7.0))
                    .top(px(y - 7.0))
                    .size(px(14.0))
                    .rounded_full()
                    .border_1()
                    .border_color(colors.surface_0)
                    .bg(if handle == 0 {
                        colors.accent
                    } else {
                        colors.info
                    })
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                            this.begin_bezier_handle_drag(
                                handle,
                                [f64::from(event.position.x), f64::from(event.position.y)],
                                cx,
                            );
                        }),
                    ),
            )
        });
        div()
            .relative()
            .w(px(BEZIER_PREVIEW_WIDTH))
            .h(px(BEZIER_PREVIEW_HEIGHT))
            .child(bezier_curve_preview(values, colors))
            .children(handles)
            .on_mouse_move(cx.listener(Self::handle_bezier_drag_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(Self::finish_bezier_handle_drag),
            )
            .into_any_element()
    }

    fn begin_bezier_handle_drag(
        &mut self,
        handle: usize,
        pointer: [f64; 2],
        cx: &mut Context<Self>,
    ) {
        let Some(controls) = self.bezier_controls.as_ref() else {
            return;
        };
        let values = *controls.draft.borrow();
        let Some(y_range) = bezier_plot_y_range(values) else {
            return;
        };
        self.bezier_drag = Some(BezierHandleDrag {
            handle,
            pointer,
            values,
            y_range,
        });
        cx.notify();
    }

    fn handle_bezier_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.bezier_drag else {
            return;
        };
        let Some(controls) = self.bezier_controls.as_ref() else {
            return;
        };
        let delta = [
            f64::from(event.position.x) - drag.pointer[0],
            f64::from(event.position.y) - drag.pointer[1],
        ];
        let Some(values) = bezier_drag_values(drag.values, drag.handle, delta, drag.y_range) else {
            return;
        };
        *controls.draft.borrow_mut() = values;
        cx.notify();
    }

    fn finish_bezier_handle_drag(
        &mut self,
        _event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.bezier_drag.take().is_some() {
            cx.notify();
        }
    }

    fn ensure_preset_controls(&mut self, snapshot: &AnimationSnapshot, cx: &mut Context<Self>) {
        let selected = snapshot.selected.as_ref();
        let can_save = snapshot.selected_count == 1
            && selected.is_some_and(|selected| {
                !selected.locked
                    && matches!(
                        &selected.source,
                        AnimSource::Timeline(model)
                            if model.preset.is_none() && !model.tracks.is_empty()
                    )
            });
        let can_apply =
            snapshot.selected_count > 0 && selected.is_none_or(|selected| !selected.locked);
        let signature = PresetControlSignature {
            selected_sid: selected.map(|selected| selected.sid.clone()),
            can_save,
            can_apply,
            preset_refs: snapshot.presets.clone(),
        };
        if self
            .preset_controls
            .as_ref()
            .is_some_and(|controls| controls.signature == signature)
        {
            return;
        }
        let Some(session) = self.session.clone() else {
            self.preset_controls = None;
            return;
        };
        let panel = cx.entity().downgrade();
        let save_session = session.clone();
        let save_panel = panel.clone();
        let name_field = cx.new(|_| {
            TextField::new(
                ElementId::Name("animation-preset-name".into()),
                String::new(),
            )
            .placeholder(t("ui-animation-preset-name"))
            .disabled(!can_save)
            .on_submit(move |value, _window, app| {
                let name = value.trim().to_owned();
                let result = save_session.update(app, |session, session_cx| {
                    let result = session.save_selected_animation_as_preset(&name);
                    if result.is_ok() {
                        session_cx.notify();
                    }
                    result
                });
                let status = match result {
                    Ok(_) => Some(t("ui-animation-preset-saved")),
                    Err(error) => Some(error.to_string()),
                };
                let _panel_update = save_panel.update(app, |panel, panel_cx| {
                    panel.status = status;
                    panel_cx.notify();
                });
            })
        });
        let submit_field = name_field.clone();
        let save = cx.new(|_| {
            Button::new("animation-preset-save", t("ui-animation-save-preset"))
                .size(ButtonSize::Compact)
                .variant(ButtonVariant::Primary)
                .disabled(!can_save)
                .on_press(move |_, window, app| {
                    submit_field.update(app, |field, field_cx| field.submit(window, field_cx));
                })
        });
        let rows = signature
            .preset_refs
            .iter()
            .cloned()
            .map(|reference| {
                let preset_id = reference.id.clone();
                let apply_id = preset_id.clone();
                let apply_button_id =
                    ElementId::Name(format!("animation-preset-apply-{preset_id}").into());
                let apply_session = session.clone();
                let apply_panel = panel.clone();
                let apply = cx.new(move |_| {
                    Button::new(apply_button_id, t("ui-animation-preset-apply"))
                        .size(ButtonSize::Compact)
                        .variant(ButtonVariant::Secondary)
                        .disabled(!can_apply)
                        .on_press(move |_, _, app| {
                            let action_id = apply_id.clone();
                            let result = apply_session.update(app, |session, session_cx| {
                                let result =
                                    session.apply_animation_preset_to_selection(&action_id);
                                if result.as_ref().is_ok_and(|count| *count > 0) {
                                    session_cx.notify();
                                }
                                result
                            });
                            let status = match result {
                                Ok(count) if count > 0 => Some(t("ui-animation-preset-applied")),
                                Ok(_) => None,
                                Err(error) => Some(error.to_string()),
                            };
                            let _panel_update = apply_panel.update(app, |panel, panel_cx| {
                                panel.status = status;
                                panel_cx.notify();
                            });
                        })
                });
                let delete_id = preset_id.clone();
                let delete_button_id =
                    ElementId::Name(format!("animation-preset-delete-{preset_id}").into());
                let delete_session = session.clone();
                let delete_panel = panel.clone();
                let delete = cx.new(move |_| {
                    Button::new(delete_button_id, t("ui-animation-preset-delete"))
                        .size(ButtonSize::Compact)
                        .variant(ButtonVariant::Danger)
                        .on_press(move |_, _, app| {
                            let action_id = delete_id.clone();
                            let result = delete_session.update(app, |session, session_cx| {
                                let result = session.delete_animation_preset(&action_id);
                                if result.as_ref().is_ok_and(|deleted| *deleted) {
                                    session_cx.notify();
                                }
                                result
                            });
                            let status = match result {
                                Ok(true) => Some(t("ui-animation-preset-deleted")),
                                Ok(false) => None,
                                Err(error) => Some(error.to_string()),
                            };
                            let _panel_update = delete_panel.update(app, |panel, panel_cx| {
                                panel.status = status;
                                panel_cx.notify();
                            });
                        })
                });
                PresetRowButtons {
                    reference,
                    apply,
                    delete,
                }
            })
            .collect();
        self.preset_controls = Some(PresetControls {
            signature,
            name_field,
            save,
            rows,
        });
    }

    fn sync_slider(&mut self, snapshot: &AnimationSnapshot, cx: &mut Context<Self>) {
        let value = snapshot
            .duration_seconds
            .filter(|duration| duration.is_finite() && *duration > 0.0)
            .map(|duration| snapshot.playhead_seconds / duration * 100.0)
            .unwrap_or(0.0);
        if let Some(slider) = self.slider.as_ref() {
            slider.update(cx, |slider, slider_cx| slider.set_value(value, slider_cx));
        }
    }

    /// Sable UI thread remains responsive: advance the pure session clock at 16ms,
    /// then let the existing CanvasPanel observer render a disposable frame snapshot.
    fn start_playback_pump(&mut self, cx: &mut Context<Self>) {
        if self.pumping || self.session.is_none() {
            return;
        }
        self.pumping = true;
        let session = self.session.clone();
        cx.spawn(async move |panel, cx| {
            let mut previous = Instant::now();
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let now = Instant::now();
                let delta_seconds = now.duration_since(previous).as_secs_f64();
                previous = now;
                let Some(session) = session.as_ref() else {
                    break;
                };
                let (playing, error) = match session.update(cx, |session, session_cx| {
                    let result = session.advance_animation_playback(delta_seconds);
                    if result.as_ref().is_ok_and(|changed| *changed) {
                        session_cx.notify();
                    }
                    (
                        session.animation_snapshot().playing,
                        result.err().map(|error| error.to_string()),
                    )
                }) {
                    Ok(result) => result,
                    Err(error) => (false, Some(error.to_string())),
                };
                if let Some(error) = error {
                    let _ = panel.update(cx, |panel, panel_cx| {
                        panel.status = Some(error.to_string());
                        panel_cx.notify();
                    });
                    break;
                }
                if panel
                    .update(cx, |panel, panel_cx| {
                        if let Some(session) = panel.session.as_ref() {
                            let snapshot = session.read(panel_cx).animation_snapshot();
                            panel.sync_slider(&snapshot, panel_cx);
                            panel.ensure_buttons(&snapshot, panel_cx);
                        }
                        panel_cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
                if !playing {
                    break;
                }
            }
            let _ = panel.update(cx, |panel, panel_cx| {
                panel.pumping = false;
                panel_cx.notify();
            });
        })
        .detach();
    }
}

fn keyframe_button_states(snapshot: &AnimationSnapshot) -> Vec<KeyframeButtonState> {
    let Some(selected) = snapshot.selected.as_ref() else {
        return Vec::new();
    };
    let AnimSource::Timeline(model) = &selected.source else {
        return Vec::new();
    };
    let duration_seconds = model.duration_ms / 1000.0;
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
        return Vec::new();
    }
    let disabled = selected.locked || snapshot.selected_count != 1;
    let mut seen = Vec::new();
    let mut states = Vec::new();
    for (track, frames) in &model.tracks {
        for frame in frames {
            let Some(offset_permille) = keyframe_offset_permille(frame.t, duration_seconds) else {
                continue;
            };
            if !seen.contains(&(*track, offset_permille)) {
                seen.push((*track, offset_permille));
            } else {
                continue;
            }
            let selected_keyframe = snapshot.selected_keyframes.iter().any(|keyframe| {
                keyframe.sid == selected.sid
                    && keyframe.track == *track
                    && keyframe.offset_permille == offset_permille
            });
            states.push(KeyframeButtonState {
                sid: selected.sid.clone(),
                track: *track,
                offset_permille,
                selected: selected_keyframe,
                disabled,
            });
        }
    }
    states
}

fn track_key(track: TrackProp) -> &'static str {
    match track {
        TrackProp::Position => "position",
        TrackProp::Scale => "scale",
        TrackProp::Opacity => "opacity",
        TrackProp::Rotate => "rotation",
    }
}

fn selected_easing(snapshot: &AnimationSnapshot) -> Option<Easing> {
    let selected = snapshot.selected.as_ref()?;
    let AnimSource::Timeline(model) = &selected.source else {
        return None;
    };
    let duration_seconds = model.duration_ms / 1000.0;
    let mut active = None;
    let mut found = false;
    for selected_key in snapshot
        .selected_keyframes
        .iter()
        .filter(|key| key.sid == selected.sid)
    {
        let frame = model.frames_of(selected_key.track).iter().find(|frame| {
            keyframe_offset_permille(frame.t, duration_seconds)
                == Some(selected_key.offset_permille)
        })?;
        if active.is_some_and(|easing| easing != frame.easing) {
            return None;
        }
        active = Some(frame.easing);
        found = true;
    }
    if found {
        active
    } else {
        None
    }
}

fn easing_controls_enabled(snapshot: &AnimationSnapshot) -> bool {
    snapshot.selected_count == 1
        && snapshot.selected.as_ref().is_some_and(|selected| {
            !selected.locked && matches!(&selected.source, AnimSource::Timeline(_))
        })
        && !snapshot.selected_keyframes.is_empty()
}

fn easing_label(easing: Easing) -> String {
    t(match easing {
        Easing::Linear => "ui-animation-ease-linear",
        Easing::Ease => "ui-animation-ease-ease",
        Easing::EaseIn => "ui-animation-ease-in",
        Easing::EaseOut => "ui-animation-ease-out",
        Easing::EaseInOut => "ui-animation-ease-in-out",
        Easing::Bezier(..) => "ui-animation-ease-custom",
    })
}

fn bezier_control_range(index: usize) -> (f64, f64) {
    match index {
        0 | 2 => (0.0, 1.0),
        _ => (-10.0, 10.0),
    }
}

fn bezier_control_points(easing: Option<Easing>) -> [f64; 4] {
    match easing {
        Some(Easing::Bezier(x1, y1, x2, y2)) => [x1, y1, x2, y2],
        Some(Easing::Linear) => [0.0, 0.0, 1.0, 1.0],
        Some(Easing::Ease) => [0.25, 0.1, 0.25, 1.0],
        Some(Easing::EaseIn) => [0.42, 0.0, 1.0, 1.0],
        Some(Easing::EaseOut) => [0.0, 0.0, 0.58, 1.0],
        Some(Easing::EaseInOut) | None => [0.42, 0.0, 0.58, 1.0],
    }
}

fn bezier_easing(values: [f64; 4]) -> Option<Easing> {
    let [x1, y1, x2, y2] = values;
    if !x1.is_finite()
        || !(0.0..=1.0).contains(&x1)
        || !y1.is_finite()
        || y1.abs() > 10.0
        || !x2.is_finite()
        || !(0.0..=1.0).contains(&x2)
        || !y2.is_finite()
        || y2.abs() > 10.0
    {
        return None;
    }
    Some(Easing::Bezier(x1, y1, x2, y2))
}

fn bezier_handle_position(values: [f64; 4], handle: usize) -> Option<(f32, f32)> {
    if handle > 1 || bezier_easing(values).is_none() {
        return None;
    }
    let range = bezier_plot_y_range(values)?;
    let x_index = handle * 2;
    let y_index = x_index + 1;
    let x = values[x_index] as f32 * BEZIER_PREVIEW_WIDTH;
    let y_fraction = ((range.1 - values[y_index]) / (range.1 - range.0)).clamp(0.0, 1.0);
    Some((x, y_fraction as f32 * BEZIER_PREVIEW_HEIGHT))
}

fn bezier_drag_values(
    start: [f64; 4],
    handle: usize,
    pointer_delta: [f64; 2],
    y_range: (f64, f64),
) -> Option<[f64; 4]> {
    if handle > 1
        || pointer_delta.iter().any(|value| !value.is_finite())
        || !y_range.0.is_finite()
        || !y_range.1.is_finite()
        || y_range.1 <= y_range.0
    {
        return None;
    }
    let mut values = start;
    let x_index = handle * 2;
    let y_index = x_index + 1;
    values[x_index] =
        (start[x_index] + pointer_delta[0] / f64::from(BEZIER_PREVIEW_WIDTH)).clamp(0.0, 1.0);
    values[y_index] = (start[y_index]
        - pointer_delta[1] / f64::from(BEZIER_PREVIEW_HEIGHT) * (y_range.1 - y_range.0))
        .clamp(-10.0, 10.0);
    bezier_easing(values).map(|_| values)
}

fn bezier_preview_values(values: [f64; 4], sample_count: usize) -> Option<Vec<f64>> {
    if sample_count < 2 {
        return Some(Vec::new());
    }
    let easing = bezier_easing(values)?;
    let timing = easing.to_timing();
    Some(
        (0..sample_count)
            .map(|index| timing.eval(index as f64 / (sample_count - 1) as f64))
            .collect(),
    )
}

const BEZIER_PREVIEW_WIDTH: f32 = 160.0;
const BEZIER_PREVIEW_HEIGHT: f32 = 48.0;

fn bezier_plot_y_range(values: [f64; 4]) -> Option<(f64, f64)> {
    if values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let minimum = values[1].min(values[3]).min(0.0);
    let maximum = values[1].max(values[3]).max(1.0);
    let span = (maximum - minimum).max(1e-6);
    let padding = span * 0.1;
    Some((minimum - padding, maximum + padding))
}

fn bezier_curve_preview(values: [f64; 4], colors: ColorTokens) -> AnyElement {
    const SAMPLES: usize = 48;
    let Some(samples) = bezier_preview_values(values, SAMPLES) else {
        return div().into_any_element();
    };
    let Some(y_range) = bezier_plot_y_range(values) else {
        return div().into_any_element();
    };
    let bar_width = BEZIER_PREVIEW_WIDTH / SAMPLES as f32;
    div()
        .relative()
        .w(px(BEZIER_PREVIEW_WIDTH))
        .h(px(BEZIER_PREVIEW_HEIGHT))
        .overflow_hidden()
        .bg(colors.surface_1)
        .border_1()
        .border_color(colors.border_subtle)
        .children(samples.iter().enumerate().map(|(index, value)| {
            let fraction = ((value - y_range.0) / (y_range.1 - y_range.0)).clamp(0.0, 1.0);
            let height = (fraction as f32 * BEZIER_PREVIEW_HEIGHT).max(1.0);
            div()
                .absolute()
                .left(px(index as f32 * bar_width))
                .bottom(px(0.0))
                .w(px(bar_width.max(1.0)))
                .h(px(height))
                .bg(colors.accent)
        }))
        .into_any_element()
}

fn bezier_control_field(
    label: impl Into<sable::gpui::SharedString>,
    field: Entity<NumberField>,
    colors: ColorTokens,
) -> AnyElement {
    v_flex()
        .w(px(74.0))
        .gap_1()
        .child(
            div()
                .text_size(px(10.0))
                .text_color(colors.text_secondary)
                .child(label.into()),
        )
        .child(field)
        .into_any_element()
}

fn track_label(track: TrackProp) -> String {
    t(match track {
        TrackProp::Position => "ui-animation-track-position",
        TrackProp::Scale => "ui-animation-track-scale",
        TrackProp::Opacity => "ui-animation-track-opacity",
        TrackProp::Rotate => "ui-animation-track-rotation",
    })
}

const TIMELINE_TRACK_WIDTH_PX: f32 = 900.0;
const TIMELINE_MARKER_WIDTH_PX: f32 = 54.0;
const TIMELINE_LABEL_WIDTH_PX: f32 = 96.0;

fn keyframe_marker_position(offset_permille: u16) -> (f32, f32) {
    let progress = f32::from(offset_permille.min(1000)) / 1000.0;
    let point_x = progress * TIMELINE_TRACK_WIDTH_PX;
    let left = (point_x - TIMELINE_MARKER_WIDTH_PX / 2.0)
        .clamp(0.0, TIMELINE_TRACK_WIDTH_PX - TIMELINE_MARKER_WIDTH_PX);
    (point_x, left)
}

fn displayed_keyframe_offset(state: &KeyframeButtonState, drag: Option<&KeyframeDragState>) -> u16 {
    let Some(drag) = drag.filter(|drag| drag.active && drag.delta.is_some()) else {
        return state.offset_permille;
    };
    let marker = AnimationKeyframeSelection {
        sid: state.sid.clone(),
        track: state.track,
        offset_permille: state.offset_permille,
    };
    let Some(index) = drag
        .source_markers
        .iter()
        .position(|source| source == &marker)
    else {
        return state.offset_permille;
    };
    drag.preview_markers
        .get(index)
        .map_or(state.offset_permille, |preview| preview.offset_permille)
}

fn displayed_keyframe_copies(drag: Option<&KeyframeDragState>) -> &[AnimationKeyframeSelection] {
    drag.filter(|drag| drag.active && drag.copy && drag.delta.is_some())
        .map_or(&[], |drag| drag.preview_copy_markers.as_slice())
}

fn keyframe_drag_delta(pixel_delta: f64) -> Result<Option<KeyframeOffsetDelta>, String> {
    if !pixel_delta.is_finite() {
        return Err(t("ui-animation-drag-invalid-distance"));
    }
    if pixel_delta.abs() < 3.0 {
        return Ok(None);
    }
    let permille = (pixel_delta / f64::from(TIMELINE_TRACK_WIDTH_PX) * 1000.0).round();
    if !permille.is_finite() || !(-1000.0..=1000.0).contains(&permille) {
        return Err(t("ui-animation-drag-out-of-range"));
    }
    #[allow(clippy::cast_possible_truncation)]
    let permille = permille as i32;
    KeyframeOffsetDelta::new(permille)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn keyframe_context_handler(
    session: Entity<ProjectSession>,
    panel: WeakEntity<ProjectAnimationPanel>,
    marker: AnimationKeyframeSelection,
    action: KeyframeContextAction,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, _window, app| {
        let marker = marker.clone();
        let result: Result<usize, String> = session.update(app, |session, session_cx| {
            if !session
                .animation_snapshot()
                .selected_keyframes
                .contains(&marker)
            {
                session
                    .select_animation_keyframe(
                        &marker.sid,
                        marker.track,
                        marker.offset_permille,
                        false,
                    )
                    .map_err(|error| error.to_string())?;
            }
            let affected = match action {
                KeyframeContextAction::Delete => {
                    match session.dispatch_catalog("anim.keyframe_delete") {
                        CommandOutcome::Applied => 1,
                        CommandOutcome::NotRun { reason } => return Err(reason),
                    }
                }
                KeyframeContextAction::SetEasing(easing) => session
                    .set_selected_keyframe_easing(easing)
                    .map_err(|error| error.to_string())?,
                KeyframeContextAction::NudgeCatalog(catalog_id) => {
                    match session.dispatch_catalog(catalog_id) {
                        CommandOutcome::Applied => 1,
                        CommandOutcome::NotRun { reason } => return Err(reason),
                    }
                }
            };
            if affected > 0 {
                session_cx.notify();
            }
            Ok(affected)
        });
        let status = match result {
            Ok(count) if count > 0 => Some(t("ui-animation-command-applied")),
            Ok(_) => None,
            Err(error) => Some(error),
        };
        let _panel_update = panel.update(app, |panel, panel_cx| {
            panel.status = status;
            panel_cx.notify();
        });
    }
}

fn timeline_rows(
    model: &AnimModel,
    buttons: &[KeyframeButton],
    drag: Option<&KeyframeDragState>,
    session: Entity<ProjectSession>,
    panel: WeakEntity<ProjectAnimationPanel>,
    colors: ColorTokens,
    cx: &mut Context<ProjectAnimationPanel>,
) -> impl IntoElement {
    let mut rows = Vec::new();
    for (property, _) in &model.tracks {
        let mut markers = Vec::new();
        for button in buttons
            .iter()
            .filter(|button| button.state.track == *property)
        {
            let display_offset = displayed_keyframe_offset(&button.state, drag);
            let (point_x, button_left) = keyframe_marker_position(display_offset);
            let guide_color = if button.state.selected {
                colors.accent
            } else {
                colors.border_subtle
            };
            markers.push(
                div()
                    .absolute()
                    .left(px(point_x))
                    .top(px(0.0))
                    .w(px(1.0))
                    .h_full()
                    .bg(guide_color)
                    .into_any_element(),
            );

            let marker = AnimationKeyframeSelection {
                sid: button.state.sid.clone(),
                track: button.state.track,
                offset_permille: button.state.offset_permille,
            };
            let mouse_down_marker = marker.clone();
            let context_marker = marker.clone();
            let context_session = session.clone();
            let context_panel = panel.clone();
            let id = ElementId::Name(
                format!(
                    "keyframe-hit-{}-{}-{}",
                    button.state.sid,
                    track_key(button.state.track),
                    button.state.offset_permille
                )
                .into(),
            );
            let hit_target = div()
                .id(id)
                .absolute()
                .left(px(button_left))
                .top(px(0.0))
                .w(px(TIMELINE_MARKER_WIDTH_PX))
                .child(button.button.clone())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |panel, event: &MouseDownEvent, _window, panel_cx| {
                        panel.begin_keyframe_drag(
                            mouse_down_marker.clone(),
                            f64::from(event.position.x),
                            event.modifiers.shift || event.modifiers.platform,
                            event.modifiers.alt,
                        );
                        panel_cx.notify();
                    }),
                )
                .context_menu(move |mut menu, _window, _menu_cx| {
                    menu = menu.item(PopupMenuItem::new(t("cmd-anim-keyframe-delete")).on_click(
                        keyframe_context_handler(
                            context_session.clone(),
                            context_panel.clone(),
                            context_marker.clone(),
                            KeyframeContextAction::Delete,
                        ),
                    ));
                    menu = menu.separator();
                    for easing in Easing::PRESETS {
                        menu = menu.item(PopupMenuItem::new(easing_label(easing)).on_click(
                            keyframe_context_handler(
                                context_session.clone(),
                                context_panel.clone(),
                                context_marker.clone(),
                                KeyframeContextAction::SetEasing(easing),
                            ),
                        ));
                    }
                    menu.separator()
                        .item(
                            PopupMenuItem::new(t("ui-animation-keyframe-nudge-earlier")).on_click(
                                keyframe_context_handler(
                                    context_session.clone(),
                                    context_panel.clone(),
                                    context_marker.clone(),
                                    KeyframeContextAction::NudgeCatalog(
                                        "anim.keyframe_nudge_earlier",
                                    ),
                                ),
                            ),
                        )
                        .item(
                            PopupMenuItem::new(t("ui-animation-keyframe-nudge-later")).on_click(
                                keyframe_context_handler(
                                    context_session.clone(),
                                    context_panel.clone(),
                                    context_marker.clone(),
                                    KeyframeContextAction::NudgeCatalog(
                                        "anim.keyframe_nudge_later",
                                    ),
                                ),
                            ),
                        )
                })
                .into_any_element();
            markers.push(hit_target);
        }

        for marker in displayed_keyframe_copies(drag)
            .iter()
            .filter(|marker| marker.track == *property)
        {
            let (point_x, button_left) = keyframe_marker_position(marker.offset_permille);
            markers.push(
                div()
                    .absolute()
                    .left(px(point_x))
                    .top(px(0.0))
                    .w(px(1.0))
                    .h_full()
                    .bg(colors.accent)
                    .into_any_element(),
            );
            markers.push(
                div()
                    .absolute()
                    .left(px(button_left))
                    .top(px(4.0))
                    .w(px(TIMELINE_MARKER_WIDTH_PX))
                    .h(px(24.0))
                    .rounded_md()
                    .border_1()
                    .border_color(colors.accent)
                    .into_any_element(),
            );
        }

        rows.push(
            h_flex()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .child(
                    div()
                        .w(px(TIMELINE_LABEL_WIDTH_PX))
                        .child(track_label(*property)),
                )
                .child(
                    div()
                        .relative()
                        .w(px(TIMELINE_TRACK_WIDTH_PX))
                        .h(px(32.0))
                        .child(
                            div()
                                .absolute()
                                .left(px(0.0))
                                .top(px(16.0))
                                .w_full()
                                .h(px(1.0))
                                .bg(colors.border_subtle),
                        )
                        .children(markers),
                )
                .into_any_element(),
        );
    }
    v_flex()
        .w(px(TIMELINE_LABEL_WIDTH_PX + TIMELINE_TRACK_WIDTH_PX))
        .gap_1()
        .children(rows)
}

impl Render for ProjectAnimationPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme(cx).colors;
        let snapshot = self
            .session
            .as_ref()
            .map(|session| session.read(cx).animation_snapshot())
            .unwrap_or_default();
        let (frame_snap_enabled, frame_rate) = self
            .session
            .as_ref()
            .map(|session| {
                let session = session.read(cx);
                (
                    session.animation_frame_snap_enabled(),
                    session.animation_frame_rate(),
                )
            })
            .unwrap_or((true, AnimationFrameRate::Fps30));
        self.sync_slider(&snapshot, cx);
        self.ensure_buttons(&snapshot, cx);
        self.ensure_frame_snap_buttons(&snapshot, frame_snap_enabled, frame_rate, cx);
        self.ensure_keyframe_buttons(&snapshot, cx);
        self.ensure_easing_buttons(&snapshot, cx);
        self.ensure_bezier_controls(&snapshot, cx);
        self.ensure_preset_controls(&snapshot, cx);
        if snapshot.playing {
            self.start_playback_pump(cx);
        }

        let state = ControlState::from_snapshot(&snapshot);
        let selected = snapshot.selected.as_ref();
        let body = if self.session.is_none() {
            div()
                .child(
                    self.error
                        .clone()
                        .unwrap_or_else(|| t("ui-animation-no-project")),
                )
                .into_any_element()
        } else if !state.selected {
            div().child(t("ui-animation-select-one")).into_any_element()
        } else if state.locked {
            div().child(t("ui-inspector-locked")).into_any_element()
        } else {
            match selected.map(|item| &item.source) {
                Some(AnimSource::Timeline(model)) => {
                    if let Some(session) = self.session.clone() {
                        timeline_rows(
                            model,
                            &self.keyframe_buttons,
                            self.keyframe_drag.as_ref(),
                            session,
                            cx.entity().downgrade(),
                            colors,
                            cx,
                        )
                        .into_any_element()
                    } else {
                        div().child(t("ui-animation-no-project")).into_any_element()
                    }
                }
                Some(AnimSource::Foreign(_)) => div()
                    .child(t("ui-animation-foreign-readonly"))
                    .into_any_element(),
                Some(AnimSource::None) => div()
                    .child(t("ui-animation-no-timeline"))
                    .into_any_element(),
                None => div().child(t("ui-animation-select-one")).into_any_element(),
            }
        };
        let duration = snapshot
            .duration_seconds
            .map(|seconds| format!("{:.2}s", seconds))
            .unwrap_or_else(|| "—".to_owned());
        let playhead = format!("{:.2}s / {duration}", snapshot.playhead_seconds);
        let controls = self.buttons.as_ref();
        let frame_snap_buttons = self.frame_snap_buttons.as_ref();
        let easing_buttons = self.easing_buttons.as_ref();
        let bezier_ui = self.bezier_controls.as_ref().map(|controls| {
            let values = *controls.draft.borrow();
            let fields = [
                "ui-animation-bezier-x1",
                "ui-animation-bezier-y1",
                "ui-animation-bezier-x2",
                "ui-animation-bezier-y2",
            ];
            h_flex()
                .items_end()
                .gap_1()
                .child(
                    div()
                        .w(px(96.0))
                        .text_color(colors.text_secondary)
                        .child(t("ui-animation-custom-bezier")),
                )
                .children(
                    fields
                        .iter()
                        .zip(controls.fields.iter())
                        .map(|(label, field)| {
                            bezier_control_field(t(label), field.clone(), colors)
                        }),
                )
                .child(self.bezier_curve_editor(values, colors, cx))
                .child(controls.apply.clone())
                .into_any_element()
        });
        let preset_ui = self.preset_controls.as_ref().map(|controls| {
            let rows = controls
                .rows
                .iter()
                .map(|row| {
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(div().flex_1().child(row.reference.display_name.clone()))
                        .child(row.apply.clone())
                        .child(row.delete.clone())
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            let empty = rows.is_empty();
            let mut panel = v_flex()
                .gap_1()
                .child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(t("ui-animation-presets")),
                )
                .child(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(controls.name_field.clone())
                        .child(controls.save.clone()),
                )
                .children(rows);
            if empty {
                panel = panel.child(
                    div()
                        .text_color(colors.text_secondary)
                        .child(t("ui-animation-presets-empty")),
                );
            }
            panel.into_any_element()
        });
        let drag_readout = self
            .keyframe_drag
            .as_ref()
            .filter(|drag| drag.active)
            .and_then(|drag| drag.delta)
            .map(|delta| {
                t_args(
                    "ui-animation-keyframe-drag-delta",
                    &[(
                        "delta",
                        FluentValue::from(format!("{:+}", delta.permille())),
                    )],
                )
            });
        let status = self.status.clone();
        let slider = if state.source == SourceKind::Timeline && !state.locked {
            self.slider
                .as_ref()
                .map(|slider| slider.clone().into_any_element())
                .unwrap_or_else(|| div().into_any_element())
        } else {
            div()
                .h(px(12.0))
                .w_full()
                .bg(colors.surface_1)
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2()
            .bg(colors.surface_0)
            .text_color(colors.text_primary)
            .on_mouse_move(cx.listener(Self::handle_keyframe_drag_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::finish_keyframe_drag))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::finish_keyframe_drag))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(14.0))
                            .child(t("ui-shell-panel-animation")),
                    )
                    .child(div().flex_1())
                    .children(
                        drag_readout.map(|readout| div().text_color(colors.accent).child(readout)),
                    )
                    .child(div().text_color(colors.text_secondary).child(playhead)),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .children(controls.into_iter().flat_map(|buttons| {
                        [
                            buttons.create.clone().into_any_element(),
                            buttons.play.clone().into_any_element(),
                            buttons.stop.clone().into_any_element(),
                            buttons.loop_toggle.clone().into_any_element(),
                            buttons.keyframe_add.clone().into_any_element(),
                            buttons.keyframe_delete.clone().into_any_element(),
                            buttons.clear.clone().into_any_element(),
                        ]
                    })),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .w(px(TIMELINE_LABEL_WIDTH_PX))
                            .child(t("ui-animation-frame-rate")),
                    )
                    .children(frame_snap_buttons.into_iter().flat_map(|controls| {
                        std::iter::once(controls.toggle.clone().into_any_element()).chain(
                            controls
                                .rates
                                .iter()
                                .cloned()
                                .map(|button| button.into_any_element()),
                        )
                    })),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .w(px(TIMELINE_LABEL_WIDTH_PX))
                            .child(t("ui-animation-easing")),
                    )
                    .children(easing_buttons.into_iter().flat_map(|controls| {
                        controls
                            .buttons
                            .iter()
                            .cloned()
                            .map(|button| button.into_any_element())
                    })),
            )
            .children(bezier_ui)
            .children(preset_ui)
            .child(slider)
            .child(
                div()
                    .id("animation-timeline-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_x_scroll()
                    .overflow_y_scroll()
                    .text_color(colors.text_secondary)
                    .child(body),
            )
            .children(status.map(|message| div().text_color(colors.warning).child(message)))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bezier_drag_values, bezier_easing, bezier_handle_position, bezier_plot_y_range,
        bezier_preview_values, displayed_keyframe_copies, displayed_keyframe_offset,
        easing_controls_enabled, keyframe_button_states, keyframe_drag_delta,
        keyframe_marker_position, selected_easing, KeyframeButtonState, KeyframeDragState,
        TIMELINE_MARKER_WIDTH_PX, TIMELINE_TRACK_WIDTH_PX,
    };
    use vb_animation::timeline::{AnimModel, AnimSource, Easing, KeyValue, Keyframe, TrackProp};
    use vb_session::project::{
        AnimationKeyframeSelection, AnimationSnapshot, KeyframeOffsetDelta,
        SelectedAnimationSnapshot,
    };

    fn snapshot() -> AnimationSnapshot {
        AnimationSnapshot {
            selected_count: 1,
            selected: Some(SelectedAnimationSnapshot {
                sid: "box-stable-id".into(),
                name: "Box".into(),
                locked: false,
                source: AnimSource::Timeline(AnimModel {
                    duration_ms: 2000.0,
                    delay_ms: 0.0,
                    iterations: 1.0,
                    tracks: vec![
                        (
                            TrackProp::Position,
                            vec![
                                Keyframe {
                                    t: 1.0,
                                    value: KeyValue::Offset(20.0, 10.0),
                                    easing: Easing::Linear,
                                },
                                Keyframe {
                                    t: 1.0001,
                                    value: KeyValue::Offset(21.0, 10.0),
                                    easing: Easing::Linear,
                                },
                            ],
                        ),
                        (
                            TrackProp::Opacity,
                            vec![Keyframe {
                                t: 0.5,
                                value: KeyValue::Alpha(0.5),
                                easing: Easing::Linear,
                            }],
                        ),
                    ],
                    extra: Vec::new(),
                    preset: None,
                }),
                static_values: ((0.0, 0.0), 1.0, 1.0, 0.0),
            }),
            presets: Vec::new(),
            selected_keyframes: vec![AnimationKeyframeSelection {
                sid: "box-stable-id".into(),
                track: TrackProp::Position,
                offset_permille: 500,
            }],
            duration_seconds: Some(2.0),
            playhead_seconds: 1.0,
            playing: false,
            looping: false,
        }
    }

    #[test]
    fn timeline_markers_follow_time_proportionally_and_stay_in_bounds() {
        let start = keyframe_marker_position(0);
        let middle = keyframe_marker_position(500);
        let end = keyframe_marker_position(1000);

        assert_eq!(start.0, 0.0);
        assert_eq!(middle.0, TIMELINE_TRACK_WIDTH_PX / 2.0);
        assert_eq!(end.0, TIMELINE_TRACK_WIDTH_PX);
        for (_, left) in [start, middle, end] {
            assert!(left >= 0.0);
            assert!(left + TIMELINE_MARKER_WIDTH_PX <= TIMELINE_TRACK_WIDTH_PX);
        }
        assert_eq!(start.1, 0.0);
        assert_eq!(
            middle.1,
            (TIMELINE_TRACK_WIDTH_PX - TIMELINE_MARKER_WIDTH_PX) / 2.0
        );
        assert_eq!(end.1, TIMELINE_TRACK_WIDTH_PX - TIMELINE_MARKER_WIDTH_PX);
    }

    #[test]
    fn marker_specs_normalize_time_deduplicate_and_project_selection() {
        let states = keyframe_button_states(&snapshot());
        assert_eq!(states.len(), 2);
        assert!(states.iter().any(|state| {
            state.track == TrackProp::Position
                && state.offset_permille == 500
                && state.selected
                && !state.disabled
        }));
        assert!(states.iter().any(|state| {
            state.track == TrackProp::Opacity && state.offset_permille == 250 && !state.selected
        }));
    }

    #[test]
    fn marker_specs_are_read_only_for_foreign_or_locked_animation() {
        let mut locked = snapshot();
        locked.selected.as_mut().unwrap().locked = true;
        assert!(keyframe_button_states(&locked)
            .iter()
            .all(|state: &KeyframeButtonState| state.disabled));

        let mut foreign = snapshot();
        foreign.selected.as_mut().unwrap().source = AnimSource::Foreign("site".into());
        assert!(keyframe_button_states(&foreign).is_empty());

        let empty = AnimationSnapshot::default();
        assert!(keyframe_button_states(&empty).is_empty());
        assert!(!easing_controls_enabled(&empty));
    }

    #[test]
    fn easing_controls_reflect_mixed_selection_and_read_only_states() {
        let uniform = snapshot();
        assert!(easing_controls_enabled(&uniform));
        assert_eq!(selected_easing(&uniform), Some(Easing::Linear));

        let mut mixed = snapshot();
        if let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(model),
            ..
        }) = mixed.selected.as_mut()
        {
            model
                .tracks
                .iter_mut()
                .find(|(track, _)| *track == TrackProp::Opacity)
                .expect("opacity track")
                .1[0]
                .easing = Easing::EaseIn;
        }
        mixed.selected_keyframes.push(AnimationKeyframeSelection {
            sid: "box-stable-id".into(),
            track: TrackProp::Opacity,
            offset_permille: 250,
        });
        assert!(easing_controls_enabled(&mixed));
        assert_eq!(selected_easing(&mixed), None);

        let mut locked = uniform.clone();
        locked.selected.as_mut().unwrap().locked = true;
        assert!(!easing_controls_enabled(&locked));
        let mut foreign = uniform;
        foreign.selected.as_mut().unwrap().source = AnimSource::Foreign("site".into());
        assert!(!easing_controls_enabled(&foreign));
    }

    #[test]
    fn custom_bezier_controls_validate_and_preview_with_shared_timing() {
        let values = [0.25, 0.1, 0.25, 1.0];
        let easing = bezier_easing(values).expect("valid custom Bezier");
        let samples = bezier_preview_values(values, 3).expect("curve preview");
        assert_eq!(samples.len(), 3);
        assert!(samples[0].abs() < 1e-9);
        assert!((samples[1] - easing.to_timing().eval(0.5)).abs() < 1e-9);
        assert!((samples[2] - 1.0).abs() < 1e-9);
        assert!(bezier_easing([1.01, 0.0, 0.25, 1.0]).is_none());
        assert!(bezier_easing([0.25, f64::NAN, 0.25, 1.0]).is_none());
        assert!(bezier_preview_values(values, 1).unwrap().is_empty());
    }

    #[test]
    fn bezier_drag_handles_follow_preview_and_clamp_to_valid_css_controls() {
        let initial = [0.25, 0.1, 0.75, 0.9];
        let position = bezier_handle_position(initial, 0).expect("first handle");
        assert!((position.0 - 40.0).abs() < 1e-4);
        assert!((position.1 - 40.0).abs() < 1e-4);

        let range = bezier_plot_y_range(initial).expect("preview y range");
        let moved =
            bezier_drag_values(initial, 0, [16.0, -8.0], range).expect("valid pointer move");
        assert!((moved[0] - 0.35).abs() < 1e-9);
        assert!((moved[1] - 0.3).abs() < 1e-9);

        let outside = [0.95, 9.9, 0.75, 0.9];
        let clamped = bezier_drag_values(outside, 0, [100.0, -100.0], range)
            .expect("clamped controls remain valid");
        assert_eq!(clamped[0], 1.0);
        assert_eq!(clamped[1], 10.0);
        assert!(bezier_drag_values(initial, 2, [1.0, 1.0], range).is_none());
        assert!(bezier_drag_values(initial, 0, [f64::NAN, 0.0], range).is_none());
    }

    #[test]
    fn keyframe_drag_delta_uses_screen_threshold_and_signed_permille() {
        assert_eq!(keyframe_drag_delta(2.99).unwrap(), None);
        assert_eq!(keyframe_drag_delta(3.0).unwrap().unwrap().permille(), 3);
        assert_eq!(keyframe_drag_delta(-45.0).unwrap().unwrap().permille(), -50);
        assert!(keyframe_drag_delta(901.0).is_err());
        assert!(keyframe_drag_delta(f64::NAN).is_err());
    }

    #[test]
    fn alt_copy_drag_preserves_sources_and_exposes_ghost_markers() {
        let states = keyframe_button_states(&snapshot());
        let source = states
            .iter()
            .map(|state| AnimationKeyframeSelection {
                sid: state.sid.clone(),
                track: state.track,
                offset_permille: state.offset_permille,
            })
            .collect::<Vec<_>>();
        let copies = source
            .iter()
            .map(|marker| AnimationKeyframeSelection {
                sid: marker.sid.clone(),
                track: marker.track,
                offset_permille: marker.offset_permille + 100,
            })
            .collect::<Vec<_>>();
        let drag = KeyframeDragState {
            marker: source[0].clone(),
            start_x: 100.0,
            additive: false,
            copy: true,
            active: true,
            delta: Some(KeyframeOffsetDelta::new(100).unwrap()),
            source_markers: source.clone(),
            preview_markers: source.clone(),
            preview_copy_markers: copies.clone(),
        };

        assert_eq!(
            displayed_keyframe_offset(&states[0], Some(&drag)),
            source[0].offset_permille
        );
        assert_eq!(displayed_keyframe_copies(Some(&drag)), copies.as_slice());
        let move_drag = KeyframeDragState {
            copy: false,
            ..drag.clone()
        };
        assert!(displayed_keyframe_copies(Some(&move_drag)).is_empty());
    }

    #[test]
    fn keyframe_drag_preview_moves_only_selected_marker_positions() {
        let states = keyframe_button_states(&snapshot());
        let position = states
            .iter()
            .find(|state| state.track == TrackProp::Position)
            .unwrap();
        let drag = KeyframeDragState {
            marker: AnimationKeyframeSelection {
                sid: position.sid.clone(),
                track: position.track,
                offset_permille: position.offset_permille,
            },
            start_x: 100.0,
            additive: false,
            copy: false,
            active: true,
            delta: Some(KeyframeOffsetDelta::new(100).unwrap()),
            source_markers: vec![AnimationKeyframeSelection {
                sid: position.sid.clone(),
                track: position.track,
                offset_permille: position.offset_permille,
            }],
            preview_markers: vec![AnimationKeyframeSelection {
                sid: position.sid.clone(),
                track: position.track,
                offset_permille: 600,
            }],
            preview_copy_markers: Vec::new(),
        };
        assert_eq!(displayed_keyframe_offset(position, Some(&drag)), 600);
        assert_eq!(displayed_keyframe_offset(position, None), 500);
        let opacity = states
            .iter()
            .find(|state| state.track == TrackProp::Opacity)
            .unwrap();
        assert_eq!(displayed_keyframe_offset(opacity, Some(&drag)), 250);
    }
}
