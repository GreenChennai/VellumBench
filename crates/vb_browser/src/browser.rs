//! 系统浏览器发现与进程托管(ADR-0020:浏览器作为渲染后端)。

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::httpc;

/// 环境变量覆盖:显式浏览器可执行文件路径。
pub const ENV_BROWSER_PATH: &str = "VB_BROWSER_PATH";
/// 环境变量开关:GPU 光栅化(值 1/true/on 生效)。`--gpu` 显式参数优先。
pub const ENV_GPU: &str = "VB_GPU";

/// 浏览器启动附加选项。
#[derive(Debug, Clone, Copy, Default)]
pub struct LaunchOptions {
    /// GPU 光栅化:走 ANGLE→D3D11(NVIDIA/AMD/Intel 通用,驱动各自接手)。
    /// 关闭(默认)时 `--disable-gpu` 软件光栅 —— 跨机逐像素可复现
    /// (ADR-0022 口径);打开时光栅/合成落在显卡,MV 级长片提速明显,
    /// 代价是与软件光栅存在固定 AA 微差(MAD ≈ 1/255,肉眼无别)。
    pub gpu: bool,
}

/// GPU 开关判定:显式参数 > 环境变量(1/true/on)> 默认关。
pub fn gpu_requested(explicit: Option<bool>) -> bool {
    if let Some(v) = explicit {
        return v;
    }
    matches!(
        std::env::var(ENV_GPU).map(|v| v.to_ascii_lowercase()),
        Ok(ref v) if v == "1" || v == "true" || v == "on"
    )
}

const EDGE_PATHS: &[&str] = &[
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
];
const CHROME_PATHS: &[&str] = &[
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
];

/// 环境变量派生的候选路径:per-user 安装(`%LOCALAPPDATA%`)与
/// 自定义 Program Files 盘符。硬编码 C: 盘路径覆盖不了这两种常见形态。
fn env_browser_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        v.push(local.join(r"Google\Chrome\Application\chrome.exe"));
    }
    for key in ["PROGRAMFILES", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(key) {
            let pf = PathBuf::from(pf);
            v.push(pf.join(r"Google\Chrome\Application\chrome.exe"));
            v.push(pf.join(r"Microsoft\Edge\Application\msedge.exe"));
        }
    }
    v
}

/// 在 Playwright 缓存里找最新版 chrome-headless-shell(逐帧截屏比
/// headless=new 快 ~2×:老式 headless 直连软件通路,无 viz 表面中转)。
fn discover_headless_shell() -> Option<PathBuf> {
    if std::env::var("VB_NO_SHELL")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        return None;
    }
    let base = std::env::var_os("LOCALAPPDATA")?;
    let root = PathBuf::from(base).join("ms-playwright");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&root)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("chromium_headless_shell-"))
                .unwrap_or(false)
        })
        .collect();
    // 版本号目录取最大(字典序对同位数版本号即数值序)
    versions.sort();
    for dir in versions.iter().rev() {
        let exe = dir
            .join("chrome-headless-shell-win64")
            .join("chrome-headless-shell.exe");
        if exe.is_file() {
            return Some(exe);
        }
    }
    None
}

/// 发现系统浏览器:显式参数 → `VB_BROWSER_PATH` → **系统 Edge → Chrome** →
/// chrome-headless-shell(仅 `VB_PREFER_SHELL=1` 或系统里一个浏览器都没有)。
///
/// 系统浏览器优先的理由(下游实测教训):环境里来路不明的旧版
/// headless-shell(如 Playwright 缓存的 HeadlessChrome/153)会让
/// captureScreenshot 帧级挂死,整条浏览器车道静默降级——正确性优先于
/// shell 的 ~2× 截屏速度;确认自己缓存的 shell 可用后设
/// `VB_PREFER_SHELL=1` 可拿回速度。
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
    let system = EDGE_PATHS
        .iter()
        .chain(CHROME_PATHS.iter())
        .map(Path::new)
        .chain(env_browser_paths().iter().map(Path::new))
        .find(|p| p.is_file())
        .map(Path::to_path_buf);
    let prefer_shell = std::env::var("VB_PREFER_SHELL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if prefer_shell {
        if let Some(shell) = discover_headless_shell() {
            return Some(shell);
        }
    }
    system.or_else(|| {
        if prefer_shell {
            None
        } else {
            discover_headless_shell()
        }
    })
}

/// 浏览器版本号(报告环境指纹用)。
/// **不可**用 `msedge --version` 子进程探测:Edge 该参数不退出,会永久挂起。
/// 改从已启动实例的 DevTools /json/version 读取。
pub fn browser_version(exe: &Path) -> String {
    exe.display().to_string()
}

/// 从 `<user-data-dir>/DevToolsActivePort` 读内核自选端口。
///
/// 这是跨平台稳妥回退:Windows 版 msedge headless 把端点**只写文件、
/// 不打印 stderr 行**(实测),只解析 stderr 的实现在 Edge 上必然失败。
/// 文件首行 = 端口,次行 = browser websocket 路径(本模块只需端口)。
fn read_active_port(user_data_dir: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(user_data_dir.join("DevToolsActivePort")).ok()?;
    text.lines().next()?.trim().parse::<u16>().ok()
}

/// 从 "DevTools listening on ws://127.0.0.1:PORT/…" 行中取端口。
fn port_from_listen_line(line: &str) -> Option<u16> {
    let idx = line.find("DevTools listening on ws://")?;
    let rest = &line[idx + "DevTools listening on ws://".len()..];
    let hostport = rest
        .trim()
        .trim_start_matches("ws://")
        .trim_end_matches('/');
    let hostport = hostport.split_once('/').map(|(h, _)| h).unwrap_or(hostport);
    hostport
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse::<u16>().ok())
}

/// chrome-headless-shell(Playwright 同款老式 headless 实现)探测:
/// 截图走直连软件通路,空闲页面每帧 ~300ms;headless=new 的 viz 表面
/// 要 ~530ms。二进制名含 headless-shell / headless_shell 即认定为 shell。
fn is_headless_shell(exe: &Path) -> bool {
    exe.file_name()
        .and_then(|n| n.to_str())
        .map(|n| {
            let l = n.to_ascii_lowercase();
            l.contains("headless-shell")
                || l.contains("headless_shell")
                || l == "chrome-headless-shell.exe"
        })
        .unwrap_or(false)
}

/// 与 playwright headless 对齐的渲染相关默认参数。
fn launch_args(user_data_dir: &Path, debug_port: u16, gpu: bool, shell: bool) -> Vec<String> {
    let mut v = vec![
        format!("--remote-debugging-port={debug_port}"),
        format!("--user-data-dir={}", user_data_dir.display()),
    ];
    // shell 二进制本身就是 headless,不认 --headless=new
    if !shell {
        v.push("--headless=new".into());
    }
    v.extend([
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
        // Playwright 同款:禁 LCD 次像素文本 AA,省一遍文本光栅滤波
        // (文本密集页可感知),也与无障碍/截图口径对齐
        "--disable-lcd-text".into(),
    ]);
    if gpu {
        // GPU 光栅化:headless 必须显式走 ANGLE→D3D11,否则仍是软件光栅。
        // 实测渲染器串形如 "ANGLE (AMD, AMD Radeon RX 9070 GRE … D3D11)",
        // NVIDIA/AMD/Intel 均由各自驱动接手;CanvasOopRasterization 让
        // canvas2d 位图光栅化也进 GPU 进程。
        v.extend([
            "--use-gl=angle".into(),
            "--use-angle=d3d11".into(),
            "--enable-gpu-rasterization".into(),
            "--ignore-gpu-blocklist".into(),
            "--enable-zero-copy".into(),
            "--disable-gpu-vsync".into(),
            "--enable-features=CanvasOopRasterization".into(),
        ]);
    } else {
        // 软件光栅:规避本机 GPU 驱动差异与崩溃(R4),跨机结果可复现
        v.push("--disable-gpu".into());
    }
    v.extend([
        // 逐帧截屏的生命线:headless=new 对"空闲页面"(无 CSS 动画、无 rAF,
        // 典型如 SEEK 驱动的确定性渲染页)上,Page.captureScreenshot 等合成器
        // 调度产下一帧,实测固定 ~530ms/帧且 --gpu 无改善。begin-frame-control
        // 让 CDP 显式发起 BeginFrame(HeadlessExperimental.beginFrame 产帧 +
        // 截图一步完成),配 frame-rate-limit/vsync 解除把每帧等待压到毫秒级。
        "--enable-begin-frame-control".into(),
        "--disable-frame-rate-limit".into(),
        "--disable-gpu-vsync".into(),
        "--run-all-compositor-stages-before-draw".into(),
        "--hide-scrollbars".into(),
        "--mute-audio".into(),
        "--password-store=basic".into(),
        "--use-mock-keychain".into(),
        "--no-service-autorun".into(),
    ]);
    v
}

/// 清扫 %TEMP% 里陈旧的 kiln-* 工作目录(浏览器 user-data-dir / 段视频 /
/// GIF·MP4 中转):进程崩溃、断电、watchdog 强杀时 Drop 不会执行,残留在
/// 下游机器上按几十个计。每进程首个浏览器启动时清扫一次;只动 mtime 超
/// 6 小时的目录(并发运行中的新目录绝不误删),失败静默(best-effort)。
fn sweep_stale_temp_dirs() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let cutoff = std::time::SystemTime::now() - Duration::from_secs(6 * 3600);
        // 前缀清单 = 全部 kiln 临时产物的落盘命名:browser/anim/mp4/gif 是
        // 各车道的工作目录;wc = WebCodecs 上传 sink;raster = dompaint
        // data:URI 落盘降级;paintlist = domexport 调试 dump(文件非目录)。
        // 新增落盘路径时必须同步这里,否则异常退出残留永不回收。
        const PREFIXES: [&str; 7] = [
            "kiln-browser-",
            "kiln-anim-",
            "kiln-mp4-",
            "kiln-gif-",
            "kiln-wc-",
            "kiln-raster-",
            "kiln-paintlist-",
        ];
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if !PREFIXES.iter().any(|p| name.starts_with(p)) {
                continue;
            }
            let fresh = entry
                .metadata()
                .and_then(|m| m.modified())
                .map(|t| t > cutoff)
                .unwrap_or(true); // 拿不到时间 = 当它是新的,宁留勿删
            if !fresh {
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
                    let _ = std::fs::remove_dir_all(entry.path());
                } else {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    });
}

/// 托管一个 headless 浏览器进程,提供打开新页面的能力。
pub struct BrowserProcess {
    pub exe: PathBuf,
    pub port: u16,
    child: Child,
    user_data_dir: PathBuf,
}

impl BrowserProcess {
    /// 启动浏览器并解析 DevTools 端点(软件光栅,默认口径)。
    pub fn launch(exe: &Path) -> Result<Self, String> {
        Self::launch_with(exe, LaunchOptions::default())
    }

    /// 按选项启动浏览器并解析 DevTools 端点。
    ///
    /// 双路取端口:① stderr 的 `DevTools listening on ws://…`(Chrome 打印);
    /// ② `<user-data-dir>/DevToolsActivePort` 文件(msedge headless 在 Windows
    /// 上只写文件不打印,只认 ① 的实现会整体失败)。
    pub fn launch_with(exe: &Path, opts: LaunchOptions) -> Result<Self, String> {
        sweep_stale_temp_dirs();
        let port = 0; // 由内核自选,stderr 回报
                      // 并发多实例(动画分段并行渲染)同 pid 同瞬间启动,nanos 会撞名;
                      // 进程级原子计数保证唯一
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let user_data_dir = std::env::temp_dir().join(format!(
            "kiln-browser-{}-{}-{seq}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&user_data_dir).map_err(|e| e.to_string())?;
        let shell = is_headless_shell(exe);
        let args = launch_args(&user_data_dir, port, opts.gpu, shell);
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
        // 两条取得端点的路:stderr 行(Chrome 会打印)与 DevToolsActivePort
        // 文件(msedge headless 只写文件)。逐 200ms 双路轮询,任一先到即用。
        let mut resolved = None;
        while std::time::Instant::now() < deadline {
            if let Some(p) = read_active_port(&user_data_dir) {
                resolved = Some(p);
                break;
            }
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(line) => {
                    if let Some(p) = port_from_listen_line(&line) {
                        resolved = Some(p);
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                // stderr 已关闭不代表失败:继续按文件路等到 deadline
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(Duration::from_millis(200));
                    if let Some(p) = read_active_port(&user_data_dir) {
                        resolved = Some(p);
                    }
                    break;
                }
            }
        }
        let Some(resolved) = resolved else {
            let _ = child.kill();
            let _ = child.wait();
            // 此时尚未构造 BrowserProcess,Drop 不会执行,须手动清理
            let _ = std::fs::remove_dir_all(&user_data_dir);
            return Err(format!(
                "未捕获 DevTools 端点:{} 既未在 stderr 打印 DevTools 行,\
                 也未写出 {}/DevToolsActivePort。可设环境变量 VB_BROWSER_PATH \
                 指定 Chrome/Edge 可执行文件后重试",
                exe.display(),
                user_data_dir.display()
            ));
        };
        Ok(BrowserProcess {
            exe: exe.to_path_buf(),
            port: resolved,
            child,
            user_data_dir,
        })
    }

    /// 浏览器版本(DevTools /json/version 的 Browser 字段,如 "Edg/131.0.2903.86")。
    pub fn version(&self) -> String {
        // 多实例并发冷启动时 DevTools HTTP 响应显著变慢(下游实测 w4
        // "HTTP 读取超时"):5s → 15s + 一次重试
        let req = || {
            httpc::request(
                "127.0.0.1",
                self.port,
                "GET",
                "/json/version",
                Duration::from_secs(15),
            )
        };
        match req().or_else(|_| req()) {
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
                // 并发冷启动时 /json/new 可慢到秒级×多:30s 防误杀
                Duration::from_secs(30),
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
            // 已自行退出的浏览器不再 taskkill:child.id() 返回的 PID 可能
            // 已被 OS 复用给无关进程,/T /F 会强杀别人的进程树
            let already_exited = matches!(self.child.try_wait(), Ok(Some(_)));
            if !already_exited {
                let pid = self.child.id();
                let _ = Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
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
