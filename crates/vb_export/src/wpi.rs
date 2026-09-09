//! WPI 浏览器引擎桥(设计文档 07 篇 §四阶段 A,ADR-0005)。
//!
//! 进程调用 WPI CLI(`python src/cli.py`),零改动 WPI:
//! `--source <dir|html|url> --output <file> --format PNG|GIF|MP4|PDF
//!  --width N --scale 1|2|4|8 --transparent --max-wait S`
//!
//! 导出前把当前文档渲染为临时 HTML 目录(07 篇 §八),完成后清理。

use std::path::{Path, PathBuf};
use std::process::Command;

use vb_doc::export::write_project;
use vb_doc::model::Document;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WpiFormat {
    Png,
    Gif,
    Mp4,
    Pdf,
}

impl WpiFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            WpiFormat::Png => "PNG",
            WpiFormat::Gif => "GIF",
            WpiFormat::Mp4 => "MP4",
            WpiFormat::Pdf => "PDF",
        }
    }
}

pub struct WpiExportRequest {
    pub format: WpiFormat,
    /// 1 | 2 | 4 | 8(WPI 的 scale 档位)
    pub scale: u32,
    pub width: u32,
    pub transparent: bool,
    /// 输出文件(扩展名需匹配格式)
    pub out: PathBuf,
    /// 秒
    pub max_wait: f32,
}

pub struct WpiExportResult {
    pub out: PathBuf,
    pub warnings: Vec<String>,
}

/// 默认 WPI 仓库路径(本机约定;可在设置覆盖)。
pub const DEFAULT_WPI_DIR: &str = r"E:\平日资料\GitHub\WPI";

/// 检测 WPI 可用性(cli.py 存在即认为可用)。
pub fn wpi_available(wpi_dir: &Path) -> bool {
    wpi_dir.join("src").join("cli.py").is_file()
}

/// 浏览器引擎导出:文档 → 临时 HTML 目录 → WPI CLI → 输出文件。
pub fn export_via_wpi(
    doc: &Document,
    _project_dir: &Path,
    req: &WpiExportRequest,
    wpi_dir: &Path,
) -> Result<WpiExportResult, String> {
    if !wpi_available(wpi_dir) {
        return Err(format!(
            "WPI 不可用:未找到 {}(浏览器引擎导出需要 WPI;原生引擎仍可用)",
            wpi_dir.join("src").join("cli.py").display()
        ));
    }

    // 临时 HTML 目录(07 篇 §八:vsm-export-{pid}-{seq})
    let seq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_millis())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("vsm-export-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;

    let result = (|| -> Result<WpiExportResult, String> {
        write_project(doc, &tmp).map_err(|e| format!("临时 HTML 写出失败:{e}"))?;

        let mut warnings = Vec::new();
        let mut cmd = Command::new("python");
        cmd.current_dir(wpi_dir)
            .arg("src/cli.py")
            .arg("--source")
            .arg(&tmp)
            .arg("--output")
            .arg(&req.out)
            .arg("--format")
            .arg(req.format.as_str())
            .arg("--width")
            .arg(req.width.to_string())
            .arg("--scale")
            .arg(req.scale.to_string())
            .arg("--max-wait")
            .arg(req.max_wait.to_string());
        if req.transparent {
            cmd.arg("--transparent");
        }
        let output = cmd.output().map_err(|e| format!("WPI 进程启动失败:{e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        for line in stdout.lines().chain(stderr.lines()) {
            let t = line.trim();
            if t.starts_with('⚠') || t.to_lowercase().contains("warn") {
                warnings.push(t.to_string());
            }
        }
        if !output.status.success() {
            return Err(format!(
                "WPI 退出码 {:?}:{}",
                output.status.code(),
                if stderr.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    stderr.trim().to_string()
                }
            ));
        }
        if !req.out.is_file() {
            return Err("WPI 未产生输出文件".into());
        }
        Ok(WpiExportResult {
            out: req.out.clone(),
            warnings,
        })
    })();

    // 任务结束先清理残留再开下一个(既有约定)
    let _ = std::fs::remove_dir_all(&tmp);
    result
}
