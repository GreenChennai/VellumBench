//! 阶段 5(AI 规范 9 项菜单)新增命令的实现(副文档 06)。
//!
//! 单独成文件的原因:`app.rs` 有「≤2000 行」红线,菜单批次的命令数量较多。
//! `run_command` 的兜底分支转发到本文件(未落地的 id 不再 panic,而是走
//! **统一计划提示**)。
//!
//! 纪律:
//! 1. **每个已注册 id 都必须被处理**(否则返回 `false` 由调用方报错);
//! 2. 未落地的功能**不静默** —— 统一 `toast_warn(计划说明)`,说明单一真相在
//!    `shortcuts::PLANNED`;
//! 3. **可测边界**:选择类命令的判据全部沉为接收 `&Document` 的**纯函数**
//!    (见 [`same_targets`] / [`all_of_targets`] / [`inverse_targets`]),
//!    `VellumApp` 方法只负责把结果写进 `selection`。

use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeId, NodeKind};

use crate::app::appearance::{self, Effect};
use crate::app::{color_panel, panels, VellumApp};
use crate::shortcuts;

// ─────────────────────────── 效果默认参数 ───────────────────────────
// 与外观面板(`appearance/ui.rs`)同一组默认值;均为**文档内容色**,非 UI 皮肤。

fn default_drop_shadow() -> Effect {
    Effect::DropShadow {
        x: 4.0,
        y: 4.0,
        blur: 8.0,
        spread: 0.0,
        color: "#00000066".into(), // vb-token-ok: 投影默认色(文档内容)
    }
}

fn default_inner_shadow() -> Effect {
    Effect::InnerShadow {
        x: 0.0,
        y: 2.0,
        blur: 6.0,
        spread: 0.0,
        color: "#00000066".into(), // vb-token-ok: 内阴影默认色(文档内容)
    }
}

fn default_outer_glow() -> Effect {
    Effect::OuterGlow {
        blur: 12.0,
        color: "#2e86ff80".into(), // vb-token-ok: 外发光默认色(文档内容)
    }
}

fn default_inner_glow() -> Effect {
    Effect::InnerGlow {
        blur: 10.0,
        color: "#ffffff40".into(), // vb-token-ok: 内发光默认色(文档内容)
    }
}

// ─────────────────────────── 选择判据(纯函数) ───────────────────────────

/// 「相同 →」的判据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameKey {
    Fill,
    Stroke,
    StrokeWidth,
}

/// 「对象 →」的判据。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllOf {
    Text,
    Locked,
    Hidden,
}

impl AllOf {
    fn hit(self, n: &vb_doc::model::Node) -> bool {
        match self {
            AllOf::Text => matches!(n.kind, NodeKind::Text { .. }),
            AllOf::Locked => n.locked,
            AllOf::Hidden => n.hidden,
        }
    }

    fn label(self) -> &'static str {
        match self {
            AllOf::Text => "文本对象",
            AllOf::Locked => "锁定对象",
            AllOf::Hidden => "隐藏对象",
        }
    }
}

/// 节点所属画板(向上找 `NodeKind::Artboard`)。
pub fn artboard_of(doc: &Document, id: NodeId) -> Option<NodeId> {
    let mut cur = Some(id);
    while let Some(c) = cur {
        let n = doc.nodes.get(c)?;
        if matches!(n.kind, NodeKind::Artboard) {
            return Some(c);
        }
        cur = n.parent;
    }
    None
}

/// 子树 DFS(前序,含 `root` 自身)。
pub fn subtree(doc: &Document, root: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        out.push(id);
        if let Some(n) = doc.nodes.get(id) {
            for &c in n.children.iter().rev() {
                stack.push(c);
            }
        }
    }
    out
}

/// 可选中的对象:排除画板自身与 `#text` 叶(它们不参与对象级选择)。
fn selectable(doc: &Document, id: NodeId) -> bool {
    doc.nodes
        .get(id)
        .map(|n| !matches!(n.kind, NodeKind::Artboard) && n.tag != "#text")
        .unwrap_or(false)
}

/// 文档内全部可选对象(按画板序,画板内 DFS 序)。
fn all_selectable(doc: &Document) -> Vec<String> {
    doc.artboards
        .clone()
        .into_iter()
        .flat_map(|ab| subtree(doc, ab))
        .filter(|nid| selectable(doc, *nid))
        .filter_map(|nid| doc.nodes.get(nid).map(|n| n.sid.as_str().to_string()))
        .collect()
}

/// 判据取值(取不到 → `None`,该对象不参与"相同")。
fn same_value(doc: &Document, sid: &str, key: SameKey) -> Option<String> {
    match key {
        SameKey::Fill => color_panel::read_target_color(doc, sid, false),
        SameKey::Stroke => color_panel::read_target_color(doc, sid, true),
        SameKey::StrokeWidth => doc
            .find_by_sid(sid)
            .and_then(|nid| doc.nodes.get(nid))
            .and_then(|n| {
                n.style_get("border-width")
                    .or_else(|| n.style_get("stroke-width"))
                    .map(|v| v.to_string())
            }),
    }
}

/// 「选择相同 →」:返回与 `base` 同值的全部可选对象(含 base 自身)。
/// `base` 无该属性 → 空表(调用方据此提示)。
pub fn same_targets(doc: &Document, base: &str, key: SameKey) -> Vec<String> {
    let Some(want) = same_value(doc, base, key) else {
        return Vec::new();
    };
    all_selectable(doc)
        .into_iter()
        .filter(|sid| same_value(doc, sid, key).as_deref() == Some(want.as_str()))
        .collect()
}

/// 「对象 → 全部文本/锁定/隐藏」。
pub fn all_of_targets(doc: &Document, which: AllOf) -> Vec<String> {
    all_selectable(doc)
        .into_iter()
        .filter(|sid| {
            doc.find_by_sid(sid)
                .and_then(|nid| doc.nodes.get(nid))
                .map(|n| which.hit(n))
                .unwrap_or(false)
        })
        .collect()
}

/// 「反向」:参照对象所在画板内、当前**未选中**的可选对象。
/// 无选区时参照 `doc.artboards[0]`。
pub fn inverse_targets(doc: &Document, selected: &[String]) -> Vec<String> {
    let ab = selected
        .last()
        .and_then(|s| doc.find_by_sid(s))
        .and_then(|id| artboard_of(doc, id))
        .or_else(|| doc.artboards.first().copied());
    let Some(ab) = ab else {
        return Vec::new();
    };
    subtree(doc, ab)
        .into_iter()
        .filter(|id| selectable(doc, *id))
        .filter_map(|id| doc.nodes.get(id).map(|n| n.sid.as_str().to_string()))
        .filter(|sid| !selected.contains(sid))
        .collect()
}

/// 「上/下移一个对象」:同画板内 DFS 序循环,返回下一项。
pub fn step_target(
    doc: &Document,
    selected: &[String],
    forward: bool,
) -> Option<(String, usize, usize)> {
    let ab = selected
        .last()
        .and_then(|s| doc.find_by_sid(s))
        .and_then(|id| artboard_of(doc, id))
        .or_else(|| doc.artboards.first().copied())?;
    let list: Vec<String> = subtree(doc, ab)
        .into_iter()
        .filter(|id| selectable(doc, *id))
        .filter_map(|id| doc.nodes.get(id).map(|n| n.sid.as_str().to_string()))
        .collect();
    if list.is_empty() {
        return None;
    }
    let cur = selected
        .last()
        .and_then(|s| list.iter().position(|x| x == s));
    let next = match cur {
        Some(i) if forward => (i + 1) % list.len(),
        Some(i) => (i + list.len() - 1) % list.len(),
        None => 0,
    };
    Some((list[next].clone(), next, list.len()))
}

// ─────────────────────────── 命令分发 ───────────────────────────

impl VellumApp {
    /// `run_command` 兜底转发:处理阶段 5 菜单新增的命令。
    /// 返回 `false` 表示该 id 确实没有实现(由调用方报错并 debug_assert)。
    pub(crate) fn run_menu_command(&mut self, id: &str) -> bool {
        // 计划项:统一提示(文案单一真相在 `shortcuts::PLANNED`)
        if let Some(reason) = shortcuts::planned_reason(id) {
            self.toast_warn(reason);
            return true;
        }
        match id {
            // ── 文字 ──
            // 09-O(05-4-A2):查找/替换缺失字体(打开专项对话框;
            // 打开文档时的自动检测在 assemble::construct)
            "text.find_font" => {
                let missing = crate::app::font_dialog::scan_missing_fonts(&self.doc);
                if missing.is_empty() {
                    self.say("查找字体:文档没有缺失字体(判定口径见对话框说明)");
                } else {
                    self.font_dialog = Some(crate::app::font_dialog::FontDialogState::new(missing));
                    self.say("查找字体:发现缺失字体,已打开替换对话框");
                }
                true
            }
            "text.upper_case" | "text.lower_case" => {
                let upper = id == "text.upper_case";
                let mut n = 0;
                for sid in self.selection.clone() {
                    let Some(nid) = self.doc.find_by_sid(&sid) else {
                        continue;
                    };
                    let cur = match &self.doc.nodes.get(nid).unwrap().kind {
                        NodeKind::Text { text, .. } => text.clone(),
                        _ => continue,
                    };
                    let new = if upper {
                        cur.to_uppercase()
                    } else {
                        cur.to_lowercase()
                    };
                    if new != cur {
                        self.exec(Command::SetText {
                            sid: sid.clone(),
                            new,
                            old: None,
                        });
                        n += 1;
                    }
                }
                if n == 0 {
                    self.toast_warn("更改大小写:选中对象里没有可改的文本");
                } else {
                    self.say(format!("已更改 {n} 个文本对象的大小写"));
                }
                true
            }
            // ── 选择 ──
            "select.inverse" => {
                let before = self.selection.len();
                let hits = inverse_targets(&self.doc, &self.selection);
                self.selection = hits;
                self.say(format!(
                    "反向选择:{before} → {} 个对象",
                    self.selection.len()
                ));
                true
            }
            "select.next_object" | "select.prev_object" => {
                match step_target(&self.doc, &self.selection, id == "select.next_object") {
                    Some((sid, i, n)) => {
                        self.selection = vec![sid];
                        self.say(format!("已选中 {} / {}", i + 1, n));
                    }
                    None => self.say("选择:当前画板没有可选对象"),
                }
                true
            }
            "select.same_fill" | "select.same_stroke" | "select.same_stroke_width" => {
                let Some(base) = self.selection.last().cloned() else {
                    self.toast_warn("选择相同:先选中一个参照对象");
                    return true;
                };
                let key = match id {
                    "select.same_fill" => SameKey::Fill,
                    "select.same_stroke" => SameKey::Stroke,
                    _ => SameKey::StrokeWidth,
                };
                let hits = same_targets(&self.doc, &base, key);
                if hits.is_empty() {
                    self.toast_warn("选择相同:参照对象没有该属性");
                } else {
                    let n = hits.len();
                    self.selection = hits;
                    self.say(format!("选择相同:{n} 个对象"));
                }
                true
            }
            "select.all_text" | "select.all_locked" | "select.all_hidden" => {
                let which = match id {
                    "select.all_text" => AllOf::Text,
                    "select.all_locked" => AllOf::Locked,
                    _ => AllOf::Hidden,
                };
                let hits = all_of_targets(&self.doc, which);
                let n = hits.len();
                self.selection = hits;
                if n == 0 {
                    self.say(format!("没有{}", which.label()));
                } else {
                    self.say(format!("已选中 {n} 个{}", which.label()));
                }
                true
            }
            // ── 效果 ──
            "effect.repeat_last" => {
                match self.last_effect.clone() {
                    Some(e) => self.apply_effect(e),
                    None => self
                        .toast_warn("应用上一个效果:还没有可重复的效果(先用「效果」菜单添加一个)"),
                }
                true
            }
            "effect.drop_shadow" => self.remember_and_apply(default_drop_shadow()),
            "effect.inner_shadow" => self.remember_and_apply(default_inner_shadow()),
            "effect.outer_glow" => self.remember_and_apply(default_outer_glow()),
            "effect.inner_glow" => self.remember_and_apply(default_inner_glow()),
            "effect.round_corners" => self.remember_and_apply(Effect::RoundCorners { radius: 8.0 }),
            "effect.gaussian_blur" => self.remember_and_apply(Effect::GaussianBlur { radius: 4.0 }),
            "effect.feather" => self.remember_and_apply(Effect::Feather { radius: 12.0 }),
            // ── 窗口:工作区 ──
            // X-7(05-4-A2):新建工作区对话框(保存当前布局为命名预设 +
            // 用户预设列表切换/删除;预设落 workspace.json workspace_presets)
            "window.new_workspace" => {
                self.workspace_dialog_open = true;
                self.say("工作区:可保存当前布局为命名预设,并可切换/删除");
                true
            }
            // 工作区预设与 `workspace.json` 联动(副文档 07-4-2):
            // 预设会**覆盖**工具栏停靠位(这正是"工作区"的意义),并立即落盘。
            // 04-2:预设同步次级面板坞(停靠/组选择),不再出现级联浮窗。
            "window.workspace_basic" => {
                self.panels_hidden = false;
                self.dock_collapsed = false;
                self.panel_tab = panels::TAB_PROPERTIES;
                // 九面板全部关闭 + 全部停靠(下次打开从次级坞出现)
                for p in super::panel_dock::SecPanel::ALL {
                    self.sec_set_open(p, false);
                    self.sec.floating[p.index()] = false;
                }
                self.toolbar_dock = super::dock_layout::DockSide::Left;
                self.toolbar_columns = 1;
                self.say("工作区:基本功能(工具箱在左 + 属性面板)");
                self.save_workspace();
                true
            }
            "window.workspace_type" => {
                self.panels_hidden = false;
                self.dock_collapsed = false;
                self.panel_tab = panels::TAB_PROPERTIES;
                for p in super::panel_dock::SecPanel::ALL {
                    self.sec_set_open(p, false);
                    self.sec.floating[p.index()] = false;
                }
                // 排版工作区:字符 + 段落停靠打开,聚焦文字组
                self.sec_set_open(super::panel_dock::SecPanel::Char, true);
                self.sec_set_open(super::panel_dock::SecPanel::Para, true);
                self.sec_focus(super::panel_dock::SecPanel::Char);
                self.toolbar_dock = super::dock_layout::DockSide::Left;
                self.toolbar_columns = 1;
                self.say("工作区:排版(工具箱在左 + 字符/段落面板停靠)");
                self.save_workspace();
                true
            }
            "window.workspace_export" => {
                self.show_export = true;
                for p in super::panel_dock::SecPanel::ALL {
                    self.sec_set_open(p, false);
                    self.sec.floating[p.index()] = false;
                }
                self.toolbar_dock = super::dock_layout::DockSide::Bottom;
                self.toolbar_columns = 1;
                self.say("工作区:导出(工具箱在底 + 导出对话框)");
                self.save_workspace();
                true
            }
            // ── 窗口:面板坞 Tab(与 F4/F7 同一批面板)──
            "window.tab_properties"
            | "window.tab_layers"
            | "window.tab_artboards"
            | "window.tab_tokens" => {
                let tab = match id {
                    "window.tab_layers" => panels::TAB_LAYERS,
                    "window.tab_artboards" => panels::TAB_ARTBOARDS,
                    "window.tab_tokens" => panels::TAB_TOKENS,
                    _ => panels::TAB_PROPERTIES,
                };
                self.panels_hidden = false;
                self.dock_collapsed = false;
                self.panel_tab = tab;
                self.say(format!("面板坞:{}", panels::TAB_LABELS[tab]));
                true
            }
            _ => false,
        }
    }

    fn remember_and_apply(&mut self, e: Effect) -> bool {
        self.last_effect = Some(e.clone());
        self.apply_effect(e);
        true
    }

    /// 对全部选中对象追加一条效果(经外观模型;失败给中文提示)。
    fn apply_effect(&mut self, e: Effect) {
        if self.selection.is_empty() {
            self.toast_warn("效果:先选中一个对象");
            return;
        }
        let mut ok = 0;
        let mut err: Option<String> = None;
        for sid in self.selection.clone() {
            match appearance::add_effect_cmd(&self.doc, &sid, e.clone()) {
                Ok(Some(cmd)) => {
                    self.exec(cmd);
                    ok += 1;
                }
                Ok(None) => {}
                Err(msg) => err = Some(msg),
            }
        }
        if let Some(msg) = err {
            self.toast_warn(msg);
        } else if ok > 0 {
            self.say(format!("已为 {ok} 个对象添加效果"));
        }
    }
}
