//! **vb_session 纯度门禁**(22 篇 §3.1「零 UI 依赖」+ §7.1 依赖方向;G-UI7 同族)。
//!
//! vb_session 是换宿主的渐进载体,它必须**连 UI 库都不认识**:
//! 1. 源码(含注释与文档)不得出现任何 UI 栈的路径表达 —— 机制照搬
//!    `vb_kit/tests/shell_purity.rs` 的静态防线(字面量拆写防本文件自伤);
//! 2. 依赖白名单:vb_* 仅 `vb_common` / `vb_doc` / `vb_tools`(吸附引擎
//!    的依赖注记见 `src/snap.rs` 模块文档),外加 serde 系;任何新增依赖
//!    必须先过本测试这道"申报关"。

use std::fs;
use std::path::{Path, PathBuf};

/// 递归收集 `dir` 下全部 `.rs` 文件(稳定排序,失败信息可定位)。
fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries =
            fs::read_dir(&d).unwrap_or_else(|e| panic!("读目录失败 {}: {e}", d.display()));
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR")))
}

/// 纯度一:源码零 UI 栈痕迹(路径表达,大小写不敏感;连注释都不许)。
#[test]
fn no_ui_stack_paths_in_sources() {
    // 被禁 token = 两侧 UI 栈与新宿主控件库的**路径表达**(带冒号即视为
    // 代码/类型引用;字面量拆写防本测试文件自身误伤,tests/ 不在扫描范围)。
    let forbidden = ["egui:", "gpui:", "sable:"];
    let mut violations = Vec::new();
    for file in rs_files(&manifest_dir().join("src")) {
        let content =
            fs::read_to_string(&file).unwrap_or_else(|e| panic!("读 {} 失败:{e}", file.display()));
        let lower = content.to_lowercase();
        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        for token in forbidden {
            if lower.contains(token) {
                let line = lower.find(token).map(|i| content[..i].lines().count() + 1);
                violations.push(format!(
                    "{file_name}:{}: 含 UI 栈痕迹 {token:?}(vb_session 零 UI 依赖,22 篇 §3.1)",
                    line.unwrap_or(0)
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "vb_session 触碰 UI 依赖红线(它必须连 UI 库都不认识):\n{}",
        violations.join("\n")
    );
}

/// 纯度二:依赖白名单(vb_* 三件套 + serde 系;dev-dependencies 只许 serde_json)。
#[test]
fn dependencies_stay_in_allowlist() {
    let toml = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("读 Cargo.toml 失败");
    let mut section = "";
    let mut vb_deps = Vec::new();
    let mut other_deps = Vec::new();
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed;
            continue;
        }
        // 只看依赖段内的条目行 `name = …`
        if !(section == "[dependencies]" || section == "[dev-dependencies]") {
            continue;
        }
        let Some((name, _)) = trimmed.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if section == "[dependencies]" {
            if let Some(stripped) = name.strip_prefix("vb_") {
                vb_deps.push(format!("vb_{stripped}"));
            } else if name != "serde" {
                other_deps.push(name.to_string());
            }
        } else if name != "serde_json" {
            other_deps.push(format!("dev:{name}"));
        }
    }
    vb_deps.sort();
    other_deps.sort();
    assert_eq!(
        vb_deps,
        ["vb_common", "vb_doc", "vb_tools"],
        "vb_* 依赖偏离白名单(改动须经本测试申报;vb_doc/vb_tools 注记见 src/snap.rs)"
    );
    assert_eq!(
        other_deps,
        ["fluent", "serde_json"],
        "出现白名单外依赖(只许 serde / fluent[R0 i18n 再导出 FluentValue]/ serde_json[mru recent.json 运行时依赖,申报见 Cargo.toml]/dev serde_json):{other_deps:?}"
    );
}
