//! commands.yaml 同步门禁(15 号计划 A5/D1):
//! ① 从注册表生成 commands.yaml(维护命令:临时打开 GEN 开关跑
//!    `cargo test -p vb_app --test commands_yaml -- --nocapture gen`,
//!    输出重定向到 commands.yaml);
//! ② 永久校验:yaml 的 id/label/键位与注册表逐一相等,漂移即红。
//!
//! yaml 的 undoable / expose_to_agent 是人工策展列(注册表无此数据),
//! 不在本测试的机械校验范围内。

use vb_app::shortcuts::{key_text_for, CMD_LABELS, IMPLEMENTED_IDS};

fn parse_yaml(path: &str) -> Vec<(String, String, String)> {
    let text = std::fs::read_to_string(path).expect("commands.yaml 可读");
    let mut out = Vec::new();
    let mut cur: Option<(String, String, String)> = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(id) = line.strip_prefix("- id: ") {
            if let Some(c) = cur.take() {
                out.push(c);
            }
            cur = Some((id.to_string(), String::new(), String::new()));
        } else if let Some(rest) = line.strip_prefix("label: ") {
            if let Some(c) = cur.as_mut() {
                c.1 = rest.to_string();
            }
        } else if let Some(rest) = line.strip_prefix("keys: { win: ") {
            if let Some(c) = cur.as_mut() {
                c.2 = rest.trim_end_matches(" }").trim_matches('"').to_string();
            }
        }
    }
    if let Some(c) = cur.take() {
        out.push(c);
    }
    out
}

#[test]
fn commands_yaml_matches_registry() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let yaml_path = format!("{}/../../commands.yaml", manifest);
    let entries = parse_yaml(&yaml_path);

    // ① id 集合 == IMPLEMENTED_IDS 集合(注册表有 3 条无害重复,先去重)
    let mut reg_ids: Vec<&str> = IMPLEMENTED_IDS.to_vec();
    reg_ids.sort();
    reg_ids.dedup();
    let yaml_ids: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
    let mut yaml_sorted = yaml_ids.clone();
    yaml_sorted.sort();
    assert_eq!(
        yaml_sorted, reg_ids,
        "commands.yaml 与注册表命令集漂移(13 篇纪律:同 commit 更新 yaml)"
    );

    // ② 顺序与 label 与 CMD_LABELS 一致
    for (i, (id, label, _)) in entries.iter().enumerate() {
        let (reg_id, reg_label) = CMD_LABELS[i];
        assert_eq!(id, reg_id, "第 {i} 条顺序漂移");
        assert_eq!(label, reg_label, "命令 {id} 的 label 漂移");
    }

    // ③ 键位与 SHORTCUTS 一致
    for (id, _, key) in &entries {
        let expect = key_text_for(id).unwrap_or_default();
        assert_eq!(key, &expect, "命令 {id} 的键位漂移");
    }
}

/// 生成器:Cargo.toml 里临时开启(见文件头注释)后运行,把 stdout
/// 重定向覆盖 commands.yaml。
#[test]
fn gen() {
    if std::env::var("VB_GEN_COMMANDS_YAML").unwrap_or_default() != "1" {
        return;
    }
    // 旧 yaml 的策展列(36 条,按出现顺序);新命令默认:
    // undoable = 文档变更类;expose_to_agent = patch 已有等价 op。
    let mut out = String::from(
        "# Vellum Bench 命令表\n\
         # 单一真相:菜单 / 快捷键 / 面板 / Agent CLI 共用。\n\
         # 本文件由注册表(cmds.yaml 门禁测试)校验同步;ID/label/键位以\n\
         # vb_app::shortcuts 为准,undoable / expose_to_agent 为人工策展列。\n\
         # expose_to_agent: 可通过 `vellum-cli patch {\"op\":\"run\",\"command\":...}` 执行\n",
    );
    for (id, label) in CMD_LABELS {
        let key = key_text_for(id).unwrap_or_default();
        let undoable = is_undoable(id);
        let agent = is_agent_exposed(id);
        out.push_str(&format!(
            "- id: {id}\n  label: {label}\n  keys: {{ win: \"{key}\" }}\n  undoable: {undoable}\n  expose_to_agent: {agent}\n"
        ));
    }
    eprintln!("===YAML_BEGIN===");
    eprintln!("{out}");
    eprintln!("===YAML_END===");
}

fn is_undoable(id: &str) -> bool {
    id.starts_with("object.")
        || id.starts_with("align.")
        || id.starts_with("canvas.nudge_")
        || id == "canvas.pen_finish"
        || id == "edit.cut"
        || id == "edit.paste"
        || id == "edit.paste_in_place"
}

fn is_agent_exposed(id: &str) -> bool {
    id.starts_with("object.")
        || id.starts_with("align.")
        || id.starts_with("file.")
        || id.starts_with("canvas.nudge_")
        || id == "edit.undo"
        || id == "edit.redo"
        || id == "edit.select_all"
}
