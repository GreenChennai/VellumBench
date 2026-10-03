#!/usr/bin/env python3
"""命令目录 FTL 生成器(R0 i18n 地基,设计 22 §4 第 7 条)。

从 `crates/vb_app/src/shortcuts/catalog.rs` 的 CMD_LABELS 机械解析全部命令
(id + 中文标签),生成 `i18n/zh.ftl` 与 `i18n/en.ftl` 的 `cmd-*` 目录段:

- key 规范:`cmd-<命令 id,点转连字符>`(如 `object.group` → `cmd-object-group`);
- zh 值与 CMD_LABELS **逐字一致**(机械抽取,禁止手抄/润色——术语门禁与
  UI 现状都不许漂);
- en 值来自本文件内置译表(EN),遵守 CONTEXT.md 术语表与禁用表:
  画板=Artboard、编组=Group、对象=Object、面板坞=Tab Dock、主页=Launcher、
  能力台账=Capability Ledger、组件语境用 Symbol(禁用 Frame/Component);
- 生成段以 BEGIN/END 标记包裹,幂等(重复运行整段替换,不重不漏)。

用法:`python tools/gen_cmd_ftl.py`(catalog.rs 变更后重跑即可再生成;
`vb_app/tests/i18n_catalog_complete.rs` 是产物完整性门禁)。
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CATALOG = ROOT / "crates/vb_app/src/shortcuts/catalog.rs"
FILES = {"zh": ROOT / "i18n/zh.ftl", "en": ROOT / "i18n/en.ftl"}
BEGIN = "# ── BEGIN cmd-catalog(由 tools/gen_cmd_ftl.py 生成;勿手改)──"
END = "# ── END cmd-catalog ──"

# 英文译表(人工策展,唯一手写部分;新增命令必须在此补一行,脚本漏译即红)。
# 词汇以 CONTEXT.md 术语表为唯一真相;禁用词 Frame / Component / smart object。
EN: dict[str, str] = {
    "file.new": "New Project… (dialog, opens in a new window)",
    "file.open": "Open Project…",
    "file.save": "Save",
    "file.export_dialog": "Export…",
    "file.export_repeat": "Repeat Export (current artboard PNG @2x)",
    "app.quit": "Quit",
    "edit.undo": "Undo",
    "edit.redo": "Redo",
    "edit.select_all": "Select All (current artboard)",
    "edit.copy": "Copy",
    "edit.cut": "Cut",
    "edit.paste": "Paste",
    "edit.paste_in_place": "Paste in Place (in front)",
    "object.group": "Group",
    "object.ungroup": "Ungroup",
    "object.transform_again": "Transform Again",
    "object.bring_forward": "Bring Forward",
    "object.bring_to_front": "Bring to Front",
    "object.send_backward": "Send Backward",
    "object.send_to_back": "Send to Back",
    "object.delete": "Delete Object",
    "object.lock": "Lock Selection",
    "object.unlock_all": "Unlock All",
    "object.hide": "Hide Selection",
    "object.show_all": "Show All",
    "align.left": "Horizontal Align Left",
    "align.hcenter": "Horizontal Align Center",
    "align.right": "Horizontal Align Right",
    "align.top": "Vertical Align Top",
    "align.vcenter": "Vertical Align Center",
    "align.bottom": "Vertical Align Bottom",
    "path.union": "Pathfinder: Union",
    "path.subtract": "Pathfinder: Minus Front",
    "path.intersect": "Pathfinder: Intersect",
    "path.xor": "Pathfinder: Exclude",
    "object.distribute_h": "Distribute Horizontally",
    "object.distribute_v": "Distribute Vertically",
    "view.zoom_in": "Zoom In",
    "view.zoom_out": "Zoom Out",
    "view.fit": "Fit in Window",
    "view.actual_size": "Actual Size 100%",
    "view.outline": "Outline Mode (wireframe)",
    "view.toggle_grid": "Show / Hide Grid",
    "view.toggle_smart_guides": "Toggle Smart Guides",
    "view.toggle_theme": "Dark / Light Theme",
    "tool.select": "Selection Tool",
    "tool.rect": "Rectangle Tool",
    "tool.ellipse": "Ellipse Tool",
    "tool.line": "Line Tool",
    "tool.pen": "Pen Tool",
    "tool.direct_select": "Direct Selection Tool",
    "tool.zoom": "Zoom Tool",
    "tool.hand": "Hand Tool",
    "tool.text": "Type Tool",
    "tool.eyedropper": "Eyedropper Tool",
    "tool.artboard": "Artboard Tool",
    "tool.gradient": "Gradient Tool",
    "tool.scissors": "Scissors Tool",
    "tool.group_select": "Group Selection Tool",
    "tool.rotate": "Rotate Tool (click to set center, drag to rotate; Shift = 15°)",
    "tool.mirror": "Mirror Tool (click to set center, drag to set the mirror axis)",
    "tool.scale": "Scale Tool (click to set center, drag to scale; Shift = proportional)",
    "tool.free_transform": "Free Transform Tool (drag the selection corners, anchored diagonally)",
    "tool.pencil": "Pencil Tool (freehand drawing, thinned to a path by fidelity)",
    "tool.curvature": "Curvature Tool (click a vector path to auto-fit smooth control points)",
    "edit.pencil_fidelity": "Settings → Pencil Fidelity (cycles 1–16px)",
    "tool.slice": "Slice Tool (Shift+K drag to create data-vb-slice slices)",
    "object.slice_from_selection": "Slice → From Selection",
    "file.place_image": "Place Image… (pick a file into assets/, sets src on selection or creates an img)",
    "object.replace_image": "Replace Image… (keeps geometry; SetImageSrc is undoable)",
    "view.pixel_preview": "Pixel Preview (snaps to the physical pixel grid at zoom ≥8×)",
    "tool.measure": "Measure Tool (drag to measure distance, click to annotate object size)",
    "canvas.cancel": "Cancel / Clear Selection",
    "canvas.pen_finish": "Pen: Finish Path",
    "canvas.nudge_left": "Nudge Left 1px",
    "canvas.nudge_right": "Nudge Right 1px",
    "canvas.nudge_up": "Nudge Up 1px",
    "canvas.nudge_down": "Nudge Down 1px",
    "view.toggle_rulers": "Show / Hide Rulers",
    "view.toggle_guides": "Show / Hide Guides",
    "view.lock_guides": "Lock Guides",
    "view.guides_from_selection": "Make Guides from Selection",
    "app.command_palette": "Command Palette",
    "view.next_artboard": "Next Artboard",
    "view.prev_artboard": "Previous Artboard",
    "view.next_panel_tab": "Cycle Right Panel Tab",
    "view.zoom_to_selection": "Zoom to Selection",
    "app.about": "About",
    "view.toggle_layers_panel": "Toggle Layers Panel",
    "view.toggle_all_panels": "Hide / Restore All Panels",
    "view.toggle_char_panel": "Toggle Character Panel",
    "view.toggle_para_panel": "Toggle Paragraph Panel",
    "tool.text_cycle_mode": "Type Tool: Cycle Point / Area",
    "view.toggle_appearance_panel": "Toggle Appearance Panel",
    "view.toggle_stroke_panel": "Toggle Stroke Panel",
    "view.toggle_gradient_panel": "Toggle Gradient Panel",
    "view.toggle_opacity_panel": "Toggle Opacity Panel",
    "view.toggle_color_panel": "Toggle Color Panel",
    "color.toggle_target": "Color: Toggle Fill/Stroke",
    "color.swap_fill_stroke": "Color: Swap Fill and Stroke",
    "color.default_fill_stroke": "Color: Reset Default Fill/Stroke",
    "file.close": "Close Window (document)",
    "file.import_html": "Import HTML…",
    "file.doc_settings": "Document Settings… (project name / output mode / grid & guides, applied on OK)",
    "file.resolve_conflict": "Compare and Merge… (three-way compare of external changes: disk/memory/autosave)",
    "file.print": "Print… (current artboard → temp PDF → opened by the system)",
    "edit.preferences": "Preferences… (nine categories; changes apply immediately)",
    "edit.keyboard_shortcuts": "Keyboard Shortcuts… (keymap editor, schemes stored in keymap.json)",
    "object.clip_mask": "Make Clip Mask",
    "object.release_clip_mask": "Release Clip Mask",
    "object.outline_stroke": "Outline Stroke",
    "text.upper_case": "Change Case → UPPERCASE",
    "text.lower_case": "Change Case → lowercase",
    "text.create_outlines": "Create Outlines",
    "text.find_font": "Find Font… (detect and replace missing fonts, undoable)",
    "select.inverse": "Inverse Selection",
    "select.next_object": "Select Next Object Above",
    "select.prev_object": "Select Next Object Below",
    "select.same_fill": "Select Same Fill Color",
    "select.same_stroke": "Select Same Stroke Color",
    "select.same_stroke_width": "Select Same Stroke Weight",
    "select.all_text": "Select All Text Objects",
    "select.all_locked": "Select All Locked Objects",
    "select.all_hidden": "Select All Hidden Objects",
    "effect.repeat_last": "Apply Last Effect",
    "effect.drop_shadow": "Effect: Drop Shadow",
    "effect.inner_shadow": "Effect: Inner Shadow",
    "effect.outer_glow": "Effect: Outer Glow",
    "effect.inner_glow": "Effect: Inner Glow",
    "effect.round_corners": "Effect: Round Corners",
    "effect.gaussian_blur": "Effect: Gaussian Blur",
    "effect.feather": "Effect: Feather",
    "effect.distort": "Distort and Transform…",
    "view.hide_edges": "Hide Edges",
    "view.browser_proof": "Browser Proof…",
    "window.workspace_basic": "Workspace: Essentials",
    "window.workspace_type": "Workspace: Typography",
    "window.workspace_export": "Workspace: Export",
    "window.new_workspace": "New Workspace… (save the current layout as a named preset, switchable/deletable)",
    "window.tab_properties": "Tab Dock: Properties",
    "window.tab_layers": "Tab Dock: Layers",
    "window.tab_artboards": "Tab Dock: Artboards",
    "window.tab_tokens": "Tab Dock: Tokens",
    "help.shortcuts": "Shortcut Cheat Sheet…",
    "help.check_update": "Check for Updates…",
    "help.capabilities": "Capability Ledger (what is done / not done)",
    "view.dock_toolbar_top": "Toolbar: Dock to Top",
    "view.dock_toolbar_left": "Toolbar: Dock to Left",
    "view.dock_toolbar_right": "Toolbar: Dock to Right",
    "view.dock_toolbar_bottom": "Toolbar: Dock to Bottom",
    "view.toolbar_columns_1": "Toolbar: Single Column",
    "view.toolbar_columns_2": "Toolbar: Double Column",
    "path.merge": "Pathfinder: Merge",
    "path.subtract_back": "Pathfinder: Minus Back",
    "path.crop": "Pathfinder: Crop",
    "path.divide": "Pathfinder: Divide",
    "path.trim": "Pathfinder: Trim",
    "path.outline": "Pathfinder: Outline",
    "view.toggle_transform_panel": "Toggle Transform Panel",
    "view.toggle_align_panel": "Toggle Align Panel",
    "align.to_selection": "Align To: Selection",
    "align.to_key_object": "Align To: Key Object",
    "align.to_artboard": "Align To: Artboard",
    "object.distribute_hspace": "Distribute Horizontal Spacing",
    "object.distribute_vspace": "Distribute Vertical Spacing",
    "file.home": "Launcher… (open the launcher window)",
    "home.new_project": "Launcher: New Project",
    "home.open_project": "Launcher: Open Project…",
    "home.new_from_template": "Launcher: New from Template",
    "home.open_selected": "Launcher: Open Selected Recent Project",
    "home.remove_selected": "Launcher: Remove Selected Recent Item",
    "home.select_next": "Launcher: Select Next",
    "home.select_prev": "Launcher: Select Previous",
    "home.pin_selected": "Launcher: Pin / Unpin Selected",
    "home.search": "Launcher: Search Recent Projects",
    "home.restore_session": "Launcher: Restore Last Session",
    "home.capabilities": "Launcher: Capability Ledger",
    "view.developer_stats": "Dev Stats (debug data, hidden by default)",
    "view.toggle_hints": "Hint Bar (operation tips / getting started, dismissible)",
    "view.toggle_motion": "UI Motion (fade / transitions, dismissible; same switch in Preferences → General)",
    "view.ui_scale_up": "UI Scale: Step Up",
    "view.ui_scale_down": "UI Scale: Step Down",
    "view.ui_scale_reset": "UI Scale: Reset to 100%",
    "edit.toggle_unsupported_tools": "Tools → Show Unsupported Tools",
    "edit.autosave_interval": "Settings → Autosave Interval (cycles off/30/60/120/300)",
    "view.toggle_history_panel": "Toggle History Panel (undo history is jumpable)",
    "file.health_check": "Project Health Check…",
    "view.toggle_assets_panel": "Toggle Assets Panel (assets/ inventory + references + locate/replace)",
    "view.breakpoint_cycle": "Breakpoint Preview: Cycle (default → each breakpoint)",
    "style.state_toggle": "Panel State: Normal / Hover",
    "object.symbol_create": "Create Symbol (promote the selection to the master; its position becomes the first instance)",
    "object.symbol_detach": "Detach Instance (instance becomes a plain element, no longer synced with the master)",
    "object.symbol_reset_overrides": "Reset Overrides (instance restored to the master's current content)",
    "object.symbol_swap_main": "Swap Master Definition (instance content becomes the new definition and syncs the other instances)",
    "object.symbol_select_instances": "Select All Instances (same master)",
    "view.toggle_timeline_panel": "Timeline Panel (keyframe tracks + play preview)",
    "anim.play_toggle": "Animation Preview: Play / Pause",
    "anim.stop": "Animation Preview: Stop and Rewind",
    "anim.loop_toggle": "Animation Preview: Loop On / Off",
    "anim.keyframe_add": "Add Keyframe at Playhead (selected objects; values from static values)",
    "anim.keyframe_delete": "Delete Selected Keyframes",
    "anim.clear": "Clear Object Animation (removes @keyframes and animation)",
    "edit.plugins": "Plugin Manager… (install/authorize/enable/logs/restart; plugins are external processes, zero-permission by default)",
    "view.toggle_plugins_panel": "Toggle Plugins Panel (panels registered by Running plugins, controlled UI)",
}


def parse_catalog() -> list[tuple[str, str]]:
    """机械解析 CMD_LABELS:支持单行与多行元组,值原样保留(逐字一致)。"""
    src = CATALOG.read_text(encoding="utf-8")
    m = re.search(r"pub const CMD_LABELS: &\[\(&str, &str\)\] = &\[(.*?)\n\];", src, re.S)
    if not m:
        sys.exit("FAIL: catalog.rs 里找不到 CMD_LABELS")
    entries = re.findall(r'\(\s*"([^"]+)"\s*,\s*"([^"]*)"\s*,?\s*\)', m.group(1), re.S)
    if not entries:
        sys.exit("FAIL: CMD_LABELS 解析为空(正则与源码漂移?)")
    return entries


def cmd_key(cmd_id: str) -> str:
    """key 规范:cmd-<id 的点与下划线都转连字符>(门禁正则
    `^cmd-[a-z0-9-]+\\s*=` 只认小写字母/数字/连字符)。"""
    return "cmd-" + re.sub(r"[._]", "-", cmd_id)


def validate(entries: list[tuple[str, str]]) -> None:
    ids = [i for i, _ in entries]
    dupes = sorted({i for i in ids if ids.count(i) > 1})
    if dupes:
        sys.exit(f"FAIL: CMD_LABELS 重复 id:{dupes}")
    missing = sorted(set(ids) - set(EN))
    extra = sorted(set(EN) - set(ids))
    if missing:
        sys.exit(f"FAIL: 英文译表缺 {len(missing)} 条:{missing}")
    if extra:
        sys.exit(f"FAIL: 英文译表多出(命令已删?):{extra}")
    for cid, label in entries:
        if not label.strip():
            sys.exit(f"FAIL: {cid} 中文标签为空")
        for tag, val in (("zh", label), ("en", EN[cid])):
            if any(c in val for c in "{}"):
                sys.exit(f"FAIL: {cid} {tag} 值含 FTL 花括号,需转义")
            if val.lstrip()[:1] in "#*[.-":
                sys.exit(f"FAIL: {cid} {tag} 值以 FTL 特殊字符开头:{val[:1]!r}")
    keys = [cmd_key(i) for i in ids]
    collisions = sorted({k for k in keys if keys.count(k) > 1})
    if collisions:
        sys.exit(f"FAIL: key 规范化后撞车:{collisions}")


def render_block(entries: list[tuple[str, str]], lang: str) -> str:
    lines = [BEGIN]
    for cid, label in entries:
        value = label if lang == "zh" else EN[cid]
        lines.append(f"{cmd_key(cid)} = {value}")
    lines.append(END)
    return "\n".join(lines)


def upsert(path: pathlib.Path, block: str) -> int:
    """把生成段写入 ftl(有标记则整段替换,无标记则追加);返回 cmd key 数。"""
    text = path.read_text(encoding="utf-8")
    pat = re.compile(re.escape(BEGIN) + r"\n.*?\n" + re.escape(END), re.S)
    if pat.search(text):
        text = pat.sub(lambda _: block, text)
    else:
        text = text.rstrip("\n") + "\n\n" + block + "\n"
    path.write_text(text, encoding="utf-8", newline="\n")
    return len(re.findall(r"^cmd-[a-z0-9-]+\s*=", block, re.M))


def main() -> None:
    entries = parse_catalog()
    validate(entries)
    for lang, path in FILES.items():
        n = upsert(path, render_block(entries, lang))
        print(f"{path.name}: {n} cmd keys(zh/en 各 {len(entries)} 条)")
    print(f"OK: {len(entries)} 条命令目录已生成")


if __name__ == "__main__":
    main()
