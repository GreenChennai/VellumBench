//! 工具栏四向停靠(阶段 6 / 副文档 07-1)与布局持久化(07-2)。
//!
//! **V3 决策**:自研轻量停靠(`enum DockSide` + `egui::Panel` 分支),
//! **不引 `egui_dock`** —— 工具栏只需 4 个位置 + 吸附,不值得一个 UI 框架;
//! 也与 `design/14` F8「面板固定停靠、不做浮动」的精神一致
//! (**面板**仍不可浮动,本模块只管**工具栏**)。
//!
//! **吸附不做浮动**(07-3 决策):拖动只在四边之间吸附,松手落在中间则回弹原位。
//!
//! 本模块只放**纯数据与纯函数**(停靠几何、吸附判定、配置读写),
//! 渲染在 `toolbar.rs`,`VellumApp` 只持有 [`WorkspaceConfig`] 的子集。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 工具栏停靠边。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockSide {
    Top,
    Left,
    Right,
    Bottom,
}

impl DockSide {
    pub const ALL: [DockSide; 4] = [
        DockSide::Top,
        DockSide::Left,
        DockSide::Right,
        DockSide::Bottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DockSide::Top => "顶部",
            DockSide::Left => "左侧",
            DockSide::Right => "右侧",
            DockSide::Bottom => "底部",
        }
    }

    /// 该边是否为竖条(左/右停靠 → 单/双列;顶/底 → 单行)。
    pub fn is_vertical(self) -> bool {
        matches!(self, DockSide::Left | DockSide::Right)
    }

    /// 吸附判定(纯函数):指针落在窗口边缘 `band` 像素内 → 最近的那条边。
    ///
    /// 四条边的距离取**到边界的距离**(不是到中心),因此角上按下会选中
    /// 距离更近的那条边;都不在带内 → `None`(调用方据此**回弹**,不做浮动)。
    pub fn nearest(px: f32, py: f32, w: f32, h: f32, band: f32) -> Option<DockSide> {
        let (dl, dr, dt, db) = (px, w - px, py, h - py);
        let min = dl.min(dr).min(dt).min(db);
        if min > band {
            return None;
        }
        let mut best = DockSide::Left;
        let mut best_d = dl;
        for (d, side) in [
            (dr, DockSide::Right),
            (dt, DockSide::Top),
            (db, DockSide::Bottom),
        ] {
            if d < best_d {
                best_d = d;
                best = side;
            }
        }
        Some(best)
    }
}

/// 工具栏停靠后的尺寸(宽, 高)。
///
/// - 左/右:宽 60(单列)/ 88(双列),高占满(由 Panel 决定,这里给最小高);
/// - 顶/底:高 = 控制条同高(`space::CONTROL_BAR_HEIGHT`),宽占满。
pub fn toolbar_size(side: DockSide, columns: u8) -> (f32, f32) {
    let cols = columns.clamp(1, 2) as f32;
    match side {
        DockSide::Left | DockSide::Right => (60.0 + (cols - 1.0) * 28.0, 0.0),
        DockSide::Top | DockSide::Bottom => (0.0, vb_ui::theme::space::CONTROL_BAR_HEIGHT),
    }
}

/// 单/双列钳制(顶/底停靠时列数无意义,恒 1)。
pub fn clamp_columns(side: DockSide, columns: u8) -> u8 {
    if side.is_vertical() {
        columns.clamp(1, 2)
    } else {
        1
    }
}

// ─────────────────────────── 布局持久化(workspace.json) ───────────────────────────

/// 当前 schema 版本。**加字段必须同时抬版本 + 写迁移**(07 §6 风险)。
pub const SCHEMA_VERSION: u32 = 1;

/// 工作区布局(`workspace.json`)。
///
/// 覆盖 07-2-1 要求的四项:工具栏停靠位 + 单双列 + 面板坞展开态 + 面板 Tab 顺序,
/// 另存面板坞当前 Tab 与「是否隐藏所有面板」。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    pub schema_version: u32,
    /// 工具栏停靠边。
    pub toolbar_dock: DockSide,
    /// 左/右停靠时的列数(1 或 2)。
    pub toolbar_columns: u8,
    /// 面板坞是否折叠(02-1 的「用户手动折叠」态)。
    pub dock_collapsed: bool,
    /// 面板坞 Tab 顺序(语义 id 的排列)。
    pub panel_order: Vec<usize>,
    /// 面板坞当前 Tab。
    pub panel_tab: usize,
    /// 是否处于「Tab 隐藏所有面板」态。
    pub panels_hidden: bool,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            // 08-9 口径:默认**底部停靠** —— 与目标⑥原文「底部工具栏可以移动到
            // 顶部/左边/右边」一致,也保住既有用户的肌肉记忆(此前就是底部工具条)。
            toolbar_dock: DockSide::Bottom,
            toolbar_columns: 1,
            dock_collapsed: false,
            panel_order: vec![0, 1, 2, 3],
            panel_tab: 0,
            panels_hidden: false,
        }
    }
}

/// 配置文件路径。
///
/// 解析顺序:`VB_WORKSPACE` 环境变量(测试/便携用)→ `%APPDATA%\VellumBench\`
/// (Windows)/ `$XDG_CONFIG_HOME|$HOME/.config/vellum-bench/`(其它平台)。
/// 全部不可得 → `None`(调用方退化为纯内存,并如实提示)。
pub fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VB_WORKSPACE") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    let base = if cfg!(windows) {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".config"))
            })
    }?;
    Some(base.join("VellumBench").join("workspace.json"))
}

/// 读取工作区配置。返回 `(配置, 告警)`。
///
/// **损坏不静默**(07-2-2):文件存在但解析失败 / 版本更新 → 回退默认,
/// 并把中文原因交回调用方(由 `VellumApp` 写进状态栏与日志)。
pub fn load_from(path: &std::path::Path) -> (WorkspaceConfig, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return (WorkspaceConfig::default(), None), // 首次运行:无文件不是错误
    };
    match serde_json::from_str::<WorkspaceConfig>(&text) {
        Ok(cfg) => {
            if cfg.schema_version != SCHEMA_VERSION {
                return (
                    WorkspaceConfig::default(),
                    Some(format!(
                        "workspace.json 版本 {} 与当前 {} 不符,已回退默认布局",
                        cfg.schema_version, SCHEMA_VERSION
                    )),
                );
            }
            (normalize(cfg), None)
        }
        Err(e) => (
            WorkspaceConfig::default(),
            Some(format!("workspace.json 解析失败,已回退默认布局:{e}")),
        ),
    }
}

/// 读取真实路径上的配置(读不到路径 → 默认 + 告警)。
pub fn load() -> (WorkspaceConfig, Option<String>) {
    match config_path() {
        Some(p) => load_from(&p),
        None => (
            WorkspaceConfig::default(),
            Some("找不到配置目录,本次布局不会持久化(可用 VB_WORKSPACE 指定)".into()),
        ),
    }
}

/// 写回配置(原子性:先写临时文件再改名,避免半截 JSON)。
pub fn save_to(path: &std::path::Path, cfg: &WorkspaceConfig) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败:{e}"))?;
    }
    let mut cfg = cfg.clone();
    cfg.schema_version = SCHEMA_VERSION;
    let text =
        serde_json::to_string_pretty(&cfg).map_err(|e| format!("序列化工作区配置失败:{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写工作区配置失败:{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("提交工作区配置失败:{e}"))
}

/// 写回真实路径(无路径 → 报错说明,不静默)。
pub fn save(cfg: &WorkspaceConfig) -> Result<(), String> {
    let p = config_path().ok_or_else(|| "找不到配置目录,布局未持久化".to_string())?;
    save_to(&p, cfg)
}

/// 把越界/非法字段拉回合法域(容忍手改过的文件)。
pub fn normalize(mut cfg: WorkspaceConfig) -> WorkspaceConfig {
    cfg.toolbar_columns = clamp_columns(cfg.toolbar_dock, cfg.toolbar_columns);
    cfg.schema_version = SCHEMA_VERSION;
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_picks_closest_edge_within_band() {
        // 窗口 1000×800,带 48
        assert_eq!(
            DockSide::nearest(10.0, 400.0, 1000.0, 800.0, 48.0),
            Some(DockSide::Left)
        );
        assert_eq!(
            DockSide::nearest(995.0, 400.0, 1000.0, 800.0, 48.0),
            Some(DockSide::Right)
        );
        assert_eq!(
            DockSide::nearest(500.0, 5.0, 1000.0, 800.0, 48.0),
            Some(DockSide::Top)
        );
        assert_eq!(
            DockSide::nearest(500.0, 795.0, 1000.0, 800.0, 48.0),
            Some(DockSide::Bottom)
        );
    }

    #[test]
    fn nearest_returns_none_in_the_middle() {
        // 松手落在中间 → 回弹(不做浮动,07-3 决策)
        assert_eq!(DockSide::nearest(500.0, 400.0, 1000.0, 800.0, 48.0), None);
    }

    #[test]
    fn nearest_prefers_truly_closest_at_corners() {
        // 左上角但更靠上边(到上边 6px,到左边 20px)
        assert_eq!(
            DockSide::nearest(20.0, 6.0, 1000.0, 800.0, 48.0),
            Some(DockSide::Top)
        );
        // 左上角但更靠左边
        assert_eq!(
            DockSide::nearest(6.0, 20.0, 1000.0, 800.0, 48.0),
            Some(DockSide::Left)
        );
    }

    #[test]
    fn toolbar_sizes_follow_side_and_columns() {
        assert_eq!(toolbar_size(DockSide::Left, 1).0, 60.0);
        assert_eq!(toolbar_size(DockSide::Right, 2).0, 88.0);
        assert!(toolbar_size(DockSide::Top, 2).1 > 0.0);
        assert_eq!(
            toolbar_size(DockSide::Bottom, 1).1,
            toolbar_size(DockSide::Top, 1).1
        );
    }

    #[test]
    fn columns_clamped_for_horizontal_docks() {
        assert_eq!(clamp_columns(DockSide::Top, 2), 1, "顶/底停靠只有单行");
        assert_eq!(clamp_columns(DockSide::Left, 5), 2, "列数上限 2");
        assert_eq!(clamp_columns(DockSide::Right, 0), 1, "列数下限 1");
    }

    #[test]
    fn corrupt_file_falls_back_with_warning() {
        let p = std::env::temp_dir().join(format!("vb-ws-bad-{}.json", std::process::id()));
        std::fs::write(&p, "{ 这不是 JSON").unwrap();
        let (cfg, warn) = load_from(&p);
        assert_eq!(cfg, WorkspaceConfig::default());
        assert!(warn.unwrap().contains("解析失败"), "损坏必须告警");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let p = std::env::temp_dir().join("vb-ws-does-not-exist-xyz.json");
        let _ = std::fs::remove_file(&p);
        let (cfg, warn) = load_from(&p);
        assert_eq!(cfg, WorkspaceConfig::default());
        assert!(warn.is_none(), "首次运行无文件不该告警");
    }

    #[test]
    fn save_then_load_roundtrip() {
        let p = std::env::temp_dir().join(format!("vb-ws-rt-{}.json", std::process::id()));
        let cfg = WorkspaceConfig {
            toolbar_dock: DockSide::Left,
            toolbar_columns: 2,
            panel_tab: 2,
            panel_order: vec![1, 0, 3, 2],
            ..WorkspaceConfig::default()
        };
        save_to(&p, &cfg).unwrap();
        let (back, warn) = load_from(&p);
        assert!(warn.is_none());
        assert_eq!(back, cfg);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn stale_schema_version_falls_back() {
        let p = std::env::temp_dir().join(format!("vb-ws-old-{}.json", std::process::id()));
        std::fs::write(&p, r#"{"schema_version":99,"toolbar_dock":"top"}"#).unwrap();
        let (cfg, warn) = load_from(&p);
        assert_eq!(cfg.toolbar_dock, DockSide::Bottom, "应回退默认");
        assert!(warn.unwrap().contains("版本"), "版本不符必须告警");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn normalize_pulls_illegal_columns_back() {
        let cfg = WorkspaceConfig {
            toolbar_dock: DockSide::Top,
            toolbar_columns: 2,
            ..WorkspaceConfig::default()
        };
        assert_eq!(normalize(cfg).toolbar_columns, 1);
    }
}
