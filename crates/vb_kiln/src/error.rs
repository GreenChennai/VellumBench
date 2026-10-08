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

    /// 协作式取消(硬骨头 #6):调用方经 `CancelToken` 请求停止,导出在
    /// 最近的分段边界检查点中止。与失败三态区分:不是错误,产物不落盘,
    /// kiln-cli 据此返回退出码 130(成功 0 / 失败非 0)。
    #[error("导出已取消")]
    Cancelled,
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
    /// 字体子集化失败,回退嵌入全量字体(EXP-01):CID 恒等映射保内容正确,
    /// 文件更大;计数进报告不静默(RB-06)。
    FontSubsetFallback { count: usize },
    /// 字形未入子集重映射表(EXP-01 防御):按原 GID 直发,可能缺字。
    GlyphRemapMiss { count: usize },
    /// GIF 调色板实际走了哪条编码路径(UP-4)。
    ///
    /// 此前**无处声明**:两条路径质量不同(ffmpeg `palettegen=stats_mode=diff` +
    /// `paletteuse=dither=sierra2_4a` 两 pass vs 纯 Rust 感知量化 LZW 单 pass),
    /// 而门控只是 `if ffmpeg_available()`,报告里看不出来 —— 下游无法判断
    /// 「这台机器没 ffmpeg」还是「ffmpeg 桥失败已回退」。`via_ffmpeg=false`
    /// 时**色阶与体积都会劣化**,应当让调用方显式知道。
    ///
    /// 非降级:两条路径都产出合法 GIF89a,内容一致,只是质量档位不同。
    GifPalettePath {
        via_ffmpeg: bool,
        /// ffmpeg 路径失败而回退时的原始原因(纯 Rust 路径为 None)。
        ffmpeg_error: Option<String>,
    },
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
            KilnWarning::FontSubsetFallback { .. } => "font_subset_fallback",
            KilnWarning::GlyphRemapMiss { .. } => "glyph_remap_miss",
            KilnWarning::GifPalettePath { .. } => "gif_palette_path",
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
                | KilnWarning::GlyphRemapMiss { .. }
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
            KilnWarning::FontSubsetFallback { count } => {
                format!("{count} 个字体子集化失败,已回退全量嵌入(CID 恒等映射,内容正确;文件更大)")
            }
            KilnWarning::GlyphRemapMiss { count } => {
                format!("{count} 个字形未入子集重映射表,按原 GID 直发(可能缺字,请检查字体文件)")
            }
            KilnWarning::GifPalettePath {
                via_ffmpeg, ffmpeg_error,
            } => match (via_ffmpeg, ffmpeg_error) {
                (true, None) => {
                    "GIF 调色板:ffmpeg palettegen(diff)+ paletteuse(sierra2_4a)两 pass".into()
                }
                (false, None) => "GIF 调色板:纯 Rust 感知量化 LZW 单 pass(未装 ffmpeg;色阶与体积劣于两 pass,装 ffmpeg 可升档)".into(),
                (false, Some(e)) => {
                    format!("GIF 调色板:ffmpeg 两 pass 失败已回退纯 Rust 单 pass({e});装 ffmpeg 可升档")
                }
                // 不变量:`via_ffmpeg=true` 只在 ffmpeg 桥成功时置位,此时不可能
                // 带错误原因。保留兜底分支而不是 unreachable!(),免得将来有人
                // 改了置位逻辑后 panic 在错误处理路径上。
                (true, Some(e)) => format!(
                    "GIF 调色板:ffmpeg 两 pass({e})"
                ),
            },
        }
    }
}
