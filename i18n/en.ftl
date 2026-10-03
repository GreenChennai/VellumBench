# Vellum Bench UI copy (English)
# 05-7 i18n skeleton: key set must exactly match zh.ftl (enforced by the
# vb_app::i18n gate test). Plain `key = value` lines; `#` comments.
# Coverage (honest scope): only the dialogs / menus / panels added in this
# batch (breakpoint switcher, breakpoint editing, pseudo-class state,
# preferences language row, doc-settings breakpoint row).

# ── Preferences · General ──
prefs.ui-language = UI language
prefs.ui-language-help = Skeleton scope: only dialogs and menus added in this batch follow this setting; the rest of the UI stays Chinese for now

# ── Breakpoints (responsive) ──
bp.switcher = Breakpoint
bp.default = Default
bp.edit-banner = Breakpoint overrides
bp.edit-note = At this breakpoint only width/height/position/visibility/font-size are editable; switch back to Default for everything else (exported as @media blocks)
bp.unsupported = Not editable at this breakpoint
bp.doc-settings = Breakpoints (px, comma-separated; empty = none)
bp.doc-settings-help = Saved into the vb-breakpoints meta in index.html; switch preview width from the status bar
bp.status-hint = Breakpoint preview: a band marks the breakpoint width on the canvas; contents still render with base styles — verify overrides via View → Browser Proof

# ── Pseudo-class state (minimal loop) ──
state.label = State
state.normal = Normal
state.hover = Hover
state.hover-note = Hover minimal loop: fill/text color/opacity/font-size/visibility land in a selector:hover rule; the canvas does not simulate hover, use View → Browser Proof

# ── New host vb_kit / vb_shell (R0 gap batch; G-UI3: no bare copy in render paths) ──
# Hand-maintained section outside the gen_cmd_ftl.py BEGIN/END block (rerunning
# the generator leaves it alone). Key scheme: ui-<panel>-<semantic>, hyphenated.
# English follows the CONTEXT.md glossary (Capability Ledger; glossary-banned words avoided).

# Capability Ledger panel (filter chips)
ui-cap-filter-all = All
ui-cap-filter-done = Done
ui-cap-filter-partial = Partial
ui-cap-filter-dropped = Dropped
ui-cap-filter-agent-only = Agent-reproducible only
# Capability Ledger panel (counts line / empty state / footer / Agent column)
ui-cap-counts = { $total } total · { $done } done · { $partial } partial · { $dropped } dropped
ui-cap-empty = No matching entries (current filter combination)
ui-cap-footer = Data source: vb_session::capabilities (single source of truth) · three states: Done / Partial + whereabouts / Dropped
ui-cap-agent-repro = reproducible ×{ $n }
# Capability Ledger panel (three-state badges; Planned is gate-guaranteed empty, kept as fallback)
ui-cap-badge-done = Done
ui-cap-badge-partial = Partial
ui-cap-badge-planned = Planned
ui-cap-badge-dropped = Dropped
# Shell (vb_shell)
ui-shell-window-title = VellumBench · sable Preview Host
ui-shell-panel-capabilities = Capability Ledger
ui-shell-panel-canvas = Canvas (R1)
ui-shell-canvas-placeholder = Canvas (R1) — takes over once the canvas presentation channel is ruled by ADR-0047
ui-shell-canvas-subtitle = R0 preview host: window / theme / dock layout skeleton

# ── BEGIN cmd-catalog(由 tools/gen_cmd_ftl.py 生成;勿手改)──
cmd-file-new = New Project… (dialog, opens in a new window)
cmd-file-open = Open Project…
cmd-file-save = Save
cmd-file-export-dialog = Export…
cmd-file-export-repeat = Repeat Export (current artboard PNG @2x)
cmd-app-quit = Quit
cmd-edit-undo = Undo
cmd-edit-redo = Redo
cmd-edit-select-all = Select All (current artboard)
cmd-edit-copy = Copy
cmd-edit-cut = Cut
cmd-edit-paste = Paste
cmd-edit-paste-in-place = Paste in Place (in front)
cmd-object-group = Group
cmd-object-ungroup = Ungroup
cmd-object-transform-again = Transform Again
cmd-object-bring-forward = Bring Forward
cmd-object-bring-to-front = Bring to Front
cmd-object-send-backward = Send Backward
cmd-object-send-to-back = Send to Back
cmd-object-delete = Delete Object
cmd-object-lock = Lock Selection
cmd-object-unlock-all = Unlock All
cmd-object-hide = Hide Selection
cmd-object-show-all = Show All
cmd-align-left = Horizontal Align Left
cmd-align-hcenter = Horizontal Align Center
cmd-align-right = Horizontal Align Right
cmd-align-top = Vertical Align Top
cmd-align-vcenter = Vertical Align Center
cmd-align-bottom = Vertical Align Bottom
cmd-path-union = Pathfinder: Union
cmd-path-subtract = Pathfinder: Minus Front
cmd-path-intersect = Pathfinder: Intersect
cmd-path-xor = Pathfinder: Exclude
cmd-object-distribute-h = Distribute Horizontally
cmd-object-distribute-v = Distribute Vertically
cmd-view-zoom-in = Zoom In
cmd-view-zoom-out = Zoom Out
cmd-view-fit = Fit in Window
cmd-view-actual-size = Actual Size 100%
cmd-view-outline = Outline Mode (wireframe)
cmd-view-toggle-grid = Show / Hide Grid
cmd-view-toggle-smart-guides = Toggle Smart Guides
cmd-view-toggle-theme = Dark / Light Theme
cmd-tool-select = Selection Tool
cmd-tool-rect = Rectangle Tool
cmd-tool-ellipse = Ellipse Tool
cmd-tool-line = Line Tool
cmd-tool-pen = Pen Tool
cmd-tool-direct-select = Direct Selection Tool
cmd-tool-zoom = Zoom Tool
cmd-tool-hand = Hand Tool
cmd-tool-text = Type Tool
cmd-tool-eyedropper = Eyedropper Tool
cmd-tool-artboard = Artboard Tool
cmd-tool-gradient = Gradient Tool
cmd-tool-scissors = Scissors Tool
cmd-tool-group-select = Group Selection Tool
cmd-tool-rotate = Rotate Tool (click to set center, drag to rotate; Shift = 15°)
cmd-tool-mirror = Mirror Tool (click to set center, drag to set the mirror axis)
cmd-tool-scale = Scale Tool (click to set center, drag to scale; Shift = proportional)
cmd-tool-free-transform = Free Transform Tool (drag the selection corners, anchored diagonally)
cmd-tool-pencil = Pencil Tool (freehand drawing, thinned to a path by fidelity)
cmd-tool-curvature = Curvature Tool (click a vector path to auto-fit smooth control points)
cmd-edit-pencil-fidelity = Settings → Pencil Fidelity (cycles 1–16px)
cmd-tool-slice = Slice Tool (Shift+K drag to create data-vb-slice slices)
cmd-object-slice-from-selection = Slice → From Selection
cmd-file-place-image = Place Image… (pick a file into assets/, sets src on selection or creates an img)
cmd-object-replace-image = Replace Image… (keeps geometry; SetImageSrc is undoable)
cmd-view-pixel-preview = Pixel Preview (snaps to the physical pixel grid at zoom ≥8×)
cmd-tool-measure = Measure Tool (drag to measure distance, click to annotate object size)
cmd-canvas-cancel = Cancel / Clear Selection
cmd-canvas-pen-finish = Pen: Finish Path
cmd-canvas-nudge-left = Nudge Left 1px
cmd-canvas-nudge-right = Nudge Right 1px
cmd-canvas-nudge-up = Nudge Up 1px
cmd-canvas-nudge-down = Nudge Down 1px
cmd-view-toggle-rulers = Show / Hide Rulers
cmd-view-toggle-guides = Show / Hide Guides
cmd-view-lock-guides = Lock Guides
cmd-view-guides-from-selection = Make Guides from Selection
cmd-app-command-palette = Command Palette
cmd-view-next-artboard = Next Artboard
cmd-view-prev-artboard = Previous Artboard
cmd-view-next-panel-tab = Cycle Right Panel Tab
cmd-view-zoom-to-selection = Zoom to Selection
cmd-app-about = About
cmd-view-toggle-layers-panel = Toggle Layers Panel
cmd-view-toggle-all-panels = Hide / Restore All Panels
cmd-view-toggle-char-panel = Toggle Character Panel
cmd-view-toggle-para-panel = Toggle Paragraph Panel
cmd-tool-text-cycle-mode = Type Tool: Cycle Point / Area
cmd-view-toggle-appearance-panel = Toggle Appearance Panel
cmd-view-toggle-stroke-panel = Toggle Stroke Panel
cmd-view-toggle-gradient-panel = Toggle Gradient Panel
cmd-view-toggle-opacity-panel = Toggle Opacity Panel
cmd-view-toggle-color-panel = Toggle Color Panel
cmd-color-toggle-target = Color: Toggle Fill/Stroke
cmd-color-swap-fill-stroke = Color: Swap Fill and Stroke
cmd-color-default-fill-stroke = Color: Reset Default Fill/Stroke
cmd-file-close = Close Window (document)
cmd-file-import-html = Import HTML…
cmd-file-doc-settings = Document Settings… (project name / output mode / grid & guides, applied on OK)
cmd-file-resolve-conflict = Compare and Merge… (three-way compare of external changes: disk/memory/autosave)
cmd-file-print = Print… (current artboard → temp PDF → opened by the system)
cmd-edit-preferences = Preferences… (nine categories; changes apply immediately)
cmd-edit-keyboard-shortcuts = Keyboard Shortcuts… (keymap editor, schemes stored in keymap.json)
cmd-object-clip-mask = Make Clip Mask
cmd-object-release-clip-mask = Release Clip Mask
cmd-object-outline-stroke = Outline Stroke
cmd-text-upper-case = Change Case → UPPERCASE
cmd-text-lower-case = Change Case → lowercase
cmd-text-create-outlines = Create Outlines
cmd-text-find-font = Find Font… (detect and replace missing fonts, undoable)
cmd-select-inverse = Inverse Selection
cmd-select-next-object = Select Next Object Above
cmd-select-prev-object = Select Next Object Below
cmd-select-same-fill = Select Same Fill Color
cmd-select-same-stroke = Select Same Stroke Color
cmd-select-same-stroke-width = Select Same Stroke Weight
cmd-select-all-text = Select All Text Objects
cmd-select-all-locked = Select All Locked Objects
cmd-select-all-hidden = Select All Hidden Objects
cmd-effect-repeat-last = Apply Last Effect
cmd-effect-drop-shadow = Effect: Drop Shadow
cmd-effect-inner-shadow = Effect: Inner Shadow
cmd-effect-outer-glow = Effect: Outer Glow
cmd-effect-inner-glow = Effect: Inner Glow
cmd-effect-round-corners = Effect: Round Corners
cmd-effect-gaussian-blur = Effect: Gaussian Blur
cmd-effect-feather = Effect: Feather
cmd-effect-distort = Distort and Transform…
cmd-view-hide-edges = Hide Edges
cmd-view-browser-proof = Browser Proof…
cmd-window-workspace-basic = Workspace: Essentials
cmd-window-workspace-type = Workspace: Typography
cmd-window-workspace-export = Workspace: Export
cmd-window-new-workspace = New Workspace… (save the current layout as a named preset, switchable/deletable)
cmd-window-tab-properties = Tab Dock: Properties
cmd-window-tab-layers = Tab Dock: Layers
cmd-window-tab-artboards = Tab Dock: Artboards
cmd-window-tab-tokens = Tab Dock: Tokens
cmd-help-shortcuts = Shortcut Cheat Sheet…
cmd-help-check-update = Check for Updates…
cmd-help-capabilities = Capability Ledger (what is done / not done)
cmd-view-dock-toolbar-top = Toolbar: Dock to Top
cmd-view-dock-toolbar-left = Toolbar: Dock to Left
cmd-view-dock-toolbar-right = Toolbar: Dock to Right
cmd-view-dock-toolbar-bottom = Toolbar: Dock to Bottom
cmd-view-toolbar-columns-1 = Toolbar: Single Column
cmd-view-toolbar-columns-2 = Toolbar: Double Column
cmd-path-merge = Pathfinder: Merge
cmd-path-subtract-back = Pathfinder: Minus Back
cmd-path-crop = Pathfinder: Crop
cmd-path-divide = Pathfinder: Divide
cmd-path-trim = Pathfinder: Trim
cmd-path-outline = Pathfinder: Outline
cmd-view-toggle-transform-panel = Toggle Transform Panel
cmd-view-toggle-align-panel = Toggle Align Panel
cmd-align-to-selection = Align To: Selection
cmd-align-to-key-object = Align To: Key Object
cmd-align-to-artboard = Align To: Artboard
cmd-object-distribute-hspace = Distribute Horizontal Spacing
cmd-object-distribute-vspace = Distribute Vertical Spacing
cmd-file-home = Launcher… (open the launcher window)
cmd-home-new-project = Launcher: New Project
cmd-home-open-project = Launcher: Open Project…
cmd-home-new-from-template = Launcher: New from Template
cmd-home-open-selected = Launcher: Open Selected Recent Project
cmd-home-remove-selected = Launcher: Remove Selected Recent Item
cmd-home-select-next = Launcher: Select Next
cmd-home-select-prev = Launcher: Select Previous
cmd-home-pin-selected = Launcher: Pin / Unpin Selected
cmd-home-search = Launcher: Search Recent Projects
cmd-home-restore-session = Launcher: Restore Last Session
cmd-home-capabilities = Launcher: Capability Ledger
cmd-view-developer-stats = Dev Stats (debug data, hidden by default)
cmd-view-toggle-hints = Hint Bar (operation tips / getting started, dismissible)
cmd-view-toggle-motion = UI Motion (fade / transitions, dismissible; same switch in Preferences → General)
cmd-view-ui-scale-up = UI Scale: Step Up
cmd-view-ui-scale-down = UI Scale: Step Down
cmd-view-ui-scale-reset = UI Scale: Reset to 100%
cmd-edit-toggle-unsupported-tools = Tools → Show Unsupported Tools
cmd-edit-autosave-interval = Settings → Autosave Interval (cycles off/30/60/120/300)
cmd-view-toggle-history-panel = Toggle History Panel (undo history is jumpable)
cmd-file-health-check = Project Health Check…
cmd-view-toggle-assets-panel = Toggle Assets Panel (assets/ inventory + references + locate/replace)
cmd-view-breakpoint-cycle = Breakpoint Preview: Cycle (default → each breakpoint)
cmd-style-state-toggle = Panel State: Normal / Hover
cmd-object-symbol-create = Create Symbol (promote the selection to the master; its position becomes the first instance)
cmd-object-symbol-detach = Detach Instance (instance becomes a plain element, no longer synced with the master)
cmd-object-symbol-reset-overrides = Reset Overrides (instance restored to the master's current content)
cmd-object-symbol-swap-main = Swap Master Definition (instance content becomes the new definition and syncs the other instances)
cmd-object-symbol-select-instances = Select All Instances (same master)
cmd-view-toggle-timeline-panel = Timeline Panel (keyframe tracks + play preview)
cmd-anim-play-toggle = Animation Preview: Play / Pause
cmd-anim-stop = Animation Preview: Stop and Rewind
cmd-anim-loop-toggle = Animation Preview: Loop On / Off
cmd-anim-keyframe-add = Add Keyframe at Playhead (selected objects; values from static values)
cmd-anim-keyframe-delete = Delete Selected Keyframes
cmd-anim-clear = Clear Object Animation (removes @keyframes and animation)
cmd-edit-plugins = Plugin Manager… (install/authorize/enable/logs/restart; plugins are external processes, zero-permission by default)
cmd-view-toggle-plugins-panel = Toggle Plugins Panel (panels registered by Running plugins, controlled UI)
# ── END cmd-catalog ──

# ── vb_shell(R0 launcher/CLI;manual section;single-line messages — gate parser is single-line)──
ui-shell-usage = vellum-sable — VellumBench sable preview host (R0) | Usage: vellum-sable opens the launcher · --project <dir> goes straight to a project window (ADR-0033) · <project-dir> same as --project · --help | --version
ui-shell-project-arg-required = vellum-sable: --project requires a project directory argument
ui-shell-arg-unknown = vellum-sable: unknown argument {$arg} (see --help)
ui-shell-project-dir-missing = vellum-sable: project directory does not exist
ui-shell-launcher-title = VellumBench · Launcher
ui-shell-launcher-search-placeholder = Search name or path…
ui-shell-launcher-invalid-path = Path no longer exists
ui-shell-launcher-empty = No projects yet
ui-shell-launcher-empty-hint = Open your first project with vellum-sable --project <dir>; it will appear in recents
ui-shell-launcher-no-match = No projects matching "{$query}"
ui-shell-launcher-footer = ↑↓ select · Enter open · R remove · click row to open
