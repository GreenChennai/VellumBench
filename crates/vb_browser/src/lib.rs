//! `vb_browser` — 车道 B(浏览器车道,ADR-0020)。
//!
//! Rust 原生 CDP 客户端(零新增外部依赖:std TCP 手写 HTTP/WebSocket,
//! serde_json 解析),驱动系统 Edge/Chrome headless 完成高保真导出:
//! - PNG:`Page.captureScreenshot`(captureBeyondViewport)+ WPI 捕获协议十要素
//! - PDF:`Page.printToPDF`(screen 媒体仿真 + 精确纸张 + 超长分页回退)
//! - AI:PDF 兼容流 + `%AI9_PrivateDataBegin` 头
//!
//! 直接渲染源 HTML(静态服务挂载),不走 Document 往返(19 篇 §4.2)。

pub mod b64;
pub mod browser;
pub mod capture;
pub mod cdp;
pub mod domsnap;
pub mod httpc;
pub mod page;
pub mod print;
pub mod staticsrv;
pub mod ws;

use std::path::{Path, PathBuf};

use serde_json::json;

pub use browser::{browser_version, discover_browser};
pub use capture::{capture_png, CaptureOptions, CaptureOutcome};
pub use print::{ai_from_pdf, print_pdf, PrintOutcome};

/// 车道 B 支持的格式(PNG/PDF/AI;其余格式仍走自研车道)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneFormat {
    Png,
    Pdf,
    Ai,
}

impl LaneFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "PNG" => Some(LaneFormat::Png),
            "PDF" => Some(LaneFormat::Pdf),
            "AI" => Some(LaneFormat::Ai),
            _ => None,
        }
    }
}

/// 车道 B 导出请求。
#[derive(Debug, Clone)]
pub struct LaneRequest {
    pub format: LaneFormat,
    /// 视口宽(CSS px)= 导出宽;0 = 自适应(取内容宽)。
    pub width: u32,
    /// 高度锁定(0 = 不限)。
    pub height: u32,
    /// 原生倍率 1..=8。
    pub scale: u32,
    /// 透明背景(仅 PNG)。
    pub transparent: bool,
}

/// 车道 B 导出结果(统一 JSON 用)。
pub struct LaneOutcome {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
    pub engine_hint: String,
}

/// 车道 B 一站式导出:静态服务挂载 → 浏览器进程 → 页面会话 → 采集/打印。
pub fn export_source(source: &Path, req: &LaneRequest) -> Result<LaneOutcome, String> {
    let Some(exe) = discover_browser(None) else {
        return Err("未发现系统浏览器(Edge/Chrome);浏览器车道不可用".into());
    };
    // 源挂载:目录 → 解析 index;文件 → 挂父目录(服务存活至函数返回)
    let _srv: Option<staticsrv::StaticServer>;
    let url = if source.is_dir() {
        let srv = staticsrv::StaticServer::start(source)?;
        let u = srv.url_for_dir()?;
        _srv = Some(srv);
        u
    } else {
        let parent = source
            .parent()
            .filter(|p| p.is_dir())
            .ok_or_else(|| format!("源文件缺少父目录: {}", source.display()))?;
        let srv = staticsrv::StaticServer::start(parent)?;
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("源文件名非法")?;
        let u = format!("http://127.0.0.1:{}/{}", srv.port(), encode(name));
        _srv = Some(srv);
        u
    };
    let proc = browser::BrowserProcess::launch(&exe)?;
    let engine_hint = proc.version();
    export_with_url(&proc, &url, req, &engine_hint)
}

fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn export_with_url(
    proc: &browser::BrowserProcess,
    url: &str,
    req: &LaneRequest,
    engine_hint: &str,
) -> Result<LaneOutcome, String> {
    let mut page = page::PageSession::attach(proc)?;
    // 要素 2/3:视口只定宽(高占位同宽,防 100vh 撑爆),DSF 原生倍率
    let init_w = if req.width == 0 { 1080 } else { req.width };
    page.set_device_metrics(init_w, init_w, req.scale.clamp(1, 8))?;
    page.navigate(url)?;
    page.wait_network_idle(std::time::Duration::from_secs(3));
    page.sleep(200);

    match req.format {
        LaneFormat::Png => {
            let opts = CaptureOptions {
                width: init_w,
                height_lock: (req.height > 0).then_some(req.height),
                scale: req.scale.clamp(1, 8),
                transparent: req.transparent,
            };
            let outcome = capture_png(&mut page, &opts)?;
            Ok(LaneOutcome {
                bytes: outcome.png,
                width: outcome.width,
                height: outcome.height,
                warnings: outcome.warnings,
                engine_hint: engine_hint.to_string(),
            })
        }
        LaneFormat::Pdf | LaneFormat::Ai => {
            let outcome = print_pdf(&mut page, init_w, (req.height > 0).then_some(req.height))?;
            let bytes = if req.format == LaneFormat::Ai {
                ai_from_pdf(outcome.pdf)
            } else {
                outcome.pdf
            };
            Ok(LaneOutcome {
                bytes,
                width: outcome.width_css,
                height: outcome.height_css,
                warnings: outcome.warnings,
                engine_hint: engine_hint.to_string(),
            })
        }
    }
}

fn exe_path() -> Result<PathBuf, String> {
    discover_browser(None).ok_or_else(|| "未发现系统浏览器".into())
}

/// JSON 单行结果(与 kiln-cli 输出风格一致)。
pub fn result_json(
    ok: bool,
    format: &str,
    path: &Path,
    width: u32,
    height: u32,
    warnings: usize,
    engine: &str,
    hint: &str,
    bytes: usize,
    ms: u128,
) -> String {
    let v = json!({
        "ok": ok,
        "format": format,
        "path": path.display().to_string(),
        "width": width,
        "height": height,
        "warnings": warnings,
        "engine": engine,
        "browser": hint,
        "bytes": bytes,
        "encode_ms": ms,
    });
    v.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_format_parse() {
        assert_eq!(LaneFormat::parse("png"), Some(LaneFormat::Png));
        assert_eq!(LaneFormat::parse("AI"), Some(LaneFormat::Ai));
        assert_eq!(LaneFormat::parse("gif"), None);
    }

    #[test]
    fn encode_path() {
        assert_eq!(encode("橙青色.html"), "%E6%A9%99%E9%9D%92%E8%89%B2.html");
        assert_eq!(encode("index.html"), "index.html");
    }
}
