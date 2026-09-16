//! Kiln 错误模型:全分类、不 panic、可追踪。
//!
//! 设计纪律:异常输入(超大画布/缺失字体/损坏动画数据)不崩溃 ——
//! 返回带行动建议的错误或降级输出,并在 KilnReport 留痕。

#[derive(Debug, thiserror::Error)]
pub enum KilnError {
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),

    #[error("文档编码失败: {0}")]
    Encode(String),

    #[error("画布尺寸非法:{w}x{h}(边长须为 1..={max} 像素;降低 scale 后重试)")]
    CanvasTooLarge { w: u32, h: u32, max: u32 },

    #[error("画布面积超限:{w}x{h} = {pixels} 像素,上限 {max}(降低 scale 后重试)")]
    CanvasAreaTooLarge {
        w: u32,
        h: u32,
        pixels: u64,
        max: u64,
    },

    #[error("非法参数:{0}")]
    BadParam(String),

    #[error("动画参数非法:{0}(fps 须为 1..=60,duration 须为 0.04..=3600)")]
    BadAnimation(String),

    #[error("MP4 编码需要 ffmpeg(未在 PATH 找到);替代:导出 GIF,或安装 ffmpeg 后重试")]
    FfmpegMissing,

    #[error("ffmpeg 编码失败(退出码 {code:?}):{stderr}")]
    FfmpegFailed { code: Option<i32>, stderr: String },

    #[error("PPTX 容器写入失败:{0}")]
    PptxStructure(String),

    #[error("PDF 结构错误:{0}")]
    PdfStructure(String),
}

pub type KilnResult<T> = Result<T, KilnError>;

/// warning 分类(不中断导出,进 KilnReport)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KilnWarning {
    /// 静态画布导出动画格式:动画参数被忽略,输出单帧。
    StaticCanvasAnimation,
    /// scale 超过 8,已钳制。
    ScaleClamped(u32),
    /// JPG 不支持透明,已垫白底。
    JpgOpaqueForced,
    /// 位图资产缺失,已画占位框。
    ImageMissing { src: String },
    /// MP4 无 ffmpeg,降级为 GIF 流输出(内容相同,.mp4 扩展名)。
    Mp4DowngradedToGif,
    /// 冻结块(Frozen)以占位框输出。
    FrozenPlaceholder { name: String },
    /// 文本含非 WinAnsi 字符,PDF/EPS 内以 CID 兼容方式降级(PPTX/SVG 无此限制)。
    TextTransliterated { count: usize },
}

impl KilnWarning {
    pub fn message(&self) -> String {
        match self {
            KilnWarning::StaticCanvasAnimation => "静态画布:GIF/MP4 动画参数被忽略,输出单帧".into(),
            KilnWarning::ScaleClamped(v) => format!("倍率 {v} 超上限 8,已钳制为 8"),
            KilnWarning::JpgOpaqueForced => "JPG 不支持透明,已垫白底".into(),
            KilnWarning::ImageMissing { src } => format!("位图资产缺失:{src}(已画占位框)"),
            KilnWarning::Mp4DowngradedToGif => {
                "未安装 ffmpeg:MP4 降级为 GIF 流写入 .mp4 扩展名(播放器可打开)".into()
            }
            KilnWarning::FrozenPlaceholder { name } => {
                format!("冻结块「{}」以占位框输出", name)
            }
            KilnWarning::TextTransliterated { count } => {
                format!("PDF/EPS 内 {count} 处非拉丁字符以兼容字形降级(SVG/PPTX 保持原文)")
            }
        }
    }
}
