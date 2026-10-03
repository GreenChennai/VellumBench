//! 选中态(纯数据,零 UI 依赖)。
//!
//! # R0 转正批次(22 篇 §4 R0 交付 4)
//!
//! 骨架批的 `BTreeSet<String>` 形状在对照旧宿主现实后**让位**:
//! `vb_app::VellumApp::selection` 实际是 `Vec<String>` —— **点击序即语义**
//! (`selection.last()` 是属性面板的"主选中对象",`selection[0..1]` 参与
//! 布尔运算的主/次序,框选结果按命中序写入)。无序集合会静默改掉这些
//! 行为,违反"提取 = 零行为变化"。本批起 [`SelectionState`] 为**有序**
//! 选中集,并成为 `VellumApp.selection` 的实际字段类型(旧宿主裸 Vec
//! 字段转正为会话层类型;读取经 `Deref`/`IntoIterator` 透明透出 Vec 语义)。
//!
//! 修饰键快照([`ModifierState`])独立存在:旧宿主的修饰键来自每帧事件,
//! 不落在选中态里,故不再作为 SelectionState 的字段;多选语义裁决
//! (22 篇 §4 R3)仍经 [`ModifierState::is_additive`] 收口。
//!
//! id 采用 `String`(稳定 sid):与 `vb_app` 的 sid 同源同形;本类型
//! **不隐式去重**(沿用旧宿主"写入前自查 contains"的口径),遍历序 =
//! 插入序,渲染/测试可复现。

use serde::{Deserialize, Serialize};

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

/// 选中态:**有序**稳定 sid 列表(点击序/命中序)+ 纯数据。
///
/// 读取面:经 `Deref<Target = Vec<String>>` 透明透出(`len`/`iter`/
/// `contains`/`last`/索引切片等与裸 Vec 完全同形);写入面走下列方法,
/// 保持"空 id 忽略、增选查重"的旧宿主口径。
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SelectionState {
    sids: Vec<String>,
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
        self.sids.clear();
        self.sids.push(sid);
    }

    /// 增量加入(Shift 点选/多选;已存在或空串不动;返回是否真的新增)。
    pub fn add(&mut self, sid: impl Into<String>) -> bool {
        let sid = sid.into();
        if sid.is_empty() || self.sids.contains(&sid) {
            return false;
        }
        self.sids.push(sid);
        true
    }

    /// 批量加入(返回新增个数)。
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
        if let Some(pos) = self.sids.iter().position(|s| s == sid) {
            self.sids.remove(pos);
            false
        } else if !sid.is_empty() {
            self.sids.push(sid.to_string());
            true
        } else {
            false
        }
    }

    /// 移除(按序保留其余;返回是否真的移除)。
    pub fn remove(&mut self, sid: &str) -> bool {
        match self.sids.iter().position(|s| s == sid) {
            Some(pos) => {
                self.sids.remove(pos);
                true
            }
            None => false,
        }
    }

    /// 清空(返回清除个数;Esc 取消选择语义)。
    /// (遮蔽 `Vec::clear` 仅为本模块语义补计数;调用点忽略返回值即可。)
    pub fn clear(&mut self) -> usize {
        let n = self.sids.len();
        self.sids.clear();
        n
    }

    /// 整批替换(框选/粘贴/撤销恢复等"一次算好再写入"的路径)。
    pub fn set_all(&mut self, sids: Vec<String>) {
        self.sids = sids;
    }

    /// 成员判定(&str 入参,免 `&String` 包装;与 `contains` 同义)。
    pub fn is_selected(&self, sid: &str) -> bool {
        self.sids.iter().any(|s| s == sid)
    }

    /// 取出列表(会话态所有权转移;如 `std::mem::take` 后仍需 Vec 的路径)。
    pub fn into_vec(self) -> Vec<String> {
        self.sids
    }

    pub fn as_slice(&self) -> &[String] {
        &self.sids
    }
}

impl From<Vec<String>> for SelectionState {
    fn from(sids: Vec<String>) -> Self {
        Self { sids }
    }
}

// 与裸 Vec 的直接比较(旧宿主测试/调用点 `assert_eq!(selection, vec![…])`
// 的表达式形状保持零改动;语义 = 有序 sid 列表全等)。
impl PartialEq<Vec<String>> for SelectionState {
    fn eq(&self, other: &Vec<String>) -> bool {
        self.sids == *other
    }
}

impl PartialEq<SelectionState> for Vec<String> {
    fn eq(&self, other: &SelectionState) -> bool {
        *self == other.sids
    }
}

impl From<SelectionState> for Vec<String> {
    fn from(s: SelectionState) -> Self {
        s.sids
    }
}

impl std::ops::Deref for SelectionState {
    type Target = Vec<String>;

    fn deref(&self) -> &Vec<String> {
        &self.sids
    }
}

impl std::ops::DerefMut for SelectionState {
    fn deref_mut(&mut self) -> &mut Vec<String> {
        &mut self.sids
    }
}

// `for sid in &selection` 与旧宿主裸 Vec 写法零差异(方法解析不跨 Deref
// 找 IntoIterator,必须显式实现才能保住既有 for 循环形状)。
impl IntoIterator for SelectionState {
    type Item = String;
    type IntoIter = std::vec::IntoIter<String>;

    fn into_iter(self) -> Self::IntoIter {
        self.sids.into_iter()
    }
}

impl<'a> IntoIterator for &'a SelectionState {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.sids.iter()
    }
}

impl<'a> IntoIterator for &'a mut SelectionState {
    type Item = &'a mut String;
    type IntoIter = std::slice::IterMut<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.sids.iter_mut()
    }
}

// 注意:**不派生 Clone** —— `.clone()` 经 Deref 解析到 `Vec::clone`,
// 与旧宿主 `selection.clone()` 得到 `Vec<String>` 的口径逐字节一致
// (整装克隆会话态本就不该是默认能力,需要时 `to_vec()` + `From`)。

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
    fn click_order_is_preserved_not_sorted() {
        // 现实口径:点击序即语义(last = 主选中对象;布尔运算按 [0]/[1] 序)
        let mut s = SelectionState::new();
        s.add("c");
        s.add("a");
        s.add("b");
        assert_eq!(s.as_slice(), ["c", "a", "b"]);
        assert_eq!(s.first(), Some(&"c".to_string()));
        assert_eq!(s.last(), Some(&"b".to_string()));
    }

    #[test]
    fn add_and_extend_report_new_items_only() {
        let mut s = SelectionState::new();
        assert!(s.add("a"));
        assert!(!s.add("a"));
        assert_eq!(s.extend(["b", "a", "c"]), 2);
        assert_eq!(s.len(), 3);
        assert_eq!(s.to_vec(), ["a", "b", "c"]);
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
    fn remove_keeps_order_of_rest() {
        let mut s = SelectionState::new();
        s.extend(["a", "b", "c"]);
        assert!(s.remove("b"));
        assert!(!s.remove("b"));
        assert_eq!(s.as_slice(), ["a", "c"]);
    }

    #[test]
    fn set_all_replaces_and_vec_roundtrip() {
        let mut s = SelectionState::new();
        s.select("old");
        s.set_all(vec!["n1".into(), "n2".into()]);
        assert_eq!(s.as_slice(), ["n1", "n2"]);
        // From<Vec>/Into<Vec>:旧宿主"整批赋值"路径的形状
        let s2: SelectionState = vec!["x".to_string()].into();
        assert_eq!(s2.as_slice(), ["x"]);
        let v: Vec<String> = s2.into();
        assert_eq!(v, vec!["x".to_string()]);
    }

    #[test]
    fn deref_reads_like_bare_vec() {
        let mut s = SelectionState::new();
        s.extend(["n1", "n2"]);
        // 索引 / contains / 迭代:与旧宿主裸 Vec 完全同形
        assert_eq!(s[1], "n2");
        assert!(s.contains(&"n1".to_string()));
        assert_eq!(s.iter().count(), 2);
        // for 循环(& 与值两种)不因新类型而变
        let mut seen = Vec::new();
        for sid in &s {
            seen.push(sid.clone());
        }
        assert_eq!(seen, ["n1".to_string(), "n2".to_string()]);
        let mut owned = Vec::new();
        for sid in std::mem::take(&mut s).into_vec() {
            owned.push(sid);
        }
        assert_eq!(owned, ["n1".to_string(), "n2".to_string()]);
    }

    #[test]
    fn modifier_snapshot_is_carried() {
        // ModifierState 独立于选中态(旧宿主修饰键来自每帧事件)
        let m = ModifierState {
            shift: true,
            ..Default::default()
        };
        assert!(m.is_additive());
        assert!(!ModifierState::default().is_additive());
        assert!(ModifierState {
            platform: true,
            ..Default::default()
        }
        .is_additive());
    }

    #[test]
    fn serde_roundtrip_is_transparent_to_vec_shape() {
        let mut s = SelectionState::new();
        s.extend(["n1", "n2"]);
        let json = serde_json::to_string(&s).expect("serialize");
        // serde(transparent):线型与裸 Vec<String> 完全一致(未来落盘不换形)
        assert_eq!(json, r#"["n1","n2"]"#);
        let back: SelectionState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, s);
    }
}
