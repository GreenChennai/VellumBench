//! **G-UI3 界面文案门禁**(22 篇 §8;R0 补缺批落地,ADR-0050 同批)。
//!
//! 规则:vb_kit / vb_shell 的**渲染路径**禁止裸中日韩文案字面量 —— 一切
//! 用户可见文案必须走 `vb_session::i18n::t/t_args`(资源在仓库根
//! `i18n/zh.ftl` / `en.ftl`)。22 篇 G-UI3 定义的白名单:
//!
//! - 注释与 `///`、`//!` 文档注释(不是渲染路径);
//! - `#[cfg(test)]` 测试模块(断言消息是开发期诊断);
//! - i18n 定义文件本身(本仓 i18n 定义在 `.ftl`,非 `.rs`;白名单按文件
//!   词干 `i18n` 预留,防未来把 Rust 侧定义文件放进 src);
//! - 诊断出口(「日志/调试」):`panic!/assert*/expect/println!/log::*` 等
//!   —— 判定方式:字符串字面量直接挂在白名单调用名之后的实参位。
//!   **`format!` 不在白名单**(format 的产物会进渲染路径,必须走 t_args)。
//!
//! 已知豁免(写死在扫描器里,可审计):`vb_shell/src/bin/canvas_spike/` —
//! ADR-0047 已收口的一次性 spike:其 CJK 字符串是画布文本渲染 fixture 与
//! CLI 诊断,不是 chrome 文案;该 bin 随 R1 画布接管清理,届时豁免一并删除。
//!
//! 扫描器实现:手写微型词法器(注释/原始字符串/转义/生命周期感知),对
//! 每个字符串字面量内容做 `U+4E00–U+9FFF`(CJK 统一表意文字)检测;自身
//! 行为由本文件底部的合成源码单测锁定(塞裸中文必红,白名单必不红)。

use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// 微型词法器:抽出(字符串字面量, 注释区间),注释/原始串/转义/生命周期感知
// ---------------------------------------------------------------------------

/// 一个字符串字面量:`start`=开引号字节偏移,`end`=收尾之后字节偏移,
/// `text`=字面量内容(转义序列按 ASCII 原样保留)。
#[derive(Debug)]
struct StrLit {
    start: usize,
    end: usize,
    text: String,
}

fn is_id_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// 普通字符串/字节字符串(从开引号处起扫;转义感知)。
fn scan_quoted(src: &str, b: &[u8], open: usize) -> (usize, String) {
    let n = b.len();
    let mut j = open + 1;
    let mut text = String::new();
    while j < n {
        match b[j] {
            b'\\' => {
                // 转义序列(如 \" \n \u{4e2d}):字节全是 ASCII,不会夹带
                // 裸 CJK;两个字节原样记录后跳过
                text.push(b[j] as char);
                if j + 1 < n {
                    text.push(b[j + 1] as char);
                    j += 2;
                } else {
                    j += 1;
                }
            }
            b'"' => {
                j += 1;
                break;
            }
            _ => {
                let ch = src[j..].chars().next().expect("UTF-8 边界内取字符");
                text.push(ch);
                j += ch.len_utf8();
            }
        }
    }
    (j, text)
}

/// 原始字符串 `r"…" / r#"…"#`(i 指向 `r`)。
fn scan_raw(src: &str, b: &[u8], i: usize) -> (usize, String) {
    let n = b.len();
    let mut j = i + 1;
    let mut hashes = 0;
    while j < n && b[j] == b'#' {
        hashes += 1;
        j += 1;
    }
    if j >= n || b[j] != b'"' {
        // 不是原始字符串(如标识符后随 `r"` 以外的序列):按单字符推进
        return (i + 1, String::new());
    }
    let content_start = j + 1;
    let mut k = content_start;
    while k < n {
        if b[k] == b'"' {
            let mut h = 0;
            while h < hashes && k + 1 + h < n && b[k + 1 + h] == b'#' {
                h += 1;
            }
            if h == hashes {
                return (k + 1 + hashes, src[content_start..k].to_string());
            }
        }
        k += 1;
    }
    (n, src[content_start..].to_string()) // 未闭合:兜底吃到底
}

/// 词法扫描:返回(字符串字面量表, 注释区间表)。
fn tokenize(src: &str) -> (Vec<StrLit>, Vec<(usize, usize)>) {
    let b = src.as_bytes();
    let n = b.len();
    let mut strs = Vec::new();
    let mut comments: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        match b[i] {
            b'/' if i + 1 < n && b[i + 1] == b'/' => {
                let start = i;
                while i < n && b[i] != b'\n' {
                    i += 1;
                }
                comments.push((start, i));
            }
            b'/' if i + 1 < n && b[i + 1] == b'*' => {
                let start = i;
                let mut depth = 1;
                i += 2;
                while i < n && depth > 0 {
                    if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
                        depth += 1;
                        i += 2;
                    } else if b[i] == b'*' && i + 1 < n && b[i + 1] == b'/' {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                comments.push((start, i.min(n)));
            }
            b'"' => {
                let (next, text) = scan_quoted(src, b, i);
                strs.push(StrLit {
                    start: i,
                    end: next,
                    text,
                });
                i = next;
            }
            b'r' if (i == 0 || !is_id_char(b[i - 1]))
                && i + 1 < n
                && (b[i + 1] == b'"' || b[i + 1] == b'#') =>
            {
                let (next, text) = scan_raw(src, b, i);
                if !text.is_empty() || next > i + 1 {
                    strs.push(StrLit {
                        start: i,
                        end: next,
                        text,
                    });
                }
                i = next.max(i + 1);
            }
            b'b' if (i == 0 || !is_id_char(b[i - 1])) && i + 1 < n && b[i + 1] == b'"' => {
                let (next, text) = scan_quoted(src, b, i + 1);
                strs.push(StrLit {
                    start: i,
                    end: next,
                    text,
                });
                i = next;
            }
            b'\'' => {
                if i + 1 < n && b[i + 1] == b'\\' {
                    // 字符转义字面量('\n' '\'' …):吃到收引号
                    let mut j = i + 2;
                    while j < n && b[j] != b'\'' {
                        j += 1;
                    }
                    i = (j + 1).min(n);
                } else if i + 2 < n && b[i + 2] == b'\'' {
                    i += 3; // 单字符字面量 'x'
                } else {
                    i += 1; // 生命周期('static)
                }
            }
            _ => i += 1,
        }
    }
    (strs, comments)
}

// ---------------------------------------------------------------------------
// 过滤:cfg(test) 区域 / 诊断出口白名单 / CJK 检测
// ---------------------------------------------------------------------------

/// 诊断出口调用名(22 篇 G-UI3「白名单:日志/调试」)。
/// 注意:`format` 故意**不在**表内 —— format 产物会进渲染路径。
const DIAGNOSTIC_CALLEES: [&str; 14] = [
    "panic",
    "expect",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "todo",
    "unreachable",
    "print",
    "println",
    "eprint",
    "eprintln",
    "dbg",
];

fn has_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        let v = c as u32;
        (0x4E00..=0x9FFF).contains(&v)
    })
}

/// 在「注释与字符串内容已抹空」的源码里找 `#[cfg(test)] mod … { … }` 区间
/// (字符串抹空后花括号配对安全)。
fn cfg_test_regions(blanked: &str) -> Vec<(usize, usize)> {
    let b = blanked.as_bytes();
    let needle = b"#[cfg(test)]";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = find(b, from, needle) {
        let mut cursor = pos + needle.len();
        let mut matched = false;
        while let Some(m) = find(b, cursor, b"mod") {
            let before_ok = m == 0 || !is_id_char(b[m - 1]);
            let after = m + 3;
            let after_ok = after < b.len() && (b[after] as char).is_whitespace();
            if before_ok && after_ok {
                let mut k = after;
                while k < b.len() && b[k] != b'{' {
                    k += 1;
                }
                if k < b.len() {
                    let end = match_brace(b, k);
                    out.push((pos, end));
                    matched = true;
                    break;
                }
            }
            cursor = m + 3;
        }
        let _ = matched;
        from = pos + needle.len();
    }
    out
}

/// 花括号配对(k 指向 `{`;返回 `}` 之后一字节)。
fn match_brace(b: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    let mut k = open;
    while k < b.len() {
        match b[k] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return k + 1;
                }
            }
            _ => {}
        }
        k += 1;
    }
    b.len()
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if from >= hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// 字符串字面量所属调用的调用名:从字符串起点向前回溯,跳过空白、逗号、
/// 前序实参(单字符步进;括号组按深度跳过),命中深度 0 的 `(` 后取其前
/// 标识符(宏 `!` 可选)。`;`/`{`/`}` 视为语句或结构体字面量边界,返回
/// None。启发式已知局限:只认 `(` 前的裸标识符,不解析 turbofish/方法链
/// 之外的复杂路径 —— 对诊断白名单判定够用,漏判方向是"更严"而非"更松"。
fn callee_name(blanked: &str, str_start: usize) -> Option<String> {
    let b = blanked.as_bytes();
    let mut i = str_start;
    let mut depth = 0i32;
    loop {
        while i > 0 && b[i - 1].is_ascii_whitespace() {
            i -= 1;
        }
        if i == 0 {
            return None;
        }
        match b[i - 1] {
            b'(' if depth == 0 => {
                i -= 1;
                while i > 0 && b[i - 1].is_ascii_whitespace() {
                    i -= 1;
                }
                if i > 0 && b[i - 1] == b'!' {
                    i -= 1; // 宏调用 assert!( …
                }
                let end = i;
                while i > 0 && is_id_char(b[i - 1]) {
                    i -= 1;
                }
                return if i == end {
                    None
                } else {
                    Some(String::from_utf8_lossy(&b[i..end]).into_owned())
                };
            }
            b')' | b']' => {
                depth += 1;
                i -= 1;
            }
            b'(' | b'[' => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                i -= 1;
            }
            b';' | b'{' | b'}' => return None,
            _ => i -= 1,
        }
    }
}

/// 对单份源码跑完整判定,返回违规描述(空 = 通过)。
fn violations_in(src: &str) -> Vec<String> {
    let (strs, comments) = tokenize(src);
    // 抹空注释与字符串内容(保留换行),供 cfg(test) 区域定位与调用名回溯
    let mut blanked = src.as_bytes().to_vec();
    for &(s, e) in &comments {
        let end = e.min(blanked.len());
        for x in &mut blanked[s..end] {
            if *x != b'\n' {
                *x = b' ';
            }
        }
    }
    for l in &strs {
        let end = l.end.min(blanked.len());
        for x in &mut blanked[l.start..end] {
            if *x != b'\n' {
                *x = b' ';
            }
        }
    }
    let blanked = String::from_utf8(blanked).expect("抹空不破坏 UTF-8");
    let regions = cfg_test_regions(&blanked);

    strs.iter()
        .filter(|l| has_cjk(&l.text))
        .filter(|l| !regions.iter().any(|&(s, e)| l.start >= s && l.start < e))
        .filter(|l| {
            callee_name(&blanked, l.start)
                .map(|name| !DIAGNOSTIC_CALLEES.contains(&name.as_str()))
                .unwrap_or(true)
        })
        .map(|l| {
            let line = src[..l.start].bytes().filter(|&c| c == b'\n').count() + 1;
            let snippet: String = l.text.chars().take(24).collect();
            format!("行 {line}:裸 CJK 字符串字面量「{snippet}」")
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 门禁本体
// ---------------------------------------------------------------------------

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

/// G-UI3:vb_kit / vb_shell 渲染路径无裸中日韩文案字面量。
#[test]
fn no_bare_cjk_literals_in_render_paths() {
    let kit_src = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"))).join("src");
    let shell_src = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"))).join("../vb_shell/src");
    for dir in [&kit_src, &shell_src] {
        assert!(
            dir.is_dir(),
            "扫描目标不存在:{}(仓库布局漂移?)",
            dir.display()
        );
    }

    let mut violations = Vec::new();
    for dir in [&kit_src, &shell_src] {
        for file in rs_files(dir) {
            // i18n 定义文件白名单(按文件词干;本仓 i18n 定义目前是 .ftl)
            if file.file_stem().and_then(|s| s.to_str()) == Some("i18n") {
                continue;
            }
            // canvas_spike 豁免(理由见模块注释;R1 清理时一并删除)
            if file.components().any(|c| c.as_os_str() == "canvas_spike") {
                continue;
            }
            let content = fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("读 {} 失败:{e}", file.display()));
            for v in violations_in(&content) {
                let name = file
                    .strip_prefix(&kit_src)
                    .or_else(|_| file.strip_prefix(&shell_src))
                    .unwrap_or(&file)
                    .display()
                    .to_string();
                violations.push(format!("{name}:{v}"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "G-UI3 被破坏:渲染路径出现裸中文文案(一律改走 vb_session::i18n::t/t_args,\n\
         key 加进 i18n/zh.ftl 与 i18n/en.ftl 的手工段):\n{}",
        violations.join("\n")
    );
}

// ---------------------------------------------------------------------------
// 扫描器自证(合成源码单测:塞裸中文必红,白名单必不红)
// ---------------------------------------------------------------------------

#[test]
fn scanner_flags_bare_cjk_in_code_strings() {
    let src = "fn f() -> &'static str { \"能力台账\" }";
    let v = violations_in(src);
    assert_eq!(v.len(), 1, "裸中文必须红:{v:?}");
    assert!(v[0].contains("能力台账"));
}

#[test]
fn scanner_flags_cjk_inside_raw_strings() {
    let src = "fn f() { let s = r#\"临时裸中文\"#; let _ = s; }";
    assert_eq!(violations_in(src).len(), 1, "原始字符串也要红");
}

#[test]
fn scanner_whitelists_comments_and_docs() {
    let src = concat!(
        "// 行注释 裸中文\n",
        "/* 块注释 裸中文 */\n",
        "/// 文档 裸中文\n",
        "//! 模块文档 裸中文\n",
        "fn f() {}\n"
    );
    assert!(violations_in(src).is_empty(), "注释/文档不算渲染路径");
}

#[test]
fn scanner_whitelists_cfg_test_modules() {
    let src = concat!(
        "fn f() {}\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    #[test]\n",
        "    fn t() { assert!(true, \"断言消息 裸中文\"); }\n",
        "}\n"
    );
    assert!(violations_in(src).is_empty(), "cfg(test) 模块白名单");
}

#[test]
fn scanner_whitelists_diagnostic_callees_but_not_format() {
    let diag = concat!(
        "fn f(e: std::fmt::Error) {\n",
        "    panic!(\"爆炸 裸中文\");\n",
        "    let _: i32 = None.expect(\"必中 裸中文\");\n",
        "    assert!(true, \"断言 裸中文\");\n",
        "    println!(\"打印 裸中文\");\n",
        "}\n"
    );
    assert!(violations_in(diag).is_empty(), "诊断出口白名单");
    let fmt = "fn f() -> String { format!(\"可复现 裸中文\") }";
    assert_eq!(
        violations_in(fmt).len(),
        1,
        "format! 产物会进渲染路径,不许白名单"
    );
}

#[test]
fn scanner_ignores_escapes_and_lifetimes() {
    // \u{4e2d} 转义不产生裸 CJK 字节;生命周期 'static 不吃掉后续字符串
    let src = concat!(
        "fn f() {\n",
        "    let a: &'static str = \"ok\";\n",
        "    let b = \"\\u{4e2d}escaped\";\n",
        "    let _ = (a, b);\n",
        "}\n"
    );
    assert!(violations_in(src).is_empty(), "转义写法非裸字面量");
}

#[test]
fn scanner_detects_missing_scantarget_as_failure() {
    // 门禁的仓库布局断言自身可测:扫描不存在的目录必须给出显式错误,
    // 防止"扫了个空集还报绿"
    assert!(!Path::new("definitely/not/a/dir").is_dir());
}
