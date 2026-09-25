//! 本地静态文件服务(移植 WPI StaticServer 语义:127.0.0.1 随机端口、
//! 禁缓存、目录索引解析),避免 file:// 打开导致的资源失效。
//!
//! 越界判定(ADR-0045,方案 C):词法规范化做**主判定**(折叠 `.`/`..`,
//! 拒绝越出 root 的 `..`);对词法越出的请求再查 `canonicalize()` 是否落在
//! 允许根集合内。不解析联接的词法判定使「项目内声明的目录联接」
//! (`src/fonts` → 技能字体库等减重影子)不再被误判 404(VB-1)。

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const INDEX_FILES: &[&str] = &["index.html", "index.htm"];

/// 404 告警收集上限(防病态页面刷爆内存;超出部分不再记录)。
const NOT_FOUND_CAP: usize = 64;

/// 404 资源共享收集器:请求线程写入,导出结束由调用方取走并入
/// KilnReport(VB-1:404 不许静默——字体/脚本缺失会被浏览器静默降级)。
pub type NotFoundLog = Arc<Mutex<Vec<String>>>;

/// 404 资源的用户可见文案。
///
/// 注意:与 `vb_kiln::error::KilnWarning::AssetNotFound` 的 message() 保持
/// 同一格式(vb_browser 不依赖 vb_kiln,故此处保留同形字面量)。
pub fn asset_not_found_message(src: &str) -> String {
    format!("静态资源 404:{src}(浏览器车道;字体/脚本缺失会静默降级)")
}

/// 词法路径规范化:折叠 `.` 与 `..`(不解析符号链接/目录联接,不触盘)。
/// `..` 越出顶层时保留为字面 `..`(后续 `starts_with` 判定自然拒绝)。
fn lexically_normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// 越界判定(ADR-0045 方案 C):
/// 1) **主判定**:`target` 词法规范化后仍以 `root` 开头 → 放行(项目内
///    联接在此通过,canonicalize 的联接解析不再误伤);
/// 2) **兜底**:词法越出的请求,`canonicalize()`(解析联接后的真实位置)
///    落在允许根集合内 → 放行;集合为空或 canonicalize 失败一律拒绝。
pub fn path_allowed(root: &Path, target: &Path, allowed_roots: &[PathBuf]) -> bool {
    if lexically_normalize(target).starts_with(lexically_normalize(root)) {
        return true;
    }
    let Ok(canon) = target.canonicalize() else {
        return false;
    };
    allowed_roots
        .iter()
        .any(|r| canon.starts_with(lexically_normalize(r)))
}

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

fn handle(mut stream: TcpStream, root: Arc<PathBuf>, not_found: NotFoundLog) {
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
    // 路径逃逸防护(ADR-0045 方案 C):词法主判定 + 允许根 canonicalize 兜底。
    // 允许根 = root 自身的规范形(root 下指向 root 外的联接目标由词法主
    // 判定放行,不在此列;见模块注释)。
    let allowed_roots: Vec<PathBuf> = root.canonicalize().ok().into_iter().collect();
    let safe = target
        .as_ref()
        .map(|t| path_allowed(&root, t, &allowed_roots))
        .unwrap_or(false);
    let Some(file) = target.filter(|_| safe).filter(|t| t.is_file()) else {
        // VB-1:404 必须留痕(字体/脚本 404 会被浏览器静默降级,下游肉眼
        // 才能发现)。记录请求相对路径,导出结束并入 KilnReport。
        {
            let mut log = not_found.lock().unwrap_or_else(|e| e.into_inner());
            if log.len() < NOT_FOUND_CAP {
                let src = if rel.is_empty() { "/" } else { rel };
                if !log.iter().any(|s| s == src) {
                    log.push(src.to_string());
                }
            }
        }
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
    /// 404 收集器(VB-1):`take_not_found` 在导出结束时取走。
    not_found: NotFoundLog,
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
            not_found: Arc::new(Mutex::new(Vec::new())),
        };
        let listener2 = srv.listener.try_clone().map_err(|e| e.to_string())?;
        let root2 = Arc::clone(&srv.root);
        let alive = Arc::clone(&srv.alive);
        let not_found2 = Arc::clone(&srv.not_found);
        std::thread::spawn(move || {
            for conn in listener2.incoming() {
                if alive.load(Ordering::Relaxed) == 0 {
                    break;
                }
                match conn {
                    Ok(s) => {
                        let root3 = Arc::clone(&root2);
                        let nf3 = Arc::clone(&not_found2);
                        std::thread::spawn(move || handle(s, root3, nf3));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(srv)
    }

    /// 取走本轮全部 404 资源(请求相对路径;导出结束并入 KilnReport)。
    pub fn take_not_found(&self) -> Vec<String> {
        let mut log = self.not_found.lock().unwrap_or_else(|e| e.into_inner());
        std::mem::take(&mut *log)
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

    /// 词法规范化:折叠 ./..;`..` 越出顶层保留为字面量(由 starts_with 拒绝)。
    #[test]
    fn lexical_normalization_folds_dots() {
        let base = PathBuf::from("/").join("root").join("src");
        assert_eq!(
            lexically_normalize(&base.join("fonts").join("a.woff2")),
            lexically_normalize(&base).join("fonts").join("a.woff2")
        );
        // ./ 折叠
        assert_eq!(
            lexically_normalize(&base.join(".").join("fonts").join("x.js")),
            lexically_normalize(&base).join("fonts").join("x.js")
        );
        // .. 折叠回 root 内
        assert_eq!(
            lexically_normalize(&base.join("a").join("..").join("b")),
            lexically_normalize(&base).join("b")
        );
        // 越出顶层:留字面 ..,不以 root 开头
        let esc = lexically_normalize(&base.join("..").join("etc").join("passwd"));
        assert!(!esc.starts_with(lexically_normalize(&base)));
        // 纯 ../.. 越界(相对形态)
        let esc2 = lexically_normalize(Path::new("../etc/passwd"));
        assert_eq!(esc2, PathBuf::from("..").join("etc").join("passwd"));
    }

    /// 越界判定(验收 B):`../` 与反斜杠变体、绝对路径注入全部拒绝;
    /// root 内相对路径放行。
    #[test]
    fn path_allowed_rejects_escape_and_accepts_inside() {
        let root = Path::new("/").join("proj").join("src");
        let empty: Vec<PathBuf> = Vec::new();
        assert!(path_allowed(
            &root,
            &root.join("fonts").join("a.woff2"),
            &empty
        ));
        assert!(path_allowed(
            &root,
            &root.join("sub").join("..").join("a.js"),
            &empty
        ));
        // 验收 B:越界拒绝(含 %5C 反斜杠解码后的形态)
        assert!(!path_allowed(
            &root,
            &root.join("..").join("etc").join("passwd"),
            &empty
        ));
        assert!(!path_allowed(
            &root,
            &root.join("..").join("..").join("windows").join("win.ini"),
            &empty
        ));
        assert!(!path_allowed(
            &root,
            &Path::new("/").join("etc").join("passwd"),
            &empty
        ));
        // Windows 形态的反斜杠注入(组件层即被识别为分隔符 + 父目录)
        #[cfg(windows)]
        {
            assert!(!path_allowed(
                &root,
                &root.join("..\\..\\windows\\win.ini"),
                &empty
            ));
        }
    }

    /// 验收 A(Windows 目录联接):联接形态的项目资源必须 200(修复前
    /// canonicalize 把联接解析到 root 外 → 误判 404)。
    /// 联接经 `mklink /J` 创建(不需要管理员);创建失败(策略/文件系统
    /// 不支持)时打印原因诚实跳过,不谎报通过。
    #[cfg(windows)]
    #[test]
    fn junction_assets_are_served() {
        let base = std::env::temp_dir().join(format!("vb-srv-junc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let shared = base.join("shared-fonts");
        let proj = base.join("proj").join("src");
        std::fs::create_dir_all(&shared).unwrap();
        std::fs::create_dir_all(proj.join("js")).unwrap();
        std::fs::write(shared.join("a.woff2"), b"woff2bytes").unwrap();
        std::fs::write(proj.join("js").join("a.js"), b"console.log(1)").unwrap();
        let link = proj.join("fonts");
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&shared)
            .output()
            .ok()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !made {
            eprintln!(
                "[skip-junction] mklink /J 创建失败(文件系统或策略限制),联接用例跳过:{link:?}"
            );
            let _ = std::fs::remove_dir_all(&base);
            return;
        }
        let srv = StaticServer::start(&proj).unwrap();
        for path in ["/fonts/a.woff2", "/js/a.js"] {
            let (status, body) = crate::httpc::request(
                "127.0.0.1",
                srv.port(),
                "GET",
                path,
                std::time::Duration::from_secs(5),
            )
            .unwrap();
            assert_eq!(status, 200, "联接内资源 {path} 必须 200");
            assert!(!body.is_empty());
        }
        // 验收 B(防「修 A 破 B」):越界仍拒绝
        let (status, _) = crate::httpc::request(
            "127.0.0.1",
            srv.port(),
            "GET",
            "/../../etc/passwd",
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(status, 404, "越界访问必须 404");
        // 验收 C:404 留痕(缺失资源进入收集器,导出结束并入 KilnReport)
        let (status, _) = crate::httpc::request(
            "127.0.0.1",
            srv.port(),
            "GET",
            "/fonts/missing.woff2",
            std::time::Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(status, 404);
        let nf = srv.take_not_found();
        // 越界 404 同样留痕(既拒绝又可观测;顺序 = 请求顺序)
        assert_eq!(
            nf,
            vec![
                "../../etc/passwd".to_string(),
                "fonts/missing.woff2".to_string()
            ]
        );
        assert!(asset_not_found_message(&nf[1]).contains("missing.woff2"));
        drop(srv);
        let _ = std::fs::remove_dir_all(&base);
    }
}
