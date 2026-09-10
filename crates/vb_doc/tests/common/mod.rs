//! 往返测试共用工具(各 test target 各自 include 一份,故允许 dead_code)。
//!
//! - `roundtrip`:导入 → 导出(落盘)→ 再导入 → 再导出,返回两次导出文件表。
//! - `find` / `css`:从文件表里取内容。
//! - `check_l0_l1`:对一个 HTML 源跑 L0(不损坏)+ L1(幂等)两组断言。

#![allow(dead_code)]

use std::sync::atomic::{AtomicU32, Ordering};

use vb_doc::export::render_project;
use vb_doc::import::import_project;

/// 导入 → 导出(落盘)→ 再导入 → 再导出;返回两次导出的文件表。
/// 走真实目录,保证外链 CSS 在第二次导入时可见(与 Agent/浏览器看到的一致)。
#[allow(clippy::type_complexity)]
pub fn roundtrip(html: &str) -> (Vec<(String, String)>, Vec<(String, String)>) {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "vb-rt-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), html).unwrap();

    let r1 = import_project(&dir).unwrap();
    let out1 = render_project(&r1.doc);
    for (rel, content) in &out1.files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }

    let r2 = import_project(&dir).unwrap();
    let out2 = render_project(&r2.doc);

    let result = (out1.files, out2.files);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// 把「相对路径 → 内容」文件表写进临时目录后导入,返回文档。
/// 用于验证"HTML 里写的东西能不能被读回模型"这类单向断言。
pub fn import_files(files: &[(&str, &str)]) -> vb_doc::Document {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "vb-imp-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (rel, content) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    let r = import_project(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    r.doc
}

pub fn find(files: &[(String, String)], path: &str) -> String {
    files
        .iter()
        .find(|(p, _)| p == path)
        .map(|(_, c)| c.clone())
        .unwrap_or_else(|| panic!("缺少 {path}"))
}

pub fn css(files: &[(String, String)]) -> String {
    find(files, "styles/main.css")
}

/// 全部导出文件正文拼在一起(便于做"某段内容还在不在"的检查)。
pub fn all_text(files: &[(String, String)]) -> String {
    files
        .iter()
        .map(|(_, c)| c.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 从 HTML 源里抽出所有 `data-vb-id="xxx"` 的取值。
pub fn vb_ids(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find("data-vb-id=\"") {
        rest = &rest[i + "data-vb-id=\"".len()..];
        if let Some(j) = rest.find('"') {
            out.push(rest[..j].to_string());
            rest = &rest[j..];
        } else {
            break;
        }
    }
    out.sort();
    out.dedup();
    out
}

/// 对一段 HTML 源跑 L0 + L1;失败信息以 `Err` 返回。
///
/// - **L1 幂等**:两次导出的每个文件字节相同。
/// - **L0 不损坏**:源里的每个 `data-vb-id` 与 `<!DOCTYPE html>` 仍在输出中。
/// - **sidecar**:`expects` 中每个子串必须出现在输出里。
pub fn check_l0_l1(label: &str, src: &str, expects: &[String]) -> Result<(), String> {
    let (f1, f2) = roundtrip(src);

    // L1:逐文件字节幂等
    if f1.len() != f2.len() {
        return Err(format!(
            "[{label}] L1 幂等失败:两次导出文件数不同 {} vs {}",
            f1.len(),
            f2.len()
        ));
    }
    for (a, b) in f1.iter().zip(f2.iter()) {
        if a.0 != b.0 {
            return Err(format!("[{label}] L1 文件顺序不一致:{} vs {}", a.0, b.0));
        }
        if a.1 != b.1 {
            return Err(format!(
                "[{label}] L1 幂等失败:{}\n  第 1 次:…{}\n  第 2 次:…{}",
                a.0,
                snippet(&a.1, first_diff(&a.1, &b.1)),
                snippet(&b.1, first_diff(&a.1, &b.1))
            ));
        }
    }

    let text = all_text(&f1);
    // L0:DOCTYPE
    if !text.contains("<!DOCTYPE html>") {
        return Err(format!("[{label}] L0 丢失 <!DOCTYPE html>"));
    }
    // L0:sid 全保留
    for id in vb_ids(src) {
        if !text.contains(&format!("data-vb-id=\"{id}\"")) {
            return Err(format!("[{label}] L0 丢失 data-vb-id=\"{id}\""));
        }
    }
    // L0:sidecar 期望子串
    for needle in expects {
        if !text.contains(needle.as_str()) {
            return Err(format!("[{label}] L0 丢失期望片段:{needle}"));
        }
    }
    Ok(())
}

fn first_diff(a: &str, b: &str) -> usize {
    a.bytes()
        .zip(b.bytes())
        .position(|(x, y)| x != y)
        .unwrap_or_else(|| a.len().min(b.len()))
}

/// 差异点附近的文本窗口(便于定位不一致的内容)。
fn snippet(s: &str, at: usize) -> String {
    let start = at.saturating_sub(60);
    let end = (at + 60).min(s.len());
    if !s.is_char_boundary(start) || !s.is_char_boundary(end) {
        return format!("(第 {at} 字节,落在字符中间)");
    }
    s[start..end].replace('\n', "\\n")
}
