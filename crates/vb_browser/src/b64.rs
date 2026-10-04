//! 车道 B 传输层小工具:base64 编解码(截图数据 / WebSocket 握手键)。

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(B64_ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64_ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64_ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// base64 解码(EXP-09 严格尾块 + PERF-04 流式单缓冲)。
///
/// - 流式:逐字节滑 4 窗直接产出,不再先 collect 一份过滤后字节
///   (MB 级截图载荷此前要同时驻留输入串 + 过滤副本 + 输出三份);
/// - 严格尾块:去除空白/`=` 后长度 mod 4 == 1(6 bit 无法凑出一个完整
///   字节)返回 Err —— 截断的截图数据不再静默产出垃圾字节。
pub fn decode(text: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u32, String> {
        Ok(match c {
            b'A'..=b'Z' => (c - b'A') as u32,
            b'a'..=b'z' => (c - b'a' + 26) as u32,
            b'0'..=b'9' => (c - b'0' + 52) as u32,
            b'+' => 62,
            b'/' => 63,
            _ => return Err(format!("非法 base64 字符 {}", c as char)),
        })
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut win = [0u32; 4];
    let mut n = 0usize;
    for &b in text.as_bytes() {
        if b.is_ascii_whitespace() || b == b'=' {
            continue;
        }
        win[n] = val(b)?;
        n += 1;
        if n == 4 {
            let word = (win[0] << 18) | (win[1] << 12) | (win[2] << 6) | win[3];
            out.push((word >> 16) as u8);
            out.push((word >> 8) as u8);
            out.push(word as u8);
            n = 0;
        }
    }
    match n {
        0 => {}
        1 => return Err("base64 尾块畸形:去除填充后长度 mod 4 == 1(数据被截断?)".into()),
        2 => {
            let word = (win[0] << 18) | (win[1] << 12);
            out.push((word >> 16) as u8);
        }
        3 => {
            let word = (win[0] << 18) | (win[1] << 12) | (win[2] << 6);
            out.push((word >> 16) as u8);
            out.push((word >> 8) as u8);
        }
        _ => unreachable!("窗口计数越界"),
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_small() {
        for len in 0..40 {
            let data: Vec<u8> = (0..len as u8).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(decode(&encode(&data)).unwrap(), data, "len={len}");
        }
    }

    #[test]
    fn known_vectors() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(decode("Zm9v").unwrap(), b"foo");
    }

    /// EXP-09:畸形尾块必须 Err,不得静默产出垃圾字节(截断截图防线)。
    #[test]
    fn malformed_tail_is_strict_error() {
        assert!(decode("Z").is_err(), "mod 4 == 1 必须报错");
        assert!(decode("Zm9vZ").is_err(), "5 字符含畸形尾块");
        assert!(decode("Zm9vZm9vZ").is_err(), "9 字符含畸形尾块");
        assert!(decode("!m9v").is_err(), "非法字符仍报错");
        // 合法尾块(2/3 字符 + 填充或裸)保持可解
        assert_eq!(decode("Zg=").unwrap(), b"f");
        assert_eq!(decode("Zg").unwrap(), b"f");
        assert_eq!(decode("Zm8").unwrap(), b"fo");
        assert_eq!(decode("Zm9vZm9").unwrap(), b"foofo", "mod 4 == 3 是合法尾块");
        // 空白容忍保持(CDP 载荷偶带换行)
        assert_eq!(decode("Zm9v\n").unwrap(), b"foo");
    }
}
