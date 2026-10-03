//! Fluent i18n 底座(R0 第 7 条 / 硬骨头 #2 地基,设计 22 §4;ADR-0049 待落)。
//!
//! 职责:给新宿主(vb_shell / vb_kit)与一切非 UI crate 提供**零 UI 依赖**
//! 的取词 API,从第一行代码起禁裸文案(G-UI3 的地基)。与旧宿主的
//! `vb_app::i18n`(平键骨架,仅覆盖 05-7 批次文案)并存、互不依赖;
//! 资源同为仓库根 `i18n/zh.ftl` / `en.ftl`,既有 key 两边通用。
//!
//! 设计:
//! - **资源**:经 `include_str!` 编译期内嵌,运行期零 IO、零文件依赖。
//! - **运行时**:fluent-rs(`fluent` 0.17 + `unic-langid` 0.9,MSRV 1.67)。
//!   每语言一个并发版 `FluentBundle`(memoizer 走 Mutex,可进 `static`),
//!   `OnceLock` 惰性构建;语言状态为进程级 `AtomicU8`,`set_language`
//!   热切换即改即生效。
//! - **回退链**:当前语言 → 中文(基准)→ 内置最小表(仅 bundle 整体
//!   降级时)→ 返回 key 本身。缺词**显式可见**:release 返回 key 本身
//!   不 panic,debug 构建直接 `debug_assert!` 红,让缺词在开发期暴露。
//! - **容错**:FTL 语法错误不 panic——fluent 解析为"带错误的部分资源",
//!   坏消息跳过、好消息照常取词;一条消息都没解析出来视为 bundle 降级,
//!   回退内置最小表。任何路径不允许 unwrap 崩溃。
//! - **隔离字符**:`set_use_isolating(false)`——fluent 默认在插值两侧插
//!   U+2068/2069 隔离符,旧宿主文本栈会渲染成豆腐块;本应用文案均为
//!   短标签,不需要双向文本隔离。
//! - **点号 key 桥接**(关键决策):FTL 标识符语法不允许 `.`,而仓库
//!   既有 14 个平键带点(`prefs.ui-language` 等,旧宿主解析器的格式,
//!   05-7 起即是)**不许改名**。处理:模块边界做规范化——读入时把行首
//!   消息 id 的 `.` 换成 `-` 再交给 fluent,查询时把 key 的 `.` 同样换
//!   `-`;资源文件字节不动,带点/带连字符两种写法都能查到同一个值。
//!   命令目录 key 规范本就是 `cmd-<id 点转连字符>`,不受此桥影响。
//!
//! 门禁:`vb_app/tests/i18n_catalog_complete.rs` 断言 CMD_LABELS 全部命令
//! 在两份 ftl 都有 `cmd-*` key;`tools/check_terminology.py` 扫双语禁用词;
//! 本文件单测覆盖 t/t_args/回退链/热切换/FTL 语法容错。

use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use fluent::concurrent::FluentBundle as ConcurrentBundle;
use fluent::{FluentArgs, FluentError, FluentResource, FluentValue};
use fluent_syntax::ast;
use unic_langid::LanguageIdentifier;

/// 中文(基准)资源。
const ZH_FTL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../i18n/zh.ftl"));
/// 英文资源。
const EN_FTL: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../i18n/en.ftl"));

/// 并发版 bundle(`new_concurrent`:memoizer 走 Mutex,Send+Sync,可进
/// `static`;非并发版不可跨线程共享)。
type Bundle = ConcurrentBundle<FluentResource>;

/// 界面语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    /// 双语数组下标(zh = 0,en = 1;与 `LANG` 编码一致)。
    fn index(self) -> usize {
        match self {
            Lang::Zh => 0,
            Lang::En => 1,
        }
    }

    /// 语言代码(workspace `ui_lang` 的合法值)。
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

    fn langid(self) -> LanguageIdentifier {
        // "zh"/"en" 是字面合法 BCP-47,解析不可能失败;真失败也回 und
        // (Default)兜底,不 panic(容错条款)。
        LanguageIdentifier::from_bytes(self.code().as_bytes()).unwrap_or_default()
    }
}

/// 当前语言(0 = 中文,1 = English;进程级,初始中文)。
static LANG: AtomicU8 = AtomicU8::new(0);

/// 初始化语言(进程早期调用一次;不调用也安全,默认中文)。
/// bundle 惰性构建,首次 `t()` 时才解析内嵌 ftl。
pub fn init(lang: Lang) {
    set_language(lang);
}

/// 当前语言。
pub fn language() -> Lang {
    match LANG.load(Ordering::Relaxed) {
        1 => Lang::En,
        _ => Lang::Zh,
    }
}

/// 运行时热切换(即改即生效;下一次 `t()` 起按新语言取词)。
pub fn set_language(lang: Lang) {
    LANG.store(lang.index() as u8, Ordering::Relaxed);
}

/// 内置最小表(bundle 整体降级时的最后兜底;只放 bare-minimum 几条)。
const MINIMAL: &[(&str, &str, &str)] = &[
    // (key, zh, en)
    ("app.name", "Vellum Bench", "Vellum Bench"),
    (
        "i18n.unavailable",
        "界面文案资源加载失败",
        "UI copy resources failed to load",
    ),
];

/// 查询 key 规范化:`.` → `-`(点号 key 桥接的查询侧;带连字符的
/// key 原样通过)。
fn fluent_safe_id(key: &str) -> std::borrow::Cow<'_, str> {
    if key.contains('.') {
        std::borrow::Cow::Owned(key.replace('.', "-"))
    } else {
        std::borrow::Cow::Borrowed(key)
    }
}

/// 资源预处理:①剥 BOM;②行首消息 id 的 `.` 换 `-`(点号 key 桥接的
/// 资源侧)。只动**行首标识符**(非缩进、非注释行的第一个空白/`=` 前
/// 段),消息值一字节不动,注释与多行缩进体原样保留。
fn preprocess(src: &str) -> String {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    if !src.contains('=') {
        return src.to_string();
    }
    let mut out = String::with_capacity(src.len());
    for (i, line) in src.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let transformed = if line.is_empty()
            || line.starts_with(' ')
            || line.starts_with('\t')
            || line.starts_with('#')
        {
            None
        } else {
            let id_end = line.find([' ', '\t', '=']).unwrap_or(line.len());
            let (id, rest) = line.split_at(id_end);
            (id.contains('.')).then(|| format!("{}{rest}", id.replace('.', "-")))
        };
        match transformed {
            Some(t) => out.push_str(&t),
            None => out.push_str(line),
        }
    }
    out
}

/// 单语言目录:bundle + 降级标志(降级 = 一条消息都没解析出来)。
struct Catalog {
    bundle: Bundle,
    degraded: bool,
}

impl Catalog {
    /// 从 FTL 源构建;**语法错误不 panic**:fluent 解析为"带错误的部分
    /// 资源",坏消息跳过、好消息照常;一条消息都没有则标记降级。
    fn new(lang: Lang, src: &str) -> Catalog {
        let normalized = preprocess(src);
        let (resource, parse_errors) = match FluentResource::try_new(normalized) {
            Ok(r) => (r, Vec::new()),
            Err((r, errs)) => (r, errs),
        };
        for e in &parse_errors {
            log::debug!(target: "vb_i18n", "i18n FTL 解析错误(lang={}):{e}", lang.code());
        }
        let degraded = resource
            .entries()
            .all(|e| !matches!(e, ast::Entry::Message(_)));
        let mut bundle = Bundle::new_concurrent(vec![lang.langid()]);
        bundle.set_use_isolating(false);
        if let Err(errs) = bundle.add_resource(resource) {
            for e in errs {
                log::debug!(target: "vb_i18n", "i18n FTL 加载错误(lang={}):{e}", lang.code());
            }
        }
        if degraded {
            log::debug!(target: "vb_i18n", "i18n bundle 降级(lang={}):零条有效消息", lang.code());
        }
        Catalog { bundle, degraded }
    }

    /// 取词 + 格式化;缺消息 / 缺值 → None;格式错误(如变量缺失)记
    /// 调试日志并尽力返回已格式化部分。任何路径不 panic。
    fn format(&self, key: &str, args: Option<&FluentArgs>) -> Option<String> {
        let id = fluent_safe_id(key);
        let message = self.bundle.get_message(&id)?;
        let pattern = message.value()?;
        let mut errors: Vec<FluentError> = Vec::new();
        let value = self.bundle.format_pattern(pattern, args, &mut errors);
        for e in &errors {
            log::debug!(target: "vb_i18n", "i18n 格式错误(key={key}):{e}");
        }
        Some(value.into_owned())
    }
}

/// 双语目录(zh = 0,en = 1),进程级惰性构建一次。
fn catalogs() -> &'static [Catalog; 2] {
    static CATALOGS: OnceLock<[Catalog; 2]> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        [
            Catalog::new(Lang::Zh, ZH_FTL),
            Catalog::new(Lang::En, EN_FTL),
        ]
    })
}

/// 回退链查找:当前语言 → 中文 → 内置最小表。显式取 catalogs/lang 便于
/// 单测注入合成资源。
fn lookup_in(
    cats: &[Catalog; 2],
    lang: Lang,
    key: &str,
    args: Option<&FluentArgs>,
) -> Option<String> {
    let cur = &cats[lang.index()];
    if !cur.degraded {
        if let Some(v) = cur.format(key, args) {
            return Some(v);
        }
    }
    // 回退中文(基准)
    if lang != Lang::Zh {
        let zh = &cats[Lang::Zh.index()];
        if !zh.degraded {
            if let Some(v) = zh.format(key, args) {
                return Some(v);
            }
        }
    }
    // bundle 整体降级 → 内置最小表
    minimal_lookup(lang, key)
}

/// 内置最小表取词(表本身双语齐全,按语言直取)。
fn minimal_lookup(lang: Lang, key: &str) -> Option<String> {
    MINIMAL
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, zh, en)| match lang {
            Lang::En => (*en).to_string(),
            Lang::Zh => (*zh).to_string(),
        })
}

/// 已记录过缺词日志的 (语言, key)(每对只记一次,防每帧刷屏)。
fn log_missing_once(lang: Lang, key: &str) {
    static LOGGED: OnceLock<Mutex<HashSet<(u8, String)>>> = OnceLock::new();
    let set = LOGGED.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut s) = set.lock() {
        if s.insert((lang.index() as u8, key.to_string())) {
            log::debug!(
                target: "vb_i18n",
                "i18n 缺词:lang={} key={key}(两套资源均缺失,回退 key 本身)",
                lang.code()
            );
        }
    }
}

/// 取词:当前语言 → 回退中文 → 回退 key 本身。
///
/// 缺词在 debug 构建直接断言红(开发期暴露资源缺口);release 返回 key
/// 本身,显式可见、不静默、不 panic。
pub fn t(key: &str) -> String {
    match try_t_inner(key, None) {
        Some(v) => v,
        None => {
            debug_assert!(
                false,
                "i18n 缺词:key={key}(请在 i18n/zh.ftl 与 en.ftl 补齐)"
            );
            key.to_string()
        }
    }
}

/// 带变量插值取词:`t_args("msg-hello", &[("name", "Vellum".into())])`。
pub fn t_args<'a>(key: &str, args: &[(&'a str, FluentValue<'a>)]) -> String {
    match try_t_args(key, args) {
        Some(v) => v,
        None => {
            debug_assert!(
                false,
                "i18n 缺词:key={key}(请在 i18n/zh.ftl 与 en.ftl 补齐)"
            );
            key.to_string()
        }
    }
}

/// `t` 的显式版:缺词返回 None(测试与"允许缺省"的调用方用)。
pub fn try_t(key: &str) -> Option<String> {
    try_t_inner(key, None)
}

/// `t_args` 的显式版:缺词返回 None。
pub fn try_t_args<'a>(key: &str, args: &[(&'a str, FluentValue<'a>)]) -> Option<String> {
    let mut fa = FluentArgs::new();
    for (k, v) in args {
        fa.set(*k, v.clone());
    }
    try_t_inner(key, Some(&fa))
}

fn try_t_inner(key: &str, args: Option<&FluentArgs>) -> Option<String> {
    let lang = language();
    match lookup_in(catalogs(), lang, key, args) {
        Some(v) => Some(v),
        None => {
            log_missing_once(lang, key);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全局语言状态的互斥夹具(语言是进程级单例,改语言的单测串行化)。
    static LANG_LOCK: Mutex<()> = Mutex::new(());

    fn cat(src: &str, lang: Lang) -> Catalog {
        Catalog::new(lang, src)
    }

    #[test]
    fn embedded_ftl_has_baseline_keys() {
        let cats = catalogs();
        assert!(!cats[Lang::Zh.index()].degraded, "内嵌 zh.ftl 不应降级");
        assert!(!cats[Lang::En.index()].degraded, "内嵌 en.ftl 不应降级");
        assert_eq!(
            lookup_in(cats, Lang::Zh, "prefs.ui-language", None).as_deref(),
            Some("界面语言")
        );
        assert_eq!(
            lookup_in(cats, Lang::En, "prefs.ui-language", None).as_deref(),
            Some("UI language")
        );
    }

    #[test]
    fn t_args_interpolates_variables() {
        let c = cat(
            "msg-hello = Hello, { $name }! You have { $n } item(s).",
            Lang::En,
        );
        let mut fa = FluentArgs::new();
        fa.set("name", "Vellum");
        fa.set("n", 3);
        assert_eq!(
            c.format("msg-hello", Some(&fa)).unwrap(),
            "Hello, Vellum! You have 3 item(s)."
        );
        // 缺变量:fluent 保留原样占位,不 panic(容错)
        let got = c.format("msg-hello", None).unwrap();
        assert!(got.contains("$name"), "缺变量应保留占位:{got}");
    }

    #[test]
    fn missing_key_falls_back_explicitly() {
        // 两套资源都没有 → None(try_* 语义;t() 的 debug 断言另行单测)
        assert_eq!(try_t("definitely.not.a.key"), None);
        let cats = catalogs();
        assert_eq!(lookup_in(cats, Lang::Zh, "no.such.key", None), None);
    }

    #[test]
    fn cmd_catalog_roundtrips_through_fluent() {
        // 生成的 cmd-* 目录(204 条)fluent 能整段读入并取词
        let cats = catalogs();
        assert_eq!(
            lookup_in(cats, Lang::Zh, "cmd-object-group", None).as_deref(),
            Some("编组")
        );
        assert_eq!(
            lookup_in(cats, Lang::En, "cmd-object-group", None).as_deref(),
            Some("Group")
        );
        assert_eq!(
            lookup_in(cats, Lang::En, "cmd-file-export-dialog", None).as_deref(),
            Some("Export…"),
            "id 里的下划线转连字符后照常取词"
        );
    }

    #[test]
    fn en_missing_key_falls_back_to_zh() {
        let zh = cat("only-zh = 仅中文", Lang::Zh);
        let en = cat("unrelated = x", Lang::En);
        let cats = [zh, en];
        assert_eq!(
            lookup_in(&cats, Lang::En, "only-zh", None).as_deref(),
            Some("仅中文"),
            "en 缺 key 应回退中文"
        );
    }

    #[test]
    fn degraded_catalog_falls_back_to_minimal_table() {
        let degraded_zh = cat("# 全是注释\n", Lang::Zh);
        let degraded_en = cat("\n\n", Lang::En);
        assert!(degraded_zh.degraded && degraded_en.degraded);
        let cats = [degraded_zh, degraded_en];
        assert_eq!(
            lookup_in(&cats, Lang::Zh, "i18n.unavailable", None).as_deref(),
            Some("界面文案资源加载失败")
        );
        assert_eq!(
            lookup_in(&cats, Lang::En, "i18n.unavailable", None).as_deref(),
            Some("UI copy resources failed to load")
        );
        // 最小表也没有 → None
        assert_eq!(lookup_in(&cats, Lang::Zh, "still.missing", None), None);
    }

    #[test]
    fn dotted_keys_bridge_to_fluent_ids() {
        // 既有平键带点格式:两种写法查同一个值(点 → 连字符桥接)
        let c = cat(
            "prefs.ui-language = 界面语言\nbp.edit-banner = 断点覆盖编辑",
            Lang::Zh,
        );
        assert_eq!(
            c.format("prefs.ui-language", None).as_deref(),
            Some("界面语言")
        );
        assert_eq!(
            c.format("prefs-ui-language", None).as_deref(),
            Some("界面语言")
        );
        assert_eq!(
            c.format("bp.edit-banner", None).as_deref(),
            Some("断点覆盖编辑")
        );
        // 消息值里的点与等号不受预处理影响
        let v = cat("a.b = 值 = 值.c", Lang::Zh);
        assert_eq!(v.format("a.b", None).as_deref(), Some("值 = 值.c"));
    }

    #[test]
    fn ftl_syntax_errors_do_not_panic_and_keep_valid_messages() {
        // 坏消息 + 好消息混排:解析容错,好的照常取词,坏的返回 None
        let c = cat(
            "good = 完好消息\nbroken = { $unclosed\nalso_bad = {.}",
            Lang::Zh,
        );
        assert_eq!(c.format("good", None).as_deref(), Some("完好消息"));
        assert_eq!(c.format("broken", None), None);
        assert_eq!(c.format("also_bad", None), None);
        assert_eq!(c.format("totally.absent", None), None);
    }

    #[test]
    fn t_args_on_global_key_ignores_extra_args() {
        // 全局资源暂无占位符 key;t_args 对无变量消息=多余实参被忽略
        let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_language(Lang::Zh);
        assert_eq!(
            t_args("prefs.ui-language", &[("unused", FluentValue::from(1))]),
            "界面语言"
        );
        set_language(Lang::Zh);
    }

    #[test]
    fn language_hot_switch_takes_effect_immediately() {
        let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init(Lang::Zh);
        assert_eq!(t("prefs.ui-language"), "界面语言");
        set_language(Lang::En);
        assert_eq!(language(), Lang::En);
        assert_eq!(t("prefs.ui-language"), "UI language");
        set_language(Lang::Zh);
        assert_eq!(t("prefs.ui-language"), "界面语言");
    }

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

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "i18n 缺词")]
    fn t_missing_key_debug_asserts() {
        let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // debug 构建:缺词直接断言红(开发期暴露);release 返回 key 本身
        let _ = t("debug.assert.missing.key");
    }
}
