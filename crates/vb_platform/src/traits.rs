//! 六 trait(硬骨头 #13 的契约面;全部对象安全,`dyn` 派发可用)。
//!
//! 约定:
//! - 可失败且语义有别的操作返回 `Result`(剪贴板/光标/窗口);
//!   「选或不选」本就是用户答案的对话框返回 `Option`(None = 用户取消,非错误)。
//! - 宿主能力缺失一律 [`PlatformError::Unsupported`],不假装成功(ADR-0046)。

use std::path::PathBuf;

use crate::error::PlatformError;
use crate::types::{CursorShape, DisplayMetrics, FileFilter};

/// 窗口句柄:标识 + 标题 + 前台化(多窗口/启动器→项目窗场景的最小面)。
pub trait WindowHandle {
    /// 宿主侧窗口标识(gpui=WindowId / egui=ViewportId / null=自定)。
    fn id(&self) -> u64;

    /// 运行期改标题(启动器→「项目名」等)。
    fn set_title(&mut self, title: &str) -> Result<(), PlatformError>;

    /// 把窗口带到前台并聚焦。
    fn activate(&mut self) -> Result<(), PlatformError>;
}

/// 剪贴板:跨宿主复制/粘贴 seam(启动器「复制路径」、节点复制走 OS 通道)。
pub trait Clipboard {
    /// 写入文本(egui 宿主走命令通道;gpui/arboard 走 OS)。
    fn set_text(&mut self, text: &str) -> Result<(), PlatformError>;

    /// 读取文本。`Ok(None)` = 剪贴板当前无文本;
    /// `Err(Unsupported)` = 宿主不暴露读(如 egui 0.35 命令通道)。
    fn text(&self) -> Result<Option<String>, PlatformError>;
}

/// 文件/目录对话框(打开工程、导出落点、图片置入)。
pub trait FileDialog {
    /// 选目录;用户取消 → `None`。
    fn pick_folder(&mut self) -> Option<PathBuf>;

    /// 选文件;用户取消 → `None`。
    fn pick_file(&mut self, title: &str, filters: &[FileFilter]) -> Option<PathBuf>;

    /// 另存为;用户取消 → `None`。
    fn save_file(
        &mut self,
        title: &str,
        default_name: &str,
        filters: &[FileFilter],
    ) -> Option<PathBuf>;
}

/// 系统光标:画布工具随动(平移=Grab、捏柄=Resize* 等)。
pub trait SystemCursor {
    /// 设置当前光标形状;宿主缺失该形状 → `Err(Unsupported)`。
    fn set_shape(&mut self, shape: CursorShape) -> Result<(), PlatformError>;
}

/// 显示器指标(每显示器 DPI;画布分辨率预算/高分屏清晰度的数据源)。
pub trait DisplayInfo {
    /// 枚举已知显示器(egui 宿主=当前 viewport 所在屏一条,诚实标注)。
    fn displays(&self) -> Vec<DisplayMetrics>;
}

/// 深色模式探测(主题初值;vb_shell 开窗前定 ThemeMode)。
pub trait DarkModeProbe {
    fn is_dark_mode(&self) -> bool;
}

/// 动效偏好探测(§8.10 reduced-motion,S5 清单 ④)。
///
/// 语义:**系统级**「减少动态效果 / 客户区动画」无障碍偏好 —— Windows
/// 落在 `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`。它与应用内
/// 动效总开关(`vb_ui::theme` 的 `motion_enabled`,用户显式设置)是
/// **并联**关系:任一关 → 动画直通(`anim_time` 归零)。
///
/// 探测**失败不降级成"关"**:系统不暴露该偏好时返回 `true`(允许动画)
/// —— 拿不到证据就不替用户决定减少动效(与 [`DarkModeProbe`] 同一
/// 「读不到 = 取默认」口径)。
pub trait MotionPreferenceProbe {
    /// `true` = 系统允许客户端区动画(用户未开启「减少动态效果」)。
    fn animations_enabled(&self) -> bool;
}
