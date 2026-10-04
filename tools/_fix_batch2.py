"""一次性:抽取后编译错误的手动修复批(mechanical,按类)。"""
import json
import pathlib
import re

ROOT = pathlib.Path(".")
SIDE = json.loads(pathlib.Path("tools/ui_i18n_map.json").read_text(encoding="utf-8"))
INV = {v: k for k, v in SIDE["keys"].items()}  # key -> zh(解码后)

SUBS = {
    "crates/vb_app/src/shell.rs": [
        # fixer 重复插 & 的坍缩(regex 形式,处理器特判)
    ],
    "crates/vb_app/src/app/dialogs.rs": [
        ("let Some(which) = self.esc_dialog_top() else {\n            return;\n        };\n        match which {",
         "let Some(which) = self.esc_dialog_top() else {\n            return;\n        };\n        match which.as_str() {"),
        ("Some(vb_session::i18n::t(\"ui-app-dialogs-014\")),",
         "Some(vb_session::i18n::t(\"ui-app-dialogs-014\").as_str()),"),
        ("Some(vb_session::i18n::t(\"ui-common-cancel\"))\n        ));",
         "Some(vb_session::i18n::t(\"ui-common-cancel\").as_str())\n        ));"),
    ],
    "crates/vb_app/src/app/history.rs": [
        ("if vb_ui::components::icon_button(ui, icon, tip).clicked() {",
         "if vb_ui::components::icon_button(ui, icon, &tip).clicked() {"),
    ],
    "crates/vb_app/src/app/panels/layers/render.rs": [
        ("\"vblock\", lock_icon, lock_color, lock_tip,", "\"vblock\", lock_icon, lock_color, &lock_tip,"),
        ("\"vbeye\", eye_icon, eye_color, eye_tip,", "\"vbeye\", eye_icon, eye_color, &eye_tip,"),
        ("if icon_button(ui, icon, tip).clicked() {", "if icon_button(ui, icon, &tip).clicked() {"),
    ],
    "crates/vb_app/src/app/panels/properties.rs": [
        ("if icon_button(ui, icon, tip).clicked() {", "if icon_button(ui, icon, &tip).clicked() {"),
    ],
    "crates/vb_app/src/app/panel_dock.rs": [
        ("                },\n                tip,\n            )\n            .clicked()",
         "                },\n                &tip,\n            )\n            .clicked()"),
    ],
    "crates/vb_app/src/launcher.rs": [
        ("ui.label(vb_ui::components::strong(title));", "ui.label(vb_ui::components::strong(&title));"),
        ("ui.label(vb_ui::components::caption(ui, desc));", "ui.label(vb_ui::components::caption(ui, &desc));"),
    ],
    "crates/vb_app/src/app/control_panel/editors.rs": [
        ("        let before = sel;\n", "        let before = sel.clone();\n"),
    ],
    "crates/vb_app/src/app/conflict_dialog.rs": [
        ("pub(crate) struct SideSummary {\n    pub label: &'static str,",
         "pub(crate) struct SideSummary {\n    pub label: String,"),
        ("label: &vb_session::i18n::t(", "label: vb_session::i18n::t("),
    ],
    "crates/vb_app/src/canvas_shot.rs": [
        ("    layers: &'static str,", "    layers: String,"),
        ("layers: &vb_session::i18n::t(", "layers: vb_session::i18n::t("),
    ],
    "crates/vb_app/src/app/timeline.rs": [
        ("""                if self.anim_playing {
                    &vb_session::i18n::t("ui-common-pause")
                } else {
                    &vb_session::i18n::t("ui-common-play")
                },""",
         """                {
                    let lbl = if self.anim_playing {
                        vb_session::i18n::t("ui-common-pause")
                    } else {
                        vb_session::i18n::t("ui-common-play")
                    };
                    &lbl
                },"""),
    ],
    "crates/vb_app/src/app/color_panel.rs": [
        ("""                if stroke {
                    &vb_session::i18n::t("ui-app-color-panel-010")
                } else {
                    &vb_session::i18n::t("ui-app-color-panel-011")
                },""",
         """                {
                    let lbl = if stroke {
                        vb_session::i18n::t("ui-app-color-panel-010")
                    } else {
                        vb_session::i18n::t("ui-app-color-panel-011")
                    };
                    &lbl
                },"""),
        ("let cf = ColorField::new(if stroke { &vb_session::i18n::t(\"ui-app-color-panel-012\") } else { &vb_session::i18n::t(\"ui-app-color-panel-013\") }, &mut col)",
         "let cf = {\n            let lbl = if stroke {\n                vb_session::i18n::t(\"ui-app-color-panel-012\")\n            } else {\n                vb_session::i18n::t(\"ui-app-color-panel-013\")\n            };\n            ColorField::new(&lbl, &mut col)\n        };"),
    ],
    "crates/vb_app/src/app/keymap_dialog.rs": [
        ("    pub fn group_names() -> Vec<&'static str> {",
         "    pub fn group_names() -> Vec<String> {"),
        ("        let mut v = vec![\"全部\"];",
         "        let mut v: Vec<String> = vec![crate::i18n::t(\"ui-common-all\")];"),
        ("        v.extend(crate::shortcuts::MENU_TITLES.iter().copied());",
         "        v.extend(crate::shortcuts::MENU_TITLES.iter().map(|s| s.to_string()));"),
        ("        v.push(&vb_session::i18n::t(\"ui-common-other\"));",
         "        v.push(vb_session::i18n::t(\"ui-common-other\"));"),
    ],
    "crates/vb_app/src/shortcuts.rs": [
        ("pub fn menu_group_of(id: &str) -> &'static str {",
         "pub fn menu_group_of(id: &str) -> String {"),
        ("            return MENU_TITLES[i];",
         "            return crate::i18n::t(&format!(\"ui-menu-title-{i}\"));"),
        ("    &vb_session::i18n::t(\"ui-common-other\")\n}",
         "    crate::i18n::t(\"ui-common-other\")\n}"),
    ],
}

for path, pairs in SUBS.items():
    p = pathlib.Path(path)
    s = p.read_text(encoding="utf-8")
    for old, new in pairs:
        if isinstance(old, re.Pattern):
            s, n = old.subn(new, s)
            print(f"{path}: regex x{n}")
            continue
        n = s.count(old)
        if n == 0:
            print(f"MISS {path}: {old[:60]!r}")
            continue
        s = s.replace(old, new)
        print(f"{path}: x{n} {old[:44]!r}")
    p.write_text(s, encoding="utf-8", newline="")
print("done")
