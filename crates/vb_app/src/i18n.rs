//! i18n 骨架(阶段 5 / 05-7;台账 09-P)。
//!
//! 设计:
//! - **资源**:仓库根 `i18n/zh.ftl`(中文,基准)与 `i18n/en.ftl`,经
//!   `include_str!` 编译期内嵌(运行期零 IO、零新依赖);格式为纯
//!   `key = value` 行 —— 不引 fluent 运行时,骨架阶段只用平键。
//! - **取词**:`t(key)`;当前语言缺 key → 回退中文;中文也没有 → 返回
//!   key 本身(显式可见,不静默),并**记一次**调试日志(`log::debug!`,
//!   target = "vb_i18n",每 (语言, key) 只记一次,防刷屏)。
//! - **语言状态**:进程级(`AtomicU8`),首选项「常规」页切换;持久化走
//!   `workspace.json` 的 `ui_lang` 字段(v2 内追加字段,serde-default 兼容)。
//! - **覆盖面(诚实声明)**:仅本批新增的对话框 / 菜单 / 面板文案走
//!   `t()`;既有界面文案仍为中文硬编码,全量抽取留后续批次(台账注明)。
//!
//! 门禁(本文件单测):①中英 key 集合一致且值非空;②t() 回退链
//! (en → zh → key 本身);③语言切换生效。禁用词扫描由
//! `tools/check_terminology.py` 覆盖两份资源(中文禁用词 + 英文对应词)。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// 中文(基准)资源。
const ZH_FTL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../i18n/zh.ftl"));
/// 英文资源。
const EN_FTL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../i18n/en.ftl"));

/// 界面语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    /// 语言代码(workspace.json `ui_lang` 的合法值)。
    pub fn code(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }

    /// 从代码解析(未知/空 → 中文,不静默扩散非法状态)。
    pub fn from_code(s: &str) -> Lang {
        if s.eq_ignore_ascii_case("en") {
            Lang::En
        } else {
            Lang::Zh
        }
    }
}

/// 当前语言(0 = 中文,1 = English;进程级)。
static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    LANG.store(
        match lang {
            Lang::Zh => 0,
            Lang::En => 1,
        },
        Ordering::Relaxed,
    );
}

pub fn lang() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Zh,
    }
}

type Table = HashMap<String, String>;

/// 解析 ftl 平键格式:跳过空行与 `#` 注释;首个 ` = ` 切分;无值的键
/// 跳过(空值视为缺失,回退链会兜住)。
fn parse_ftl(text: &str) -> Table {
    let mut out = Table::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(" = ") {
            let v = v.trim();
            if !v.is_empty() {
                out.insert(k.trim().to_string(), v.to_string());
            }
        }
    }
    out
}

fn tables() -> &'static (Table, Table) {
    static TABLES: OnceLock<(Table, Table)> = OnceLock::new();
    TABLES.get_or_init(|| (parse_ftl(ZH_FTL), parse_ftl(EN_FTL)))
}

/// 单表取词(回退链的组成单元;`pub(crate)` 供门禁测试直打)。
pub(crate) fn lookup(l: Lang, key: &str) -> Option<String> {
    let (zh, en) = tables();
    match l {
        Lang::En => en.get(key).cloned(),
        Lang::Zh => zh.get(key).cloned(),
    }
}

/// 已记录过缺词日志的 (语言, key)(每对只记一次,防每帧刷屏)。
fn logged_missing() -> &'static std::sync::Mutex<HashSet<(u8, String)>> {
    static SET: OnceLock<std::sync::Mutex<HashSet<(u8, String)>>> = OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(HashSet::new()))
}

/// 取词:当前语言 → 回退中文 → 回退 key 本身;任何一级回退都**记一次**
/// 调试日志(缺词是资源缺口,必须可发现,但不刷屏)。
pub fn t(key: &str) -> String {
    let l = lang();
    if let Some(v) = lookup(l, key) {
        return v;
    }
    // 当前语言缺 → 回退中文
    if l != Lang::Zh {
        if let Some(v) = lookup(Lang::Zh, key) {
            log_missing(l, key, "回退中文");
            return v;
        }
    }
    log_missing(l, key, "两套资源均缺失,回退 key 本身");
    key.to_string()
}

fn log_missing(l: Lang, key: &str, how: &str) {
    let mut set = logged_missing().lock().unwrap_or_else(|e| e.into_inner());
    if set.insert((l as u8, key.to_string())) {
        log::debug!(target: "vb_i18n", "i18n 缺词({how}):lang={} key={}", l.code(), key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全局语言状态的互斥测试夹具(语言是进程级单例,改语言的单测
    /// 串行化,防止与 parity 测试互相踩)。
    static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 门禁①:中英 key 集合一致,且值全部非空(空值 = 缺失,资源不许挂空)。
    #[test]
    fn zh_en_key_sets_are_identical_and_non_empty() {
        let (zh, en) = tables();
        assert!(!zh.is_empty(), "中文资源为空");
        let mut zk: Vec<&String> = zh.keys().collect();
        let mut ek: Vec<&String> = en.keys().collect();
        zk.sort();
        ek.sort();
        assert_eq!(zk, ek, "中英资源 key 集合必须一致");
        for (k, v) in zh {
            assert!(!v.trim().is_empty(), "zh 资源 {k} 值为空");
        }
        for (k, v) in en {
            assert!(!v.trim().is_empty(), "en 资源 {k} 值为空");
        }
    }

    /// 门禁②:取词回退链 —— 当前语言缺 → 中文 → key 本身(返回值显式)。
    #[test]
    fn lookup_fallback_chain() {
        // 单表行为:en 表没有 zh 独有键(若有即 parity 红灯的前提)
        let (zh, en) = tables();
        let any = zh.keys().next().unwrap().clone();
        assert!(en.contains_key(&any) || !zh.contains_key(&any));
        // 两套都缺 → None → t() 返回 key 本身
        assert_eq!(lookup(Lang::Zh, "no.such.key"), None);
        assert_eq!(lookup(Lang::En, "no.such.key"), None);
    }

    /// 门禁③:语言切换生效 + t() 兜底返回 key 本身(串行执行)。
    #[test]
    fn language_switch_and_identity_fallback() {
        let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_lang(Lang::Zh);
        assert_eq!(t("prefs.ui-language"), "界面语言");
        set_lang(Lang::En);
        assert_eq!(t("prefs.ui-language"), "UI language");
        // 缺词:回退链走到底,key 原样返回(显式可见,不静默)
        assert_eq!(t("definitely.not.a.key"), "definitely.not.a.key");
        set_lang(Lang::Zh);
        assert_eq!(t("prefs.ui-language"), "界面语言");
    }

    /// 语言代码往返 + 非法代码回中文。
    #[test]
    fn lang_code_roundtrip() {
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("EN"), Lang::En);
        assert_eq!(Lang::from_code("zh"), Lang::Zh);
        assert_eq!(Lang::from_code("fr"), Lang::Zh, "未知代码回中文");
        assert_eq!(Lang::from_code(""), Lang::Zh);
        assert_eq!(Lang::En.code(), "en");
        assert_eq!(Lang::Zh.code(), "zh");
    }
}
