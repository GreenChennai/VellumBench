//! 新宿主项目画布(R0/R1):渲染 `vb_session` 投影并通过同一会话派发命令。
//!
//! Select 更新有序选择态,Hand 更新视口平移,Rect 通过可逆命令创建矩形;
//! undo/redo/save 与图像置入仍由 `ProjectSession` 操作唯一文档。复杂图像变换、裁剪、
//! 滤镜、富文本等当前 route-c 不完整支持的内容会计入告警,不宣称为完整保真渲染。

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use sable::gpui::{
    canvas, div, px, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, Styled as _,
    WeakEntity, Window,
};
use sable::kurbo::{Point as KurboPoint, Shape as _};
use sable::widgets::controls::button::{Button, ButtonSize, ButtonVariant};
use sable::widgets::prelude::{h_flex, v_flex, CommandPalette};
use sable::widgets::theme::theme;
use sable::widgets::tokens::{ColorTokens, FONT_SIZE_BODY};
use vb_render::encode::{DrawKind, DrawList, FillDef};
use vb_session::command::CommandOutcome;
use vb_session::i18n::{t, t_args, FluentValue};
use vb_session::project::{CanvasRect, ProjectSession, SnapPreview};
use vb_session::selection::ModifierState;
use vb_session::tools::ToolId;

use crate::route_gpui;
use crate::tokens::{font_size, radius, space};

/// 新宿主画布视图;文档事实仍由 `ProjectSession` 持有。
/// 宿主栅格图像选择与置入动作;`None` 表示用户取消选择。
pub type PlaceImageAction = Rc<dyn Fn(&mut Window, &mut App) -> Result<Option<String>, String>>;
/// 宿主项目文件命令;文件系统与窗口创建留在 `vb_shell`。
pub type ProjectFileAction =
    Rc<dyn Fn(&str, &mut Window, &mut App) -> Result<Option<String>, String>>;

pub struct CanvasPanel {
    focus_handle: FocusHandle,
    session: Option<Entity<ProjectSession>>,
    place_image: PlaceImageAction,
    project_file: ProjectFileAction,
    title: Option<String>,
    active_artboard_index: usize,
    artboard_count: usize,
    draw_list: Option<Arc<DrawList>>,
    error: Option<String>,
    warning_count: usize,
    /// `None` = 适合画布;`Some` = 固定缩放比例。
    zoom: Option<f64>,
    /// 最近一次布局计算的适配比例,缩放按钮以它为 Fit 模式的基数。
    fit_scale: Arc<AtomicU64>,
    /// 用户平移量(屏幕像素),不写入文档。
    pan_offset: [f64; 2],
    drag_last: Option<[f64; 2]>,
    selection_drag: Option<SelectionDrag>,
    shape_start: Option<[f64; 2]>,
    shape_preview: Option<[f64; 4]>,
    status_message: Option<String>,
    command_palette: Entity<CommandPalette>,
    command_palette_error: Option<String>,
    palette_status: Rc<RefCell<Option<String>>>,
    buttons: Option<CanvasButtons>,
    viewport_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    image_cache: Arc<Mutex<route_gpui::ImageCache>>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ActionButtonState {
    can_undo: bool,
    can_redo: bool,
    can_delete: bool,
    can_copy: bool,
    can_cut: bool,
    can_paste: bool,
    is_dirty: bool,
    artboard_count: usize,
}

#[derive(Clone)]
struct SelectionDrag {
    start: [f64; 2],
    current: [f64; 2],
    sids: Vec<String>,
    preview_delta: [f64; 2],
    snap_guides: Vec<[f64; 4]>,
}

struct CanvasButtons {
    tool_select: Entity<Button>,
    tool_rect: Entity<Button>,
    tool_ellipse: Entity<Button>,
    tool_artboard: Entity<Button>,
    tool_text: Entity<Button>,
    tool_hand: Entity<Button>,
    previous_artboard: Entity<Button>,
    next_artboard: Entity<Button>,
    grid: Entity<Button>,
    place_image: Entity<Button>,
    undo: Entity<Button>,
    redo: Entity<Button>,
    delete: Entity<Button>,
    save: Entity<Button>,
    export: Entity<Button>,
    copy: Entity<Button>,
    cut: Entity<Button>,
    paste: Entity<Button>,
    command_palette: Entity<Button>,
    action_state: ActionButtonState,
    zoom_out: Entity<Button>,
    zoom_fit: Entity<Button>,
    zoom_actual: Entity<Button>,
    zoom_in: Entity<Button>,
}

impl CanvasPanel {
    /// 从共享项目会话构造画布;导入失败时仍返回可呈现的错误面板。
    pub fn new(
        session: Option<Entity<ProjectSession>>,
        error: Option<String>,
        place_image: PlaceImageAction,
        project_file: ProjectFileAction,
        cx: &mut App,
    ) -> Entity<Self> {
        let (palette_entries, command_palette_error) = match crate::command_palette::entries() {
            Ok(entries) => (entries, None),
            Err(error) => (Vec::new(), Some(error)),
        };
        let palette_status = Rc::new(RefCell::new(None));
        let palette_status_for_execute = palette_status.clone();
        let palette_session = session.clone();
        let palette_place_image = place_image.clone();
        let palette_project_file = project_file.clone();
        let panel = cx.new(|cx| {
            let command_palette = cx.new(|_| {
                CommandPalette::new()
                    .with_entries(palette_entries)
                    .with_on_execute(move |entry, window, app| {
                        if entry.id.as_ref() == "file.place_image" {
                            let message = match palette_place_image(window, app) {
                                Ok(Some(_)) => Some(t("ui-canvas-image-added")),
                                Ok(None) => None,
                                Err(error) => Some(error),
                            };
                            if let Some(message) = message {
                                *palette_status_for_execute.borrow_mut() = Some(message);
                            }
                            return;
                        }
                        if matches!(
                            entry.id.as_ref(),
                            "file.new" | "file.open" | "file.export_dialog"
                        ) {
                            let message = match palette_project_file(entry.id.as_ref(), window, app)
                            {
                                Ok(Some(message)) => Some(message),
                                Ok(None) => None,
                                Err(error) => Some(error),
                            };
                            if let Some(message) = message {
                                *palette_status_for_execute.borrow_mut() = Some(message);
                            }
                            return;
                        }
                        let status = palette_session.as_ref().map(|session| {
                            session.update(app, |project, session_cx| {
                                let outcome = project.dispatch_catalog(entry.id.as_ref());
                                session_cx.notify();
                                outcome
                            })
                        });
                        let message = match status {
                            Some(CommandOutcome::Applied) => t("ui-canvas-command-applied"),
                            Some(CommandOutcome::NotRun { reason }) => reason,
                            None => t("ui-canvas-no-project-session"),
                        };
                        *palette_status_for_execute.borrow_mut() = Some(message);
                    })
            });
            let mut panel = Self {
                focus_handle: cx.focus_handle(),
                command_palette,
                session: session.clone(),
                place_image: place_image.clone(),
                project_file: project_file.clone(),
                title: None,
                active_artboard_index: 0,
                artboard_count: 0,
                draw_list: None,
                error,
                warning_count: 0,
                zoom: None,
                fit_scale: Arc::new(AtomicU64::new(1.0_f64.to_bits())),
                pan_offset: [0.0, 0.0],
                drag_last: None,
                selection_drag: None,
                shape_start: None,
                shape_preview: None,
                status_message: None,
                command_palette_error,
                palette_status: palette_status.clone(),
                buttons: None,
                viewport_bounds: Rc::new(Cell::new(None)),
                image_cache: Arc::new(Mutex::new(route_gpui::ImageCache::default())),
            };
            panel.refresh_projection(cx);
            panel
        });
        if let Some(session) = session {
            panel.update(cx, |_, cx| {
                cx.observe(&session, |panel, _, cx| {
                    if let Some(message) = panel.palette_status.borrow_mut().take() {
                        panel.status_message = Some(message);
                    }
                    panel.refresh_projection(cx);
                    cx.notify();
                })
                .detach();
            });
        }
        panel
    }

    fn action_button_state(&self, cx: &App) -> ActionButtonState {
        self.session
            .as_ref()
            .map(|session| {
                let session = session.read(cx);
                ActionButtonState {
                    can_undo: session.can_undo(),
                    can_redo: session.can_redo(),
                    can_delete: session.can_delete_selection(),
                    can_copy: session.can_copy_selection(),
                    can_cut: session.can_cut_selection(),
                    can_paste: session.can_paste_selection(),
                    is_dirty: session.is_dirty(),
                    artboard_count: session.artboard_count(),
                }
            })
            .unwrap_or_default()
    }

    fn command_button(
        cx: &mut Context<Self>,
        weak: WeakEntity<Self>,
        element_id: &'static str,
        command_id: &'static str,
        label: String,
        disabled: bool,
    ) -> Entity<Button> {
        cx.new(|_| {
            Button::new(element_id, label)
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Default)
                .disabled(disabled)
                .on_press(move |_, window, cx: &mut App| {
                    if let Some(panel) = weak.upgrade() {
                        panel.update(cx, |panel, cx| {
                            panel.focus_handle.focus(window);
                            panel.execute_catalog_command(command_id, window, cx);
                        });
                    }
                })
        })
    }

    fn ensure_buttons(&mut self, cx: &mut Context<Self>) {
        if self.draw_list.is_none() {
            return;
        }
        let action_state = self.action_button_state(cx);
        let weak = cx.entity().downgrade();
        if self
            .buttons
            .as_ref()
            .is_some_and(|buttons| buttons.action_state != action_state)
        {
            let undo = Self::command_button(
                cx,
                weak.clone(),
                "canvas-edit-undo",
                "edit.undo",
                t("cmd-edit-undo"),
                !action_state.can_undo,
            );
            let redo = Self::command_button(
                cx,
                weak.clone(),
                "canvas-edit-redo",
                "edit.redo",
                t("cmd-edit-redo"),
                !action_state.can_redo,
            );
            let save = Self::command_button(
                cx,
                weak.clone(),
                "canvas-file-save",
                "file.save",
                t("cmd-file-save"),
                !action_state.is_dirty,
            );
            let delete = Self::command_button(
                cx,
                weak.clone(),
                "canvas-object-delete",
                "object.delete",
                t("cmd-object-delete"),
                !action_state.can_delete,
            );
            let copy = Self::command_button(
                cx,
                weak.clone(),
                "canvas-edit-copy",
                "edit.copy",
                t("cmd-edit-copy"),
                !action_state.can_copy,
            );
            let cut = Self::command_button(
                cx,
                weak.clone(),
                "canvas-edit-cut",
                "edit.cut",
                t("cmd-edit-cut"),
                !action_state.can_cut,
            );
            let paste = Self::command_button(
                cx,
                weak.clone(),
                "canvas-edit-paste",
                "edit.paste",
                t("cmd-edit-paste"),
                !action_state.can_paste,
            );
            let previous_artboard = Self::command_button(
                cx,
                weak.clone(),
                "canvas-view-prev-artboard",
                "view.prev_artboard",
                t("cmd-view-prev-artboard"),
                action_state.artboard_count <= 1,
            );
            let next_artboard = Self::command_button(
                cx,
                weak.clone(),
                "canvas-view-next-artboard",
                "view.next_artboard",
                t("cmd-view-next-artboard"),
                action_state.artboard_count <= 1,
            );
            if let Some(buttons) = self.buttons.as_mut() {
                buttons.undo = undo;
                buttons.redo = redo;
                buttons.delete = delete;
                buttons.save = save;
                buttons.copy = copy;
                buttons.cut = cut;
                buttons.paste = paste;
                buttons.previous_artboard = previous_artboard;
                buttons.next_artboard = next_artboard;
                buttons.action_state = action_state;
            }
            return;
        }
        if self.buttons.is_some() {
            return;
        }

        let tool_select = Self::command_button(
            cx,
            weak.clone(),
            "canvas-tool-select",
            "tool.select",
            t("cmd-tool-select"),
            false,
        );
        let tool_rect = Self::command_button(
            cx,
            weak.clone(),
            "canvas-tool-rect",
            "tool.rect",
            t("cmd-tool-rect"),
            false,
        );
        let tool_ellipse = Self::command_button(
            cx,
            weak.clone(),
            "canvas-tool-ellipse",
            "tool.ellipse",
            t("cmd-tool-ellipse"),
            false,
        );
        let tool_artboard = Self::command_button(
            cx,
            weak.clone(),
            "canvas-tool-artboard",
            "tool.artboard",
            t("cmd-tool-artboard"),
            false,
        );
        let tool_text = Self::command_button(
            cx,
            weak.clone(),
            "canvas-tool-text",
            "tool.text",
            t("cmd-tool-text"),
            false,
        );
        let tool_hand = Self::command_button(
            cx,
            weak.clone(),
            "canvas-tool-hand",
            "tool.hand",
            t("cmd-tool-hand"),
            false,
        );
        let previous_artboard = Self::command_button(
            cx,
            weak.clone(),
            "canvas-view-prev-artboard",
            "view.prev_artboard",
            t("cmd-view-prev-artboard"),
            self.session.is_none() || action_state.artboard_count <= 1,
        );
        let next_artboard = Self::command_button(
            cx,
            weak.clone(),
            "canvas-view-next-artboard",
            "view.next_artboard",
            t("cmd-view-next-artboard"),
            self.session.is_none() || action_state.artboard_count <= 1,
        );
        let undo = Self::command_button(
            cx,
            weak.clone(),
            "canvas-edit-undo",
            "edit.undo",
            t("cmd-edit-undo"),
            !action_state.can_undo,
        );
        let redo = Self::command_button(
            cx,
            weak.clone(),
            "canvas-edit-redo",
            "edit.redo",
            t("cmd-edit-redo"),
            !action_state.can_redo,
        );
        let save = Self::command_button(
            cx,
            weak.clone(),
            "canvas-file-save",
            "file.save",
            t("cmd-file-save"),
            !action_state.is_dirty,
        );
        let export = Self::command_button(
            cx,
            weak.clone(),
            "canvas-file-export",
            "file.export_dialog",
            t("cmd-file-export-dialog"),
            self.session.is_none(),
        );
        let delete = Self::command_button(
            cx,
            weak.clone(),
            "canvas-object-delete",
            "object.delete",
            t("cmd-object-delete"),
            !action_state.can_delete,
        );
        let copy = Self::command_button(
            cx,
            weak.clone(),
            "canvas-edit-copy",
            "edit.copy",
            t("cmd-edit-copy"),
            !action_state.can_copy,
        );
        let cut = Self::command_button(
            cx,
            weak.clone(),
            "canvas-edit-cut",
            "edit.cut",
            t("cmd-edit-cut"),
            !action_state.can_cut,
        );
        let paste = Self::command_button(
            cx,
            weak.clone(),
            "canvas-edit-paste",
            "edit.paste",
            t("cmd-edit-paste"),
            !action_state.can_paste,
        );
        let command_palette = cx.new(|_| {
            Button::new("canvas-command-palette", t("cmd-app-command-palette"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Default)
                .disabled(self.session.is_none())
                .on_press({
                    let weak = weak.clone();
                    move |_, window, cx: &mut App| {
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| {
                                panel.open_command_palette(window, cx);
                            });
                        }
                    }
                })
        });
        let grid = Self::command_button(
            cx,
            weak.clone(),
            "canvas-view-toggle-grid",
            "view.toggle_grid",
            t("ui-canvas-grid"),
            self.session.is_none(),
        );
        let place_image_action = self.place_image.clone();
        let weak_for_place_image = weak.clone();
        let place_image = cx.new(|_| {
            Button::new("canvas-file-place-image", t("cmd-file-place-image"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Default)
                .disabled(self.session.is_none())
                .on_press(move |_, window, app: &mut App| {
                    let result = place_image_action(window, app);
                    if let Some(panel) = weak_for_place_image.upgrade() {
                        panel.update(app, |panel, cx| {
                            match result {
                                Ok(Some(_)) => {
                                    panel.refresh_projection(cx);
                                    panel.status_message = Some(t("ui-canvas-image-added"));
                                }
                                Ok(None) => {}
                                Err(error) => panel.status_message = Some(error),
                            }
                            cx.notify();
                        });
                    }
                })
        });
        let zoom_out = cx.new(|_| {
            Button::new("canvas-zoom-out", t("ui-canvas-zoom-out"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Default)
                .on_press({
                    let weak = weak.clone();
                    move |_, _, cx: &mut App| {
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| panel.zoom_out(cx));
                        }
                    }
                })
        });
        let zoom_fit = cx.new(|_| {
            Button::new("canvas-zoom-fit", t("ui-canvas-zoom-fit"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Default)
                .on_press({
                    let weak = weak.clone();
                    move |_, _, cx: &mut App| {
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| panel.set_zoom(None, cx));
                        }
                    }
                })
        });
        let zoom_actual = cx.new(|_| {
            Button::new("canvas-zoom-100", t("ui-canvas-zoom-100"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Default)
                .on_press({
                    let weak = weak.clone();
                    move |_, _, cx: &mut App| {
                        if let Some(panel) = weak.upgrade() {
                            panel.update(cx, |panel, cx| panel.set_zoom(Some(1.0), cx));
                        }
                    }
                })
        });
        let zoom_in = cx.new(|_| {
            Button::new("canvas-zoom-in", t("ui-canvas-zoom-in"))
                .variant(ButtonVariant::Secondary)
                .size(ButtonSize::Default)
                .on_press(move |_, _, cx: &mut App| {
                    if let Some(panel) = weak.upgrade() {
                        panel.update(cx, |panel, cx| panel.zoom_in(cx));
                    }
                })
        });
        self.buttons = Some(CanvasButtons {
            tool_select,
            tool_rect,
            tool_ellipse,
            tool_artboard,
            tool_text,
            tool_hand,
            previous_artboard,
            next_artboard,
            grid,
            place_image,
            undo,
            redo,
            delete,
            save,
            export,
            copy,
            cut,
            paste,
            command_palette,
            action_state,
            zoom_out,
            zoom_fit,
            zoom_actual,
            zoom_in,
        });
    }

    fn set_zoom(&mut self, zoom: Option<f64>, cx: &mut Context<Self>) {
        self.zoom = zoom.map(|value| value.clamp(0.05, 8.0));
        if self.zoom.is_none() {
            self.pan_offset = [0.0, 0.0];
        }
        cx.notify();
    }

    fn zoom_in(&mut self, cx: &mut Context<Self>) {
        let fit_scale = f64::from_bits(self.fit_scale.load(Ordering::Relaxed));
        self.set_zoom(Some(step_zoom(self.zoom, fit_scale, 1.25)), cx);
    }

    fn zoom_out(&mut self, cx: &mut Context<Self>) {
        let fit_scale = f64::from_bits(self.fit_scale.load(Ordering::Relaxed));
        self.set_zoom(Some(step_zoom(self.zoom, fit_scale, 0.8)), cx);
    }

    fn dispatch_catalog(&mut self, command_id: &'static str, cx: &mut Context<Self>) {
        let outcome = self.session.as_ref().map(|session| {
            session.update(cx, |session, session_cx| {
                let outcome = session.dispatch_catalog(command_id);
                session_cx.notify();
                outcome
            })
        });
        let Some(outcome) = outcome else {
            return;
        };
        match outcome {
            vb_session::command::CommandOutcome::Applied => {
                self.status_message = None;
                if matches!(command_id, "view.next_artboard" | "view.prev_artboard") {
                    self.zoom = None;
                    self.pan_offset = [0.0, 0.0];
                }
                self.drag_last = None;
                self.selection_drag = None;
                self.shape_start = None;
                self.shape_preview = None;
                if matches!(
                    command_id,
                    "edit.undo"
                        | "edit.redo"
                        | "edit.cut"
                        | "edit.paste"
                        | "edit.paste_in_place"
                        | "file.save"
                        | "object.delete"
                        | "view.next_artboard"
                        | "view.prev_artboard"
                ) {
                    self.refresh_projection(cx);
                }
            }
            vb_session::command::CommandOutcome::NotRun { reason } => {
                self.status_message = Some(reason);
            }
        }
        cx.notify();
    }

    fn execute_catalog_command(
        &mut self,
        command_id: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(command_id, "file.new" | "file.open" | "file.export_dialog") {
            let action = self.project_file.clone();
            match action(command_id, window, cx) {
                Ok(Some(message)) => self.status_message = Some(message),
                Ok(None) => {}
                Err(error) => self.status_message = Some(error),
            }
            cx.notify();
        } else {
            self.dispatch_catalog(command_id, cx);
        }
    }

    fn refresh_projection(&mut self, cx: &App) {
        let result = self.session.as_ref().map(|session| {
            let session = session.read(cx);
            session.active_canvas_snapshot_at(session.animation_snapshot().playhead_seconds)
        });
        match result {
            Some(Ok(snapshot)) => {
                self.warning_count = snapshot.warnings.len()
                    + preview_limitation_count(&snapshot.draw_list)
                    + usize::from(snapshot.synthetic_artboard);
                self.title = Some(snapshot.title);
                self.active_artboard_index = snapshot.artboard_index;
                self.artboard_count = snapshot.artboard_count;
                self.draw_list = Some(Arc::new(snapshot.draw_list));
                self.error = None;
            }
            Some(Err(error)) => {
                self.error = Some(error.to_string());
                self.active_artboard_index = 0;
                self.artboard_count = 0;
                self.draw_list = None;
            }
            None => {
                self.active_artboard_index = 0;
                self.artboard_count = 0;
            }
        }
    }

    fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.is_none() || self.command_palette_error.is_some() {
            self.status_message = self
                .command_palette_error
                .clone()
                .or_else(|| self.error.clone());
            cx.notify();
            return;
        }
        let opener = Some(self.focus_handle.clone());
        self.command_palette.update(cx, |palette, palette_cx| {
            palette.open(opener, window, palette_cx)
        });
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.is_held {
            return;
        }
        let modifiers = event.keystroke.modifiers;
        if let Some(command_id) = catalog_command_for_key(
            event.keystroke.key.as_str(),
            modifiers.platform,
            modifiers.shift,
            modifiers.alt,
            modifiers.modified(),
        ) {
            if command_id == "app.command_palette" {
                self.open_command_palette(window, cx);
            } else {
                self.execute_catalog_command(command_id, window, cx);
            }
        } else if event.keystroke.key.as_str().eq_ignore_ascii_case("escape") {
            let cancelled_shape = self.shape_start.take().is_some();
            let cancelled_move = self.selection_drag.take().is_some();
            if cancelled_shape || cancelled_move {
                self.shape_preview = None;
                cx.notify();
            }
        }
    }

    fn current_zoom(&self) -> f64 {
        self.zoom
            .unwrap_or_else(|| f64::from_bits(self.fit_scale.load(Ordering::Relaxed)))
    }

    fn document_point(&self, position: Point<Pixels>) -> Option<[f64; 2]> {
        let bounds = self.viewport_bounds.get()?;
        let draw_list = self.draw_list.as_deref()?;
        let viewport = [f64::from(bounds.size.width), f64::from(bounds.size.height)];
        let zoom = self.current_zoom();
        let local_position = [
            f64::from(position.x) - f64::from(bounds.origin.x),
            f64::from(position.y) - f64::from(bounds.origin.y),
        ];
        screen_to_document(
            local_position,
            viewport,
            zoom,
            self.pan_offset,
            [draw_list.w, draw_list.h],
        )
    }

    fn handle_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window);
        self.selection_drag = None;
        let tool = self
            .session
            .as_ref()
            .map(|session| session.read(cx).active_tool())
            .unwrap_or(ToolId::Select);
        if tool == ToolId::Hand {
            self.shape_start = None;
            self.shape_preview = None;
            self.drag_last = Some([f64::from(event.position.x), f64::from(event.position.y)]);
            return;
        }
        self.drag_last = None;
        if matches!(tool, ToolId::Rect | ToolId::Ellipse | ToolId::Artboard) {
            self.shape_start = self.document_point(event.position);
            self.shape_preview = None;
            cx.notify();
            return;
        }
        self.shape_start = None;
        self.shape_preview = None;
        if tool == ToolId::Text {
            if let Some(point) = self.document_point(event.position) {
                let result = self.session.as_ref().map(|session| {
                    session.update(cx, |session, session_cx| {
                        let result = session.create_text(session.active_artboard_index(), point);
                        session_cx.notify();
                        result
                    })
                });
                match result {
                    Some(Ok(_)) => {
                        self.status_message = None;
                        self.refresh_projection(cx);
                    }
                    Some(Err(error)) => self.status_message = Some(error.to_string()),
                    None => {}
                }
                cx.notify();
            }
            return;
        }
        if tool != ToolId::Select {
            return;
        }

        let start = self.document_point(event.position);
        let sid = start
            .and_then(|point| {
                self.draw_list
                    .as_deref()
                    .and_then(|list| hit_test(list, point))
            })
            .map(|item| item.sid.clone());
        let modifiers = ModifierState {
            shift: event.modifiers.shift,
            control: event.modifiers.control,
            alt: event.modifiers.alt,
            platform: event.modifiers.platform,
        };
        let drag_sids = self.session.as_ref().and_then(|session| {
            session.update(cx, |session, session_cx| {
                let selected = session.begin_pointer_selection(sid.as_deref(), modifiers);
                session_cx.notify();
                selected
            })
        });
        if let (Some(start), Some(sids)) = (start, drag_sids) {
            if !sids.is_empty() {
                self.selection_drag = Some(SelectionDrag {
                    start,
                    current: start,
                    sids,
                    preview_delta: [0.0, 0.0],
                    snap_guides: Vec::new(),
                });
            }
        }
        cx.notify();
    }

    fn handle_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let tool = self
            .session
            .as_ref()
            .map(|session| session.read(cx).active_tool())
            .unwrap_or(ToolId::Select);
        match tool {
            ToolId::Hand => {
                let current = [f64::from(event.position.x), f64::from(event.position.y)];
                if let Some(previous) = self.drag_last {
                    self.pan_offset[0] += current[0] - previous[0];
                    self.pan_offset[1] += current[1] - previous[1];
                    cx.notify();
                }
                self.drag_last = Some(current);
            }
            ToolId::Rect | ToolId::Ellipse | ToolId::Artboard => {
                let Some(start) = self.shape_start else {
                    return;
                };
                let Some(end) = self.document_point(event.position) else {
                    return;
                };
                let preview = shape_from_points(start, end);
                if self.shape_preview != preview {
                    self.shape_preview = preview;
                    cx.notify();
                }
            }
            ToolId::Select => {
                let Some(current) = self.document_point(event.position) else {
                    return;
                };
                let zoom = self.current_zoom();
                let Some(drag) = self.selection_drag.as_ref() else {
                    return;
                };
                if drag.current == current {
                    return;
                }
                let start = drag.start;
                let Some(requested) = move_delta_if_threshold(start, current, zoom) else {
                    let changed = if let Some(drag) = self.selection_drag.as_mut() {
                        let changed = drag.current != current
                            || drag.preview_delta != [0.0, 0.0]
                            || !drag.snap_guides.is_empty();
                        drag.current = current;
                        drag.preview_delta = [0.0, 0.0];
                        drag.snap_guides.clear();
                        changed
                    } else {
                        false
                    };
                    if changed {
                        cx.notify();
                    }
                    return;
                };
                let preview = self
                    .session
                    .as_ref()
                    .map(|session| {
                        session
                            .read(cx)
                            .snap_selection_delta(requested, zoom, event.modifiers.alt)
                    })
                    .unwrap_or(Ok(SnapPreview {
                        delta: requested,
                        guides: Vec::new(),
                    }));
                let preview = match preview {
                    Ok(preview) => preview,
                    Err(error) => {
                        self.status_message = Some(error.to_string());
                        SnapPreview {
                            delta: requested,
                            guides: Vec::new(),
                        }
                    }
                };
                if let Some(drag) = self.selection_drag.as_mut() {
                    let changed = drag.current != current
                        || drag.preview_delta != preview.delta
                        || drag.snap_guides != preview.guides;
                    drag.current = current;
                    drag.preview_delta = preview.delta;
                    drag.snap_guides = preview.guides;
                    if changed {
                        cx.notify();
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_mouse_up(
        &mut self,
        event: &MouseUpEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(start) = self.shape_start.take() {
            let active_tool = self
                .session
                .as_ref()
                .map(|session| session.read(cx).active_tool())
                .unwrap_or(ToolId::Select);
            let end = self.document_point(event.position).unwrap_or(start);
            self.shape_preview = None;
            self.drag_last = None;
            if let Some([x, y, width, height]) = shape_from_points(start, end) {
                let result = self.session.as_ref().map(|session| {
                    session.update(cx, |session, session_cx| {
                        let rect = CanvasRect {
                            x,
                            y,
                            width,
                            height,
                        };
                        let artboard_index = session.active_artboard_index();
                        let result = match active_tool {
                            ToolId::Ellipse => session.create_ellipse(artboard_index, rect),
                            ToolId::Artboard => session.create_artboard(rect),
                            _ => session.create_rectangle(artboard_index, rect),
                        };
                        session_cx.notify();
                        result
                    })
                });
                match result {
                    Some(Ok(_)) => {
                        self.status_message = None;
                        if active_tool == ToolId::Artboard {
                            self.zoom = None;
                            self.pan_offset = [0.0, 0.0];
                        }
                        self.refresh_projection(cx);
                    }
                    Some(Err(error)) => {
                        self.status_message = Some(error.to_string());
                    }
                    None => {}
                }
            }
            cx.notify();
            return;
        }
        if let Some(drag) = self.selection_drag.take() {
            self.drag_last = None;
            if let Some(requested_delta) = move_delta_if_threshold(
                drag.start,
                self.document_point(event.position).unwrap_or(drag.current),
                self.current_zoom(),
            ) {
                let preview = self.session.as_ref().map(|session| {
                    session.read(cx).snap_selection_delta(
                        requested_delta,
                        self.current_zoom(),
                        event.modifiers.alt,
                    )
                });
                match preview {
                    Some(Ok(preview)) => {
                        let result = self.session.as_ref().map(|session| {
                            session.update(cx, |session, session_cx| {
                                if session.selected_sids() != drag.sids.as_slice() {
                                    return Ok(0);
                                }
                                let moved = session.move_selection_by(preview.delta);
                                session_cx.notify();
                                moved
                            })
                        });
                        match result {
                            Some(Ok(moved)) if moved > 0 => {
                                self.status_message = None;
                                self.refresh_projection(cx);
                            }
                            Some(Ok(_)) => {}
                            Some(Err(error)) => self.status_message = Some(error.to_string()),
                            None => {}
                        }
                    }
                    Some(Err(error)) => self.status_message = Some(error.to_string()),
                    None => {}
                }
            }
            cx.notify();
            return;
        }
        if self.drag_last.take().is_some() {
            cx.notify();
        }
    }
}

impl Focusable for CanvasPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn tool_command_for_key(key: &str) -> Option<&'static str> {
    if key.eq_ignore_ascii_case("v") {
        Some("tool.select")
    } else if key.eq_ignore_ascii_case("m") {
        Some("tool.rect")
    } else if key.eq_ignore_ascii_case("l") {
        Some("tool.ellipse")
    } else if key.eq_ignore_ascii_case("h") {
        Some("tool.hand")
    } else if key.eq_ignore_ascii_case("t") {
        Some("tool.text")
    } else {
        None
    }
}

fn catalog_command_for_key(
    key: &str,
    platform_modifier: bool,
    shift: bool,
    alt: bool,
    modified: bool,
) -> Option<&'static str> {
    if !platform_modifier && shift && !alt && key.eq_ignore_ascii_case("o") {
        return Some("tool.artboard");
    }
    if !platform_modifier && !shift && !alt {
        match key.to_ascii_lowercase().as_str() {
            "left" | "arrowleft" | "←" => return Some("canvas.nudge_left"),
            "right" | "arrowright" | "→" => return Some("canvas.nudge_right"),
            "up" | "arrowup" | "↑" => return Some("canvas.nudge_up"),
            "down" | "arrowdown" | "↓" => return Some("canvas.nudge_down"),
            _ => {}
        }
    }
    if platform_modifier {
        if !shift {
            match key.to_ascii_lowercase().as_str() {
                "pagedown" | "page_down" => return Some("view.next_artboard"),
                "pageup" | "page_up" => return Some("view.prev_artboard"),
                _ => {}
            }
        }
        if alt {
            return None;
        }
        if !shift && (key.eq_ignore_ascii_case("quote") || key == "\"" || key == "'") {
            return Some("view.toggle_grid");
        }
        return match (key.to_ascii_lowercase().as_str(), shift) {
            ("n", false) => Some("file.new"),
            ("o", false) => Some("file.open"),
            ("e", false) => Some("file.export_dialog"),
            ("s", false) => Some("file.save"),
            ("z", false) => Some("edit.undo"),
            ("z", true) => Some("edit.redo"),
            ("a", false) => Some("edit.select_all"),
            ("c", false) => Some("edit.copy"),
            ("x", false) => Some("edit.cut"),
            ("v", false) => Some("edit.paste"),
            ("f", false) => Some("edit.paste_in_place"),
            ("k", false) => Some("app.command_palette"),
            ("g", false) => Some("object.group"),
            ("g", true) => Some("object.ungroup"),
            _ => None,
        };
    }
    if modified {
        return None;
    }
    if key.eq_ignore_ascii_case("delete") {
        Some("object.delete")
    } else {
        tool_command_for_key(key)
    }
}

/// 统计 route-c 当前不能完整表达的功能,避免把近似预览说成保真结果。
fn preview_limitation_count(list: &DrawList) -> usize {
    list.items
        .iter()
        .filter(|item| {
            let gradient_loss = match item.fill.as_ref() {
                Some(FillDef::RadialGradient { .. }) => true,
                Some(FillDef::LinearGradient { stops, .. }) => {
                    stops.len() > 2 || item.path.is_some()
                }
                _ => false,
            };
            let text_loss = item.kind == DrawKind::Text
                && item
                    .label
                    .as_ref()
                    .is_some_and(|hint| !hint.segments.is_empty() || !hint.font_family.is_empty());
            let image_loss = item.kind == DrawKind::Image
                && (item.image.is_none()
                    || item.rot.abs() > 1e-9
                    || item.opacity < 1.0
                    || item.border.is_some());
            image_loss
                || item.kind == DrawKind::FrozenPlaceholder
                || item.clip.is_some()
                || item.overflow_clip.is_some()
                || item.filter.is_some()
                || text_loss
                || gradient_loss
                || (item.ellipse && item.border.is_some())
        })
        .count()
}

fn hit_test(list: &DrawList, point: [f64; 2]) -> Option<&vb_render::encode::DrawItem> {
    list.items.iter().rev().find(|item| {
        let [x, y, width, height] = item.rect;
        if item.sid.is_empty() || item.opacity <= 0.0 || width <= 0.0 || height <= 0.0 {
            return false;
        }

        if let Some(path) = &item.path {
            let local = KurboPoint::new(point[0] - x, point[1] - y);
            return path.contains(local)
                || (item.border.is_some()
                    && point[0] >= x
                    && point[0] <= x + width
                    && point[1] >= y
                    && point[1] <= y + height);
        }

        let (local_x, local_y) = if item.rot.abs() > 1e-9 {
            let center = (x + width / 2.0, y + height / 2.0);
            let (dx, dy) = (point[0] - center.0, point[1] - center.1);
            let (sin, cos) = (-item.rot.to_radians()).sin_cos();
            (
                center.0 + dx * cos - dy * sin,
                center.1 + dx * sin + dy * cos,
            )
        } else {
            (point[0], point[1])
        };

        if item.ellipse {
            let rx = width / 2.0;
            let ry = height / 2.0;
            let dx = (local_x - (x + rx)) / rx;
            let dy = (local_y - (y + ry)) / ry;
            dx * dx + dy * dy <= 1.0
        } else {
            local_x >= x && local_x <= x + width && local_y >= y && local_y <= y + height
        }
    })
}

fn screen_to_document(
    local_position: [f64; 2],
    viewport: [f64; 2],
    zoom: f64,
    pan_offset: [f64; 2],
    document_size: [f64; 2],
) -> Option<[f64; 2]> {
    if viewport
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0)
        || document_size
            .iter()
            .any(|value| !value.is_finite() || *value <= 0.0)
        || !zoom.is_finite()
        || zoom <= 0.0
    {
        return None;
    }
    let x = local_position[0] - pan_offset[0];
    let y = local_position[1] - pan_offset[1];
    Some([
        (x - (viewport[0] - document_size[0] * zoom) / 2.0) / zoom,
        (y - (viewport[1] - document_size[1] * zoom) / 2.0) / zoom,
    ])
}

fn shape_from_points(start: [f64; 2], end: [f64; 2]) -> Option<[f64; 4]> {
    if start
        .iter()
        .chain(end.iter())
        .any(|value| !value.is_finite())
    {
        return None;
    }
    let x = start[0].min(end[0]);
    let y = start[1].min(end[1]);
    let width = (end[0] - start[0]).abs();
    let height = (end[1] - start[1]).abs();
    (width >= 2.0 && height >= 2.0).then_some([x, y, width, height])
}

fn move_delta_if_threshold(start: [f64; 2], end: [f64; 2], zoom: f64) -> Option<[f64; 2]> {
    if start
        .iter()
        .chain(end.iter())
        .any(|value| !value.is_finite())
        || !zoom.is_finite()
        || zoom <= 0.0
    {
        return None;
    }
    let delta = [end[0] - start[0], end[1] - start[1]];
    (delta[0].hypot(delta[1]) * zoom >= 3.0).then_some(delta)
}

fn fit_zoom(list: &DrawList, viewport: [f64; 2]) -> f64 {
    let padding = f64::from(space::S6) * 2.0;
    let available_width = (viewport[0] - padding).max(1.0);
    let available_height = (viewport[1] - padding).max(1.0);
    (available_width / list.w.max(1.0))
        .min(available_height / list.h.max(1.0))
        .clamp(0.05, 8.0)
}

fn step_zoom(zoom: Option<f64>, fit_scale: f64, multiplier: f64) -> f64 {
    (zoom.unwrap_or(fit_scale) * multiplier).clamp(0.05, 8.0)
}

fn zoom_label(zoom: Option<f64>) -> Option<String> {
    zoom.map(|zoom| format!("{:.0}%", zoom * 100.0))
}

fn status_button(button: Entity<Button>, selected: bool, colors: &ColorTokens) -> impl IntoElement {
    let background = if selected {
        colors.accent_muted
    } else {
        colors.surface_1
    };
    let border = if selected {
        colors.accent
    } else {
        colors.border_subtle
    };
    div()
        .px(px(space::S1))
        .py(px(space::S1))
        .rounded(px(radius::SM))
        .bg(background)
        .border_1()
        .border_color(border)
        .child(button)
}

impl Render for CanvasPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(message) = self.palette_status.borrow_mut().take() {
            self.status_message = Some(message);
        }
        self.ensure_buttons(cx);
        let colors = theme(cx).colors;
        let active_tool = self
            .session
            .as_ref()
            .map(|session| session.read(cx).active_tool())
            .unwrap_or(ToolId::Select);
        let grid_visible = self
            .session
            .as_ref()
            .is_some_and(|session| session.read(cx).grid_visible());
        let is_dirty = self
            .session
            .as_ref()
            .is_some_and(|session| session.read(cx).is_dirty());
        let title = self
            .title
            .clone()
            .unwrap_or_else(|| t("ui-canvas-load-failed"));
        let title = if is_dirty {
            format!("{title} *")
        } else {
            title
        };
        let buttons = self.buttons.as_ref();
        let zoom_label = zoom_label(self.zoom);
        let artboard_position = if self.artboard_count == 0 {
            "—".to_string()
        } else {
            format!(
                "{} / {}",
                self.active_artboard_index + 1,
                self.artboard_count
            )
        };
        let mut title_row = h_flex()
            .items_center()
            .gap(px(space::S2))
            .px(px(space::S3))
            .child(
                div()
                    .text_size(px(font_size::LABEL))
                    .text_color(colors.text_primary)
                    .child(title),
            )
            .child(div().flex_1());
        if let Some(buttons) = buttons {
            title_row = title_row
                .child(buttons.previous_artboard.clone())
                .child(
                    div()
                        .text_size(px(font_size::CAPTION))
                        .text_color(colors.text_secondary)
                        .child(artboard_position),
                )
                .child(buttons.next_artboard.clone())
                .child(buttons.export.clone());
        }
        title_row = title_row.child(
            div()
                .px(px(space::S2))
                .py(px(space::S1))
                .rounded(px(radius::SM))
                .bg(colors.accent_muted)
                .text_size(px(font_size::CAPTION))
                .text_color(colors.accent)
                .child(t("ui-canvas-preview-badge")),
        );
        let mut header = v_flex()
            .flex_shrink_0()
            .gap(px(space::S1))
            .py(px(space::S2))
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(title_row);

        if let Some(buttons) = buttons {
            let mut tools_and_zoom = h_flex()
                .items_center()
                .gap(px(space::S1))
                .px(px(space::S3))
                .child(status_button(
                    buttons.tool_select.clone(),
                    active_tool == ToolId::Select,
                    &colors,
                ))
                .child(status_button(
                    buttons.tool_rect.clone(),
                    active_tool == ToolId::Rect,
                    &colors,
                ))
                .child(status_button(
                    buttons.tool_ellipse.clone(),
                    active_tool == ToolId::Ellipse,
                    &colors,
                ))
                .child(status_button(
                    buttons.tool_artboard.clone(),
                    active_tool == ToolId::Artboard,
                    &colors,
                ))
                .child(status_button(
                    buttons.tool_text.clone(),
                    active_tool == ToolId::Text,
                    &colors,
                ))
                .child(status_button(
                    buttons.tool_hand.clone(),
                    active_tool == ToolId::Hand,
                    &colors,
                ))
                .child(div().flex_1())
                .child(buttons.zoom_out.clone())
                .child(status_button(
                    buttons.zoom_fit.clone(),
                    self.zoom.is_none(),
                    &colors,
                ))
                .child(status_button(
                    buttons.zoom_actual.clone(),
                    self.zoom == Some(1.0),
                    &colors,
                ))
                .child(buttons.zoom_in.clone());
            if let Some(zoom_label) = zoom_label {
                tools_and_zoom = tools_and_zoom.child(
                    div()
                        .text_size(px(font_size::CAPTION))
                        .text_color(colors.text_secondary)
                        .child(zoom_label),
                );
            }
            let actions = h_flex()
                .items_center()
                .gap(px(space::S1))
                .px(px(space::S3))
                .child(status_button(buttons.grid.clone(), grid_visible, &colors))
                .child(buttons.place_image.clone())
                .child(buttons.copy.clone())
                .child(buttons.cut.clone())
                .child(buttons.paste.clone())
                .child(buttons.undo.clone())
                .child(buttons.redo.clone())
                .child(buttons.delete.clone())
                .child(buttons.save.clone())
                .child(buttons.command_palette.clone())
                .child(div().flex_1());
            header = header.child(tools_and_zoom).child(actions);
        }
        if let Some(status) = self.status_message.clone() {
            header = header.child(
                h_flex()
                    .px(px(space::S3))
                    .text_size(px(font_size::CAPTION))
                    .text_color(colors.warning)
                    .child(status),
            );
        }

        let body = if let Some(error) = self.error.clone() {
            v_flex()
                .id("canvas-load-error")
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .gap(px(space::S3))
                .px(px(space::S6))
                .bg(colors.surface_0)
                .child(
                    div()
                        .text_size(px(FONT_SIZE_BODY))
                        .text_color(colors.text_primary)
                        .child(t("ui-canvas-load-failed")),
                )
                .child(
                    div()
                        .text_size(px(font_size::LABEL))
                        .text_color(colors.text_secondary)
                        .child(error),
                )
                .into_any_element()
        } else if let Some(draw_list) = self.draw_list.clone() {
            let zoom = self.zoom;
            let pan_offset = self.pan_offset;
            let selected_sids = self
                .session
                .as_ref()
                .map(|session| session.read(cx).selected_sids().to_vec())
                .unwrap_or_default();
            let selection_drag = self.selection_drag.clone();
            let shape_preview = self.shape_preview;
            let preview_ellipse = active_tool == ToolId::Ellipse;
            let fit_scale = Arc::clone(&self.fit_scale);
            let viewport_bounds = Rc::clone(&self.viewport_bounds);
            let image_cache = Arc::clone(&self.image_cache);
            let selection_color = colors.accent;
            let draft_color = colors.accent;
            let grid_color = colors.border_subtle;
            let guide_color = colors.info;
            let canvas = canvas(
                move |bounds: Bounds<Pixels>, window: &mut Window, _cx: &mut App| {
                    viewport_bounds.set(Some(bounds));
                    let viewport = [
                        f64::from(bounds.size.width).max(1.0),
                        f64::from(bounds.size.height).max(1.0),
                    ];
                    let viewport_origin = [f64::from(bounds.origin.x), f64::from(bounds.origin.y)];
                    let fit = fit_zoom(&draw_list, viewport);
                    fit_scale.store(fit.to_bits(), Ordering::Relaxed);
                    let zoom = zoom.unwrap_or(fit);
                    let transform = route_gpui::CanvasTransform {
                        zoom,
                        viewport,
                        viewport_origin,
                        pan_offset,
                    };
                    let move_sids = selection_drag
                        .as_ref()
                        .map(|drag| drag.sids.as_slice())
                        .unwrap_or(&[]);
                    let move_offset = selection_drag.as_ref().map(|drag| drag.preview_delta);
                    let offset = move_offset.unwrap_or([0.0, 0.0]);
                    let snap_guides = selection_drag
                        .as_ref()
                        .map(|drag| drag.snap_guides.clone())
                        .unwrap_or_default();
                    let mut prims = if grid_visible {
                        route_gpui::grid_primitives(&draw_list, transform, grid_color)
                    } else {
                        Vec::new()
                    };
                    let (mut document_prims, _) = {
                        let mut image_cache = image_cache
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        route_gpui::translate_with_cache(
                            &draw_list,
                            transform,
                            move_sids,
                            offset,
                            &mut image_cache,
                            window.text_system(),
                        )
                    };
                    prims.append(&mut document_prims);
                    prims.extend(route_gpui::snap_guide_primitives(
                        &draw_list,
                        transform,
                        &snap_guides,
                        guide_color,
                    ));
                    for item in &draw_list.items {
                        if selected_sids.iter().any(|sid| sid == &item.sid) {
                            let item_offset = if move_sids.iter().any(|sid| sid == &item.sid) {
                                offset
                            } else {
                                [0.0, 0.0]
                            };
                            if let Some(outline) = route_gpui::selection_outline_with_offset(
                                &draw_list,
                                item,
                                transform,
                                item_offset,
                                selection_color,
                            ) {
                                prims.push(outline);
                            }
                        }
                    }
                    if let Some(rect) = shape_preview {
                        let preview = if preview_ellipse {
                            route_gpui::draft_ellipse(
                                &draw_list,
                                rect,
                                zoom,
                                viewport,
                                viewport_origin,
                                pan_offset,
                                draft_color,
                            )
                        } else {
                            route_gpui::draft_rectangle(
                                &draw_list,
                                rect,
                                zoom,
                                viewport,
                                viewport_origin,
                                pan_offset,
                                draft_color,
                            )
                        };
                        if let Some(preview) = preview {
                            prims.push(preview);
                        }
                    }
                    prims
                },
                |_bounds: Bounds<Pixels>, prims, window: &mut Window, cx: &mut App| {
                    route_gpui::paint_prims(&prims, window, cx);
                },
            )
            .size_full();
            let canvas = div()
                .size_full()
                .child(canvas)
                .on_mouse_down(MouseButton::Left, cx.listener(Self::handle_mouse_down))
                .on_mouse_move(cx.listener(Self::handle_mouse_move))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::handle_mouse_up));

            let mut content = v_flex()
                .id("canvas-preview")
                .flex_1()
                .min_h_0()
                .bg(colors.surface_0)
                .child(canvas);
            if self.warning_count > 0 {
                content = content.child(
                    div()
                        .flex_shrink_0()
                        .px(px(space::S3))
                        .py(px(space::S2))
                        .text_size(px(font_size::CAPTION))
                        .text_color(colors.warning)
                        .border_t_1()
                        .border_color(colors.border_subtle)
                        .child(t_args(
                            "ui-canvas-preview-warning",
                            &[("count", FluentValue::from(self.warning_count as i64))],
                        )),
                );
            }
            content.into_any_element()
        } else {
            div()
                .id("canvas-preview-empty")
                .flex_1()
                .bg(colors.surface_0)
                .into_any_element()
        };

        v_flex()
            .id("canvas-panel")
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.handle_key_down(event, window, cx);
            }))
            .bg(colors.surface_0)
            .text_color(colors.text_primary)
            .child(header)
            .child(body)
            .child(self.command_palette.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{
        catalog_command_for_key, hit_test, move_delta_if_threshold, preview_limitation_count,
        screen_to_document, shape_from_points, step_zoom, tool_command_for_key, zoom_label,
    };
    use vb_render::encode::{BitmapData, DrawItem, DrawKind, DrawList, FillDef};

    const COMMANDS_YAML: &str = include_str!("../../../commands.yaml");

    fn catalog_windows_key(catalog_id: &str) -> Option<String> {
        let mut matches_command = false;
        for line in COMMANDS_YAML.lines() {
            let line = line.trim();
            if let Some(id) = line.strip_prefix("- id: ") {
                matches_command = id == catalog_id;
                continue;
            }
            if matches_command && line.starts_with("keys:") {
                let (_, windows) = line.split_once("win:")?;
                return windows
                    .trim()
                    .strip_prefix('\"')?
                    .split_once('\"')
                    .map(|(key, _)| key.replace("\\\\", "\\"));
            }
        }
        None
    }

    fn item(sid: &str, rect: [f64; 4], opacity: f32, ellipse: bool) -> DrawItem {
        DrawItem {
            sid: sid.to_string(),
            rect,
            ellipse,
            radii: [0.0; 4],
            fill: Some(FillDef::Solid([0.2, 0.4, 0.8, 1.0])),
            border: None,
            opacity,
            layer: 0,
            kind: DrawKind::Box,
            label: None,
            src: None,
            rot: 0.0,
            path: None,
            image: None,
            clip: None,
            overflow_clip: None,
            filter: None,
        }
    }

    fn list(items: Vec<DrawItem>) -> DrawList {
        DrawList {
            w: 300.0,
            h: 200.0,
            background: [1.0; 4],
            items,
        }
    }

    #[test]
    fn supported_static_images_are_not_marked_as_approximate() {
        let mut image = item("image", [0.0, 0.0, 20.0, 20.0], 1.0, false);
        image.kind = DrawKind::Image;
        image.image = Some(BitmapData {
            width: 1,
            height: 1,
            rgba: Arc::new(vec![255, 0, 0, 255]),
        });
        assert_eq!(preview_limitation_count(&list(vec![image.clone()])), 0);

        image.opacity = 0.5;
        assert_eq!(preview_limitation_count(&list(vec![image])), 1);
    }

    #[test]
    fn selection_move_starts_after_three_screen_pixels() {
        assert_eq!(move_delta_if_threshold([0.0, 0.0], [1.0, 0.0], 2.0), None);
        assert_eq!(
            move_delta_if_threshold([0.0, 0.0], [1.5, 0.0], 2.0),
            Some([1.5, 0.0])
        );
        assert_eq!(
            move_delta_if_threshold([10.0, 10.0], [10.0, 12.0], 1.5),
            Some([0.0, 2.0])
        );
        assert_eq!(
            move_delta_if_threshold([0.0, 0.0], [f64::NAN, 1.0], 1.0),
            None
        );
    }

    #[test]
    fn zoom_steps_from_fit_scale() {
        assert!((step_zoom(None, 0.375, 0.8) - 0.3).abs() < 1.0e-9);
        assert!((step_zoom(None, 0.375, 1.25) - 0.46875).abs() < 1.0e-9);
    }

    #[test]
    fn zoom_steps_use_explicit_scale() {
        assert_eq!(step_zoom(Some(1.0), 0.375, 0.8), 0.8);
    }

    #[test]
    fn zoom_label_is_hidden_in_fit_mode() {
        assert_eq!(zoom_label(None), None);
        assert_eq!(zoom_label(Some(0.5)).as_deref(), Some("50%"));
    }

    #[test]
    fn canvas_tool_and_document_shortcuts_match_command_catalog() {
        assert_eq!(tool_command_for_key("v"), Some("tool.select"));
        assert_eq!(tool_command_for_key("M"), Some("tool.rect"));
        assert_eq!(tool_command_for_key("L"), Some("tool.ellipse"));
        assert_eq!(tool_command_for_key("T"), Some("tool.text"));
        assert_eq!(tool_command_for_key("H"), Some("tool.hand"));
        assert_eq!(tool_command_for_key("p"), None);
        assert_eq!(catalog_windows_key("tool.select").as_deref(), Some("V"));
        assert_eq!(catalog_windows_key("tool.rect").as_deref(), Some("M"));
        assert_eq!(catalog_windows_key("tool.ellipse").as_deref(), Some("L"));
        assert_eq!(
            catalog_windows_key("tool.artboard").as_deref(),
            Some("Shift+O")
        );
        assert_eq!(
            catalog_command_for_key("o", false, true, false, true),
            Some("tool.artboard")
        );
        assert_eq!(catalog_windows_key("tool.text").as_deref(), Some("T"));
        assert_eq!(catalog_windows_key("tool.hand").as_deref(), Some("H"));
        assert_eq!(catalog_windows_key("object.delete").as_deref(), Some("Del"));
        assert_eq!(catalog_windows_key("edit.undo").as_deref(), Some("Ctrl+Z"));
        assert_eq!(
            catalog_windows_key("edit.redo").as_deref(),
            Some("Ctrl+Shift+Z")
        );
        assert_eq!(catalog_windows_key("edit.copy").as_deref(), Some("Ctrl+C"));
        assert_eq!(catalog_windows_key("edit.cut").as_deref(), Some("Ctrl+X"));
        assert_eq!(catalog_windows_key("edit.paste").as_deref(), Some("Ctrl+V"));
        assert_eq!(
            catalog_windows_key("edit.paste_in_place").as_deref(),
            Some("Ctrl+F")
        );
        assert_eq!(
            catalog_windows_key("edit.select_all").as_deref(),
            Some("Ctrl+A")
        );
        assert_eq!(
            catalog_command_for_key("a", true, false, false, true),
            Some("edit.select_all")
        );
        assert_eq!(
            catalog_windows_key("object.group").as_deref(),
            Some("Ctrl+G")
        );
        assert_eq!(
            catalog_windows_key("object.ungroup").as_deref(),
            Some("Ctrl+Shift+G")
        );
        assert_eq!(
            catalog_command_for_key("g", true, false, false, true),
            Some("object.group")
        );
        assert_eq!(
            catalog_command_for_key("g", true, true, false, true),
            Some("object.ungroup")
        );
        assert_eq!(
            catalog_windows_key("align.left").as_deref(),
            Some("Ctrl+Shift+L")
        );
        assert_eq!(
            catalog_windows_key("align.hcenter").as_deref(),
            Some("Ctrl+Shift+C")
        );
        assert_eq!(
            catalog_windows_key("align.right").as_deref(),
            Some("Ctrl+Shift+R")
        );
        assert_eq!(
            catalog_windows_key("align.top").as_deref(),
            Some("Ctrl+Shift+T")
        );
        assert_eq!(
            catalog_windows_key("align.vcenter").as_deref(),
            Some("Ctrl+Shift+V")
        );
        assert_eq!(
            catalog_windows_key("align.bottom").as_deref(),
            Some("Ctrl+Shift+B")
        );
        assert_eq!(catalog_windows_key("file.new").as_deref(), Some("Ctrl+N"));
        assert_eq!(catalog_windows_key("file.open").as_deref(), Some("Ctrl+O"));
        assert_eq!(
            catalog_windows_key("file.export_dialog").as_deref(),
            Some("Ctrl+E")
        );
        assert_eq!(
            catalog_windows_key("canvas.nudge_left").as_deref(),
            Some("←")
        );
        assert_eq!(
            catalog_windows_key("canvas.nudge_right").as_deref(),
            Some("→")
        );
        assert_eq!(catalog_windows_key("canvas.nudge_up").as_deref(), Some("↑"));
        assert_eq!(
            catalog_windows_key("canvas.nudge_down").as_deref(),
            Some("↓")
        );
        assert_eq!(
            catalog_command_for_key("ArrowLeft", false, false, false, false),
            Some("canvas.nudge_left")
        );
        assert_eq!(
            catalog_command_for_key("ArrowRight", false, false, false, false),
            Some("canvas.nudge_right")
        );
        assert_eq!(
            catalog_command_for_key("ArrowUp", false, false, false, false),
            Some("canvas.nudge_up")
        );
        assert_eq!(
            catalog_command_for_key("ArrowDown", false, false, false, false),
            Some("canvas.nudge_down")
        );
        assert_eq!(
            catalog_command_for_key("ArrowLeft", false, true, false, true),
            None
        );
        assert_eq!(catalog_windows_key("file.save").as_deref(), Some("Ctrl+S"));
        assert_eq!(
            catalog_windows_key("app.command_palette").as_deref(),
            Some("Ctrl+K")
        );
        assert_eq!(
            catalog_windows_key("view.toggle_grid").as_deref(),
            Some("Ctrl+Quote")
        );
        assert_eq!(
            catalog_windows_key("view.next_artboard").as_deref(),
            Some("Ctrl+PageDown")
        );
        assert_eq!(
            catalog_windows_key("view.prev_artboard").as_deref(),
            Some("Ctrl+PageUp")
        );
        assert_eq!(
            catalog_command_for_key("PageDown", true, false, false, true),
            Some("view.next_artboard")
        );
        assert_eq!(
            catalog_command_for_key("page_up", true, false, false, true),
            Some("view.prev_artboard")
        );
        assert_eq!(
            catalog_command_for_key("pagedown", true, false, true, true),
            Some("view.next_artboard")
        );
        assert_eq!(
            catalog_windows_key("object.group").as_deref(),
            Some("Ctrl+G")
        );
        assert_eq!(
            catalog_windows_key("object.ungroup").as_deref(),
            Some("Ctrl+Shift+G")
        );
        assert_eq!(
            catalog_command_for_key("g", true, false, false, true),
            Some("object.group")
        );
        assert_eq!(
            catalog_command_for_key("g", true, true, false, true),
            Some("object.ungroup")
        );
        assert_eq!(
            catalog_command_for_key("quote", true, false, false, true),
            Some("view.toggle_grid")
        );
        assert_eq!(catalog_windows_key("tool.line").as_deref(), Some("\\"));
        assert_eq!(
            catalog_command_for_key("n", true, false, false, true),
            Some("file.new")
        );
        assert_eq!(
            catalog_command_for_key("o", true, false, false, true),
            Some("file.open")
        );
        assert_eq!(
            catalog_command_for_key("e", true, false, false, true),
            Some("file.export_dialog")
        );
        assert_eq!(
            catalog_command_for_key("s", true, false, false, true),
            Some("file.save")
        );
        assert_eq!(
            catalog_command_for_key("Z", true, false, false, true),
            Some("edit.undo")
        );
        assert_eq!(
            catalog_command_for_key("z", true, true, false, true),
            Some("edit.redo")
        );
        assert_eq!(
            catalog_command_for_key("c", true, false, false, true),
            Some("edit.copy")
        );
        assert_eq!(
            catalog_command_for_key("x", true, false, false, true),
            Some("edit.cut")
        );
        assert_eq!(
            catalog_command_for_key("v", true, false, false, true),
            Some("edit.paste")
        );
        assert_eq!(
            catalog_command_for_key("f", true, false, false, true),
            Some("edit.paste_in_place")
        );
        assert_eq!(
            catalog_command_for_key("k", true, false, false, true),
            Some("app.command_palette")
        );
        assert_eq!(catalog_command_for_key("s", true, true, false, true), None);
        assert_eq!(catalog_command_for_key("v", false, true, false, true), None);
        assert_eq!(catalog_command_for_key("m", false, false, true, true), None);
        assert_eq!(
            catalog_command_for_key("Delete", false, false, false, false),
            Some("object.delete")
        );
        assert_eq!(
            catalog_command_for_key("Delete", false, true, false, true),
            None
        );
    }

    #[test]
    fn rectangle_drag_normalizes_reverse_direction_and_rejects_tiny_input() {
        assert_eq!(
            shape_from_points([50.0, 80.0], [10.0, 20.0]),
            Some([10.0, 20.0, 40.0, 60.0])
        );
        assert_eq!(shape_from_points([10.0, 20.0], [11.0, 80.0]), None);
        assert_eq!(shape_from_points([10.0, 20.0], [50.0, 21.0]), None);
        assert_eq!(shape_from_points([f64::NAN, 0.0], [50.0, 50.0]), None);
    }

    #[test]
    fn screen_to_document_accounts_for_panel_origin_and_pan() {
        let global_position = [670.0, 225.0];
        let panel_origin = [520.0, 100.0];
        let local_position = [
            global_position[0] - panel_origin[0],
            global_position[1] - panel_origin[1],
        ];
        let document = screen_to_document(
            local_position,
            [300.0, 250.0],
            1.0,
            [10.0, -5.0],
            [100.0, 50.0],
        )
        .expect("valid viewport maps to document coordinates");

        assert_eq!(document, [40.0, 30.0]);
        assert!(screen_to_document([0.0, 0.0], [0.0, 0.0], 1.0, [0.0; 2], [100.0; 2]).is_none());
    }

    #[test]
    fn hit_test_prefers_topmost_visible_stable_node() {
        let draw_list = list(vec![
            item("under", [0.0, 0.0, 100.0, 100.0], 1.0, false),
            item("top", [20.0, 20.0, 80.0, 80.0], 1.0, false),
            item("invisible", [20.0, 20.0, 80.0, 80.0], 0.0, false),
            item("", [0.0, 0.0, 300.0, 200.0], 1.0, false),
        ]);

        assert_eq!(
            hit_test(&draw_list, [40.0, 40.0]).map(|hit| hit.sid.as_str()),
            Some("top")
        );
        assert_eq!(
            hit_test(&draw_list, [10.0, 10.0]).map(|hit| hit.sid.as_str()),
            Some("under")
        );
    }

    #[test]
    fn ellipse_hit_test_uses_ellipse_geometry() {
        let draw_list = list(vec![item("ellipse", [0.0, 0.0, 100.0, 100.0], 1.0, true)]);

        assert!(hit_test(&draw_list, [50.0, 50.0]).is_some());
        assert!(hit_test(&draw_list, [98.0, 98.0]).is_none());
    }
}
