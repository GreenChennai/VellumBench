//! 选中态(纯数据 + 纯函数,零 UI 依赖)。
//!
//! **vb_app 全量迁移是后续批次,本批只立骨架**(22 篇 §4 R0 交付 4):
//! 旧宿主的框选命中、画布手势、smart guide 联动等仍在其内,按轮次迁入;
//! 本模块先固化"选中集 + 修饰键快照"这一最小状态形状,供 `vb_kit`/`vb_shell`
//! 以投影方式消费,避免新宿主再摸旧宿主私有字段。
//!
//! id 采用 `String`(稳定 sid):与 `vb_app::VellumApp::selection: Vec<String>`
//! 同源同形;集合语义用 `BTreeSet` 保证遍历序稳定(可复现渲染/测试)。

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 修饰键状态快照(手势发生瞬间的键面状态;纯数据,不持任何事件句柄)。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModifierState {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    /// 平台键(macOS Cmd / Windows Win);跨宿主统一命名,是否参与
    /// 多选裁决由调用方语义决定。
    pub platform: bool,
}

impl ModifierState {
    /// 增选语义判定(Shift 或平台键按下的点选 = 增量多选)。
    ///
    /// 22 篇 §4 R3 交互裁决的先声:多选修饰键语义在此收口,新旧宿主共用。
    pub fn is_additive(self) -> bool {
        self.shift || self.platform
    }
}

/// 选中态:稳定 sid 集合 + 修饰键快照。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectionState {
    selected: BTreeSet<String>,
    modifiers: ModifierState,
}

impl SelectionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// 单选(替换全部选中;空串忽略)。
    pub fn select(&mut self, sid: impl Into<String>) {
        let sid = sid.into();
        if sid.is_empty() {
            return;
        }
        self.selected.clear();
        self.selected.insert(sid);
    }

    /// 增量加入(框选/多选;返回是否真的新增)。
    pub fn add(&mut self, sid: impl Into<String>) -> bool {
        let sid = sid.into();
        if sid.is_empty() {
            return false;
        }
        self.selected.insert(sid)
    }

    /// 批量加入(框选;返回新增个数)。
    pub fn extend<I, S>(&mut self, ids: I) -> usize
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut added = 0;
        for sid in ids {
            if self.add(sid) {
                added += 1;
            }
        }
        added
    }

    /// 切换(点选已选项 = 取消;返回切换后的选中状态)。
    pub fn toggle(&mut self, sid: &str) -> bool {
        if self.selected.remove(sid) {
            false
        } else if !sid.is_empty() {
            self.selected.insert(sid.to_string());
            true
        } else {
            false
        }
    }

    /// 移除(返回是否真的移除)。
    pub fn remove(&mut self, sid: &str) -> bool {
        self.selected.remove(sid)
    }

    /// 清空(返回清除个数;Esc 取消选择语义)。
    pub fn clear(&mut self) -> usize {
        let n = self.selected.len();
        self.selected.clear();
        n
    }

    pub fn is_selected(&self, sid: &str) -> bool {
        self.selected.contains(sid)
    }

    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// 按稳定序遍历(渲染/测试可复现)。
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.selected.iter().map(String::as_str)
    }

    /// 首个选中项(锚点;属性面板"主选中对象"投影用)。
    pub fn first(&self) -> Option<&str> {
        self.selected.iter().next().map(String::as_str)
    }

    /// 最近一次手势的修饰键快照。
    pub fn modifiers(&self) -> ModifierState {
        self.modifiers
    }

    pub fn set_modifiers(&mut self, m: ModifierState) {
        self.modifiers = m;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_replaces_previous() {
        let mut s = SelectionState::new();
        s.select("a");
        s.select("b");
        assert_eq!(s.len(), 1);
        assert!(s.is_selected("b"));
        assert!(!s.is_selected("a"));
    }

    #[test]
    fn add_and_extend_report_new_items_only() {
        let mut s = SelectionState::new();
        assert!(s.add("a"));
        assert!(!s.add("a"));
        assert_eq!(s.extend(["b", "a", "c"]), 2);
        assert_eq!(s.len(), 3);
        assert_eq!(s.iter().collect::<Vec<_>>(), ["a", "b", "c"]);
    }

    #[test]
    fn toggle_flips_membership() {
        let mut s = SelectionState::new();
        s.select("a");
        assert!(!s.toggle("a"));
        assert!(s.is_empty());
        assert!(s.toggle("a"));
        assert!(s.is_selected("a"));
    }

    #[test]
    fn clear_returns_count() {
        let mut s = SelectionState::new();
        s.extend(["x", "y"]);
        assert_eq!(s.clear(), 2);
        assert_eq!(s.clear(), 0);
    }

    #[test]
    fn empty_ids_are_ignored() {
        let mut s = SelectionState::new();
        s.select("");
        assert!(!s.add(""));
        assert!(!s.toggle(""));
        assert!(s.is_empty());
    }

    #[test]
    fn modifier_snapshot_is_carried() {
        let mut s = SelectionState::new();
        assert!(!s.modifiers().is_additive());
        s.set_modifiers(ModifierState {
            shift: true,
            ..Default::default()
        });
        assert!(s.modifiers().is_additive());
    }

    #[test]
    fn first_is_stable_minimum() {
        let mut s = SelectionState::new();
        s.extend(["c", "a", "b"]);
        assert_eq!(s.first(), Some("a"));
    }

    #[test]
    fn serde_roundtrip() {
        let mut s = SelectionState::new();
        s.extend(["n1", "n2"]);
        s.set_modifiers(ModifierState {
            alt: true,
            ..Default::default()
        });
        let json = serde_json::to_string(&s).expect("serialize");
        let back: SelectionState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, s);
    }
}
