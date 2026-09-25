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
