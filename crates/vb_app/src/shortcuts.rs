//! 快捷键注册表与输入上下文栈(设计文档 02 篇 §一/§七,14 篇 §4.1)。
//!
//! **单一真相**:菜单标签的键位文本、按键派发、启动冲突自检、门禁自检
//! 全部读本文件。新增一条快捷键必须同时:
//!
//! 1. 在本表的 `SHORTCUTS` 登记(键位 + 生效上下文);
//! 2. 在 `IMPLEMENTED_IDS` 登记(声明已实现);
//! 3. 在 `app.rs::run_command` 的 `match` 中实现;
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
    const fn specificity(self) -> u8 {
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
    /// 允许生效的上下文集合。
    pub ctx: CtxSet,
}

impl Shortcut {
    /// 菜单右侧显示的键位文本(如 `Ctrl+Shift+Z`)。
    pub fn key_text(&self) -> String {
        let mut s = String::new();
        if self.ctrl == ModMatch::On {
            s.push_str("Ctrl+");
        }
        if self.shift == ModMatch::On {
            s.push_str("Shift+");
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

/// 全局快捷键表(单一真相)。
///
/// 排序无关:同键位多条绑定时按 `ModMatch::specificity` 择优。
pub const SHORTCUTS: &[Shortcut] = &[
    // ── 文件(全局:文本编辑态下仍需可用) ──
    Shortcut {
        id: "file.new",
        key: Key::N,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_ALL,
    },
    Shortcut {
        id: "file.open",
        key: Key::O,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_ALL,
    },
    Shortcut {
        id: "file.save",
        key: Key::S,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_ALL,
    },
    // 02 篇 §四-文件:`Mod+E` = 导出…;`Mod+Shift+E` = 上一导出(快速重复)。
    Shortcut {
        id: "file.export_dialog",
        key: Key::E,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_ALL,
    },
    Shortcut {
        id: "file.export_repeat",
        key: Key::E,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_ALL,
    },
    // ── 编辑(文本编辑态交还输入框自行处理) ──
    Shortcut {
        id: "edit.undo",
        key: Key::Z,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "edit.redo",
        key: Key::Z,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "edit.select_all",
        key: Key::A,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    // ── 对象 ──
    Shortcut {
        id: "object.group",
        key: Key::G,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.ungroup",
        key: Key::G,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.transform_again",
        key: Key::D,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.bring_forward",
        key: Key::CloseBracket,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.bring_to_front",
        key: Key::CloseBracket,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.send_backward",
        key: Key::OpenBracket,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.send_to_back",
        key: Key::OpenBracket,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.delete",
        key: Key::Delete,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.delete",
        key: Key::Backspace,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // ── 视图(B2:菜单显示的加速键必须真的绑定) ──
    // `+` 在多数键盘上需 Shift+= ,故 shift 取 Any,保证 Ctrl+Shift+= 也能放大。
    Shortcut {
        id: "view.zoom_in",
        key: Key::Plus,
        ctrl: ModMatch::On,
        shift: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.zoom_in",
        key: Key::Equals,
        ctrl: ModMatch::On,
        shift: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.zoom_out",
        key: Key::Minus,
        ctrl: ModMatch::On,
        shift: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.fit",
        key: Key::Num0,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.actual_size",
        key: Key::Num1,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_NO_TEXT,
    },
    // B3:`Mod+Y` 在 AI 里是轮廓模式,**不是重做**(重做是 Mod+Shift+Z)。
    Shortcut {
        id: "view.outline",
        key: Key::Y,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_grid",
        key: Key::Quote,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_smart_guides",
        key: Key::Semicolon,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    // ── 工具箱(02 篇 §三) ──
    Shortcut {
        id: "tool.select",
        key: Key::V,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.rect",
        key: Key::M,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.ellipse",
        key: Key::L,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.hand",
        key: Key::H,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.line",
        key: Key::Backslash,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.pen",
        key: Key::P,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.direct_select",
        key: Key::A,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.text",
        key: Key::T,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.eyedropper",
        key: Key::I,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.artboard",
        key: Key::O,
        ctrl: ModMatch::Off,
        shift: ModMatch::On,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.gradient",
        key: Key::G,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.scissors",
        key: Key::C,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.group_select",
        key: Key::Y,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.zoom",
        key: Key::Z,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    // ── 画布(方向键微移;Shift = 10px) ──
    Shortcut {
        id: "canvas.nudge_left",
        key: Key::ArrowLeft,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "canvas.nudge_right",
        key: Key::ArrowRight,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "canvas.nudge_up",
        key: Key::ArrowUp,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "canvas.nudge_down",
        key: Key::ArrowDown,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // Esc 在文本编辑态下由文本窗口自行处理(02 篇:文本态仅 Esc / Mod+Enter 生效)。
    Shortcut {
        id: "canvas.cancel",
        key: Key::Escape,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // P4 钢笔:Enter 结束开放路径
    Shortcut {
        id: "canvas.pen_finish",
        key: Key::Enter,
        ctrl: ModMatch::Off,
        shift: ModMatch::Any,
        ctx: CtxSet::one(InputContext::Canvas),
    },
    // ── P3.3 剪贴板(画布上下文;文本框内 Ctrl+C 交给 egui 原生) ──
    Shortcut {
        id: "edit.copy",
        key: Key::C,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "edit.cut",
        key: Key::X,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "edit.paste",
        key: Key::V,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    // 贴在前面(AI:Mod+F);Mod+Shift+V 留给垂直居中对齐
    Shortcut {
        id: "edit.paste_in_place",
        key: Key::F,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    // ── P3.8 对齐六快捷键(02 篇 §四,与 AI 完全一致) ──
    Shortcut {
        id: "align.left",
        key: Key::L,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.hcenter",
        key: Key::C,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.right",
        key: Key::R,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.top",
        key: Key::T,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.vcenter",
        key: Key::V,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.bottom",
        key: Key::B,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    // ── P3.9 锁定/隐藏(Mod+2/3;Alt 变体待注册表支持 Alt 维度后补) ──
    Shortcut {
        id: "object.lock",
        key: Key::Num2,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.hide",
        key: Key::Num3,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_CANVAS,
    },
    // ── P3.2 命令面板 ──
    Shortcut {
        id: "app.command_palette",
        key: Key::K,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        ctx: CTX_NO_TEXT,
    },
];

/// 已实现的命令 ID(派发层声明)。必须与 `app.rs::run_command` 的 match 覆盖一致。
pub const IMPLEMENTED_IDS: &[&str] = &[
    "file.new",
    "file.open",
    "file.save",
    "file.export_dialog",
    "file.export_repeat",
    "edit.undo",
    "edit.redo",
    "edit.select_all",
    "object.group",
    "object.ungroup",
    "object.transform_again",
    "object.bring_forward",
    "object.bring_to_front",
    "object.send_backward",
    "object.send_to_back",
    "object.delete",
    "view.zoom_in",
    "view.zoom_out",
    "view.fit",
    "view.actual_size",
    "view.outline",
    "view.toggle_grid",
    "view.toggle_smart_guides",
    "tool.select",
    "tool.rect",
    "tool.ellipse",
    "tool.line",
    "tool.pen",
    "tool.direct_select",
    "tool.zoom",
    "tool.hand",
    "tool.text",
    "tool.eyedropper",
    "tool.artboard",
    "tool.gradient",
    "tool.scissors",
    "tool.group_select",
    "canvas.nudge_left",
    "canvas.nudge_right",
    "canvas.nudge_up",
    "canvas.nudge_down",
    "canvas.cancel",
    "canvas.pen_finish",
    // 无键位绑定、仅出现在菜单里的命令
    "view.toggle_theme",
    "object.unlock_all",
    "object.show_all",
    "app.about",
    "app.quit",
    // P3 批次
    "edit.copy",
    "edit.cut",
    "edit.paste",
    "edit.paste_in_place",
    "align.left",
    "align.hcenter",
    "align.right",
    "align.top",
    "align.vcenter",
    "align.bottom",
    "object.lock",
    "object.hide",
    "app.command_palette",
    "object.distribute_h",
    "object.distribute_v",
    "view.toggle_rulers",
    "view.toggle_guides",
    "view.lock_guides",
    "view.guides_from_selection",
    "view.toggle_rulers",
    "view.toggle_guides",
    "view.lock_guides",
    "path.union",
    "path.subtract",
    "path.intersect",
    "path.xor",
];

/// 全部命令的中文名(命令面板 / 菜单 / 状态提示共用)。
pub const CMD_LABELS: &[(&str, &str)] = &[
    ("file.new", "新建文档"),
    ("file.open", "打开项目…"),
    ("file.save", "保存"),
    ("file.export_dialog", "导出…"),
    ("file.export_repeat", "上次导出(当前画板 PNG @2x)"),
    ("app.quit", "退出"),
    ("edit.undo", "撤销"),
    ("edit.redo", "重做"),
    ("edit.select_all", "全选(当前画板)"),
    ("edit.copy", "复制"),
    ("edit.cut", "剪切"),
    ("edit.paste", "粘贴"),
    ("edit.paste_in_place", "贴在前面(就地)"),
    ("object.group", "编组"),
    ("object.ungroup", "取消编组"),
    ("object.transform_again", "再次变换"),
    ("object.bring_forward", "前移一层"),
    ("object.bring_to_front", "置于顶层"),
    ("object.send_backward", "后移一层"),
    ("object.send_to_back", "置于底层"),
    ("object.delete", "删除对象"),
    ("object.lock", "锁定所选"),
    ("object.unlock_all", "解锁全部"),
    ("object.hide", "隐藏所选"),
    ("object.show_all", "显示全部"),
    ("align.left", "水平左对齐"),
    ("align.hcenter", "水平居中对齐"),
    ("align.right", "水平右对齐"),
    ("align.top", "垂直顶对齐"),
    ("align.vcenter", "垂直居中对齐"),
    ("align.bottom", "垂直底对齐"),
    ("path.union", "路径查找器:联集"),
    ("path.subtract", "路径查找器:减去顶层"),
    ("path.intersect", "路径查找器:交集"),
    ("path.xor", "路径查找器:差集"),
    ("object.distribute_h", "水平等距分布"),
    ("object.distribute_v", "垂直等距分布"),
    ("view.zoom_in", "放大"),
    ("view.zoom_out", "缩小"),
    ("view.fit", "适合窗口"),
    ("view.actual_size", "实际大小 100%"),
    ("view.outline", "轮廓模式(线框)"),
    ("view.toggle_grid", "显示 / 隐藏网格"),
    ("view.toggle_smart_guides", "智能参考线开关"),
    ("view.toggle_theme", "深色 / 浅色主题"),
    ("tool.select", "选择工具"),
    ("tool.rect", "矩形工具"),
    ("tool.ellipse", "椭圆工具"),
    ("tool.line", "直线工具"),
    ("tool.pen", "钢笔工具"),
    ("tool.direct_select", "直接选择工具"),
    ("tool.zoom", "缩放工具"),
    ("tool.hand", "抓手工具"),
    ("tool.text", "文字工具"),
    ("tool.eyedropper", "吸管工具"),
    ("tool.artboard", "画板工具"),
    ("tool.gradient", "渐变工具"),
    ("tool.scissors", "剪刀工具"),
    ("tool.group_select", "编组选择工具"),
    ("canvas.cancel", "取消 / 清空选区"),
    ("canvas.pen_finish", "钢笔:结束路径"),
    ("canvas.nudge_left", "微移左 1px"),
    ("canvas.nudge_right", "微移右 1px"),
    ("canvas.nudge_up", "微移上 1px"),
    ("canvas.nudge_down", "微移下 1px"),
    ("view.toggle_rulers", "显示 / 隐藏标尺"),
    ("view.toggle_guides", "显示 / 隐藏参考线"),
    ("view.lock_guides", "锁定参考线"),
    ("view.guides_from_selection", "从选区生成参考线"),
    ("app.command_palette", "命令面板"),
    ("app.about", "关于"),
];

/// 命令的中文名(无则返回 None)。
pub fn command_label(id: &str) -> Option<&'static str> {
    CMD_LABELS.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// 该命令 ID 是否已实现。
pub fn is_implemented(id: &str) -> bool {
    IMPLEMENTED_IDS.contains(&id)
}

/// 按 (键位, Ctrl, Shift) 查表。同键位多条绑定时取最具体的一条。
pub fn lookup(key: Key, ctrl: bool, shift: bool) -> Option<&'static Shortcut> {
    let mut best: Option<(&'static Shortcut, u8)> = None;
    for s in SHORTCUTS {
        if s.key != key || !s.ctrl.hit(ctrl) || !s.shift.hit(shift) {
            continue;
        }
        let spec = s.ctrl.specificity() + s.shift.specificity();
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

// ─────────────────────────── 菜单结构声明 ───────────────────────────

/// 菜单项。`id` 必须在 `IMPLEMENTED_IDS` 中;
/// 键位文本**自动查表**,禁止在 label 里手写。
#[derive(Debug, Clone, Copy)]
pub struct MenuItem {
    pub id: &'static str,
    pub label: &'static str,
}

pub const MENU_FILE: &[MenuItem] = &[
    MenuItem {
        id: "file.new",
        label: "新建",
    },
    MenuItem {
        id: "file.open",
        label: "打开项目…",
    },
    MenuItem {
        id: "file.save",
        label: "保存",
    },
    MenuItem {
        id: "file.export_dialog",
        label: "导出…",
    },
    MenuItem {
        id: "file.export_repeat",
        label: "上次导出(当前画板 PNG @2x)",
    },
    MenuItem {
        id: "app.quit",
        label: "退出",
    },
];

pub const MENU_EDIT: &[MenuItem] = &[
    MenuItem {
        id: "edit.undo",
        label: "撤销",
    },
    MenuItem {
        id: "edit.redo",
        label: "重做",
    },
    MenuItem {
        id: "edit.select_all",
        label: "全选(当前画板)",
    },
];

pub const MENU_OBJECT: &[MenuItem] = &[
    MenuItem {
        id: "object.group",
        label: "编组",
    },
    MenuItem {
        id: "object.ungroup",
        label: "取消编组",
    },
    MenuItem {
        id: "object.transform_again",
        label: "再次变换",
    },
    MenuItem {
        id: "object.bring_forward",
        label: "前移一层",
    },
    MenuItem {
        id: "object.bring_to_front",
        label: "置于顶层",
    },
    MenuItem {
        id: "object.send_backward",
        label: "后移一层",
    },
    MenuItem {
        id: "object.send_to_back",
        label: "置于底层",
    },
    MenuItem {
        id: "object.delete",
        label: "删除",
    },
    MenuItem {
        id: "path.union",
        label: "联集",
    },
    MenuItem {
        id: "path.subtract",
        label: "减去顶层",
    },
    MenuItem {
        id: "path.intersect",
        label: "交集",
    },
    MenuItem {
        id: "path.xor",
        label: "差集",
    },
];

pub const MENU_VIEW: &[MenuItem] = &[
    MenuItem {
        id: "view.zoom_in",
        label: "放大",
    },
    MenuItem {
        id: "view.zoom_out",
        label: "缩小",
    },
    MenuItem {
        id: "view.fit",
        label: "适合窗口",
    },
    MenuItem {
        id: "view.actual_size",
        label: "实际大小",
    },
    MenuItem {
        id: "view.outline",
        label: "轮廓模式(线框)",
    },
    MenuItem {
        id: "view.toggle_grid",
        label: "显示网格",
    },
    MenuItem {
        id: "view.toggle_smart_guides",
        label: "智能参考线",
    },
    MenuItem {
        id: "view.toggle_theme",
        label: "浅色主题",
    },
];

pub const MENU_HELP: &[MenuItem] = &[MenuItem {
    id: "app.about",
    label: "关于",
}];

/// 全部菜单声明(门禁自检用)。
pub const MENUS: &[&[MenuItem]] = &[MENU_FILE, MENU_EDIT, MENU_OBJECT, MENU_VIEW, MENU_HELP];

/// 菜单项的完整显示文本:`标签` + 制表符 + `键位`(无绑定时只有标签)。
pub fn menu_label(item: &MenuItem) -> String {
    match key_text_for(item.id) {
        Some(k) => format!("{}\t{k}", item.label),
        None => item.label.to_string(),
    }
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
                lookup(key, ctrl, shift).is_some(),
                "键位 {key:?} (ctrl={ctrl}, shift={shift}) 未绑定任何命令"
            );
        }
        assert_eq!(lookup(Key::Num0, true, false).unwrap().id, "view.fit");
        assert_eq!(
            lookup(Key::Num1, true, false).unwrap().id,
            "view.actual_size"
        );
    }

    /// 02 篇 §八 验收项:文本编辑态下按 `V` 不切工具。
    #[test]
    fn text_edit_context_swallows_tool_keys() {
        let v = lookup(Key::V, false, false).expect("V 应绑定选择工具");
        assert!(
            !fires_in(v, InputContext::TextEdit),
            "文本编辑态不应触发 {v:?}"
        );
        assert!(fires_in(v, InputContext::Canvas));

        let del = lookup(Key::Delete, false, false).expect("Delete 应绑定删除");
        assert!(
            !fires_in(del, InputContext::TextEdit),
            "文本编辑态不应删除对象"
        );

        // 但文件级命令在文本编辑态下仍应可用
        let save = lookup(Key::S, true, false).expect("Ctrl+S 应绑定保存");
        assert!(
            fires_in(save, InputContext::TextEdit),
            "文本编辑态仍应能保存"
        );
    }

    /// B3:`Mod+Y` = 轮廓模式;`Mod+Shift+Z` = 重做(不是 `Mod+Y`)。
    #[test]
    fn ctrl_y_is_outline_not_redo() {
        assert_eq!(lookup(Key::Y, true, false).unwrap().id, "view.outline");
        assert_eq!(lookup(Key::Z, true, true).unwrap().id, "edit.redo");
    }
}
// P4.2 标尺与参考线补充命令(无默认键位,经菜单/命令面板触发)
