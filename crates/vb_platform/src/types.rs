//! 平台类型:光标形状 / 文件过滤器 / 显示器指标(宿主无关的词汇表)。
//!
//! 词汇取自 vb_ui::cursor 的实际用量(Default/PointingHand/Grab/Grabbing/
//! Crosshair/Text/NotAllowed/ZoomIn/ZoomOut/Cell/Resize*)与两宿主内核的
//! 交集;宿主映射表在各自 backend 模块,缺失项走 `PlatformError::Unsupported`。

/// 画布/控件光标形状(宿主无关)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CursorShape {
    /// 默认箭头。
    Default,
    /// 可点击(手型)。
    Pointer,
    /// 十字(画笔/精确落点)。
    Crosshair,
    /// 文本工字。
    Text,
    /// 张开手(可拖拽)。
    Grab,
    /// 握拳(拖拽中)。
    Grabbing,
    /// 移动。
    Move,
    /// 忙碌。
    Wait,
    /// 禁止。
    NotAllowed,
    /// 放大。
    ZoomIn,
    /// 缩小。
    ZoomOut,
    /// 单元格选取(直接选择)。
    Cell,
    /// 横向调整。
    ResizeEw,
    /// 纵向调整。
    ResizeNs,
    /// 对角调整( northwest–southeast )。
    ResizeNwSe,
    /// 对角调整( northeast–southwest )。
    ResizeNeSw,
}

/// 文件对话框过滤器(扩展名不含点)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileFilter {
    /// 展示名(如 "Vellum 工程")。
    pub name: &'static str,
    /// 扩展名列表(如 &["vbproj", "json"])。
    pub extensions: &'static [&'static str],
}

impl FileFilter {
    pub const fn new(name: &'static str, extensions: &'static [&'static str]) -> Self {
        FileFilter { name, extensions }
    }
}

/// 单台显示器指标(硬骨头 #13:每显示器 DPI 是一等公民——画布清晰度
/// 预算与 4096 纹理消解都按显示器算,不按进程全局算)。
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayMetrics {
    /// 宿主侧显示器标识(egui=viewport id 派生 / gpui=窗口所在屏 / null=自定)。
    pub id: u64,
    /// DPI 换算因子:OS 逻辑 px ÷ 物理 px(100% 缩放 = 1.0;150% = 1.5)。
    pub scale_factor: f64,
    /// 物理分辨率(像素)。
    pub size_px: (u32, u32),
    /// 显示器在虚拟桌面中的原点(逻辑 px;宿主拿不到时 `None`,如 Wayland)。
    pub position: Option<(i32, i32)>,
    /// 是否主显示器。
    pub is_primary: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_and_metrics_are_plain_data() {
        let f = FileFilter::new("图片", &["png", "jpg"]);
        assert_eq!(f.extensions.len(), 2);
        let m = DisplayMetrics {
            id: 1,
            scale_factor: 1.5,
            size_px: (2560, 1440),
            position: Some((0, 0)),
            is_primary: true,
        };
        assert!((m.scale_factor - 1.5).abs() < f64::EPSILON);
    }
}
