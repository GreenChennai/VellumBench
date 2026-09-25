//! 最近项目(MRU)与最近会话持久化(阶段 2 / 副文档 02-2、02-6)。
//!
//! `recent.json` 与 `workspace.json` 同目录(见
//! [`crate::app::dock_layout::config_dir`]),schema:
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "items": [{ "path": "..", "name": "..", "last_opened": 0, "pinned": false, "thumb": "..?" }],
//!   "session": ["..", ".."]
//! }
//! ```
//!
//! 与 `workspace.json` 同款纪律(**02-2-6**):
//! - **原子写**:先写 `.tmp` 再改名,避免半截 JSON;
//! - **损坏回退 + 告警**:解析失败 / 版本不符 → 回退默认,中文原因交回调用方;
//! - 首次运行无文件**不是错误**(不告警)。
//!
//! 多窗口写竞争的缓解 = **单写入者**:只有外壳(`crate::shell::ShellApp`)
//! 持有本模块的内存态并落盘;项目窗口的"保存/打开"经外壳转发,不直接写。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 当前 schema 版本。**加字段必须同时抬版本 + 写迁移**(同 workspace.json)。
pub const SCHEMA_VERSION: u32 = 1;

/// 最近项目条目上限(LRU 淘汰;**固定项不淘汰**,02-2-1)。
pub const MAX_ITEMS: usize = 20;

/// 一条最近项目记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentItem {
    /// 项目目录(**规范化**绝对路径,见 [`normalize_path`])。
    pub path: String,
    /// 显示名(目录名;与文档标题无关,目录改名牌子不改)。
    pub name: String,
    /// 最近一次打开/保存/关闭时刻(Unix 秒)。
    pub last_opened: i64,
    /// 固定到列表顶部(LRU 不淘汰)。
    pub pinned: bool,
    /// 缩略图绝对路径(`.vb-cache/thumb.png`;None = 尚未生成/生成失败)。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<String>,
}

impl RecentItem {
    fn new(path: &Path, now: i64) -> Self {
        RecentItem {
            path: normalize_path(path),
            name: display_name(path),
            last_opened: now,
            pinned: false,
            thumb: None,
        }
    }

    /// 目录是否已失效(02-2-4:**置灰显示,不静默丢**)。
    pub fn is_stale(&self) -> bool {
        !Path::new(&self.path).is_dir()
    }
}

/// `recent.json` 的内存态(外壳持有;单写入者)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentStore {
    pub schema_version: u32,
    /// 最近项目(保存序 = 最近使用序,前台在先;展示排序见 [`sorted`])。
    pub items: Vec<RecentItem>,
    /// 上次会话打开的项目集合(02-6-1;退出/窗口增减时改写)。
    pub session: Vec<String>,
}

impl Default for RecentStore {
    fn default() -> Self {
        RecentStore {
            schema_version: SCHEMA_VERSION,
            items: Vec::new(),
            session: Vec::new(),
        }
    }
}

impl RecentStore {
    /// 记录一次"打开/保存/关闭"(02-2-2):置顶 + 刷新时刻 + LRU 收敛。
    /// 已存在则原地更新(保留 pinned 与缩略图),不存在则新条目前插。
    pub fn touch(&mut self, dir: &Path) {
        let now = now_secs();
        let key = path_key(dir);
        if let Some(i) = self
            .items
            .iter()
            .position(|it| path_key(Path::new(&it.path)) == key)
        {
            let it = &mut self.items[i];
            it.last_opened = now;
            it.name = display_name(dir);
            let item = self.items.remove(i);
            self.items.insert(0, item);
        } else {
            self.items.insert(0, RecentItem::new(dir, now));
        }
        self.prune_lru();
    }

    /// 回填缩略图路径(02-2-5;异步生成完成后由外壳调用)。
    pub fn set_thumb(&mut self, dir: &Path, thumb: &Path) {
        let key = path_key(dir);
        if let Some(it) = self
            .items
            .iter_mut()
            .find(|it| path_key(Path::new(&it.path)) == key)
        {
            it.thumb = Some(normalize_path(thumb));
        }
    }

    /// 从列表移除(02-2-3;主页的"移除记录"按钮)。
    pub fn remove(&mut self, dir: &Path) -> bool {
        let key = path_key(dir);
        let before = self.items.len();
        self.items.retain(|it| path_key(Path::new(&it.path)) != key);
        self.items.len() != before
    }

    /// 固定/取消固定(02-2-3)。
    pub fn toggle_pin(&mut self, dir: &Path) -> Option<bool> {
        let key = path_key(dir);
        let it = self
            .items
            .iter_mut()
            .find(|it| path_key(Path::new(&it.path)) == key)?;
        it.pinned = !it.pinned;
        Some(it.pinned)
    }

    /// LRU 收敛:超过 [`MAX_ITEMS`] 时从最旧的非固定项淘汰(**固定项不淘汰**)。
    pub fn prune_lru(&mut self) {
        while self.items.len() > MAX_ITEMS {
            let victim = self
                .items
                .iter()
                .rposition(|it| !it.pinned)
                .unwrap_or(self.items.len() - 1);
            self.items.remove(victim);
        }
    }

    /// 展示排序:固定项在先,其余按最近使用倒序。
    pub fn sorted(&self) -> Vec<&RecentItem> {
        let mut v: Vec<&RecentItem> = self.items.iter().collect();
        v.sort_by(|a, b| {
            b.pinned
                .cmp(&a.pinned)
                .then(b.last_opened.cmp(&a.last_opened))
        });
        v
    }

    /// 记录"上次会话打开的项目集合"(02-6-1;窗口增减/退出时由外壳调用)。
    pub fn set_session(&mut self, dirs: &[PathBuf]) {
        self.session = dirs.iter().map(|p| normalize_path(p)).collect();
        self.session.dedup();
    }

    /// 保存(外壳统一出口;失败返回中文原因,由调用方告警,不静默)。
    pub fn save(&self) -> Result<(), String> {
        match recent_path() {
            Some(p) => save_to(&p, self),
            None => Err("找不到配置目录,最近项目未持久化(可用 VB_RECENT 指定)".into()),
        }
    }
}

// ─────────────────────────── 路径规范化(02-2-2) ───────────────────────────

/// 显示名:目录名;根目录/无名时退化为全路径。
pub fn display_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string_lossy().to_string())
}

/// 规范化路径(存盘用):绝对化 + 分隔符统一 + 去尾分隔符。
///
/// 不用 `fs::canonicalize`:失效目录也要能入表(02-2-4),且 Windows 下
/// canonicalize 会带 `\\?\` 前缀,展示难看。
pub fn normalize_path(p: &Path) -> String {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(p)
    };
    let mut s = abs.to_string_lossy().to_string();
    if cfg!(windows) {
        s = s.replace('/', "\\");
    }
    while s.len() > 3 && s.ends_with(['\\', '/']) {
        s.pop();
    }
    s
}

/// 比较键:在 [`normalize_path`] 之上再做大小写归一(Windows 文件系统
/// 大小写不敏感;统一小写,避免"同一目录两种写法"产生双条目)。
pub fn path_key(p: &Path) -> String {
    let s = normalize_path(p);
    if cfg!(windows) {
        s.to_lowercase()
    } else {
        s
    }
}

// ─────────────────────────── 落盘(与 dock_layout 同范式) ───────────────────────────

/// `recent.json` 路径。解析顺序:`VB_RECENT` 环境变量(测试/便携用)→
/// 配置目录(与 workspace.json **同一目录解析函数**)下的 `recent.json`。
pub fn recent_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VB_RECENT") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    Some(crate::app::dock_layout::config_dir()?.join("recent.json"))
}

/// 读取(真实路径)。返回 `(store, 告警)`。
pub fn load() -> (RecentStore, Option<String>) {
    match recent_path() {
        Some(p) => load_from(&p),
        None => (
            RecentStore::default(),
            Some("找不到配置目录,最近项目不会持久化(可用 VB_RECENT 指定)".into()),
        ),
    }
}

/// 读取指定路径。**损坏不静默**:解析失败 / 版本不符 → 回退默认 + 中文告警。
pub fn load_from(path: &Path) -> (RecentStore, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return (RecentStore::default(), None), // 首次运行:无文件不是错误
    };
    match serde_json::from_str::<RecentStore>(&text) {
        Ok(mut st) => {
            if st.schema_version != SCHEMA_VERSION {
                return (
                    RecentStore::default(),
                    Some(format!(
                        "recent.json 版本 {} 与当前 {} 不符,已回退(最近项目列表清空)",
                        st.schema_version, SCHEMA_VERSION
                    )),
                );
            }
            st.schema_version = SCHEMA_VERSION;
            st.prune_lru();
            (st, None)
        }
        Err(e) => (
            RecentStore::default(),
            Some(format!("recent.json 解析失败,已回退(最近项目列表清空):{e}")),
        ),
    }
}

/// 写指定路径(原子性:先写 `.tmp` 再改名,避免半截 JSON)。
pub fn save_to(path: &Path, st: &RecentStore) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败:{e}"))?;
    }
    let mut st = st.clone();
    st.schema_version = SCHEMA_VERSION;
    let text = serde_json::to_string_pretty(&st).map_err(|e| format!("序列化最近项目失败:{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写最近项目失败:{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("提交最近项目失败:{e}"))
}

/// 当前 Unix 秒(测试外的唯一时钟入口)。
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 相对时间文案(主页列表用;纯函数便于测试)。
pub fn relative_time(unix_secs: i64, now: i64) -> String {
    let d = now - unix_secs;
    match d {
        _ if d < 60 => "刚刚".into(),
        _ if d < 3600 => format!("{} 分钟前", d / 60),
        _ if d < 86400 => format!("{} 小时前", d / 3600),
        _ if d < 86400 * 30 => format!("{} 天前", d / 86400),
        _ => {
            // 超过 30 天:落成日期(本地时区交给 chrono 太重,直接 UTC 日期够用)
            let days = std::time::Duration::from_secs(unix_secs.max(0) as u64);
            time_of(days)
        }
    }
}

/// Unix 秒 → `YYYY-MM-DD`(纯手算,避免引 chrono;1970 起的民用日期)。
fn time_of(d: std::time::Duration) -> String {
    let days = d.as_secs() / 86400;
    // 民用历换算(Howard Hinnant 的 civil_from_days 简化版)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_json(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("vb-recent-{tag}-{}.json", std::process::id()))
    }

    /// 跨平台测试路径:字面量 `C:\…` 在 unix 下 `\` 不是分隔符,`file_stem`
    /// 会把整串当文件名,`name` 断言全炸。统一走 helper 拼绝对路径
    /// (windows=`C:\` 前缀,unix=`/` 前缀),段间用平台分隔符。
    fn fx(segs: &[&str]) -> PathBuf {
        let mut pb = PathBuf::new();
        pb.push(if cfg!(windows) { "C:\\" } else { "/" });
        for s in segs {
            pb.push(s);
        }
        pb
    }

    #[test]
    fn save_then_load_roundtrip_with_session() {
        let p = tmp_json("rt");
        let _ = std::fs::remove_file(&p);
        let mut st = RecentStore::default();
        st.touch(&fx(&["proj", "landing"]));
        st.touch(&fx(&["proj", "poster"]));
        st.toggle_pin(&fx(&["proj", "landing"]));
        st.set_session(&[fx(&["proj", "landing"]), fx(&["proj", "poster"])]);
        save_to(&p, &st).unwrap();

        let (back, warn) = load_from(&p);
        assert!(warn.is_none(), "合法文件不该告警:{warn:?}");
        assert_eq!(back, st, "读写必须逐字段还原");
        assert_eq!(back.session.len(), 2, "会话字段要落盘");
        // 展示序:固定项在先(poster 是最后 touch 的,保存序在前;展示序应由 pinned 决定)
        let sorted = back.sorted();
        assert_eq!(sorted[0].name, "landing", "固定项应排在展示序最前");
        assert!(sorted[0].pinned);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn corrupt_file_falls_back_with_warning() {
        let p = tmp_json("bad");
        std::fs::write(&p, "{ 这不是 JSON").unwrap();
        let (st, warn) = load_from(&p);
        assert_eq!(st, RecentStore::default());
        assert!(warn.unwrap().contains("解析失败"), "损坏必须告警");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let p = tmp_json("missing");
        let _ = std::fs::remove_file(&p);
        let (st, warn) = load_from(&p);
        assert_eq!(st, RecentStore::default());
        assert!(warn.is_none(), "首次运行无文件不该告警");
    }

    #[test]
    fn stale_schema_version_falls_back() {
        let p = tmp_json("old");
        std::fs::write(&p, r#"{"schema_version":99,"items":[],"session":[]}"#).unwrap();
        let (st, warn) = load_from(&p);
        assert!(st.items.is_empty(), "版本不符应回退默认");
        assert!(warn.unwrap().contains("版本"), "版本不符必须告警");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn touch_moves_to_front_and_keeps_thumb_and_pin() {
        let mut st = RecentStore::default();
        st.touch(&fx(&["p", "a"]));
        st.touch(&fx(&["p", "b"]));
        assert_eq!(st.items[0].path, normalize_path(&fx(&["p", "b"])));
        st.toggle_pin(&fx(&["p", "a"]));
        st.set_thumb(&fx(&["p", "a"]), &fx(&["p", "a", ".vb-cache", "thumb.png"]));
        st.touch(&fx(&["p", "a"]));
        assert_eq!(st.items[0].name, "a");
        assert!(st.items[0].pinned, "touch 不得丢固定标记");
        assert!(st.items[0].thumb.is_some(), "touch 不得丢缩略图");
    }

    #[test]
    fn lru_evicts_oldest_unpinned_beyond_cap() {
        let mut st = RecentStore::default();
        for i in 0..(MAX_ITEMS + 3) {
            st.touch(&fx(&["p", &format!("item{i:02}")]));
        }
        assert_eq!(st.items.len(), MAX_ITEMS, "超限必须淘汰");
        assert!(
            st.items.iter().all(|it| it.name != "item00"),
            "最旧的应被淘汰"
        );
        // 固定项不淘汰
        let mut st2 = RecentStore::default();
        st2.touch(&fx(&["p", "keep-me"]));
        st2.toggle_pin(&fx(&["p", "keep-me"]));
        for i in 0..MAX_ITEMS {
            st2.touch(&fx(&["p", &format!("filler{i:02}")]));
        }
        assert_eq!(st2.items.len(), MAX_ITEMS);
        assert!(
            st2.items.iter().any(|it| it.name == "keep-me"),
            "固定项不得被 LRU 淘汰"
        );
    }

    #[test]
    fn remove_and_toggle_pin() {
        let mut st = RecentStore::default();
        st.touch(&fx(&["p", "a"]));
        assert!(st.toggle_pin(&fx(&["p", "a"])) == Some(true));
        assert!(st.remove(&fx(&["p", "a"])));
        assert!(!st.remove(&fx(&["p", "a"])), "重复移除应返回 false");
    }

    #[test]
    fn path_normalization_unifies_separators_case_and_trailing_sep() {
        let a = path_key(Path::new(r"C:\Proj\Landing\"));
        let b = path_key(Path::new("c:/proj/landing"));
        if cfg!(windows) {
            assert_eq!(a, "c:\\proj\\landing");
            assert_eq!(a, b, "分隔符/尾分隔符/大小写都要归一");
        }
        let rel = normalize_path(Path::new("examples/landing"));
        assert!(Path::new(&rel).is_absolute(), "相对路径要绝对化:{rel}");
    }

    #[test]
    fn stale_items_detect_missing_dirs() {
        let mut st = RecentStore::default();
        // 先记"幽灵目录"再记存在的目录 → 存在的目录排序在前(同为刚打开)
        let ghost = std::env::temp_dir().join("vb-不存在的目录-xyz");
        st.touch(&ghost);
        st.touch(Path::new(&std::env::temp_dir()));
        let sorted = st.sorted();
        assert!(sorted[1].is_stale(), "不存在的目录应判失效");
        assert!(!sorted[0].is_stale(), "存在的目录不应判失效");
    }

    #[test]
    fn relative_time_is_human() {
        let now = 1_800_000_000;
        assert_eq!(relative_time(now - 10, now), "刚刚");
        assert_eq!(relative_time(now - 120, now), "2 分钟前");
        assert_eq!(relative_time(now - 7200, now), "2 小时前");
        assert_eq!(relative_time(now - 86400 * 3, now), "3 天前");
        assert!(relative_time(0, now).contains('-'), "超过 30 天落日期");
    }
}
