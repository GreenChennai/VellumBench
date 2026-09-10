//! 语料库往返门禁(设计文档 10 篇门禁 3)。
//!
//! 遍历 `tests/corpus/*.html`,逐文件跑 L0(不损坏)+ L1(幂等)。
//! **新增语料只需丢一个 HTML 文件进来,无需改代码。**
//!
//! 可选 sidecar:`同名.expect`,每行一个必须出现在导出结果里的子串
//! (空行与 `#` 开头的行忽略)。
//!
//! 规模目标:P1 ≥ 20 个 → P5 = 100 个。

mod common;

use std::path::{Path, PathBuf};

use common::check_l0_l1;

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn cases() -> Vec<PathBuf> {
    let dir = corpus_dir();
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("读不到语料目录 {}:{e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "html").unwrap_or(false))
        .collect();
    v.sort();
    v
}

fn expects_for(case: &Path) -> Vec<String> {
    match std::fs::read_to_string(case.with_extension("expect")) {
        Ok(s) => s
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(str::to_string)
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// P1 目标 20 个;P5 目标 100 个。
const MIN_CASES: usize = 20;

#[test]
fn corpus_roundtrip() {
    let cases = cases();
    assert!(
        cases.len() >= MIN_CASES,
        "往返语料不足 {MIN_CASES} 个(当前 {});10 篇门禁 3 要求 P5 达 100 个",
        cases.len()
    );

    let mut failures = Vec::new();
    for case in &cases {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let src = std::fs::read_to_string(case).unwrap();
        let expects = expects_for(case);
        if let Err(e) = check_l0_l1(&name, &src, &expects) {
            failures.push(e);
        }
    }
    assert!(
        failures.is_empty(),
        "{} / {} 个语料未通过:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// 每个语料都必须自带 `data-vb-id`,否则 L0 的 sid 断言形同虚设。
#[test]
fn corpus_has_stable_ids() {
    let mut bad = Vec::new();
    for case in cases() {
        let src = std::fs::read_to_string(&case).unwrap();
        if common::vb_ids(&src).is_empty() {
            bad.push(case.file_name().unwrap().to_string_lossy().to_string());
        }
    }
    assert!(bad.is_empty(), "以下语料缺 data-vb-id:{bad:?}");
}
