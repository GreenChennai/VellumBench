//! printToPDF 打印协议(WPI `pdf_exporter.py` 移植:screen 媒体、精确纸张、
//! 超长分页回退)+ AI 头注入。

use crate::capture;
use crate::page::PageSession;

const PX_PER_MM: f64 = 96.0 / 25.4;
/// 单页高度上限(CSS px;超过分页——与 WPI `MAX_PAGE_H_PX` 一致)。
const MAX_PAGE_H_PX: u32 = 2400;

pub struct PrintOutcome {
    pub pdf: Vec<u8>,
    pub width_css: u32,
    pub height_css: u32,
    /// 分页页数(单页 = 1)。
    pub pages: u32,
    pub warnings: Vec<String>,
}

fn px_to_mm(px: f64) -> f64 {
    px / PX_PER_MM
}

/// 整页打印(先 settle,与 PNG 同一收敛协议)。
pub fn print_pdf(
    page: &mut PageSession,
    width: u32,
    height_lock: Option<u32>,
) -> Result<PrintOutcome, String> {
    page.emulate_media_screen()?;
    let mut warnings = page.collect_resource_warnings();
    let infinite = capture::settle(page)?;
    if infinite > 0 {
        warnings.push(format!("存在 {infinite} 个无限循环动画,画面可能非终态"));
    }
    let (sw, sh) = capture::content_size_value(page)?;
    let out_w = width.max(1).max(sw.min(width)); // 宽 = 视口宽(与 WPI 一致)
    let out_h = height_lock.map(|l| l.min(sh)).unwrap_or(sh);

    page.emulate_media_screen()?;
    if let Some(_lock) = height_lock {
        // 锁定高:打印视口高(内容不压缩,超出不导出)
        let w_mm = px_to_mm(out_w as f64);
        let h_mm = px_to_mm(out_h as f64) * 1.002 + 1.0;
        inject_page_margin(page)?;
        let pdf = page.print_to_pdf(w_mm / 25.4, h_mm / 25.4, false)?;
        return Ok(PrintOutcome {
            pdf,
            width_css: out_w,
            height_css: out_h,
            pages: 1,
            warnings,
        });
    }
    if out_h > MAX_PAGE_H_PX {
        // 超长:CSS @page 定宽定高分页(Edge/Chrome PDFium 对超大单页兼容差)
        let page_h_mm = px_to_mm(MAX_PAGE_H_PX as f64);
        let w_mm = px_to_mm(out_w as f64);
        inject_page_size(page, w_mm, page_h_mm)?;
        let pdf = page.print_to_pdf(w_mm / 25.4, page_h_mm / 25.4, true)?;
        let pages = (out_h + MAX_PAGE_H_PX - 1) / MAX_PAGE_H_PX;
        return Ok(PrintOutcome {
            pdf,
            width_css: out_w,
            height_css: out_h,
            pages,
            warnings,
        });
    }
    // 常规单页:纸张 = 内容尺寸(+0.2% +1mm 防尾白页,WPI 同款)
    let w_mm = px_to_mm(out_w as f64);
    let h_mm = px_to_mm(out_h as f64) * 1.002 + 1.0;
    inject_page_margin(page)?;
    let pdf = page.print_to_pdf(w_mm / 25.4, h_mm / 25.4, false)?;
    Ok(PrintOutcome {
        pdf,
        width_css: out_w,
        height_css: out_h,
        pages: 1,
        warnings,
    })
}

fn inject_style(page: &mut PageSession, css: &str) -> Result<(), String> {
    let escaped = css.replace('\\', "\\\\").replace('`', "\\`");
    page.evaluate(
        &format!(
            "(() => {{ const s = document.createElement('style'); s.textContent = `{escaped}`; document.head.appendChild(s); return true; }})()"
        ),
        false,
    )
    .map(|_| ())
}

fn inject_page_margin(page: &mut PageSession) -> Result<(), String> {
    inject_style(page, "@page { margin: 0; }")
}

fn inject_page_size(page: &mut PageSession, w_mm: f64, h_mm: f64) -> Result<(), String> {
    inject_style(
        page,
        &format!("@page {{ size: {w_mm:.2}mm {h_mm:.2}mm; margin: 0; }}"),
    )
}

/// PDF 兼容流 → Illustrator AI 头注入(复用 vb_kiln/postscript 的头格式)。
pub fn ai_from_pdf(mut pdf: Vec<u8>) -> Vec<u8> {
    const HEAD: &[u8] =
        b"%AI9_PrivateDataBegin\n%%AI8_CreatorVersion: 24.0.0\n%AI5_FileFormat 9.0\n";
    if pdf.starts_with(b"%PDF-") {
        if let Some(pos) = pdf.iter().position(|&b| b == b'\n') {
            let mut with_head = Vec::with_capacity(pdf.len() + HEAD.len());
            with_head.extend_from_slice(&pdf[..=pos]);
            with_head.extend_from_slice(HEAD);
            with_head.extend_from_slice(&pdf[pos + 1..]);
            pdf = with_head;
        }
    }
    pdf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_header_injected_after_pdf_line() {
        let mut pdf = b"%PDF-1.4\n%rest".to_vec();
        pdf.extend(vec![0u8; 10]);
        let ai = ai_from_pdf(pdf);
        let head = String::from_utf8_lossy(&ai[..60]);
        assert!(head.starts_with("%PDF-1.4\n"));
        assert!(head.contains("%AI9_PrivateDataBegin"));
        assert!(ai.ends_with(&[0u8, 0]));
    }

    #[test]
    fn ai_header_noop_on_garbage() {
        let ai = ai_from_pdf(b"not a pdf".to_vec());
        assert_eq!(ai, b"not a pdf");
    }
}
