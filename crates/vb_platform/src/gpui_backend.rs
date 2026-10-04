//! gpui 宿主实现(feature `gpui`;vb_shell 用)。
//!
//! 经 `sable::gpui` 使用 gpui 0.2.2(版本与宿主同锁,22 篇 §3.6)——本模块
//! 不直接依赖 gpui crate,sable 升级即跟随。适配器为**借用型**:在 gpui 的
//! `update`/render 闭包内以 `&mut Window` / `&mut App` 现场构造,用完即弃
//! (gpui 的 cx 不可存储,这是宿主模型决定的用法)。
//!
//! 诚实降级边界(ADR-0046):gpui 0.2.2 缺失的能力(运行期改标题、
//! Wait/Zoom/Cell 光标、多显示器拓扑)返回 `PlatformError::Unsupported`,
//! 不静默假装成功;接新内核版本后在此收口。

use crate::error::PlatformError;
use crate::traits::{
    Clipboard, DarkModeProbe, DisplayInfo, FileDialog, MotionPreferenceProbe, SystemCursor,
    WindowHandle,
};
use crate::types::{CursorShape, DisplayMetrics, FileFilter};
use sable::gpui as gp;
use std::path::PathBuf;

/// [`WindowHandle`] 的 gpui 实现(借用型;update/render 闭包内构造)。
pub struct GpuiWindow<'a> {
    window: &'a mut gp::Window,
}

impl<'a> GpuiWindow<'a> {
    pub fn new(window: &'a mut gp::Window) -> Self {
        GpuiWindow { window }
    }
}

impl WindowHandle for GpuiWindow<'_> {
    fn id(&self) -> u64 {
        self.window.window_handle().window_id().as_u64()
    }

    fn set_title(&mut self, _title: &str) -> Result<(), PlatformError> {
        Err(PlatformError::unsupported(
            "WindowHandle::set_title",
            "gpui 0.2.2 标题仅开窗期 TitlebarOptions 可定,无运行期 API",
        ))
    }

    fn activate(&mut self) -> Result<(), PlatformError> {
        // gpui 0.2.2 的窗口前台化是 Window 自身方法(平台层 activate)
        self.window.activate_window();
        Ok(())
    }
}

/// [`Clipboard`] 的 gpui 实现(`App` 剪贴板通道)。
pub struct GpuiClipboard<'a> {
    cx: &'a mut gp::App,
}

impl<'a> GpuiClipboard<'a> {
    pub fn new(cx: &'a mut gp::App) -> Self {
        GpuiClipboard { cx }
    }
}

impl Clipboard for GpuiClipboard<'_> {
    fn set_text(&mut self, text: &str) -> Result<(), PlatformError> {
        self.cx
            .write_to_clipboard(gp::ClipboardItem::new_string(text.to_owned()));
        Ok(())
    }

    fn text(&self) -> Result<Option<String>, PlatformError> {
        Ok(self.cx.read_from_clipboard().and_then(|item| item.text()))
    }
}

/// [`SystemCursor`] 的 gpui 实现(`set_window_cursor_style`,窗口级)。
pub struct GpuiCursor<'a> {
    window: &'a mut gp::Window,
}

impl<'a> GpuiCursor<'a> {
    pub fn new(window: &'a mut gp::Window) -> Self {
        GpuiCursor { window }
    }
}

impl SystemCursor for GpuiCursor<'_> {
    fn set_shape(&mut self, shape: CursorShape) -> Result<(), PlatformError> {
        let style = map_cursor(shape).ok_or_else(|| {
            PlatformError::unsupported(
                "SystemCursor::set_shape",
                format!("gpui 0.2.2 无 {shape:?} 对应光标"),
            )
        })?;
        self.window.set_window_cursor_style(style);
        Ok(())
    }
}

/// [`DisplayInfo`] 的 gpui 实现:窗口所在屏(单条;scale/尺寸为已知字段)。
///
/// 诚实边界:gpui 0.2.2 无多显示器拓扑枚举 API,返回窗口当前所在屏一条。
pub struct GpuiDisplayInfo<'a> {
    window: &'a gp::Window,
}

impl<'a> GpuiDisplayInfo<'a> {
    pub fn new(window: &'a gp::Window) -> Self {
        GpuiDisplayInfo { window }
    }
}

impl DisplayInfo for GpuiDisplayInfo<'_> {
    fn displays(&self) -> Vec<DisplayMetrics> {
        let scale = f64::from(self.window.scale_factor());
        let size = self.window.viewport_size();
        let w = f32::from(size.width).max(0.0) as u32;
        let h = f32::from(size.height).max(0.0) as u32;
        vec![DisplayMetrics {
            id: self.window.window_handle().window_id().as_u64(),
            scale_factor: scale,
            size_px: (
                (w as f64 * scale).round() as u32,
                (h as f64 * scale).round() as u32,
            ),
            // gpui 0.2.2 拿不到窗口在虚拟桌面的原点:如实 None(Wayland 同款)
            position: None,
            // 唯一已知屏 = 窗口所在屏
            is_primary: true,
        }]
    }
}

/// [`DarkModeProbe`] 的 gpui 实现(`window_appearance`)。
pub struct GpuiDarkMode<'a> {
    cx: &'a gp::App,
}

impl<'a> GpuiDarkMode<'a> {
    pub fn new(cx: &'a gp::App) -> Self {
        GpuiDarkMode { cx }
    }
}

impl DarkModeProbe for GpuiDarkMode<'_> {
    fn is_dark_mode(&self) -> bool {
        matches!(
            self.cx.window_appearance(),
            gp::WindowAppearance::Dark | gp::WindowAppearance::VibrantDark
        )
    }
}

/// [`MotionPreferenceProbe`] 的 gpui 实现(S5 清单 ④)。
///
/// gpui 0.2.2 无 reduced-motion API(sable 锁定版本,诚实记录);
/// 「减少动态效果」本就是系统级设置,与宿主框架无关 —— 实读走
/// [`crate::os_motion::OsMotionProbe`](Windows =
/// `SPI_GETCLIENTAREAANIMATION`),与 egui 宿主同一 OS 通道。
#[derive(Debug, Default, Clone, Copy)]
pub struct GpuiMotionProbe {
    os: crate::os_motion::OsMotionProbe,
}

impl GpuiMotionProbe {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MotionPreferenceProbe for GpuiMotionProbe {
    fn animations_enabled(&self) -> bool {
        self.os.animations_enabled()
    }
}

/// [`FileDialog`] 的 rfd 实现(与 egui 宿主同件;OS 服务与内核无关)。
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

/// CursorShape → gpui CursorStyle。`None` = gpui 0.2.2 无对应光标
/// (调用方转 `Unsupported`;不硬凑近似形状,降级要看得见)。
pub fn map_cursor(shape: CursorShape) -> Option<gp::CursorStyle> {
    use gp::CursorStyle as G;
    match shape {
        CursorShape::Default => Some(G::Arrow),
        CursorShape::Pointer => Some(G::PointingHand),
        CursorShape::Crosshair => Some(G::Crosshair),
        CursorShape::Text => Some(G::IBeam),
        CursorShape::Grab => Some(G::OpenHand),
        CursorShape::Grabbing => Some(G::ClosedHand),
        CursorShape::Move => Some(G::ResizeLeftRight),
        CursorShape::Wait => None,
        CursorShape::NotAllowed => Some(G::OperationNotAllowed),
        CursorShape::ZoomIn | CursorShape::ZoomOut | CursorShape::Cell => None,
        CursorShape::ResizeEw => Some(G::ResizeLeftRight),
        CursorShape::ResizeNs => Some(G::ResizeUpDown),
        CursorShape::ResizeNwSe => Some(G::ResizeUpLeftDownRight),
        CursorShape::ResizeNeSw => Some(G::ResizeUpRightDownLeft),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 映射表完整性:16 形状全有裁定(映射或显式 None),无漏网。
    #[test]
    fn cursor_map_covers_all_shapes() {
        let all = [
            CursorShape::Default,
            CursorShape::Pointer,
            CursorShape::Crosshair,
            CursorShape::Text,
            CursorShape::Grab,
            CursorShape::Grabbing,
            CursorShape::Move,
            CursorShape::Wait,
            CursorShape::NotAllowed,
            CursorShape::ZoomIn,
            CursorShape::ZoomOut,
            CursorShape::Cell,
            CursorShape::ResizeEw,
            CursorShape::ResizeNs,
            CursorShape::ResizeNwSe,
            CursorShape::ResizeNeSw,
        ];
        let mut mapped = 0;
        for s in all {
            if map_cursor(s).is_some() {
                mapped += 1;
            }
        }
        // 诚实降级清单:Wait/ZoomIn/ZoomOut/Cell 无 gpui 对应
        assert_eq!(mapped, 12, "16 形状中 12 可映射,4 显式不支持");
        assert!(map_cursor(CursorShape::ZoomIn).is_none());
        assert_eq!(
            map_cursor(CursorShape::Default),
            Some(gp::CursorStyle::Arrow)
        );
    }
}
