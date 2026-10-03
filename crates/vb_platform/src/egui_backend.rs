//! egui 宿主实现(feature `egui`;vb_app 用)。
//!
//! 最薄封装:egui 能直接做的(光标/视口命令/主题)包一层 trait;egui 0.35
//! 不暴露的**系统剪贴板读**走 arboard(eframe 同款依赖,树内已有)。
//! 文件对话框是 OS 服务,复用 vb_app 现用的 rfd——不是 egui 特性,但
//! 归口到同一 seam,面板层不再各自 import rfd。
//!
//! 全部适配器持有 `egui::Context`(rc 克隆,廉价),可在 UI 闭包外构造。

use crate::error::PlatformError;
use crate::traits::{
    Clipboard, DarkModeProbe, DisplayInfo, FileDialog, SystemCursor, WindowHandle,
};
use crate::types::{CursorShape, DisplayMetrics, FileFilter};
use std::path::PathBuf;

/// [`WindowHandle`] 的 egui 实现:视口命令通道(`ViewportCommand`)。
#[derive(Debug, Clone)]
pub struct EguiWindow {
    ctx: egui::Context,
    viewport: egui::ViewportId,
}

impl EguiWindow {
    pub fn new(ctx: egui::Context) -> Self {
        EguiWindow {
            viewport: egui::ViewportId::ROOT,
            ctx,
        }
    }

    pub fn with_viewport(ctx: egui::Context, viewport: egui::ViewportId) -> Self {
        EguiWindow { ctx, viewport }
    }
}

impl WindowHandle for EguiWindow {
    fn id(&self) -> u64 {
        self.viewport.0.value()
    }

    fn set_title(&mut self, title: &str) -> Result<(), PlatformError> {
        self.ctx.send_viewport_cmd_to(
            self.viewport,
            egui::ViewportCommand::Title(title.to_owned()),
        );
        Ok(())
    }

    fn activate(&mut self) -> Result<(), PlatformError> {
        self.ctx
            .send_viewport_cmd_to(self.viewport, egui::ViewportCommand::Focus);
        Ok(())
    }
}

/// [`Clipboard`] 写通道:egui `copy_text`(宿主集成为真 OS 剪贴板)。
///
/// egui 0.35 的 Context **不暴露系统剪贴板读**(粘贴经 RawInput 事件流),
/// 读取请用 [`OsClipboard`]——两件套各管一半,trait 不说谎。
#[derive(Debug, Clone)]
pub struct EguiClipboard {
    ctx: egui::Context,
}

impl EguiClipboard {
    pub fn new(ctx: egui::Context) -> Self {
        EguiClipboard { ctx }
    }
}

impl Clipboard for EguiClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), PlatformError> {
        self.ctx.copy_text(text.to_owned());
        Ok(())
    }

    fn text(&self) -> Result<Option<String>, PlatformError> {
        Err(PlatformError::unsupported(
            "Clipboard::text",
            "egui 0.35 命令通道只写不读;系统剪贴板读请用 vb_platform::egui_backend::OsClipboard",
        ))
    }
}

/// [`Clipboard`] 的 arboard 实现:真 OS 剪贴板(读+写,eframe 同源依赖)。
///
/// arboard 3.6 的 `get_text` 需要 `&mut`,而 trait 读口是 `&self`——
/// 内部用 `RefCell`(OS 剪贴板本就是进程级单例资源,不做跨线程共享)。
pub struct OsClipboard {
    board: std::cell::RefCell<arboard::Clipboard>,
}

impl OsClipboard {
    /// 打开 OS 剪贴板句柄(Windows 剪贴板是有锁资源,短用短弃)。
    pub fn open() -> Result<Self, PlatformError> {
        arboard::Clipboard::new()
            .map(|board| OsClipboard {
                board: std::cell::RefCell::new(board),
            })
            .map_err(|e| PlatformError::failed("Clipboard::open", e.to_string()))
    }
}

impl Clipboard for OsClipboard {
    fn set_text(&mut self, text: &str) -> Result<(), PlatformError> {
        self.board
            .borrow_mut()
            .set_text(text.to_owned())
            .map_err(|e| PlatformError::failed("Clipboard::set_text", e.to_string()))
    }

    fn text(&self) -> Result<Option<String>, PlatformError> {
        match self.board.borrow_mut().get_text() {
            Ok(t) => Ok(Some(t)),
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(PlatformError::failed("Clipboard::text", e.to_string())),
        }
    }
}

/// [`SystemCursor`] 的 egui 实现(`set_cursor_icon`)。
#[derive(Debug, Clone)]
pub struct EguiCursor {
    ctx: egui::Context,
}

impl EguiCursor {
    pub fn new(ctx: egui::Context) -> Self {
        EguiCursor { ctx }
    }
}

impl SystemCursor for EguiCursor {
    fn set_shape(&mut self, shape: CursorShape) -> Result<(), PlatformError> {
        self.ctx.set_cursor_icon(map_cursor(shape));
        Ok(())
    }
}

/// [`DisplayInfo`] 的 egui 实现:当前 viewport 所在屏(单条)。
///
/// 诚实边界:egui 0.35 只暴露**当前 viewport** 的 `ViewportInfo`,拿不到
/// 全显示器拓扑——多屏枚举是 R1 接 winit 后的事;`displays()` 返回一条,
/// 缺失字段用中性值兜底(scale=1.0)并如实留 `position: None`。
#[derive(Debug, Clone)]
pub struct EguiDisplayInfo {
    ctx: egui::Context,
}

impl EguiDisplayInfo {
    pub fn new(ctx: egui::Context) -> Self {
        EguiDisplayInfo { ctx }
    }
}

impl DisplayInfo for EguiDisplayInfo {
    fn displays(&self) -> Vec<DisplayMetrics> {
        let info = self.ctx.input(|i| i.viewport().clone());
        let scale = info
            .native_pixels_per_point
            .map(f64::from)
            .unwrap_or(1.0)
            .max(0.1);
        let size = info
            .monitor_size
            .map(|v| (v.x.max(0.0) as u32, v.y.max(0.0) as u32))
            .unwrap_or((0, 0));
        let position = info.inner_rect.map(|r| (r.min.x as i32, r.min.y as i32));
        vec![DisplayMetrics {
            id: egui::ViewportId::ROOT.0.value(),
            scale_factor: scale,
            size_px: size,
            position,
            // egui 侧唯一已知屏 = 当前 viewport 所在屏,主屏语义由宿主集成
            // (eframe/winit)保证;此处如实标注 true
            is_primary: true,
        }]
    }
}

/// [`DarkModeProbe`] 的 egui 实现:当前 style 解析结果。
#[derive(Debug, Clone)]
pub struct EguiDarkMode {
    ctx: egui::Context,
}

impl EguiDarkMode {
    pub fn new(ctx: egui::Context) -> Self {
        EguiDarkMode { ctx }
    }
}

impl DarkModeProbe for EguiDarkMode {
    fn is_dark_mode(&self) -> bool {
        // 0.35 无 ctx.style() 全局读口;theme() 返回当前解析后的主题档
        // (set_theme / 宿主集成同步的结果)
        self.ctx.theme() == egui::Theme::Dark
    }
}

/// [`FileDialog`] 的 rfd 实现(egui/gpui 两宿主共用;阻塞式,与 vb_app
/// 现有调用同型——R0 不改调用点,面板层经 trait 间接使用)。
#[derive(Debug, Default, Clone, Copy)]
pub struct RfdDialog;

impl RfdDialog {
    pub fn new() -> Self {
        RfdDialog
    }
}

impl FileDialog for RfdDialog {
    fn pick_folder(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    fn pick_file(&mut self, title: &str, filters: &[FileFilter]) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new().set_title(title);
        for f in filters {
            d = d.add_filter(f.name, f.extensions);
        }
        d.pick_file()
    }

    fn save_file(
        &mut self,
        title: &str,
        default_name: &str,
        filters: &[FileFilter],
    ) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(default_name);
        for f in filters {
            d = d.add_filter(f.name, f.extensions);
        }
        d.save_file()
    }
}

/// CursorShape → egui CursorIcon(全量映射,无缺失项)。
pub fn map_cursor(shape: CursorShape) -> egui::CursorIcon {
    match shape {
        CursorShape::Default => egui::CursorIcon::Default,
        CursorShape::Pointer => egui::CursorIcon::PointingHand,
        CursorShape::Crosshair => egui::CursorIcon::Crosshair,
        CursorShape::Text => egui::CursorIcon::Text,
        CursorShape::Grab => egui::CursorIcon::Grab,
        CursorShape::Grabbing => egui::CursorIcon::Grabbing,
        CursorShape::Move => egui::CursorIcon::Move,
        CursorShape::Wait => egui::CursorIcon::Wait,
        CursorShape::NotAllowed => egui::CursorIcon::NotAllowed,
        CursorShape::ZoomIn => egui::CursorIcon::ZoomIn,
        CursorShape::ZoomOut => egui::CursorIcon::ZoomOut,
        CursorShape::Cell => egui::CursorIcon::Cell,
        CursorShape::ResizeEw => egui::CursorIcon::ResizeHorizontal,
        CursorShape::ResizeNs => egui::CursorIcon::ResizeVertical,
        CursorShape::ResizeNwSe => egui::CursorIcon::ResizeNwSe,
        CursorShape::ResizeNeSw => egui::CursorIcon::ResizeNeSw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// headless Context:写剪贴板 → 帧输出带 CopyText 命令(集成层执行)。
    #[test]
    fn clipboard_write_becomes_copy_text_command() {
        let ctx = egui::Context::default();
        let mut clip = EguiClipboard::new(ctx.clone());
        clip.set_text("D:\\proj").unwrap();
        let out = ctx.end_pass();
        assert!(out.platform_output.commands.iter().any(|c| matches!(
            c,
            egui::OutputCommand::CopyText(t) if t == "D:\\proj"
        )));
        // 读:诚实 Unsupported,不是 None(语义可区分)
        let err = clip.text().unwrap_err();
        assert!(err.is_unsupported());
    }

    /// headless Context:光标随工具切换(画布平移 → Grab)。
    #[test]
    fn cursor_shape_maps_to_egui_icon() {
        let ctx = egui::Context::default();
        let mut cursor = EguiCursor::new(ctx.clone());
        cursor.set_shape(CursorShape::Grabbing).unwrap();
        let out = ctx.end_pass();
        assert_eq!(out.platform_output.cursor_icon, egui::CursorIcon::Grabbing);
    }

    /// 主题初值:深色探测跟随当前 style 解析结果。
    #[test]
    fn dark_mode_follows_theme() {
        let ctx = egui::Context::default();
        let probe = EguiDarkMode::new(ctx.clone());
        ctx.set_theme(egui::Theme::Dark);
        assert!(probe.is_dark_mode(), "Dark 主题下 is_dark_mode 必须为 true");
    }

    /// 窗口:ROOT 视口 id + 标题/聚焦命令可发(headless 缓冲不崩)。
    #[test]
    fn window_title_and_focus_commands_are_accepted() {
        let ctx = egui::Context::default();
        let mut w = EguiWindow::new(ctx.clone());
        // ROOT 视口的 Id::NULL → u64::MAX(egui 语义,非窗口计数)
        assert_eq!(w.id(), egui::ViewportId::ROOT.0.value());
        w.set_title("启动器").unwrap();
        w.activate().unwrap();
        let out = ctx.end_pass();
        let cmds = &out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("ROOT 视口必有输出")
            .commands;
        assert!(
            cmds.iter()
                .any(|c| matches!(c, egui::ViewportCommand::Title(t) if t == "启动器")),
            "标题应成为视口命令:{cmds:?}"
        );
        assert!(
            cmds.iter()
                .any(|c| matches!(c, egui::ViewportCommand::Focus)),
            "聚焦应成为视口命令:{cmds:?}"
        );
    }

    /// 显示器:headless 无集成数据 → 中性兜底单条(scale=1.0,不 panic)。
    #[test]
    fn display_info_degrades_to_neutral_entry_headless() {
        let ctx = egui::Context::default();
        let di = EguiDisplayInfo::new(ctx);
        let ds = di.displays();
        assert_eq!(ds.len(), 1, "egui 宿主诚实返回当前 viewport 一条");
        assert!((ds[0].scale_factor - 1.0).abs() < f64::EPSILON);
    }
}
