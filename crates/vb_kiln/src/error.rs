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
    /// 拉丁文本无字体数据可嵌入,以未嵌入 Helvetica 输出(车道 K 兜底)。
    UnembeddedLatinText,
    /// MP4 无 ffmpeg,降级为 GIF 流输出(内容相同,.mp4 扩展名)。
    Mp4DowngradedToGif,
    /// 冻结块(Frozen)以占位框输出。
    FrozenPlaceholder { name: String },
    /// 文本含非 WinAnsi 字符,PDF/EPS 内以 CID 兼容方式降级(PPTX/SVG 无此限制)。
    TextTransliterated { count: usize },
    /// 静态资源 404(VB-1):浏览器车道;字体/脚本缺失会被浏览器静默降级。
    AssetNotFound { src: String },
    /// 某 CSS 属性/绘制原语在当前车道不被支持,已忽略(VB-2)。
    /// 同属性同车道聚合计数;命中置 degraded(输出与源不等价)。
    UnsupportedPropertyDropped {
        prop: String,
        count: usize,
        lane: &'static str,
    },
    /// 内联 SVG 子树在矢量输出中被栅格化(VB-2;真矢量丢失,外观保留)。
    InlineSvgRasterized { count: usize, format: &'static str },
    /// 非矩形/不可解析的裁剪形状退化为无裁剪(VB-2)。
    ClipShapeApproximated { shape: String, count: usize },
    /// SVG 导入跳过的对象类别聚合(VB-4):效果丢失但内容不丢。
    ImportObjectSkipped { kind: String, count: usize },
}

impl KilnWarning {
    /// 稳定类别键(小写蛇形;供 KilnReport::warnings_by_kind 聚合与
    /// 下游门禁,如「unsupported_dropped > 0 拒绝交付矢量稿」)。
    pub fn kind(&self) -> &'static str {
        match self {
            KilnWarning::StaticCanvasAnimation => "static_canvas_animation",
            KilnWarning::ScaleClamped(_) => "scale_clamped",
            KilnWarning::JpgOpaqueForced => "jpg_opaque_forced",
            KilnWarning::ImageMissing { .. } => "image_missing",
            KilnWarning::UnembeddedLatinText => "unembedded_latin_text",
            KilnWarning::Mp4DowngradedToGif => "mp4_downgraded_to_gif",
            KilnWarning::FrozenPlaceholder { .. } => "frozen_placeholder",
            KilnWarning::TextTransliterated { .. } => "text_transliterated",
            KilnWarning::AssetNotFound { .. } => "asset_not_found",
            KilnWarning::UnsupportedPropertyDropped { .. } => "unsupported_dropped",
            KilnWarning::InlineSvgRasterized { .. } => "inline_svg_rasterized",
            KilnWarning::ClipShapeApproximated { .. } => "clip_shape_approximated",
            KilnWarning::ImportObjectSkipped { .. } => "import_object_skipped",
        }
    }

    /// 是否语义降级(VB-2/ADR-0046):输出与源不等价。
    /// 命中任一条 KilnReport.degraded 必须为 true。
    pub fn is_degrading(&self) -> bool {
        matches!(
            self,
            KilnWarning::UnsupportedPropertyDropped { .. }
                | KilnWarning::InlineSvgRasterized { .. }
                | KilnWarning::ClipShapeApproximated { .. }
                | KilnWarning::ImportObjectSkipped { .. }
        )
    }

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
            KilnWarning::UnembeddedLatinText => {
                "拉丁文本缺字体数据:PDF 以未嵌入 Helvetica 兜底;                 浏览器车道(--engine auto/browser)可获全字体嵌入".into()
            }
            KilnWarning::TextTransliterated { count } => {
                format!("PDF/EPS 内 {count} 处非拉丁字符以兼容字形降级(SVG/PPTX 保持原文)")
            }
            KilnWarning::AssetNotFound { src } => {
                // 与 vb_browser::staticsrv::asset_not_found_message 同格式
                format!("静态资源 404:{src}(浏览器车道;字体/脚本缺失会静默降级)")
            }
            KilnWarning::UnsupportedPropertyDropped { prop, count, lane } => {
                format!(
                    "{lane} 车道不支持 {prop},已忽略 {count} 处(输出与源不等价;浏览器车道可保真)"
                )
            }
            KilnWarning::InlineSvgRasterized { count, format } => {
                format!("{format} 输出中 {count} 个内联 SVG 子树被栅格化(外观保留,真矢量丢失)")
            }
            KilnWarning::ClipShapeApproximated { shape, count } => {
                format!("裁剪形状 {shape} 不受支持,{count} 处退化为无裁剪")
            }
            KilnWarning::ImportObjectSkipped { kind, count } => {
                format!("导入跳过 {kind}×{count}(内容已导入,效果不带)")
            }
        }
    }
}
