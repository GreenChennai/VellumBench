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

/// 默认 WPI 仓库路径(本机约定;可用 `VB_WPI_DIR` 环境变量覆盖)。
pub const DEFAULT_WPI_DIR: &str = r"E:\平日资料\GitHub\WPI";

/// 解析 WPI 仓库目录:`VB_WPI_DIR` 环境变量优先,其次本机历史约定路径。
/// 都不可用时返回 None(开源环境下不再默认依赖单机绝对路径)。
pub fn resolve_wpi_dir() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("VB_WPI_DIR") {
        let p = PathBuf::from(v);
        if wpi_available(&p) {
            return Some(p);
        }
    }
    let fallback = PathBuf::from(DEFAULT_WPI_DIR);
    if wpi_available(&fallback) {
        Some(fallback)
    } else {
        None
    }
}

/// 检测 WPI 可用性(cli.py 存在即认为可用)。
pub fn wpi_available(wpi_dir: &Path) -> bool {
    wpi_dir.join("src").join("cli.py").is_file()
}

/// 浏览器引擎导出:文档 → 临时 HTML 目录(含项目资产)→ WPI CLI → 输出文件。
pub fn export_via_wpi(
    doc: &Document,
    project_dir: &Path,
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
        // 此前临时目录只有 HTML/CSS,<img> 全部 404,PDF/GIF/MP4 缺图
        copy_project_extras(project_dir, &tmp).map_err(|e| format!("项目资产复制失败:{e}"))?;

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

/// 把项目里浏览器需要的静态文件复制进临时目录:`assets/` 整棵 +
/// 顶层散文件(favicon 等)。`index.html` 与 `styles/` 由 write_project
/// 重新生成,不得用旧文件覆盖。
fn copy_project_extras(project_dir: &Path, tmp: &Path) -> Result<(), String> {
    fn copy_tree(src: &Path, dst: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let to = dst.join(entry.file_name());
            if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                copy_tree(&entry.path(), &to)?;
            } else {
                std::fs::copy(entry.path(), &to).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    let assets = project_dir.join("assets");
    if assets.is_dir() {
        copy_tree(&assets, &tmp.join("assets"))?;
    }
    for entry in std::fs::read_dir(project_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        if entry.file_name() == "index.html" {
            continue;
        }
        std::fs::copy(entry.path(), tmp.join(entry.file_name())).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W7:assets/ 与顶层散文件要进临时目录;index.html 不得被旧文件覆盖。
    #[test]
    fn copy_project_extras_copies_assets_but_not_source_html() {
        let base = std::env::temp_dir().join(format!("vb-wpi-src-{}", std::process::id()));
        let tmp = std::env::temp_dir().join(format!("vb-wpi-tmp-{}", std::process::id()));
        let _ = (
            std::fs::remove_dir_all(&base),
            std::fs::remove_dir_all(&tmp),
        );
        std::fs::create_dir_all(base.join("assets")).unwrap();
        std::fs::create_dir_all(base.join("styles")).unwrap();
        std::fs::write(base.join("index.html"), "<html>old</html>").unwrap();
        std::fs::write(base.join("styles").join("main.css"), "body{}").unwrap();
        std::fs::write(base.join("assets").join("logo.png"), b"\x89PNG").unwrap();
        std::fs::write(base.join("favicon.ico"), b"ico").unwrap();
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("index.html"), "<html>new</html>").unwrap();

        copy_project_extras(&base, &tmp).expect("复制成功");

        assert_eq!(
            std::fs::read(tmp.join("assets").join("logo.png")).unwrap(),
            b"\x89PNG",
            "assets 应被复制"
        );
        assert!(tmp.join("favicon.ico").is_file(), "顶层散文件应被复制");
        assert!(
            !tmp.join("styles").exists(),
            "styles/ 由 write_project 生成,不得复制旧文件"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.join("index.html")).unwrap(),
            "<html>new</html>",
            "旧 index.html 不得覆盖新渲染"
        );
        let _ = (
            std::fs::remove_dir_all(&base),
            std::fs::remove_dir_all(&tmp),
        );
    }
}
