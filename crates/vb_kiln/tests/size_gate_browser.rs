//! P0-3 尺寸门回归(08a 探针 E 判据):kiln-cli 默认/auto 车道对画板声明
//! 尺寸的取景必须严格相等(物理像素 / PDF 页尺寸 = 画板声明)。
//!
//! 结构:
//! - 纯单测:`abprobe` 探测与视口计划(无浏览器依赖,任何环境必跑);
//! - e2e(真机):驱动 `kiln-cli`(CARGO_BIN_EXE,即真实 CLI 参数面与
//!   auto 择道),程序化读取 PNG IHDR 头与 PDF MediaBox——**不采信 JSON
//!   自报尺寸**;无系统浏览器(Edge/Chrome)的环境诚实跳过,不挂 CI。
//!
//! 夹具:`tests/fixtures/p03_*`(无标记 1920×1080 / 显式标记 750×1334 /
//! 无声明流式)。

use std::path::{Path, PathBuf};
use std::process::Command;

use vb_kiln::abprobe::{probe_artboard, viewport_plan};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// CDP 探活:无系统浏览器时 e2e 跳过(不因环境缺浏览器挂 CI)。
fn browser_available() -> bool {
    vb_browser::discover_browser(None).is_some()
}

/// PNG 头尺寸(IHDR,恒为首块):宽在 16..20、高在 20..24(大端)。
fn png_size(path: &Path) -> (u32, u32) {
    let d = std::fs::read(path).expect("PNG 读取失败");
    assert_eq!(&d[..8], b"\x89PNG\r\n\x1a\n", "不是 PNG: {path:?}");
    (
        u32::from_be_bytes([d[16], d[17], d[18], d[19]]),
        u32::from_be_bytes([d[20], d[21], d[22], d[23]]),
    )
}

/// PDF 首页 MediaBox(`[0 0 w h]`,px-as-pt 口径,与 native 车道一致)。
fn pdf_mediabox(path: &Path) -> (f64, f64) {
    let d = std::fs::read(path).expect("PDF 读取失败");
    let needle = b"/MediaBox [0 0 ";
    let pos = d
        .windows(needle.len())
        .position(|w| w == *needle)
        .expect("PDF 缺少 MediaBox");
    let rest = &d[pos + needle.len()..];
    let end = rest
        .iter()
        .position(|&b| b == b']')
        .expect("MediaBox 未闭合");
    let s = std::str::from_utf8(&rest[..end]).expect("MediaBox 非UTF-8");
    let mut it = s.split_whitespace();
    let w: f64 = it.next().expect("MediaBox 缺宽").parse().expect("宽非数字");
    let h: f64 = it.next().expect("MediaBox 缺高").parse().expect("高非数字");
    (w, h)
}

fn run_kiln_export(source: &Path, output: &Path, format: &str) -> String {
    let r = Command::new(env!("CARGO_BIN_EXE_kiln-cli"))
        .args([
            "export",
            "--source",
            source.to_str().expect("源路径非UTF-8"),
            "--output",
            output.to_str().expect("输出路径非UTF-8"),
            "--format",
            format,
        ])
        .output()
        .expect("kiln-cli 启动失败");
    assert!(
        r.status.success(),
        "kiln-cli 退出码非 0:{}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8_lossy(&r.stdout).to_string()
}

// ---------------------------------------------------------------- 纯单测

#[test]
fn probe_untagged_poster_declares_1920x1080() {
    let p = probe_artboard(&fixture("p03_untagged")).expect("应识别出画板");
    assert_eq!((p.width, p.height), (Some(1920), Some(1080)));
    assert!(!p.explicit, "无标记 = 启发式识别,诚实降级");
}

#[test]
fn probe_tagged_artboard_declares_750x1334() {
    let p = probe_artboard(&fixture("p03_tagged")).expect("应识别出画板");
    assert_eq!((p.width, p.height), (Some(750), Some(1334)));
    assert!(p.explicit, "显式标记不降级");
}

#[test]
fn viewport_plan_without_declaration_keeps_1080_fallback() {
    let p = probe_artboard(&fixture("p03_nodecl")).expect("导入应成功");
    assert_eq!((p.width, p.height), (None, None));
    assert_eq!(
        viewport_plan(&fixture("p03_nodecl"), 0, 0),
        (1080, 1080, false)
    );
}

// ------------------------------------------------------------ e2e(真机)

#[test]
fn e2e_auto_pdf_untagged_1920x1080_mediabox_strict() {
    // 探针实锤:browser-dom 矢量道视口固定 1080,1920 宽内容被裁成 1080×1080
    if !browser_available() {
        eprintln!("[skip] 无系统浏览器(Edge/Chrome),CDP 用例跳过");
        return;
    }
    let out = std::env::temp_dir().join(format!("p03-e2e-{}.pdf", std::process::id()));
    let stdout = run_kiln_export(&fixture("p03_untagged"), &out, "PDF");
    let (w, h) = pdf_mediabox(&out);
    assert_eq!((w, h), (1920.0, 1080.0), "PDF 页尺寸必须与声明严格相等");
    assert!(
        stdout.contains("\"width\":1920") && stdout.contains("\"height\":1080"),
        "{stdout}"
    );
    // 缺显式标记:诚实降级标注(语义保留,但不再影响尺寸)
    assert!(stdout.contains("\"degraded_artboard\":true"), "{stdout}");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn e2e_auto_png_untagged_1920x1080_header_strict() {
    if !browser_available() {
        eprintln!("[skip] 无系统浏览器(Edge/Chrome),CDP 用例跳过");
        return;
    }
    let out = std::env::temp_dir().join(format!("p03-e2e-{}.png", std::process::id()));
    let stdout = run_kiln_export(&fixture("p03_untagged"), &out, "PNG");
    assert_eq!(png_size(&out), (1920, 1080), "PNG 头尺寸必须与声明严格相等");
    assert!(stdout.contains("\"engine\":\"browser\""), "{stdout}");
    assert!(stdout.contains("\"degraded_artboard\":true"), "{stdout}");
    let _ = std::fs::remove_file(&out);
}

#[test]
fn e2e_auto_png_tagged_750x1334_no_fullpage_stretch() {
    // 探针实锤:750×1334(带 body margin)曾被整页拉伸成 1080×1350
    if !browser_available() {
        eprintln!("[skip] 无系统浏览器(Edge/Chrome),CDP 用例跳过");
        return;
    }
    let out = std::env::temp_dir().join(format!("p03-e2e-tag-{}.png", std::process::id()));
    let stdout = run_kiln_export(&fixture("p03_tagged"), &out, "PNG");
    assert_eq!(png_size(&out), (750, 1334));
    assert!(stdout.contains("\"degraded_artboard\":false"), "{stdout}");
    let _ = std::fs::remove_file(&out);
}
