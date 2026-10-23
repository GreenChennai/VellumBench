//! VellumBench command-palette entries projected from the root catalog.
//!
//! The catalog owns IDs and shortcuts; this module only filters to commands that
//! the new host currently dispatches and resolves localized labels/aliases.

use sable::widgets::prelude::CommandEntry;
use vb_session::i18n::{t, t_args, FluentValue};

const COMMANDS_YAML: &str = include_str!("../../../commands.yaml");

/// Catalog commands currently supported by `ProjectSession` or `CanvasPanel`.
const SUPPORTED_COMMANDS: &[&str] = &[
    "file.new",
    "file.open",
    "file.export_dialog",
    "file.save",
    "file.place_image",
    "view.toggle_grid",
    "view.next_artboard",
    "view.prev_artboard",
    "canvas.nudge_left",
    "canvas.nudge_right",
    "canvas.nudge_up",
    "canvas.nudge_down",
    "edit.undo",
    "edit.redo",
    "edit.select_all",
    "edit.copy",
    "edit.cut",
    "edit.paste",
    "edit.paste_in_place",
    "object.delete",
    "object.group",
    "object.ungroup",
    "object.lock",
    "object.unlock_all",
    "object.hide",
    "object.show_all",
    "align.to_selection",
    "align.to_key_object",
    "align.to_artboard",
    "align.left",
    "align.hcenter",
    "align.right",
    "align.top",
    "align.vcenter",
    "align.bottom",
    "anim.create",
    "anim.play_toggle",
    "anim.stop",
    "anim.loop_toggle",
    "anim.clear",
    "anim.keyframe_add",
    "anim.keyframe_delete",
    "anim.keyframe_nudge_earlier",
    "anim.keyframe_nudge_later",
    "tool.select",
    "tool.rect",
    "tool.ellipse",
    "tool.artboard",
    "tool.text",
    "tool.hand",
];

/// Build a searchable Sable palette from catalog entries supported by the new host.
///
/// Errors are returned instead of panicking so a malformed catalog can be surfaced
/// in the project status bar. Unsupported legacy commands are deliberately omitted.
pub fn entries() -> Result<Vec<CommandEntry>, String> {
    let catalog: serde_yaml::Value = serde_yaml::from_str(COMMANDS_YAML).map_err(|error| {
        t_args(
            "ui-command-palette-parse-failed",
            &[("error", FluentValue::from(error.to_string()))],
        )
    })?;
    let records = catalog
        .as_sequence()
        .ok_or_else(|| t("ui-command-palette-not-list"))?;

    SUPPORTED_COMMANDS
        .iter()
        .map(|command_id| {
            let record = records
                .iter()
                .find(|record| {
                    record.get("id").and_then(serde_yaml::Value::as_str) == Some(command_id)
                })
                .ok_or_else(|| {
                    t_args(
                        "ui-command-palette-missing-command",
                        &[("id", FluentValue::from((*command_id).to_string()))],
                    )
                })?;
            let fallback_label = record
                .get("label")
                .and_then(serde_yaml::Value::as_str)
                .ok_or_else(|| {
                    t_args(
                        "ui-command-palette-missing-label",
                        &[("id", FluentValue::from((*command_id).to_string()))],
                    )
                })?;
            let label_key = match *command_id {
                "anim.create" => "ui-animation-create-preset".to_string(),
                "anim.keyframe_nudge_earlier" => "ui-animation-keyframe-nudge-earlier".to_string(),
                "anim.keyframe_nudge_later" => "ui-animation-keyframe-nudge-later".to_string(),
                _ => format!("cmd-{}", command_id.replace(['.', '_'], "-")),
            };
            let localized_label = t(&label_key);
            let label = if localized_label == label_key || localized_label.trim().is_empty() {
                fallback_label.to_string()
            } else {
                localized_label
            };
            let raw_shortcut = record
                .get("keys")
                .and_then(|keys| keys.get("win"))
                .and_then(serde_yaml::Value::as_str)
                .unwrap_or_default();
            let aliases = aliases_for(command_id)
                .iter()
                .map(|alias| (*alias).into())
                .collect();
            Ok(CommandEntry {
                id: (*command_id).into(),
                label: label.into(),
                keystroke: normalize_keystroke(raw_shortcut).into(),
                category: String::new().into(),
                aliases,
            })
        })
        .collect()
}

fn normalize_keystroke(raw: &str) -> String {
    let mut control = false;
    let mut shift = false;
    let mut alt = false;
    let mut key = None;
    for part in raw
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => control = true,
            "shift" => shift = true,
            "alt" | "option" => alt = true,
            other => {
                key = Some(
                    other
                        .replace("pagedown", "page_down")
                        .replace("pageup", "page_up"),
                )
            }
        }
    }
    let mut parts = Vec::with_capacity(4);
    if shift {
        parts.push("shift".to_string());
    }
    if control {
        parts.push("ctrl".to_string());
    }
    if alt {
        parts.push("alt".to_string());
    }
    if let Some(key) = key {
        parts.push(key);
    }
    parts.join("-")
}

fn aliases_for(command_id: &str) -> &'static [&'static str] {
    match command_id {
        "file.new" => &["new project", "new", "xinxiangmu", "xinjianxiangmu"],
        "file.open" => &["open project", "open", "dakai", "dakaixiangmu"],
        "file.export_dialog" => &["export", "export svg", "export png", "daochu", "dc"],
        "file.save" => &["save", "baocun", "bc"],
        "file.place_image" => &["place image", "image", "tupian", "tp"],
        "view.toggle_grid" => &["grid", "toggle grid", "wangge", "wg"],
        "view.next_artboard" => &["next artboard", "next page", "xiayi huaban"],
        "view.prev_artboard" => &["previous artboard", "previous page", "shangyi huaban"],
        "canvas.nudge_left" => &["nudge left", "move left", "zuoyi", "zy"],
        "canvas.nudge_right" => &["nudge right", "move right", "youyi", "yy"],
        "canvas.nudge_up" => &["nudge up", "move up", "shangyi", "sy"],
        "canvas.nudge_down" => &["nudge down", "move down", "xiayi", "xy"],
        "edit.undo" => &["undo", "chexiao", "cx"],
        "edit.redo" => &["redo", "chongzuo", "cz"],
        "edit.select_all" => &["select all", "quanxuan", "qx"],
        "edit.copy" => &["copy", "fuzhi", "fz"],
        "edit.cut" => &["cut", "jianqie", "jq"],
        "edit.paste" | "edit.paste_in_place" => &["paste", "zhantie", "zt"],
        "object.delete" => &["delete", "shanchu", "sc"],
        "object.group" => &["group", "zuhe", "zh"],
        "object.ungroup" => &["ungroup", "quxiaozu", "qxz"],
        "object.lock" | "object.unlock_all" => &["lock", "suoding", "sd"],
        "object.hide" | "object.show_all" => &["visibility", "yincang", "yc"],
        "align.left" => &["align left", "left", "zuo"],
        "align.hcenter" => &["horizontal center", "hcenter", "shuipingju zhong"],
        "align.right" => &["align right", "right", "you"],
        "align.top" => &["align top", "top", "ding"],
        "align.vcenter" => &["vertical center", "vcenter", "chuizhiju zhong"],
        "align.bottom" => &["align bottom", "bottom", "di"],
        "align.to_selection" => &["align target selection", "duiqi xuanqu"],
        "align.to_key_object" => &["align target key object", "duiqi guanjian duixiang"],
        "align.to_artboard" => &["align target artboard", "duiqi huaban"],
        "anim.create" => &["create animation", "motion preset", "chuangjian donghua"],
        "anim.play_toggle" => &["play animation", "pause animation", "donghuayulan"],
        "anim.stop" => &["stop animation", "rewind animation", "tingzhi donghua"],
        "anim.loop_toggle" => &["loop animation", "repeat animation", "xunhuan donghua"],
        "anim.clear" => &["clear animation", "remove animation", "qingchu donghua"],
        "anim.keyframe_add" => &["add keyframe", "keyframe at playhead", "tianjia guanjian"],
        "anim.keyframe_delete" => &["delete keyframe", "remove keyframe", "shanchu guanjian"],
        "anim.keyframe_nudge_earlier" => &[
            "move keyframes earlier",
            "shift keyframes left",
            "guanjian qianyi",
        ],
        "anim.keyframe_nudge_later" => &[
            "move keyframes later",
            "shift keyframes right",
            "guanjian houyi",
        ],
        "tool.select" => &["select", "xuanze", "xz"],
        "tool.rect" => &["rectangle", "juxing", "jx"],
        "tool.ellipse" => &["ellipse", "tuoyuan", "ty"],
        "tool.artboard" => &["artboard", "huaban", "hb"],
        "tool.text" => &["text", "wenzi", "wz"],
        "tool.hand" => &["hand", "zhua shou", "zs"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::{entries, normalize_keystroke, COMMANDS_YAML, SUPPORTED_COMMANDS};

    #[test]
    fn palette_entries_match_supported_catalog_ids_and_localized_labels() {
        let parsed: serde_yaml::Value = serde_yaml::from_str(COMMANDS_YAML).expect("valid catalog");
        let records = parsed.as_sequence().expect("catalog is a list");
        let mut command_ids = std::collections::HashSet::new();
        for record in records {
            let id = record
                .get("id")
                .and_then(serde_yaml::Value::as_str)
                .expect("every catalog entry has an ID");
            assert!(command_ids.insert(id), "duplicate command ID: {id}");
        }
        let entries = entries().expect("all supported entries exist");
        assert_eq!(entries.len(), SUPPORTED_COMMANDS.len());
        for id in SUPPORTED_COMMANDS {
            let entry = entries.iter().find(|entry| entry.id.as_ref() == *id);
            assert!(entry.is_some(), "missing palette entry {id}");
            let record = parsed
                .as_sequence()
                .and_then(|records| {
                    records.iter().find(|record| {
                        record.get("id").and_then(serde_yaml::Value::as_str) == Some(id)
                    })
                })
                .expect("catalog ID exists");
            assert!(record
                .get("label")
                .and_then(serde_yaml::Value::as_str)
                .is_some());
        }
        let new_project = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "file.new")
            .expect("new-project command is supported");
        assert!(new_project
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "new project"));
        let open_project = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "file.open")
            .expect("open-project command is supported");
        assert!(open_project
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "open project"));
        let export = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "file.export_dialog")
            .expect("export command is supported");
        assert!(export
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "export"));
        let grid = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "view.toggle_grid")
            .expect("grid command is supported");
        assert!(grid.aliases.iter().any(|alias| alias.as_ref() == "grid"));
        assert!(grid.aliases.iter().any(|alias| alias.as_ref() == "wangge"));
        let place_image = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "file.place_image")
            .expect("place-image command is supported");
        assert!(place_image
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "place image"));
        let group = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "object.group")
            .expect("group command is supported");
        assert!(group.aliases.iter().any(|alias| alias.as_ref() == "group"));
        let ungroup = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "object.ungroup")
            .expect("ungroup command is supported");
        assert!(ungroup
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "ungroup"));
        let select_all = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "edit.select_all")
            .expect("select-all command is supported");
        assert!(select_all
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "select all"));
        assert!(select_all
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "quanxuan"));
        let add_keyframe = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "anim.keyframe_add")
            .expect("keyframe-add command is supported");
        assert!(add_keyframe
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "add keyframe"));
        let delete_keyframe = entries
            .iter()
            .find(|entry| entry.id.as_ref() == "anim.keyframe_delete")
            .expect("keyframe-delete command is supported");
        assert!(delete_keyframe
            .aliases
            .iter()
            .any(|alias| alias.as_ref() == "delete keyframe"));

        let catalog: serde_yaml::Value =
            serde_yaml::from_str(COMMANDS_YAML).expect("valid command catalog");
        let records = catalog.as_sequence().expect("catalog list");
        for (command_id, alias, label_key) in [
            (
                "anim.keyframe_nudge_earlier",
                "move keyframes earlier",
                "ui-animation-keyframe-nudge-earlier",
            ),
            (
                "anim.keyframe_nudge_later",
                "move keyframes later",
                "ui-animation-keyframe-nudge-later",
            ),
        ] {
            assert_eq!(
                records
                    .iter()
                    .filter(|record| {
                        record.get("id").and_then(serde_yaml::Value::as_str) == Some(command_id)
                    })
                    .count(),
                1,
                "each nudge command must have exactly one YAML record"
            );
            let entry = entries
                .iter()
                .find(|entry| entry.id.as_ref() == command_id)
                .expect("nudge command is supported");
            assert!(entry
                .aliases
                .iter()
                .any(|candidate| candidate.as_ref() == alias));
            let expected = vb_session::i18n::t(label_key);
            assert_eq!(entry.label.as_ref(), expected.as_str());
        }
    }

    #[test]
    fn animation_create_uses_host_ui_translation_key() {
        let entry = entries()
            .expect("supported commands")
            .into_iter()
            .find(|entry| entry.id.as_ref() == "anim.create")
            .expect("animation creation command");
        let expected = vb_session::i18n::t("ui-animation-create-preset");
        assert_eq!(entry.label.as_ref(), expected.as_str());
    }

    #[test]
    fn catalog_shortcuts_normalize_to_sable_keystroke_syntax() {
        assert_eq!(normalize_keystroke("Ctrl+K"), "ctrl-k");
        assert_eq!(normalize_keystroke("Ctrl+Shift+Z"), "shift-ctrl-z");
        assert_eq!(normalize_keystroke("Ctrl+PageDown"), "ctrl-page_down");
        assert_eq!(normalize_keystroke(""), "");
    }
}
