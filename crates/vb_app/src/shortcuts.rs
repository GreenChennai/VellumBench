//! 快捷键注册表与输入上下文栈(设计文档 02 篇 §一/§七,14 篇 §4.1)。
//!
//! **单一真相**:菜单标签的键位文本、按键派发、启动冲突自检、门禁自检
//! 全部读本文件。新增一条快捷键必须同时:
//!
//! 1. 在本表的 `SHORTCUTS` 登记(键位 + 生效上下文);
//! 2. 在 `IMPLEMENTED_IDS` 登记(声明已实现);
//! 3. 在 `app/dispatch.rs::run_command` 的 `match` 中实现;
//! 4. 若出现在菜单里,使用 `MENU_*` 声明(键位文本自动查表,禁止手写)。
//!
//! 违反 1/2 会被告警测试 `registry_is_consistent` 拦住;违反 4 会被
//! `menu_items_are_implemented` 拦住;违反 3 会触发 `run_command` 的
//! debug 断言。

use egui::Key;

// ─────────────────────────── 输入上下文栈 ───────────────────────────

/// 输入上下文栈(02 篇 §一;14 篇 §4.1 细化为 5 级)。
///
/// 数值越大优先级越高,**只有栈顶上下文消费按键**。
/// 文本编辑态下除 `Esc` / `Mod+Enter` 外不触发任何工具快捷键。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum InputContext {
    /// 全局:任何上下文都生效(文件级命令)
    Global = 0,
    /// 画布:工具切换 / 选择 / 删除 / 方向键
    Canvas = 1,
    /// 工具活动态:绘制或变换进行中
    Tool = 2,
    /// 面板或输入框聚焦(数值框、下拉、单行输入)
    PanelFocus = 3,
    /// 文本编辑态(文本窗口、多行输入)
    TextEdit = 4,
}

impl InputContext {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Canvas => "canvas",
            Self::Tool => "tool.active",
            Self::PanelFocus => "panel.focus",
            Self::TextEdit => "text.edit",
        }
    }
}

/// 上下文集合(位掩码)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CtxSet(u8);

impl CtxSet {
    pub const fn one(c: InputContext) -> Self {
        Self(1u8 << c as u8)
    }
    pub const fn all() -> Self {
        Self(0b1_1111)
    }
    pub const fn or(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
    pub const fn and(self, o: Self) -> Self {
        Self(self.0 & o.0)
    }
    /// 去掉某个上下文。
    pub const fn not(self, c: InputContext) -> Self {
        Self(self.0 & !(1u8 << c as u8))
    }
    pub const fn contains(self, c: InputContext) -> bool {
        self.0 & (1u8 << c as u8) != 0
    }
    pub const fn bits(self) -> u8 {
        self.0
    }
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// 任何上下文都生效。
pub const CTX_ALL: CtxSet = CtxSet::all();
/// 画布 + 工具活动态(不含面板聚焦与文本编辑)。
pub const CTX_CANVAS: CtxSet =
    CtxSet::one(InputContext::Canvas).or(CtxSet::one(InputContext::Tool));
/// 除文本编辑态外的一切上下文。
pub const CTX_NO_TEXT: CtxSet = CTX_ALL.not(InputContext::TextEdit);

// ─────────────────────────── 修饰键匹配 ───────────────────────────

/// 修饰键匹配规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModMatch {
    /// 不关心是否按下
    Any,
    /// 必须按下
    On,
    /// 必须未按下
    Off,
}

impl ModMatch {
    pub const fn hit(self, down: bool) -> bool {
        match self {
            Self::Any => true,
            Self::On => down,
            Self::Off => !down,
        }
    }
    /// 越具体越优先(Any=0,On/Off=1),用于同键位多条绑定的择优。
    pub(crate) const fn specificity(self) -> u8 {
        match self {
            Self::Any => 0,
            Self::On | Self::Off => 1,
        }
    }
    /// 两条规则是否可能同时命中同一次按键(冲突判定用)。
    const fn overlaps(self, o: Self) -> bool {
        matches!(
            (self, o),
            (Self::Any, _) | (_, Self::Any) | (Self::On, Self::On) | (Self::Off, Self::Off)
        )
    }
}

// ─────────────────────────── 快捷键表 ───────────────────────────

/// 一条快捷键绑定。`id` 与 `commands.yaml` 同源。
#[derive(Debug, Clone, Copy)]
pub struct Shortcut {
    pub id: &'static str,
    pub key: Key,
    pub ctrl: ModMatch,
    pub shift: ModMatch,
    /// Alt 修饰键(04 阶段加入:`Ctrl+Alt+T` 段落面板需要 Alt 维度才能
    /// 与 `Ctrl+T` 字符面板、`Ctrl+Shift+T` 对齐在同一键位上互斥)。
    pub alt: ModMatch,
    /// 允许生效的上下文集合。
    pub ctx: CtxSet,
}

impl Shortcut {
    /// 菜单右侧显示的键位文本(如 `Ctrl+Shift+Z`、`Ctrl+Alt+T`)。
    pub fn key_text(&self) -> String {
        let mut s = String::new();
        if self.ctrl == ModMatch::On {
            s.push_str("Ctrl+");
        }
        if self.shift == ModMatch::On {
            s.push_str("Shift+");
        }
        if self.alt == ModMatch::On {
            s.push_str("Alt+");
        }
        s.push_str(key_label(self.key));
        s
    }

    /// 与该快捷键是否会争抢同一次按键。
    pub fn conflicts_with(&self, o: &Shortcut) -> bool {
        self.key == o.key
            && self.ctx.and(o.ctx).bits() != 0
            && self.ctrl.overlaps(o.ctrl)
            && self.shift.overlaps(o.shift)
            && self.alt.overlaps(o.alt)
    }
}

/// egui 键位 → 面板/菜单上显示的名字。
pub fn key_label(key: Key) -> &'static str {
    match key {
        Key::Escape => "Esc",
        Key::Delete => "Del",
        Key::Backspace => "Backspace",
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        other => other.symbol_or_name(),
    }
}
// 06-1 按职责拆分(纯搬移,零行为变化):绑定表 / 命令目录 / 菜单结构
// 各自成模块;对外路径经下方 `pub use` 保持不变。
mod binds;
mod catalog;
mod menus;

pub use binds::SHORTCUTS;
pub use catalog::{CMD_LABELS, IMPLEMENTED_IDS};
pub use menus::{
    menu_label, pathfinder_tip, planned_reason, MenuItem, MENUS, MENU_EDIT, MENU_EFFECT, MENU_FILE,
    MENU_HELP, MENU_OBJECT, MENU_SELECT, MENU_TITLES, MENU_TYPE, MENU_VIEW, MENU_WINDOW,
    PATHFINDER_TIPS, PLANNED,
};

/// 命令的中文名(无则返回 None)。
pub fn command_label(id: &str) -> Option<&'static str> {
    CMD_LABELS.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// 该命令 ID 是否已实现。
pub fn is_implemented(id: &str) -> bool {
    IMPLEMENTED_IDS.contains(&id)
}

/// 按 (键位, Ctrl, Shift, Alt) 查表。同键位多条绑定时取最具体的一条。
pub fn lookup(key: Key, ctrl: bool, shift: bool, alt: bool) -> Option<&'static Shortcut> {
    let mut best: Option<(&'static Shortcut, u8)> = None;
    for s in SHORTCUTS {
        if s.key != key || !s.ctrl.hit(ctrl) || !s.shift.hit(shift) || !s.alt.hit(alt) {
            continue;
        }
        let spec = s.ctrl.specificity() + s.shift.specificity() + s.alt.specificity();
        if best.map(|(_, b)| spec > b).unwrap_or(true) {
            best = Some((s, spec));
        }
    }
    best.map(|(s, _)| s)
}

/// 某条快捷键在给定上下文下是否生效。
pub fn fires_in(s: &Shortcut, top: InputContext) -> bool {
    s.ctx.contains(top)
}

/// 冲突自检(02 篇 §七-3:启动无冲突 warning)。
/// 返回每对冲突的两条绑定;正常情况下为空。
pub fn conflicts() -> Vec<(&'static Shortcut, &'static Shortcut)> {
    let mut out = Vec::new();
    for (i, a) in SHORTCUTS.iter().enumerate() {
        for b in &SHORTCUTS[i + 1..] {
            if a.conflicts_with(b) {
                out.push((a, b));
            }
        }
    }
    out
}

/// 查某命令 ID 的键位文本(菜单渲染用;无绑定则返回 None)。
pub fn key_text_for(id: &str) -> Option<String> {
    SHORTCUTS.iter().find(|s| s.id == id).map(|s| s.key_text())
}

/// 命令所属菜单的标题(键位编辑器分组用);不在任何菜单里的命令
/// (工具/主页/命令面板等)返回「其他」。
pub fn menu_group_of(id: &str) -> &'static str {
    for (i, menu) in MENUS.iter().enumerate() {
        if menu.iter().any(|item| item.id == id) {
            return MENU_TITLES[i];
        }
    }
    "其他"
}

/// 输入上下文栈顶判定的**纯函数**(B1 派发层回归的测试面;
/// `VellumApp::input_context` 把 GUI 状态投影成本函数的四个布尔输入)。
///
/// 优先级:文本编辑 / 命令面板 / 任意输入框持有键盘 > 拖拽进行中 > 画布。
pub fn top_input_context(
    editing_text: bool,
    palette_open: bool,
    wants_keyboard: bool,
    dragging: bool,
) -> InputContext {
    if editing_text || palette_open || wants_keyboard {
        return InputContext::TextEdit;
    }
    if dragging {
        return InputContext::Tool;
    }
    InputContext::Canvas
}
// ─────────────────────────── 门禁自检 ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 表内无冲突(02 篇 §七-3)。
    #[test]
    fn registry_is_conflict_free() {
        let c = conflicts();
        assert!(
            c.is_empty(),
            "快捷键冲突:{}",
            c.iter()
                .map(|(a, b)| format!("{} ({}) ↔ {} ({})", a.id, a.key_text(), b.id, b.key_text()))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }

    /// 每条绑定都必须落在已实现清单里(防止"绑了没实现"= 界面说谎)。
    #[test]
    fn shortcuts_are_implemented() {
        for s in SHORTCUTS {
            assert!(
                is_implemented(s.id),
                "快捷键 {} 绑定了未实现的命令 {}",
                s.key_text(),
                s.id
            );
        }
    }

    /// 门禁 9(B2 回归防护):菜单里出现的每个命令都必须已实现;
    /// 且凡是有键位绑定的命令,菜单显示的键位文本必然来自注册表。
    #[test]
    fn menu_items_are_implemented() {
        for menu in MENUS {
            for item in *menu {
                assert!(
                    is_implemented(item.id),
                    "菜单「{}」引用了未实现的命令 {}",
                    item.label,
                    item.id
                );
                let text = menu_label(item);
                if let Some(k) = key_text_for(item.id) {
                    assert!(
                        text.ends_with(&k),
                        "菜单「{}」的键位文本 {text:?} 未包含注册表键位 {k:?}",
                        item.label
                    );
                }
            }
        }
    }

    /// 阶段 5(AI 规范 9 项菜单,副文档 06 §3 V1 已确认):
    /// 菜单栏标题固定为 9 项、「关于」归「帮助」;每项非空且都有命令 ID。
    #[test]
    fn menu_bar_has_nine_ai_standard_menus() {
        assert_eq!(MENUS.len(), 9, "菜单栏必须是 AI 规范 9 项");
        assert_eq!(
            MENU_TITLES,
            [
                "文件", "编辑", "对象", "文字", "选择", "效果", "视图", "窗口", "帮助"
            ],
        );
        for (i, menu) in MENUS.iter().enumerate() {
            assert!(!menu.is_empty(), "菜单「{}」为空", MENU_TITLES[i]);
            for it in *menu {
                assert!(!it.label.is_empty(), "菜单项 {} 无标签", it.id);
                assert!(it.id.contains('.'), "菜单项 id 必须带域前缀:{}", it.id);
            }
        }
        // 「关于」归帮助,顶层不另设「关于」
        assert!(!MENU_TITLES.contains(&"关于"));
        assert!(MENU_HELP.iter().any(|i| i.id == "app.about"));
    }

    /// 「已登记但未落地」的项:必须**同样已注册**(否则 Agent 调用会静默失败),
    /// 且必须配非空计划说明(菜单据此置灰 + 悬停提示,绝不点了没反应)。
    #[test]
    fn planned_items_are_registered_and_reasoned() {
        for (id, msg) in PLANNED {
            assert!(is_implemented(id), "计划项 {id} 未注册为命令");
            assert!(!msg.is_empty(), "计划项 {id} 缺计划说明");
            assert!(
                msg.contains("计划于 v"),
                "计划项 {id} 的说明必须写明版本:{msg}"
            );
        }
        // 已落地的项不得出现在计划表里(避免"实现了却仍置灰")
        for (id, _) in PLANNED {
            assert!(
                MENUS.iter().any(|m| m.iter().any(|i| i.id == *id)),
                "计划项 {id} 不在任何菜单里(应移出 PLANNED 或补菜单项)"
            );
        }
    }

    /// 02 篇 §八 验收项:「菜单显示的每个加速键都生效」。
    /// 逐条断言视图菜单 4 个加速键可被查表命中。
    #[test]
    fn view_menu_accelerators_are_bound() {
        for (key, ctrl, shift) in [
            (Key::Plus, true, false),
            (Key::Equals, true, false),
            (Key::Plus, true, true), // Ctrl+Shift+= (实体键盘的 +)
            (Key::Minus, true, false),
            (Key::Num0, true, false),
            (Key::Num1, true, false),
        ] {
            assert!(
                lookup(key, ctrl, shift, false).is_some(),
                "键位 {key:?} (ctrl={ctrl}, shift={shift}) 未绑定任何命令"
            );
        }
        assert_eq!(
            lookup(Key::Num0, true, false, false).unwrap().id,
            "view.fit"
        );
        assert_eq!(
            lookup(Key::Num1, true, false, false).unwrap().id,
            "view.actual_size"
        );
    }

    /// 02 篇 §八 验收项:文本编辑态下按 `V` 不切工具。
    #[test]
    fn text_edit_context_swallows_tool_keys() {
        let v = lookup(Key::V, false, false, false).expect("V 应绑定选择工具");
        assert!(
            !fires_in(v, InputContext::TextEdit),
            "文本编辑态不应触发 {v:?}"
        );
        assert!(fires_in(v, InputContext::Canvas));

        let del = lookup(Key::Delete, false, false, false).expect("Delete 应绑定删除");
        assert!(
            !fires_in(del, InputContext::TextEdit),
            "文本编辑态不应删除对象"
        );

        // 但文件级命令在文本编辑态下仍应可用
        let save = lookup(Key::S, true, false, false).expect("Ctrl+S 应绑定保存");
        assert!(
            fires_in(save, InputContext::TextEdit),
            "文本编辑态仍应能保存"
        );
    }

    /// B3:`Mod+Y` = 轮廓模式;`Mod+Shift+Z` = 重做(不是 `Mod+Y`)。
    #[test]
    fn ctrl_y_is_outline_not_redo() {
        assert_eq!(
            lookup(Key::Y, true, false, false).unwrap().id,
            "view.outline"
        );
        assert_eq!(lookup(Key::Z, true, true, false).unwrap().id, "edit.redo");
    }

    /// 04:Ctrl+T = 字符面板,Ctrl+Alt+T = 段落面板,Ctrl+Shift+T = 顶对齐
    /// —— 同一键位 T 靠 Alt/Shift 维度互斥(alt 维度加入后的互斥回归)。
    #[test]
    fn t_key_modifiers_are_mutually_exclusive() {
        assert_eq!(
            lookup(Key::T, true, false, false).unwrap().id,
            "view.toggle_char_panel"
        );
        assert_eq!(
            lookup(Key::T, true, false, true).unwrap().id,
            "view.toggle_para_panel"
        );
        assert_eq!(lookup(Key::T, true, true, false).unwrap().id, "align.top");
        assert_eq!(lookup(Key::T, false, false, false).unwrap().id, "tool.text");
        assert_eq!(
            lookup(Key::T, false, true, false).unwrap().id,
            "tool.text_cycle_mode"
        );
    }

    /// design/15 B1 派发层回归:输入上下文判定的**纯函数**在文本编辑/
    /// 面板输入聚焦时必须落到 `TextEdit`,而工具键(V/M/T)、Delete、
    /// Backspace、Shift+T 都不在 TextEdit 生效 —— 注册表测试
    /// `text_edit_context_swallows_tool_keys` 锁"绑定侧",本测试锁
    /// "运行时判定侧",两侧都绿才算 B1 收口。
    #[test]
    fn b1_dispatch_context_swallows_tool_keys() {
        // 文本编辑 / 命令面板 / 输入框聚焦 → 一律 TextEdit
        for (editing, palette, wants) in [
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, true, true),
        ] {
            let top = top_input_context(editing, palette, wants, false);
            assert_eq!(
                top,
                InputContext::TextEdit,
                "editing={editing} palette={palette} wants={wants}"
            );
            for (key, ctrl, shift, alt) in [
                (Key::V, false, false, false),      // 切选择工具
                (Key::Delete, false, false, false), // 删对象
                (Key::Backspace, false, false, false),
                (Key::T, false, true, false), // Shift+T 循环
                (Key::M, false, false, false),
            ] {
                if let Some(sc) = lookup(key, ctrl, shift, alt) {
                    assert!(!fires_in(sc, top), "TextEdit 态不得触发 {}({sc:?})", sc.id);
                }
            }
            // 文件级命令仍然可用(02 篇 §一)
            let save = lookup(Key::S, true, false, false).unwrap();
            assert!(fires_in(save, top));
        }
        // 无编辑无拖拽 → Canvas;拖拽中 → Tool
        assert_eq!(
            top_input_context(false, false, false, false),
            InputContext::Canvas
        );
        assert_eq!(
            top_input_context(false, false, false, true),
            InputContext::Tool
        );
    }
}
// P4.2 标尺与参考线补充命令(无默认键位,经菜单/命令面板触发)
