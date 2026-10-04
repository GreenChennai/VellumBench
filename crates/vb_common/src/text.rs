//! 文本小工具(跨 crate 共用;BOM/URL 解码等)。
//!
//! 提取成公共函数的纪律:同一逻辑出现两处以上(如 HTML 与 CSS 读盘的
//! BOM 剥除)必须单源,不许各写一份手工对齐。

/// 剥 UTF-8 BOM(`\u{FEFF}` 前缀)。Windows 记事本等编辑器出品
/// 的文本常带 BOM,留着会把首条选择器/标签匹配失败。
pub fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{FEFF}').unwrap_or(s)
}

/// 最小 percent 解码(`%20` → 空格;仅限 UTF-8 文本路径)。
///
/// 非法序列(`%` 后不足两位或非 hex)按字面保留 —— 与浏览器的
/// 宽容行为一致,不丢字符;调用方负责后续的路径校验。
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let h = bytes.get(i + 1).and_then(|b| (*b as char).to_digit(16));
            let l = bytes.get(i + 2).and_then(|b| (*b as char).to_digit(16));
            if let (Some(h), Some(l)) = (h, l) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_bom_removes_only_leading_bom() {
        assert_eq!(strip_bom("\u{FEFF}body {}"), "body {}");
        assert_eq!(strip_bom("body {}"), "body {}");
        assert_eq!(strip_bom("\u{FEFF}"), "");
        assert_eq!(strip_bom("a\u{FEFF}b"), "a\u{FEFF}b", "中间的 BOM 是内容");
    }

    #[test]
    fn percent_decode_basic_and_invalid() {
        assert_eq!(percent_decode("styles/main%20x.css"), "styles/main x.css");
        assert_eq!(percent_decode("%7Bx%7D"), "{x}");
        assert_eq!(percent_decode("100%"), "100%", "非法序列按字面保留");
        assert_eq!(percent_decode("%4"), "%4");
        assert_eq!(percent_decode("plain.css"), "plain.css");
        assert_eq!(percent_decode("%E4%B8%AD"), "中");
    }
}
