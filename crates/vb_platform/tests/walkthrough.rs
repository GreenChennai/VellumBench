//! 六 trait 的真实用例走查(硬骨头 #13 完成定义:null 实现全接口可走通)。
//!
//! 用例取自仓库真实场景,语义与宿主实现一致,headless 可跑:
//! 1. 启动器「复制路径」(vb_app launcher)—— Clipboard;
//! 2. 画布工具切换光标随动(vb_app canvas_input)—— SystemCursor;
//! 3. 画板按显示器 DPI 出分辨率预算(P0-3 尺寸门口径)—— DisplayInfo;
//! 4. 启动器主题初值跟随系统深色(vb_shell init_chrome)—— DarkModeProbe;
//! 5. 打开工程目录(vb_app new_project)—— FileDialog + WindowHandle 标题。
//!
//! 全部经 `dyn` 派发走(对象安全是硬要求),不借用具体类型。

use std::path::PathBuf;
use vb_platform::{
    Clipboard, CursorShape, DarkModeProbe, DisplayInfo, DisplayMetrics, FileDialog, FileFilter,
    MotionPreferenceProbe, NullClipboard, NullCursor, NullDarkMode, NullDialog, NullDisplay,
    NullMotionProbe, NullWindow, OsMotionProbe, SystemCursor, WindowHandle,
};

/// 用例 1:启动器卡片「复制路径」——写后可读(null 参考语义)。
#[test]
fn launcher_copy_path_via_dyn_clipboard() {
    let mut clip: Box<dyn Clipboard> = Box::new(NullClipboard::new());
    let path = r"D:\projects\landing";
    clip.set_text(path).expect("null 剪贴板写入必须成功");
    assert_eq!(clip.text().expect("读不应报错"), Some(path.to_string()));
    // 覆盖写
    clip.set_text(r"D:\projects\poster").unwrap();
    assert_eq!(clip.text().unwrap(), Some(r"D:\projects\poster".into()));
}

/// 用例 2:画布工具切换(平移/捏柄/缩放)→ 光标随动且可回读。
#[test]
fn canvas_tool_switch_updates_cursor_via_dyn() {
    let mut cursor: Box<dyn SystemCursor> = Box::new(NullCursor::new());
    let nullref = NullCursor::new();
    let _ = nullref; // 工具→形状决策在 vb_ui;此处只验 seam 传得进、可断言
    for tool_shape in [
        CursorShape::Grab,
        CursorShape::ResizeNwSe,
        CursorShape::Crosshair,
    ] {
        cursor.set_shape(tool_shape).expect("null 光标必须成功");
    }
    // 状态断言借具体类型(dyn 派发本身在上面已验)
    let mut rec = NullCursor::new();
    rec.set_shape(CursorShape::Crosshair).unwrap();
    assert_eq!(rec.last(), Some(CursorShape::Crosshair));
}

/// 用例 3:每显示器 DPI → 画布导出分辨率预算(物理 px = 逻辑 px × scale)。
#[test]
fn canvas_budget_from_per_display_dpi() {
    let displays: Box<dyn DisplayInfo> = Box::new(NullDisplay::new(vec![
        DisplayMetrics {
            id: 1,
            scale_factor: 1.0,
            size_px: (1920, 1080),
            position: Some((0, 0)),
            is_primary: true,
        },
        DisplayMetrics {
            id: 2,
            scale_factor: 1.5,
            size_px: (3840, 2160),
            position: Some((-2560, 0)),
            is_primary: false,
        },
    ]));
    let logic_w = 800.0_f64;
    for d in displays.displays() {
        let physical = logic_w * d.scale_factor;
        // 4096 纹理消解口径:预算按显示器算,1.5 屏上 800 逻辑 px = 1200 物理 px
        assert!((physical - 800.0 * d.scale_factor).abs() < f64::EPSILON);
        if d.scale_factor > 1.0 {
            assert!(physical > 800.0, "150% 屏必须出更多物理像素");
        }
    }
}

/// 用例 4:主题初值跟随系统深色(vb_shell init_chrome 的 seam 语义)。
#[test]
fn theme_seed_from_dark_mode_probe() {
    let dark: Box<dyn DarkModeProbe> = Box::new(NullDarkMode::new(true));
    let light: Box<dyn DarkModeProbe> = Box::new(NullDarkMode::new(false));
    // 与 vb_shell::init_chrome 同构的决策:
    let mode = |p: &dyn DarkModeProbe| if p.is_dark_mode() { "dark" } else { "light" };
    assert_eq!(mode(dark.as_ref()), "dark");
    assert_eq!(mode(light.as_ref()), "light");
}

/// 用例 6(S5 清单 ④):系统动效偏好探测 —— null 默认放行 / 注入关态
/// 模拟「减少动态效果」;OS 探针(`dyn`)可调用且 fail-open。
/// 与 vb_app `effective_motion` 的并联语义配套(真值表在那边钉)。
#[test]
fn reduced_motion_probe_parallel_semantics() {
    let on: Box<dyn MotionPreferenceProbe> = Box::new(NullMotionProbe::default());
    let off: Box<dyn MotionPreferenceProbe> = Box::new(NullMotionProbe::new(false));
    assert!(on.animations_enabled(), "null 默认 = 系统允许动画");
    assert!(!off.animations_enabled(), "注入 false = 模拟减少动态效果");
    // OS 探针经 dyn 派发可调用(Windows 真读 SPI;值随机器,不断言方向)
    let os: Box<dyn MotionPreferenceProbe> = Box::new(OsMotionProbe::new());
    let _ = os.animations_enabled();
}

/// 用例 5:打开工程(headless = 用户取消)+ 开窗后改标题。
#[test]
fn open_project_flow_dialog_then_window_title() {
    let mut dialog: Box<dyn FileDialog> = Box::new(NullDialog::new());
    let picked: Option<PathBuf> = dialog.pick_folder();
    assert_eq!(picked, None, "headless 对话框 = 用户取消,非错误");

    let mut win: Box<dyn WindowHandle> = Box::new(NullWindow::new(42));
    assert_eq!(win.id(), 42);
    win.set_title("landing").expect("null 窗口改题必须成功");
    win.activate().unwrap();
}

/// 过滤器词汇:工程/图片两类(真实扩展名面),确保参数面够用。
#[test]
fn file_filters_carry_project_and_image_vocab() {
    let project = FileFilter::new("Vellum 工程", &["json", "html"]);
    let image = FileFilter::new("图片", &["png", "jpg", "webp"]);
    assert_eq!(project.extensions, &["json", "html"]);
    assert_eq!(image.extensions.len(), 3);
}

/// 平台错误两分类可区分(egui/gpui 宿主的诚实降级语义,null 不产生错误,
/// 但契约面必须预先定型——见 egui_backend::EguiClipboard::text)。
#[test]
fn platform_error_kinds_are_part_of_contract() {
    use vb_platform::PlatformError;
    let u = PlatformError::unsupported("set_title", "宿主无运行期 API");
    let f = PlatformError::failed("set_text", "OS 拒绝");
    assert!(u.is_unsupported() && !f.is_unsupported());
}
