//! 工具栏四向停靠(阶段 6 / 副文档 07-1)与布局持久化(07-2)。
//!
//! **V3 决策**:自研轻量停靠(`enum DockSide` + `egui::Panel` 分支),
//! **不引 `egui_dock`** —— 工具栏只需 4 个位置 + 吸附,不值得一个 UI 框架;
//! 也与 `design/14` F8 的精神一致。
//! **04-2 更新(W5/W8 决策)**:九个次级面板(字符/段落/外观/描边/渐变/
//! 透明度/颜色/变换/对齐)默认**停靠**进次级面板坞 Tab 组;浮窗作为
//! **受控选项**保留(默认关,自动排布防级联,见 `panel_dock` 模块)——
//! 其摆放(`PanelPlacement` / `sec_*` 字段)随 `workspace.json` 记忆。
//!
//! 本模块只放**纯数据与纯函数**(停靠几何、吸附判定、配置读写),
//! 渲染在 `toolbar.rs` / `panel_dock.rs`,`VellumApp` 只持有
//! [`WorkspaceConfig`] 的子集。

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

impl Default for DockSide {
    /// 默认停靠 = 底部(与 `WorkspaceConfig::default` 的 08-9 口径一致;
    /// `LayoutSnapshot` 的 serde 默认依赖此实现)。
    fn default() -> Self {
        DockSide::Bottom
    }
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

/// 当前 schema 版本。**加字段必须同时抬版本 + 写迁移**(07 §6 风险);
/// v1 → v2(04-2/04-4):新增次级面板摆放与 开发者统计/提示 开关,
/// 旧文件走 `migrate_v1` 静默补默认,**不回退**(工具栏/面板坞偏好保住)。
pub const SCHEMA_VERSION: u32 = 2;
/// v2 版本号(const 模式匹配用;与 [`SCHEMA_VERSION`] 同值)。
const SCHEMA_VERSION_V2: u64 = 2;
/// v1 版本号(迁移源)。
const SCHEMA_VERSION_V1: u64 = 1;

/// 次级面板摆放记忆(04-2-4):浮窗态 + 浮窗位置(像素)。
///
/// 停靠面板忽略 `pos`;九面板的下标约定在 `panel_dock::SecPanel::ALL`。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct PanelPlacement {
    /// 是否浮窗(默认 false = 停靠次级坞)。
    pub floating: bool,
    /// 浮窗位置([x, y] 像素;停靠态忽略)。
    #[serde(default)]
    pub pos: [f32; 2],
}

/// 用户自定义工作区预设(X-7「新建工作区」):命名 + 布局快照。
///
/// 只存**布局**(停靠位/面板坞/次级坞),不含首选项数据(主题/缩放等
/// 是全局偏好,不属于某个工作区);`layout` 字段全部 serde 默认,
/// 旧文件缺字段自动补齐。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspacePreset {
    /// 预设名(用户输入;应用/删除按名匹配)。
    pub name: String,
    /// 布局快照。
    pub layout: LayoutSnapshot,
}

/// 工作区布局快照(`WorkspacePreset.layout`):与 `WorkspaceConfig` 的
/// 布局字段一一对应,**不含**首选项与 `workspace_presets` 自身。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LayoutSnapshot {
    /// 工具栏停靠边。
    pub toolbar_dock: DockSide,
    /// 左/右停靠时的列数(1 或 2)。
    pub toolbar_columns: u8,
    /// 面板坞是否折叠。
    pub dock_collapsed: bool,
    /// 面板坞 Tab 顺序。
    pub panel_order: Vec<usize>,
    /// 面板坞当前 Tab。
    pub panel_tab: usize,
    /// 是否「Tab 隐藏所有面板」。
    pub panels_hidden: bool,
    /// 次级面板浮窗态(`panel_dock::SecPanel::ALL` 下标)。
    pub sec_floating: Vec<bool>,
    /// 次级面板浮窗位置。
    pub sec_pos: Vec<[f32; 2]>,
    /// 次级坞组显示顺序。
    pub sec_group_order: Vec<usize>,
    /// 次级坞当前组。
    pub sec_active_group: usize,
}

impl LayoutSnapshot {
    /// 归一化(容忍手改文件):数组长度/越界值拉回合法域(与
    /// [`normalize`] 同一口径)。
    pub fn normalized(mut self) -> Self {
        const SEC_PANELS: usize = 13;
        const SEC_GROUPS: usize = 7;
        self.toolbar_columns = clamp_columns(self.toolbar_dock, self.toolbar_columns);
        if self.panel_order.is_empty() {
            self.panel_order = vec![0, 1, 2, 3];
        }
        if self.panel_tab >= 4 {
            self.panel_tab = 0;
        }
        if self.sec_floating.len() < SEC_PANELS {
            self.sec_floating.resize(SEC_PANELS, false);
        }
        if self.sec_pos.len() < SEC_PANELS {
            self.sec_pos.resize(SEC_PANELS, [0.0, 0.0]);
        }
        {
            let seen: std::collections::HashSet<usize> =
                self.sec_group_order.iter().copied().collect();
            for g in 0..SEC_GROUPS {
                if !seen.contains(&g) {
                    self.sec_group_order.push(g);
                }
            }
            self.sec_group_order.truncate(SEC_GROUPS);
        }
        if self.sec_active_group >= SEC_GROUPS {
            self.sec_active_group = 0;
        }
        self
    }
}

/// 工作区布局(`workspace.json`)。
///
/// 覆盖 07-2-1 要求的四项:工具栏停靠位 + 单双列 + 面板坞展开态 + 面板 Tab 顺序,
/// 另存面板坞当前 Tab、「是否隐藏所有面板」、次级面板摆放(04-2)与
/// 开发者统计/提示开关(04-4)。
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
    /// 04-2:九个次级面板的浮窗态(下标 = `panel_dock::SecPanel::ALL` 顺序)。
    pub sec_floating: Vec<bool>,
    /// 04-2:九个次级面板的浮窗位置记忆(与 `sec_floating` 同下标)。
    pub sec_pos: Vec<[f32; 2]>,
    /// 04-2:次级坞组显示顺序(`panel_dock::SecGroup::ALL` 下标排列)。
    pub sec_group_order: Vec<usize>,
    /// 04-2:次级坞当前组(下标)。
    pub sec_active_group: usize,
    /// 04-4:开发者统计浮层开关(默认关 —— FPS/帧时间/节点/显卡只在此可见)。
    pub dev_stats: bool,
    /// 04-4:提示条开关(默认开;关闭后状态栏教学/操作提示不再显示)。
    pub hints: bool,
    /// 04-3:UI 缩放因子(乘在系统 DPI 之上;默认 1.0 = 跟随系统)。
    ///
    /// v2 内**追加字段**兼容旧文件:容器级 `#[serde(default)]` 让 v1 迁移与
    /// v2 旧文件都自动补 1.0,偏好(工具栏/面板摆放)原样保留,不抬版本。
    pub ui_scale: f32,
    /// 04-6:工具箱是否显示「未支持工具」(默认关;design/06 §二:
    /// 未支持工具默认隐藏,开启后置灰显示,点击给「计划于 vX」提示)。
    pub show_all_tools: bool,
    /// 07-A:自动保存间隔(秒;0 = 关;默认 60s,档位表见
    /// `crate::autosave::INTERVAL_STEPS`)。
    ///
    /// v2 内**追加字段**兼容旧文件(与 `ui_scale` 同法):容器级
    /// `#[serde(default)]` 自动补默认,不抬版本。
    pub autosave_interval_secs: u32,
    /// 05-2(X-5):铅笔保真度容差(px;默认 4)。v2 内追加字段,
    /// 与 `ui_scale` 同法自动补默认,不抬版本。
    pub pencil_fidelity: f64,
    // ── 阶段 5(05-4-A2:首选项九分类收编;全部 v2 内追加字段,
    //    与 `ui_scale` 同法 serde-default 兼容旧文件,不抬版本)──
    /// 主题(true = 深色;首选项「外观」页收编,启动时应用)。
    pub theme_dark: bool,
    /// 标尺默认显示(首选项「单位与标尺」页)。
    pub rulers_default: bool,
    /// 网格默认显示(首选项「参考线与网格」页)。
    pub grid_default: bool,
    /// 参考线默认显示(首选项「参考线与网格」页)。
    pub guides_default: bool,
    /// 智能参考线默认开(首选项「智能参考线」页)。
    pub smart_guides_default: bool,
    /// 网格基础间距 px(画布网格以此为基随缩放分级;默认 64 = 既有行为)。
    pub grid_size: f64,
    /// 新建文本默认字号 px(首选项「文字」页)。
    pub text_default_size: f64,
    /// 新建文本默认颜色(首选项「文字」页;CSS 色字符串)。
    pub text_default_color: String,
    /// 新建文本默认字体族(空 = 继承;首选项「文字」页)。
    pub text_default_family: String,
    /// 新画板默认预设(`panels::artboards::AB_PRESETS` 下标;首选项「画板」页)。
    pub artboard_preset: usize,
    /// 自动保存快照保留份数(1–3;首选项「数据」页收编既有滚动快照)。
    pub autosave_keep: u32,
    /// 05-7 i18n:界面语言代码("zh" / "en";首选项「常规」页)。
    /// v2 内追加字段,与 `ui_scale` 同法 serde-default 兼容旧文件,不抬版本。
    pub ui_lang: String,
    /// 用户自定义工作区预设(X-7「新建工作区」;最后写入胜,原子替换)。
    pub workspace_presets: Vec<WorkspacePreset>,
    // ── 第四轮 U-2/H-1(全部 v2 内追加字段,serde-default 兼容旧文件,
    //    与 `ui_scale` 同法自动补默认,不抬版本)──
    /// 主右坞宽度记忆(px;可拖 [`crate::app::panels`],拖完落盘)。
    pub dock_width: f32,
    /// 次级面板坞宽度记忆(px;范围 `panel_dock::SEC_DOCK_MIN..=MAX`)。
    pub sec_dock_width: f32,
    /// 次级坞用户折叠偏好(与主坞 `dock_collapsed` 同法;<1600 强制折叠
    /// 不回写本值)。
    pub sec_dock_collapsed: bool,
    /// H-1 动效总开关(true = 开;首选项「常规」页/视图菜单可关)。
    pub motion_enabled: bool,
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
            // 05-9:次级面板十二项(在 07-K 十一项上新增「时间轴」)、六组
            sec_floating: vec![false; 13],
            sec_pos: vec![[0.0, 0.0]; 13],
            sec_group_order: vec![0, 1, 2, 3, 4, 5, 6],
            sec_active_group: 0,
            dev_stats: false,
            hints: true,
            ui_scale: 1.0,
            show_all_tools: false,
            autosave_interval_secs: crate::autosave::DEFAULT_INTERVAL_SECS,
            pencil_fidelity: 4.0,
            // ── 阶段 5(05-4-A2:首选项九分类默认值)──
            theme_dark: true,
            rulers_default: true,
            grid_default: true,
            guides_default: true,
            smart_guides_default: true,
            grid_size: 64.0,
            text_default_size: 24.0,
            text_default_color: "#1a1a1a".into(),
            text_default_family: String::new(),
            // AB_PRESETS[1] = Web 1440×900(与新建画板的既有默认一致)
            artboard_preset: 1,
            autosave_keep: crate::autosave::MAX_SNAPSHOTS as u32,
            // 05-7:界面语言默认中文
            ui_lang: "zh".into(),
            workspace_presets: Vec::new(),
            // ── 第四轮 U-2/H-1 默认值 ──
            dock_width: vb_ui::theme::space::DOCK_WIDTH,
            sec_dock_width: super::panel_dock::SEC_DOCK_WIDTH,
            sec_dock_collapsed: false,
            motion_enabled: true,
        }
    }
}

/// 配置目录(**所有全局配置共用**:workspace.json / recent.json)。
///
/// 解析顺序:`%APPDATA%\VellumBench\`(Windows)/
/// `$XDG_CONFIG_HOME|$HOME/.config/vellum-bench/`(其它平台)。
/// 全部不可得 → `None`(调用方退化为纯内存,并如实提示)。
pub fn config_dir() -> Option<PathBuf> {
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
    Some(base.join("VellumBench"))
}

/// 配置文件路径(`workspace.json`)。
///
/// 解析顺序:`VB_WORKSPACE` 环境变量(测试/便携用)→ 配置目录(见
/// [`config_dir`])下的 `workspace.json`。
pub fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VB_WORKSPACE") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    Some(config_dir()?.join("workspace.json"))
}

/// 读取工作区配置。返回 `(配置, 告警)`。
///
/// **损坏不静默**(07-2-2):文件存在但解析失败 / 版本更新 → 回退默认,
/// 并把中文原因交回调用方(由 `VellumApp` 写进状态栏与日志)。
/// v1 → v2 走 `migrate_v1` 静默迁移(旧偏好全保留)。
pub fn load_from(path: &std::path::Path) -> (WorkspaceConfig, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return (WorkspaceConfig::default(), None), // 首次运行:无文件不是错误
    };
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            return (
                WorkspaceConfig::default(),
                Some(format!("workspace.json 解析失败,已回退默认布局:{e}")),
            );
        }
    };
    let version = value
        .get("schema_version")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::MAX);
    match version {
        // const 模式:SCHEMA_VERSION 是 const,u64 字面等值匹配
        SCHEMA_VERSION_V2 => match serde_json::from_value::<WorkspaceConfig>(value) {
            Ok(cfg) => (normalize(cfg), None),
            Err(e) => (
                WorkspaceConfig::default(),
                Some(format!("workspace.json 解析失败,已回退默认布局:{e}")),
            ),
        },
        // v1(04-2 之前的文件):补齐 sec_*/dev_stats/hints 默认值,偏好保留
        SCHEMA_VERSION_V1 => match migrate_v1(value) {
            Ok(cfg) => (normalize(cfg), None),
            Err(e) => (
                WorkspaceConfig::default(),
                Some(format!("workspace.json v1 迁移失败,已回退默认布局:{e}")),
            ),
        },
        _ => (
            WorkspaceConfig::default(),
            Some(format!(
                "workspace.json 版本 {version} 与当前 {SCHEMA_VERSION} 不符,已回退默认布局"
            )),
        ),
    }
}

/// v1 → v2 迁移(04-2):反序列化进带 `#[serde(default)]` 的当前结构,
/// 新字段自动补默认;再把版本号抬到当前。
fn migrate_v1(value: serde_json::Value) -> Result<WorkspaceConfig, String> {
    let mut cfg: WorkspaceConfig = serde_json::from_value(value).map_err(|e| e.to_string())?;
    cfg.schema_version = SCHEMA_VERSION;
    Ok(cfg)
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
    // 07-K:老文件(sec_* 数组 10 长度)静默扩位到 11(新面板默认停靠);
    // 05-9:11 → 12(时间轴面板)、组顺序 5 项 → 6 项:缺的组号按序补尾。
    const SEC_PANELS: usize = 13;
    const SEC_GROUPS: usize = 7;
    if cfg.sec_floating.len() < SEC_PANELS {
        cfg.sec_floating.resize(SEC_PANELS, false);
    }
    if cfg.sec_pos.len() < SEC_PANELS {
        cfg.sec_pos.resize(SEC_PANELS, [0.0, 0.0]);
    }
    {
        let seen: std::collections::HashSet<usize> = cfg.sec_group_order.iter().copied().collect();
        for g in 0..SEC_GROUPS {
            if !seen.contains(&g) {
                cfg.sec_group_order.push(g);
            }
        }
        cfg.sec_group_order.truncate(SEC_GROUPS);
    }
    // 04-3:UI 缩放因子夹回合理档位域(0.5×~3.0×;非法/手改值回 1.0)
    if !(0.5..=3.0).contains(&cfg.ui_scale) || !cfg.ui_scale.is_finite() {
        cfg.ui_scale = 1.0;
    }
    // 07-A:自动保存间隔必须落在档位表内(手改的怪值回默认 60s)
    if !crate::autosave::INTERVAL_STEPS.contains(&cfg.autosave_interval_secs) {
        cfg.autosave_interval_secs = crate::autosave::DEFAULT_INTERVAL_SECS;
    }
    // 05-2:铅笔保真度夹回 1..=20(design/06 §3.7「保真度参数 0–20px」)
    if !(1.0..=20.0).contains(&cfg.pencil_fidelity) || !cfg.pencil_fidelity.is_finite() {
        cfg.pencil_fidelity = 4.0;
    }
    // ── 阶段 5(05-4-A2:首选项新增字段越界拉回)──
    // 网格间距:2–256px(手改怪值回默认 64)
    if !(2.0..=256.0).contains(&cfg.grid_size) || !cfg.grid_size.is_finite() {
        cfg.grid_size = 64.0;
    }
    // 新建文本默认字号:4–200px
    if !(4.0..=200.0).contains(&cfg.text_default_size) || !cfg.text_default_size.is_finite() {
        cfg.text_default_size = 24.0;
    }
    if !cfg.text_default_color.starts_with('#') {
        cfg.text_default_color = "#1a1a1a".into();
    }
    // 新画板预设下标必须在 AB_PRESETS 表内(超界回 Web 1440×900)
    if cfg.artboard_preset >= super::panels::artboards::AB_PRESETS.len() {
        cfg.artboard_preset = 1;
    }
    // 快照保留数夹回 1..=MAX_SNAPSHOTS(滚动结构固定 3 槽)
    if cfg.autosave_keep == 0 || cfg.autosave_keep > crate::autosave::MAX_SNAPSHOTS as u32 {
        cfg.autosave_keep = crate::autosave::MAX_SNAPSHOTS as u32;
    }
    // 工作区预设:布局逐份归一化,预设名去空白;同名去重(保留首份)
    for p in &mut cfg.workspace_presets {
        p.layout = std::mem::take(&mut p.layout).normalized();
        p.name = p.name.trim().to_string();
    }
    cfg.workspace_presets.retain(|p| !p.name.is_empty());
    cfg.workspace_presets.dedup_by(|a, b| a.name == b.name);
    // ── 第四轮 U-2:坞宽记忆夹回合法拖拽域(手改怪值回默认)──
    if !(vb_ui::dock::DOCK_MIN..=vb_ui::dock::DOCK_MAX).contains(&cfg.dock_width) {
        cfg.dock_width = vb_ui::theme::space::DOCK_WIDTH;
    }
    if !(super::panel_dock::SEC_DOCK_MIN..=super::panel_dock::SEC_DOCK_MAX)
        .contains(&cfg.sec_dock_width)
    {
        cfg.sec_dock_width = crate::app::panel_dock::SEC_DOCK_WIDTH;
    }
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

    /// 04-2:v1 文件静默迁移 —— 旧偏好保留,新字段补默认,不告警。
    #[test]
    fn v1_file_migrates_silently_with_prefs_kept() {
        let p = std::env::temp_dir().join(format!("vb-ws-v1-{}.json", std::process::id()));
        // v1 时代的真实形状:没有 sec_* / dev_stats / hints 字段
        std::fs::write(
            &p,
            r#"{"schema_version":1,"toolbar_dock":"left","toolbar_columns":2,
                "dock_collapsed":false,"panel_order":[1,0,3,2],"panel_tab":2,
                "panels_hidden":false}"#,
        )
        .unwrap();
        let (cfg, warn) = load_from(&p);
        assert!(warn.is_none(), "v1 迁移不该告警:{warn:?}");
        assert_eq!(cfg.schema_version, SCHEMA_VERSION);
        assert_eq!(cfg.toolbar_dock, DockSide::Left, "旧偏好必须保留");
        assert_eq!(cfg.panel_order, vec![1, 0, 3, 2]);
        assert!(!cfg.sec_floating.iter().any(|&f| f), "新字段补默认:全停靠");
        assert_eq!(
            cfg.sec_floating.len(),
            13,
            "05-10 起为十三面板(旧文件自动扩位)"
        );
        assert_eq!(
            cfg.sec_group_order.len(),
            7,
            "05-10 起为七组(旧文件自动补组号)"
        );
        assert!(!cfg.dev_stats, "开发者统计默认关(04-4)");
        assert!(cfg.hints, "提示条默认开(04-4)");
        assert_eq!(
            cfg.autosave_interval_secs,
            crate::autosave::DEFAULT_INTERVAL_SECS,
            "07-A:旧文件补自动保存默认档"
        );
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

    /// 04-3:UI 缩放因子 —— v2 旧文件(无该字段)补默认 1.0,偏好不丢;
    /// 非法值(手改)夹回 1.0。
    #[test]
    fn ui_scale_is_backward_compatible_and_clamped() {
        let p = std::env::temp_dir().join(format!("vb-ws-scale-{}.json", std::process::id()));
        // 合法 v2 旧文件(无新字段)
        std::fs::write(
            &p,
            r#"{"schema_version":2,"toolbar_dock":"left","toolbar_columns":2,
                "dock_collapsed":false,"panel_order":[0,1,2,3],"panel_tab":0,
                "panels_hidden":false}"#,
        )
        .unwrap();
        let (cfg, warn) = load_from(&p);
        assert!(warn.is_none());
        assert_eq!(cfg.ui_scale, 1.0, "旧 v2 文件缺字段应补 1.0");
        assert!(!cfg.show_all_tools, "未支持工具默认隐藏(04-6)");
        assert_eq!(cfg.toolbar_dock, DockSide::Left, "旧偏好保留");
        // 手改的非法值
        std::fs::write(
            &p,
            r#"{"schema_version":2,"toolbar_dock":"top","ui_scale":99}"#,
        )
        .unwrap();
        let (cfg, _) = load_from(&p);
        assert_eq!(normalize(cfg).ui_scale, 1.0, "非法缩放必须回 1.0");
        let _ = std::fs::remove_file(&p);
    }

    /// 05-4-A2:首选项新增字段 —— 旧 v2 文件(无这些字段)自动补默认,
    /// 既有偏好(停靠位)保留;非法值(手改)逐项拉回合法域。
    #[test]
    fn prefs_fields_are_backward_compatible_and_clamped() {
        let p = std::env::temp_dir().join(format!("vb-ws-prefs-{}.json", std::process::id()));
        // 旧 v2 文件(只有阶段 6/7 时代的字段)
        std::fs::write(
            &p,
            r#"{"schema_version":2,"toolbar_dock":"right","toolbar_columns":1,
                "dock_collapsed":false,"panel_order":[0,1,2,3],"panel_tab":0,
                "panels_hidden":false}"#,
        )
        .unwrap();
        let (cfg, warn) = load_from(&p);
        assert!(warn.is_none(), "旧文件补默认不该告警:{warn:?}");
        assert!(cfg.theme_dark, "默认深色主题");
        assert!(cfg.rulers_default && cfg.grid_default && cfg.guides_default);
        assert!(cfg.smart_guides_default);
        assert_eq!(cfg.grid_size, 64.0, "网格间距默认 64(= 既有画布行为)");
        assert_eq!(cfg.text_default_size, 24.0);
        assert_eq!(cfg.artboard_preset, 1, "默认新画板预设 = Web 1440×900");
        assert_eq!(cfg.autosave_keep, 3, "快照保留数默认 = 滚动快照上限");
        assert!(cfg.workspace_presets.is_empty());
        assert_eq!(cfg.toolbar_dock, DockSide::Right, "既有偏好保留");

        // 手改的非法值逐项拉回
        std::fs::write(
            &p,
            r#"{"schema_version":2,"toolbar_dock":"left","grid_size":9999,
                "text_default_size":0.5,"artboard_preset":99,"autosave_keep":7,
                "text_default_color":"red","workspace_presets":[
                    {"name":" 我的名 ","layout":{"toolbar_dock":"top","panel_tab":9}},
                    {"name":"我的名","layout":{}}]}"#,
        )
        .unwrap();
        let (cfg, warn) = load_from(&p);
        assert!(warn.is_none(), "normalize 兜底,不该升级为告警:{warn:?}");
        let cfg = normalize(cfg);
        assert_eq!(cfg.grid_size, 64.0, "网格间距越界回默认");
        assert_eq!(cfg.text_default_size, 24.0);
        assert_eq!(cfg.artboard_preset, 1);
        assert_eq!(cfg.autosave_keep, 3);
        assert_eq!(cfg.text_default_color, "#1a1a1a");
        assert_eq!(cfg.workspace_presets.len(), 1, "同名预设去重 + 空白名清理");
        assert_eq!(cfg.workspace_presets[0].name, "我的名");
        assert_eq!(
            cfg.workspace_presets[0].layout.toolbar_dock,
            DockSide::Top,
            "预设布局保留"
        );
        assert_eq!(
            cfg.workspace_presets[0].layout.panel_tab, 0,
            "预设布局越界值归一化"
        );
        let _ = std::fs::remove_file(&p);
    }

    /// X-7:工作区预设 save/load 往返(布局快照逐字段还原)。
    #[test]
    fn workspace_preset_roundtrips() {
        let p = std::env::temp_dir().join(format!("vb-ws-preset-{}.json", std::process::id()));
        let mut cfg = WorkspaceConfig::default();
        cfg.workspace_presets.push(WorkspacePreset {
            name: "我的工作区".into(),
            layout: LayoutSnapshot {
                // 注意:Top 停靠只有单行(normalize 会把列数拉回 1),
                // 用 Left+2 才能验证列数字段原样往返
                toolbar_dock: DockSide::Left,
                toolbar_columns: 2,
                dock_collapsed: true,
                panel_order: vec![2, 1, 0, 3],
                panel_tab: 2,
                panels_hidden: false,
                sec_floating: vec![true; 13],
                sec_pos: vec![[10.0, 20.0]; 13],
                sec_group_order: vec![0, 1, 2, 3, 4, 5, 6],
                sec_active_group: 3,
            },
        });
        save_to(&p, &cfg).unwrap();
        let (back, warn) = load_from(&p);
        assert!(warn.is_none());
        assert_eq!(back.workspace_presets, cfg.workspace_presets, "逐字段还原");
        let _ = std::fs::remove_file(&p);
    }

    /// 第四轮 U-2/H-1:坞宽记忆 / 次级坞折叠 / 动效开关 —— 旧 v2 文件
    /// (无这些字段)serde-default 自动补默认,偏好不丢、不抬版本;
    /// 手改的怪值由 normalize 夹回合法域。
    #[test]
    fn dock_width_and_motion_fields_are_backward_compatible() {
        let p = std::env::temp_dir().join(format!("vb-ws-u2-{}.json", std::process::id()));
        // 旧 v2 文件形状(第四轮之前):无 dock_width / sec_dock_* / motion_enabled
        std::fs::write(
            &p,
            r#"{"schema_version":2,"toolbar_dock":"left","toolbar_columns":1,
                "dock_collapsed":false,"panel_order":[0,1,2,3],"panel_tab":0,
                "panels_hidden":false,"ui_lang":"zh"}"#,
        )
        .unwrap();
        let (cfg, warn) = load_from(&p);
        assert!(warn.is_none(), "旧文件补默认不该告警:{warn:?}");
        assert!(
            (cfg.dock_width - vb_ui::theme::space::DOCK_WIDTH).abs() < f32::EPSILON,
            "主坞宽缺省 = 280"
        );
        assert!(
            (cfg.sec_dock_width - crate::app::panel_dock::SEC_DOCK_WIDTH).abs() < f32::EPSILON,
            "次级坞宽缺省 = 300"
        );
        assert!(!cfg.sec_dock_collapsed, "次级坞默认展开");
        assert!(cfg.motion_enabled, "动效默认开(H-1)");
        // 写回再读:新字段逐值往返
        let mut cfg = cfg;
        cfg.dock_width = 340.0;
        cfg.sec_dock_width = 260.0;
        cfg.sec_dock_collapsed = true;
        cfg.motion_enabled = false;
        save_to(&p, &cfg).unwrap();
        let (back, warn) = load_from(&p);
        assert!(warn.is_none());
        assert!((back.dock_width - 340.0).abs() < f32::EPSILON);
        assert!((back.sec_dock_width - 260.0).abs() < f32::EPSILON);
        assert!(back.sec_dock_collapsed);
        assert!(!back.motion_enabled);
        // 手改怪值夹回
        std::fs::write(
            &p,
            r#"{"schema_version":2,"toolbar_dock":"bottom","dock_width":9999,
                "sec_dock_width":-5,"panel_order":[0,1,2,3],"panel_tab":0,
                "panels_hidden":false}"#,
        )
        .unwrap();
        let (cfg, warn) = load_from(&p);
        assert!(warn.is_none(), "normalize 兜底不升级为告警:{warn:?}");
        let cfg = normalize(cfg);
        assert_eq!(
            cfg.dock_width,
            vb_ui::theme::space::DOCK_WIDTH,
            "越界主坞宽回默认"
        );
        assert_eq!(cfg.sec_dock_width, crate::app::panel_dock::SEC_DOCK_WIDTH);
        let _ = std::fs::remove_file(&p);
    }
}
