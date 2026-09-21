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
        alt: ModMatch::Any,
        ctx: CTX_ALL,
    },
    Shortcut {
        id: "file.open",
        key: Key::O,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_ALL,
    },
    Shortcut {
        id: "file.save",
        key: Key::S,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_ALL,
    },
    // 02 篇 §四-文件:`Mod+E` = 导出…;`Mod+Shift+E` = 上一导出(快速重复)。
    Shortcut {
        id: "file.export_dialog",
        key: Key::E,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_ALL,
    },
    Shortcut {
        id: "file.export_repeat",
        key: Key::E,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_ALL,
    },
    // ── 编辑(文本编辑态交还输入框自行处理) ──
    Shortcut {
        id: "edit.undo",
        key: Key::Z,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "edit.redo",
        key: Key::Z,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "edit.select_all",
        key: Key::A,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // ── 对象 ──
    Shortcut {
        id: "object.group",
        key: Key::G,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.ungroup",
        key: Key::G,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.transform_again",
        key: Key::D,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.bring_forward",
        key: Key::CloseBracket,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.bring_to_front",
        key: Key::CloseBracket,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.send_backward",
        key: Key::OpenBracket,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.send_to_back",
        key: Key::OpenBracket,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.delete",
        key: Key::Delete,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.delete",
        key: Key::Backspace,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // ── 视图(B2:菜单显示的加速键必须真的绑定) ──
    // `+` 在多数键盘上需 Shift+= ,故 shift 取 Any,保证 Ctrl+Shift+= 也能放大。
    Shortcut {
        id: "view.zoom_in",
        key: Key::Plus,
        ctrl: ModMatch::On,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.zoom_in",
        key: Key::Equals,
        ctrl: ModMatch::On,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.zoom_out",
        key: Key::Minus,
        ctrl: ModMatch::On,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.fit",
        key: Key::Num0,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.actual_size",
        key: Key::Num1,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // B3:`Mod+Y` 在 AI 里是轮廓模式,**不是重做**(重做是 Mod+Shift+Z)。
    Shortcut {
        id: "view.outline",
        key: Key::Y,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_grid",
        key: Key::Quote,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_smart_guides",
        key: Key::Semicolon,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // ── 工具箱(02 篇 §三) ──
    Shortcut {
        id: "tool.select",
        key: Key::V,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.rect",
        key: Key::M,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.ellipse",
        key: Key::L,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.hand",
        key: Key::H,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.line",
        key: Key::Backslash,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.pen",
        key: Key::P,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.direct_select",
        key: Key::A,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.text",
        key: Key::T,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // 04-3:Shift+T 在 点/区域 文本间循环(路径文本 v1.5,登记冻结点)
    Shortcut {
        id: "tool.text_cycle_mode",
        key: Key::T,
        ctrl: ModMatch::Off,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.eyedropper",
        key: Key::I,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.artboard",
        key: Key::O,
        ctrl: ModMatch::Off,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.gradient",
        key: Key::G,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.scissors",
        key: Key::C,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.group_select",
        key: Key::Y,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "tool.zoom",
        key: Key::Z,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // ── 画布(方向键微移;Shift = 10px) ──
    Shortcut {
        id: "canvas.nudge_left",
        key: Key::ArrowLeft,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "canvas.nudge_right",
        key: Key::ArrowRight,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "canvas.nudge_up",
        key: Key::ArrowUp,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "canvas.nudge_down",
        key: Key::ArrowDown,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // Esc 在文本编辑态下由文本窗口自行处理(02 篇:文本态仅 Esc / Mod+Enter 生效)。
    Shortcut {
        id: "canvas.cancel",
        key: Key::Escape,
        ctrl: ModMatch::Any,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // P4 钢笔:Enter 结束开放路径
    Shortcut {
        id: "canvas.pen_finish",
        key: Key::Enter,
        ctrl: ModMatch::Off,
        shift: ModMatch::Any,
        alt: ModMatch::Any,
        ctx: CtxSet::one(InputContext::Canvas),
    },
    // ── P3.3 剪贴板(画布上下文;文本框内 Ctrl+C 交给 egui 原生) ──
    Shortcut {
        id: "edit.copy",
        key: Key::C,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "edit.cut",
        key: Key::X,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "edit.paste",
        key: Key::V,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // 贴在前面(AI:Mod+F);Mod+Shift+V 留给垂直居中对齐
    Shortcut {
        id: "edit.paste_in_place",
        key: Key::F,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // ── P3.8 对齐六快捷键(02 篇 §四,与 AI 完全一致) ──
    Shortcut {
        id: "align.left",
        key: Key::L,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.hcenter",
        key: Key::C,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.right",
        key: Key::R,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.top",
        key: Key::T,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.vcenter",
        key: Key::V,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "align.bottom",
        key: Key::B,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // ── P3.9 锁定/隐藏(Mod+2/3;Alt 变体待注册表支持 Alt 维度后补) ──
    Shortcut {
        id: "object.lock",
        key: Key::Num2,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    Shortcut {
        id: "object.hide",
        key: Key::Num3,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_CANVAS,
    },
    // ── P3.2 命令面板 ──
    // ── C3:画板导航 / 面板 Tab ──
    Shortcut {
        id: "view.next_artboard",
        key: Key::PageDown,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.prev_artboard",
        key: Key::PageUp,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.next_panel_tab",
        key: Key::F4,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // ── S1-b 面板显隐(design/02 §四-面板显隐 / design/03 §五) ──
    // F7 = 图层面板(我们的面板坞里切到图层 Tab / 折叠);Tab = 隐藏/恢复所有面板。
    // 两条键位此前均未占用(commands.yaml 无 Tab/F7 绑定),文本编辑态不触发。
    Shortcut {
        id: "view.toggle_layers_panel",
        key: Key::F7,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_all_panels",
        key: Key::Tab,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "app.command_palette",
        key: Key::K,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // ── 04 字符/段落面板(design/03 §5.10;Ctrl+T / Ctrl+Alt+T) ──
    Shortcut {
        id: "view.toggle_char_panel",
        key: Key::T,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Off,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_para_panel",
        key: Key::T,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::On,
        ctx: CTX_NO_TEXT,
    },
    // ── S4 外观/描边面板(design/03 §5.9 / §5.7;⇧F6 / ^F10) ──
    // F6 = 颜色面板(design/03 §5.4,渐变/颜色面板阶段 4 后续入册);
    // ⇧F6 = 外观、^F10 = 描边,均为独立浮窗显隐,文本编辑态不触发。
    Shortcut {
        id: "view.toggle_appearance_panel",
        key: Key::F6,
        ctrl: ModMatch::Off,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_stroke_panel",
        key: Key::F10,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // ── S4-b 渐变 / 透明度 / 颜色面板(design/03 §5.6 / §5.8 / §5.4)──
    // ^F9 渐变、⇧^F10 透明度、F6 颜色;均为独立浮窗显隐,文本编辑态不触发。
    // 颜色动作键 X / Shift+X / D 与 AI 一致(纯 X、纯 D 无既有占用)。
    Shortcut {
        id: "view.toggle_gradient_panel",
        key: Key::F9,
        ctrl: ModMatch::On,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_opacity_panel",
        key: Key::F10,
        ctrl: ModMatch::On,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_color_panel",
        key: Key::F6,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "color.toggle_target",
        key: Key::X,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "color.swap_fill_stroke",
        key: Key::X,
        ctrl: ModMatch::Off,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "color.default_fill_stroke",
        key: Key::D,
        ctrl: ModMatch::Off,
        shift: ModMatch::Off,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    // ── 阶段 2:变换数值面板(⇧F8)/ 对齐面板(⇧F7)(design/03 §5.2/§5.3)──
    // 两条键位此前均未占用(F7 单键 = 图层、F8 无绑定)。
    Shortcut {
        id: "view.toggle_transform_panel",
        key: Key::F8,
        ctrl: ModMatch::Off,
        shift: ModMatch::On,
        alt: ModMatch::Any,
        ctx: CTX_NO_TEXT,
    },
    Shortcut {
        id: "view.toggle_align_panel",
        key: Key::F7,
        ctrl: ModMatch::Off,
        shift: ModMatch::On,
        alt: ModMatch::Any,
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
    "view.next_artboard",
    "view.prev_artboard",
    "view.next_panel_tab",
    "view.zoom_to_selection",
    // S1-b 面板显隐
    "view.toggle_layers_panel",
    "view.toggle_all_panels",
    // 04 字符/段落面板与文字工具模式循环
    "view.toggle_char_panel",
    "view.toggle_para_panel",
    "tool.text_cycle_mode",
    // S4 外观/描边面板显隐(⇧F6 / ^F10)
    "view.toggle_appearance_panel",
    "view.toggle_stroke_panel",
    // S4-b 渐变/透明度/颜色面板 + 颜色动作(^F9 / ⇧^F10 / F6;X / Shift+X / D)
    "view.toggle_gradient_panel",
    "view.toggle_opacity_panel",
    "view.toggle_color_panel",
    "color.toggle_target",
    "color.swap_fill_stroke",
    "color.default_fill_stroke",
    // ── 阶段 5(AI 规范 9 项菜单;副文档 06)──
    "file.close",
    "file.import_html",
    "file.print",
    "edit.preferences",
    "edit.keyboard_shortcuts",
    "object.clip_mask",
    "object.release_clip_mask",
    "object.outline_stroke",
    "text.upper_case",
    "text.lower_case",
    "text.create_outlines",
    "text.find_font",
    "select.inverse",
    "select.next_object",
    "select.prev_object",
    "select.same_fill",
    "select.same_stroke",
    "select.same_stroke_width",
    "select.all_text",
    "select.all_locked",
    "select.all_hidden",
    "effect.repeat_last",
    "effect.drop_shadow",
    "effect.inner_shadow",
    "effect.outer_glow",
    "effect.inner_glow",
    "effect.round_corners",
    "effect.gaussian_blur",
    "effect.feather",
    "effect.distort",
    "view.hide_edges",
    "view.browser_proof",
    "window.workspace_basic",
    "window.workspace_type",
    "window.workspace_export",
    "window.new_workspace",
    "window.tab_properties",
    "window.tab_layers",
    "window.tab_artboards",
    "window.tab_tokens",
    "help.shortcuts",
    "help.check_update",
    "help.capabilities",
    // 阶段 6(工具箱四向停靠 + 单双列;副文档 07)
    "view.dock_toolbar_top",
    "view.dock_toolbar_left",
    "view.dock_toolbar_right",
    "view.dock_toolbar_bottom",
    "view.toolbar_columns_1",
    "view.toolbar_columns_2",
    // ── 阶段 2(副文档 03:路径查找器扩展 / 变换 / 对齐)──
    "path.merge",
    "path.subtract_back",
    "path.crop",
    "view.toggle_transform_panel",
    "view.toggle_align_panel",
    "align.to_selection",
    "align.to_key_object",
    "align.to_artboard",
    "object.distribute_hspace",
    "object.distribute_vspace",
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
    ("view.next_artboard", "下一画板"),
    ("view.prev_artboard", "上一画板"),
    ("view.next_panel_tab", "切换右侧面板 Tab"),
    ("view.zoom_to_selection", "缩放到选区"),
    ("app.about", "关于"),
    // S1-b 面板显隐
    ("view.toggle_layers_panel", "图层面板显隐"),
    ("view.toggle_all_panels", "隐藏 / 恢复所有面板"),
    // 04 字符/段落面板
    ("view.toggle_char_panel", "字符面板显隐"),
    ("view.toggle_para_panel", "段落面板显隐"),
    ("tool.text_cycle_mode", "文字工具:点/区域循环"),
    // S4 外观/描边面板(design/03 §5.9 / §5.7)
    ("view.toggle_appearance_panel", "外观面板显隐"),
    ("view.toggle_stroke_panel", "描边面板显隐"),
    // S4-b 渐变/透明度/颜色(design/03 §5.6 / §5.8 / §5.4)
    ("view.toggle_gradient_panel", "渐变面板显隐"),
    ("view.toggle_opacity_panel", "透明度面板显隐"),
    ("view.toggle_color_panel", "颜色面板显隐"),
    ("color.toggle_target", "颜色:切换填充/描边"),
    ("color.swap_fill_stroke", "颜色:交换填充与描边"),
    ("color.default_fill_stroke", "颜色:恢复默认填色/描边色"),
    // 阶段 5(AI 规范 9 项菜单;副文档 06)
    ("file.close", "关闭文档"),
    ("file.import_html", "导入 HTML…"),
    ("file.print", "打印…"),
    ("edit.preferences", "首选项…"),
    ("edit.keyboard_shortcuts", "键盘快捷键…"),
    ("object.clip_mask", "建立剪切蒙版"),
    ("object.release_clip_mask", "释放剪切蒙版"),
    ("object.outline_stroke", "轮廓化描边"),
    ("text.upper_case", "更改大小写 → 大写"),
    ("text.lower_case", "更改大小写 → 小写"),
    ("text.create_outlines", "创建轮廓"),
    ("text.find_font", "查找字体…"),
    ("select.inverse", "选择反向"),
    ("select.next_object", "选择上方的下一个对象"),
    ("select.prev_object", "选择下方的下一个对象"),
    ("select.same_fill", "选择相同填充色"),
    ("select.same_stroke", "选择相同描边色"),
    ("select.same_stroke_width", "选择相同描边粗细"),
    ("select.all_text", "选择全部文本对象"),
    ("select.all_locked", "选择所有锁定对象"),
    ("select.all_hidden", "选择所有隐藏对象"),
    ("effect.repeat_last", "应用上一个效果"),
    ("effect.drop_shadow", "效果:投影"),
    ("effect.inner_shadow", "效果:内阴影"),
    ("effect.outer_glow", "效果:外发光"),
    ("effect.inner_glow", "效果:内发光"),
    ("effect.round_corners", "效果:圆角"),
    ("effect.gaussian_blur", "效果:高斯模糊"),
    ("effect.feather", "效果:羽化"),
    ("effect.distort", "扭曲和变换…"),
    ("view.hide_edges", "隐藏边缘"),
    ("view.browser_proof", "浏览器校对…"),
    ("window.workspace_basic", "工作区:基本功能"),
    ("window.workspace_type", "工作区:排版"),
    ("window.workspace_export", "工作区:导出"),
    ("window.new_workspace", "新建工作区…"),
    ("window.tab_properties", "面板坞:属性"),
    ("window.tab_layers", "面板坞:图层"),
    ("window.tab_artboards", "面板坞:画板"),
    ("window.tab_tokens", "面板坞:令牌"),
    ("help.shortcuts", "键位速查表…"),
    ("help.check_update", "检查更新…"),
    ("help.capabilities", "能力台账(做了什么/没做什么)"),
    // 阶段 6(工具箱四向停靠 + 单双列)
    ("view.dock_toolbar_top", "工具箱:停靠到顶部"),
    ("view.dock_toolbar_left", "工具箱:停靠到左侧"),
    ("view.dock_toolbar_right", "工具箱:停靠到右侧"),
    ("view.dock_toolbar_bottom", "工具箱:停靠到底部"),
    ("view.toolbar_columns_1", "工具箱:单列"),
    ("view.toolbar_columns_2", "工具箱:双列"),
    // 阶段 2(副文档 03)
    ("path.merge", "路径查找器:合并"),
    ("path.subtract_back", "路径查找器:减去后方对象"),
    ("path.crop", "路径查找器:裁剪"),
    ("view.toggle_transform_panel", "变换面板显隐"),
    ("view.toggle_align_panel", "对齐面板显隐"),
    ("align.to_selection", "对齐到:选区"),
    ("align.to_key_object", "对齐到:关键对象"),
    ("align.to_artboard", "对齐到:画板"),
    ("object.distribute_hspace", "水平等间隙分布"),
    ("object.distribute_vspace", "垂直等间隙分布"),
];

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

// ─────────────────────────── 菜单结构声明 ───────────────────────────

/// 菜单项。`id` 必须在 `IMPLEMENTED_IDS` 中;
/// 键位文本**自动查表**,禁止在 label 里手写。
#[derive(Debug, Clone, Copy)]
pub struct MenuItem {
    pub id: &'static str,
    pub label: &'static str,
}

/// 「已登记但尚未落地」的菜单项:id → 计划说明。
///
/// 阶段 5(副文档 06 §3)裁定:未实现项**保留在菜单里但置灰**,
/// 悬停即见「计划于 vX」——**绝不出现点了没反应的项**(`design/06 §七`)。
/// 这些 id **同样是已注册命令**(进 `IMPLEMENTED_IDS` / `commands.yaml`),
/// 因此 Agent 经 `run` 调用时也会拿到同一句提示,而不是静默失败。
pub const PLANNED: &[(&str, &str)] = &[
    ("file.close", "计划于 v2:多文档与关闭确认"),
    (
        "file.import_html",
        "计划于 v2:把外部 HTML 作为新画板导入(当前请用「打开项目」)",
    ),
    (
        "file.print",
        "计划于 v2:系统打印(当前请用「导出 → PDF」替代)",
    ),
    ("edit.preferences", "计划于 v2:首选项九分类面板"),
    (
        "edit.keyboard_shortcuts",
        "计划于 v2:键位自定义(当前键位见「帮助 → 命令搜索」)",
    ),
    (
        "object.clip_mask",
        "计划于 v2:剪切蒙版建模(`overflow:hidden` 之外的路径级裁剪)",
    ),
    ("object.release_clip_mask", "计划于 v2:释放剪切蒙版"),
    (
        "object.outline_stroke",
        "计划于 v2:轮廓化描边(当前请用视图菜单的轮廓模式查看线框)",
    ),
    (
        "text.create_outlines",
        "计划于 v2:文字转轮廓(需字形轮廓导出)",
    ),
    ("text.find_font", "计划于 v2:查找/替换缺失字体"),
    (
        "effect.distort",
        "计划于 v2:扭曲与变换效果(无 CSS 无损对应)",
    ),
    ("view.hide_edges", "计划于 v2:隐藏边缘(选中框细节开关)"),
    (
        "view.browser_proof",
        "计划于 v2:浏览器校对(接入 WPI 截图比对)",
    ),
    (
        "window.new_workspace",
        "计划于 v2:自定义工作区(当前提供三个内置工作区)",
    ),
    (
        "help.shortcuts",
        "计划于 v2:键位速查表(当前请用「命令搜索」)",
    ),
    ("help.check_update", "计划于 v2:检查更新"),
];

/// 该命令是否「已登记但尚未落地」;返回计划说明(菜单据此置灰 + 悬停提示)。
pub fn planned_reason(id: &str) -> Option<&'static str> {
    PLANNED.iter().find(|(i, _)| *i == id).map(|(_, m)| *m)
}

/// 菜单栏标题(AI 规范 9 项;V1 已确认 2026-09-20)。与 [`MENUS`] 一一对应。
pub const MENU_TITLES: [&str; 9] = [
    "文件", "编辑", "对象", "文字", "选择", "效果", "视图", "窗口", "帮助",
];

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
        id: "file.close",
        label: "关闭文档",
    },
    MenuItem {
        id: "file.import_html",
        label: "导入 HTML…",
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
        id: "file.print",
        label: "打印…",
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
    MenuItem {
        id: "edit.preferences",
        label: "首选项…",
    },
    MenuItem {
        id: "edit.keyboard_shortcuts",
        label: "键盘快捷键…",
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
    MenuItem {
        id: "path.merge",
        label: "合并",
    },
    MenuItem {
        id: "path.subtract_back",
        label: "减去后方对象",
    },
    MenuItem {
        id: "path.crop",
        label: "裁剪",
    },
    MenuItem {
        id: "object.distribute_h",
        label: "水平等距分布",
    },
    MenuItem {
        id: "object.distribute_v",
        label: "垂直等距分布",
    },
    MenuItem {
        id: "object.lock",
        label: "锁定所选对象",
    },
    MenuItem {
        id: "object.unlock_all",
        label: "解锁全部对象",
    },
    MenuItem {
        id: "object.hide",
        label: "隐藏所选对象",
    },
    MenuItem {
        id: "object.show_all",
        label: "显示全部对象",
    },
    MenuItem {
        id: "object.clip_mask",
        label: "建立剪切蒙版",
    },
    MenuItem {
        id: "object.release_clip_mask",
        label: "释放剪切蒙版",
    },
    MenuItem {
        id: "object.outline_stroke",
        label: "轮廓化描边",
    },
];

// ── 阶段 5 新增四个菜单(文字 / 选择 / 效果 / 窗口)──

/// 文字菜单(`design/03 §二`)。
pub const MENU_TYPE: &[MenuItem] = &[
    MenuItem {
        id: "view.toggle_char_panel",
        label: "字符",
    },
    MenuItem {
        id: "view.toggle_para_panel",
        label: "段落",
    },
    MenuItem {
        id: "tool.text_cycle_mode",
        label: "点文字 / 区域文字",
    },
    MenuItem {
        id: "text.upper_case",
        label: "更改大小写 → 大写",
    },
    MenuItem {
        id: "text.lower_case",
        label: "更改大小写 → 小写",
    },
    MenuItem {
        id: "text.create_outlines",
        label: "创建轮廓",
    },
    MenuItem {
        id: "text.find_font",
        label: "查找字体…",
    },
];

/// 选择菜单(`design/03 §二`)。
pub const MENU_SELECT: &[MenuItem] = &[
    MenuItem {
        id: "edit.select_all",
        label: "全部(当前画板)",
    },
    MenuItem {
        id: "canvas.cancel",
        label: "取消选择",
    },
    MenuItem {
        id: "select.inverse",
        label: "反向",
    },
    MenuItem {
        id: "select.next_object",
        label: "上方的下一个对象",
    },
    MenuItem {
        id: "select.prev_object",
        label: "下方的下一个对象",
    },
    MenuItem {
        id: "select.same_fill",
        label: "相同 → 填充色",
    },
    MenuItem {
        id: "select.same_stroke",
        label: "相同 → 描边色",
    },
    MenuItem {
        id: "select.same_stroke_width",
        label: "相同 → 描边粗细",
    },
    MenuItem {
        id: "select.all_text",
        label: "对象 → 全部文本对象",
    },
    MenuItem {
        id: "select.all_locked",
        label: "对象 → 所有锁定对象",
    },
    MenuItem {
        id: "select.all_hidden",
        label: "对象 → 所有隐藏对象",
    },
];

/// 效果菜单(`design/06 §4.6` 映射表;每条落 CSS 见外观面板)。
pub const MENU_EFFECT: &[MenuItem] = &[
    MenuItem {
        id: "effect.repeat_last",
        label: "应用上一个效果",
    },
    MenuItem {
        id: "effect.drop_shadow",
        label: "风格化 → 投影",
    },
    MenuItem {
        id: "effect.inner_shadow",
        label: "风格化 → 内阴影",
    },
    MenuItem {
        id: "effect.outer_glow",
        label: "风格化 → 外发光",
    },
    MenuItem {
        id: "effect.inner_glow",
        label: "风格化 → 内发光",
    },
    MenuItem {
        id: "effect.round_corners",
        label: "风格化 → 圆角",
    },
    MenuItem {
        id: "effect.gaussian_blur",
        label: "模糊 → 高斯模糊",
    },
    MenuItem {
        id: "effect.feather",
        label: "羽化…",
    },
    MenuItem {
        id: "effect.distort",
        label: "扭曲和变换…",
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
    MenuItem {
        id: "view.next_artboard",
        label: "下一画板",
    },
    MenuItem {
        id: "view.prev_artboard",
        label: "上一画板",
    },
    MenuItem {
        id: "view.zoom_to_selection",
        label: "缩放到选区",
    },
    MenuItem {
        id: "view.toggle_rulers",
        label: "显示标尺",
    },
    MenuItem {
        id: "view.toggle_guides",
        label: "显示参考线",
    },
    MenuItem {
        id: "view.lock_guides",
        label: "锁定参考线",
    },
    MenuItem {
        id: "view.guides_from_selection",
        label: "从选区生成参考线",
    },
    MenuItem {
        id: "view.hide_edges",
        label: "隐藏边缘",
    },
    MenuItem {
        id: "view.browser_proof",
        label: "浏览器校对…",
    },
];

/// 窗口菜单(`design/03 §二`):工作区 + 面板坞 Tab + 各面板显隐。
///
/// 面板项与副文档 02 的 `F 键` **同源**(同一批 `view.toggle_*` 命令),
/// 因此键位文本自动出现在右侧(`menu_label` 查注册表,禁止手写)。
pub const MENU_WINDOW: &[MenuItem] = &[
    MenuItem {
        id: "window.workspace_basic",
        label: "工作区 → 基本功能",
    },
    MenuItem {
        id: "window.workspace_type",
        label: "工作区 → 排版",
    },
    MenuItem {
        id: "window.workspace_export",
        label: "工作区 → 导出",
    },
    MenuItem {
        id: "window.new_workspace",
        label: "新建工作区…",
    },
    MenuItem {
        id: "view.dock_toolbar_top",
        label: "工具箱 → 停靠到顶部",
    },
    MenuItem {
        id: "view.dock_toolbar_left",
        label: "工具箱 → 停靠到左侧",
    },
    MenuItem {
        id: "view.dock_toolbar_right",
        label: "工具箱 → 停靠到右侧",
    },
    MenuItem {
        id: "view.dock_toolbar_bottom",
        label: "工具箱 → 停靠到底部",
    },
    MenuItem {
        id: "view.toolbar_columns_1",
        label: "工具箱 → 单列",
    },
    MenuItem {
        id: "view.toolbar_columns_2",
        label: "工具箱 → 双列",
    },
    MenuItem {
        id: "view.toggle_transform_panel",
        label: "变换面板",
    },
    MenuItem {
        id: "view.toggle_align_panel",
        label: "对齐面板",
    },
    MenuItem {
        id: "align.to_selection",
        label: "对齐到 → 选区",
    },
    MenuItem {
        id: "align.to_key_object",
        label: "对齐到 → 关键对象",
    },
    MenuItem {
        id: "align.to_artboard",
        label: "对齐到 → 画板",
    },
    MenuItem {
        id: "window.tab_properties",
        label: "属性",
    },
    MenuItem {
        id: "window.tab_layers",
        label: "图层",
    },
    MenuItem {
        id: "window.tab_artboards",
        label: "画板",
    },
    MenuItem {
        id: "window.tab_tokens",
        label: "令牌",
    },
    MenuItem {
        id: "view.toggle_char_panel",
        label: "字符面板",
    },
    MenuItem {
        id: "view.toggle_para_panel",
        label: "段落面板",
    },
    MenuItem {
        id: "view.toggle_appearance_panel",
        label: "外观面板",
    },
    MenuItem {
        id: "view.toggle_stroke_panel",
        label: "描边面板",
    },
    MenuItem {
        id: "view.toggle_gradient_panel",
        label: "渐变面板",
    },
    MenuItem {
        id: "view.toggle_opacity_panel",
        label: "透明度面板",
    },
    MenuItem {
        id: "view.toggle_color_panel",
        label: "颜色面板",
    },
    MenuItem {
        id: "view.toggle_layers_panel",
        label: "图层面板显隐",
    },
    MenuItem {
        id: "view.toggle_all_panels",
        label: "隐藏所有面板",
    },
];

pub const MENU_HELP: &[MenuItem] = &[
    MenuItem {
        id: "app.command_palette",
        label: "命令搜索…",
    },
    MenuItem {
        id: "help.shortcuts",
        label: "键位速查表…",
    },
    MenuItem {
        id: "help.check_update",
        label: "检查更新…",
    },
    MenuItem {
        id: "help.capabilities",
        label: "能力台账…",
    },
    MenuItem {
        id: "app.about",
        label: "关于",
    },
];

/// 全部菜单声明(门禁自检用)。**顺序与 [`MENU_TITLES`] 一一对应**。
pub const MENUS: &[&[MenuItem]] = &[
    MENU_FILE,
    MENU_EDIT,
    MENU_OBJECT,
    MENU_TYPE,
    MENU_SELECT,
    MENU_EFFECT,
    MENU_VIEW,
    MENU_WINDOW,
    MENU_HELP,
];

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
