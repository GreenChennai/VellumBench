//! i18n 命令目录完整性门禁(R0 i18n 地基,设计 22 §4 第 7 条;G-UI3 前置)。
//!
//! 断言 `crates/vb_app/src/shortcuts/catalog.rs::CMD_LABELS` 的**每一条**
//! 命令在 `i18n/zh.ftl` 与 `i18n/en.ftl` 都有对应 `cmd-*` key(解析正则
//! 语义:`^cmd-[a-z0-9-]+\s*=`,同一分隔符两边都要有),缺一即红并打印
//! 缺失清单。另断言:
//! ① zh 值与 CMD_LABELS **逐字一致**(术语门禁与 UI 现状都不许漂;
//!    漂移的合法路径是改 catalog.rs 后重跑 `tools/gen_cmd_ftl.py`);
//! ② 05-7 批次的既有 14 个 key(prefs.* / bp.* / state.*)两边都还在
//!    (向后兼容,不许删);
//! ③ 两份 ftl 的 `cmd-*` key 集合恰好相等、无孤儿、值非空。
//!
//! en 翻译的词汇纪律(CONTEXT.md 术语表/禁用表)由 `tools/check_terminology.py`
//! 覆盖;本测试只管结构完整性。

use std::collections::{BTreeMap, BTreeSet};
use vb_app::shortcuts::CMD_LABELS;

/// 05-7 批次既有 key(i18n 骨架;不许改名不许删)。
const LEGACY_KEYS: &[&str] = &[
    "prefs.ui-language",
    "prefs.ui-language-help",
    "bp.switcher",
    "bp.default",
    "bp.edit-banner",
    "bp.edit-note",
    "bp.unsupported",
    "bp.doc-settings",
    "bp.doc-settings-help",
    "bp.status-hint",
    "state.label",
    "state.normal",
    "state.hover",
    "state.hover-note",
];

/// 命令 id → ftl key:点与下划线都转连字符(与 tools/gen_cmd_ftl.py 同口径)。
fn cmd_key(cmd_id: &str) -> String {
    let suffix: String = cmd_id
        .chars()
        .map(|c| match c {
            '.' | '_' => '-',
            other => other,
        })
        .collect();
    format!("cmd-{suffix}")
}

/// 解析 ftl:全部 `key = value` 平键行(key = 首个 `=` 前去尾空白;
/// 跳过空行与 `#` 注释;key 非空、值非空)。cmd-* 子集另行做
/// `^cmd-[a-z0-9-]+\s*=` 字符集校验(见 parse 内断言)。
fn parse_ftl(path: &str) -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path} 可读:{e}"));
    let mut out = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(eq) = line.find('=') else {
            panic!("{path}:{}:非注释行缺少 '=':{line}", i + 1);
        };
        let key = line[..eq].trim_end();
        let value = line[eq + 1..].trim();
        assert!(!key.is_empty(), "{path}:{}:空 key:{line}", i + 1);
        assert!(!value.is_empty(), "{path}:{}:{key} 值为空", i + 1);
        if let Some(body) = key.strip_prefix("cmd-") {
            // 与门禁正则 ^cmd-[a-z0-9-]+\s*= 同义
            assert!(
                body.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    && body.ends_with(|c: char| c.is_ascii_alphanumeric()),
                "{path}:{}:key 不符合 ^cmd-[a-z0-9-]+\\s*=:{line}",
                i + 1
            );
        }
        assert!(
            out.insert(key.to_string(), value.to_string()).is_none(),
            "{path}:{}:重复 key {key}",
            i + 1
        );
    }
    out
}

#[test]
fn i18n_catalog_complete() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let zh = parse_ftl(&format!("{manifest}/../../i18n/zh.ftl"));
    let en = parse_ftl(&format!("{manifest}/../../i18n/en.ftl"));
    let cmd_keys = |m: &BTreeMap<String, String>| {
        m.keys()
            .filter(|k| k.starts_with("cmd-"))
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    let zh_keys = cmd_keys(&zh);
    let en_keys = cmd_keys(&en);
    assert_eq!(zh_keys, en_keys, "zh/en 的 cmd-* key 集合漂移");

    // ①+完整性:CMD_LABELS 每条都必须在两份 ftl 里,且 zh 值逐字一致
    let mut missing: Vec<String> = Vec::new();
    let mut drifted: Vec<String> = Vec::new();
    for (id, label) in CMD_LABELS {
        let key = cmd_key(id);
        match zh.get(&key) {
            None => missing.push(format!("{key}(zh 缺)")),
            Some(v) if v != label => drifted.push(format!("{key}:ftl={v:?} catalog={label:?}")),
            _ => {}
        }
        if !en.contains_key(&key) {
            missing.push(format!("{key}(en 缺)"));
        }
    }
    assert!(
        missing.is_empty(),
        "i18n 命令目录缺 {} 条(补法:更新 tools/gen_cmd_ftl.py 后重跑):\n{}",
        missing.len(),
        missing.join("\n")
    );
    assert!(
        drifted.is_empty(),
        "zh.ftl 值与 CMD_LABELS 漂移 {} 条(合法路径:改 catalog.rs 后重跑 tools/gen_cmd_ftl.py):\n{}",
        drifted.len(),
        drifted.join("\n")
    );

    // 反向:ftl 里不允许有 CMD_LABELS 对不上的孤儿 cmd key(改 id 后忘再生成)
    let expected: BTreeSet<String> = CMD_LABELS.iter().map(|(id, _)| cmd_key(id)).collect();
    let orphans: Vec<String> = zh_keys.difference(&expected).cloned().collect();
    assert!(
        orphans.is_empty(),
        "ftl 存在孤儿 cmd key(命令已删/改名,需重跑 tools/gen_cmd_ftl.py):{orphans:?}"
    );

    // ② 既有 14 key 保留:两份 ftl 都还在,且有值(空值 = 缺失)
    for key in LEGACY_KEYS {
        assert!(zh.contains_key(*key), "既有 key 被删:{key}(zh)");
        assert!(en.contains_key(*key), "既有 key 被删:{key}(en)");
    }
    assert_eq!(zh_keys.len(), 204, "cmd-* 数量漂移(204 条命令目录)");
}
