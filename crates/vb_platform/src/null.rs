//! null 实现(恒可用):测试与 headless 的 fallback。
//!
//! 不是"永远什么都不做"的空壳——除 [`NullDialog`](用户取消语义)外,其余
//! 都以**内存态记录请求**,供断言与状态回读:`NullClipboard` 存文本,
//! `NullCursor` 记最后形状,`NullWindow` 记标题。全接口可走通 =
//! `tests/walkthrough.rs` 的真实用例层(复制路径/工具光标/DPI 预算/主题初值)。

use crate::error::PlatformError;
use crate::traits::{
    Clipboard, DarkModeProbe, DisplayInfo, FileDialog, SystemCursor, WindowHandle,
};
use crate::types::{CursorShape, DisplayMetrics, FileFilter};
use std::path::PathBuf;

/// 内存剪贴板:set_text 存、text 取(全宿主中唯一带读写的参考实现)。
#[derive(Debug, Default, Clone)]
pub struct NullClipboard {
    buf: Option<String>,
}

impl NullClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clipboard for NullClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), PlatformError> {
        self.buf = Some(text.to_owned());
        Ok(())
    }

    fn text(&self) -> Result<Option<String>, PlatformError> {
        Ok(self.buf.clone())
    }
}

/// 恒「用户取消」的对话框(headless 语义:不弹 OS UI)。
#[derive(Debug, Default, Clone, Copy)]
pub struct NullDialog;

impl NullDialog {
    pub fn new() -> Self {
        NullDialog
    }
}

impl FileDialog for NullDialog {
    fn pick_folder(&mut self) -> Option<PathBuf> {
        None
    }

    fn pick_file(&mut self, _title: &str, _filters: &[FileFilter]) -> Option<PathBuf> {
        None
    }

    fn save_file(
        &mut self,
        _title: &str,
        _default_name: &str,
        _filters: &[FileFilter],
    ) -> Option<PathBuf> {
        None
    }
}

/// 记录最后请求的光标(断言「工具切换 → 光标随动」用)。
#[derive(Debug, Default, Clone)]
pub struct NullCursor {
    last: Option<CursorShape>,
}

impl NullCursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// 最后一次设置的形状(未设置过为 None)。
    pub fn last(&self) -> Option<CursorShape> {
        self.last
    }
}

impl SystemCursor for NullCursor {
    fn set_shape(&mut self, shape: CursorShape) -> Result<(), PlatformError> {
        self.last = Some(shape);
        Ok(())
    }
}

/// 记录标题的窗口(id 自定)。
#[derive(Debug, Clone)]
pub struct NullWindow {
    id: u64,
    title: String,
}

impl NullWindow {
    pub fn new(id: u64) -> Self {
        NullWindow {
            id,
            title: String::new(),
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

impl WindowHandle for NullWindow {
    fn id(&self) -> u64 {
        self.id
    }

    fn set_title(&mut self, title: &str) -> Result<(), PlatformError> {
        self.title = title.to_owned();
        Ok(())
    }

    fn activate(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
}

/// 可配置显示器表(headless 测试自定拓扑:如 1.0 主屏 + 1.5 副屏)。
#[derive(Debug, Default, Clone)]
pub struct NullDisplay {
    metrics: Vec<DisplayMetrics>,
}

impl NullDisplay {
    pub fn new(metrics: Vec<DisplayMetrics>) -> Self {
        NullDisplay { metrics }
    }
}

impl DisplayInfo for NullDisplay {
    fn displays(&self) -> Vec<DisplayMetrics> {
        self.metrics.clone()
    }
}

/// 可配置深色态。
#[derive(Debug, Default, Clone, Copy)]
pub struct NullDarkMode {
    dark: bool,
}

impl NullDarkMode {
    pub fn new(dark: bool) -> Self {
        NullDarkMode { dark }
    }
}

impl DarkModeProbe for NullDarkMode {
    fn is_dark_mode(&self) -> bool {
        self.dark
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_roundtrip_in_memory() {
        let mut c = NullClipboard::new();
        assert_eq!(c.text().unwrap(), None);
        c.set_text("路径").unwrap();
        assert_eq!(c.text().unwrap().as_deref(), Some("路径"));
    }

    #[test]
    fn cursor_records_last_shape() {
        let mut c = NullCursor::new();
        assert_eq!(c.last(), None);
        c.set_shape(CursorShape::Grab).unwrap();
        assert_eq!(c.last(), Some(CursorShape::Grab));
    }

    #[test]
    fn window_records_title() {
        let mut w = NullWindow::new(7);
        assert_eq!(w.id(), 7);
        w.set_title("项目").unwrap();
        assert_eq!(w.title(), "项目");
    }

    #[test]
    fn dialog_is_always_cancelled_display_is_configurable() {
        let mut d = NullDialog::new();
        assert_eq!(d.pick_folder(), None);
        let probe = NullDarkMode::new(true);
        assert!(probe.is_dark_mode());
        let empty = NullDisplay::default();
        assert!(empty.displays().is_empty());
    }
}
