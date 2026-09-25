//! 次级面板坞(04-2 / 副文档 04 P0-⑥)。
//!
//! **缺陷**:字符/段落/外观/描边/渐变/透明度/颜色/变换/对齐 九个面板原为
//! 独立浮窗,连续打开时**级联堆叠**遮挡画布左上 ~1/4(P0-⑥ 实证
//! `d-char.png`),违反 ADR-0024「面板保持停靠」精神。
//!
//! **W5/W8 决策**(副文档 04 §3):**自研 Tab 停靠,不引 `egui_dock`** ——
//! 需要的只是「停靠 + Tab + 记忆」三件事:
//! 1. 九个面板归入四个 **Tab 组**:「字符|段落」「外观|描边|透明度」
//!    「变换|对齐」「渐变|颜色」;默认**停靠**进右坞旁的次级坞,
//!    同一时刻只显示一组 → 画布不再被遮挡;
//! 2. **保留浮窗能力但默认关闭**;浮窗位置由 [`floating_origins`]
//!    自动排布(格子化,两两不相交)—— 级联在结构上不可能复现;
//! 3. 每面板的「是否浮窗 + 浮窗位置」与组顺序 / 当前组写入
//!    `workspace.json`(schema v2,见 `dock_layout::PanelPlacement`)。
//!
//! 阶段 7 扩员:07-D 增「历史」入变换组;07-K 增「资产」独立成组;
//! 05-9 增「时间轴」独立成组(原六组十二面板,老文件的短数组由
//! `dock_layout::normalize` 静默补位)。
//! 05-10(09-J)再增「插件」独立成组:Running 插件的注册面板渲染进这里
//! (受控 UI 描述,见 `app/plugins.rs`)。
//!
//! 本模块持有:`SecPanel` / `SecGroup` 语义模型、坞与浮窗渲染、
//! 防级联纯函数 [`floating_origins`] 与 **04-2-6 度量门禁**单测。
//! 各面板的「开关」真值仍在 `VellumApp` 的 `*_open` 布尔
//! (菜单命令 / 快捷键 / 能力台账都指向它们),本模块只做投影与摆放。

use egui::{Color32, Rect};
use vb_ui::components::{caption, icon_button, PanelTabs};
use vb_ui::icons;
use vb_ui::icons::Name;
use vb_ui::theme;

use super::dock_layout;
use super::VellumApp;

/// 本地两色插值(悬停过渡;与 `vb_ui::components` 内部实现同式)。
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

/// Tab 组 → 折叠图标条上的代表图标(U-2;全部复用既有语义图标,
/// 只有插件组新 Lucide 字形 `puzzle`,`all_icons_resolve` 把关)。
fn sec_group_icon(g: SecGroup) -> Name {
    match g {
        SecGroup::Text => Name::ToolText,
        SecGroup::Looks => Name::PanelProperties,
        SecGroup::Xform => Name::ToolRotate,
        SecGroup::Paint => Name::PanelTokens,
        SecGroup::Assets => Name::KindImage,
        SecGroup::Timeline => Name::Play,
        SecGroup::Plugins => Name::Puzzle,
    }
}

/// 次级面板坞默认宽(可拖 [`SEC_DOCK_MIN`]–[`SEC_DOCK_MAX`])。
pub const SEC_DOCK_WIDTH: f32 = 300.0;
/// 次级面板坞最小宽。
pub const SEC_DOCK_MIN: f32 = 240.0;
/// 次级面板坞最大宽。
pub const SEC_DOCK_MAX: f32 = 380.0;
/// 低于此窗口宽度次级坞强制折叠为图标条(U-2:两坞同开挤压画布;
/// 主右坞的 1200 阈值在 `vb_ui::dock::COLLAPSE_BELOW`,两者独立生效)。
pub const SEC_COLLAPSE_BELOW: f32 = 1600.0;
/// 折叠态图标条宽度(与主坞 `vb_ui::dock::ICON_RAIL_WIDTH` 一致)。
pub const SEC_RAIL_WIDTH: f32 = 40.0;

/// 次级坞此刻应否折叠(纯函数;U-2,规则与主坞 `vb_ui::dock` 同构)。
///
/// | 窗口宽 | 用户偏好(展开) | 结果 |
/// |---|---|---|
/// | < 1600 | 任意 | 强制折叠(图标条;不回写用户偏好) |
/// | ≥ 1600 | 未折叠 | 展开 |
/// | ≥ 1600 | 折叠 | 折叠(用户折的尊重) |
pub fn sec_should_collapse(window_width: f32, user_collapsed: bool) -> bool {
    window_width < SEC_COLLAPSE_BELOW || user_collapsed
}

/// 窄窗(强制折叠)下图标条点击是否允许展开(<1600 只切组不展开)。
pub fn sec_rail_can_expand(window_width: f32) -> bool {
    window_width >= SEC_COLLAPSE_BELOW
}

/// 次级坞宽度钳制(可拖 240–380;默认 [`SEC_DOCK_WIDTH`] = 300)。
pub fn clamp_sec_width(w: f32) -> f32 {
    w.clamp(SEC_DOCK_MIN, SEC_DOCK_MAX)
}

/// 防级联格子的步进(浮窗自动排布用):横向格子宽。
const CELL_W: f32 = 340.0;
/// 防级联格子的步进:纵向格子高(面板 default_width ≤ 324,格子内
/// 左上角对齐;超高面板向下延伸,但**起点永不重叠** → 无级联遮挡)。
const CELL_H: f32 = 300.0;
/// 格子之间的空隙(保证相邻格子矩形**严格不相交**,含边界)。
const CELL_GAP: f32 = 12.0;
/// 浮窗排布距视口边缘的内边距。
const MARGIN: f32 = 24.0;

/// 可停靠面板(语义 id;持久化下标 = [`SecPanel::ALL`] 顺序)。
///
/// 阶段 7(07-D)新增 `History`:撤销历史面板,归入「变换」组
/// (定夺允许"变换组或独立历史组";入组零迁移成本,组数保持 4)。
/// 阶段 7b(07-K)新增 `Assets`:资产面板,独立「资产」组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecPanel {
    Char,
    Para,
    Appearance,
    Stroke,
    Gradient,
    Opacity,
    Color,
    Transform,
    Align,
    /// 撤销历史(阶段 7 / 07-D;真值布尔 = `VellumApp.history_open`)。
    History,
    /// 资产(阶段 7b / 07-K;真值布尔 = `VellumApp.assets_open`)。
    Assets,
    /// 动效时间轴(阶段 5 / 05-9,09-I;真值布尔 = `VellumApp.timeline_open`)。
    Timeline,
    /// 插件(阶段 5 / 05-10,09-J;真值布尔 = `VellumApp.plugins_panel_open`)。
    Plugins,
}

impl SecPanel {
    /// 全部面板(顺序 = `workspace.json` `sec_floating` / `sec_pos` 下标)。
    pub const ALL: [SecPanel; 13] = [
        SecPanel::Char,
        SecPanel::Para,
        SecPanel::Appearance,
        SecPanel::Stroke,
        SecPanel::Gradient,
        SecPanel::Opacity,
        SecPanel::Color,
        SecPanel::Transform,
        SecPanel::Align,
        SecPanel::History,
        SecPanel::Assets,
        SecPanel::Timeline,
        SecPanel::Plugins,
    ];

    /// 面板标题(Tab 文本与浮窗标题共用)。
    pub fn label(self) -> &'static str {
        match self {
            SecPanel::Char => "字符",
            SecPanel::Para => "段落",
            SecPanel::Appearance => "外观",
            SecPanel::Stroke => "描边",
            SecPanel::Gradient => "渐变",
            SecPanel::Opacity => "透明度",
            SecPanel::Color => "颜色",
            SecPanel::Transform => "变换",
            SecPanel::Align => "对齐",
            SecPanel::History => "历史",
            SecPanel::Assets => "资产",
            SecPanel::Timeline => "时间轴",
            SecPanel::Plugins => "插件",
        }
    }

    /// 所属 Tab 组(04-2-1 分组;07-D 历史入「变换」组;07-K 资产独立成组)。
    pub fn group(self) -> SecGroup {
        match self {
            SecPanel::Char | SecPanel::Para => SecGroup::Text,
            SecPanel::Appearance | SecPanel::Stroke | SecPanel::Opacity => SecGroup::Looks,
            SecPanel::Transform | SecPanel::Align | SecPanel::History => SecGroup::Xform,
            SecPanel::Gradient | SecPanel::Color => SecGroup::Paint,
            SecPanel::Assets => SecGroup::Assets,
            SecPanel::Timeline => SecGroup::Timeline,
            SecPanel::Plugins => SecGroup::Plugins,
        }
    }

    /// 持久化下标(`SecPanel::ALL` 中的位置)。
    pub fn index(self) -> usize {
        SecPanel::ALL
            .iter()
            .position(|&p| p == self)
            .expect("SecPanel::ALL 必须包含全部面板")
    }
}

/// 五个 Tab 组(04-2-1:文字 / 外观 / 变换 / 颜色;07-K 增:资产)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecGroup {
    /// 文字组:字符 | 段落。
    Text,
    /// 外观组:外观 | 描边 | 透明度。
    Looks,
    /// 变换组:变换 | 对齐 | 历史。
    Xform,
    /// 颜色组:渐变 | 颜色。
    Paint,
    /// 资产组:资产(07-K;与文档数据强相关,不混入外观/变换语义组)。
    Assets,
    /// 时间轴组:时间轴(05-9,09-I;播放控制 + 关键帧轨道,独立成组)。
    Timeline,
    /// 插件组:插件(05-10,09-J;插件注册的受控面板,独立成组)。
    Plugins,
}

impl SecGroup {
    /// 全部组(默认顺序;用户可重排,持久化为 `sec_group_order`)。
    pub const ALL: [SecGroup; 7] = [
        SecGroup::Text,
        SecGroup::Looks,
        SecGroup::Xform,
        SecGroup::Paint,
        SecGroup::Assets,
        SecGroup::Timeline,
        SecGroup::Plugins,
    ];

    /// 组标题。
    pub fn label(self) -> &'static str {
        match self {
            SecGroup::Text => "文字",
            SecGroup::Looks => "外观",
            SecGroup::Xform => "变换",
            SecGroup::Paint => "颜色",
            SecGroup::Assets => "资产",
            SecGroup::Timeline => "时间轴",
            SecGroup::Plugins => "插件",
        }
    }

    /// 组成员(固定顺序;07-D 历史入「变换」组;07-K 资产独立成组)。
    pub fn panels(self) -> &'static [SecPanel] {
        match self {
            SecGroup::Text => &[SecPanel::Char, SecPanel::Para],
            SecGroup::Looks => &[SecPanel::Appearance, SecPanel::Stroke, SecPanel::Opacity],
            SecGroup::Xform => &[SecPanel::Transform, SecPanel::Align, SecPanel::History],
            SecGroup::Paint => &[SecPanel::Gradient, SecPanel::Color],
            SecGroup::Assets => &[SecPanel::Assets],
            SecGroup::Timeline => &[SecPanel::Timeline],
            SecGroup::Plugins => &[SecPanel::Plugins],
        }
    }

    /// 持久化下标(`SecGroup::ALL` 中的位置)。
    pub fn index(self) -> usize {
        SecGroup::ALL
            .iter()
            .position(|&g| g == self)
            .expect("SecGroup::ALL 必须包含全部组")
    }

    /// 按持久化下标取组(越界回退第一组)。
    pub fn from_index(i: usize) -> SecGroup {
        SecGroup::ALL.get(i).copied().unwrap_or(SecGroup::Text)
    }
}

/// 次级面板坞会话态(`VellumApp.sec` 字段)。
///
/// 持久化时投影到 `WorkspaceConfig` 的 `sec_*` 字段(见 `dock_layout`);
/// `pos_memory` 只标记「该浮窗位置已从真实矩形读回过」,不落盘 ——
/// egui 的 `default_position` 仅首帧生效,首帧之后把真实矩形写进
/// `pos`,重启即还原用户拖过的位置;没拖过则走自动排布格子。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SecDockState {
    /// 当前组(持久化下标,经 `SecGroup::from_index` 解释)。
    pub active_group: usize,
    /// 当前选中的面板(组内;None = 回退组内第一个开着的)。
    pub active: Option<SecPanel>,
    /// 组显示顺序(`SecGroup::ALL` 下标的排列;右键 Tab 可重排)。
    pub group_order: [usize; 7],
    /// 各面板是否浮窗(下标 = `SecPanel::ALL` 顺序;05-10 起为 13 项)。
    pub floating: [bool; 13],
    /// 各面板浮窗位置记忆(像素;仅浮窗态有意义)。
    pub pos: [[f32; 2]; 13],
    /// 浮窗位置是否已从真实矩形读回(会话哨兵,不持久化)。
    pub pos_memory: [bool; 13],
}

impl Default for SecDockState {
    fn default() -> Self {
        Self {
            active_group: 0,
            active: None,
            group_order: [0, 1, 2, 3, 4, 5, 6],
            floating: [false; 13],
            pos: [[0.0; 2]; 13],
            pos_memory: [false; 13],
        }
    }
}

impl SecDockState {
    /// 从工作区配置还原(越界/缺字段一律回退默认;容忍手改文件)。
    pub fn from_config(cfg: &dock_layout::WorkspaceConfig) -> Self {
        let mut s = Self {
            active_group: cfg.sec_active_group.min(SecGroup::ALL.len() - 1),
            ..Self::default()
        };
        // 组顺序:合法排列才采用,否则保默认(缺位/越界由调用侧 normalize 补)
        let mut order = [0usize; 7];
        for (i, &v) in cfg.sec_group_order.iter().take(7).enumerate() {
            order[i] = v.min(6);
        }
        if order.iter().collect::<std::collections::HashSet<_>>().len() == 7 {
            s.group_order = order;
        }
        for (i, &f) in cfg.sec_floating.iter().take(13).enumerate() {
            s.floating[i] = f;
        }
        for (i, &p) in cfg.sec_pos.iter().take(13).enumerate() {
            s.pos[i] = p;
            s.pos_memory[i] = p != [0.0, 0.0]; // 有非零记忆位 → 直接当已读回
        }
        s
    }
}

/// 防级联浮窗自动排布(纯函数;04-2-3 / 04-2-6 门禁测试直接打这里)。
///
/// 第 `n` 个浮窗(按「开着的浮窗」的稳定顺序计数)落在视口内的一个
/// **专属格子**上:行优先、每行 `cols` 格、超出视口高度则不折行(
/// 保持靠上,让用户显式选择)。同一格子永不分给两个面板 → 两两
/// 格子矩形**不相交**;起点全部夹在视口内 → 不会出现越界窗口。
pub fn floating_origins(n: usize, viewport: Rect) -> Vec<egui::Pos2> {
    let avail_w = (viewport.width() - MARGIN * 2.0).max(CELL_W);
    let cols = ((avail_w / CELL_W).floor() as usize).clamp(1, 8);
    let y0 = viewport.top() + MARGIN + theme::space::CONTROL_BAR_HEIGHT;
    (0..n)
        .map(|i| {
            let col = i % cols;
            let row = i / cols;
            let x = (viewport.left() + MARGIN + col as f32 * (CELL_W + CELL_GAP))
                .min(viewport.right() - CELL_W - 4.0);
            let y =
                (y0 + row as f32 * (CELL_H + CELL_GAP)).min(viewport.bottom() - CELL_H.min(160.0));
            egui::pos2(x.max(viewport.left() + MARGIN), y.max(y0))
        })
        .collect()
}

impl VellumApp {
    // ---------- 开关投影(真值在九个 `*_open` 布尔) ----------

    /// 面板是否打开(读九个真值布尔)。
    pub(crate) fn sec_is_open(&self, p: SecPanel) -> bool {
        match p {
            SecPanel::Char => self.char_panel_open,
            SecPanel::Para => self.para_panel_open,
            SecPanel::Appearance => self.appearance_panel_open,
            SecPanel::Stroke => self.stroke_panel_open,
            SecPanel::Gradient => self.gradient_panel_open,
            SecPanel::Opacity => self.opacity_panel_open,
            SecPanel::Color => self.color_panel_open,
            SecPanel::Transform => self.transform_panel_open,
            SecPanel::Align => self.align_panel_open,
            SecPanel::History => self.history_open,
            SecPanel::Assets => self.assets_open,
            SecPanel::Timeline => self.timeline_open,
            SecPanel::Plugins => self.plugins_panel_open,
        }
    }

    /// 写面板开关(浮窗 × / 坞内关闭按钮 / 工作区预设共用)。
    pub(crate) fn sec_set_open(&mut self, p: SecPanel, open: bool) {
        match p {
            SecPanel::Char => self.char_panel_open = open,
            SecPanel::Para => self.para_panel_open = open,
            SecPanel::Appearance => self.appearance_panel_open = open,
            SecPanel::Stroke => self.stroke_panel_open = open,
            SecPanel::Gradient => self.gradient_panel_open = open,
            SecPanel::Opacity => self.opacity_panel_open = open,
            SecPanel::Color => self.color_panel_open = open,
            SecPanel::Transform => self.transform_panel_open = open,
            SecPanel::Align => self.align_panel_open = open,
            SecPanel::History => self.history_open = open,
            SecPanel::Assets => self.assets_open = open,
            SecPanel::Timeline => self.timeline_open = open,
            SecPanel::Plugins => self.plugins_panel_open = open,
        }
    }

    /// 面板被「显式打开」后的可见反馈:切到所在组并选中该面板;
    /// 若所有面板正被 Tab 隐藏,先恢复(04-2-5:打开必须有可见反馈)。
    pub(crate) fn sec_focus(&mut self, p: SecPanel) {
        if self.sec_is_open(p) {
            self.panels_hidden = false;
            self.sec.active_group = p.group().index();
            self.sec.active = Some(p);
        }
    }

    /// 面板是否浮窗(持久化摆放)。
    pub(crate) fn sec_is_floating(&self, p: SecPanel) -> bool {
        self.sec.floating[p.index()]
    }

    /// 当前面板选区:活跃面板被关掉时回退到同组第一个开着的面板。
    fn sec_effective(&self) -> Option<SecPanel> {
        let g = SecGroup::from_index(self.sec.active_group);
        if let Some(p) = self.sec.active {
            if self.sec_is_open(p) && p.group() == g {
                return Some(p);
            }
        }
        g.panels().iter().copied().find(|&p| self.sec_is_open(p))
    }

    /// 当前是否处于浮窗态的面板个数 ≥ 1(坞显隐判定用:全部浮窗化时
    /// 不再占布局)。
    fn has_docked_panel(&self) -> bool {
        SecPanel::ALL
            .iter()
            .copied()
            .any(|p| self.sec_is_open(p) && !self.sec_is_floating(p))
    }

    // ---------- 主循环装配(app.rs `ui()` 调用) ----------

    /// 十三面板统一入口:停靠次级坞(窄窗折叠为图标条,U-2)/ 受控浮窗。
    pub(crate) fn show_secondary_panels(&mut self, ui: &mut egui::Ui) {
        if self.panels_hidden {
            return;
        }
        // 停靠坞:至少有一个面板处于停靠态才占布局(不占画布);
        // 折叠判定 = 「窗宽 <1600 强制 + 用户偏好」的纯函数(规则表见
        // [`sec_should_collapse`],与主右坞 `vb_ui::dock` 同构)。
        if self.has_docked_panel() {
            let width = ui.ctx().viewport_rect().width();
            if sec_should_collapse(width, self.sec_dock_collapsed) {
                self.sec_dock_rail(ui, width);
            } else {
                let r = egui::Panel::right("sec_dock")
                    .default_size(clamp_sec_width(self.sec_dock_width))
                    .size_range(SEC_DOCK_MIN..=SEC_DOCK_MAX)
                    .resizable(true)
                    .frame(egui::Frame::new().fill(theme::tokens(ui.ctx()).bg_panel))
                    .show(ui, |ui| {
                        self.sec_dock_body(ui);
                    });
                // U-2:把 egui 会话内维护的实际宽度读回记忆位;拖动结束后
                // `workspace_dirty` 落盘(与主右坞同一套写通路径)。
                let w = r.response.rect.width();
                if (w - self.sec_dock_width).abs() > 0.5 {
                    self.sec_dock_width = clamp_sec_width(w);
                }
            }
        }
        // 浮窗(受控,默认关闭;画在上层,不占 Panel 布局)
        let floating: Vec<SecPanel> = SecPanel::ALL
            .iter()
            .copied()
            .filter(|&p| self.sec_is_open(p) && self.sec_is_floating(p))
            .collect();
        let viewport = ui.ctx().viewport_rect();
        let origins = floating_origins(floating.len(), viewport);
        for (n, &p) in floating.iter().enumerate() {
            self.show_sec_floating(ui, p, origins[n]);
        }
    }

    /// 次级坞折叠态:40px 图标条(每 Tab 组一个图标;U-2)。
    ///
    /// 与主坞 `dock_rail` 同一套交互:<1600 强制折叠窗口下点击只切组
    /// 不展开;≥1600 点击展开并切到该组。
    fn sec_dock_rail(&mut self, ui: &mut egui::Ui, viewport_width: f32) {
        let t = theme::tokens(ui.ctx());
        egui::Panel::right("sec_dock_rail")
            .exact_size(SEC_RAIL_WIDTH)
            .resizable(false)
            .frame(egui::Frame::new().fill(t.bg_panel))
            .show(ui, |ui| {
                ui.add_space(theme::space::S2);
                let can_expand = sec_rail_can_expand(viewport_width);
                if icon_button(ui, Name::Expanded, "展开次级坞").clicked() && can_expand {
                    self.sec_dock_collapsed = false;
                }
                ui.separator();
                // 用户排的组序渲染;每组的代表图标见 [`sec_group_icon`]。
                for &gi in &self.sec.group_order {
                    let g = SecGroup::from_index(gi);
                    let selected = self.sec.active_group == gi;
                    let has_open = g.panels().iter().any(|&p| self.sec_is_open(p));
                    let hint = if can_expand {
                        format!("{}(点击展开并切换)", g.label())
                    } else {
                        format!("{}(窗口过窄,仅切换)", g.label())
                    };
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::Vec2::splat(theme::space::ROW_HEIGHT),
                        egui::Sense::click(),
                    );
                    let hover_t = ui.ctx().animate_bool_with_time(
                        ui.id().with(("vbsecrail", gi)),
                        resp.hovered(),
                        theme::anim_time(ui.ctx(), theme::motion::HOVER),
                    );
                    let fill = if selected && has_open {
                        t.accent_dim
                    } else {
                        blend(Color32::TRANSPARENT, t.bg_hover, hover_t)
                    };
                    ui.painter().rect_filled(rect, theme::radius::md(), fill);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        sec_group_icon(g).glyph().to_string(),
                        icons::font(16.0),
                        if selected && has_open {
                            t.accent
                        } else {
                            t.text_2
                        },
                    );
                    if resp.clicked() {
                        self.sec.active_group = gi;
                        self.sec.active = None; // 回退组内第一个开着的面板
                        if can_expand {
                            self.sec_dock_collapsed = false;
                        }
                    }
                    let _ = resp.on_hover_text(hint);
                }
            });
    }

    /// 次级坞内容:组 Tab 条 + 组内子 Tab + 面板正文 + 浮窗/关闭控制。
    fn sec_dock_body(&mut self, ui: &mut egui::Ui) {
        // ── 组 Tab 条(顺序 = sec_group_order;右键可重排)──
        let group_labels: Vec<&str> = self
            .sec
            .group_order
            .iter()
            .map(|&gi| SecGroup::from_index(gi).label())
            .collect();
        let mut gslot = self
            .sec
            .group_order
            .iter()
            .position(|&gi| gi == self.sec.active_group)
            .unwrap_or(0);
        let mut reorder: Option<(usize, i32)> = None;
        ui.horizontal(|ui| {
            let r = PanelTabs::new(&group_labels, &mut gslot)
                .reorderable(true)
                .ui_ex(ui);
            if r.changed {
                self.sec.active_group = self.sec.group_order[gslot];
                self.sec.active = None; // 组切换 → 回退到组内第一个开着的面板
            }
            reorder = r.reorder;
        });
        if let Some((i, dir)) = reorder {
            let j = (i as i32 + dir).clamp(0, (SecGroup::ALL.len() - 1) as i32) as usize;
            self.sec.group_order.swap(i, j);
        }

        // ── 组内子 Tab(只列开着的面板)+ 控制 ──
        // 04-3 顺带修(P1-⑦ 实测:次级坞 + 主右坞同开时按钮被裁切):
        // PanelTabs 会吞掉**全部**可用宽(available_width/条数),把行尾的
        // 浮窗/关闭按钮挤出坞外。先为两个图标按钮预留宽度,Tab 条只占剩余。
        let group = SecGroup::from_index(self.sec.active_group);
        let open_panels: Vec<SecPanel> = group
            .panels()
            .iter()
            .copied()
            .filter(|&p| self.sec_is_open(p))
            .collect();
        let Some(active) = self.sec_effective() else {
            ui.add_space(theme::space::S5);
            ui.label(caption(
                ui,
                "该组面板均已关闭 —— 用「窗口」菜单或快捷键打开(如 ⇧F6 外观、Ctrl+T 字符)。",
            ));
            return;
        };
        ui.horizontal(|ui| {
            // 预留:2 个图标按钮(各一行高)+ 项间距
            let reserve = 2.0 * (theme::space::ROW_HEIGHT + ui.spacing().item_spacing.x);
            let tabs_w = (ui.available_width() - reserve).max(72.0);
            ui.allocate_ui(egui::vec2(tabs_w, theme::space::ROW_HEIGHT), |ui| {
                let labels: Vec<&str> = open_panels.iter().map(|p| p.label()).collect();
                let mut slot = open_panels.iter().position(|&p| p == active).unwrap_or(0);
                if PanelTabs::new(&labels, &mut slot).ui(ui) {
                    self.sec.active = Some(open_panels[slot]);
                }
            });
            // 浮窗 ⇄ 停靠 切换(04-2-3:浮窗保留但默认关闭)
            let floating = self.sec_is_floating(active);
            let tip = if floating {
                "该面板当前是浮窗;点击停靠回面板坞"
            } else {
                "把该面板改为浮窗(位置自动排布,不级联)"
            };
            if icon_button(
                ui,
                if floating {
                    Name::PanelProperties
                } else {
                    Name::Expanded
                },
                tip,
            )
            .clicked()
            {
                self.sec.floating[active.index()] = !floating;
            }
            // 关闭(与浮窗 × 同语义)
            if icon_button(ui, Name::Close, &format!("关闭「{}」面板", active.label())).clicked()
            {
                self.sec_set_open(active, false);
                self.say(format!("「{}」面板:已关闭", active.label()));
            }
        });
        ui.separator();
        self.sec_panel_body(ui, active);
    }

    /// 单面板正文(坞内与浮窗共用;`pub(crate)` 的各面板 `_body`)。
    fn sec_panel_body(&mut self, ui: &mut egui::Ui, p: SecPanel) {
        match p {
            SecPanel::Char => self.char_panel_body(ui),
            SecPanel::Para => self.para_panel_body(ui),
            SecPanel::Appearance => self.appearance_panel_body(ui),
            SecPanel::Stroke => self.stroke_panel_body(ui),
            SecPanel::Gradient => self.gradient_panel_body(ui),
            SecPanel::Opacity => self.opacity_panel_body(ui),
            SecPanel::Color => self.color_panel_body(ui),
            SecPanel::Transform => self.transform_panel_body(ui),
            SecPanel::Align => self.align_panel_body(ui),
            SecPanel::History => self.history_panel_body(ui),
            SecPanel::Assets => self.assets_panel_body(ui),
            SecPanel::Timeline => self.timeline_panel_body(ui),
            SecPanel::Plugins => self.plugins_panel_body(ui),
        }
    }

    /// 受控浮窗:位置 = 已记忆位置(用户拖过)或自动排布格子(防级联)。
    fn show_sec_floating(&mut self, ui: &mut egui::Ui, p: SecPanel, auto: egui::Pos2) {
        let idx = p.index();
        let remembered = self.sec.pos[idx];
        let has_memory = self.sec.pos_memory[idx];
        let mut open = true;
        let mut win = egui::Window::new(format!("{}(浮窗)", p.label()))
            .open(&mut open)
            .collapsible(false)
            .default_width(300.0);
        win = if has_memory {
            win.default_pos(egui::pos2(remembered[0], remembered[1]))
        } else {
            win.default_pos(auto)
        };
        let resp = win.show(ui.ctx(), |ui| self.sec_panel_body(ui, p));
        // 位置记忆:拿到真实窗口矩形后记一次(此后 egui 自己维护位置,
        // 用户拖动更新的是它的内存;重启则用这里的记忆位还原)
        if let Some(r) = &resp {
            if !self.sec.pos_memory[idx] {
                self.sec.pos[idx] = [r.response.rect.left(), r.response.rect.top()];
                self.sec.pos_memory[idx] = true;
            }
        }
        self.sec_set_open(p, open);
    }
}

// ─────────────────────── 04-2-6 度量门禁(单测) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 参考视口(与实测截图同量级:1680×1000)。
    fn ref_viewport() -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1680.0, 1000.0))
    }

    /// U-2 规则表逐行:<1600 强制折叠(用户偏好不回写),≥1600 尊重用户。
    #[test]
    fn sec_dock_collapse_rule_table() {
        assert!(sec_should_collapse(1599.0, false), "<1600 强制折叠");
        assert!(sec_should_collapse(1599.0, true));
        assert!(!sec_should_collapse(1600.0, false), "边界含等号");
        assert!(sec_should_collapse(1600.0, true), "用户折的尊重");
        assert!(!sec_should_collapse(1920.0, false));
        assert!(!sec_rail_can_expand(1599.0), "窄窗图标条只切组");
        assert!(sec_rail_can_expand(1600.0));
        // 宽度钳制与默认值
        assert_eq!(clamp_sec_width(300.0), 300.0);
        assert_eq!(clamp_sec_width(0.0), SEC_DOCK_MIN);
        assert_eq!(clamp_sec_width(9999.0), SEC_DOCK_MAX);
        assert!(!sec_should_collapse(ref_viewport().width(), false));
    }

    /// U-2 门禁:窗口 <1600 时次级坞折成图标条;1600px 整以**默认宽度**
    /// (主坞 280 + 次级坞 300)铺开时画布仍 ≥60%(用户手动拖宽不受此限)。
    #[test]
    fn narrow_window_canvas_keeps_60_percent() {
        // 窄窗:两坞都按折叠态(图标条)计
        for w in [1024.0f32, 1280.0, 1599.0] {
            assert!(sec_should_collapse(w, false), "{w}px 次级坞必须折叠");
            let canvas = w - SEC_RAIL_WIDTH - vb_ui::dock::ICON_RAIL_WIDTH;
            assert!(
                canvas / w >= 0.60,
                "{w}px 窗口下画布仅 {canvas}px({:.0}%),低于 60% 下限",
                canvas / w * 100.0
            );
        }
        // 1600px 整:次级坞展开,默认宽度下画布 ≥60%
        let w = 1600.0;
        assert!(!sec_should_collapse(w, false));
        let canvas = w - vb_ui::theme::space::DOCK_WIDTH - SEC_DOCK_WIDTH;
        assert!(
            canvas / w >= 0.60,
            "1600px 默认布局画布仅 {canvas}px({:.0}%),低于 60%",
            canvas / w * 100.0
        );
    }

    /// 确定性构造:显式喂默认工作区配置,绕过机器上的真实
    /// `workspace.json`(单测必须与运行环境解耦)。
    fn app_fresh() -> VellumApp {
        let ctx = egui::Context::default();
        VellumApp::construct(
            &ctx,
            dock_layout::WorkspaceConfig::default(),
            [0, 1, 2, 3],
            None,
            vb_doc::model::Document::new_default(),
            None,
        )
    }

    fn cell_rects(origins: &[egui::Pos2]) -> Vec<Rect> {
        origins
            .iter()
            .map(|&o| Rect::from_min_size(o, egui::vec2(CELL_W, CELL_H)))
            .collect()
    }

    /// 门禁 1:连开 7 个面板且全部浮窗 → 自动排布格子两两不相交(无级联),
    /// 起点全部落在视口内。
    #[test]
    fn seven_floating_panels_never_cascade() {
        let vp = ref_viewport();
        let origins = floating_origins(7, vp);
        assert_eq!(origins.len(), 7);
        let cells = cell_rects(&origins);
        for (i, a) in cells.iter().enumerate() {
            assert!(
                vp.contains(a.min) && vp.intersects(*a),
                "浮窗 {i} 起点越界:{a:?}(视口 {vp:?})"
            );
            assert!(vp.contains(a.min), "浮窗 {i} 左上角必须落在视口内:{a:?}");
            for (j, b) in cells.iter().enumerate().skip(i + 1) {
                assert!(
                    !a.intersects(*b),
                    "浮窗 {i} 与 {j} 的排布格子相交 → 级联回归:{a:?} vs {b:?}"
                );
            }
        }
    }

    /// 门禁 2:默认态(全停靠)连开 7 个面板 → 零浮窗;画布可见面积损失
    /// = 次级坞宽 × 坞高,在参考视口下 ≤ 20%。
    #[test]
    fn seven_docked_panels_loss_within_cap() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh();
        // 默认:全部停靠、全部关闭
        for &p in &SecPanel::ALL {
            assert!(!app.sec_is_floating(p), "浮窗必须默认关闭({p:?})");
            app.sec_set_open(p, true);
        }
        // 连开 7 个后:仍无一个浮窗(全部在坞内 = 零画布遮挡面)
        let floating_open = SecPanel::ALL
            .iter()
            .copied()
            .filter(|&p| app.sec_is_open(p) && app.sec_is_floating(p))
            .count();
        assert_eq!(floating_open, 0, "默认停靠态不允许出现浮窗");
        // 画布可见面积损失(参考视口):坞宽 / 视口宽 ≤ 20%
        let loss = SEC_DOCK_WIDTH / ref_viewport().width();
        assert!(
            loss <= 0.20,
            "次级坞占参考视口宽度 {:.1}%,超过 20% 上限",
            loss * 100.0
        );
    }

    /// 门禁 3:分组合法性 —— 十面板全部有组(07-D 新增历史)、四组覆盖
    /// 不重不漏、标题唯一。
    #[test]
    fn groups_cover_all_panels_exactly_once() {
        let mut seen = std::collections::HashSet::new();
        for g in SecGroup::ALL {
            for &p in g.panels() {
                assert_eq!(p.group(), g, "{p:?} 的组归属与组员清单不一致");
                assert!(seen.insert(p), "{p:?} 被两组同时收录");
            }
        }
        assert_eq!(
            seen.len(),
            SecPanel::ALL.len(),
            "五个组必须恰好覆盖全部面板"
        );
        let mut labels = std::collections::HashSet::new();
        for &p in &SecPanel::ALL {
            assert!(labels.insert(p.label()), "面板标题重复:{}", p.label());
        }
    }

    /// 门禁 4:`workspace.json` 摆放往返(浮窗 + 位置逐字段还原)。
    #[test]
    fn placements_roundtrip_through_workspace_config() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh();
        app.sec_set_open(SecPanel::Char, true);
        app.sec.floating[SecPanel::Char.index()] = true;
        app.sec.pos[SecPanel::Char.index()] = [120.0, 88.0];
        let cfg = app.workspace_config();
        assert!(cfg.sec_floating[SecPanel::Char.index()]);
        assert_eq!(cfg.sec_pos[SecPanel::Char.index()], [120.0, 88.0]);
        // 还原路径:从配置重建会话态,浮窗态与位置逐字段回来
        let rebuilt = SecDockState::from_config(&cfg);
        assert!(rebuilt.floating[SecPanel::Char.index()]);
        assert_eq!(rebuilt.pos[SecPanel::Char.index()], [120.0, 88.0]);
        // 关闭 Char,打开并聚焦 Align:选中态切到变换组
        app.sec_set_open(SecPanel::Char, false);
        app.sec_set_open(SecPanel::Align, true);
        app.sec_focus(SecPanel::Align);
        assert_eq!(app.sec.active_group, SecPanel::Align.group().index());
        assert_eq!(app.sec.active, Some(SecPanel::Align));
    }

    /// 门禁 5:from_config 的非法输入防御(越界组序 / 缺字段)。
    #[test]
    fn sec_dock_state_from_config_is_defensive() {
        let cfg = dock_layout::WorkspaceConfig {
            sec_active_group: 99,
            sec_group_order: vec![2, 9, 1, 0],
            ..dock_layout::WorkspaceConfig::default()
        };
        let s = SecDockState::from_config(&cfg);
        assert!(s.active_group < SecGroup::ALL.len(), "越界组下标必须夹回");
        assert_eq!(
            std::collections::HashSet::<_>::from_iter(s.group_order).len(),
            SecGroup::ALL.len(),
            "非法排列(重复/缺位)必须回退默认顺序"
        );
    }
}
