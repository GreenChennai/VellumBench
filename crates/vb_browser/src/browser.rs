//! 系统浏览器发现与进程托管(ADR-0020:浏览器作为渲染后端)。

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::httpc;

/// 环境变量覆盖:显式浏览器可执行文件路径。
pub const ENV_BROWSER_PATH: &str = "VB_BROWSER_PATH";

const EDGE_PATHS: &[&str] = &[
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
];
const CHROME_PATHS: &[&str] = &[
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

/// 发现系统浏览器:显式参数 → 环境变量 → Edge → Chrome。
pub fn discover_browser(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Ok(v) = std::env::var(ENV_BROWSER_PATH) {
        let pb = PathBuf::from(v);
        if pb.is_file() {
            return Some(pb);
        }
    }
    EDGE_PATHS
        .iter()
        .chain(CHROME_PATHS.iter())
        .map(Path::new)
        .find(|p| p.is_file())
        .map(Path::to_path_buf)
}

/// 浏览器版本号(报告环境指纹用)。
/// **不可**用 `msedge --version` 子进程探测:Edge 该参数不退出,会永久挂起。
/// 改从已启动实例的 DevTools /json/version 读取。
pub fn browser_version(exe: &Path) -> String {
    exe.display().to_string()
}

/// 与 playwright headless 对齐的渲染相关默认参数。
fn launch_args(user_data_dir: &Path, debug_port: u16) -> Vec<String> {
    vec![
        format!("--remote-debugging-port={debug_port}"),
        format!("--user-data-dir={}", user_data_dir.display()),
        "--headless=new".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-dev-shm-usage".into(),
        "--disable-background-networking".into(),
        "--disable-background-timer-throttling".into(),
        "--disable-backgrounding-occluded-windows".into(),
        "--disable-breakpad".into(),
        "--disable-renderer-backgrounding".into(),
        "--disable-hang-monitor".into(),
        "--disable-ipc-flooding-protection".into(),
        "--force-color-profile=srgb".into(),
        // 软件光栅:规避本机 GPU 驱动差异与崩溃(R4),跨机结果可复现
        "--disable-gpu".into(),
        "--hide-scrollbars".into(),
        "--mute-audio".into(),
        "--password-store=basic".into(),
        "--use-mock-keychain".into(),
        "--no-service-autorun".into(),
    ]
}

/// 托管一个 headless 浏览器进程,提供打开新页面的能力。
pub struct BrowserProcess {
    pub exe: PathBuf,
    pub port: u16,
    child: Child,
    user_data_dir: PathBuf,
}

impl BrowserProcess {
    /// 启动浏览器并解析 DevTools 端点(解析 stderr 的 "DevTools listening on ws://")。
    pub fn launch(exe: &Path) -> Result<Self, String> {
        let port = 0; // 由内核自选,stderr 回报
        let user_data_dir = std::env::temp_dir().join(format!(
            "kiln-browser-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&user_data_dir).map_err(|e| e.to_string())?;
        let args = launch_args(&user_data_dir, port);
        let mut child = Command::new(exe)
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("浏览器进程启动失败({}): {e}", exe.display()))?;
        let stderr = child.stderr.take().ok_or("无法读取浏览器 stderr")?;
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        // stderr 阻塞读放到后台线程,主线程带超时收(channel);否则浏览器
        // 不吐行时 read_line 永久阻塞,deadline 形同虚设
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let mut ws_endpoint = None;
        while std::time::Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    if let Some(idx) = line.find("DevTools listening on ws://") {
                        ws_endpoint = Some(
                            line[idx + "DevTools listening on ws://".len()..]
                                .trim()
                                .to_string(),
                        );
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let Some(endpoint) = ws_endpoint else {
            let _ = child.kill();
            return Err(format!(
                "未捕获 DevTools 端点({} 可能不支持 --remote-debugging-port=0)",
                exe.display()
            ));
        };
        // ws://127.0.0.1:PORT/devtools/browser/UUID
        let rest = endpoint.trim_start_matches("ws://").trim_end_matches('/');
        let (hostport, _path) = rest.split_once('/').unwrap_or((rest, ""));
        let resolved = hostport
            .rsplit_once(':')
            .and_then(|(_, p)| p.parse::<u16>().ok())
            .ok_or_else(|| format!("DevTools 端口解析失败: {endpoint}"))?;
        Ok(BrowserProcess {
            exe: exe.to_path_buf(),
            port: resolved,
            child,
            user_data_dir,
        })
    }

    /// 浏览器版本(DevTools /json/version 的 Browser 字段,如 "Edg/131.0.2903.86")。
    pub fn version(&self) -> String {
        match httpc::request(
            "127.0.0.1",
            self.port,
            "GET",
            "/json/version",
            Duration::from_secs(5),
        ) {
            Ok((200, body)) => serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("Browser")
                        .and_then(Value::as_str)
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "unknown".into()),
            _ => "unknown".into(),
        }
    }

    /// 打开新标签页,返回其 webSocketDebuggerUrl 路径部分。
    pub fn new_tab(&self, about: &str) -> Result<String, String> {
        let target = format!("/json/new?{}", about);
        // Chrome 111+ 要求 PUT;旧内核只认 GET
        for method in ["PUT", "GET"] {
            match httpc::request(
                "127.0.0.1",
                self.port,
                method,
                &target,
                Duration::from_secs(5),
            ) {
                Ok((200, body)) => {
                    let v: Value = serde_json::from_slice(&body)
                        .map_err(|e| format!("标签页信息解析失败: {e}"))?;
                    let ws = v
                        .get("webSocketDebuggerUrl")
                        .and_then(Value::as_str)
                        .ok_or("缺少 webSocketDebuggerUrl")?;
                    let path = ws
                        .splitn(4, '/')
                        .nth(3)
                        .map(|p| format!("/{p}"))
                        .ok_or("webSocketDebuggerUrl 形态异常")?;
                    return Ok(path);
                }
                Ok((405, _)) => continue, // 换下一个方法重试
                Ok((status, body)) => {
                    return Err(format!(
                        "打开标签页失败 HTTP {status}: {}",
                        String::from_utf8_lossy(&body)
                    ));
                }
                Err(e) => return Err(e),
            }
        }
        Err("打开标签页失败:内核不接受 PUT/GET /json/new".into())
    }

    /// 终止整棵进程树(浏览器渲染器是子进程,直接 kill 父进程不彻底)。
    fn kill_tree(&mut self) {
        #[cfg(windows)]
        {
            let pid = self.child.id();
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        self.kill_tree();
        let _ = std::fs::remove_dir_all(&self.user_data_dir);
    }
}
