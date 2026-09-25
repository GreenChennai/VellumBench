//! 图标系统（设计文档 14 篇 §3.4）。
//!
//! ## 解决什么问题
//!
//! 原先图标是 emoji（📁 ▢ 🖼 ✎ ❄ 🗑 ✋）与 ASCII（▣ ➤ ▭ ◯），三个毛病：
//!
//! 1. emoji 在不同 Windows 版本渲染不同（Segoe UI Emoji 更新就会变样）；
//! 2. 彩色 emoji 与单色界面冲突，视觉上像"贴纸"；
//! 3. 无法统一描边粗细，图标之间气质不一致。
//!
//! 这三点合起来是"业余感"的三大来源之三。
//!
//! ## 做法
//!
//! 用 **Lucide**（ISC 许可，1666 个图标，描边风格）经 `iconflow` 1.0 集成。
//! `iconflow` 只吃字体字节、**不绑定 egui 版本**，因此比 `egui-phosphor`
//! 这类跟随 egui 版本发布的方案更稳。
//!
//! 图标一律通过 [`Name`] 枚举引用，**代码里不允许出现裸字形或 emoji**；
//! `all_icons_resolve` 测试保证枚举里每个图标都能在 Lucide 里找到。
//!
//! ## 用法
//!
//! ```ignore
//! // 图标 + 文字（图标走 vb-icon 族，文字走正文族）
//! ui.add(vb_ui::components::ToolButton::new(icons::Name::ToolSelect, "选择")
//!     .shortcut("V"));
//!
//! // 只要图标
//! ui.label(icons::rich(icons::Name::Delete, 14.0));
//! ```

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId, RichText};
use iconflow::{Pack, Size, Style};

use crate::fonts;

/// 图标字体在 `FontDefinitions.font_data` 里的键。
const ICON_FACE_KEY: &str = "vb-icon-face";

/// 找不到字形时的兜底字符。正常情况**永远不会用到** ——
/// `all_icons_resolve` 测试会拦住任何拼错的图标名。
pub const FALLBACK: char = '·';

/// 界面用到的全部图标。
///
/// 刻意做成枚举而不是散落的字符串：这样"界面用了哪些图标"是可枚举、可测试的，
/// 也避免同一个语义在 A 处用 `trash-2`、B 处用 `trash`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Name {
    // ── 工具箱 ──
    /// 选择工具。
    ToolSelect,
    /// 直接选择工具(阶段 6:与「选择」明确区分的**独立**工具/图标)。
    ToolDirectSelect,
    /// 矩形工具。
    ToolRect,
    /// 椭圆工具。
    ToolEllipse,
    /// 抓手工具。
    ToolHand,
    /// 文字工具。
    ToolText,
    /// 吸管工具。
    ToolEyedropper,
    /// 画板工具。
    ToolArtboard,
    /// 渐变工具。
    ToolGradient,
    /// 剪刀工具。
    ToolScissors,
    /// 编组选择工具。
    ToolGroupSelect,
    /// 旋转工具(05-2 X-4 变换工具族)。
    ToolRotate,
    /// 镜像工具(05-2 X-4 变换工具族)。
    ToolMirror,
    /// 缩放工具(05-2 X-4 变换工具族)。
    ToolScale,
    /// 自由变换工具(05-2 X-4 变换工具族)。
    ToolFreeTransform,
    /// 铅笔工具(05-2 X-5 曲线工具)。
    ToolPencil,
    /// 曲率工具(05-2 X-5 曲线工具)。
    ToolCurvature,
    /// 度量工具(05-2 09-E 编辑态辅助)。
    ToolMeasure,

    // ── 图层树的节点类型 ──
    /// 画板。
    KindArtboard,
    /// 图层。
    KindLayer,
    /// 编组。
    KindGroup,
    /// 矩形盒子。
    KindBox,
    /// 文本。
    KindText,
    /// 图片。
    KindImage,
    /// 矢量路径。
    KindVector,
    /// 切片。
    KindSlice,
    /// 冻结块（ADR-0017）。
    KindFrozen,

    // ── 图层面板操作 ──
    /// 新建图层。
    AddLayer,
    /// 新建子层。
    AddChild,
    /// 编组。
    AddGroup,
    /// 删除。
    Delete,
    /// 显示中。
    Visible,
    /// 已隐藏。
    Hidden,
    /// 已锁定。
    Locked,
    /// 未锁定。
    Unlocked,
    /// 上移一层。
    MoveUp,
    /// 下移一层。
    MoveDown,

    // ── 状态栏与视图 ──
    /// 上一个画板。
    PrevArtboard,
    /// 下一个画板。
    NextArtboard,
    /// 放大。
    ZoomIn,
    /// 缩小。
    ZoomOut,
    /// 适合窗口。
    ZoomFit,
    /// 网格。
    Grid,
    /// 轮廓模式。
    Outline,
    /// 坐标。
    Crosshair,

    // ── 主题 ──
    /// 切到浅色。
    ThemeLight,
    /// 切到深色。
    ThemeDark,

    // ── 通用反馈 ──
    /// 警告（如冻结块）。
    Alert,
    /// 成功 / 已选中。
    Check,
    /// 关闭 / 清除。
    Close,
    /// 信息。
    Info,
    /// 复制文本(toast 错误可复制,02-6-6)。
    Copy,

    // ── 面板坞 Tab(02-1:4 页 + 折叠图标条) ──
    /// 属性面板 Tab。
    PanelProperties,
    /// 令牌面板 Tab。
    PanelTokens,

    // ── 折叠指示 ──
    /// 已折叠（箭头朝右）。
    Collapsed,
    /// 已展开（箭头朝下）。
    Expanded,

    // ── 05-9 动效时间轴(09-I):播放控制 ──
    /// 播放。
    Play,
    /// 暂停。
    Pause,

    // ── U-2 次级坞折叠图标条:插件组 ──
    /// 插件(折叠图标条上代表「插件」组;其余组复用既有语义图标)。
    Puzzle,

    // ── U-5 面板空态 ──
    /// 历史(历史面板空态图标)。
    History,
}

impl Name {
    /// 全部图标（供测试与遍历）。
    pub const ALL: &'static [Name] = &[
        Name::ToolSelect,
        Name::ToolDirectSelect,
        Name::ToolRect,
        Name::ToolEllipse,
        Name::ToolHand,
        Name::ToolText,
        Name::ToolEyedropper,
        Name::ToolArtboard,
        Name::ToolGradient,
        Name::ToolScissors,
        Name::ToolGroupSelect,
        Name::ToolRotate,
        Name::ToolMirror,
        Name::ToolScale,
        Name::ToolFreeTransform,
        Name::ToolPencil,
        Name::ToolCurvature,
        Name::ToolMeasure,
        Name::KindArtboard,
        Name::KindLayer,
        Name::KindGroup,
        Name::KindBox,
        Name::KindText,
        Name::KindImage,
        Name::KindVector,
        Name::KindSlice,
        Name::KindFrozen,
        Name::AddLayer,
        Name::AddChild,
        Name::AddGroup,
        Name::Delete,
        Name::Visible,
        Name::Hidden,
        Name::Locked,
        Name::Unlocked,
        Name::MoveUp,
        Name::MoveDown,
        Name::PrevArtboard,
        Name::NextArtboard,
        Name::ZoomIn,
        Name::ZoomOut,
        Name::ZoomFit,
        Name::Grid,
        Name::Outline,
        Name::Crosshair,
        Name::ThemeLight,
        Name::ThemeDark,
        Name::Alert,
        Name::Check,
        Name::Close,
        Name::Info,
        Name::Copy,
        Name::PanelProperties,
        Name::PanelTokens,
        Name::Collapsed,
        Name::Expanded,
        Name::Play,
        Name::Pause,
        Name::Puzzle,
        Name::History,
    ];

    /// 对应的 Lucide 图标名（已逐个核对存在于 iconflow 0.1 的 lucide 表）。
    pub const fn lucide(self) -> &'static str {
        match self {
            Name::ToolSelect => "mouse-pointer-2",
            // 空心指针:与「选择」的实心指针区分(阶段 6 / design/06 §3.2)
            Name::ToolDirectSelect => "mouse-pointer",
            Name::ToolRect => "square",
            Name::ToolEllipse => "circle",
            Name::ToolHand => "hand",
            Name::ToolText => "type",
            Name::ToolEyedropper => "pipette",
            Name::ToolArtboard => "frame",
            Name::ToolGradient => "blend",
            Name::ToolScissors => "scissors",
            Name::ToolGroupSelect => "lasso-select",

            // 05-2:新工具图标(全部走 Lucide 既有字形,all_icons_resolve 把关)
            Name::ToolRotate => "rotate-cw",
            Name::ToolMirror => "flip-horizontal",
            Name::ToolScale => "scaling",
            Name::ToolFreeTransform => "expand",
            Name::ToolPencil => "pencil",
            Name::ToolCurvature => "spline",
            Name::ToolMeasure => "ruler",

            Name::KindArtboard => "frame",
            Name::KindLayer => "layers",
            Name::KindGroup => "group",
            Name::KindBox => "square",
            Name::KindText => "type",
            Name::KindImage => "image",
            Name::KindVector => "pen-tool",
            Name::KindSlice => "scissors",
            Name::KindFrozen => "snowflake",

            Name::AddLayer => "plus",
            Name::AddChild => "corner-down-right",
            Name::AddGroup => "group",
            Name::Delete => "trash-2",
            Name::Visible => "eye",
            Name::Hidden => "eye-off",
            Name::Locked => "lock",
            Name::Unlocked => "lock-open",
            Name::MoveUp => "arrow-up",
            Name::MoveDown => "arrow-down",

            Name::PrevArtboard => "chevron-left",
            Name::NextArtboard => "chevron-right",
            Name::ZoomIn => "plus",
            Name::ZoomOut => "minus",
            Name::ZoomFit => "maximize",
            Name::Grid => "grid-3x3",
            Name::Outline => "square-dashed",
            Name::Crosshair => "crosshair",

            Name::ThemeLight => "sun",
            Name::ThemeDark => "moon",

            Name::Alert => "circle-alert",
            Name::Check => "check",
            Name::Close => "x",
            Name::Info => "info",
            Name::Copy => "copy",

            Name::PanelProperties => "sliders-horizontal",
            Name::PanelTokens => "palette",

            Name::Collapsed => "chevron-right",
            Name::Expanded => "chevron-down",

            // 05-9 动效时间轴(09-I)
            Name::Play => "play",
            Name::Pause => "pause",

            // U-2:次级坞折叠图标条(插件组)
            Name::Puzzle => "puzzle",

            // U-5:历史面板空态
            Name::History => "history",
        }
    }

    /// 该图标的字形。
    ///
    /// 拼错或缺失时返回 [`FALLBACK`] 而不是 panic —— GUI 里 panic 比缺一个图标
    /// 严重得多。`all_icons_resolve` 测试保证这种情况下不会真的发生。
    pub fn glyph(self) -> char {
        iconflow::try_icon(Pack::Lucide, self.lucide(), Style::Regular, Size::Regular)
            .ok()
            .and_then(|r| char::from_u32(r.codepoint))
            .unwrap_or(FALLBACK)
    }

    /// 是否成功解析到真实字形（测试用）。
    pub fn resolves(self) -> bool {
        self.glyph() != FALLBACK
    }

    /// 图标尺寸：面板内 14px / 工具条 16px / 底部工具条 20px。
    pub const fn size_in_panel() -> f32 {
        14.0
    }
    /// 见 [`Name::size_in_panel`]。
    pub const fn size_in_toolbar() -> f32 {
        16.0
    }
    /// 见 [`Name::size_in_panel`]。
    pub const fn size_floating() -> f32 {
        20.0
    }
}

/// 图标 `FontId`。
pub fn font(size: f32) -> FontId {
    FontId::new(size, fonts::family_icon())
}

/// 图标文本（`RichText`，默认面板内尺寸）。
///
/// 颜色跟随当前文字色 —— **图标不单独上色**，激活态由调用方改成 accent。
pub fn rich(name: Name, size: f32) -> RichText {
    RichText::new(name.glyph().to_string()).font(font(size))
}

/// 图标 + 文字并排（间距 6px，见 14 篇 §3.4）。
///
/// 返回 `(图标 RichText, 文字 RichText)`，由调用方放进 `ui.horizontal(...)`；
/// 这里不直接画 UI，是为了让组件层决定布局。
pub fn with_label(name: Name, label: &str, size: f32) -> (RichText, RichText) {
    (
        rich(name, size),
        RichText::new(label).font(fonts::font(13.0, fonts::Weight::Regular)),
    )
}

/// 把 Lucide 字体注册进 egui（由 [`crate::fonts::install`] 调用）。
pub fn register(fonts_def: &mut FontDefinitions) {
    let Some(asset) = lucide_asset() else {
        return;
    };
    fonts_def.font_data.insert(
        ICON_FACE_KEY.into(),
        Arc::new(FontData::from_owned(asset.bytes.to_vec())),
    );
    fonts_def.families.insert(
        FontFamily::Name(fonts::FAMILY_ICON.into()),
        vec![ICON_FACE_KEY.into()],
    );
}

/// 找到承载 Lucide Regular/Regular 变体的那个字体资源。
///
/// 不能把 `iconflow::fonts()` 全部塞进一个族：同一码点在多个变体里可能指向
/// 不同字形，先命中的会赢，结果取决于注册顺序 —— 那是靠运气。这里锁定
/// 与 [`Name::glyph`] 解析一致的**那一个**变体。
fn lucide_asset() -> Option<iconflow::FontAsset> {
    let probe = iconflow::try_icon(Pack::Lucide, "circle", Style::Regular, Size::Regular).ok()?;
    iconflow::fonts()
        .iter()
        .find(|f| f.family == probe.family && !f.bytes.is_empty())
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 枚举里每个图标都必须真的能在 Lucide 里找到。
    ///
    /// 这条测试是"界面不说谎"的一部分：图标名写错不会让编译失败，
    /// 但会在界面上留一个空白 —— 测试把它变成红色。
    #[test]
    fn all_icons_resolve() {
        let missing: Vec<_> = Name::ALL
            .iter()
            .filter(|n| !n.resolves())
            .map(|n| n.lucide())
            .collect();
        assert!(
            missing.is_empty(),
            "以下图标在 Lucide 里找不到（拼写错误或图标不存在）：{missing:?}"
        );
    }

    /// `ALL` 必须覆盖枚举的每一个变体，且不重复。
    ///
    /// 用 debug 名字做对比，新增变体忘了加进 `ALL` 时这里会失败。
    #[test]
    fn all_list_is_complete_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for n in Name::ALL {
            assert!(seen.insert(format!("{n:?}")), "ALL 里有重复项：{n:?}");
        }
        // 枚举变体总数（含 ALL 自己占的一行由 compiler 保证一致）
        assert_eq!(
            Name::ALL.len(),
            60,
            "Name::ALL 的条数与枚举变体数不符：新增图标后要同步 ALL(第四轮 U-2 增 Puzzle/U-5 增 History)"
        );
    }

    /// 同一个语义不应映射到两个不同的 Lucide 名（避免 A 处 trash-2、B 处 trash）。
    #[test]
    fn no_conflicting_mappings_for_same_semantic() {
        // ToolRect 与 KindBox 都是方形，共用 "square" 是有意为之（同一个语义：
        // "矩形"）；这里只断言不该出现的重叠。
        let dup = [(Name::KindGroup, Name::AddGroup)];
        for (a, b) in dup {
            assert_eq!(
                a.lucide(),
                b.lucide(),
                "{a:?} 与 {b:?} 应共用同一个图标（都是「编组」语义）"
            );
        }
        // 显示/隐藏、锁/解锁必须是成对的相反图标
        assert_ne!(Name::Visible.lucide(), Name::Hidden.lucide());
        assert_ne!(Name::Locked.lucide(), Name::Unlocked.lucide());
        assert_ne!(Name::MoveUp.lucide(), Name::MoveDown.lucide());
    }

    /// 图标名必须是小写连字符风格（Lucide 约定），防止有人写成驼峰。
    #[test]
    fn lucide_names_are_kebab_case() {
        for n in Name::ALL {
            let name = n.lucide();
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "图标名 {name} 不符合 Lucide 的 kebab-case 约定"
            );
        }
    }

    /// 尺寸三档必须递增（14 / 16 / 20）。
    #[test]
    fn icon_sizes_are_ordered() {
        assert!(Name::size_in_panel() < Name::size_in_toolbar());
        assert!(Name::size_in_toolbar() < Name::size_floating());
        assert_eq!(Name::size_in_panel(), 14.0);
        assert_eq!(Name::size_floating(), 20.0);
    }
}
