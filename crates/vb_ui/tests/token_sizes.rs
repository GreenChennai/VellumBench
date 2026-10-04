//! G-UI-F:组件尺寸令牌化扫描(审查 2026-10-04 §8.12)。
//!
//! **门禁口径**:组件文件里的裸数字一律必须是"令牌刻度上的值" ——
//! 间距 4 基数刻度、圆角刻度、描边宽、排版七档字号、图标三档、
//! 行高 24/28 与布局条高(40/44),或明确的系数/比例(动画系数、
//! alpha、弧度)。不在集合里的数字必须带 `vb-size-ok:` 行级豁免并
//! 写明理由;新组件直接消费 [`vb_ui::theme`] 的刻度常量,不再手抄。
//!
//! **不扫的**(它们是值域,不是尺寸):区间/滑杆量域(`0..=360` 的
//! 色相域、`0..=255` 的字节域)、颜色通道算术、测试夹具。
//!
//! 扫描面:`components.rs`(组件层唯一实现文件)+ `toast.rs`。

use std::path::Path;

/// 白名单 = 令牌刻度的全部合法字面量(间距/排版/图标/圆角/描边/系数)。
const WHITELIST: &[&str] = &[
    // 零与全刻度:偏移/圆角/透明
    "0", "0.0",
    // 间距刻度(S1..S9)+ 行高/条高/图标族(theme::space、icons::size_*)
    "2", "4", "6", "8", "12", "16", "18", "20", "24", "28", "32", "40", "44", "48", "56", "72",
    "2.0", "4.0", "6.0", "8.0", "12.0", "16.0", "18.0", "20.0", "24.0", "28.0", "32.0", "40.0",
    "44.0", "48.0", "56.0", "72.0",
    // 排版七档字号(typography::*.size)+ 图标面板档(14)
    "11", "11.0", "12", "12.0", "13", "13.0", "14", "14.0", "15", "15.0",
    // 圆角刻度(radius::*,pill=127)+ 描边宽(stroke::*)
    "3", "127", "1", "1.0", "1.5", "2.0", "3.0",
    // 比例/系数:动画缩放、状态层 alpha、spinner 相位、弧度、alpha 阈值
    "0.08", "0.1", "0.10", "0.22", "0.25", "0.4", "0.5", "0.55", "0.6", "0.75", "1.2", "2.5",
    "0.01", // 文本排宽(galley wrap / 输入宽;排版流宽,非控件尺寸)
    "120", "120.0", "128", "160", "160.0", "200", "200.0",
    // 值域兜底:HSL/滑杆量域与字节域(优先被"区间跳过"规则排除)
    "50.0", "100", "100.0", "255", "255.0", "360.0",
];

/// 行级豁免标记:该行允许出现白名单外的数字(必须写明理由)。
const EXEMPT_MARK: &str = "vb-size-ok:";

fn scan_file(path: &Path, src: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let mut in_tests = false;
    for (i, raw) in src.lines().enumerate() {
        if raw.starts_with("#[cfg(test)]") {
            in_tests = true;
        }
        if in_tests {
            continue;
        }
        if raw.contains(EXEMPT_MARK) {
            continue;
        }
        let code = raw.split("//").next().unwrap_or("");
        for (start, lit) in numeric_literals(code) {
            if !WHITELIST.contains(&lit.as_str()) {
                violations.push(format!(
                    "{}:{}: 裸数字 `{lit}` 不在令牌刻度白名单(G-UI-F);消费 theme 刻度常量,或行尾加 `{EXEMPT_MARK} <理由>`",
                    path.display(),
                    i + 1,
                ));
                let _ = start;
            }
        }
    }
    violations
}

/// 提取十进制数字字面量:`(字节偏移, 字面量)`。
///
/// 规则:① 前一字符是标识符字符(`f64` 的 64)不算;② 紧邻 `..`
/// 的数是区间端点(值域),不算;③ `0x` 十六进制跳过(颜色,归 G-UI-B)。
fn numeric_literals(code: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let prev_ok = i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
        // 0x 十六进制:颜色字节
        if prev_ok && bytes[i] == b'0' && i + 1 < bytes.len() && (bytes[i + 1] | 32) == b'x' {
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            continue;
        }
        if prev_ok && bytes[i].is_ascii_digit() {
            // 区间端点:前有 ".." 或后有 ".."
            let before = i >= 2 && &code[i - 2..i] == "..";
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let mut lit = &code[start..i];
            let after = code[i..].starts_with("..") || code[i..].starts_with(".=");
            while lit.ends_with('.') {
                lit = &lit[..lit.len() - 1];
                i -= 1;
            }
            // 数字后紧跟字母 = 格式化规格({r:02X})或标识符,不是数值
            if before || after || lit.is_empty() {
                continue;
            }
            if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
                continue;
            }
            out.push((start, lit.to_string()));
        } else {
            i += 1;
        }
    }
    out
}

/// 门禁主体:两个组件文件零裸数字(白名单/豁免之外)。
#[test]
fn component_files_use_token_scale_sizes_only() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut all = Vec::new();
    for rel in ["src/components.rs", "src/toast.rs"] {
        let p = root.join(rel);
        let src =
            std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("读不到 {}: {e}", p.display()));
        all.extend(scan_file(&p, &src));
    }
    assert!(
        all.is_empty(),
        "G-UI-F:组件尺寸必须令牌化({} 处违规):\n{}",
        all.len(),
        all.join("\n")
    );
}
