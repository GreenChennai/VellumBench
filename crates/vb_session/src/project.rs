//! 项目会话与宿主无关文档投影。
//!
//! `ProjectSession` 是新宿主打开 HTML/CSS 项目的唯一入口。文档模型保持私有；
//! 启用 `canvas-projection` 时只暴露不可编辑的 `DrawList` 快照，避免 UI 面板
//! 直接持有或改写第二份文档模型。

#[cfg(feature = "canvas-projection")]
use std::collections::HashMap;
use std::collections::HashSet;
#[cfg(feature = "canvas-projection")]
use std::fs::{self, OpenOptions};
#[cfg(feature = "canvas-projection")]
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
#[cfg(feature = "canvas-projection")]
use std::sync::Mutex;

use crate::command::{CommandId, CommandOutcome, CommandRequest, Dispatcher};
use crate::i18n::t;
use crate::selection::{ModifierState, SelectionState};
use crate::tools::{ToolId, ToolState};
use vb_animation::preset::{
    parse_animation_presets, preset_keyframes_name, serialize_preset_blocks, AnimationPreset,
    AnimationPresetRef,
};
use vb_animation::timeline::{
    keyframe_offset_permille, parse_anim_source, serialize_animation, serialize_animation_named,
    serialize_keyframes, static_values, AnimModel, AnimSource, Easing, KeyValue, Keyframe,
    TrackProp,
};
use vb_common::color::parse_color;
use vb_doc::export::write_project;
use vb_doc::import::{import_project, FontFace, ImportResult};
use vb_doc::model::{Geom, Node, NodeId, NodeKind, NodeTree, TextMode};
use vb_doc::{Command, Document, UndoStack, VbError};
#[cfg(feature = "canvas-projection")]
use vb_render::encode::BitmapData;
use vb_tools::align::{aligned_delta, AbsBox, AlignMode, AlignTo};

/// 画布内的 host-independent 矩形几何,坐标相对所属画板。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// 暂存的智能吸附平移与画板本地参考线;它本身不修改文档。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SnapPreview {
    pub delta: [f64; 2],
    pub guides: Vec<[f64; 4]>,
}

/// 检查器读取的单对象属性投影;不暴露文档 Node 或 NodeId。
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedNodeSnapshot {
    pub sid: String,
    pub name: String,
    pub kind: &'static str,
    pub geometry: CanvasRect,
    pub fill: Option<String>,
    /// 是否为当前对象类型支持的单色填充/文字颜色,复杂填充保持只读。
    pub fill_editable: bool,
    pub text: Option<String>,
    /// 是否可无损编辑纯文本;带富文本段注记的文本必须只读。
    pub text_editable: bool,
    pub locked: bool,
}

/// 检查器投影;多选仅公开数量,避免把首个对象误当成批量几何值。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InspectorSnapshot {
    pub selected_count: usize,
    pub selected: Option<SelectedNodeSnapshot>,
}

/// Selected-node animation projection; foreign CSS keyframes are explicit read-only data.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectedAnimationSnapshot {
    pub sid: String,
    pub name: String,
    pub locked: bool,
    pub source: AnimSource,
    pub static_values: ((f64, f64), f64, f64, f64),
}

/// Stable session-only identity for one keyframe inside a selected animation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnimationKeyframeSelection {
    pub sid: String,
    pub track: TrackProp,
    pub offset_permille: u16,
}

/// Signed movement expressed in CSS timeline permille.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyframeOffsetDelta(i16);

impl KeyframeOffsetDelta {
    /// Construct a bounded time offset; values outside one full timeline are rejected.
    ///
    /// # Errors
    /// Returns a parse error unless `permille` is in `-1000..=1000`.
    pub fn new(permille: i32) -> Result<Self, VbError> {
        if !(-1000..=1000).contains(&permille) {
            return Err(VbError::Parse("关键帧位移必须位于 -1000..=1000‰".into()));
        }
        Ok(Self(permille as i16))
    }

    /// Signed CSS timeline offset.
    pub const fn permille(self) -> i16 {
        self.0
    }
}

/// Discrete timeline frame rates used to quantize keyframe drags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationFrameRate {
    Fps24,
    Fps30,
    Fps60,
}

impl AnimationFrameRate {
    pub const ALL: [Self; 3] = [Self::Fps24, Self::Fps30, Self::Fps60];

    pub const fn fps(self) -> u16 {
        match self {
            Self::Fps24 => 24,
            Self::Fps30 => 30,
            Self::Fps60 => 60,
        }
    }
}

#[derive(Debug, Clone)]
struct KeyframeMovePlan {
    sid: String,
    model: AnimModel,
    moved: Vec<AnimationKeyframeSelection>,
    playhead_seconds: f64,
}

#[derive(Debug, Clone)]
struct KeyframeCopyPlan {
    sid: String,
    model: AnimModel,
    copied: Vec<AnimationKeyframeSelection>,
    playhead_seconds: f64,
}

/// Animation inspector data without exposing the private Document or NodeId.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimationSnapshot {
    pub selected_count: usize,
    pub selected: Option<SelectedAnimationSnapshot>,
    /// Validated shared named CSS animation presets available to the timeline UI.
    pub presets: Vec<AnimationPresetRef>,
    /// 当前本地时间轴的会话选区;文档/命令写入由 Session 方法负责。
    pub selected_keyframes: Vec<AnimationKeyframeSelection>,
    /// 可播放模型的总时长(秒);外部 CSS 动画保持只读且不显示可编辑时长。
    pub duration_seconds: Option<f64>,
    /// 会话视图态播放头(秒),不写入 Document 或 Undo。
    pub playhead_seconds: f64,
    pub playing: bool,
    pub looping: bool,
}

fn complex_background_value(node: &Node) -> Option<&str> {
    node.style.iter().find_map(|declaration| {
        let value = declaration.value.trim();
        let complex = match declaration.prop.as_str() {
            "background" => parse_color(value).is_none(),
            "background-image" => !value.eq_ignore_ascii_case("none"),
            _ => false,
        };
        complex.then_some(declaration.value.as_str())
    })
}

fn inspector_fill_property(node: &Node) -> Option<&'static str> {
    match &node.kind {
        NodeKind::Box => complex_background_value(node)
            .is_none()
            .then_some("background-color"),
        NodeKind::Text { .. } => Some("color"),
        _ => None,
    }
}

fn validate_animation_model(model: &AnimModel) -> Result<(), VbError> {
    const MAX_DURATION_MS: f64 = 60_000.0;
    const MAX_OFFSET_PX: f64 = 1_000_000.0;
    if !model.duration_ms.is_finite()
        || !(1.0..=MAX_DURATION_MS).contains(&model.duration_ms)
        || !model.delay_ms.is_finite()
        || !(0.0..=MAX_DURATION_MS).contains(&model.delay_ms)
        || !(model.iterations == f64::INFINITY
            || (model.iterations.is_finite()
                && model.iterations >= 1.0
                && model.iterations <= 10_000.0
                && model.iterations.fract() == 0.0))
    {
        return Err(VbError::Parse(
            "动画时长、延迟或迭代次数超出允许范围".into(),
        ));
    }

    let duration_seconds = model.duration_ms / 1000.0;
    let mut easing_by_offset = std::collections::BTreeMap::<i64, Easing>::new();
    for (_, frames) in &model.tracks {
        if frames.len() > 4096 {
            return Err(VbError::Parse("单条动画轨道关键帧不能超过 4096 个".into()));
        }
        for frame in frames {
            if !frame.t.is_finite() || frame.t < 0.0 || frame.t > duration_seconds {
                return Err(VbError::Parse("关键帧时间超出动画时长".into()));
            }
            let offset = i64::from(
                keyframe_offset_permille(frame.t, duration_seconds)
                    .ok_or_else(|| VbError::Parse("关键帧时间超出动画时长".into()))?,
            );
            match easing_by_offset.get(&offset) {
                Some(existing) if existing != &frame.easing => {
                    return Err(VbError::Parse(
                        "同一关键帧停靠点的不同轨道必须使用相同缓动".into(),
                    ));
                }
                None => {
                    easing_by_offset.insert(offset, frame.easing);
                }
                _ => {}
            }
            let valid_value = match &frame.value {
                KeyValue::Offset(x, y) => {
                    x.is_finite()
                        && y.is_finite()
                        && x.abs() <= MAX_OFFSET_PX
                        && y.abs() <= MAX_OFFSET_PX
                }
                KeyValue::Factor(value) => value.is_finite() && *value > 0.0 && *value <= 1000.0,
                KeyValue::Alpha(value) => value.is_finite() && (0.0..=1.0).contains(value),
                KeyValue::Angle(value) => value.is_finite() && value.abs() <= 360_000.0,
            };
            if !valid_value {
                return Err(VbError::Parse("关键帧包含无效的数值".into()));
            }
            if let Easing::Bezier(x1, y1, x2, y2) = frame.easing {
                if !x1.is_finite()
                    || !(0.0..=1.0).contains(&x1)
                    || !y1.is_finite()
                    || y1.abs() > 10.0
                    || !x2.is_finite()
                    || !(0.0..=1.0).contains(&x2)
                    || !y2.is_finite()
                    || y2.abs() > 10.0
                {
                    return Err(VbError::Parse("贝塞尔缓动控制点超出允许范围".into()));
                }
            }
        }
    }

    for (offset, declarations) in &model.extra {
        if !offset.is_finite()
            || !(0.0..=1.0).contains(&f64::from(*offset))
            || declarations.iter().any(|(property, value)| {
                property.is_empty()
                    || property.len() > 128
                    || value.len() > 4096
                    || property.chars().any(char::is_control)
                    || value.chars().any(char::is_control)
            })
        {
            return Err(VbError::Parse("动画中的未建模 CSS 声明无效或过长".into()));
        }
    }
    Ok(())
}

#[derive(Clone)]
struct ClipboardEntry {
    parent_sid: String,
    index: usize,
    tree: NodeTree,
}

struct GroupTarget {
    sid: String,
    parent_sid: String,
    index: usize,
    member_sids: Vec<String>,
}

#[cfg(feature = "canvas-projection")]
#[derive(Default)]
struct ProjectImageCache {
    by_source: HashMap<String, BitmapData>,
    decoded_bytes: usize,
}

#[cfg(feature = "canvas-projection")]
impl ProjectImageCache {
    const MAX_ENTRIES: usize = 64;
    const MAX_BYTES: usize = 256 * 1024 * 1024;

    fn load(&mut self, root: &Path, src: &str) -> Option<BitmapData> {
        let cacheable = src.len() <= 1_024 && !src.to_ascii_lowercase().starts_with("data:");
        if cacheable {
            if let Some(bitmap) = self.by_source.get(src) {
                return Some(bitmap.clone());
            }
        }
        let bitmap = vb_render::assets::load_project_bitmap(root, src)?;
        let size = bitmap.rgba.len();
        if cacheable && size <= Self::MAX_BYTES {
            if self.by_source.len() >= Self::MAX_ENTRIES
                || self.decoded_bytes.saturating_add(size) > Self::MAX_BYTES
            {
                self.by_source.clear();
                self.decoded_bytes = 0;
            }
            self.decoded_bytes = self.decoded_bytes.saturating_add(size);
            self.by_source.insert(src.to_owned(), bitmap.clone());
        }
        Some(bitmap)
    }
}

#[cfg(feature = "canvas-projection")]
fn write_project_image_asset(
    project_dir: &Path,
    sid: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<String, VbError> {
    let root = project_dir.canonicalize()?;
    let assets_dir = root.join("assets");
    if !assets_dir.exists() {
        match fs::create_dir(&assets_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::symlink_metadata(&assets_dir)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(VbError::Conflict(
            "项目 assets 路径必须是项目根目录内的真实目录".into(),
        ));
    }
    let assets_root = assets_dir.canonicalize()?;
    if !assets_root.starts_with(&root) {
        return Err(VbError::Conflict("项目 assets 目录越出项目根目录".into()));
    }

    for suffix in 0..1024_u32 {
        let file_name = if suffix == 0 {
            format!("vb-image-{sid}.{extension}")
        } else {
            format!("vb-image-{sid}-{suffix}.{extension}")
        };
        let destination = assets_root.join(&file_name);
        let mut output = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = output.write_all(bytes).and_then(|()| output.sync_all()) {
            drop(output);
            let _ = fs::remove_file(&destination);
            return Err(error.into());
        }
        return Ok(format!("assets/{file_name}"));
    }

    Err(VbError::Conflict(
        "无法为图像分配唯一的项目资源文件名".into(),
    ))
}

/// 单个项目的宿主无关会话。
pub struct ProjectSession {
    project_dir: PathBuf,
    document: Document,
    warnings: Vec<String>,
    font_faces: Vec<FontFace>,
    synthetic_artboard: bool,
    tools: ToolState,
    align_to: AlignTo,
    grid_visible: bool,
    active_artboard_index: usize,
    animation_playhead_seconds: f64,
    animation_playing: bool,
    animation_looping: bool,
    animation_frame_snap_enabled: bool,
    animation_frame_rate: AnimationFrameRate,
    selected_keyframes: Vec<AnimationKeyframeSelection>,
    selection: SelectionState,
    undo: UndoStack,
    clipboard: Vec<ClipboardEntry>,
    paste_offset: u32,
    #[cfg(feature = "canvas-projection")]
    image_cache: Mutex<ProjectImageCache>,
}

impl std::fmt::Debug for ProjectSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectSession")
            .field("project_dir", &self.project_dir)
            .field("title", &self.document.meta.title)
            .field("revision", &self.document.rev)
            .field("selected_sids", &self.selection.as_slice())
            .field("clipboard_items", &self.clipboard.len())
            .field("active_tool", &self.tools.active())
            .field("align_to", &self.align_to)
            .field("grid_visible", &self.grid_visible)
            .field("active_artboard_index", &self.active_artboard_index)
            .field(
                "animation_playhead_seconds",
                &self.animation_playhead_seconds,
            )
            .field("animation_playing", &self.animation_playing)
            .field("animation_looping", &self.animation_looping)
            .field(
                "animation_frame_snap_enabled",
                &self.animation_frame_snap_enabled,
            )
            .field("animation_frame_rate", &self.animation_frame_rate)
            .field("selected_keyframe_count", &self.selected_keyframes.len())
            .finish_non_exhaustive()
    }
}

impl ProjectSession {
    /// 从项目目录或 `index.html` 建立会话。
    pub fn open(path: impl AsRef<Path>) -> Result<Self, VbError> {
        let imported = import_project(path.as_ref())?;
        Ok(Self::from_import(imported))
    }

    /// 在空目录创建并保存一个默认 1440×900 HTML/CSS 项目。
    ///
    /// 已有非空目录从不覆盖;用户取消路径选择应由宿主在调用前处理。
    pub fn new_project(path: impl AsRef<Path>) -> Result<Self, VbError> {
        let project_dir = path.as_ref();
        if project_dir.as_os_str().is_empty() {
            return Err(VbError::Parse("新项目目录不能为空".into()));
        }
        if project_dir.exists() {
            if !project_dir.is_dir() {
                return Err(VbError::Parse(format!(
                    "新项目路径不是目录: {}",
                    project_dir.display()
                )));
            }
            let mut entries = std::fs::read_dir(project_dir)?;
            if entries.next().transpose()?.is_some() {
                return Err(VbError::Parse(format!(
                    "新项目目录必须为空，不覆盖已有文件: {}",
                    project_dir.display()
                )));
            }
        } else {
            std::fs::create_dir_all(project_dir)?;
        }

        let title = project_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| "未命名".into());
        let mut session = Self::from_document(
            project_dir.to_path_buf(),
            Document::new(&title, "zh-CN"),
            Vec::new(),
            Vec::new(),
            false,
        );
        session.save()?;
        Ok(session)
    }

    fn from_import(imported: ImportResult) -> Self {
        Self::from_document(
            imported.project_dir,
            imported.doc,
            imported.warnings,
            imported.font_faces,
            imported.synthetic_artboard,
        )
    }

    fn from_document(
        project_dir: PathBuf,
        document: Document,
        warnings: Vec<String>,
        font_faces: Vec<FontFace>,
        synthetic_artboard: bool,
    ) -> Self {
        Self {
            project_dir,
            document,
            warnings,
            font_faces,
            synthetic_artboard,
            tools: ToolState::new(ToolId::Select),
            align_to: AlignTo::default(),
            grid_visible: false,
            active_artboard_index: 0,
            animation_playhead_seconds: 0.0,
            animation_playing: false,
            animation_looping: false,
            animation_frame_snap_enabled: true,
            animation_frame_rate: AnimationFrameRate::Fps30,
            selected_keyframes: Vec::new(),
            selection: SelectionState::new(),
            undo: UndoStack::new(),
            clipboard: Vec::new(),
            paste_offset: 0,
            #[cfg(feature = "canvas-projection")]
            image_cache: Mutex::new(ProjectImageCache::default()),
        }
    }

    /// 项目标题(来自 HTML `<title>` / 文档元信息)。
    pub fn title(&self) -> &str {
        &self.document.meta.title
    }

    /// 导入后的规范项目根目录。
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    /// 导入阶段的可见降级或解析告警。
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// 项目内声明的 Web 字体,后续渲染接线使用。
    pub fn font_faces(&self) -> &[FontFace] {
        &self.font_faces
    }

    /// 导入器是否为缺少显式标记的 HTML 合成了画板。
    pub fn has_synthetic_artboard(&self) -> bool {
        self.synthetic_artboard
    }

    /// 当前活跃工具;损坏/未初始化状态安全回退到选择工具。
    pub fn active_tool(&self) -> ToolId {
        self.tools.active().unwrap_or(ToolId::Select)
    }

    /// 当前对齐目标;默认以选区包围盒为准。
    pub fn align_to(&self) -> AlignTo {
        self.align_to
    }

    /// 更新对齐目标;返回目标是否改变。
    pub fn set_align_to(&mut self, target: AlignTo) -> bool {
        if self.align_to == target {
            return false;
        }
        self.align_to = target;
        true
    }

    /// 对齐目标对应的目录命令 id,供 UI 表达激活态而不依赖 vb_tools 类型。
    pub fn align_target_catalog_id(&self) -> &'static str {
        match self.align_to {
            AlignTo::Selection => "align.to_selection",
            AlignTo::KeyObject => "align.to_key_object",
            AlignTo::Artboard => "align.to_artboard",
        }
    }

    /// 当前是否显示画布网格;视图状态不进入文档撤销栈。
    pub fn grid_visible(&self) -> bool {
        self.grid_visible
    }

    /// 当前活跃画板序号;仅视图状态,不进入文档撤销栈。
    pub fn active_artboard_index(&self) -> usize {
        self.active_artboard_index
    }

    /// 当前文档画板数量。
    pub fn artboard_count(&self) -> usize {
        self.document.artboards().len()
    }

    /// 切换画板并清除旧画板选区;返回是否发生切换。
    pub fn set_active_artboard(&mut self, index: usize) -> Result<bool, VbError> {
        if index >= self.document.artboards().len() {
            return Err(VbError::NoSuchNode(format!("artboard index {index}")));
        }
        if self.active_artboard_index == index {
            return Ok(false);
        }
        self.active_artboard_index = index;
        self.selection.clear();
        self.selected_keyframes.clear();
        self.animation_playhead_seconds = 0.0;
        self.animation_playing = false;
        Ok(true)
    }

    /// 循环到下一画板;单画板文档返回 `false`。
    pub fn next_artboard(&mut self) -> Result<bool, VbError> {
        let count = self.artboard_count();
        if count == 0 {
            return Err(VbError::NoSuchNode("document has no artboards".into()));
        }
        self.set_active_artboard((self.active_artboard_index + 1) % count)
    }

    /// 循环到上一画板;单画板文档返回 `false`。
    pub fn previous_artboard(&mut self) -> Result<bool, VbError> {
        let count = self.artboard_count();
        if count == 0 {
            return Err(VbError::NoSuchNode("document has no artboards".into()));
        }
        let previous = if self.active_artboard_index == 0 {
            count - 1
        } else {
            self.active_artboard_index - 1
        };
        self.set_active_artboard(previous)
    }

    /// 当前有序选择集(稳定 sid),只读暴露给视图投影。
    pub fn selected_sids(&self) -> &[String] {
        self.selection.as_slice()
    }

    /// 输出单选检查器投影;多选只返回数量,不将第一个对象伪装成批量值。
    pub fn inspector_snapshot(&self) -> InspectorSnapshot {
        let selected_sids = self.selection.as_slice();
        let selected = if selected_sids.len() == 1 {
            let sid = &selected_sids[0];
            self.document
                .find_by_sid(sid)
                .and_then(|id| self.document.node(id))
                .map(|node| {
                    let fill_property = inspector_fill_property(node);
                    let fill = match &node.kind {
                        NodeKind::Box => complex_background_value(node)
                            .or_else(|| node.style_get("background-color"))
                            .or_else(|| node.style_get("background"))
                            .or_else(|| node.style_get("background-image")),
                        NodeKind::Text { .. } => node.style_get("color"),
                        _ => None,
                    }
                    .map(str::to_owned);
                    let fill_editable = !node.locked
                        && fill_property.is_some()
                        && fill_property
                            .and_then(|property| node.style_get(property))
                            .is_none_or(|value| parse_color(value).is_some());
                    SelectedNodeSnapshot {
                        sid: sid.clone(),
                        name: node.name.clone(),
                        kind: node.kind.kind_name(),
                        geometry: CanvasRect {
                            x: node.geom.x,
                            y: node.geom.y,
                            width: node.geom.w,
                            height: node.geom.h,
                        },
                        fill,
                        fill_editable,
                        text: node.text().map(str::to_owned),
                        text_editable: matches!(
                            &node.kind,
                            NodeKind::Text { text, segments, .. }
                                if segments.is_empty()
                                    && !text.contains('\n') && !text.contains('\r')
                        ),
                        locked: node.locked,
                    }
                })
        } else {
            None
        };
        InspectorSnapshot {
            selected_count: selected_sids.len(),
            selected,
        }
    }

    /// 列出文档中通过元数据与标准 @keyframes 双重校验的共享动画预设。
    pub fn available_animation_presets(&self) -> Vec<AnimationPresetRef> {
        parse_animation_presets(&self.document.raw_css)
            .into_iter()
            .map(|preset| preset.reference)
            .collect()
    }

    fn animation_preset(&self, id: &str) -> Option<AnimationPreset> {
        parse_animation_presets(&self.document.raw_css)
            .into_iter()
            .find(|preset| preset.reference.id == id)
    }

    fn unique_animation_preset_ref(
        &self,
        display_name: &str,
    ) -> Result<AnimationPresetRef, VbError> {
        let display_name = display_name.trim();
        if AnimationPresetRef::new("preset", display_name).is_none() {
            return Err(VbError::Parse(
                "预设名称不能为空且不能超过 120 个字符".into(),
            ));
        }
        let mut slug = String::new();
        let mut separator = false;
        for ch in display_name.chars() {
            if ch.is_ascii_alphanumeric() {
                if separator && !slug.is_empty() && !slug.ends_with('-') {
                    slug.push('-');
                }
                slug.push(ch.to_ascii_lowercase());
                separator = false;
            } else {
                separator = true;
            }
        }
        slug = slug.trim_matches('-').to_string();
        if slug.is_empty() {
            let hash = display_name
                .as_bytes()
                .iter()
                .fold(0xcbf29ce484222325u64, |mut h, byte| {
                    h ^= u64::from(*byte);
                    h.wrapping_mul(0x100000001b3)
                });
            slug = format!("preset-{hash:016x}");
        }
        if !slug.as_bytes()[0].is_ascii_lowercase() {
            slug.insert_str(0, "p-");
        }
        slug.truncate(38);
        let existing: HashSet<String> = self
            .available_animation_presets()
            .into_iter()
            .map(|reference| reference.id)
            .collect();
        for sequence in 1..=10_000u32 {
            let suffix = if sequence == 1 {
                String::new()
            } else {
                format!("-{sequence}")
            };
            let keep = 48usize.saturating_sub(suffix.len());
            let candidate = format!("{}{}", &slug[..slug.len().min(keep)], suffix);
            let metadata_prefix = format!("@vb-animation-preset-v1 {candidate} ");
            if !existing.contains(&candidate)
                && !self
                    .document
                    .raw_css
                    .iter()
                    .any(|block| block.trim_start().starts_with(&metadata_prefix))
            {
                return AnimationPresetRef::new(candidate, display_name)
                    .ok_or_else(|| VbError::Parse("无效的动画预设标识".into()));
            }
        }
        Err(VbError::Conflict("无法生成唯一的动画预设标识".into()))
    }

    /// 将当前单对象 Timeline 另存为共享预设;不改变该对象的本地动画。
    pub fn save_selected_animation_as_preset(
        &mut self,
        display_name: &str,
    ) -> Result<AnimationPresetRef, VbError> {
        let selected = self
            .animation_snapshot()
            .selected
            .ok_or_else(|| VbError::Conflict("保存动画预设需要单对象选择".into()))?;
        if selected.locked {
            return Err(VbError::Conflict(format!("对象 {} 已锁定", selected.sid)));
        }
        self.validate_unlocked_ancestors(
            self.document
                .find_by_sid(&selected.sid)
                .ok_or_else(|| VbError::NoSuchNode(selected.sid.clone()))?,
        )?;
        let AnimSource::Timeline(mut model) = selected.source else {
            return Err(VbError::Unsupported(
                "只能将本地时间轴动画另存为预设".into(),
            ));
        };
        if model.is_empty() {
            return Err(VbError::Conflict("空动画不能保存为预设".into()));
        }
        let reference = self.unique_animation_preset_ref(display_name)?;
        model.preset = Some(reference.clone());
        validate_animation_model(&model)?;
        let preset = AnimationPreset {
            reference: reference.clone(),
            model,
        };
        let (metadata, keyframes) = serialize_preset_blocks(&preset)
            .ok_or_else(|| VbError::Parse("无法序列化动画预设".into()))?;
        self.undo.end_session();
        self.undo.push(
            &mut self.document,
            Command::SetAnimationPreset {
                id: reference.id.clone(),
                new_metadata: Some(metadata),
                new_keyframes: Some(keyframes),
                old: None,
            },
        )?;
        Ok(reference)
    }

    /// 将共享预设应用到当前选择;共享 @keyframes 不复制到节点私有块。
    pub fn apply_animation_preset_to_selection(&mut self, id: &str) -> Result<usize, VbError> {
        let preset = self
            .animation_preset(id)
            .ok_or_else(|| VbError::NoSuchNode(format!("animation preset {id}")))?;
        let selected = self.selection.as_slice();
        if selected.is_empty() {
            return Err(VbError::Conflict("应用动画预设需要对象选择".into()));
        }
        let animation = serialize_animation_named(&preset_keyframes_name(id), &preset.model);
        let mut commands = Vec::new();
        for sid in selected {
            let node_id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            self.validate_unlocked_ancestors(node_id)?;
            let node = self
                .document
                .node(node_id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            if node.locked || matches!(&node.kind, NodeKind::Artboard) {
                return Err(VbError::Conflict(format!("对象 {sid} 不支持动画预设")));
            }
            if node.style_get("animation") == Some(animation.as_str()) {
                continue;
            }
            commands.push(Command::SetNodeAnimation {
                sid: sid.clone(),
                new_keyframes: None,
                new_animation: Some(animation.clone()),
                old: None,
            });
        }
        if commands.is_empty() {
            return Ok(0);
        }
        let count = commands.len();
        self.undo.end_session();
        let command = if count == 1 {
            commands.pop().expect("count checked")
        } else {
            Command::Compound { cmds: commands }
        };
        self.undo.push(&mut self.document, command)?;
        self.selected_keyframes.clear();
        self.animation_playhead_seconds = 0.0;
        self.animation_playing = false;
        Ok(count)
    }

    /// 删除未被节点、断点、伪类或其他 raw CSS 引用的共享预设。
    pub fn delete_animation_preset(&mut self, id: &str) -> Result<bool, VbError> {
        self.animation_preset(id)
            .ok_or_else(|| VbError::NoSuchNode(format!("animation preset {id}")))?;
        let name = preset_keyframes_name(id);
        let references = |value: &str| {
            value
                .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, ',' | ';' | '(' | ')'))
                .any(|part| part == name)
        };
        let referenced_by_nodes = self.document.nodes.values().any(|node| {
            node.style.iter().any(|declaration| {
                declaration.prop == "animation" && references(&declaration.value)
            })
        });
        let referenced_by_media = self.document.media_rules.iter().any(|rule| {
            rule.decls.iter().any(|declaration| {
                declaration.prop == "animation" && references(&declaration.value)
            })
        });
        let referenced_by_pseudo = self.document.pseudo_rules.iter().any(|rule| {
            rule.decls.iter().any(|declaration| {
                declaration.prop == "animation" && references(&declaration.value)
            })
        });
        let referenced_by_rules = referenced_by_media || referenced_by_pseudo;
        let metadata_prefix = format!("@vb-animation-preset-v1 {id} ");
        let referenced_by_raw_css = self.document.raw_css.iter().any(|block| {
            let trimmed = block.trim_start();
            !trimmed.starts_with(&metadata_prefix)
                && !trimmed.starts_with(&format!("@keyframes {name}"))
                && references(trimmed)
        });
        if referenced_by_nodes || referenced_by_rules || referenced_by_raw_css {
            return Err(VbError::Conflict(format!(
                "动画预设 {id} 仍被引用，不能删除"
            )));
        }
        self.undo.end_session();
        self.undo.push(
            &mut self.document,
            Command::SetAnimationPreset {
                id: id.to_string(),
                new_metadata: None,
                new_keyframes: None,
                old: None,
            },
        )?;
        Ok(true)
    }

    /// 输出当前单选对象的动画模型;外部命名 CSS 动画保留为只读状态.
    pub fn animation_snapshot(&self) -> AnimationSnapshot {
        let selected_sids = self.selection.as_slice();
        let selected = if selected_sids.len() == 1 {
            let sid = &selected_sids[0];
            self.document
                .find_by_sid(sid)
                .and_then(|id| self.document.node(id))
                .map(|node| SelectedAnimationSnapshot {
                    sid: sid.clone(),
                    name: node.name.clone(),
                    locked: node.locked,
                    source: parse_anim_source(&self.document, sid),
                    static_values: static_values(&self.document, sid),
                })
        } else {
            None
        };
        let duration_seconds = selected.as_ref().and_then(|selected| {
            let AnimSource::Timeline(model) = &selected.source else {
                return None;
            };
            let duration = model.duration_ms / 1000.0;
            let delay = model.delay_ms / 1000.0;
            let iterations = if model.iterations.is_finite() {
                model.iterations.max(1.0)
            } else {
                1.0
            };
            let total = delay + duration * iterations;
            (duration.is_finite() && duration > 0.0 && total.is_finite() && total > 0.0)
                .then_some(total)
        });
        let playhead_seconds = duration_seconds
            .map(|duration| self.animation_playhead_seconds.clamp(0.0, duration))
            .unwrap_or(0.0);
        let selected_keyframes = selected
            .as_ref()
            .and_then(|selected| {
                let AnimSource::Timeline(model) = &selected.source else {
                    return None;
                };
                Some(
                    self.selected_keyframes
                        .iter()
                        .filter(|selected_keyframe| {
                            selected_keyframe.sid == selected.sid
                                && model.tracks.iter().any(|(track, frames)| {
                                    *track == selected_keyframe.track
                                        && frames.iter().any(|frame| {
                                            keyframe_offset_permille(
                                                frame.t,
                                                model.duration_ms / 1000.0,
                                            ) == Some(selected_keyframe.offset_permille)
                                        })
                                })
                        })
                        .cloned()
                        .collect(),
                )
            })
            .unwrap_or_default();
        AnimationSnapshot {
            selected_count: selected_sids.len(),
            selected,
            presets: self.available_animation_presets(),
            selected_keyframes,
            duration_seconds,
            playhead_seconds,
            playing: self.animation_playing && duration_seconds.is_some(),
            looping: self.animation_looping,
        }
    }

    /// 调整会话播放头;不触碰 Document revision 或 Undo。
    pub fn set_animation_playhead_seconds(&mut self, seconds: f64) -> Result<bool, VbError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(VbError::Parse("动画播放头必须是有限非负秒数".into()));
        }
        let duration = self
            .animation_snapshot()
            .duration_seconds
            .ok_or_else(|| VbError::Unsupported("当前选择没有可播放的本地动画".into()))?;
        let next = seconds.min(duration);
        if (next - self.animation_playhead_seconds).abs() <= 1e-9 && !self.animation_playing {
            return Ok(false);
        }
        self.animation_playhead_seconds = next;
        self.animation_playing = false;
        Ok(true)
    }

    /// 设置或切换播放状态;无有效单选动画时拒绝启动。
    pub fn set_animation_playing(&mut self, playing: bool) -> Result<bool, VbError> {
        if !playing {
            if !self.animation_playing {
                return Ok(false);
            }
            self.animation_playing = false;
            return Ok(true);
        }
        let duration = self
            .animation_snapshot()
            .duration_seconds
            .ok_or_else(|| VbError::Unsupported("当前选择没有可播放的本地动画".into()))?;
        if self.animation_playing {
            return Ok(false);
        }
        if self.animation_playhead_seconds >= duration {
            self.animation_playhead_seconds = 0.0;
        }
        self.animation_playing = true;
        Ok(true)
    }

    /// 切换播放/暂停状态。
    pub fn toggle_animation_playback(&mut self) -> Result<bool, VbError> {
        self.set_animation_playing(!self.animation_playing)
    }

    /// 停止并回到动画起点。
    pub fn stop_animation_playback(&mut self) -> bool {
        let changed = self.animation_playing || self.animation_playhead_seconds != 0.0;
        self.animation_playing = false;
        self.animation_playhead_seconds = 0.0;
        changed
    }

    /// 设置是否循环;该状态仅属于当前会话视图。
    pub fn set_animation_looping(&mut self, looping: bool) -> bool {
        if self.animation_looping == looping {
            return false;
        }
        self.animation_looping = looping;
        true
    }

    pub fn animation_frame_snap_enabled(&self) -> bool {
        self.animation_frame_snap_enabled
    }

    pub fn animation_frame_rate(&self) -> AnimationFrameRate {
        self.animation_frame_rate
    }

    pub fn set_animation_frame_snap_enabled(&mut self, enabled: bool) -> bool {
        if self.animation_frame_snap_enabled == enabled {
            return false;
        }
        self.animation_frame_snap_enabled = enabled;
        true
    }

    pub fn set_animation_frame_rate(&mut self, frame_rate: AnimationFrameRate) -> bool {
        if self.animation_frame_rate == frame_rate {
            return false;
        }
        self.animation_frame_rate = frame_rate;
        true
    }

    /// Quantize one shared keyframe delta to the selected animation's frame grid.
    ///
    /// # Errors
    /// Returns an error unless exactly one editable local animation is selected.
    pub fn snap_selected_keyframe_delta(
        &self,
        delta: KeyframeOffsetDelta,
    ) -> Result<KeyframeOffsetDelta, VbError> {
        if !self.animation_frame_snap_enabled || delta.permille() == 0 {
            return Ok(delta);
        }
        let selected = self
            .animation_snapshot()
            .selected
            .ok_or_else(|| VbError::Conflict("帧吸附需要单对象动画选择".into()))?;
        if selected.locked {
            return Err(VbError::Conflict(format!("对象 {} 已锁定", selected.sid)));
        }
        let AnimSource::Timeline(model) = selected.source else {
            return Err(VbError::Unsupported("帧吸附仅支持本地时间轴动画".into()));
        };
        let duration_seconds = model.duration_ms / 1000.0;
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            return Err(VbError::Parse("动画时长无效".into()));
        }
        let fps = f64::from(self.animation_frame_rate.fps());
        let requested_seconds = f64::from(delta.permille()) * duration_seconds / 1000.0;
        let snapped_seconds = (requested_seconds * fps).round() / fps;
        let snapped_permille = (snapped_seconds / duration_seconds * 1000.0).round();
        if !snapped_permille.is_finite() || !(-1000.0..=1000.0).contains(&snapped_permille) {
            return Err(VbError::Parse("帧吸附结果超出时间轴范围".into()));
        }
        #[allow(clippy::cast_possible_truncation)]
        KeyframeOffsetDelta::new(snapped_permille as i32)
    }

    /// 推进播放头,由宿主动画帧循环调用;返回是否发生变化。
    pub fn advance_animation_playback(&mut self, delta_seconds: f64) -> Result<bool, VbError> {
        if !delta_seconds.is_finite() || delta_seconds < 0.0 {
            return Err(VbError::Parse("动画帧增量必须是有限非负秒数".into()));
        }
        if !self.animation_playing || delta_seconds == 0.0 {
            return Ok(false);
        }
        let Some(duration) = self.animation_snapshot().duration_seconds else {
            self.animation_playing = false;
            return Ok(false);
        };
        let next = self.animation_playhead_seconds + delta_seconds;
        if !next.is_finite() {
            return Err(VbError::Parse("动画播放头超出有限范围".into()));
        }
        if next >= duration {
            if self.animation_looping {
                self.animation_playhead_seconds = next.rem_euclid(duration);
            } else {
                self.animation_playhead_seconds = duration;
                self.animation_playing = false;
            }
        } else {
            self.animation_playhead_seconds = next;
        }
        Ok(true)
    }

    /// 为当前单选对象创建可编辑的一秒水平缓动预设。
    pub fn create_default_animation(&mut self) -> Result<bool, VbError> {
        let snapshot = self.animation_snapshot();
        let selected = snapshot
            .selected
            .ok_or_else(|| VbError::Conflict("动画操作需要单对象选择".into()))?;
        if selected.locked {
            return Err(VbError::Conflict(format!("对象 {} 已锁定", selected.sid)));
        }
        match &selected.source {
            AnimSource::Timeline(_) => return Ok(false),
            AnimSource::Foreign(name) => {
                return Err(VbError::Unsupported(format!(
                    "外部命名动画 {name} 保持只读"
                )));
            }
            AnimSource::None => {}
        }
        let model = AnimModel {
            duration_ms: 1_000.0,
            delay_ms: 0.0,
            iterations: 1.0,
            tracks: vec![(
                TrackProp::Position,
                vec![
                    Keyframe {
                        t: 0.0,
                        value: KeyValue::Offset(0.0, 0.0),
                        easing: Easing::EaseInOut,
                    },
                    Keyframe {
                        t: 1.0,
                        value: KeyValue::Offset(48.0, 0.0),
                        easing: Easing::EaseInOut,
                    },
                ],
            )],
            extra: Vec::new(),
            preset: None,
        };
        self.set_selected_animation(&selected.sid, &model)
    }

    /// 清除当前单选对象的时间轴动画;原有外部 CSS 动画保持只读。
    pub fn clear_selected_animation(&mut self) -> Result<bool, VbError> {
        let selected = self
            .animation_snapshot()
            .selected
            .ok_or_else(|| VbError::Conflict("动画操作需要单对象选择".into()))?;
        self.set_selected_animation(&selected.sid, &AnimModel::default())
    }

    /// 写回当前单选对象的动画模型;外部动画和锁定节点不可覆盖。
    pub fn set_selected_animation(
        &mut self,
        sid: &str,
        model: &AnimModel,
    ) -> Result<bool, VbError> {
        let [selected_sid] = self.selection.as_slice() else {
            return Err(VbError::Conflict("动画检查器仅支持单对象选择".into()));
        };
        if selected_sid != sid {
            return Err(VbError::Conflict("动画编辑对象已不再处于选择状态".into()));
        }
        let id = self
            .document
            .find_by_sid(sid)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        self.validate_unlocked_ancestors(id)?;
        let node = self
            .document
            .node(id)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        if node.locked {
            return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
        }
        if matches!(&node.kind, NodeKind::Artboard) {
            return Err(VbError::Unsupported("画板不能添加对象动画".into()));
        }

        let source = parse_anim_source(&self.document, sid);
        if let AnimSource::Foreign(name) = &source {
            return Err(VbError::Unsupported(format!(
                "外部命名动画 {name} 保持只读"
            )));
        }
        if !model.is_empty() {
            validate_animation_model(model)?;
        }
        if let Some(reference) = &model.preset {
            let AnimSource::Timeline(current) = &source else {
                return Err(VbError::Conflict("共享动画预设引用已失效".into()));
            };
            if current.preset.as_ref() != Some(reference) {
                return Err(VbError::Conflict(
                    "不能通过另一个节点改写共享动画预设".into(),
                ));
            }
            if current == model {
                return Ok(false);
            }
            if model.is_empty() {
                return Err(VbError::Conflict("命名动画预设不能为空".into()));
            }
            let preset = AnimationPreset {
                reference: reference.clone(),
                model: model.clone(),
            };
            let (new_metadata, new_keyframes) = serialize_preset_blocks(&preset)
                .ok_or_else(|| VbError::Parse("无法序列化共享动画预设".into()))?;
            let animation_name = preset_keyframes_name(&reference.id);
            let animation_value = serialize_animation_named(&animation_name, model);
            let uses_name = |value: &str| {
                value
                    .split(|ch: char| ch.is_ascii_whitespace() || matches!(ch, ',' | ';'))
                    .any(|token| {
                        token.trim_matches(|ch| matches!(ch, '\'' | '"')) == animation_name
                    })
            };
            let mut referenced_nodes = Vec::new();
            for (node_id, node) in self.document.nodes.iter() {
                let Some(current_animation) = node.style_get("animation") else {
                    continue;
                };
                if !uses_name(current_animation) {
                    continue;
                }
                if current_animation.contains(',') {
                    return Err(VbError::Unsupported(
                        "包含多个并行动画的对象暂不支持全局预设编辑".into(),
                    ));
                }
                if node.locked || matches!(&node.kind, NodeKind::Artboard) {
                    return Err(VbError::Conflict(format!(
                        "共享动画预设引用了不可编辑的对象 {}",
                        node.sid.as_str()
                    )));
                }
                self.validate_unlocked_ancestors(node_id)?;
                referenced_nodes.push((node.sid.as_str().to_owned(), current_animation.to_owned()));
            }
            if !referenced_nodes
                .iter()
                .any(|(reference_sid, _)| reference_sid == sid)
            {
                return Err(VbError::Conflict("共享动画预设引用与文档不一致".into()));
            }
            self.undo.end_session();
            let mut commands = vec![Command::SetAnimationPreset {
                id: reference.id.clone(),
                new_metadata: Some(new_metadata),
                new_keyframes: Some(new_keyframes),
                old: None,
            }];
            for (reference_sid, current_animation) in referenced_nodes {
                if current_animation != animation_value {
                    commands.push(Command::SetNodeAnimation {
                        sid: reference_sid,
                        new_keyframes: None,
                        new_animation: Some(animation_value.clone()),
                        old: None,
                    });
                }
            }
            let command = if commands.len() == 1 {
                commands.remove(0)
            } else {
                Command::Compound { cmds: commands }
            };
            self.undo.push(&mut self.document, command)?;
            self.selected_keyframes.clear();
            self.animation_playhead_seconds = 0.0;
            self.animation_playing = false;
            return Ok(true);
        }
        match &source {
            AnimSource::Timeline(current) if current == model => return Ok(false),
            AnimSource::None if model.is_empty() => return Ok(false),
            _ => {}
        }
        let (new_keyframes, new_animation) = if model.is_empty() {
            (None, None)
        } else {
            (
                Some(serialize_keyframes(sid, model)),
                Some(serialize_animation(sid, model)),
            )
        };
        self.undo.end_session();
        self.undo.push(
            &mut self.document,
            Command::SetNodeAnimation {
                sid: sid.to_owned(),
                new_keyframes,
                new_animation,
                old: None,
            },
        )?;
        self.selected_keyframes.clear();
        self.animation_playhead_seconds = 0.0;
        self.animation_playing = false;
        Ok(true)
    }

    /// 在当前时间轴播放头补齐缺失的四轨静态值关键帧;外部动画只读。
    pub fn add_keyframes_at_playhead(&mut self) -> Result<usize, VbError> {
        let selected = self
            .animation_snapshot()
            .selected
            .ok_or_else(|| VbError::Conflict("关键帧操作需要单对象选择".into()))?;
        if selected.locked {
            return Err(VbError::Conflict(format!("对象 {} 已锁定", selected.sid)));
        }
        let AnimSource::Timeline(mut model) = selected.source else {
            return Err(VbError::Unsupported("请先为对象创建本地时间轴动画".into()));
        };
        validate_animation_model(&model)?;
        let duration_seconds = model.duration_ms / 1000.0;
        let delay_seconds = model.delay_ms / 1000.0;
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            return Err(VbError::Parse("动画时长无效".into()));
        }
        let total = self
            .animation_snapshot()
            .duration_seconds
            .ok_or_else(|| VbError::Unsupported("当前动画没有可用时长".into()))?;
        let playhead = self.animation_playhead_seconds.clamp(0.0, total);
        let local_time = if playhead < delay_seconds {
            0.0
        } else if playhead >= total && model.iterations.is_finite() {
            duration_seconds
        } else {
            (playhead - delay_seconds).rem_euclid(duration_seconds)
        };
        let offset = keyframe_offset_permille(local_time, duration_seconds)
            .ok_or_else(|| VbError::Parse("播放头无法映射到关键帧停靠点".into()))?;
        let easing = model
            .tracks
            .iter()
            .flat_map(|(_, frames)| frames)
            .find(|frame| {
                keyframe_offset_permille(frame.t, model.duration_ms / 1000.0) == Some(offset)
            })
            .map(|frame| frame.easing)
            .unwrap_or(Easing::Linear);
        let ((x, y), scale, opacity, rotation) = static_values(&self.document, &selected.sid);
        let values = [
            (TrackProp::Position, KeyValue::Offset(x, y)),
            (TrackProp::Scale, KeyValue::Factor(scale)),
            (TrackProp::Opacity, KeyValue::Alpha(opacity)),
            (TrackProp::Rotate, KeyValue::Angle(rotation)),
        ];
        let mut added = Vec::new();
        for (track, value) in values {
            if model.frames_of(track).iter().any(|frame| {
                keyframe_offset_permille(frame.t, model.duration_ms / 1000.0) == Some(offset)
            }) {
                continue;
            }
            let frame = Keyframe {
                t: local_time,
                value,
                easing,
            };
            if let Some((_, frames)) = model.tracks.iter_mut().find(|(prop, _)| *prop == track) {
                frames.push(frame);
                frames.sort_by(|a, b| a.t.total_cmp(&b.t));
            } else {
                model.tracks.push((track, vec![frame]));
                model.tracks.sort_by_key(|(prop, _)| {
                    TrackProp::ALL
                        .iter()
                        .position(|candidate| candidate == prop)
                        .unwrap_or(usize::MAX)
                });
            }
            added.push(AnimationKeyframeSelection {
                sid: selected.sid.clone(),
                track,
                offset_permille: offset,
            });
        }
        if added.is_empty() {
            return Ok(0);
        }
        self.set_selected_animation(&selected.sid, &model)?;
        self.animation_playhead_seconds = playhead;
        self.selected_keyframes = added;
        Ok(self.selected_keyframes.len())
    }

    /// 删除会话中选中的关键帧;底层写入仍由一条可撤销的 SetNodeAnimation 完成。
    pub fn delete_selected_keyframes(&mut self) -> Result<usize, VbError> {
        let selected = self
            .animation_snapshot()
            .selected
            .ok_or_else(|| VbError::Conflict("关键帧操作需要单对象选择".into()))?;
        if selected.locked {
            return Err(VbError::Conflict(format!("对象 {} 已锁定", selected.sid)));
        }
        let AnimSource::Timeline(mut model) = selected.source else {
            return Err(VbError::Unsupported(
                "当前对象没有可编辑的本地时间轴".into(),
            ));
        };
        let selected_keys: Vec<_> = self
            .selected_keyframes
            .iter()
            .filter(|key| key.sid == selected.sid)
            .cloned()
            .collect();
        if selected_keys.is_empty() {
            return Ok(0);
        }
        let before = self.animation_playhead_seconds;
        let mut removed = 0;
        for (track, frames) in &mut model.tracks {
            let old_len = frames.len();
            frames.retain(|frame| {
                let Some(offset) = keyframe_offset_permille(frame.t, model.duration_ms / 1000.0)
                else {
                    return true;
                };
                !selected_keys
                    .iter()
                    .any(|key| key.track == *track && key.offset_permille == offset)
            });
            removed += old_len - frames.len();
        }
        model.tracks.retain(|(_, frames)| !frames.is_empty());
        if removed == 0 {
            self.selected_keyframes.clear();
            return Ok(0);
        }
        self.set_selected_animation(&selected.sid, &model)?;
        self.selected_keyframes.clear();
        if let Some(duration) = self.animation_snapshot().duration_seconds {
            self.animation_playhead_seconds = before.min(duration);
        }
        Ok(removed)
    }

    /// 选择本地动画中的某个关键帧;Shift/平台修饰符由调用方映射到 `additive`。
    pub fn select_animation_keyframe(
        &mut self,
        sid: &str,
        track: TrackProp,
        offset_permille: u16,
        additive: bool,
    ) -> Result<bool, VbError> {
        if offset_permille > 1000 {
            return Err(VbError::Parse("关键帧位置必须位于 0..=1000‰".into()));
        }
        let [selected_sid] = self.selection.as_slice() else {
            return Err(VbError::Conflict("关键帧选择需要单对象选择".into()));
        };
        if selected_sid != sid {
            return Err(VbError::Conflict("关键帧所属对象不再处于选择状态".into()));
        }
        let snapshot = self.animation_snapshot();
        let selected = snapshot
            .selected
            .ok_or_else(|| VbError::Conflict("动画对象不存在".into()))?;
        if selected.locked {
            return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
        }
        let AnimSource::Timeline(model) = selected.source else {
            return Err(VbError::Unsupported("关键帧仅支持本地时间轴动画".into()));
        };
        let exists = model.frames_of(track).iter().any(|frame| {
            keyframe_offset_permille(frame.t, model.duration_ms / 1000.0) == Some(offset_permille)
        });
        if !exists {
            return Err(VbError::NoSuchNode(format!(
                "keyframe {sid}/{track:?}/{offset_permille}"
            )));
        }
        let key = AnimationKeyframeSelection {
            sid: sid.to_owned(),
            track,
            offset_permille,
        };
        let selection_before = self.selected_keyframes.clone();
        if additive {
            if let Some(index) = self
                .selected_keyframes
                .iter()
                .position(|existing| existing == &key)
            {
                self.selected_keyframes.remove(index);
            } else {
                self.selected_keyframes.push(key);
            }
        } else {
            self.selected_keyframes.clear();
            self.selected_keyframes.push(key);
        }
        let local = f64::from(offset_permille) / 1000.0 * model.duration_ms / 1000.0;
        let playhead =
            (model.delay_ms / 1000.0 + local).min(snapshot.duration_seconds.unwrap_or(local));
        let changed = self.selected_keyframes != selection_before
            || (self.animation_playhead_seconds - playhead).abs() > 1e-9
            || self.animation_playing;
        self.animation_playhead_seconds = playhead;
        self.animation_playing = false;
        Ok(changed)
    }

    /// 将选中的关键帧应用同一缓动;同一 CSS 停靠点跨轨缓动冲突时原子拒绝。
    pub fn set_selected_keyframe_easing(&mut self, easing: Easing) -> Result<usize, VbError> {
        let [sid] = self.selection.as_slice() else {
            return Err(VbError::Conflict("缓动编辑需要单对象选择".into()));
        };
        let sid = sid.clone();
        let snapshot = self.animation_snapshot();
        let selected = snapshot
            .selected
            .ok_or_else(|| VbError::Conflict("动画对象不存在".into()))?;
        if selected.locked {
            return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
        }
        let AnimSource::Timeline(mut model) = selected.source else {
            return Err(VbError::Unsupported("缓动编辑仅支持本地时间轴动画".into()));
        };
        let selected_keys: Vec<_> = self
            .selected_keyframes
            .iter()
            .filter(|key| key.sid == sid)
            .cloned()
            .collect();
        if selected_keys.is_empty() {
            return Ok(0);
        }

        let duration_seconds = model.duration_ms / 1000.0;
        let mut changed = 0;
        for (track, frames) in &mut model.tracks {
            for frame in frames {
                let Some(offset) = keyframe_offset_permille(frame.t, duration_seconds) else {
                    continue;
                };
                if selected_keys
                    .iter()
                    .any(|key| key.track == *track && key.offset_permille == offset)
                    && frame.easing != easing
                {
                    frame.easing = easing;
                    changed += 1;
                }
            }
        }
        if changed == 0 {
            return Ok(0);
        }
        validate_animation_model(&model)?;

        let playhead = self.animation_playhead_seconds;
        self.set_selected_animation(&sid, &model)?;
        self.animation_playhead_seconds = playhead.min(
            self.animation_snapshot()
                .duration_seconds
                .unwrap_or(playhead),
        );
        self.selected_keyframes = selected_keys;
        Ok(changed)
    }

    /// 预览将同一千分位位移应用到全部已选关键帧;不修改会话或文档。
    ///
    /// # Errors
    /// 返回冲突/只读错误(跨对象、锁定/外部动画、越界或停靠点碰撞)。
    pub fn preview_selected_keyframe_move(
        &self,
        delta: KeyframeOffsetDelta,
    ) -> Result<Vec<AnimationKeyframeSelection>, VbError> {
        if delta.permille() == 0 || self.selected_keyframes.is_empty() {
            return Ok(self.selected_keyframes.clone());
        }
        Ok(self
            .plan_selected_keyframe_move(delta)?
            .map(|plan| plan.moved)
            .unwrap_or_default())
    }

    /// 提交一次多关键帧位移事务;同一轨道目标碰撞、越界、锁定或跨对象选择原子拒绝。
    ///
    /// # Errors
    /// 返回冲突/只读错误;所有验证在 `SetNodeAnimation` 写入前完成。
    pub fn move_selected_keyframes_by(
        &mut self,
        delta: KeyframeOffsetDelta,
    ) -> Result<usize, VbError> {
        if delta.permille() == 0 || self.selected_keyframes.is_empty() {
            return Ok(0);
        }
        let Some(plan) = self.plan_selected_keyframe_move(delta)? else {
            return Ok(0);
        };
        let moved_count = plan.moved.len();
        if !self.set_selected_animation(&plan.sid, &plan.model)? {
            return Ok(0);
        }
        let duration = self
            .animation_snapshot()
            .duration_seconds
            .unwrap_or(plan.playhead_seconds);
        self.animation_playhead_seconds = plan.playhead_seconds.min(duration);
        self.animation_playing = false;
        self.selected_keyframes = plan.moved;
        Ok(moved_count)
    }

    /// Move selected keyframes by a fixed 1% of the local timeline duration.
    ///
    /// # Errors
    /// Returns an error unless `direction` is -1 or 1, or the move violates timeline bounds/collision constraints.
    pub fn nudge_selected_keyframes_by_percent(&mut self, direction: i8) -> Result<usize, VbError> {
        let delta = match direction {
            -1 => -10,
            1 => 10,
            _ => {
                return Err(VbError::Parse(
                    "keyframe nudge direction must be -1 or 1".into(),
                ))
            }
        };
        self.move_selected_keyframes_by(KeyframeOffsetDelta::new(delta)?)
    }

    /// Preview copies of all selected keyframes; the original animation is unchanged.
    ///
    /// # Errors
    /// Returns an error for out-of-range targets, occupied same-track stops, locked/foreign animations, or incompatible cross-track easing.
    pub fn preview_selected_keyframe_copy(
        &self,
        delta: KeyframeOffsetDelta,
    ) -> Result<Vec<AnimationKeyframeSelection>, VbError> {
        if delta.permille() == 0 || self.selected_keyframes.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .plan_selected_keyframe_copy(delta)?
            .map(|plan| plan.copied)
            .unwrap_or_default())
    }

    /// Copy all selected keyframes by one bounded offset in a single undoable transaction.
    ///
    /// # Errors
    /// Returns an error for out-of-range/occupied targets, locked or foreign animations, or model validation failures.
    pub fn duplicate_selected_keyframes_by(
        &mut self,
        delta: KeyframeOffsetDelta,
    ) -> Result<usize, VbError> {
        if delta.permille() == 0 || self.selected_keyframes.is_empty() {
            return Ok(0);
        }
        let Some(plan) = self.plan_selected_keyframe_copy(delta)? else {
            return Ok(0);
        };
        let count = plan.copied.len();
        if count == 0 || !self.set_selected_animation(&plan.sid, &plan.model)? {
            return Ok(0);
        }
        let duration = self
            .animation_snapshot()
            .duration_seconds
            .unwrap_or(plan.playhead_seconds);
        self.animation_playhead_seconds = plan.playhead_seconds.min(duration);
        self.animation_playing = false;
        self.selected_keyframes = plan.copied;
        Ok(count)
    }

    fn plan_selected_keyframe_copy(
        &self,
        delta: KeyframeOffsetDelta,
    ) -> Result<Option<KeyframeCopyPlan>, VbError> {
        if delta.permille() == 0 || self.selected_keyframes.is_empty() {
            return Ok(None);
        }
        // Reuse move planning for the same single-session validation, bounds checks,
        // target uniqueness and complete selected-set normalization.
        let Some(move_plan) = self.plan_selected_keyframe_move(delta)? else {
            return Ok(None);
        };
        let snapshot = self.animation_snapshot();
        let selected = snapshot
            .selected
            .ok_or_else(|| VbError::Conflict("动画对象不存在".into()))?;
        let AnimSource::Timeline(mut model) = selected.source else {
            return Err(VbError::Unsupported("外部 CSS 动画关键帧保持只读".into()));
        };
        let duration = model.duration_ms / 1000.0;
        if !duration.is_finite() || duration <= 0.0 {
            return Err(VbError::Parse("动画时长无效".into()));
        }
        if move_plan.moved.len() != self.selected_keyframes.len() {
            return Err(VbError::Conflict("关键帧复制计划与选区不一致".into()));
        }
        let shift_seconds = f64::from(delta.permille()) / 1000.0 * duration;
        let mut copies = Vec::with_capacity(self.selected_keyframes.len());
        for (source, target) in self.selected_keyframes.iter().zip(&move_plan.moved) {
            if source.sid != move_plan.sid
                || source.sid != target.sid
                || source.track != target.track
            {
                return Err(VbError::Conflict("关键帧复制计划已变化".into()));
            }
            if model.frames_of(source.track).iter().any(|frame| {
                keyframe_offset_permille(frame.t, duration) == Some(target.offset_permille)
            }) {
                return Err(VbError::Conflict("复制目标已存在同轨关键帧".into()));
            }
            let mut frames = model.frames_of(source.track).iter().filter(|frame| {
                keyframe_offset_permille(frame.t, duration) == Some(source.offset_permille)
            });
            let Some(source_frame) = frames.next() else {
                return Err(VbError::NoSuchNode(format!(
                    "keyframe {}/{:?}/{}",
                    source.sid, source.track, source.offset_permille
                )));
            };
            if frames.next().is_some() {
                return Err(VbError::Conflict("同一轨道存在重复关键帧停靠点".into()));
            }
            let mut copy = source_frame.clone();
            copy.t = source_frame.t + shift_seconds;
            if keyframe_offset_permille(copy.t, duration) != Some(target.offset_permille) {
                return Err(VbError::Conflict("关键帧复制后未落在目标停靠点".into()));
            }
            copies.push((source.track, copy));
        }
        for (track, copy) in copies {
            let Some((_, frames)) = model
                .tracks
                .iter_mut()
                .find(|(candidate, _)| *candidate == track)
            else {
                return Err(VbError::Conflict("关键帧轨道已变化".into()));
            };
            frames.push(copy);
            frames.sort_by(|left, right| left.t.total_cmp(&right.t));
        }
        validate_animation_model(&model)?;
        Ok(Some(KeyframeCopyPlan {
            sid: move_plan.sid,
            model,
            copied: move_plan.moved,
            playhead_seconds: self.animation_playhead_seconds,
        }))
    }

    fn plan_selected_keyframe_move(
        &self,
        delta: KeyframeOffsetDelta,
    ) -> Result<Option<KeyframeMovePlan>, VbError> {
        if delta.permille() == 0 || self.selected_keyframes.is_empty() {
            return Ok(None);
        }
        let [sid] = self.selection.as_slice() else {
            return Err(VbError::Conflict("关键帧移动需要单对象选择".into()));
        };
        let sid = sid.clone();
        if self.selected_keyframes.iter().any(|key| key.sid != sid) {
            return Err(VbError::Conflict("关键帧选择含有其他对象".into()));
        }
        let node_id = self
            .document
            .find_by_sid(&sid)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        self.validate_unlocked_ancestors(node_id)?;
        let snapshot = self.animation_snapshot();
        let selected = snapshot
            .selected
            .ok_or_else(|| VbError::Conflict("动画对象不存在".into()))?;
        if selected.locked {
            return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
        }
        let AnimSource::Timeline(mut model) = selected.source else {
            return Err(VbError::Unsupported("外部动画关键帧保持只读".into()));
        };
        validate_animation_model(&model)?;
        let duration = model.duration_ms / 1000.0;
        if !duration.is_finite() || duration <= 0.0 {
            return Err(VbError::Parse("动画时长无效".into()));
        }

        // (track, frame index, source offset, target offset, exact source time)
        let mut planned: Vec<(TrackProp, usize, u16, u16, f64)> = Vec::new();
        for key in &self.selected_keyframes {
            if key.offset_permille > 1000 {
                return Err(VbError::Parse("关键帧位置必须位于 0..=1000‰".into()));
            }
            if planned.iter().any(|(track, _, source, _, _)| {
                *track == key.track && *source == key.offset_permille
            }) {
                return Err(VbError::Conflict("重复关键帧选区".into()));
            }
            let mut matches = model
                .frames_of(key.track)
                .iter()
                .enumerate()
                .filter(|(_, frame)| {
                    keyframe_offset_permille(frame.t, duration) == Some(key.offset_permille)
                });
            let Some((frame_index, frame)) = matches.next() else {
                return Err(VbError::NoSuchNode(format!(
                    "keyframe {sid}/{:?}/{}",
                    key.track, key.offset_permille
                )));
            };
            if matches.next().is_some() {
                return Err(VbError::Conflict("同一轨道存在重复关键帧停靠点".into()));
            }
            let original_time = frame.t;
            let target = i32::from(key.offset_permille)
                .checked_add(i32::from(delta.permille()))
                .filter(|target| (0..=1000).contains(target))
                .ok_or_else(|| VbError::Conflict("关键帧移动不能越过时间轴边界".into()))?
                as u16;
            if planned.iter().any(|(track, _, _, prior_target, _)| {
                *track == key.track && *prior_target == target
            }) {
                return Err(VbError::Conflict("移动后的同轨关键帧停靠点重复".into()));
            }
            planned.push((
                key.track,
                frame_index,
                key.offset_permille,
                target,
                original_time,
            ));
        }

        // Validate against the complete moving set so selected keys can cross each
        // other's original positions without being mistaken for stationary collisions.
        for (track, _, _, target, _) in &planned {
            let moving_indices: Vec<usize> = planned
                .iter()
                .filter(|(candidate_track, _, _, _, _)| candidate_track == track)
                .map(|(_, index, _, _, _)| *index)
                .collect();
            if model
                .frames_of(*track)
                .iter()
                .enumerate()
                .any(|(index, frame)| {
                    keyframe_offset_permille(frame.t, duration) == Some(*target)
                        && !moving_indices.contains(&index)
                })
            {
                return Err(VbError::Conflict("同一轨道已有关键帧占据目标位置".into()));
            }
        }

        let shift_seconds = (f64::from(delta.permille()) / 1000.0) * duration;
        for (track, frame_index, _, target, original_time) in &planned {
            let Some((_, frames)) = model
                .tracks
                .iter_mut()
                .find(|(candidate, _)| candidate == track)
            else {
                return Err(VbError::Conflict("关键帧轨道已变化".into()));
            };
            let Some(frame) = frames.get_mut(*frame_index) else {
                return Err(VbError::Conflict("关键帧索引已变化".into()));
            };
            frame.t = *original_time + shift_seconds;
            if keyframe_offset_permille(frame.t, duration) != Some(*target) {
                return Err(VbError::Conflict("关键帧移动后未落在目标停靠点".into()));
            }
        }
        for (_, frames) in &mut model.tracks {
            frames.sort_by(|left, right| left.t.total_cmp(&right.t));
        }
        validate_animation_model(&model)?;

        let moved = planned
            .into_iter()
            .map(
                |(track, _, _, offset_permille, _)| AnimationKeyframeSelection {
                    sid: sid.clone(),
                    track,
                    offset_permille,
                },
            )
            .collect();
        Ok(Some(KeyframeMovePlan {
            sid,
            model,
            moved,
            playhead_seconds: self.animation_playhead_seconds,
        }))
    }

    /// 将单选检查器几何写回文档命令;多选、锁定对象及越界值拒绝写入。
    pub fn set_selected_geometry(&mut self, rect: CanvasRect) -> Result<bool, VbError> {
        const MAX_GEOMETRY: f64 = 1_000_000.0;
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        if [rect.x, rect.y, rect.width, rect.height, right, bottom]
            .iter()
            .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
            || rect.width <= 0.0
            || rect.height <= 0.0
        {
            return Err(VbError::Parse(
                "对象几何必须有限、尺寸为正且边界不超过 1000000 px".into(),
            ));
        }

        let [sid] = self.selection.as_slice() else {
            return Err(VbError::Conflict("几何检查器仅支持单对象选择".into()));
        };
        let sid = sid.clone();
        let id = self
            .document
            .find_by_sid(&sid)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        let node = self
            .document
            .node(id)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        if node.locked {
            return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
        }
        let new = Geom {
            x: rect.x,
            y: rect.y,
            w: rect.width,
            h: rect.height,
        };
        if node.geom == new {
            return Ok(false);
        }
        self.undo.push(
            &mut self.document,
            Command::SetGeom {
                sid,
                new,
                old: None,
                old_declared: None,
            },
        )?;
        Ok(true)
    }

    /// 写入单选对象的纯色填充/文字颜色;渐变与图片背景保持只读。
    pub fn set_selected_fill(&mut self, sid: &str, value: &str) -> Result<bool, VbError> {
        if value.len() > 128 {
            return Err(VbError::Parse("颜色值不能超过 128 字节".into()));
        }
        let [selected_sid] = self.selection.as_slice() else {
            return Err(VbError::Conflict("颜色检查器仅支持单对象选择".into()));
        };
        if selected_sid != sid {
            return Err(VbError::Conflict("颜色编辑对象已不再处于选择状态".into()));
        }
        let id = self
            .document
            .find_by_sid(sid)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        let node = self
            .document
            .node(id)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        if node.locked {
            return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
        }
        let property = inspector_fill_property(node)
            .ok_or_else(|| VbError::Unsupported("该对象使用复杂填充,暂不可编辑颜色".into()))?;
        let existing = node.style_get(property);
        if existing.is_some_and(|color| parse_color(color).is_none()) {
            return Err(VbError::Unsupported("当前颜色不是可编辑的纯色值".into()));
        }
        let normalized = parse_color(value)
            .ok_or_else(|| VbError::Parse("颜色值无效,请使用 CSS 颜色格式".into()))?
            .to_shortest_hex();
        if existing
            .and_then(parse_color)
            .is_some_and(|color| color.to_shortest_hex() == normalized)
        {
            return Ok(false);
        }
        let mut styled_node = node.clone();
        styled_node.style_set(property, &normalized);
        self.undo.end_session();
        self.undo.push(
            &mut self.document,
            Command::SetStyle {
                sid: sid.to_owned(),
                new: styled_node.style,
                old: None,
            },
        )?;
        Ok(true)
    }

    /// 写入单选纯单行文本;富文本和多行内容保持只读以保护结构。
    pub fn set_selected_text(&mut self, sid: &str, value: &str) -> Result<bool, VbError> {
        let [selected_sid] = self.selection.as_slice() else {
            return Err(VbError::Conflict("文本检查器仅支持单对象选择".into()));
        };
        if selected_sid != sid {
            return Err(VbError::Conflict("文本编辑对象已不再处于选择状态".into()));
        }
        let id = self
            .document
            .find_by_sid(sid)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        let node = self
            .document
            .node(id)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        if node.locked {
            return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
        }
        let NodeKind::Text { text, segments, .. } = &node.kind else {
            return Err(VbError::Unsupported("该对象不是文本".into()));
        };
        if !segments.is_empty() || text.contains('\n') || text.contains('\r') {
            return Err(VbError::Unsupported(
                "富文本或多行文本暂不可编辑,以保留原有格式".into(),
            ));
        }
        if value.contains('\n') || value.contains('\r') {
            return Err(VbError::Parse("文本输入框只接受单行内容".into()));
        }
        if text == value {
            return Ok(false);
        }
        self.undo.push(
            &mut self.document,
            Command::SetText {
                sid: sid.to_owned(),
                new: value.to_owned(),
                old: None,
            },
        )?;
        Ok(true)
    }

    /// 为单个未锁定对象设置非空单行名称;通过可撤销 Rename 命令写回。
    pub fn set_selected_name(&mut self, sid: &str, value: &str) -> Result<bool, VbError> {
        let [selected_sid] = self.selection.as_slice() else {
            return Err(VbError::Conflict("名称检查器仅支持单对象选择".into()));
        };
        if selected_sid != sid {
            return Err(VbError::Conflict("重命名对象已不再处于选择状态".into()));
        }
        let new = value.trim();
        if new.is_empty() {
            return Err(VbError::Parse("对象名称不能为空".into()));
        }
        if new.chars().count() > 256 || new.chars().any(char::is_control) {
            return Err(VbError::Parse(
                "对象名称最多 256 字符且不能包含控制字符".into(),
            ));
        }
        let id = self
            .document
            .find_by_sid(sid)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        let node = self
            .document
            .node(id)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        if node.locked {
            return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
        }
        if node.name == new {
            return Ok(false);
        }
        self.undo.push(
            &mut self.document,
            Command::Rename {
                sid: sid.to_owned(),
                new: new.to_owned(),
                old: None,
            },
        )?;
        Ok(true)
    }

    /// 以会话语义更新对象选择;点空白清空,Shift 点选切换成员。
    pub fn select_sid(&mut self, sid: Option<&str>, modifiers: ModifierState) -> bool {
        let before = self.selection.as_slice().to_vec();
        match sid {
            Some(sid) if modifiers.is_additive() => {
                self.selection.toggle(sid);
            }
            Some(sid) => self.selection.select(sid),
            None if !modifiers.is_additive() => {
                self.selection.clear();
            }
            None => {}
        }
        let changed = self.selection.as_slice() != before;
        if changed {
            self.selected_keyframes.clear();
        }
        changed
    }

    /// 选中当前画板下所有可见、未锁定的直接子节点。
    pub fn select_all_in_artboard(&mut self, artboard_index: usize) -> Result<usize, VbError> {
        let artboard_id = self
            .document
            .artboards()
            .get(artboard_index)
            .copied()
            .ok_or_else(|| VbError::NoSuchNode(format!("artboard index {artboard_index}")))?;
        let children = self
            .document
            .node(artboard_id)
            .ok_or_else(|| VbError::NoSuchNode(format!("artboard {artboard_index}")))?
            .children
            .clone();
        let selected: Vec<String> = children
            .into_iter()
            .filter_map(|id| self.document.node(id))
            .filter(|node| !node.hidden && !node.locked)
            .map(|node| node.sid.as_str().to_owned())
            .collect();
        let count = selected.len();
        let previous = self.selection.as_slice().to_vec();
        self.selection.set_all(selected);
        if self.selection.as_slice() != previous {
            self.selected_keyframes.clear();
        }
        Ok(count)
    }

    /// 开始画布指针选择;点击已选对象保留有序多选并返回可拖动的 SID 快照。
    pub fn begin_pointer_selection(
        &mut self,
        sid: Option<&str>,
        modifiers: ModifierState,
    ) -> Option<Vec<String>> {
        if modifiers.is_additive() {
            self.select_sid(sid, modifiers);
            return None;
        }
        if modifiers.control || modifiers.alt {
            return None;
        }

        match sid {
            Some(sid) if self.selection.is_selected(sid) => {
                Some(self.selection.as_slice().to_vec())
            }
            Some(sid) => {
                self.select_sid(Some(sid), modifiers);
                Some(self.selection.as_slice().to_vec())
            }
            None => {
                self.select_sid(None, modifiers);
                None
            }
        }
    }

    /// 文档相对最近一次导入/保存是否有命令变更。
    pub fn is_dirty(&self) -> bool {
        self.undo.is_dirty()
    }

    /// 是否存在可撤销命令。
    pub fn can_undo(&self) -> bool {
        self.undo.can_undo()
    }

    /// 是否存在可重做命令。
    pub fn can_redo(&self) -> bool {
        self.undo.can_redo()
    }

    /// 在指定画板插入默认蓝色矩形;文档变更只经可逆 `Command::Insert`。
    pub fn create_rectangle(
        &mut self,
        artboard_index: usize,
        rect: CanvasRect,
    ) -> Result<String, VbError> {
        self.create_basic_shape(artboard_index, rect, false)
    }

    /// 在指定画板插入默认蓝色椭圆;以文档 CSS `border-radius:50%` 表达几何。
    pub fn create_ellipse(
        &mut self,
        artboard_index: usize,
        rect: CanvasRect,
    ) -> Result<String, VbError> {
        self.create_basic_shape(artboard_index, rect, true)
    }

    /// 在当前画板坐标创建画板根节点,作为一条可撤销 Insert 命令。
    pub fn create_artboard(&mut self, rect: CanvasRect) -> Result<String, VbError> {
        const MAX_GEOMETRY: f64 = 1_000_000.0;
        let values = [rect.x, rect.y, rect.width, rect.height];
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        if values.iter().any(|value| !value.is_finite())
            || rect.width <= 0.0
            || rect.height <= 0.0
            || [right, bottom]
                .iter()
                .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
            || [rect.x, rect.y, rect.width, rect.height]
                .iter()
                .any(|value| value.abs() > MAX_GEOMETRY)
        {
            return Err(VbError::Parse(
                "画板几何必须为有限值、正尺寸且边界不超过 1000000 px".into(),
            ));
        }
        let active_id = self
            .document
            .artboards()
            .get(self.active_artboard_index)
            .copied()
            .ok_or_else(|| VbError::NoSuchNode("当前文档没有画板".into()))?;
        let active = self
            .document
            .node(active_id)
            .ok_or_else(|| VbError::NoSuchNode("当前画板不存在".into()))?;
        let x = active.geom.x + rect.x;
        let y = active.geom.y + rect.y;
        if !x.is_finite()
            || !y.is_finite()
            || (x + rect.width).abs() > MAX_GEOMETRY
            || (y + rect.height).abs() > MAX_GEOMETRY
        {
            return Err(VbError::Parse("新画板超出文档坐标范围".into()));
        }
        let root = self.document.root();
        let root_node = self
            .document
            .node(root)
            .ok_or_else(|| VbError::NoSuchNode("文档根节点不存在".into()))?;
        let parent_sid = root_node.sid.as_str().to_string();
        let index = root_node.children.len();
        let name = format!("Artboard {}", self.document.artboards().len() + 1);
        let mut node = Node::new(NodeKind::Artboard, name, self.document.alloc_sid());
        node.geom = Geom {
            x,
            y,
            w: rect.width,
            h: rect.height,
        };
        let sid = node.sid.as_str().to_string();
        self.undo.end_session();
        self.undo.push(
            &mut self.document,
            Command::Insert {
                parent_sid,
                index,
                tree: NodeTree {
                    node,
                    children: Vec::new(),
                },
            },
        )?;
        self.active_artboard_index = self.document.artboards().len().saturating_sub(1);
        self.selection.select(sid.clone());
        Ok(sid)
    }

    /// 在画板坐标创建可编辑的纯文本节点;后续样式/文本仍以 HTML/CSS 文档为准。
    pub fn create_text(
        &mut self,
        artboard_index: usize,
        point: [f64; 2],
    ) -> Result<String, VbError> {
        const MAX_GEOMETRY: f64 = 1_000_000.0;
        if point
            .iter()
            .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
        {
            return Err(VbError::Parse(
                "文本坐标必须为有限值且单轴不超过 1000000 px".into(),
            ));
        }
        let artboard = self
            .document
            .artboards()
            .get(artboard_index)
            .copied()
            .ok_or_else(|| VbError::NoSuchNode(format!("画板索引 {artboard_index}")))?;
        let parent = self
            .document
            .node(artboard)
            .ok_or_else(|| VbError::NoSuchNode(format!("画板索引 {artboard_index}")))?;
        let parent_sid = parent.sid.as_str().to_string();
        let index = parent.children.len();
        let mut node = Node::new(
            NodeKind::Text {
                text: "Text".into(),
                mode: TextMode::Point,
                segments: Vec::new(),
            },
            "Text",
            self.document.alloc_sid(),
        );
        node.geom = Geom {
            x: point[0],
            y: point[1],
            w: 200.0,
            h: 48.0,
        };
        node.authored = [true; 4];
        node.style_set("font-size", "32px");
        node.style_set("color", "#4F9FFF");
        let sid = node.sid.as_str().to_string();
        self.undo.push(
            &mut self.document,
            Command::Insert {
                parent_sid,
                index,
                tree: NodeTree {
                    node,
                    children: Vec::new(),
                },
            },
        )?;
        self.selection.select(sid.clone());
        Ok(sid)
    }

    /// 复制并插入一个项目内栅格图像;远端/路径内容仅在安全复制后以相对 assets 路径落盘。
    #[cfg(feature = "canvas-projection")]
    pub fn place_image_from_path(
        &mut self,
        artboard_index: usize,
        source: &Path,
    ) -> Result<String, VbError> {
        let source_extension = source
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| VbError::Parse("图像文件缺少受支持的扩展名".into()))?;
        let expected_extension = match source_extension.as_str() {
            "png" => "png",
            "jpg" | "jpeg" => "jpg",
            "webp" => "webp",
            "bmp" => "bmp",
            "tif" | "tiff" => "tif",
            _ => {
                return Err(VbError::Parse(
                    "仅支持 PNG/JPEG/WebP/BMP/TIFF 栅格图像".into(),
                ))
            }
        };

        let mut input = fs::File::open(source)?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut input)
            .take(vb_render::assets::MAX_PROJECT_IMAGE_SOURCE_BYTES.saturating_add(1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.is_empty() || bytes.len() > vb_render::assets::MAX_PROJECT_IMAGE_SOURCE_BYTES {
            return Err(VbError::Parse("图像文件为空或超过 32 MiB 上限".into()));
        }
        let (bitmap, actual_extension) =
            vb_render::assets::decode_project_bitmap_with_extension(&bytes)
                .ok_or_else(|| VbError::Parse("无法解码或图像尺寸超过安全上限".into()))?;
        if actual_extension != expected_extension {
            return Err(VbError::Parse("图像内容格式与文件扩展名不匹配".into()));
        }

        let artboard_id = self
            .document
            .artboards()
            .get(artboard_index)
            .copied()
            .ok_or_else(|| VbError::NoSuchNode(format!("画板索引 {artboard_index}")))?;
        let artboard = self
            .document
            .node(artboard_id)
            .ok_or_else(|| VbError::NoSuchNode(format!("画板索引 {artboard_index}")))?;
        if artboard.locked {
            return Err(VbError::Conflict("目标画板已锁定".into()));
        }
        let board_width = artboard.geom.w;
        let board_height = artboard.geom.h;
        let parent_sid = artboard.sid.as_str().to_string();
        let index = artboard.children.len();
        if !board_width.is_finite()
            || !board_height.is_finite()
            || board_width <= 0.0
            || board_height <= 0.0
        {
            return Err(VbError::Conflict("目标画板尺寸无效".into()));
        }

        let original_width = f64::from(bitmap.width);
        let original_height = f64::from(bitmap.height);
        let scale = (board_width / original_width)
            .min(board_height / original_height)
            .min(1.0);
        let width = original_width * scale;
        let height = original_height * scale;
        if !scale.is_finite() || scale <= 0.0 || width <= 0.0 || height <= 0.0 {
            return Err(VbError::Conflict("图像无法放入目标画板".into()));
        }

        let stem = source
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name: String = stem
            .chars()
            .filter(|character| !character.is_control())
            .take(128)
            .collect();
        let name = if name.trim().is_empty() {
            t("ui-session-default-image-name")
        } else {
            name
        };
        let sid = self.document.alloc_sid();
        let relative_src =
            write_project_image_asset(&self.project_dir, sid.as_str(), actual_extension, &bytes)?;
        let mut node = Node::new(
            NodeKind::Image {
                src: relative_src.clone(),
            },
            name.clone(),
            sid.clone(),
        );
        node.geom = Geom {
            x: (board_width - width) / 2.0,
            y: (board_height - height) / 2.0,
            w: width,
            h: height,
        };
        node.authored = [true; 4];
        node.style_set("object-fit", "contain");
        node.style_set("object-position", "center");
        node.attrs.insert("src".into(), relative_src.clone());
        node.attrs.insert("alt".into(), name);

        let sid_string = sid.as_str().to_string();
        self.undo.end_session();
        let insertion = self.undo.push(
            &mut self.document,
            Command::Insert {
                parent_sid,
                index,
                tree: NodeTree {
                    node,
                    children: Vec::new(),
                },
            },
        );
        if let Err(error) = insertion {
            let _ = fs::remove_file(self.project_dir.join(relative_src));
            return Err(error);
        }
        self.selection.select(sid_string.clone());
        Ok(sid_string)
    }

    fn create_basic_shape(
        &mut self,
        artboard_index: usize,
        rect: CanvasRect,
        ellipse: bool,
    ) -> Result<String, VbError> {
        const MAX_GEOMETRY: f64 = 1_000_000.0;
        let shape_name = if ellipse { "椭圆" } else { "矩形" };
        if [rect.x, rect.y, rect.width, rect.height]
            .iter()
            .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
            || rect.width <= 0.0
            || rect.height <= 0.0
        {
            return Err(VbError::Parse(format!(
                "{shape_name}坐标必须有限且尺寸必须大于零(单轴上限 1000000 px)"
            )));
        }

        let artboard = self
            .document
            .artboards()
            .get(artboard_index)
            .copied()
            .ok_or_else(|| VbError::NoSuchNode(format!("画板索引 {artboard_index}")))?;
        let parent = self
            .document
            .node(artboard)
            .ok_or_else(|| VbError::NoSuchNode(format!("画板索引 {artboard_index}")))?;
        let parent_sid = parent.sid.as_str().to_string();
        let index = parent.children.len();

        let mut node = Node::new(NodeKind::Box, shape_name, self.document.alloc_sid());
        node.geom = Geom {
            x: rect.x,
            y: rect.y,
            w: rect.width,
            h: rect.height,
        };
        node.authored = [true; 4];
        node.style_set("background-color", "#4F9FFF");
        if ellipse {
            node.style_set("border-radius", "50%");
        }
        let sid = node.sid.as_str().to_string();
        self.undo.push(
            &mut self.document,
            Command::Insert {
                parent_sid,
                index,
                tree: NodeTree {
                    node,
                    children: Vec::new(),
                },
            },
        )?;
        self.selection.select(sid.clone());
        Ok(sid)
    }

    fn deletion_targets(&self) -> Result<Vec<String>, VbError> {
        let selected = self.selection.as_slice();
        if selected.is_empty() {
            return Ok(Vec::new());
        }
        let selected_sids: HashSet<&str> = selected.iter().map(String::as_str).collect();
        let mut selected_artboards = 0;
        for sid in selected {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            if node.locked {
                return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
            }
            if matches!(node.kind, NodeKind::Artboard) {
                selected_artboards += 1;
            }
        }
        if selected_artboards >= self.document.artboards().len() {
            return Err(VbError::Conflict("至少保留一块画板".into()));
        }

        let mut targets = Vec::with_capacity(selected.len());
        for sid in selected {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let mut parent = self.document.node(id).and_then(|node| node.parent);
            let mut has_selected_ancestor = false;
            while let Some(parent_id) = parent {
                let Some(ancestor) = self.document.node(parent_id) else {
                    break;
                };
                if selected_sids.contains(ancestor.sid.as_str()) {
                    has_selected_ancestor = true;
                    break;
                }
                parent = ancestor.parent;
            }
            if !has_selected_ancestor {
                targets.push(sid.clone());
            }
        }
        Ok(targets)
    }

    /// 对有序选择批量修改显隐/锁定,并以一条命令整体撤销。
    pub fn set_selected_flags(
        &mut self,
        hidden: Option<bool>,
        locked: Option<bool>,
    ) -> Result<usize, VbError> {
        if hidden.is_none() && locked.is_none() {
            return Ok(0);
        }
        let mut commands = Vec::with_capacity(self.selection.as_slice().len());
        for sid in self.selection.as_slice() {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let next_hidden = hidden.filter(|value| *value != node.hidden);
            let next_locked = locked.filter(|value| *value != node.locked);
            if next_hidden.is_some() || next_locked.is_some() {
                commands.push(Command::SetFlags {
                    sid: sid.clone(),
                    hidden: next_hidden,
                    locked: next_locked,
                    old: None,
                });
            }
        }
        if commands.is_empty() {
            return Ok(0);
        }
        let changed = commands.len();
        self.undo.end_session();
        self.undo.push_compound(&mut self.document, commands)?;
        Ok(changed)
    }

    fn selected_artboard_sid(&self) -> Option<String> {
        let [sid] = self.selection.as_slice() else {
            return None;
        };
        let id = self.document.find_by_sid(sid)?;
        self.document
            .node(id)
            .filter(|node| matches!(node.kind, NodeKind::Artboard))
            .map(|_| sid.clone())
    }

    fn can_delete_artboard(&self, sid: &str) -> bool {
        let Some(id) = self.document.find_by_sid(sid) else {
            return false;
        };
        let Some(node) = self.document.node(id) else {
            return false;
        };
        matches!(node.kind, NodeKind::Artboard)
            && !node.locked
            && self.document.artboards().len() > 1
    }

    fn delete_selected_artboard(&mut self, sid: &str) -> Result<usize, VbError> {
        let id = self
            .document
            .find_by_sid(sid)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        let node = self
            .document
            .node(id)
            .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
        if !matches!(node.kind, NodeKind::Artboard) {
            return Ok(0);
        }
        if node.locked {
            return Err(VbError::Conflict(format!("画板 {sid} 已锁定")));
        }
        let artboards = self.document.artboards();
        if artboards.len() <= 1 {
            return Err(VbError::Conflict("至少保留一块画板".into()));
        }
        let deleted_index = artboards
            .iter()
            .position(|artboard| *artboard == id)
            .ok_or_else(|| VbError::NoSuchNode(format!("画板 {sid}")))?;
        let active_sid = artboards
            .get(self.active_artboard_index)
            .and_then(|active| self.document.node(*active))
            .map(|active| active.sid.as_str().to_string());
        self.undo.end_session();
        self.undo.push(
            &mut self.document,
            Command::Delete {
                target_sid: sid.to_string(),
                captured: None,
            },
        )?;
        self.selection.clear();
        let remaining = self.document.artboards();
        self.active_artboard_index = if active_sid.as_deref() != Some(sid) {
            active_sid
                .as_deref()
                .and_then(|active_sid| {
                    remaining.iter().position(|active| {
                        self.document
                            .node(*active)
                            .is_some_and(|node| node.sid.as_str() == active_sid)
                    })
                })
                .unwrap_or(0)
        } else {
            deleted_index
                .saturating_sub(1)
                .min(remaining.len().saturating_sub(1))
        };
        Ok(1)
    }

    /// 当前选择能否完整删除,不违反至少保留一块画板的不变量。
    pub fn can_delete_selection(&self) -> bool {
        if let Some(sid) = self.selected_artboard_sid() {
            return self.can_delete_artboard(&sid);
        }
        self.deletion_targets()
            .is_ok_and(|targets| !targets.is_empty())
    }

    /// 将有序选择折叠为最上层子树并以一条可撤销复合命令删除。
    pub fn delete_selection(&mut self) -> Result<usize, VbError> {
        if let Some(sid) = self.selected_artboard_sid() {
            return self.delete_selected_artboard(&sid);
        }
        let targets = self.deletion_targets()?;
        if targets.is_empty() {
            return Ok(0);
        }
        let commands = targets
            .into_iter()
            .map(|target_sid| Command::Delete {
                target_sid,
                captured: None,
            })
            .collect::<Vec<_>>();
        let deleted = commands.len();
        self.undo.end_session();
        self.undo.push_compound(&mut self.document, commands)?;
        self.selection.clear();
        Ok(deleted)
    }

    fn selected_clipboard_entries(&self) -> Result<Vec<ClipboardEntry>, VbError> {
        let selected = self.selection.as_slice();
        if selected.is_empty() {
            return Ok(Vec::new());
        }
        let selected_sids = selected.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut entries = Vec::with_capacity(selected.len());
        for sid in selected {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            if matches!(node.kind, NodeKind::Artboard) {
                return Err(VbError::Conflict("画板不能复制或剪切".into()));
            }

            let mut ancestor = node.parent;
            let mut has_selected_ancestor = false;
            while let Some(parent_id) = ancestor {
                let Some(parent) = self.document.node(parent_id) else {
                    break;
                };
                if selected_sids.contains(parent.sid.as_str()) {
                    has_selected_ancestor = true;
                    break;
                }
                ancestor = parent.parent;
            }
            if has_selected_ancestor {
                continue;
            }

            let parent_id = node
                .parent
                .ok_or_else(|| VbError::Conflict("根节点不能复制".into()))?;
            let parent = self
                .document
                .node(parent_id)
                .ok_or_else(|| VbError::NoSuchNode(format!("parent of sid {sid}")))?;
            let index = parent
                .children
                .iter()
                .position(|child_id| {
                    self.document
                        .node(*child_id)
                        .is_some_and(|child| child.sid.as_str() == sid)
                })
                .ok_or_else(|| VbError::NoSuchNode(format!("parent slot of sid {sid}")))?;
            let tree = NodeTree::from_document(&self.document, id)
                .ok_or_else(|| VbError::NoSuchNode(format!("subtree of sid {sid}")))?;
            entries.push(ClipboardEntry {
                parent_sid: parent.sid.as_str().to_string(),
                index,
                tree,
            });
        }
        entries.sort_by(|left, right| {
            left.parent_sid
                .cmp(&right.parent_sid)
                .then_with(|| left.index.cmp(&right.index))
        });
        Ok(entries)
    }

    /// 当前选择是否可复制,剪贴板始终局限于当前会话。
    pub fn can_copy_selection(&self) -> bool {
        self.selected_clipboard_entries()
            .is_ok_and(|entries| !entries.is_empty())
    }

    /// 当前选择是否可剪切。
    pub fn can_cut_selection(&self) -> bool {
        self.can_copy_selection() && self.can_delete_selection()
    }

    /// 当前会话剪贴板是否有可粘贴内容。
    pub fn can_paste_selection(&self) -> bool {
        !self.clipboard.is_empty()
    }

    /// 复制所选的最上层子树到会话内剪贴板;不改动或污染 HTML 文档。
    pub fn copy_selection(&mut self) -> Result<usize, VbError> {
        let entries = self.selected_clipboard_entries()?;
        if entries.is_empty() {
            return Ok(0);
        }
        let copied = entries.len();
        self.clipboard = entries;
        self.paste_offset = 0;
        Ok(copied)
    }

    /// 剪切一组最上层子树;剪贴板只在可撤销删除成功后更新。
    pub fn cut_selection(&mut self) -> Result<usize, VbError> {
        let entries = self.selected_clipboard_entries()?;
        if entries.is_empty() {
            return Ok(0);
        }
        let deleted = self.delete_selection()?;
        if deleted != entries.len() {
            return Err(VbError::Conflict("剪切目标与文档删除范围不一致".into()));
        }
        self.clipboard = entries;
        self.paste_offset = 0;
        Ok(deleted)
    }

    /// 粘贴剪贴板子树;普通粘贴阶梯偏移 20px,原位粘贴保持几何并置于原对象上方。
    pub fn paste_selection(&mut self, in_place: bool) -> Result<usize, VbError> {
        if self.clipboard.is_empty() {
            return Ok(0);
        }
        let offset = if in_place {
            0.0
        } else {
            20.0 * f64::from(self.paste_offset.saturating_add(1))
        };
        let mut commands = Vec::with_capacity(self.clipboard.len());
        let mut pasted_sids = Vec::with_capacity(self.clipboard.len());
        let mut inserted_per_parent = std::collections::HashMap::<String, usize>::new();

        for entry in self.clipboard.clone() {
            let parent_id = self
                .document
                .find_by_sid(&entry.parent_sid)
                .or_else(|| self.document.artboards().first().copied())
                .ok_or_else(|| VbError::Conflict("没有可粘贴到的画板".into()))?;
            let parent = self
                .document
                .node(parent_id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {}", entry.parent_sid)))?;
            if parent.locked {
                return Err(VbError::Conflict(format!(
                    "目标容器 {} 已锁定",
                    parent.sid.as_str()
                )));
            }
            let parent_sid = parent.sid.as_str().to_string();
            let current_len = parent.children.len();
            let base_index = if in_place && parent_sid == entry.parent_sid {
                entry.index.saturating_add(1).min(current_len)
            } else {
                current_len
            };
            let inserted = inserted_per_parent.entry(parent_sid.clone()).or_default();
            let index = base_index.saturating_add(*inserted);
            *inserted = inserted.saturating_add(1);

            let mut tree = entry.tree;
            Self::rekey_tree(&mut self.document, &mut tree);
            Self::offset_tree(&mut tree, [offset, offset])?;
            pasted_sids.push(tree.root_sid().to_string());
            commands.push(Command::Insert {
                parent_sid,
                index,
                tree,
            });
        }

        let pasted = commands.len();
        self.undo.end_session();
        self.undo.push_compound(&mut self.document, commands)?;
        self.selection.set_all(pasted_sids);
        if !in_place {
            self.paste_offset = self.paste_offset.saturating_add(1);
        }
        Ok(pasted)
    }

    fn rekey_tree(document: &mut Document, tree: &mut NodeTree) {
        tree.node.sid = document.alloc_sid();
        for child in &mut tree.children {
            Self::rekey_tree(document, child);
        }
    }

    fn offset_tree(tree: &mut NodeTree, offset: [f64; 2]) -> Result<(), VbError> {
        const MAX_GEOMETRY: f64 = 1_000_000.0;
        let geom = &mut tree.node.geom;
        geom.x += offset[0];
        geom.y += offset[1];
        let right = geom.x + geom.w;
        let bottom = geom.y + geom.h;
        if [geom.x, geom.y, geom.w, geom.h, right, bottom]
            .iter()
            .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
        {
            return Err(VbError::Parse("粘贴后对象超出画板坐标范围".into()));
        }
        for child in &mut tree.children {
            Self::offset_tree(child, offset)?;
        }
        Ok(())
    }

    fn selected_move_targets(&self) -> Result<Vec<String>, VbError> {
        let selected = self.selection.as_slice();
        let selected_sids = selected.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut targets = Vec::with_capacity(selected.len());
        for sid in selected {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            if matches!(&node.kind, NodeKind::Artboard) {
                return Err(VbError::Conflict("画板需通过画板工具调整".into()));
            }
            if node.locked {
                return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
            }
            self.validate_unlocked_ancestors(id)?;

            let mut parent = node.parent;
            let mut has_selected_ancestor = false;
            while let Some(parent_id) = parent {
                let ancestor = self
                    .document
                    .node(parent_id)
                    .ok_or_else(|| VbError::NoSuchNode(format!("parent of sid {sid}")))?;
                if selected_sids.contains(ancestor.sid.as_str()) {
                    has_selected_ancestor = true;
                    break;
                }
                parent = ancestor.parent;
            }
            if !has_selected_ancestor {
                targets.push(sid.clone());
            }
        }
        Ok(targets)
    }

    /// 计算文档预览吸附;不改文档。`bypass` 对应临时 Alt 直拖。
    pub fn snap_selection_delta(
        &self,
        requested_delta: [f64; 2],
        zoom: f64,
        bypass: bool,
    ) -> Result<SnapPreview, VbError> {
        const MAX_GEOMETRY: f64 = 1_000_000.0;
        if requested_delta
            .iter()
            .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
        {
            return Err(VbError::Parse(
                "移动距离必须为有限值且单轴不超过 1000000 px".into(),
            ));
        }
        if bypass || !zoom.is_finite() || zoom <= 0.0 {
            return Ok(SnapPreview {
                delta: requested_delta,
                guides: Vec::new(),
            });
        }
        let targets = self.selected_move_targets()?;
        if targets.is_empty() {
            return Ok(SnapPreview {
                delta: requested_delta,
                guides: Vec::new(),
            });
        }
        let mut ids = Vec::with_capacity(targets.len());
        let mut parent_id = None;
        let mut artboard_id = None;
        for sid in &targets {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let current_parent = node.parent;
            let current_artboard = vb_tools::artboard_of(&self.document, id);
            if current_parent.is_none()
                || current_artboard
                    != self
                        .document
                        .artboards()
                        .get(self.active_artboard_index)
                        .copied()
            {
                return Ok(SnapPreview {
                    delta: requested_delta,
                    guides: Vec::new(),
                });
            }
            if parent_id.is_some_and(|parent| Some(parent) != current_parent)
                || artboard_id.is_some_and(|artboard| Some(artboard) != current_artboard)
            {
                return Ok(SnapPreview {
                    delta: requested_delta,
                    guides: Vec::new(),
                });
            }
            parent_id = current_parent;
            artboard_id = current_artboard;
            ids.push(id);
        }
        let parent = parent_id.ok_or_else(|| VbError::Conflict("移动对象缺少父级".into()))?;
        let artboard = artboard_id.ok_or_else(|| VbError::Conflict("移动对象不属于画板".into()))?;
        let (offset_x, offset_y) =
            crate::snap::parent_offset_in_artboard(&self.document, Some(parent), Some(artboard));
        let mut bounds: Option<(f64, f64, f64, f64)> = None;
        for id in &ids {
            let rect = vb_tools::abs_bbox(&self.document, *id)
                .ok_or_else(|| VbError::NoSuchNode("移动对象没有有效几何".into()))?;
            bounds = Some(match bounds {
                Some((left, top, right, bottom)) => (
                    left.min(rect.x0),
                    top.min(rect.y0),
                    right.max(rect.x1),
                    bottom.max(rect.y1),
                ),
                None => (rect.x0, rect.y0, rect.x1, rect.y1),
            });
        }
        let (left, top, right, bottom) =
            bounds.ok_or_else(|| VbError::Conflict("没有可移动对象".into()))?;
        let base_x = left - offset_x;
        let base_y = top - offset_y;
        let geometry = Geom {
            x: base_x + requested_delta[0],
            y: base_y + requested_delta[1],
            w: right - left,
            h: bottom - top,
        };
        let (snapped_x, snapped_y, world_guides) = crate::snap::smart_snap_many(
            &self.document,
            &ids,
            Some(parent),
            Some(artboard),
            &geometry,
            6.0 / zoom,
        );
        let (artboard_x, artboard_y) = self.document.artboard_origin(artboard);
        let guides = world_guides
            .into_iter()
            .map(|[x0, y0, x1, y1]| {
                [
                    x0 - artboard_x,
                    y0 - artboard_y,
                    x1 - artboard_x,
                    y1 - artboard_y,
                ]
            })
            .collect();
        Ok(SnapPreview {
            delta: [snapped_x - base_x, snapped_y - base_y],
            guides,
        })
    }

    /// 对有序选择提交一次多对象平移;每个鼠标手势只占一条可撤销复合命令。
    pub fn move_selection_by(&mut self, delta: [f64; 2]) -> Result<usize, VbError> {
        const MAX_GEOMETRY: f64 = 1_000_000.0;
        if delta
            .iter()
            .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
        {
            return Err(VbError::Parse(
                "移动距离必须为有限值且单轴不超过 1000000 px".into(),
            ));
        }
        let targets = self.selected_move_targets()?;
        if delta == [0.0, 0.0] || targets.is_empty() {
            return Ok(0);
        }

        let mut commands = Vec::with_capacity(targets.len());
        for sid in targets {
            let id = self
                .document
                .find_by_sid(&sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let mut new = node.geom;
            new.x += delta[0];
            new.y += delta[1];
            let right = new.x + new.w;
            let bottom = new.y + new.h;
            if [new.x, new.y, new.w, new.h, right, bottom]
                .iter()
                .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
            {
                return Err(VbError::Parse(format!(
                    "移动会使对象 {sid} 超出画板坐标范围"
                )));
            }
            commands.push(Command::SetGeom {
                sid,
                new,
                old: None,
                old_declared: None,
            });
        }

        let moved = commands.len();
        self.undo.end_session();
        self.undo.push_compound(&mut self.document, commands)?;
        Ok(moved)
    }

    fn alignment_artboard_box(&self, id: NodeId) -> Option<AbsBox> {
        let node = self.document.node(id)?;
        let artboard_id = if matches!(&node.kind, NodeKind::Artboard) {
            id
        } else {
            vb_tools::artboard_of(&self.document, id)?
        };
        let artboard = self.document.node(artboard_id)?;
        Some(AbsBox::new(0.0, 0.0, artboard.geom.w, artboard.geom.h))
    }

    fn dispatch_alignment(&mut self, mode: AlignMode) -> CommandOutcome {
        match self.align_selection(mode) {
            Ok(count) if count > 0 => CommandOutcome::Applied,
            Ok(_) => CommandOutcome::not_run(t("ui-app-commands-021")),
            Err(error) => CommandOutcome::not_run(error.to_string()),
        }
    }

    fn dispatch_nudge(&mut self, delta: [f64; 2]) -> CommandOutcome {
        match self.move_selection_by(delta) {
            Ok(count) if count > 0 => CommandOutcome::Applied,
            Ok(_) => CommandOutcome::not_run("没有可移动的对象"),
            Err(error) => CommandOutcome::not_run(error.to_string()),
        }
    }

    /// 对齐当前选择;几何内核复用 `vb_tools::align`,变更整体进入一条复合撤销命令。
    pub fn align_selection(&mut self, mode: AlignMode) -> Result<usize, VbError> {
        if self.selection.as_slice().is_empty() {
            return Ok(0);
        }
        let selected_sids: HashSet<&str> = self
            .selection
            .as_slice()
            .iter()
            .map(String::as_str)
            .collect();
        let mut roots = Vec::with_capacity(selected_sids.len());
        for sid in self.selection.as_slice() {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            if node.locked {
                return Err(VbError::Conflict(format!("对象 {sid} 已锁定")));
            }
            let mut parent = node.parent;
            let mut selected_ancestor = false;
            while let Some(parent_id) = parent {
                let Some(ancestor) = self.document.node(parent_id) else {
                    break;
                };
                if ancestor.locked {
                    return Err(VbError::Conflict(format!("对象 {sid} 的祖先图层已锁定")));
                }
                if selected_sids.contains(ancestor.sid.as_str()) {
                    selected_ancestor = true;
                    break;
                }
                parent = ancestor.parent;
            }
            if selected_ancestor {
                continue;
            }
            if let Some(bounds) = AbsBox::of(&self.document, id) {
                roots.push((sid.clone(), node.geom, bounds, id));
            }
        }
        if roots.is_empty() {
            return Ok(0);
        }

        let last_id = roots
            .last()
            .map(|(_, _, _, id)| *id)
            .ok_or_else(|| VbError::Conflict("没有可对齐的对象".into()))?;
        let target = match self.align_to {
            AlignTo::KeyObject => roots.last().map(|(_, _, bounds, _)| *bounds),
            AlignTo::Artboard => self.alignment_artboard_box(last_id),
            AlignTo::Selection if roots.len() > 1 => {
                let bounds = roots
                    .iter()
                    .map(|(_, _, bounds, _)| *bounds)
                    .collect::<Vec<_>>();
                AbsBox::union(&bounds)
            }
            AlignTo::Selection => self.alignment_artboard_box(last_id),
        }
        .ok_or_else(|| VbError::Conflict("无法解析对齐目标".into()))?;

        const MAX_GEOMETRY: f64 = 1_000_000.0;
        let mut commands = Vec::with_capacity(roots.len());
        for (sid, geom, bounds, _) in roots {
            let (dx, dy) = aligned_delta(mode, &bounds, &target);
            if dx.abs() < 0.5 && dy.abs() < 0.5 {
                continue;
            }
            let new = Geom {
                x: geom.x + dx,
                y: geom.y + dy,
                w: geom.w,
                h: geom.h,
            };
            let right = new.x + new.w;
            let bottom = new.y + new.h;
            if [new.x, new.y, new.w, new.h, right, bottom]
                .iter()
                .any(|value| !value.is_finite() || value.abs() > MAX_GEOMETRY)
            {
                return Err(VbError::Parse(format!(
                    "对齐会使对象 {sid} 超出画板坐标范围"
                )));
            }
            commands.push(Command::SetGeom {
                sid,
                new,
                old: None,
                old_declared: None,
            });
        }
        if commands.is_empty() {
            return Ok(0);
        }
        let changed = commands.len();
        self.undo.end_session();
        self.undo.push_compound(&mut self.document, commands)?;
        Ok(changed)
    }

    fn validate_unlocked_subtree(&self, root: NodeId) -> Result<(), VbError> {
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("node {id:?}")))?;
            if node.locked {
                return Err(VbError::Conflict(format!(
                    "对象 {} 或其子图层已锁定",
                    node.sid.as_str()
                )));
            }
            pending.extend(node.children.iter().copied());
        }
        Ok(())
    }

    fn validate_unlocked_ancestors(&self, node: NodeId) -> Result<(), VbError> {
        let mut parent = self.document.node(node).and_then(|node| node.parent);
        while let Some(parent_id) = parent {
            let ancestor = self
                .document
                .node(parent_id)
                .ok_or_else(|| VbError::NoSuchNode(format!("parent {parent_id:?}")))?;
            if ancestor.locked {
                return Err(VbError::Conflict(format!(
                    "对象 {} 的祖先图层已锁定",
                    ancestor.sid.as_str()
                )));
            }
            parent = ancestor.parent;
        }
        Ok(())
    }

    fn selected_group_members(&self) -> Result<Vec<String>, VbError> {
        let selected = self.selection.as_slice();
        if selected.len() < 2 {
            return Ok(Vec::new());
        }
        let selected_sids = selected.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut members = Vec::with_capacity(selected.len());
        for sid in selected {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            if matches!(&node.kind, NodeKind::Artboard) {
                return Err(VbError::Conflict("画板不能编入组".into()));
            }
            self.validate_unlocked_subtree(id)?;
            self.validate_unlocked_ancestors(id)?;

            let mut ancestor = node.parent;
            let mut has_selected_ancestor = false;
            while let Some(parent_id) = ancestor {
                let parent = self
                    .document
                    .node(parent_id)
                    .ok_or_else(|| VbError::NoSuchNode(format!("parent of sid {sid}")))?;
                if selected_sids.contains(parent.sid.as_str()) {
                    has_selected_ancestor = true;
                    break;
                }
                ancestor = parent.parent;
            }
            if has_selected_ancestor {
                continue;
            }

            let parent_id = node
                .parent
                .ok_or_else(|| VbError::Conflict("根节点不能编组".into()))?;
            let parent = self
                .document
                .node(parent_id)
                .ok_or_else(|| VbError::NoSuchNode(format!("parent of sid {sid}")))?;
            if parent.locked {
                return Err(VbError::Conflict(format!(
                    "目标父图层 {} 已锁定",
                    parent.sid.as_str()
                )));
            }
            let index = parent
                .children
                .iter()
                .position(|child| {
                    self.document
                        .node(*child)
                        .is_some_and(|candidate| candidate.sid.as_str() == sid)
                })
                .ok_or_else(|| VbError::NoSuchNode(format!("parent slot of sid {sid}")))?;
            members.push((parent.sid.as_str().to_string(), index, sid.clone()));
        }
        if members.len() < 2 {
            return Ok(Vec::new());
        }
        let parent_sid = &members[0].0;
        if members.iter().any(|member| &member.0 != parent_sid) {
            return Err(VbError::Conflict("编组成员必须同属一个父级".into()));
        }
        members.sort_by_key(|member| member.1);
        Ok(members.into_iter().map(|member| member.2).collect())
    }

    fn selected_ungroup_targets(&self) -> Result<Vec<GroupTarget>, VbError> {
        let selected = self.selection.as_slice();
        if selected.is_empty() {
            return Ok(Vec::new());
        }
        let selected_sids = selected.iter().map(String::as_str).collect::<HashSet<_>>();
        let mut targets = Vec::new();
        for sid in selected {
            let id = self
                .document
                .find_by_sid(sid)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            let node = self
                .document
                .node(id)
                .ok_or_else(|| VbError::NoSuchNode(format!("sid {sid}")))?;
            if !matches!(&node.kind, NodeKind::Group) {
                continue;
            }
            self.validate_unlocked_subtree(id)?;
            self.validate_unlocked_ancestors(id)?;

            let mut ancestor = node.parent;
            let mut has_selected_ancestor = false;
            while let Some(parent_id) = ancestor {
                let parent = self
                    .document
                    .node(parent_id)
                    .ok_or_else(|| VbError::NoSuchNode(format!("parent of sid {sid}")))?;
                if selected_sids.contains(parent.sid.as_str()) {
                    has_selected_ancestor = true;
                    break;
                }
                ancestor = parent.parent;
            }
            if has_selected_ancestor {
                continue;
            }

            let parent_id = node
                .parent
                .ok_or_else(|| VbError::Conflict("根编组不能解组".into()))?;
            let parent = self
                .document
                .node(parent_id)
                .ok_or_else(|| VbError::NoSuchNode(format!("parent of sid {sid}")))?;
            let index = parent
                .children
                .iter()
                .position(|child| {
                    self.document
                        .node(*child)
                        .is_some_and(|candidate| candidate.sid.as_str() == sid)
                })
                .ok_or_else(|| VbError::NoSuchNode(format!("parent slot of sid {sid}")))?;
            let member_sids = node
                .children
                .iter()
                .filter_map(|child| self.document.node(*child))
                .map(|child| child.sid.as_str().to_string())
                .collect::<Vec<_>>();
            if !member_sids.is_empty() {
                targets.push(GroupTarget {
                    sid: sid.clone(),
                    parent_sid: parent.sid.as_str().to_string(),
                    index,
                    member_sids,
                });
            }
        }
        // Ungroup right-to-left within each parent so earlier captured indices stay valid.
        targets.sort_by(|left, right| {
            left.parent_sid
                .cmp(&right.parent_sid)
                .then_with(|| right.index.cmp(&left.index))
        });
        Ok(targets)
    }

    /// 当前选择是否能作为同一父级下的对象编组。
    pub fn can_group_selection(&self) -> bool {
        self.selected_group_members()
            .is_ok_and(|members| members.len() >= 2)
    }

    /// 将选择中最上层的同级子树编为一个可撤销组。
    pub fn group_selection(&mut self) -> Result<Option<String>, VbError> {
        let member_sids = self.selected_group_members()?;
        if member_sids.len() < 2 {
            return Ok(None);
        }
        let group_sid = self.document.alloc_sid().as_str().to_string();
        let command = Command::Group {
            member_sids,
            name: "Group".into(),
            group_sid: group_sid.clone(),
            old_slots: None,
        };
        self.undo.end_session();
        self.undo.push(&mut self.document, command)?;
        self.selection.select(group_sid.clone());
        Ok(Some(group_sid))
    }

    /// 当前选择中是否含可安全解组的组。
    pub fn can_ungroup_selection(&self) -> bool {
        self.selected_ungroup_targets()
            .is_ok_and(|targets| !targets.is_empty())
    }

    /// 解开所选组,多个组作为一个可撤销事务并选中其直接成员。
    pub fn ungroup_selection(&mut self) -> Result<usize, VbError> {
        let targets = self.selected_ungroup_targets()?;
        if targets.is_empty() {
            return Ok(0);
        }
        let count = targets.len();
        let selected_sids = targets
            .iter()
            .flat_map(|target| target.member_sids.iter().cloned())
            .collect();
        let commands = targets
            .into_iter()
            .map(|target| Command::Ungroup {
                group_sid: target.sid,
                captured: None,
            })
            .collect::<Vec<_>>();
        self.undo.end_session();
        self.undo.push_compound(&mut self.document, commands)?;
        self.selection.set_all(selected_sids);
        Ok(count)
    }

    fn retain_live_selection(&mut self) {
        let live = self
            .selection
            .as_slice()
            .iter()
            .filter(|sid| self.document.find_by_sid(sid.as_str()).is_some())
            .cloned()
            .collect();
        self.selection.set_all(live);
    }

    fn normalize_active_artboard(&mut self) {
        let count = self.document.artboards().len();
        self.active_artboard_index = if count == 0 {
            0
        } else {
            self.active_artboard_index.min(count - 1)
        };
    }

    /// 撤销一条文档命令;无历史时返回 `false`。
    pub fn undo(&mut self) -> Result<bool, VbError> {
        if !self.undo.can_undo() {
            return Ok(false);
        }
        self.undo.undo(&mut self.document)?;
        self.retain_live_selection();
        self.selected_keyframes.clear();
        self.normalize_active_artboard();
        Ok(true)
    }

    /// 重做一条文档命令;无重做历史时返回 `false`。
    pub fn redo(&mut self) -> Result<bool, VbError> {
        if !self.undo.can_redo() {
            return Ok(false);
        }
        self.undo.redo(&mut self.document)?;
        self.retain_live_selection();
        self.selected_keyframes.clear();
        self.normalize_active_artboard();
        Ok(true)
    }

    /// 原子导出到原项目目录;只有完整写入成功后才清除 dirty 状态。
    pub fn save(&mut self) -> Result<Vec<PathBuf>, VbError> {
        let written = write_project(&self.document, &self.project_dir)?;
        self.undo.mark_saved();
        Ok(written)
    }

    /// 将指定画板投影为渲染就绪快照。
    ///
    /// 该值是临时视图数据,不是第二份可编辑文档;命令仍只能修改会话中的
    /// `Document`。`canvas-projection` feature 由 `vb_kit` 显式启用。
    #[cfg(feature = "canvas-projection")]
    pub fn canvas_snapshot(&self, index: usize) -> Result<CanvasSnapshot, VbError> {
        let artboard_count = self.document.artboards().len();
        let artboard = self
            .document
            .artboards()
            .get(index)
            .copied()
            .ok_or_else(|| VbError::NoSuchNode(format!("画板索引 {index}")))?;
        let mut draw_list = vb_render::encode::encode_artboard(&self.document, artboard)?;
        let mut image_cache = self
            .image_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut attached_bytes = 0usize;
        vb_render::encode::attach_images(&mut draw_list, &mut |src| {
            let bitmap = image_cache.load(&self.project_dir, src)?;
            let next_bytes = attached_bytes.saturating_add(bitmap.rgba.len());
            if next_bytes > ProjectImageCache::MAX_BYTES {
                return None;
            }
            attached_bytes = next_bytes;
            Some(bitmap)
        });
        drop(image_cache);
        let layers = project_layers(&self.document, artboard);

        Ok(CanvasSnapshot {
            title: self.title().to_string(),
            project_dir: self.project_dir.clone(),
            artboard_index: index,
            artboard_count,
            warnings: self.warnings.clone(),
            synthetic_artboard: self.synthetic_artboard,
            selected_sids: self.selection.as_slice().to_vec(),
            layers,
            draw_list,
        })
    }

    /// 将当前活跃画板投影为渲染就绪快照。
    #[cfg(feature = "canvas-projection")]
    pub fn active_canvas_snapshot(&self) -> Result<CanvasSnapshot, VbError> {
        self.canvas_snapshot(self.active_artboard_index)
    }

    /// Render a disposable snapshot of an artboard at a CSS animation time in seconds.
    #[cfg(feature = "canvas-projection")]
    pub fn canvas_snapshot_at(
        &self,
        index: usize,
        time_seconds: f64,
    ) -> Result<CanvasSnapshot, VbError> {
        let mut snapshot = self.canvas_snapshot(index)?;
        vb_render::animation::apply_document_animations(
            &self.document,
            &mut snapshot.draw_list,
            time_seconds,
        )?;
        Ok(snapshot)
    }

    /// Render the active artboard at a CSS animation time without mutating the document.
    #[cfg(feature = "canvas-projection")]
    pub fn active_canvas_snapshot_at(&self, time_seconds: f64) -> Result<CanvasSnapshot, VbError> {
        self.canvas_snapshot_at(self.active_artboard_index, time_seconds)
    }
}

/// 图层列表只读投影,名称/类型/层级来自文档树,不暴露 `NodeId`。
#[cfg(feature = "canvas-projection")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayerSnapshot {
    pub sid: String,
    pub name: String,
    pub kind: &'static str,
    pub depth: usize,
    pub has_children: bool,
    pub hidden: bool,
    pub locked: bool,
}

/// 按 artboard 树构造顶层在前、同级 z 序从上到下的平面 outliner 投影。
#[cfg(feature = "canvas-projection")]
fn project_layers(document: &Document, artboard: NodeId) -> Vec<LayerSnapshot> {
    let mut layers = Vec::new();
    let mut pending = vec![(artboard, 0_usize)];
    while let Some((id, depth)) = pending.pop() {
        let Some(node) = document.node(id) else {
            continue;
        };
        layers.push(LayerSnapshot {
            sid: node.sid.as_str().to_string(),
            name: if node.name.trim().is_empty() {
                node.tag.clone()
            } else {
                node.name.clone()
            },
            kind: node.kind.kind_name(),
            depth,
            has_children: !node.children.is_empty(),
            hidden: node.hidden,
            locked: node.locked,
        });
        for &child in &node.children {
            pending.push((child, depth.saturating_add(1)));
        }
    }
    layers
}

/// 不可编辑的画布投影,由 `vb_session` 从文档单一真相生成。
#[cfg(feature = "canvas-projection")]
#[derive(Debug, Clone)]
pub struct CanvasSnapshot {
    pub title: String,
    pub project_dir: PathBuf,
    pub artboard_index: usize,
    pub artboard_count: usize,
    pub warnings: Vec<String>,
    pub synthetic_artboard: bool,
    pub selected_sids: Vec<String>,
    pub layers: Vec<LayerSnapshot>,
    pub draw_list: vb_render::encode::DrawList,
}

impl ProjectSession {
    /// 按 `commands.yaml` 的目录 id 调度当前已迁移的命令。
    pub fn dispatch_catalog(&mut self, catalog_id: &str) -> CommandOutcome {
        self.dispatch(CommandRequest::new(CommandId::from_catalog(catalog_id)))
    }
}

impl Dispatcher for ProjectSession {
    fn dispatch(&mut self, req: CommandRequest) -> CommandOutcome {
        match req.id.as_str() {
            "canvas.nudge_left" => self.dispatch_nudge([-1.0, 0.0]),
            "canvas.nudge_right" => self.dispatch_nudge([1.0, 0.0]),
            "canvas.nudge_up" => self.dispatch_nudge([0.0, -1.0]),
            "canvas.nudge_down" => self.dispatch_nudge([0.0, 1.0]),
            "tool.select" => {
                self.tools.set_active(ToolId::Select);
                CommandOutcome::Applied
            }
            "tool.hand" => {
                self.tools.set_active(ToolId::Hand);
                CommandOutcome::Applied
            }
            "tool.rect" => {
                self.tools.set_active(ToolId::Rect);
                CommandOutcome::Applied
            }
            "tool.ellipse" => {
                self.tools.set_active(ToolId::Ellipse);
                CommandOutcome::Applied
            }
            "tool.artboard" => {
                self.tools.set_active(ToolId::Artboard);
                CommandOutcome::Applied
            }
            "tool.text" => {
                self.tools.set_active(ToolId::Text);
                CommandOutcome::Applied
            }
            "view.toggle_grid" => {
                self.grid_visible = !self.grid_visible;
                CommandOutcome::Applied
            }
            "align.to_selection" => {
                self.set_align_to(AlignTo::Selection);
                CommandOutcome::Applied
            }
            "align.to_key_object" => {
                self.set_align_to(AlignTo::KeyObject);
                CommandOutcome::Applied
            }
            "align.to_artboard" => {
                self.set_align_to(AlignTo::Artboard);
                CommandOutcome::Applied
            }
            "align.left" => self.dispatch_alignment(AlignMode::Left),
            "align.hcenter" => self.dispatch_alignment(AlignMode::HCenter),
            "align.right" => self.dispatch_alignment(AlignMode::Right),
            "align.top" => self.dispatch_alignment(AlignMode::Top),
            "align.vcenter" => self.dispatch_alignment(AlignMode::VCenter),
            "align.bottom" => self.dispatch_alignment(AlignMode::Bottom),
            "edit.undo" => match self.undo() {
                Ok(true) => CommandOutcome::Applied,
                Ok(false) => CommandOutcome::not_run("没有可撤销的操作"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "edit.redo" => match self.redo() {
                Ok(true) => CommandOutcome::Applied,
                Ok(false) => CommandOutcome::not_run("没有可重做的操作"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "file.save" => match self.save() {
                Ok(_) => CommandOutcome::Applied,
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "anim.create" => match self.create_default_animation() {
                Ok(true) => CommandOutcome::Applied,
                Ok(false) => CommandOutcome::not_run("当前对象已经有时间轴动画"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "anim.clear" => match self.clear_selected_animation() {
                Ok(true) => CommandOutcome::Applied,
                Ok(false) => CommandOutcome::not_run("当前对象没有可清除的动画"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "anim.keyframe_add" => match self.add_keyframes_at_playhead() {
                Ok(added) if added > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("播放头已有全部轨道关键帧"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "anim.keyframe_delete" => match self.delete_selected_keyframes() {
                Ok(deleted) if deleted > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("没有选中的关键帧"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "anim.keyframe_nudge_earlier" => match self.nudge_selected_keyframes_by_percent(-1) {
                Ok(moved) if moved > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("没有选中的关键帧"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "anim.keyframe_nudge_later" => match self.nudge_selected_keyframes_by_percent(1) {
                Ok(moved) if moved > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("没有选中的关键帧"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "anim.play_toggle" => match self.toggle_animation_playback() {
                Ok(true) => CommandOutcome::Applied,
                Ok(false) => CommandOutcome::not_run("动画播放状态未变化"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "anim.stop" => {
                if self.stop_animation_playback() {
                    CommandOutcome::Applied
                } else {
                    CommandOutcome::not_run("动画已停止并位于起点")
                }
            }
            "anim.loop_toggle" => {
                if self.set_animation_looping(!self.animation_looping) {
                    CommandOutcome::Applied
                } else {
                    CommandOutcome::not_run("循环状态未变化")
                }
            }
            "object.delete" => match self.delete_selection() {
                Ok(deleted) if deleted > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("没有可删除的对象"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "object.group" => match self.group_selection() {
                Ok(Some(_)) => CommandOutcome::Applied,
                Ok(None) => CommandOutcome::not_run("请在同一父级下选择至少两个未锁定对象"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "object.ungroup" => match self.ungroup_selection() {
                Ok(count) if count > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("没有可解组的对象"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "view.next_artboard" => match self.next_artboard() {
                Ok(true) => CommandOutcome::Applied,
                Ok(false) => CommandOutcome::not_run("当前项目只有一个画板"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "view.prev_artboard" => match self.previous_artboard() {
                Ok(true) => CommandOutcome::Applied,
                Ok(false) => CommandOutcome::not_run("当前项目只有一个画板"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "edit.select_all" => match self.select_all_in_artboard(self.active_artboard_index()) {
                Ok(selected) if selected > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("当前画板没有可选节点"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "edit.copy" => match self.copy_selection() {
                Ok(copied) if copied > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("没有可复制的对象"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "edit.cut" => match self.cut_selection() {
                Ok(cut) if cut > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("没有可剪切的对象"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "edit.paste" => match self.paste_selection(false) {
                Ok(pasted) if pasted > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("剪贴板为空"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            "edit.paste_in_place" => match self.paste_selection(true) {
                Ok(pasted) if pasted > 0 => CommandOutcome::Applied,
                Ok(_) => CommandOutcome::not_run("剪贴板为空"),
                Err(error) => CommandOutcome::not_run(error.to_string()),
            },
            other => CommandOutcome::not_run(format!("新宿主暂不支持命令 {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use vb_animation::timeline::{Keyframe, TrackProp};

    static TEMP_PROJECT_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    const PIXEL_PNG_BYTES: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8,
        0xcf, 0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn fixture_session(document: Document) -> FixtureSession {
        let temp_project = TempProjectDir::new();
        let session = ProjectSession::from_import(ImportResult {
            doc: document,
            warnings: vec!["外链资源未加载".to_string()],
            project_dir: temp_project.path().to_path_buf(),
            synthetic_artboard: false,
            font_faces: Vec::new(),
        });
        FixtureSession {
            session,
            _temp_project: temp_project,
        }
    }

    fn sample_animation_model() -> AnimModel {
        AnimModel {
            duration_ms: 1_000.0,
            delay_ms: 100.0,
            iterations: 2.0,
            tracks: vec![
                (
                    TrackProp::Position,
                    vec![
                        Keyframe {
                            t: 0.0,
                            value: KeyValue::Offset(0.0, 0.0),
                            easing: Easing::Linear,
                        },
                        Keyframe {
                            t: 1.0,
                            value: KeyValue::Offset(40.0, 20.0),
                            easing: Easing::EaseInOut,
                        },
                    ],
                ),
                (
                    TrackProp::Opacity,
                    vec![
                        Keyframe {
                            t: 0.0,
                            value: KeyValue::Alpha(1.0),
                            easing: Easing::Linear,
                        },
                        Keyframe {
                            t: 1.0,
                            value: KeyValue::Alpha(0.5),
                            easing: Easing::EaseInOut,
                        },
                    ],
                ),
            ],
            extra: Vec::new(),
            preset: None,
        }
    }

    fn sample_keyframe_move_model() -> AnimModel {
        let mut model = sample_animation_model();
        model.duration_ms = 2_000.0;
        for (_, frames) in &mut model.tracks {
            frames[1].t = 1.8;
        }
        model
            .tracks
            .iter_mut()
            .find(|(track, _)| *track == TrackProp::Position)
            .expect("position track")
            .1[0]
            .t = 0.8;
        model
            .tracks
            .iter_mut()
            .find(|(track, _)| *track == TrackProp::Opacity)
            .expect("opacity track")
            .1[0]
            .t = 1.2;
        model
    }

    struct TempProjectDir(PathBuf);

    impl TempProjectDir {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let sequence = TEMP_PROJECT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "vellumbench-session-edit-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("create temporary project directory");
            std::fs::write(
                dir.join("index.html"),
                "<html><head><title>Session edit</title></head><body><div data-vb-artboard style=\"width:1440px;height:900px;background:#ffffff\"></div></body></html>",
            )
            .expect("write temporary project HTML");
            Self(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempProjectDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct FixtureSession {
        session: ProjectSession,
        _temp_project: TempProjectDir,
    }

    impl std::ops::Deref for FixtureSession {
        type Target = ProjectSession;

        fn deref(&self) -> &Self::Target {
            &self.session
        }
    }

    impl std::ops::DerefMut for FixtureSession {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.session
        }
    }

    #[test]
    fn new_project_writes_default_document_and_never_overwrites_nonempty_folder() {
        let parent = TempProjectDir::new();
        let new_dir = parent.path().join("New Design");
        std::fs::create_dir(&new_dir).expect("create empty project directory");

        let session = ProjectSession::new_project(&new_dir).expect("create new project");
        assert_eq!(session.title(), "New Design");
        assert_eq!(session.artboard_count(), 1);
        assert!(!session.is_dirty());
        assert!(new_dir.join("index.html").is_file());
        let reopened = ProjectSession::open(&new_dir).expect("reopen new project");
        assert_eq!(reopened.title(), "New Design");
        assert_eq!(reopened.artboard_count(), 1);
        assert!(!reopened.is_dirty());

        let occupied = parent.path().join("Keep Existing");
        std::fs::create_dir(&occupied).expect("create occupied directory");
        let sentinel = occupied.join("keep.txt");
        std::fs::write(&sentinel, "user data").expect("write sentinel");
        assert!(ProjectSession::new_project(&occupied).is_err());
        assert_eq!(
            std::fs::read_to_string(sentinel).expect("preserved sentinel"),
            "user data"
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn shape_edit_undo_redo_save_and_reload_round_trip() {
        let project = TempProjectDir::new();
        let mut session = ProjectSession::open(project.path()).expect("open HTML fixture");
        let original_count = session
            .canvas_snapshot(0)
            .expect("initial artboard")
            .draw_list
            .items
            .len();
        assert!(!session.is_dirty());
        assert!(!session.can_undo());
        assert!(!session.can_redo());

        let rectangle_sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        assert!(session.is_dirty());
        assert!(session.can_undo());
        assert_eq!(session.selected_sids(), [rectangle_sid.as_str()]);
        let edited = session.canvas_snapshot(0).expect("edited artboard");
        assert_eq!(edited.draw_list.items.len(), original_count + 1);
        assert!(edited
            .draw_list
            .items
            .iter()
            .any(|item| item.sid == rectangle_sid));

        assert!(session.dispatch_catalog("edit.undo").is_applied());
        assert!(!session.can_undo());
        assert!(session.can_redo());
        assert!(session.selected_sids().is_empty());
        assert_eq!(
            session
                .canvas_snapshot(0)
                .expect("undone artboard")
                .draw_list
                .items
                .len(),
            original_count
        );

        assert!(session.dispatch_catalog("edit.redo").is_applied());
        assert!(session.can_undo());
        assert!(session.is_dirty());
        let ellipse_sid = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 140.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert ellipse");
        assert!(session.dispatch_catalog("edit.undo").is_applied());
        let without_ellipse = session.canvas_snapshot(0).expect("undone ellipse");
        assert_eq!(without_ellipse.draw_list.items.len(), original_count + 1);
        assert!(!without_ellipse
            .draw_list
            .items
            .iter()
            .any(|item| item.sid == ellipse_sid));
        assert!(session.can_redo());

        assert!(session.dispatch_catalog("edit.redo").is_applied());
        let with_ellipse = session.canvas_snapshot(0).expect("ellipse artboard");
        assert_eq!(with_ellipse.draw_list.items.len(), original_count + 2);
        assert!(with_ellipse
            .draw_list
            .items
            .iter()
            .any(|item| item.sid == ellipse_sid && item.ellipse));

        assert!(session.dispatch_catalog("file.save").is_applied());
        assert!(!session.is_dirty());

        let reopened = ProjectSession::open(project.path()).expect("reload saved project");
        let persisted = reopened.canvas_snapshot(0).expect("persisted artboard");
        assert!(persisted
            .draw_list
            .items
            .iter()
            .any(|item| item.sid == rectangle_sid));
        assert!(persisted
            .draw_list
            .items
            .iter()
            .any(|item| item.sid == ellipse_sid && item.ellipse));
        assert_eq!(persisted.draw_list.items.len(), original_count + 2);
    }

    #[cfg(feature = "canvas-projection")]
    fn item_rect(session: &ProjectSession, sid: &str) -> [f64; 4] {
        session
            .canvas_snapshot(0)
            .expect("project artboard")
            .draw_list
            .items
            .into_iter()
            .find(|item| item.sid == sid)
            .expect("stable item id exists in preview")
            .rect
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn delete_selection_is_compound_undoable_and_preserves_last_artboard() {
        let mut session = fixture_session(Document::new_default());
        let rect_sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        let ellipse_sid = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 140.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert ellipse");
        session.select_sid(Some(&rect_sid), ModifierState::default());
        session.select_sid(
            Some(&ellipse_sid),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let before_delete = session.undo.undo_len();

        assert!(session.can_delete_selection());
        assert_eq!(session.delete_selection().unwrap(), 2);
        assert_eq!(session.undo.undo_len(), before_delete + 1);
        assert!(session.selected_sids().is_empty());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 1);

        assert!(session.undo().unwrap());
        let restored = session.canvas_snapshot(0).unwrap();
        assert_eq!(restored.draw_list.items.len(), 3);
        assert!(restored
            .draw_list
            .items
            .iter()
            .any(|item| item.sid == rect_sid));
        assert!(restored
            .draw_list
            .items
            .iter()
            .any(|item| item.sid == ellipse_sid));
        assert!(session.redo().unwrap());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 1);

        let artboard = session.document.artboards()[0];
        let artboard_sid = session
            .document
            .node(artboard)
            .unwrap()
            .sid
            .as_str()
            .to_string();
        session.select_sid(Some(&artboard_sid), ModifierState::default());
        let revision = session.document.rev;
        assert!(!session.can_delete_selection());
        assert!(session.delete_selection().is_err());
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.artboard_count(), 1);
        assert!(matches!(
            session.dispatch_catalog("object.delete"),
            CommandOutcome::NotRun { .. }
        ));
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn selected_layer_flags_are_atomic_and_locked_nodes_cannot_move_or_delete() {
        let mut session = fixture_session(Document::new_default());
        let rect_sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        let ellipse_sid = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 140.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert ellipse");
        session.select_sid(Some(&rect_sid), ModifierState::default());
        session.select_sid(
            Some(&ellipse_sid),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let history_before = session.undo.undo_len();

        assert_eq!(
            session.set_selected_flags(Some(true), Some(true)).unwrap(),
            2
        );
        assert_eq!(session.undo.undo_len(), history_before + 1);
        let snapshot = session.canvas_snapshot(0).unwrap();
        for sid in [&rect_sid, &ellipse_sid] {
            let layer = snapshot
                .layers
                .iter()
                .find(|layer| &layer.sid == sid)
                .expect("selected layer exists");
            assert!(layer.hidden);
            assert!(layer.locked);
        }
        let revision = session.document.rev;
        assert!(session.move_selection_by([5.0, 0.0]).is_err());
        assert!(!session.can_delete_selection());
        assert!(session.delete_selection().is_err());
        assert_eq!(session.document.rev, revision);

        assert!(session.undo().unwrap());
        let restored = session.canvas_snapshot(0).unwrap();
        for sid in [&rect_sid, &ellipse_sid] {
            let layer = restored
                .layers
                .iter()
                .find(|layer| &layer.sid == sid)
                .expect("restored layer exists");
            assert!(!layer.hidden);
            assert!(!layer.locked);
        }
        assert!(session.redo().unwrap());
        assert_eq!(
            session.set_selected_flags(Some(true), Some(true)).unwrap(),
            0
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn moving_multiple_selected_nodes_is_discrete_and_undoable() {
        let mut session = fixture_session(Document::new_default());
        let rect_sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        let ellipse_sid = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 140.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert ellipse");
        session.select_sid(Some(&rect_sid), ModifierState::default());
        session.select_sid(
            Some(&ellipse_sid),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let selected = vec![rect_sid.clone(), ellipse_sid.clone()];
        assert_eq!(session.selected_sids(), selected);
        let before_rect = item_rect(&session, &rect_sid);
        let before_ellipse = item_rect(&session, &ellipse_sid);
        let history_before = session.undo.undo_len();

        assert_eq!(session.move_selection_by([12.0, -7.0]).unwrap(), 2);
        assert_eq!(session.move_selection_by([3.0, 4.0]).unwrap(), 2);
        assert_eq!(session.undo.undo_len(), history_before + 2);
        assert_eq!(item_rect(&session, &rect_sid), [35.0, 27.0, 80.0, 50.0]);
        assert_eq!(item_rect(&session, &ellipse_sid), [155.0, 27.0, 80.0, 50.0]);

        assert!(session.undo().unwrap());
        assert_eq!(item_rect(&session, &rect_sid), [32.0, 23.0, 80.0, 50.0]);
        assert_eq!(session.selected_sids(), selected);
        assert!(session.undo().unwrap());
        assert_eq!(item_rect(&session, &rect_sid), before_rect);
        assert_eq!(item_rect(&session, &ellipse_sid), before_ellipse);
        assert_eq!(session.selected_sids(), selected);
        assert!(session.redo().unwrap());
        assert_eq!(item_rect(&session, &rect_sid), [32.0, 23.0, 80.0, 50.0]);
        assert!(session.redo().unwrap());
        assert_eq!(item_rect(&session, &rect_sid), [35.0, 27.0, 80.0, 50.0]);
        assert_eq!(session.selected_sids(), selected);

        let project_dir = session.project_dir.clone();
        session.save().unwrap();
        assert!(!session.is_dirty());
        let reopened = ProjectSession::open(project_dir).unwrap();
        assert_eq!(item_rect(&reopened, &rect_sid), [35.0, 27.0, 80.0, 50.0]);
        assert_eq!(
            item_rect(&reopened, &ellipse_sid),
            [155.0, 27.0, 80.0, 50.0]
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn smart_snap_selection_union_aligns_to_fixed_sibling_and_alt_bypasses() {
        let mut session = fixture_session(Document::new_default());
        let rect_a = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 20.0,
                    width: 20.0,
                    height: 20.0,
                },
            )
            .expect("insert first selected rectangle");
        let rect_b = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 60.0,
                    y: 20.0,
                    width: 20.0,
                    height: 20.0,
                },
            )
            .expect("insert second selected rectangle");
        session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 100.0,
                    y: 20.0,
                    width: 20.0,
                    height: 20.0,
                },
            )
            .expect("insert fixed sibling");
        session.select_sid(Some(&rect_a), ModifierState::default());
        session.select_sid(
            Some(&rect_b),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let selection = vec![rect_a, rect_b];
        let revision = session.document.rev;

        let snapped = session
            .snap_selection_delta([15.0, 0.0], 1.0, false)
            .expect("selection snap preview");
        assert_eq!(snapped.delta, [20.0, 0.0]);
        assert!(!snapped.guides.is_empty());
        assert_eq!(session.selected_sids(), selection);
        assert_eq!(session.document.rev, revision, "preview is non-mutating");

        let direct = session
            .snap_selection_delta([15.0, 0.0], 1.0, true)
            .expect("Alt bypass preview");
        assert_eq!(direct.delta, [15.0, 0.0]);
        assert!(direct.guides.is_empty());
        assert_eq!(session.document.rev, revision);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn undo_to_saved_project_state_clears_dirty_across_branches() {
        let mut session = fixture_session(Document::new_default());
        session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        session.save().expect("save baseline");
        assert!(!session.is_dirty());

        session.move_selection_by([10.0, 5.0]).unwrap();
        assert!(session.is_dirty());
        assert!(session.undo().unwrap());
        assert!(!session.is_dirty(), "undo returns to the savepoint");
        assert!(session.redo().unwrap());
        assert!(session.is_dirty());
        assert!(session.undo().unwrap());
        assert!(!session.is_dirty());

        // A divergent edit at the same undo depth must get a new state token.
        session.move_selection_by([3.0, 2.0]).unwrap();
        assert!(session.is_dirty());
        assert!(session.undo().unwrap());
        assert!(
            !session.is_dirty(),
            "branch undo returns to the same savepoint"
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn inspector_geometry_edit_is_single_object_undoable_and_bounds_checked() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        let initial = session.inspector_snapshot();
        assert_eq!(initial.selected_count, 1);
        let selected = initial.selected.expect("single selected node");
        assert_eq!(selected.sid, sid);
        assert_eq!(selected.name, "矩形");
        assert_eq!(selected.kind, "box");
        assert_eq!(selected.fill.as_deref(), Some("#4F9FFF"));
        assert_eq!(
            selected.geometry,
            CanvasRect {
                x: 20.0,
                y: 30.0,
                width: 80.0,
                height: 50.0,
            }
        );

        let edited = CanvasRect {
            x: 42.0,
            y: 56.0,
            width: 130.0,
            height: 90.0,
        };
        assert!(session.set_selected_geometry(edited).unwrap());
        assert_eq!(
            session.inspector_snapshot().selected.unwrap().geometry,
            edited
        );
        assert!(session.undo().unwrap());
        assert_eq!(session.selected_sids(), [sid.as_str()]);
        assert_eq!(
            session.inspector_snapshot().selected.unwrap().geometry,
            selected.geometry
        );
        assert!(session.redo().unwrap());
        assert_eq!(
            session.inspector_snapshot().selected.unwrap().geometry,
            edited
        );
        assert!(!session.set_selected_geometry(edited).unwrap());

        let revision = session.document.rev;
        let invalid = CanvasRect {
            x: 999_950.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        };
        assert!(session.set_selected_geometry(invalid).is_err());
        assert_eq!(session.document.rev, revision);
        assert_eq!(
            session.inspector_snapshot().selected.unwrap().geometry,
            edited
        );

        session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 200.0,
                    y: 30.0,
                    width: 40.0,
                    height: 40.0,
                },
            )
            .expect("insert ellipse");
        assert_eq!(session.inspector_snapshot().selected_count, 1);
        let other_sid = session.selected_sids()[0].clone();
        session.select_sid(Some(&sid), ModifierState::default());
        session.select_sid(
            Some(&other_sid),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let multiple = session.inspector_snapshot();
        assert_eq!(multiple.selected_count, 2);
        assert!(multiple.selected.is_none());
        let revision = session.document.rev;
        assert!(session.set_selected_geometry(edited).is_err());
        assert_eq!(session.document.rev, revision);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn inspector_solid_fill_edit_normalizes_undoes_and_persists() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        fixture.save().expect("save fill baseline");
        let initial = fixture
            .inspector_snapshot()
            .selected
            .expect("box selection");
        assert!(initial.fill_editable);
        assert_eq!(initial.fill.as_deref(), Some("#4F9FFF"));

        assert!(fixture
            .set_selected_fill(&sid, "rgba(255, 0, 0, 1)")
            .expect("set normalized box fill"));
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .fill
                .as_deref(),
            Some("#f00")
        );
        assert!(!fixture
            .set_selected_fill(&sid, "#FF0000")
            .expect("equivalent color is a no-op"));
        let invalid_revision = fixture.document.rev;
        assert!(fixture.set_selected_fill(&sid, "not-a-color").is_err());
        assert_eq!(fixture.document.rev, invalid_revision);
        assert!(fixture.is_dirty());
        assert!(fixture.undo().expect("undo fill"));
        assert!(!fixture.is_dirty());
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .fill
                .as_deref(),
            Some("#4F9FFF")
        );
        assert!(fixture.redo().expect("redo fill"));
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .fill
                .as_deref(),
            Some("#f00")
        );

        let project_dir = fixture.project_dir().to_path_buf();
        fixture.save().expect("save colored box");
        let mut reopened = ProjectSession::open(project_dir).expect("reopen colored box");
        reopened.select_sid(Some(&sid), ModifierState::default());
        assert_eq!(
            reopened
                .inspector_snapshot()
                .selected
                .unwrap()
                .fill
                .as_deref(),
            Some("#f00")
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn inspector_text_color_uses_color_property_and_complex_fills_stay_read_only() {
        let mut session = fixture_session(Document::new_default());
        let text_sid = session.create_text(0, [10.0, 20.0]).expect("insert text");
        assert!(session.inspector_snapshot().selected.unwrap().fill_editable);
        assert!(session
            .set_selected_fill(&text_sid, "hsl(0 100% 50%)")
            .expect("set text color"));
        let text_id = session.document.find_by_sid(&text_sid).unwrap();
        let text_node = session.document.node(text_id).unwrap();
        assert_eq!(text_node.style_get("color"), Some("#f00"));
        assert_eq!(text_node.style_get("background-color"), None);

        let box_sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 80.0,
                    y: 20.0,
                    width: 40.0,
                    height: 30.0,
                },
            )
            .expect("insert gradient box");
        let box_id = session.document.find_by_sid(&box_sid).unwrap();
        session
            .document
            .nodes_mut()
            .get_mut(box_id)
            .expect("gradient box node")
            .style_set("background-image", "none");
        assert!(session.inspector_snapshot().selected.unwrap().fill_editable);
        session
            .document
            .nodes_mut()
            .get_mut(box_id)
            .expect("gradient box node")
            .style_set("background", "linear-gradient(red, blue)");
        let revision = session.document.rev;
        let gradient = session.inspector_snapshot().selected.unwrap();
        assert!(!gradient.fill_editable);
        assert_eq!(gradient.fill.as_deref(), Some("linear-gradient(red, blue)"));
        assert!(session.set_selected_fill(&box_sid, "#00ff00").is_err());
        assert!(session.set_selected_fill(&box_sid, "not-a-color").is_err());
        assert_eq!(session.document.rev, revision);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn inspector_fill_rejects_locked_and_multi_selection_atomically() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 40.0,
                },
            )
            .expect("insert rectangle");
        session
            .set_selected_flags(None, Some(true))
            .expect("lock selected rectangle");
        let locked = session
            .inspector_snapshot()
            .selected
            .expect("locked selection");
        assert!(locked.locked);
        assert!(!locked.fill_editable);
        let revision = session.document.rev;
        assert!(session.set_selected_fill(&sid, "#fff").is_err());
        assert_eq!(session.document.rev, revision);

        session
            .set_selected_flags(None, Some(false))
            .expect("unlock selected rectangle");
        let other_sid = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 80.0,
                    y: 20.0,
                    width: 30.0,
                    height: 40.0,
                },
            )
            .expect("insert ellipse");
        session.select_sid(Some(&sid), ModifierState::default());
        session.select_sid(
            Some(&other_sid),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let multiple = session.inspector_snapshot();
        assert_eq!(multiple.selected_count, 2);
        assert!(multiple.selected.is_none());
        let revision = session.document.rev;
        assert!(session.set_selected_fill(&sid, "#fff").is_err());
        assert_eq!(session.document.rev, revision);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn text_tool_creates_editable_node_and_round_trips() {
        let mut session = fixture_session(Document::new_default());
        assert!(session.dispatch_catalog("tool.text").is_applied());
        assert_eq!(session.active_tool(), ToolId::Text);

        let sid = session.create_text(0, [120.0, 80.0]).expect("insert text");
        let selected = session
            .inspector_snapshot()
            .selected
            .expect("new text is selected");
        assert_eq!(selected.sid, sid);
        assert_eq!(selected.text.as_deref(), Some("Text"));
        assert!(selected.text_editable);
        assert!(session.is_dirty());

        let revision = session.document.rev;
        assert!(session.create_text(0, [f64::NAN, 0.0]).is_err());
        assert_eq!(session.document.rev, revision);
        assert!(session.undo().expect("undo text insertion"));
        assert_eq!(session.inspector_snapshot().selected_count, 0);
        assert!(session.redo().expect("redo text insertion"));
        session.select_sid(Some(&sid), ModifierState::default());
        assert!(session.set_selected_text(&sid, "Heading").unwrap());

        let project_dir = session.project_dir().to_path_buf();
        session.save().expect("save text project");
        let mut reopened = ProjectSession::open(project_dir).expect("reopen text project");
        reopened.select_sid(Some(&sid), ModifierState::default());
        assert_eq!(
            reopened
                .inspector_snapshot()
                .selected
                .expect("reopened text selection")
                .text
                .as_deref(),
            Some("Heading")
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn inspector_text_edit_is_undoable_and_round_trips_plain_text() {
        let temp_project = TempProjectDir::new();
        std::fs::write(
            temp_project.path().join("index.html"),
            r#"<!doctype html><html><head><title>Text fixture</title></head><body><div data-vb-artboard style="width:1440px;height:900px"><h1 data-vb-name="Headline" style="position:absolute;left:20px;top:30px;width:300px;height:70px;color:#ffe9c9">Before</h1></div></body></html>"#,
        )
        .expect("write text fixture");
        let mut fixture = FixtureSession {
            session: ProjectSession::from_import(
                import_project(temp_project.path()).expect("import text fixture"),
            ),
            _temp_project: temp_project,
        };
        let sid = fixture
            .canvas_snapshot(0)
            .expect("text artboard")
            .layers
            .into_iter()
            .find(|layer| layer.kind == "text" && layer.name == "Headline")
            .expect("plain text layer")
            .sid;
        fixture.select_sid(Some(&sid), ModifierState::default());
        let initial = fixture
            .inspector_snapshot()
            .selected
            .expect("text selection");
        assert!(initial.text_editable);
        assert_eq!(initial.text.as_deref(), Some("Before"));

        assert!(fixture.set_selected_text(&sid, "After").unwrap());
        assert!(fixture.is_dirty());
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .text
                .as_deref(),
            Some("After")
        );
        assert!(fixture.undo().unwrap());
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .text
                .as_deref(),
            Some("Before")
        );
        assert!(fixture.redo().unwrap());
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .text
                .as_deref(),
            Some("After")
        );

        let project_dir = fixture.project_dir().to_path_buf();
        fixture.save().unwrap();
        assert!(!fixture.is_dirty());
        let mut reopened = ProjectSession::open(project_dir).expect("reopen edited text");
        reopened.select_sid(Some(&sid), ModifierState::default());
        assert_eq!(
            reopened
                .inspector_snapshot()
                .selected
                .expect("reopened text selection")
                .text
                .as_deref(),
            Some("After")
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn inspector_text_edit_refuses_rich_text_without_mutation() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert text fixture node");
        let node_id = fixture.document.find_by_sid(&sid).expect("fixture node");
        fixture.document.nodes_mut().get_mut(node_id).unwrap().kind = NodeKind::Text {
            text: "Rich".to_string(),
            mode: vb_doc::model::TextMode::Point,
            segments: vec![vb_doc::model::TextSeg {
                start: 0,
                end: 1,
                style: vb_doc::model::SegStyle::default(),
            }],
        };
        let revision = fixture.document.rev;
        assert!(
            !fixture
                .inspector_snapshot()
                .selected
                .expect("rich text selection")
                .text_editable
        );
        assert!(fixture.set_selected_text(&sid, "Changed").is_err());
        assert_eq!(fixture.document.rev, revision);
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .text
                .as_deref(),
            Some("Rich")
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn inspector_text_editor_blocks_multiline_and_newline_submissions() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert text fixture node");
        let node_id = fixture.document.find_by_sid(&sid).expect("fixture node");
        fixture.document.nodes_mut().get_mut(node_id).unwrap().kind = NodeKind::Text {
            text: "Single line".to_string(),
            mode: vb_doc::model::TextMode::Point,
            segments: Vec::new(),
        };
        assert!(
            fixture
                .inspector_snapshot()
                .selected
                .expect("plain text selection")
                .text_editable
        );

        let revision = fixture.document.rev;
        assert!(fixture
            .set_selected_text(&sid, "first line\nsecond line")
            .is_err());
        assert_eq!(fixture.document.rev, revision);
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .text
                .as_deref(),
            Some("Single line")
        );

        fixture.document.nodes_mut().get_mut(node_id).unwrap().kind = NodeKind::Text {
            text: "First line\nSecond line".to_string(),
            mode: vb_doc::model::TextMode::Point,
            segments: Vec::new(),
        };
        assert!(
            !fixture
                .inspector_snapshot()
                .selected
                .expect("multiline selection")
                .text_editable
        );
        let multiline_revision = fixture.document.rev;
        assert!(fixture.set_selected_text(&sid, "single line").is_err());
        assert_eq!(fixture.document.rev, multiline_revision);
        assert_eq!(
            fixture
                .inspector_snapshot()
                .selected
                .unwrap()
                .text
                .as_deref(),
            Some("First line\nSecond line")
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn pointer_selection_preserves_group_and_shift_toggles() {
        let mut session = fixture_session(Document::new_default());
        let rect_sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        let ellipse_sid = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 140.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert ellipse");
        let plain = ModifierState::default();
        let shift = ModifierState {
            shift: true,
            ..plain
        };

        assert_eq!(
            session.begin_pointer_selection(Some(&rect_sid), plain),
            Some(vec![rect_sid.clone()])
        );
        assert_eq!(
            session.begin_pointer_selection(Some(&ellipse_sid), shift),
            None
        );
        let group = vec![rect_sid.clone(), ellipse_sid.clone()];
        assert_eq!(session.selected_sids(), group);
        assert_eq!(
            session.begin_pointer_selection(Some(&rect_sid), plain),
            Some(group.clone())
        );
        assert_eq!(session.selected_sids(), group);
        assert_eq!(
            session.begin_pointer_selection(Some(&rect_sid), shift),
            None
        );
        assert_eq!(session.selected_sids(), [ellipse_sid]);
        assert_eq!(session.begin_pointer_selection(None, plain), None);
        assert!(session.selected_sids().is_empty());
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn moving_selection_rejects_invalid_offsets_without_mutating_document() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: -20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        let before = item_rect(&session, &sid);
        let revision = session.document.rev;

        for delta in [[f64::NAN, 0.0], [1_000_001.0, 0.0], [1_000_000.0, 0.0]] {
            assert!(session.move_selection_by(delta).is_err());
            assert_eq!(session.document.rev, revision);
            assert_eq!(item_rect(&session, &sid), before);
        }
        assert_eq!(session.move_selection_by([0.0, 0.0]).unwrap(), 0);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn copy_paste_rekeys_subtrees_offsets_and_round_trips_as_single_undo_steps() {
        let mut session = fixture_session(Document::new_default());
        let rectangle_sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 24.0,
                    y: 36.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        let ellipse_sid = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 140.0,
                    y: 160.0,
                    width: 60.0,
                    height: 40.0,
                },
            )
            .expect("insert ellipse");
        session.save().expect("save source shapes");
        let clean_revision = session.document.rev;
        session.select_sid(Some(&rectangle_sid), ModifierState::default());
        session.select_sid(
            Some(&ellipse_sid),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );

        assert!(session.can_copy_selection());
        assert!(session.can_cut_selection());
        assert_eq!(session.copy_selection().unwrap(), 2);
        assert_eq!(session.document.rev, clean_revision, "copy is read-only");
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 3);
        let undo_before = session.undo.undo_len();

        assert_eq!(session.paste_selection(false).unwrap(), 2);
        assert_eq!(session.undo.undo_len(), undo_before + 1);
        let offset_sids = session.selected_sids().to_vec();
        assert_eq!(offset_sids.len(), 2);
        assert_ne!(offset_sids[0], offset_sids[1]);
        assert!(offset_sids
            .iter()
            .all(|sid| sid != &rectangle_sid && sid != &ellipse_sid));
        assert_eq!(item_rect(&session, &offset_sids[0])[0], 44.0);
        assert_eq!(item_rect(&session, &offset_sids[0])[1], 56.0);
        assert_eq!(item_rect(&session, &offset_sids[1])[0], 160.0);
        assert_eq!(item_rect(&session, &offset_sids[1])[1], 180.0);
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 5);

        assert_eq!(session.paste_selection(true).unwrap(), 2);
        assert_eq!(session.undo.undo_len(), undo_before + 2);
        let in_place_sids = session.selected_sids().to_vec();
        assert_eq!(item_rect(&session, &in_place_sids[0])[0], 24.0);
        assert_eq!(item_rect(&session, &in_place_sids[1])[0], 140.0);
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 7);
        session.save().expect("save duplicated shapes");
        let reopened = ProjectSession::open(session.project_dir()).expect("reopen pasted project");
        assert_eq!(
            reopened.canvas_snapshot(0).unwrap().draw_list.items.len(),
            7
        );

        assert!(session.undo().unwrap());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 5);
        assert!(session.undo().unwrap());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 3);
        assert!(session.redo().unwrap());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 5);
        assert!(session.redo().unwrap());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 7);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn cut_is_atomic_undoable_and_keeps_private_clipboard() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 12.0,
                    y: 18.0,
                    width: 40.0,
                    height: 30.0,
                },
            )
            .expect("insert rectangle");
        session.save().expect("save source rectangle");
        assert!(session.can_cut_selection());

        assert_eq!(session.cut_selection().unwrap(), 1);
        assert!(session.selected_sids().is_empty());
        assert!(session.can_paste_selection());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 1);
        assert!(session.undo().unwrap());
        assert!(session.document.find_by_sid(&sid).is_some());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 2);

        assert_eq!(session.paste_selection(false).unwrap(), 1);
        let pasted_sid = session.selected_sids()[0].clone();
        assert_ne!(pasted_sid, sid);
        assert_eq!(item_rect(&session, &pasted_sid), [32.0, 38.0, 40.0, 30.0]);
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 3);
        session.save().expect("save cut-and-pasted project");
        let reopened = ProjectSession::open(session.project_dir()).expect("reopen cut project");
        assert_eq!(
            reopened.canvas_snapshot(0).unwrap().draw_list.items.len(),
            3
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn edit_clipboard_commands_dispatch_and_empty_clipboard_is_safe() {
        let mut session = fixture_session(Document::new_default());
        assert!(!session.can_paste_selection());
        assert!(matches!(
            session.dispatch_catalog("edit.paste"),
            CommandOutcome::NotRun { .. }
        ));
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 10.0,
                    y: 20.0,
                    width: 40.0,
                    height: 30.0,
                },
            )
            .expect("insert rectangle");
        assert!(session.dispatch_catalog("edit.copy").is_applied());
        assert!(session.dispatch_catalog("edit.paste").is_applied());
        assert_ne!(session.selected_sids()[0], sid);
        assert!(session.dispatch_catalog("edit.paste_in_place").is_applied());
        assert!(session.dispatch_catalog("edit.cut").is_applied());
        assert!(session.dispatch_catalog("edit.undo").is_applied());
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn rectangle_rejects_invalid_geometry_without_mutating_document() {
        let mut session = fixture_session(Document::new_default());
        for rect in [
            CanvasRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 20.0,
            },
            CanvasRect {
                x: f64::NAN,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
            CanvasRect {
                x: 1_000_001.0,
                y: 0.0,
                width: 20.0,
                height: 20.0,
            },
        ] {
            assert!(session.create_rectangle(0, rect).is_err());
            assert!(session.create_ellipse(0, rect).is_err());
        }
        assert!(!session.is_dirty());
        assert!(!session.can_undo());
        assert_eq!(session.canvas_snapshot(0).unwrap().draw_list.items.len(), 1);
    }

    #[test]
    fn reports_missing_index_html_for_project_directory() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "vellumbench-session-missing-index-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create fixture directory");
        let result = ProjectSession::open(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        let error = result.expect_err("directory without index.html must not open");
        assert!(error.to_string().contains("index.html"));
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn opens_html_project_and_projects_its_artboard() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "vellumbench-session-html-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create fixture directory");
        std::fs::write(
            dir.join("index.html"),
            "<html><head><title>Session smoke</title></head><body><div data-vb-artboard style=\"width:1440px;height:900px;background:#ffffff\"></div></body></html>",
        )
        .expect("write HTML fixture");

        let result = ProjectSession::open(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let session = result.expect("valid HTML project must open");
        let snapshot = session.canvas_snapshot(0).expect("first artboard snapshot");

        assert_eq!(session.title(), "Session smoke");
        assert_eq!(session.artboard_count(), 1);
        assert_eq!(snapshot.title, "Session smoke");
        assert_eq!(snapshot.artboard_count, 1);
        assert!(snapshot.draw_list.w > 0.0);
        assert!(snapshot.draw_list.h > 0.0);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn layer_snapshot_preserves_tree_depth_and_topmost_sibling_order() {
        let mut session = fixture_session(Document::new_default());
        let first = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 40.0,
                },
            )
            .expect("insert first layer");
        let second = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 50.0,
                    y: 60.0,
                    width: 70.0,
                    height: 80.0,
                },
            )
            .expect("insert topmost layer");

        let snapshot = session.canvas_snapshot(0).expect("project layer tree");
        assert_eq!(snapshot.layers[0].kind, "artboard");
        assert_eq!(snapshot.layers[0].depth, 0);
        assert_eq!(snapshot.layers[1].sid, second);
        assert_eq!(snapshot.layers[1].kind, "box");
        assert_eq!(snapshot.layers[1].depth, 1);
        assert_eq!(snapshot.layers[2].sid, first);
        assert_eq!(snapshot.layers[2].depth, 1);
        assert_eq!(snapshot.selected_sids, [second]);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn grid_visibility_is_view_only_and_does_not_dirty_or_undo_document() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        assert!(!session.grid_visible());
        assert!(!session.is_dirty());
        assert!(!session.can_undo());

        assert!(session.dispatch_catalog("view.toggle_grid").is_applied());
        assert!(session.grid_visible());
        assert!(!session.is_dirty());
        assert!(!session.can_undo());

        assert!(session.dispatch_catalog("view.toggle_grid").is_applied());
        assert!(!session.grid_visible());
        assert!(!session.is_dirty());
        assert!(!session.can_undo());
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn canvas_snapshot_is_a_read_only_artboard_projection() {
        let session = fixture_session(Document::new_default());
        let snapshot = session.canvas_snapshot(0).expect("default artboard");

        assert_eq!(snapshot.title, "未命名");
        assert_eq!(snapshot.artboard_index, 0);
        assert_eq!(snapshot.artboard_count, 1);
        assert_eq!(snapshot.draw_list.w, 1440.0);
        assert_eq!(snapshot.draw_list.h, 900.0);
        assert_eq!(snapshot.draw_list.items.len(), 1);
        assert_eq!(snapshot.warnings, ["外链资源未加载"]);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn canvas_snapshot_attaches_confined_inline_image_projection() {
        const PIXEL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";
        let temporary = TempProjectDir::new();
        let html = format!(
            "<html><head><title>Image fixture</title></head><body><div data-vb-artboard style=\"width:640px;height:480px;position:relative\"><img src=\"{PIXEL}\" style=\"position:absolute;left:10px;top:12px;width:1px;height:1px\"></div></body></html>"
        );
        std::fs::write(temporary.path().join("index.html"), html)
            .expect("write inline image fixture");
        let session = ProjectSession::open(temporary.path()).expect("import image fixture");
        let snapshot = session.canvas_snapshot(0).expect("project image snapshot");
        let image = snapshot
            .draw_list
            .items
            .iter()
            .find(|item| item.kind == vb_render::encode::DrawKind::Image)
            .expect("imported image draw item");
        let bitmap = image
            .image
            .as_ref()
            .expect("inline bitmap decoded and attached");
        assert_eq!((bitmap.width, bitmap.height), (1, 1));
        assert_eq!(bitmap.rgba.as_slice(), &[255, 0, 0, 255]);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn place_image_copies_confined_raster_and_round_trips_undo_redo_save() {
        let source_root = TempProjectDir::new();
        let source = source_root.path().join("pixel.png");
        std::fs::write(&source, PIXEL_PNG_BYTES).expect("write selected image");
        let mut session = fixture_session(Document::new_default());

        let sid = session
            .place_image_from_path(0, &source)
            .expect("place valid raster image");
        assert!(session.is_dirty());
        let first = session.canvas_snapshot(0).expect("placed image snapshot");
        let image = first
            .draw_list
            .items
            .iter()
            .find(|item| item.sid == sid)
            .expect("image draw item");
        assert_eq!(image.kind, vb_render::encode::DrawKind::Image);
        let src = image.src.as_deref().expect("relative project asset src");
        assert!(src.starts_with("assets/vb-image-"));
        assert!(!Path::new(src).is_absolute());
        assert_eq!(
            image
                .image
                .as_ref()
                .map(|bitmap| (bitmap.width, bitmap.height)),
            Some((1, 1))
        );
        assert!(session.project_dir().join(src).is_file());

        assert!(session.undo().expect("undo image placement"));
        assert!(!session.is_dirty());
        assert!(session
            .canvas_snapshot(0)
            .expect("image undo snapshot")
            .draw_list
            .items
            .iter()
            .all(|item| item.sid != sid));
        assert!(session.redo().expect("redo image placement"));
        assert!(session.is_dirty());

        let project_dir = session.project_dir().to_path_buf();
        session.save().expect("save placed image");
        let reopened = ProjectSession::open(project_dir).expect("reopen image project");
        let persisted = reopened
            .canvas_snapshot(0)
            .expect("reopened image snapshot");
        let image = persisted
            .draw_list
            .items
            .iter()
            .find(|item| item.sid == sid)
            .expect("reopened image draw item");
        assert!(image.image.is_some());
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn place_image_rejects_invalid_content_without_creating_project_assets() {
        let source_root = TempProjectDir::new();
        let source = source_root.path().join("not-an-image.png");
        std::fs::write(&source, b"not a raster").expect("write invalid image");
        let mut session = fixture_session(Document::new_default());
        let revision = session.document.rev;

        assert!(session.place_image_from_path(0, &source).is_err());
        assert_eq!(session.document.rev, revision);
        assert!(!session.is_dirty());
        assert!(!session.project_dir().join("assets").exists());
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn place_image_rejects_payload_with_mismatched_extension_without_assets() {
        let parent = TempProjectDir::new();
        let project_dir = parent.path().join("New Design");
        std::fs::create_dir(&project_dir).expect("create empty project folder");
        let mut session = ProjectSession::new_project(&project_dir).expect("create project");
        let source = parent.path().join("png-bytes-named-jpeg.jpg");
        std::fs::write(&source, PIXEL_PNG_BYTES).expect("write PNG payload");
        let revision = session.document.rev;

        assert!(session.place_image_from_path(0, &source).is_err());
        assert_eq!(session.document.rev, revision);
        assert!(!session.is_dirty());
        assert!(!project_dir.join("assets").exists());
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn image_asset_storage_never_overwrites_existing_files() {
        let project = TempProjectDir::new();
        let first = write_project_image_asset(project.path(), "abc123", "png", b"original")
            .expect("write first asset");
        let second = write_project_image_asset(project.path(), "abc123", "png", b"replacement")
            .expect("deduplicate asset name");

        assert_eq!(first, "assets/vb-image-abc123.png");
        assert_eq!(second, "assets/vb-image-abc123-1.png");
        assert_eq!(
            std::fs::read(project.path().join(&first)).expect("read original"),
            b"original"
        );
        assert_eq!(
            std::fs::read(project.path().join(&second)).expect("read duplicate"),
            b"replacement"
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn canvas_snapshot_rejects_an_invalid_artboard_index() {
        let session = fixture_session(Document::new_empty("空文档", "zh-CN"));
        let error = session
            .canvas_snapshot(0)
            .expect_err("document without artboards cannot be drawn");

        assert!(error.to_string().contains("画板索引 0"));
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn artboard_navigation_is_view_only_clears_selection_and_wraps() {
        let mut document = Document::new_default();
        document.new_artboard("第二画板", 640.0, 480.0);
        let mut fixture = fixture_session(document);
        let session = &mut fixture;
        let first_sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 40.0,
                    height: 50.0,
                },
            )
            .expect("insert on first artboard");
        let second_sid = session
            .create_ellipse(
                1,
                CanvasRect {
                    x: 70.0,
                    y: 80.0,
                    width: 90.0,
                    height: 60.0,
                },
            )
            .expect("insert on second artboard");
        session.selection.select(first_sid);
        let undo_before = session.undo.undo_len();
        let dirty_before = session.is_dirty();

        assert!(session.set_active_artboard(1).unwrap());
        assert_eq!(session.active_artboard_index(), 1);
        assert_eq!(session.active_canvas_snapshot().unwrap().draw_list.w, 640.0);
        assert!(session.selected_sids().is_empty());
        assert_eq!(session.undo.undo_len(), undo_before);
        assert_eq!(session.is_dirty(), dirty_before);

        assert!(matches!(
            session.dispatch_catalog("edit.select_all"),
            CommandOutcome::Applied
        ));
        assert_eq!(session.selected_sids(), &[second_sid]);
        assert!(matches!(
            session.dispatch_catalog("view.next_artboard"),
            CommandOutcome::Applied
        ));
        assert_eq!(session.active_artboard_index(), 0);
        assert!(session.selected_sids().is_empty());
        assert!(matches!(
            session.dispatch_catalog("view.prev_artboard"),
            CommandOutcome::Applied
        ));
        assert_eq!(session.active_artboard_index(), 1);
        assert!(session.set_active_artboard(2).is_err());
        assert_eq!(session.active_artboard_index(), 1);
        assert_eq!(session.undo.undo_len(), undo_before);
        assert_eq!(session.is_dirty(), dirty_before);

        let mut single = fixture_session(Document::new_default());
        assert!(!single.next_artboard().unwrap());
        assert!(!single.previous_artboard().unwrap());
        assert_eq!(single.active_artboard_index(), 0);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn create_artboard_is_reversible_and_activates_new_board() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let undo_before = session.undo.undo_len();
        let revision_before = session.document.rev;
        assert!(session
            .create_artboard(CanvasRect {
                x: 20.0,
                y: 30.0,
                width: 0.0,
                height: 480.0,
            })
            .is_err());
        assert_eq!(session.document.rev, revision_before);
        assert_eq!(session.artboard_count(), 1);

        let sid = session
            .create_artboard(CanvasRect {
                x: 200.0,
                y: 120.0,
                width: 640.0,
                height: 480.0,
            })
            .expect("create second artboard");
        assert_eq!(session.artboard_count(), 2);
        assert_eq!(session.active_artboard_index(), 1);
        assert!(session.is_dirty());
        assert_eq!(session.undo.undo_len(), undo_before + 1);
        assert!(session.selected_sids().contains(&sid));
        assert_eq!(
            session
                .active_canvas_snapshot()
                .expect("second board snapshot")
                .draw_list
                .w,
            640.0
        );

        assert!(session.undo().expect("undo artboard creation"));
        assert_eq!(session.artboard_count(), 1);
        assert_eq!(session.active_artboard_index(), 0);
        assert!(session.selected_sids().is_empty());
        assert!(!session.is_dirty());

        assert!(session.redo().expect("redo artboard creation"));
        assert_eq!(session.artboard_count(), 2);
        assert!(session.is_dirty());
        session.save().expect("save both artboards");
        let reopened = ProjectSession::open(&session.project_dir).expect("reopen project");
        assert_eq!(reopened.artboard_count(), 2);
        assert_eq!(
            reopened
                .canvas_snapshot(1)
                .expect("persisted second board")
                .draw_list
                .h,
            480.0
        );
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn deleting_artboard_is_undoable_and_never_removes_the_last_board() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let first_sid = session
            .document
            .node(session.document.artboards()[0])
            .expect("first artboard")
            .sid
            .as_str()
            .to_string();
        let second_sid = session
            .create_artboard(CanvasRect {
                x: 160.0,
                y: 100.0,
                width: 640.0,
                height: 480.0,
            })
            .expect("second artboard");
        session.select_sid(Some(&second_sid), ModifierState::default());
        assert!(session.can_delete_selection());
        assert_eq!(session.delete_selection().expect("delete second board"), 1);
        assert_eq!(session.artboard_count(), 1);
        assert_eq!(session.active_artboard_index(), 0);

        session.select_sid(Some(&first_sid), ModifierState::default());
        let revision = session.document.rev;
        assert!(!session.can_delete_selection());
        assert!(session.delete_selection().is_err());
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.artboard_count(), 1);

        assert!(session.undo().expect("undo board deletion"));
        assert_eq!(session.artboard_count(), 2);
        assert!(session.redo().expect("redo board deletion"));
        assert_eq!(session.artboard_count(), 1);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn selection_alignment_uses_absolute_boxes_and_one_undo_step() {
        let mut session = fixture_session(Document::new_default());
        let first = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 30.0,
                    y: 40.0,
                    width: 60.0,
                    height: 50.0,
                },
            )
            .expect("insert first rectangle");
        let second = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 200.0,
                    y: 100.0,
                    width: 40.0,
                    height: 40.0,
                },
            )
            .expect("insert second ellipse");
        session
            .selection
            .set_all(vec![first.clone(), second.clone()]);
        let undo_before = session.undo.undo_len();

        assert_eq!(session.align_to(), AlignTo::Selection);
        assert_eq!(session.align_selection(AlignMode::Left).unwrap(), 1);
        let first_id = session.document.find_by_sid(&first).expect("first SID");
        let second_id = session.document.find_by_sid(&second).expect("second SID");
        assert_eq!(session.document.node(first_id).unwrap().geom.x, 30.0);
        assert_eq!(session.document.node(second_id).unwrap().geom.x, 30.0);
        assert_eq!(session.undo.undo_len(), undo_before + 1);

        assert!(session.undo().unwrap());
        assert_eq!(session.document.node(second_id).unwrap().geom.x, 200.0);
        assert_eq!(session.selected_sids(), [first, second]);
        assert!(session.redo().unwrap());
        assert_eq!(session.document.node(second_id).unwrap().geom.x, 30.0);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn alignment_targets_key_object_or_artboard_from_catalog() {
        let mut session = fixture_session(Document::new_default());
        let first = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 30.0,
                    y: 40.0,
                    width: 60.0,
                    height: 50.0,
                },
            )
            .expect("insert first rectangle");
        let key = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 200.0,
                    y: 100.0,
                    width: 40.0,
                    height: 40.0,
                },
            )
            .expect("insert key ellipse");
        session.selection.set_all(vec![first.clone(), key.clone()]);

        assert_eq!(session.align_target_catalog_id(), "align.to_selection");
        assert!(session.dispatch_catalog("align.to_key_object").is_applied());
        assert_eq!(session.align_target_catalog_id(), "align.to_key_object");
        assert!(session.dispatch_catalog("align.right").is_applied());
        let first_id = session.document.find_by_sid(&first).expect("first SID");
        let key_id = session.document.find_by_sid(&key).expect("key SID");
        assert_eq!(session.document.node(first_id).unwrap().geom.x, 180.0);
        assert_eq!(session.document.node(key_id).unwrap().geom.x, 200.0);

        assert!(session.dispatch_catalog("align.to_artboard").is_applied());
        assert_eq!(session.align_target_catalog_id(), "align.to_artboard");
        assert!(session.dispatch_catalog("align.hcenter").is_applied());
        assert_eq!(session.document.node(first_id).unwrap().geom.x, 690.0);
        assert_eq!(session.document.node(key_id).unwrap().geom.x, 700.0);
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn alignment_rejects_locked_selection_without_partial_mutation() {
        let mut session = fixture_session(Document::new_default());
        let first = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 30.0,
                    y: 40.0,
                    width: 60.0,
                    height: 50.0,
                },
            )
            .expect("insert first rectangle");
        let second = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 200.0,
                    y: 100.0,
                    width: 40.0,
                    height: 40.0,
                },
            )
            .expect("insert second ellipse");
        session
            .selection
            .set_all(vec![first.clone(), second.clone()]);
        session.set_selected_flags(None, Some(true)).unwrap();
        let revision = session.document.rev;
        let first_id = session.document.find_by_sid(&first).expect("first SID");
        let second_id = session.document.find_by_sid(&second).expect("second SID");
        let original = [
            session.document.node(first_id).unwrap().geom,
            session.document.node(second_id).unwrap().geom,
        ];

        assert!(session.align_selection(AlignMode::Left).is_err());
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.document.node(first_id).unwrap().geom, original[0]);
        assert_eq!(session.document.node(second_id).unwrap().geom, original[1]);
    }

    #[test]
    fn alignment_without_selection_is_a_nonmutation() {
        let mut session = fixture_session(Document::new_default());
        let revision = session.document.rev;
        assert_eq!(session.align_selection(AlignMode::Left).unwrap(), 0);
        assert_eq!(session.document.rev, revision);
        assert!(matches!(
            session.dispatch_catalog("align.left"),
            CommandOutcome::NotRun { .. }
        ));
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn group_ungroup_and_undo_preserve_ids_geometry_and_html_round_trip() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let first = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 80.0,
                    height: 50.0,
                },
            )
            .expect("insert rectangle");
        let second = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 140.0,
                    y: 45.0,
                    width: 60.0,
                    height: 40.0,
                },
            )
            .expect("insert ellipse");
        let before = [item_rect(session, &first), item_rect(session, &second)];
        session.select_sid(Some(&first), ModifierState::default());
        session.select_sid(
            Some(&second),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let undo_before_group = session.undo.undo_len();

        let group_sid = session
            .group_selection()
            .expect("group siblings")
            .expect("two siblings produce a group");
        assert_eq!(session.undo.undo_len(), undo_before_group + 1);
        assert_eq!(session.selected_sids(), [group_sid.as_str()]);
        assert!(session.can_ungroup_selection());
        assert!(!session.can_group_selection());
        let grouped = session.canvas_snapshot(0).expect("grouped tree");
        assert!(grouped
            .layers
            .iter()
            .any(|layer| layer.sid == group_sid && layer.kind == "group"));
        assert!(grouped
            .layers
            .iter()
            .any(|layer| layer.sid == first && layer.depth == 2));
        assert!(grouped
            .layers
            .iter()
            .any(|layer| layer.sid == second && layer.depth == 2));

        assert!(session.undo().expect("undo group"));
        let ungrouped = session.canvas_snapshot(0).expect("undo restores siblings");
        assert!(!ungrouped.layers.iter().any(|layer| layer.sid == group_sid));
        assert_eq!(item_rect(session, &first), before[0]);
        assert_eq!(item_rect(session, &second), before[1]);
        assert!(session.redo().expect("redo group"));
        assert!(session
            .canvas_snapshot(0)
            .unwrap()
            .layers
            .iter()
            .any(|layer| layer.sid == group_sid));

        session.select_sid(Some(&group_sid), ModifierState::default());
        let undo_before_ungroup = session.undo.undo_len();
        assert_eq!(session.ungroup_selection().unwrap(), 1);
        assert_eq!(session.undo.undo_len(), undo_before_ungroup + 1);
        assert_eq!(session.selected_sids(), [first.clone(), second.clone()]);
        assert_eq!(item_rect(session, &first), before[0]);
        assert_eq!(item_rect(session, &second), before[1]);
        assert!(session.undo().expect("undo ungroup"));
        assert!(session
            .canvas_snapshot(0)
            .unwrap()
            .layers
            .iter()
            .any(|layer| layer.sid == group_sid));
        assert!(session.redo().expect("redo ungroup"));

        session.save().expect("save ungrouped document");
        let reopened = ProjectSession::open(session.project_dir()).expect("reopen project");
        let reopened_layers = reopened.canvas_snapshot(0).unwrap().layers;
        assert!(reopened_layers.iter().any(|layer| layer.sid == first));
        assert!(reopened_layers.iter().any(|layer| layer.sid == second));
        assert!(!reopened_layers.iter().any(|layer| layer.sid == group_sid));
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn grouping_rejects_mixed_parents_and_locked_members_atomically() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let first = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 10.0,
                    y: 20.0,
                    width: 40.0,
                    height: 30.0,
                },
            )
            .expect("insert first sibling");
        let second = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 70.0,
                    y: 20.0,
                    width: 40.0,
                    height: 30.0,
                },
            )
            .expect("insert second sibling");
        session.select_sid(Some(&first), ModifierState::default());
        session.select_sid(
            Some(&second),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let group_sid = session.group_selection().unwrap().unwrap();
        let third = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 150.0,
                    y: 20.0,
                    width: 40.0,
                    height: 30.0,
                },
            )
            .expect("insert outside sibling");
        session.select_sid(
            Some(&first),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let revision_before_mixed = session.document.rev;
        assert!(session.group_selection().is_err());
        assert_eq!(session.document.rev, revision_before_mixed);
        assert!(session
            .canvas_snapshot(0)
            .unwrap()
            .layers
            .iter()
            .any(|layer| layer.sid == group_sid));

        session.set_selected_flags(None, Some(true)).unwrap();
        let revision_before_locked = session.document.rev;
        assert!(session.group_selection().is_err());
        assert_eq!(session.document.rev, revision_before_locked);
        assert!(session.document.find_by_sid(&third).is_some());
    }

    #[cfg(feature = "canvas-projection")]
    #[test]
    fn grouping_commands_dispatch_through_catalog() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let first = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 10.0,
                    y: 10.0,
                    width: 30.0,
                    height: 20.0,
                },
            )
            .unwrap();
        let second = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 60.0,
                    y: 10.0,
                    width: 30.0,
                    height: 20.0,
                },
            )
            .unwrap();
        session.select_sid(Some(&first), ModifierState::default());
        session.select_sid(
            Some(&second),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        assert!(session.can_group_selection());
        assert!(session.dispatch_catalog("object.group").is_applied());
        assert!(session.can_ungroup_selection());
        assert!(session.dispatch_catalog("object.ungroup").is_applied());
    }

    #[test]
    fn animation_session_projection_round_trips_and_undoes_to_clean_savepoint() {
        let parent = TempProjectDir::new();
        let project_dir = parent.path().join("animated");
        let mut session = ProjectSession::new_project(&project_dir).expect("new project");
        let sid = session
            .create_text(0, [120.0, 80.0])
            .expect("create animation target");
        session.save().expect("save text baseline");
        session.select_sid(Some(&sid), ModifierState::default());

        let initial = session.animation_snapshot();
        assert_eq!(initial.selected_count, 1);
        assert!(matches!(
            initial.selected.as_ref().map(|selected| &selected.source),
            Some(AnimSource::None)
        ));
        assert_eq!(
            initial.selected.as_ref().unwrap().static_values,
            ((0.0, 0.0), 1.0, 1.0, 0.0)
        );

        let model = sample_animation_model();
        assert!(session
            .set_selected_animation(&sid, &model)
            .expect("write CSS animation"));
        assert!(session.is_dirty());
        let animated = session.animation_snapshot();
        assert!(matches!(
            animated.selected.as_ref().map(|selected| &selected.source),
            Some(AnimSource::Timeline(current)) if current == &model
        ));
        assert!(session
            .document
            .raw_css
            .iter()
            .any(|block| { block.contains(&vb_doc::commands::anim_block_name(&sid)) }));

        assert!(session.undo().expect("undo animation"));
        assert!(!session.is_dirty(), "undo returns to saved text baseline");
        assert!(matches!(
            session
                .animation_snapshot()
                .selected
                .as_ref()
                .map(|selected| &selected.source),
            Some(AnimSource::None)
        ));
        assert!(session.redo().expect("redo animation"));
        assert!(session.is_dirty());
        assert!(!session
            .set_selected_animation(&sid, &model)
            .expect("same model is a no-op"));
        session.save().expect("save animated project");
        assert!(!session.is_dirty());

        let mut reopened = ProjectSession::open(&project_dir).expect("reopen animated project");
        reopened.select_sid(Some(&sid), ModifierState::default());
        let persisted = reopened.animation_snapshot();
        assert!(matches!(
            persisted.selected.as_ref().map(|selected| &selected.source),
            Some(AnimSource::Timeline(current)) if current == &model
        ));
        assert!(!reopened.is_dirty());
    }

    #[test]
    fn animated_canvas_snapshot_applies_frame_without_mutating_document() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create animated rectangle");
        let mut model = sample_animation_model();
        for (_, frames) in &mut model.tracks {
            for frame in frames {
                frame.easing = Easing::Linear;
            }
        }
        assert!(fixture.set_selected_animation(&sid, &model).unwrap());
        fixture.save().unwrap();

        let keyframes = vb_animation::parse_keyframes(&fixture.document.raw_css);
        let node_id = fixture.document.find_by_sid(&sid).expect("animated SID");
        let resolved = vb_animation::resolve_node_anim(&fixture.document, node_id, &keyframes)
            .expect("serialized animation resolves from Document CSS");
        let state = vb_animation::eval_node(&resolved, 0.6);
        assert_eq!(state.opacity, Some(0.75));
        assert!(state.transform.is_some());

        let revision = fixture.document.rev;
        let undo_len = fixture.undo.undo_len();
        let before = fixture
            .canvas_snapshot(0)
            .unwrap()
            .draw_list
            .items
            .into_iter()
            .find(|item| item.sid == sid)
            .expect("static item");
        let halfway = fixture
            .canvas_snapshot_at(0, 0.6)
            .unwrap()
            .draw_list
            .items
            .into_iter()
            .find(|item| item.sid == sid)
            .expect("animated item");

        assert!((before.rect[0] - 48.0).abs() < 1e-6);
        assert!(
            (halfway.rect[0] - 68.0).abs() < 1e-6,
            "expected x=68, got rect={:?}; evaluated animation state={state:?}",
            halfway.rect
        );
        assert!((halfway.rect[1] - 74.0).abs() < 1e-6);
        assert!((halfway.opacity - 0.75).abs() < 1e-6);
        assert_eq!(fixture.document.rev, revision);
        assert_eq!(fixture.undo.undo_len(), undo_len);
        assert!(!fixture.is_dirty());
        assert!(fixture.canvas_snapshot_at(0, f64::NAN).is_err());
    }

    #[test]
    fn animation_playback_is_view_state_and_wraps_without_dirtying_document() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create animated rectangle");
        let model = sample_animation_model();
        assert!(fixture.set_selected_animation(&sid, &model).unwrap());
        fixture.save().unwrap();

        let revision = fixture.document.rev;
        let undo_len = fixture.undo.undo_len();
        let initial = fixture.animation_snapshot();
        assert!((initial.duration_seconds.unwrap() - 2.1).abs() < 1e-9);
        assert_eq!(initial.playhead_seconds, 0.0);
        assert!(!initial.playing);

        assert!(fixture.set_animation_playhead_seconds(0.6).unwrap());
        assert!(fixture.set_animation_playing(true).unwrap());
        assert!(fixture.advance_animation_playback(0.5).unwrap());
        assert!((fixture.animation_snapshot().playhead_seconds - 1.1).abs() < 1e-9);

        assert!(fixture.set_animation_looping(true));
        assert!(fixture.advance_animation_playback(1.2).unwrap());
        assert!((fixture.animation_snapshot().playhead_seconds - 0.2).abs() < 1e-9);
        assert!(fixture.stop_animation_playback());
        assert_eq!(fixture.animation_snapshot().playhead_seconds, 0.0);
        assert!(!fixture.animation_snapshot().playing);

        assert_eq!(fixture.document.rev, revision);
        assert_eq!(fixture.undo.undo_len(), undo_len);
        assert!(!fixture.is_dirty());
        assert!(fixture.set_animation_playhead_seconds(f64::NAN).is_err());
        assert!(fixture.advance_animation_playback(f64::INFINITY).is_err());
    }

    #[test]
    fn animation_frame_snap_quantizes_by_rate_without_dirtying_document() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create frame-snap target");
        fixture
            .set_selected_animation(&sid, &sample_animation_model())
            .expect("set local animation");
        fixture.save().expect("save frame-snap baseline");

        let revision = fixture.document.rev;
        let undo_len = fixture.undo.undo_len();
        let raw = KeyframeOffsetDelta::new(110).unwrap();
        assert!(fixture.animation_frame_snap_enabled());
        assert_eq!(fixture.animation_frame_rate(), AnimationFrameRate::Fps30);
        assert_eq!(
            fixture
                .snap_selected_keyframe_delta(raw)
                .unwrap()
                .permille(),
            100
        );

        assert!(fixture.set_animation_frame_rate(AnimationFrameRate::Fps24));
        assert_eq!(
            fixture
                .snap_selected_keyframe_delta(raw)
                .unwrap()
                .permille(),
            125
        );
        assert!(fixture.set_animation_frame_rate(AnimationFrameRate::Fps60));
        assert_eq!(
            fixture
                .snap_selected_keyframe_delta(raw)
                .unwrap()
                .permille(),
            117
        );
        assert!(fixture.set_animation_frame_snap_enabled(false));
        assert_eq!(fixture.snap_selected_keyframe_delta(raw).unwrap(), raw);

        assert!(fixture.set_animation_frame_snap_enabled(true));
        assert!(fixture.set_animation_frame_rate(AnimationFrameRate::Fps30));
        assert_eq!(
            fixture
                .snap_selected_keyframe_delta(KeyframeOffsetDelta::new(-110).unwrap())
                .unwrap()
                .permille(),
            -100
        );
        assert_eq!(fixture.document.rev, revision);
        assert_eq!(fixture.undo.undo_len(), undo_len);
        assert!(!fixture.is_dirty());
    }

    #[test]
    fn animation_frame_snap_quantizes_shared_delta_without_dirtying_document() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create frame-snap target");
        fixture
            .set_selected_animation(&sid, &sample_animation_model())
            .expect("set local animation");
        fixture.save().expect("save frame-snap baseline");

        let revision = fixture.document.rev;
        let undo_len = fixture.undo.undo_len();
        let raw = KeyframeOffsetDelta::new(110).unwrap();
        assert!(fixture.animation_frame_snap_enabled());
        assert_eq!(fixture.animation_frame_rate(), AnimationFrameRate::Fps30);
        assert_eq!(
            fixture
                .snap_selected_keyframe_delta(raw)
                .unwrap()
                .permille(),
            100
        );

        assert!(fixture.set_animation_frame_rate(AnimationFrameRate::Fps24));
        assert_eq!(
            fixture
                .snap_selected_keyframe_delta(raw)
                .unwrap()
                .permille(),
            125
        );
        assert!(fixture.set_animation_frame_rate(AnimationFrameRate::Fps60));
        assert_eq!(
            fixture
                .snap_selected_keyframe_delta(raw)
                .unwrap()
                .permille(),
            117
        );
        assert!(fixture.set_animation_frame_snap_enabled(false));
        assert_eq!(fixture.snap_selected_keyframe_delta(raw).unwrap(), raw);

        assert!(fixture.set_animation_frame_snap_enabled(true));
        assert!(fixture.set_animation_frame_rate(AnimationFrameRate::Fps30));
        assert_eq!(
            fixture
                .snap_selected_keyframe_delta(KeyframeOffsetDelta::new(-110).unwrap())
                .unwrap()
                .permille(),
            -100
        );
        assert_eq!(fixture.document.rev, revision);
        assert_eq!(fixture.undo.undo_len(), undo_len);
        assert!(!fixture.is_dirty());
    }

    #[test]
    fn animation_preset_clear_and_play_commands_are_reversible_and_view_only() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 80.0,
                    y: 100.0,
                    width: 120.0,
                    height: 64.0,
                },
            )
            .expect("create animated rectangle");
        assert!(fixture.dispatch_catalog("anim.create").is_applied());
        assert_eq!(
            fixture
                .animation_snapshot()
                .selected
                .as_ref()
                .map(|selected| &selected.sid),
            Some(&sid)
        );
        assert!(matches!(
            fixture
                .animation_snapshot()
                .selected
                .as_ref()
                .map(|selected| &selected.source),
            Some(AnimSource::Timeline(_))
        ));
        fixture.save().unwrap();

        let revision = fixture.document.rev;
        let undo_len = fixture.undo.undo_len();
        assert!(fixture.dispatch_catalog("anim.loop_toggle").is_applied());
        assert!(fixture.set_animation_playhead_seconds(0.25).unwrap());
        assert!(fixture.dispatch_catalog("anim.play_toggle").is_applied());
        assert!(fixture.advance_animation_playback(0.25).unwrap());
        assert!(fixture.dispatch_catalog("anim.stop").is_applied());
        assert_eq!(fixture.animation_snapshot().playhead_seconds, 0.0);
        assert_eq!(fixture.document.rev, revision);
        assert_eq!(fixture.undo.undo_len(), undo_len);
        assert!(!fixture.is_dirty());

        assert!(fixture.dispatch_catalog("anim.clear").is_applied());
        assert!(matches!(
            fixture
                .animation_snapshot()
                .selected
                .as_ref()
                .map(|selected| &selected.source),
            Some(AnimSource::None)
        ));
        assert!(fixture.undo().unwrap());
        assert!(matches!(
            fixture
                .animation_snapshot()
                .selected
                .as_ref()
                .map(|selected| &selected.source),
            Some(AnimSource::Timeline(_))
        ));
        assert!(!fixture.is_dirty());
    }

    #[test]
    fn named_animation_preset_is_shared_editable_and_round_trips() {
        let mut fixture = fixture_session(Document::new_default());
        let first = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 64.0,
                    y: 80.0,
                    width: 100.0,
                    height: 60.0,
                },
            )
            .expect("create first animation target");
        assert!(fixture
            .set_selected_animation(&first, &sample_animation_model())
            .unwrap());
        fixture.save().unwrap();

        let preset = fixture
            .save_selected_animation_as_preset("Pulse")
            .expect("save named preset");
        assert_eq!(preset.id, "pulse");
        assert_eq!(fixture.available_animation_presets(), vec![preset.clone()]);
        let after_save = fixture.animation_snapshot();
        let AnimSource::Timeline(local) = after_save.selected.unwrap().source else {
            panic!("local animation remains local after Save As");
        };
        assert!(local.preset.is_none());

        assert_eq!(
            fixture
                .apply_animation_preset_to_selection(&preset.id)
                .unwrap(),
            1
        );
        let second = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 240.0,
                    y: 80.0,
                    width: 100.0,
                    height: 60.0,
                },
            )
            .expect("create second animation target");
        assert_eq!(
            fixture
                .apply_animation_preset_to_selection(&preset.id)
                .unwrap(),
            1
        );

        fixture.select_sid(Some(&first), ModifierState::default());
        let mut shared_model = match fixture.animation_snapshot().selected.unwrap().source {
            AnimSource::Timeline(model) if model.preset.as_ref() == Some(&preset) => model,
            other => panic!("expected shared preset Timeline, got {other:?}"),
        };
        shared_model.duration_ms = 2_000.0;
        assert!(fixture
            .set_selected_animation(&first, &shared_model)
            .unwrap());
        fixture.select_sid(Some(&second), ModifierState::default());
        let second_snapshot = fixture.animation_snapshot();
        let AnimSource::Timeline(second_model) = second_snapshot.selected.unwrap().source else {
            panic!("second object should share a Timeline");
        };
        assert_eq!(second_model.preset.as_ref(), Some(&preset));
        assert_eq!(second_model.duration_ms, 2_000.0);

        assert!(fixture.undo().unwrap(), "undo shared definition edit");
        fixture.select_sid(Some(&second), ModifierState::default());
        let after_undo = fixture.animation_snapshot();
        let AnimSource::Timeline(undone) = after_undo.selected.unwrap().source else {
            panic!("shared animation remains attached after undo");
        };
        assert_eq!(undone.preset.as_ref(), Some(&preset));
        assert_eq!(undone.duration_ms, 1_000.0);
        assert!(fixture.redo().unwrap(), "redo shared definition edit");
        fixture.save().unwrap();

        let project_dir = fixture._temp_project.path().to_path_buf();
        let mut reopened = ProjectSession::open(&project_dir).expect("reopen named preset project");
        assert_eq!(reopened.available_animation_presets(), vec![preset.clone()]);
        reopened.select_sid(Some(&first), ModifierState::default());
        let reopened_snapshot = reopened.animation_snapshot();
        let AnimSource::Timeline(reopened_model) = reopened_snapshot.selected.unwrap().source
        else {
            panic!("reopened first object should reference a Timeline");
        };
        assert_eq!(reopened_model.preset.as_ref(), Some(&preset));
        assert_eq!(reopened_model.duration_ms, 2_000.0);
        reopened.select_sid(Some(&second), ModifierState::default());
        let second_reopened = reopened.animation_snapshot();
        let AnimSource::Timeline(second_model) = second_reopened.selected.unwrap().source else {
            panic!("reopened second object should reference a Timeline");
        };
        assert_eq!(second_model.preset.as_ref(), Some(&preset));
        assert_eq!(second_model.duration_ms, 2_000.0);
    }

    #[test]
    fn named_animation_preset_delete_is_reference_safe_and_undoable() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 64.0,
                    y: 80.0,
                    width: 100.0,
                    height: 60.0,
                },
            )
            .expect("create animation target");
        assert!(fixture
            .set_selected_animation(&sid, &sample_animation_model())
            .unwrap());
        let preset = fixture
            .save_selected_animation_as_preset("Pulse")
            .expect("save named preset");
        assert_eq!(
            fixture
                .apply_animation_preset_to_selection(&preset.id)
                .unwrap(),
            1
        );
        let before_delete = fixture.document.raw_css.clone();
        let rev_before_delete = fixture.document.rev;
        assert!(fixture.delete_animation_preset(&preset.id).is_err());
        assert_eq!(fixture.document.raw_css, before_delete);
        assert_eq!(fixture.document.rev, rev_before_delete);

        assert!(fixture.clear_selected_animation().unwrap());
        assert!(fixture.delete_animation_preset(&preset.id).unwrap());
        assert!(fixture.available_animation_presets().is_empty());
        assert!(fixture.undo().unwrap(), "undo preset deletion");
        assert_eq!(fixture.available_animation_presets(), vec![preset]);
    }

    #[test]
    fn foreign_animation_is_read_only_and_invalid_models_do_not_mutate() {
        let mut foreign = fixture_session(Document::new_default());
        let sid = foreign
            .create_text(0, [32.0, 48.0])
            .expect("create foreign animation target");
        foreign.select_sid(Some(&sid), ModifierState::default());
        let id = foreign.document.find_by_sid(&sid).expect("text sid");
        foreign
            .document
            .nodes_mut()
            .get_mut(id)
            .expect("text node")
            .style_set("animation", "pulse 1s linear");
        foreign
            .document
            .raw_css
            .push("@keyframes pulse { from { opacity: 0; } to { opacity: 1; } }".into());
        let raw_before = foreign.document.raw_css.clone();
        let revision_before = foreign.document.rev;
        assert!(matches!(
            foreign.animation_snapshot().selected.unwrap().source,
            AnimSource::Foreign(ref name) if name == "pulse"
        ));
        assert!(foreign
            .set_selected_animation(&sid, &sample_animation_model())
            .is_err());
        assert_eq!(foreign.document.raw_css, raw_before);
        assert_eq!(foreign.document.rev, revision_before);

        let mut invalid = fixture_session(Document::new_default());
        let invalid_sid = invalid
            .create_text(0, [32.0, 48.0])
            .expect("create invalid-model target");
        invalid.select_sid(Some(&invalid_sid), ModifierState::default());
        let mut invalid_model = sample_animation_model();
        invalid_model.duration_ms = f64::NAN;
        let revision_before = invalid.document.rev;
        assert!(invalid
            .set_selected_animation(&invalid_sid, &invalid_model)
            .is_err());
        assert_eq!(invalid.document.rev, revision_before);
        assert!(matches!(
            invalid.animation_snapshot().selected.unwrap().source,
            AnimSource::None
        ));
    }

    #[test]
    fn animation_edits_require_single_selection_and_unlocked_ancestry() {
        let mut fixture = fixture_session(Document::new_default());
        let first = fixture
            .create_text(0, [32.0, 48.0])
            .expect("create first animation target");
        let second = fixture
            .create_rectangle(
                0,
                CanvasRect {
                    x: 96.0,
                    y: 48.0,
                    width: 24.0,
                    height: 24.0,
                },
            )
            .expect("create second animation target");
        fixture.select_sid(Some(&first), ModifierState::default());
        fixture.select_sid(
            Some(&second),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let revision_before = fixture.document.rev;
        assert!(fixture
            .set_selected_animation(&first, &sample_animation_model())
            .is_err());
        assert_eq!(fixture.document.rev, revision_before);

        fixture.select_sid(Some(&first), ModifierState::default());
        let artboard = fixture.document.artboards()[0];
        fixture
            .document
            .nodes_mut()
            .get_mut(artboard)
            .unwrap()
            .locked = true;
        let revision_before = fixture.document.rev;
        assert!(fixture
            .set_selected_animation(&first, &sample_animation_model())
            .is_err());
        assert_eq!(fixture.document.rev, revision_before);
    }

    #[test]
    fn keyframes_add_delete_and_selection_round_trip_through_session_commands() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create keyframe target");
        session.save().expect("save rectangle baseline");
        assert!(session.create_default_animation().unwrap());
        session.save().expect("save animation baseline");

        assert!(session.set_animation_playhead_seconds(0.5).unwrap());
        assert_eq!(session.add_keyframes_at_playhead().unwrap(), 4);
        let selected = session.animation_snapshot();
        assert_eq!(selected.selected_keyframes.len(), 4);
        assert!(selected
            .selected_keyframes
            .iter()
            .all(|key| key.offset_permille == 500));
        assert!(session.is_dirty());
        assert_eq!(session.add_keyframes_at_playhead().unwrap(), 0);
        assert!(session.undo().unwrap());
        assert!(!session.is_dirty());
        assert!(session.animation_snapshot().selected_keyframes.is_empty());
        assert!(session.redo().unwrap());
        assert!(session.is_dirty());
        session.save().expect("save inserted keyframes");
        let mut reopened = ProjectSession::open(&session.project_dir).unwrap();
        reopened.select_sid(Some(&sid), ModifierState::default());
        let persisted = reopened.animation_snapshot();
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(model),
            ..
        }) = persisted.selected
        else {
            panic!("saved timeline should reopen");
        };
        assert!(TrackProp::ALL.iter().all(|track| {
            model.frames_of(*track).iter().any(|frame| {
                keyframe_offset_permille(frame.t, model.duration_ms / 1000.0) == Some(500)
            })
        }));

        let revision = session.document.rev;
        let undo_len = session.undo.undo_len();
        assert!(session.set_animation_playhead_seconds(0.9).unwrap());
        assert!(session
            .select_animation_keyframe(&sid, TrackProp::Position, 500, false)
            .unwrap());
        assert_eq!(session.animation_snapshot().playhead_seconds, 0.5);
        assert!(!session
            .select_animation_keyframe(&sid, TrackProp::Position, 500, false)
            .unwrap());
        assert!(session
            .select_animation_keyframe(&sid, TrackProp::Opacity, 500, true)
            .unwrap());
        assert_eq!(session.animation_snapshot().selected_keyframes.len(), 2);
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.undo.undo_len(), undo_len);
        assert!(!session.is_dirty());

        assert_eq!(session.delete_selected_keyframes().unwrap(), 2);
        assert!(session.is_dirty());
        assert!(session.undo().unwrap());
        assert!(!session.is_dirty());
        let restored = session.animation_snapshot();
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(model),
            ..
        }) = restored.selected
        else {
            panic!("undo should preserve the saved animation");
        };
        assert!(model.frames_of(TrackProp::Position).iter().any(|frame| {
            keyframe_offset_permille(frame.t, model.duration_ms / 1000.0) == Some(500)
        }));
        assert!(model.frames_of(TrackProp::Opacity).iter().any(|frame| {
            keyframe_offset_permille(frame.t, model.duration_ms / 1000.0) == Some(500)
        }));
    }

    #[test]
    fn selected_keyframe_easing_is_undoable_and_persists_after_reopen() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create easing target");
        session.select_sid(Some(&sid), ModifierState::default());
        assert!(session.create_default_animation().unwrap());
        assert!(session.set_animation_playhead_seconds(0.5).unwrap());
        assert_eq!(session.add_keyframes_at_playhead().unwrap(), 4);
        session.save().expect("save easing baseline");
        assert_eq!(session.animation_snapshot().selected_keyframes.len(), 4);

        assert_eq!(
            session
                .set_selected_keyframe_easing(Easing::EaseOut)
                .unwrap(),
            4
        );
        assert!(session.is_dirty());
        let Some(SelectedNodeSnapshot { .. }) = session.inspector_snapshot().selected else {
            panic!("single selection should remain present");
        };
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(after),
            ..
        }) = session.animation_snapshot().selected
        else {
            panic!("local timeline should remain editable");
        };
        assert!(TrackProp::ALL.iter().all(|track| {
            after.frames_of(*track).iter().any(|frame| {
                keyframe_offset_permille(frame.t, after.duration_ms / 1000.0) == Some(500)
                    && frame.easing == Easing::EaseOut
            })
        }));

        assert!(session.undo().unwrap());
        assert!(!session.is_dirty());
        assert!(session.redo().unwrap());
        assert!(session.is_dirty());
        session.save().expect("save changed easing");
        let mut reopened = ProjectSession::open(&session.project_dir).unwrap();
        reopened.select_sid(Some(&sid), ModifierState::default());
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(reopened_model),
            ..
        }) = reopened.animation_snapshot().selected
        else {
            panic!("timeline should reopen after easing edit");
        };
        assert!(TrackProp::ALL.iter().all(|track| {
            reopened_model.frames_of(*track).iter().any(|frame| {
                keyframe_offset_permille(frame.t, reopened_model.duration_ms / 1000.0) == Some(500)
                    && frame.easing == Easing::EaseOut
            })
        }));
    }

    #[test]
    fn selected_keyframe_easing_conflicts_are_rejected_atomically() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create conflict target");
        session.select_sid(Some(&sid), ModifierState::default());
        session.create_default_animation().unwrap();
        session.set_animation_playhead_seconds(0.5).unwrap();
        session.add_keyframes_at_playhead().unwrap();
        session
            .select_animation_keyframe(&sid, TrackProp::Position, 500, false)
            .unwrap();
        let revision = session.document.rev;
        let raw_css = session.document.raw_css.clone();
        let undo_len = session.undo.undo_len();
        let selected = session.animation_snapshot().selected_keyframes.clone();

        assert!(session
            .set_selected_keyframe_easing(Easing::EaseIn)
            .is_err());
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.document.raw_css, raw_css);
        assert_eq!(session.undo.undo_len(), undo_len);
        assert_eq!(session.animation_snapshot().selected_keyframes, selected);
    }

    #[test]
    fn keyframe_offsets_follow_duration_while_stored_times_remain_seconds() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create two-second animation target");
        session.select_sid(Some(&sid), ModifierState::default());
        let mut model = sample_animation_model();
        model.duration_ms = 2000.0;
        for (_, frames) in &mut model.tracks {
            for frame in frames {
                if (frame.t - 1.0).abs() < 1e-9 {
                    frame.t = 2.0;
                }
            }
        }
        assert!(session.set_selected_animation(&sid, &model).unwrap());
        session.save().expect("save two-second timeline");
        assert!(session.set_animation_playhead_seconds(1.1).unwrap());
        assert_eq!(session.add_keyframes_at_playhead().unwrap(), 4);
        let snapshot = session.animation_snapshot();
        assert!(snapshot
            .selected_keyframes
            .iter()
            .all(|keyframe| keyframe.offset_permille == 500));
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(model),
            ..
        }) = snapshot.selected
        else {
            panic!("local timeline should remain editable");
        };
        for track in TrackProp::ALL {
            assert!(model.frames_of(track).iter().any(|frame| {
                keyframe_offset_permille(frame.t, model.duration_ms / 1000.0) == Some(500)
                    && (frame.t - 1.0).abs() < 1e-9
            }));
        }
        session.save().expect("save normalized keyframe");
        let mut reopened = ProjectSession::open(&session.project_dir).unwrap();
        reopened.select_sid(Some(&sid), ModifierState::default());
        let reopened = reopened.animation_snapshot();
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(model),
            ..
        }) = reopened.selected
        else {
            panic!("two-second animation should reopen");
        };
        assert!(model
            .frames_of(TrackProp::Position)
            .iter()
            .any(|frame| (frame.t - 1.0).abs() < 1e-9));
    }

    #[test]
    fn selected_keyframe_move_preserves_multi_track_spacing_and_undoes_atomically() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create keyframe move target");
        session.select_sid(Some(&sid), ModifierState::default());
        let model = sample_keyframe_move_model();
        assert!(session.set_selected_animation(&sid, &model).unwrap());
        session.save().expect("save keyframe move baseline");
        session
            .select_animation_keyframe(&sid, TrackProp::Position, 400, false)
            .unwrap();
        session
            .select_animation_keyframe(&sid, TrackProp::Opacity, 600, true)
            .unwrap();

        let revision = session.document.rev;
        let raw_css = session.document.raw_css.clone();
        let undo_len = session.undo.undo_len();
        let playhead = session.animation_snapshot().playhead_seconds;
        let mut preview_offsets = session
            .preview_selected_keyframe_move(KeyframeOffsetDelta::new(100).unwrap())
            .unwrap()
            .into_iter()
            .map(|key| key.offset_permille)
            .collect::<Vec<_>>();
        preview_offsets.sort_unstable();
        assert_eq!(preview_offsets, vec![500, 700]);
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.document.raw_css, raw_css);
        assert_eq!(session.undo.undo_len(), undo_len);
        assert!(!session.is_dirty());
        assert_eq!(
            session
                .animation_snapshot()
                .selected_keyframes
                .iter()
                .map(|key| key.offset_permille)
                .collect::<Vec<_>>(),
            vec![400, 600]
        );

        assert_eq!(
            session
                .move_selected_keyframes_by(KeyframeOffsetDelta::new(100).unwrap())
                .unwrap(),
            2
        );
        assert_eq!(session.document.rev, revision + 1);
        assert_eq!(session.undo.undo_len(), undo_len + 1);
        assert_eq!(session.animation_snapshot().playhead_seconds, playhead);
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(moved),
            ..
        }) = session.animation_snapshot().selected
        else {
            panic!("local timeline should remain editable");
        };
        let position_time = moved
            .frames_of(TrackProp::Position)
            .iter()
            .find(|frame| {
                keyframe_offset_permille(frame.t, moved.duration_ms / 1000.0) == Some(500)
            })
            .expect("moved position keyframe")
            .t;
        let opacity_time = moved
            .frames_of(TrackProp::Opacity)
            .iter()
            .find(|frame| {
                keyframe_offset_permille(frame.t, moved.duration_ms / 1000.0) == Some(700)
            })
            .expect("moved opacity keyframe")
            .t;
        assert!((position_time - 1.0).abs() < 1e-9);
        // The CSS parser round-trips percentage times through f32 precision.
        let parsed_time_epsilon = 1e-6;
        assert!(
            (opacity_time - 1.4).abs() < parsed_time_epsilon,
            "expected opacity t≈1.4s, got {opacity_time}; model={moved:?}"
        );
        assert!((opacity_time - position_time - 0.4).abs() < parsed_time_epsilon);
        assert!(session.is_dirty());

        assert!(session.undo().unwrap());
        assert!(!session.is_dirty());
        assert!(session.redo().unwrap());
        assert!(session.is_dirty());
        session.save().expect("save moved keyframes");
        let mut reopened = ProjectSession::open(&session.project_dir).unwrap();
        reopened.select_sid(Some(&sid), ModifierState::default());
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(reopened_model),
            ..
        }) = reopened.animation_snapshot().selected
        else {
            panic!("moved timeline should reopen");
        };
        assert!(reopened_model
            .frames_of(TrackProp::Position)
            .iter()
            .any(|frame| (frame.t - 1.0).abs() < 1e-9));
        assert!(reopened_model
            .frames_of(TrackProp::Opacity)
            .iter()
            .any(|frame| {
                keyframe_offset_permille(frame.t, reopened_model.duration_ms / 1000.0) == Some(700)
                    && (frame.t - 1.4).abs() < parsed_time_epsilon
            }));
    }

    #[test]
    fn selected_same_track_keyframes_can_cross_their_source_stops_together() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create crossing target");
        session.select_sid(Some(&sid), ModifierState::default());
        let mut model = sample_keyframe_move_model();
        let position = model
            .tracks
            .iter_mut()
            .find(|(track, _)| *track == TrackProp::Position)
            .expect("position track");
        position.1.insert(
            1,
            Keyframe {
                t: 1.0,
                value: KeyValue::Offset(12.0, 6.0),
                easing: Easing::Linear,
            },
        );
        assert!(session.set_selected_animation(&sid, &model).unwrap());
        session.save().expect("save crossing baseline");
        session
            .select_animation_keyframe(&sid, TrackProp::Position, 400, false)
            .unwrap();
        session
            .select_animation_keyframe(&sid, TrackProp::Position, 500, true)
            .unwrap();

        let preview = session
            .preview_selected_keyframe_move(KeyframeOffsetDelta::new(100).unwrap())
            .unwrap();
        assert_eq!(
            preview
                .iter()
                .map(|key| key.offset_permille)
                .collect::<Vec<_>>(),
            vec![500, 600]
        );
        assert_eq!(
            session
                .move_selected_keyframes_by(KeyframeOffsetDelta::new(100).unwrap())
                .unwrap(),
            2
        );
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(moved),
            ..
        }) = session.animation_snapshot().selected
        else {
            panic!("local timeline should remain editable");
        };
        let mut moved_offsets = moved
            .frames_of(TrackProp::Position)
            .iter()
            .map(|frame| keyframe_offset_permille(frame.t, moved.duration_ms / 1000.0).unwrap())
            .collect::<Vec<_>>();
        moved_offsets.sort_unstable();
        assert_eq!(moved_offsets, vec![500, 600, 900]);
    }

    #[test]
    fn selected_keyframe_move_rejects_bounds_and_same_track_collisions_atomically() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create collision target");
        session.select_sid(Some(&sid), ModifierState::default());
        let mut model = sample_keyframe_move_model();
        let position = model
            .tracks
            .iter_mut()
            .find(|(track, _)| *track == TrackProp::Position)
            .expect("position track");
        position.1.insert(
            1,
            Keyframe {
                t: 1.2,
                value: KeyValue::Offset(20.0, 10.0),
                easing: Easing::Linear,
            },
        );
        assert!(session.set_selected_animation(&sid, &model).unwrap());
        session.save().expect("save collision baseline");
        session
            .select_animation_keyframe(&sid, TrackProp::Position, 400, false)
            .unwrap();

        let revision = session.document.rev;
        let raw_css = session.document.raw_css.clone();
        let undo_len = session.undo.undo_len();
        let selected = session.animation_snapshot().selected_keyframes.clone();
        assert!(session
            .preview_selected_keyframe_move(KeyframeOffsetDelta::new(200).unwrap())
            .is_err());
        assert!(session
            .move_selected_keyframes_by(KeyframeOffsetDelta::new(200).unwrap())
            .is_err());
        assert!(session
            .preview_selected_keyframe_move(KeyframeOffsetDelta::new(-500).unwrap())
            .is_err());
        assert!(session
            .move_selected_keyframes_by(KeyframeOffsetDelta::new(-500).unwrap())
            .is_err());
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.document.raw_css, raw_css);
        assert_eq!(session.undo.undo_len(), undo_len);
        assert_eq!(session.animation_snapshot().selected_keyframes, selected);
        assert!(!session.is_dirty());
    }

    #[test]
    fn keyframe_nudge_catalog_commands_move_exactly_one_percent_as_one_transaction() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create nudge target");
        session.select_sid(Some(&sid), ModifierState::default());
        assert!(session
            .set_selected_animation(&sid, &sample_keyframe_move_model())
            .unwrap());
        session.save().expect("save nudge baseline");
        session
            .select_animation_keyframe(&sid, TrackProp::Position, 400, false)
            .unwrap();
        session
            .select_animation_keyframe(&sid, TrackProp::Opacity, 600, true)
            .unwrap();
        assert!(session.animation_frame_snap_enabled());

        let revision = session.document.rev;
        let undo_len = session.undo.undo_len();
        assert!(matches!(
            session.dispatch_catalog("anim.keyframe_nudge_earlier"),
            CommandOutcome::Applied
        ));
        assert_eq!(session.document.rev, revision + 1);
        assert_eq!(session.undo.undo_len(), undo_len + 1);
        assert_eq!(
            session
                .animation_snapshot()
                .selected_keyframes
                .iter()
                .map(|key| key.offset_permille)
                .collect::<Vec<_>>(),
            vec![390, 590]
        );

        assert!(session.undo().unwrap());
        assert!(!session.is_dirty());
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(restored),
            ..
        }) = session.animation_snapshot().selected
        else {
            panic!("undo should restore the baseline timeline");
        };
        assert!(restored.frames_of(TrackProp::Position).iter().any(|frame| {
            keyframe_offset_permille(frame.t, restored.duration_ms / 1000.0) == Some(400)
        }));
        assert!(restored.frames_of(TrackProp::Opacity).iter().any(|frame| {
            keyframe_offset_permille(frame.t, restored.duration_ms / 1000.0) == Some(600)
        }));

        assert!(session.redo().unwrap());
        assert!(session.is_dirty());
        session.save().expect("save nudged timeline");
        let mut reopened = ProjectSession::open(&session.project_dir).unwrap();
        reopened.select_sid(Some(&sid), ModifierState::default());
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(reopened_model),
            ..
        }) = reopened.animation_snapshot().selected
        else {
            panic!("reopened nudge should remain an editable timeline");
        };
        assert!(reopened_model
            .frames_of(TrackProp::Position)
            .iter()
            .any(|frame| {
                keyframe_offset_permille(frame.t, reopened_model.duration_ms / 1000.0) == Some(390)
            }));
        assert!(reopened_model
            .frames_of(TrackProp::Opacity)
            .iter()
            .any(|frame| {
                keyframe_offset_permille(frame.t, reopened_model.duration_ms / 1000.0) == Some(590)
            }));
        reopened
            .select_animation_keyframe(&sid, TrackProp::Position, 390, false)
            .unwrap();
        reopened
            .select_animation_keyframe(&sid, TrackProp::Opacity, 590, true)
            .unwrap();
        assert!(matches!(
            reopened.dispatch_catalog("anim.keyframe_nudge_later"),
            CommandOutcome::Applied
        ));
        assert_eq!(
            reopened
                .animation_snapshot()
                .selected_keyframes
                .iter()
                .map(|key| key.offset_permille)
                .collect::<Vec<_>>(),
            vec![400, 600]
        );
    }

    #[test]
    fn selected_keyframe_duplicate_keeps_originals_and_is_one_undoable_step() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create keyframe copy target");
        session.select_sid(Some(&sid), ModifierState::default());
        let model = sample_keyframe_move_model();
        assert!(session.set_selected_animation(&sid, &model).unwrap());
        session.save().expect("save keyframe copy baseline");
        session
            .select_animation_keyframe(&sid, TrackProp::Position, 400, false)
            .unwrap();
        session
            .select_animation_keyframe(&sid, TrackProp::Opacity, 600, true)
            .unwrap();

        let revision = session.document.rev;
        let raw_css = session.document.raw_css.clone();
        let undo_len = session.undo.undo_len();
        let playhead = session.animation_snapshot().playhead_seconds;
        let mut preview = session
            .preview_selected_keyframe_copy(KeyframeOffsetDelta::new(100).unwrap())
            .unwrap()
            .into_iter()
            .map(|key| key.offset_permille)
            .collect::<Vec<_>>();
        preview.sort_unstable();
        assert_eq!(preview, vec![500, 700]);
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.document.raw_css, raw_css);
        assert_eq!(session.undo.undo_len(), undo_len);
        assert!(!session.is_dirty());
        assert_eq!(
            session
                .animation_snapshot()
                .selected_keyframes
                .iter()
                .map(|key| key.offset_permille)
                .collect::<Vec<_>>(),
            vec![400, 600]
        );

        assert_eq!(
            session
                .duplicate_selected_keyframes_by(KeyframeOffsetDelta::new(100).unwrap())
                .unwrap(),
            2
        );
        assert_eq!(session.document.rev, revision + 1);
        assert_eq!(session.undo.undo_len(), undo_len + 1);
        assert_eq!(session.animation_snapshot().playhead_seconds, playhead);
        assert_eq!(
            session
                .animation_snapshot()
                .selected_keyframes
                .iter()
                .map(|key| key.offset_permille)
                .collect::<Vec<_>>(),
            vec![500, 700]
        );
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(copied),
            ..
        }) = session.animation_snapshot().selected
        else {
            panic!("local timeline should remain editable");
        };
        for (track, source, target) in [
            (TrackProp::Position, 400, 500),
            (TrackProp::Opacity, 600, 700),
        ] {
            let offsets = copied
                .frames_of(track)
                .iter()
                .filter_map(|frame| keyframe_offset_permille(frame.t, copied.duration_ms / 1000.0))
                .collect::<Vec<_>>();
            assert!(
                offsets.contains(&source),
                "source stop {source} must remain"
            );
            assert!(offsets.contains(&target), "copied stop {target} must exist");
        }
        assert!(session.is_dirty());
        assert!(session.undo().unwrap());
        assert!(!session.is_dirty());
        assert!(session.redo().unwrap());
        assert!(session.is_dirty());
        session.save().expect("save copied keyframes");

        let mut reopened = ProjectSession::open(&session.project_dir).unwrap();
        reopened.select_sid(Some(&sid), ModifierState::default());
        let Some(SelectedAnimationSnapshot {
            source: AnimSource::Timeline(reopened_model),
            ..
        }) = reopened.animation_snapshot().selected
        else {
            panic!("copied timeline should reopen");
        };
        assert!(reopened_model
            .frames_of(TrackProp::Position)
            .iter()
            .any(
                |frame| keyframe_offset_permille(frame.t, reopened_model.duration_ms / 1000.0)
                    == Some(400)
            ));
        assert!(reopened_model
            .frames_of(TrackProp::Position)
            .iter()
            .any(
                |frame| keyframe_offset_permille(frame.t, reopened_model.duration_ms / 1000.0)
                    == Some(500)
            ));
        assert!(reopened_model
            .frames_of(TrackProp::Opacity)
            .iter()
            .any(
                |frame| keyframe_offset_permille(frame.t, reopened_model.duration_ms / 1000.0)
                    == Some(600)
            ));
        assert!(reopened_model
            .frames_of(TrackProp::Opacity)
            .iter()
            .any(
                |frame| keyframe_offset_permille(frame.t, reopened_model.duration_ms / 1000.0)
                    == Some(700)
            ));
    }

    #[test]
    fn selected_keyframe_duplicate_rejects_bounds_and_collisions_atomically() {
        let mut session = fixture_session(Document::new_default());
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 48.0,
                    y: 64.0,
                    width: 100.0,
                    height: 80.0,
                },
            )
            .expect("create duplicate-conflict target");
        session.select_sid(Some(&sid), ModifierState::default());
        let mut model = sample_keyframe_move_model();
        let position = model
            .tracks
            .iter_mut()
            .find(|(track, _)| *track == TrackProp::Position)
            .expect("position track");
        position.1.push(Keyframe {
            t: 1.0,
            value: KeyValue::Offset(24.0, 12.0),
            easing: Easing::Linear,
        });
        position.1.sort_by(|left, right| left.t.total_cmp(&right.t));
        assert!(session.set_selected_animation(&sid, &model).unwrap());
        session.save().expect("save duplicate-conflict baseline");
        session
            .select_animation_keyframe(&sid, TrackProp::Position, 400, false)
            .unwrap();

        let revision = session.document.rev;
        let raw_css = session.document.raw_css.clone();
        let undo_len = session.undo.undo_len();
        let selected = session.animation_snapshot().selected_keyframes.clone();
        assert!(session
            .preview_selected_keyframe_copy(KeyframeOffsetDelta::new(100).unwrap())
            .is_err());
        assert!(session
            .duplicate_selected_keyframes_by(KeyframeOffsetDelta::new(100).unwrap())
            .is_err());
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.document.raw_css, raw_css);
        assert_eq!(session.undo.undo_len(), undo_len);
        assert_eq!(session.animation_snapshot().selected_keyframes, selected);
        assert!(!session.is_dirty());

        session
            .select_animation_keyframe(&sid, TrackProp::Position, 900, false)
            .unwrap();
        let selected = session.animation_snapshot().selected_keyframes.clone();
        assert!(session
            .preview_selected_keyframe_copy(KeyframeOffsetDelta::new(200).unwrap())
            .is_err());
        assert!(session
            .duplicate_selected_keyframes_by(KeyframeOffsetDelta::new(200).unwrap())
            .is_err());
        assert_eq!(session.document.rev, revision);
        assert_eq!(session.document.raw_css, raw_css);
        assert_eq!(session.undo.undo_len(), undo_len);
        assert_eq!(session.animation_snapshot().selected_keyframes, selected);
        assert!(!session.is_dirty());
    }

    #[test]
    fn animation_model_rejects_conflicting_easings_for_same_css_stop() {
        let mut fixture = fixture_session(Document::new_default());
        let sid = fixture
            .create_text(0, [48.0, 64.0])
            .expect("create easing validation target");
        fixture.select_sid(Some(&sid), ModifierState::default());
        let mut model = sample_animation_model();
        let (_, position_frames) = model
            .tracks
            .iter_mut()
            .find(|(property, _)| *property == TrackProp::Position)
            .expect("position track");
        position_frames
            .iter_mut()
            .find(|frame| frame.t == 1.0)
            .expect("end position keyframe")
            .easing = Easing::Bezier(0.25, 0.1, 0.25, 1.0);
        let revision_before = fixture.document.rev;
        let css_before = fixture.document.raw_css.clone();

        assert!(fixture.set_selected_animation(&sid, &model).is_err());
        assert_eq!(fixture.document.rev, revision_before);
        assert_eq!(fixture.document.raw_css, css_before);
        assert!(matches!(
            fixture.animation_snapshot().selected.unwrap().source,
            AnimSource::None
        ));
    }

    #[test]
    fn canvas_nudge_commands_move_one_pixel_and_are_undoable() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 20.0,
                    y: 30.0,
                    width: 40.0,
                    height: 25.0,
                },
            )
            .expect("insert nudge target");
        let initial = session
            .document
            .node(session.document.find_by_sid(&sid).expect("nudge target"))
            .expect("nudge node")
            .geom;
        let cases = [
            ("canvas.nudge_left", -1.0, 0.0),
            ("canvas.nudge_right", 1.0, 0.0),
            ("canvas.nudge_up", 0.0, -1.0),
            ("canvas.nudge_down", 0.0, 1.0),
        ];
        for (catalog_id, dx, dy) in cases {
            let undo_before = session.undo.undo_len();
            assert!(
                session.dispatch_catalog(catalog_id).is_applied(),
                "{catalog_id}"
            );
            let moved = session
                .document
                .node(session.document.find_by_sid(&sid).expect("nudge target"))
                .expect("nudge node")
                .geom;
            assert_eq!(moved.x, initial.x + dx, "{catalog_id} x");
            assert_eq!(moved.y, initial.y + dy, "{catalog_id} y");
            assert_eq!(session.undo.undo_len(), undo_before + 1, "{catalog_id}");
            assert!(session.undo().expect("undo nudge"));
            let restored = session
                .document
                .node(session.document.find_by_sid(&sid).expect("nudge target"))
                .expect("nudge node")
                .geom;
            assert_eq!(restored, initial, "{catalog_id} undo");
        }

        let mut empty = fixture_session(Document::new_default());
        assert!(matches!(
            empty.dispatch_catalog("canvas.nudge_left"),
            CommandOutcome::NotRun { .. }
        ));
    }

    #[test]
    fn dispatches_supported_tool_commands_and_reports_unknown() {
        let mut session = fixture_session(Document::new_default());

        assert_eq!(session.active_tool(), ToolId::Select);
        assert!(session.dispatch_catalog("tool.hand").is_applied());
        assert_eq!(session.active_tool(), ToolId::Hand);
        assert!(session.dispatch_catalog("tool.select").is_applied());
        assert_eq!(session.active_tool(), ToolId::Select);
        assert!(session.dispatch_catalog("tool.rect").is_applied());
        assert_eq!(session.active_tool(), ToolId::Rect);
        assert!(session.dispatch_catalog("tool.ellipse").is_applied());
        assert_eq!(session.active_tool(), ToolId::Ellipse);
        assert!(session.dispatch_catalog("tool.text").is_applied());
        assert_eq!(session.active_tool(), ToolId::Text);
        assert!(matches!(
            session.dispatch_catalog("edit.undo"),
            CommandOutcome::NotRun { .. }
        ));
        assert!(matches!(
            session.dispatch_catalog("edit.redo"),
            CommandOutcome::NotRun { .. }
        ));
        assert!(matches!(
            session.dispatch_catalog("history.undo"),
            CommandOutcome::NotRun { .. }
        ));
    }

    #[test]
    fn selected_name_rejects_empty_controls_locked_and_multiselect_without_mutation() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 20.0,
                },
            )
            .expect("insert rectangle");
        let original = session.inspector_snapshot().selected.unwrap().name;
        let too_long = "x".repeat(257);
        let invalid = ["", "  ", "line\nbreak", "tab\tname", too_long.as_str()];
        for value in invalid {
            let revision = session.document.rev;
            assert!(session.set_selected_name(&sid, value).is_err());
            assert_eq!(session.document.rev, revision);
            assert_eq!(
                session.inspector_snapshot().selected.unwrap().name,
                original
            );
        }
        assert!(!session
            .set_selected_name(&sid, &format!(" {original} "))
            .expect("canonical no-op"));

        session.set_selected_flags(None, Some(true)).unwrap();
        let locked_revision = session.document.rev;
        assert!(session.set_selected_name(&sid, "Locked name").is_err());
        assert_eq!(session.document.rev, locked_revision);
        session.set_selected_flags(None, Some(false)).unwrap();

        let other = session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 80.0,
                    y: 20.0,
                    width: 30.0,
                    height: 20.0,
                },
            )
            .expect("insert second node");
        session.select_sid(Some(&sid), ModifierState::default());
        session.select_sid(
            Some(&other),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            },
        );
        let multi_revision = session.document.rev;
        assert!(session.set_selected_name(&sid, "No multi-rename").is_err());
        assert_eq!(session.document.rev, multi_revision);
    }

    #[test]
    fn selected_name_is_trimmed_undoable_and_persists_after_reopen() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let sid = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 20.0,
                },
            )
            .expect("insert rectangle");
        let original = session.inspector_snapshot().selected.unwrap().name;

        assert!(session
            .set_selected_name(&sid, "  Hero banner  ")
            .expect("rename selected node"));
        assert_eq!(
            session.inspector_snapshot().selected.unwrap().name,
            "Hero banner"
        );
        session.save().expect("save renamed node");
        assert!(!session.is_dirty());
        assert!(session.undo().expect("undo rename"));
        assert_eq!(
            session.inspector_snapshot().selected.unwrap().name,
            original
        );
        assert!(session.is_dirty());
        assert!(session.redo().expect("redo rename"));
        assert_eq!(
            session.inspector_snapshot().selected.unwrap().name,
            "Hero banner"
        );
        assert!(!session.is_dirty());
        let reopened = ProjectSession::open(session.project_dir()).expect("reopen renamed project");
        assert!(reopened
            .document
            .find_by_sid(&sid)
            .and_then(|id| reopened.document.node(id))
            .is_some_and(|node| node.name == "Hero banner"));
    }

    #[test]
    fn select_all_uses_artboard_and_skips_hidden_or_locked_nodes() {
        let mut fixture = fixture_session(Document::new_default());
        let session = &mut fixture;
        let visible = session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 10.0,
                    y: 10.0,
                    width: 30.0,
                    height: 20.0,
                },
            )
            .expect("insert visible node");
        session
            .create_ellipse(
                0,
                CanvasRect {
                    x: 50.0,
                    y: 10.0,
                    width: 30.0,
                    height: 20.0,
                },
            )
            .expect("insert hidden node");
        session.set_selected_flags(Some(true), None).unwrap();
        session
            .create_rectangle(
                0,
                CanvasRect {
                    x: 90.0,
                    y: 10.0,
                    width: 30.0,
                    height: 20.0,
                },
            )
            .expect("insert locked node");
        session.set_selected_flags(None, Some(true)).unwrap();
        session.select_sid(None, ModifierState::default());

        assert_eq!(session.select_all_in_artboard(0).unwrap(), 1);
        assert_eq!(session.selected_sids(), [visible.as_str()]);
        assert!(session.dispatch_catalog("edit.select_all").is_applied());
        assert_eq!(session.selected_sids(), [visible.as_str()]);
        assert!(session.select_all_in_artboard(99).is_err());
    }

    #[test]
    fn selection_preserves_click_order_and_shift_toggles() {
        let mut session = fixture_session(Document::new_default());
        assert!(session.select_sid(Some("a1"), ModifierState::default()));
        assert!(session.select_sid(
            Some("b2"),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            }
        ));
        assert_eq!(session.selected_sids(), ["a1", "b2"]);
        assert!(session.select_sid(
            Some("a1"),
            ModifierState {
                shift: true,
                ..ModifierState::default()
            }
        ));
        assert_eq!(session.selected_sids(), ["b2"]);
        assert!(session.select_sid(None, ModifierState::default()));
        assert!(session.selected_sids().is_empty());
    }
}
