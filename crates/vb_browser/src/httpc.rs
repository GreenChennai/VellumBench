//! 极简 HTTP/1.1 客户端:只服务 DevTools 本地端点(/json/new、/json/list)。

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// 发起本地 HTTP 请求,返回 (状态码, 响应体)。
/// 只支持无 TLS、无 chunked(DevTools 端点返回定长或连接关闭分界)。
pub fn request(
    host: &str,
    port: u16,
    method: &str,
    path: &str,
    timeout: Duration,
) -> Result<(u16, Vec<u8>), String> {
    let mut stream =
        TcpStream::connect((host, port)).map_err(|e| format!("连接 {host}:{port} 失败: {e}"))?;
    stream.set_read_timeout(Some(timeout)).ok();
    stream.set_write_timeout(Some(timeout)).ok();
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("HTTP 请求写入失败: {e}"))?;
    // 读到完整响应:优先按 Content-Length 精确读满;连接关闭或超时兜底
    let deadline = std::time::Instant::now() + timeout;
    let mut raw = Vec::with_capacity(8 * 1024);
    let mut buf = [0u8; 16 * 1024];
    let (status, head, body) = loop {
        // 头部是否完整
        if let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&raw[..header_end]).to_ascii_lowercase();
            let status: u16 = head
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| format!("HTTP 状态行解析失败: {head}"))?;
            let body = raw[header_end + 4..].to_vec();
            let content_length: Option<usize> = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok());
            match content_length {
                Some(want) if body.len() >= want => {
                    break (status, head, body[..want].to_vec());
                }
                None if head.contains("transfer-encoding: chunked") => {
                    // chunked:读到 0 终止块;超时则按现有数据尽力解包
                    if dechunk_complete(&body) || std::time::Instant::now() >= deadline {
                        break (status, head, body);
                    }
                }
                _ => {
                    if std::time::Instant::now() >= deadline {
                        break (status, head, body);
                    }
                }
            }
        } else if std::time::Instant::now() >= deadline {
            return Err("HTTP 响应缺少头部结束符(超时)".into());
        }
        match stream.read(&mut buf) {
            Ok(0) => {
                // 连接关闭:头部齐则按已收数据收尾,否则报错
                if let Some(header_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&raw[..header_end]).to_ascii_lowercase();
                    let status: u16 = head
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    break (status, head, raw[header_end + 4..].to_vec());
                }
                return Err("HTTP 连接在响应前关闭".into());
            }
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if std::time::Instant::now() >= deadline {
                    return Err("HTTP 读取超时".into());
                }
            }
            Err(e) => return Err(format!("HTTP 读取失败: {e}")),
        }
    };
    let mut out = body;
    if head.contains("transfer-encoding: chunked") {
        out = dechunk(&out);
    }
    Ok((status, out))
}

/// chunked 数据是否已含终止块(0 长度 chunk)。
fn dechunk_complete(data: &[u8]) -> bool {
    data.windows(5).any(|w| w == b"\r\n0\r\n\r\n")
}

fn dechunk(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while let Some(line_end) = data[pos..]
        .windows(2)
        .position(|w| w == b"\r\n")
        .map(|p| pos + p)
    {
        let size_str = String::from_utf8_lossy(&data[pos..line_end]);
        let size = match usize::from_str_radix(size_str.trim().split(';').next().unwrap_or("0"), 16)
        {
            Ok(s) => s,
            Err(_) => break,
        };
        if size == 0 {
            break;
        }
        let start = line_end + 2;
        let end = (start + size).min(data.len());
        out.extend_from_slice(&data[start..end]);
        pos = end + 2;
    }
    out
}
