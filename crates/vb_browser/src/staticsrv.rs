//! 本地静态文件服务(移植 WPI StaticServer 语义:127.0.0.1 随机端口、
//! 禁缓存、目录索引解析),避免 file:// 打开导致的资源失效。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const INDEX_FILES: &[&str] = &["index.html", "index.htm"];

fn mime_of(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("eot") => "application/vnd.ms-fontobject",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain; charset=utf-8",
        Some("xml") => "application/xml; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(
                    std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"),
                    16,
                ) {
                    out.push(b);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn percent_encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn resolve_index(dir: &Path) -> Option<PathBuf> {
    let mut htmls: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
                    .unwrap_or(false)
        })
        .collect();
    htmls.sort();
    for idx in INDEX_FILES {
        let cand = dir.join(idx);
        if cand.is_file() {
            return Some(cand);
        }
    }
    htmls.into_iter().next()
}

fn handle(mut stream: TcpStream, root: Arc<PathBuf>) {
    let peer = stream.try_clone();
    let Ok(peer) = peer else { return };
    let mut reader = BufReader::new(peer);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");
    // 消费剩余请求头(读到空行)
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) => break,
            Ok(_) if h == "\r\n" || h == "\n" => break,
            Ok(_) => {}
            Err(_) => return,
        }
    }
    if method != "GET" && method != "HEAD" {
        let _ = stream.write_all(b"HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n");
        return;
    }
    let decoded = percent_decode(path.split('?').next().unwrap_or("/"));
    let rel = decoded.trim_start_matches('/');
    let target = if rel.is_empty() {
        resolve_index(&root)
    } else {
        let cand = root.join(rel);
        if cand.is_dir() {
            resolve_index(&cand)
        } else {
            Some(cand)
        }
    };
    // 路径逃逸防护
    let safe = target
        .as_ref()
        .and_then(|t| t.canonicalize().ok())
        .map(|c| c.starts_with(root.canonicalize().unwrap_or_else(|_| root.to_path_buf())))
        .unwrap_or(false);
    let Some(file) = target.filter(|_| safe).filter(|t| t.is_file()) else {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        return;
    };
    let Ok(body) = std::fs::read(&file) else {
        let _ =
            stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n");
        return;
    };
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-cache, no-store, must-revalidate\r\nPragma: no-cache\r\nConnection: close\r\n\r\n",
        mime_of(&file),
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    if method == "GET" {
        let _ = stream.write_all(&body);
    }
    let _ = stream.flush();
}

/// 随机端口静态服务;Drop 即停。
pub struct StaticServer {
    root: Arc<PathBuf>,
    listener: TcpListener,
    alive: Arc<AtomicUsize>,
}

impl StaticServer {
    pub fn start(root: &Path) -> Result<Self, String> {
        if !root.is_dir() {
            return Err(format!("源目录不存在: {}", root.display()));
        }
        let listener =
            TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("静态服务绑定失败: {e}"))?;
        let srv = StaticServer {
            root: Arc::new(root.to_path_buf()),
            listener,
            alive: Arc::new(AtomicUsize::new(1)),
        };
        let listener2 = srv.listener.try_clone().map_err(|e| e.to_string())?;
        let root2 = Arc::clone(&srv.root);
        let alive = Arc::clone(&srv.alive);
        std::thread::spawn(move || {
            for conn in listener2.incoming() {
                if alive.load(Ordering::Relaxed) == 0 {
                    break;
                }
                match conn {
                    Ok(s) => {
                        let root3 = Arc::clone(&root2);
                        std::thread::spawn(move || handle(s, root3));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(srv)
    }

    pub fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    /// 目录挂载的入口 URL(解析 index;无 index 报错,与 WPI 语义一致)。
    pub fn url_for_dir(&self) -> Result<String, String> {
        let index = resolve_index(&self.root)
            .ok_or_else(|| format!("目录中未找到任何 HTML 文件: {}", self.root.display()))?;
        let name = index
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("index.html");
        Ok(format!(
            "http://127.0.0.1:{}/{}",
            self.port(),
            percent_encode_path(name)
        ))
    }

    /// 单文件挂载(挂父目录,URL 指向该文件;相对资源按父目录解析)。
    pub fn url_for_file(&self, file: &Path) -> String {
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("index.html");
        format!(
            "http://127.0.0.1:{}/{}",
            self.port(),
            percent_encode_path(name)
        )
    }
}

impl Drop for StaticServer {
    fn drop(&mut self) {
        self.alive.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_and_encode() {
        assert_eq!(
            percent_decode("%E6%A9%99%E9%9D%92%E8%89%B2.html"),
            "橙青色.html"
        );
        assert_eq!(
            percent_encode_path("橙青色.html"),
            "%E6%A9%99%E9%9D%92%E8%89%B2.html"
        );
        assert_eq!(percent_encode_path("index.html"), "index.html");
    }

    #[test]
    fn serves_index_and_fonts() {
        let base = std::env::temp_dir().join(format!("vb-srv-t-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("fonts")).unwrap();
        std::fs::write(base.join("index.html"), b"<html><body>x</body></html>").unwrap();
        std::fs::write(base.join("fonts").join("a.ttf"), b"fontbytes").unwrap();
        let srv = StaticServer::start(&base).unwrap();
        let url = srv.url_for_dir().unwrap();
        let (status, body) = crate::httpc::request(
            "127.0.0.1",
            srv.port(),
            "GET",
            &url.splitn(4, '/').nth(3).map(|p| format!("/{p}")).unwrap(),
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(status, 200);
        assert!(body.starts_with(b"<html>"));
        let (status, body) = crate::httpc::request(
            "127.0.0.1",
            srv.port(),
            "GET",
            "/fonts/a.ttf",
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"fontbytes");
        let (status, _) = crate::httpc::request(
            "127.0.0.1",
            srv.port(),
            "GET",
            "/../etc",
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(status, 404);
        let _ = std::fs::remove_dir_all(&base);
    }
}
