//! **G-UI7 壳纯度 + G-UI2 hex 纪律**(vb_kit 雏形;22 篇 §8)。
//!
//! G-UI7 全量(菜单项→命令 ID 解析、动画求值单源等)随面板批次铺开;
//! 本文件先锁两条机器可查的底线:
//! 1. 源码不得出现文档模型/窑炉两个实体 crate 的名字与旧 egui 栈的路径
//!    表达 —— 依赖方向硬规则(22 篇 §3.1)的静态防线,连注释引用都不许
//!    (防"先抄进来再接线");
//! 2. hex 颜色字面量只允许出现在 `src/tokens.rs`(组件一律经主题/令牌取色)。

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

fn src_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"))).join("src")
}

/// G-UI7(雏形):vb_kit 源码不出现被禁依赖的痕迹。
#[test]
fn no_document_model_kiln_or_egui_in_sources() {
    // 被禁 token:文档模型 / 窑炉两实体 crate 与旧 egui 栈的路径表达。
    // (字面量拆写防本测试文件自身误伤;tests/ 不在扫描范围,纯双保险。)
    let forbidden: [(&str, &str); 3] = [("vb_do", "c"), ("vb_kil", "n"), ("egui:", ":")];
    let mut violations = Vec::new();
    for file in rs_files(&src_dir()) {
        let content =
            fs::read_to_string(&file).unwrap_or_else(|e| panic!("读 {} 失败:{e}", file.display()));
        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        for (head, tail) in forbidden {
            if content.contains(&format!("{head}{tail}")) {
                violations.push(format!("{file_name}: 含被禁依赖痕迹 {head}{tail}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "vb_kit 触碰依赖红线(22 篇 §3.1:文档语义只经 vb_session 投影):\n{}",
        violations.join("\n")
    );
}

/// G-UI2(雏形):hex 颜色字面量只许出现在 `src/tokens.rs`。
#[test]
fn hex_color_literals_only_in_tokens() {
    let src = src_dir();
    let whitelist = ["tokens.rs"];
    let mut violations = Vec::new();
    for file in rs_files(&src) {
        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        if whitelist.contains(&file_name) {
            continue;
        }
        let content =
            fs::read_to_string(&file).unwrap_or_else(|e| panic!("读 {} 失败:{e}", file.display()));
        // 正则 `#[0-9a-fA-F]{6}` 的手写等价扫描('#' 后恰 6 位 hex)
        let bytes = content.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            if b != b'#' {
                continue;
            }
            let hex_chars = content[i + 1..].chars().take(6).collect::<Vec<_>>();
            if hex_chars.len() == 6 && hex_chars.iter().all(|c| c.is_ascii_hexdigit()) {
                let line = content[..i].lines().count() + 1;
                violations.push(format!("{file_name}:{line}: hex 颜色字面量出 tokens.rs"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "G-UI2 hex 纪律被破坏(取色必须走主题/令牌):\n{}",
        violations.join("\n")
    );
}

/// 硬骨头 #13 完成定义(22 篇 §3.7):vb_kit 源码零 OS 直引——
/// `windows::` / `winit::` 一个都不许出现。窗口/剪贴板/对话框/光标/DPI/
/// 深色探测一律经 `vb_platform` 六 trait 注入,宿主实现在 vb_app/vb_shell。
#[test]
fn no_os_direct_imports_in_sources() {
    // 被禁 token:Windows API crate 与 winit(egui/gpui 底层窗口系统)的
    // 路径表达。tests/ 不在扫描范围(与既有 purity 口径一致)。
    let forbidden: [&str; 2] = ["windows::", "winit::"];
    let mut violations = Vec::new();
    for file in rs_files(&src_dir()) {
        let content =
            fs::read_to_string(&file).unwrap_or_else(|e| panic!("读 {} 失败:{e}", file.display()));
        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        for token in forbidden {
            if content.contains(token) {
                let line = content
                    .lines()
                    .enumerate()
                    .find(|(_, l)| l.contains(token))
                    .map(|(i, _)| i + 1)
                    .unwrap_or(0);
                violations.push(format!("{file_name}:{line}: OS 直引 `{token}`"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "vb_kit 触碰 OS 红线(22 篇 §3.7:平台触点归口 vb_platform 六 trait):\n{}",
        violations.join("\n")
    );
}
