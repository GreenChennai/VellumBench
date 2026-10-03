//! 启动器纯逻辑(22 篇 §4 R0 交付 1;ADR-0033 启动器窗口)。
//!
//! 只做数据与状态机,零 UI 依赖(不 import GPUI):MRU 展示序(固定项在先、
//! 其余按最近使用倒序,与 [`vb_session::mru::RecentStore::sorted`] 同式)、
//! 搜索过滤(名称/路径包含,大小写不敏感;空串 = 全部)、键盘导航语义
//! (↑ 饱和上移、↓ 夹紧下移、搜索变更选中回零、列表缩短选中夹紧)——
//! 与旧宿主 `vb_app::launcher` 的 `filtered` / `home.select_next` /
//! `home.select_prev` 语义逐条对齐;GPUI 绘制在 bin(`main.rs`)。
//!
//! [`view_indices`] 返回 `RecentStore::items` 的**下标**而非引用:启动器的
//! "R 移除记录"要按路径回写删除,下标便于 UI 直接取行与定位。

use vb_session::mru::{RecentItem, RecentStore};

/// 启动器状态机(纯数据;不含任何宿主句柄)。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LauncherModel {
    /// 搜索过滤词(02-3-3:按名称/路径)。
    pub search: String,
    /// 过滤后展示列表的选中下标(键盘 ↑↓ / Enter 的作用对象)。
    pub selected: usize,
}

impl LauncherModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// 搜索词变更:词入状态 + **选中回零**(旧宿主 `resp.changed()` 语义:
    /// 过滤集变了,原下标指向的条目已无意义)。
    pub fn set_search(&mut self, q: impl Into<String>) {
        self.search = q.into();
        self.selected = 0;
    }

    /// ↑:选中上移(饱和,到顶停;空列表不动)。旧 `home.select_prev`。
    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    /// ↓:选中下移(到列表底停;空列表不动)。旧 `home.select_next`。
    pub fn select_next(&mut self, view_len: usize) {
        if view_len > 0 {
            self.selected = (self.selected + 1).min(view_len - 1);
        }
    }

    /// 列表缩短(过滤/移除)后夹紧选中;空列表归零。
    /// 旧宿主渲染前的 `selected.min(view.len() - 1)` 同义(空列表时旧代码
    /// 提前 return,此处显式归零,不 panic)。
    pub fn clamp(&mut self, view_len: usize) {
        self.selected = if view_len == 0 {
            0
        } else {
            self.selected.min(view_len - 1)
        };
    }

    /// 当前选中条目(展示序下标表 + 存储解析;空表/越界 = None)。
    pub fn selected_item<'a>(
        &self,
        view: &[usize],
        store: &'a RecentStore,
    ) -> Option<&'a RecentItem> {
        view.get(self.selected).and_then(|&i| store.items.get(i))
    }
}

/// 展示序下标表:`RecentStore::sorted()` 的展示序(固定项在先,其余按
/// 最近使用倒序;稳定排序)再按搜索词过滤。返回 `store.items` 的下标。
///
/// 过滤语义(02-3-3,与旧宿主 `filtered` 一致):搜索词 trim 后按名称或
/// 路径**包含**匹配,大小写不敏感;空串 = 全部。
pub fn view_indices(store: &RecentStore, search: &str) -> Vec<usize> {
    let mut order: Vec<usize> = (0..store.items.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = (&store.items[a], &store.items[b]);
        b.pinned
            .cmp(&a.pinned)
            .then(b.last_opened.cmp(&a.last_opened))
    });
    let q = search.trim().to_lowercase();
    if q.is_empty() {
        return order;
    }
    order.retain(|&i| {
        let it = &store.items[i];
        it.name.to_lowercase().contains(&q) || it.path.to_lowercase().contains(&q)
    });
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 直接构造条目(绕过 touch 的时钟/盘面副作用,列表语义单测用)。
    fn item(name: &str, path: &str, last_opened: i64, pinned: bool) -> RecentItem {
        RecentItem {
            path: path.into(),
            name: name.into(),
            last_opened,
            pinned,
            thumb: None,
        }
    }

    fn store(items: Vec<RecentItem>) -> RecentStore {
        RecentStore {
            schema_version: vb_session::mru::SCHEMA_VERSION,
            items,
            session: Vec::new(),
        }
    }

    fn names<'a>(store: &'a RecentStore, view: &[usize]) -> Vec<&'a str> {
        view.iter().map(|&i| store.items[i].name.as_str()).collect()
    }

    // ─────────────────────────── 展示序(排序) ───────────────────────────

    #[test]
    fn view_orders_pinned_first_then_recency() {
        let st = store(vec![
            item("a", r"C:\p\a", 100, false),
            item("b", r"C:\p\b", 300, false),
            item("c", r"C:\p\c", 100, true),
            item("d", r"C:\p\d", 200, true),
        ]);
        // 固定项在先(组内按最近使用倒序):d(200) > c(100);非固定:b(300) > a(100)
        assert_eq!(names(&st, &view_indices(&st, "")), vec!["d", "c", "b", "a"]);
    }

    // ─────────────────────────── 过滤(02-3-3) ───────────────────────────

    #[test]
    fn filter_matches_name_and_path_case_insensitive() {
        let st = store(vec![
            item("豆屿咖啡", r"C:\p\landing", 100, false),
            item("Poster", r"C:\p\海报\poster", 90, false),
        ]);
        assert_eq!(
            names(&st, &view_indices(&st, "")),
            vec!["豆屿咖啡", "Poster"],
            "空串 = 全部"
        );
        assert_eq!(
            names(&st, &view_indices(&st, "  ")),
            vec!["豆屿咖啡", "Poster"],
            "纯空白 = 全部"
        );
        assert_eq!(
            names(&st, &view_indices(&st, "豆屿")).len(),
            1,
            "按名称匹配"
        );
        assert_eq!(
            names(&st, &view_indices(&st, "LANDING")).len(),
            1,
            "路径匹配大小写不敏感"
        );
        assert_eq!(
            names(&st, &view_indices(&st, "海报")).len(),
            1,
            "路径中文段可匹配"
        );
        assert_eq!(
            names(&st, &view_indices(&st, "poster")),
            vec!["Poster"],
            "名称大小写不敏感"
        );
        assert!(names(&st, &view_indices(&st, "不存在")).is_empty());
    }

    #[test]
    fn filter_preserves_display_order_and_indexes_resolve() {
        let st = store(vec![
            item("a", r"C:\p\a", 100, false),
            item("b-landing", r"C:\p\b", 300, false),
            item("c", r"C:\p\c-landing", 200, false),
        ]);
        let view = view_indices(&st, "landing");
        assert_eq!(
            names(&st, &view),
            vec!["b-landing", "c"],
            "过滤不改变展示序"
        );
        let model = LauncherModel {
            search: "landing".into(),
            selected: 1,
        };
        assert_eq!(model.selected_item(&view, &st).unwrap().name, "c");
    }

    // ─────────────────────────── 导航状态机(02-3-4) ───────────────────────────

    #[test]
    fn navigation_clamps_at_both_ends() {
        let mut m = LauncherModel::new();
        m.select_next(3);
        assert_eq!(m.selected, 1);
        m.select_next(3);
        m.select_next(3);
        assert_eq!(m.selected, 2, "到底停");
        m.select_next(3);
        assert_eq!(m.selected, 2, "越界按底夹紧");
        m.select_prev();
        m.select_prev();
        assert_eq!(m.selected, 0);
        m.select_prev();
        assert_eq!(m.selected, 0, "到顶停(饱和)");
    }

    #[test]
    fn navigation_on_empty_list_is_noop() {
        let mut m = LauncherModel::new();
        m.select_next(0);
        m.select_prev();
        assert_eq!(m.selected, 0, "空列表导航不得越界");
    }

    #[test]
    fn search_change_resets_selection_to_zero() {
        let mut m = LauncherModel::new();
        m.select_next(5);
        m.select_next(5);
        assert_eq!(m.selected, 2);
        m.set_search("x");
        assert_eq!(m.selected, 0, "旧宿主 resp.changed() 语义");
        assert_eq!(m.search, "x");
    }

    #[test]
    fn clamp_after_removal_and_full_clear() {
        let mut m = LauncherModel::new();
        m.select_next(5);
        m.select_next(5);
        m.clamp(3); // 移除一条后列表剩 3
        assert_eq!(m.selected, 2);
        m.clamp(0); // 过滤后全空
        assert_eq!(m.selected, 0, "空列表选中归零,不得 panic");
    }

    #[test]
    fn selected_item_resolves_through_index_table() {
        // a 更近(a=200 > b=100):展示序 [a, b]
        let st = store(vec![
            item("a", r"C:\p\a", 200, false),
            item("b", r"C:\p\b", 100, false),
        ]);
        let view = view_indices(&st, "");
        let mut m = LauncherModel::new();
        assert_eq!(m.selected_item(&view, &st).unwrap().name, "a");
        m.select_next(view.len());
        assert_eq!(m.selected_item(&view, &st).unwrap().name, "b");
        m.set_search("无匹配词");
        let empty = view_indices(&st, &m.search);
        assert!(m.selected_item(&empty, &st).is_none());
        assert!(LauncherModel::new().selected_item(&[], &st).is_none());
    }
}
