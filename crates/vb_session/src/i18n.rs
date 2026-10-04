//! i18n 取词入口(R0 合并批:骨架 × i18n 地基两分支的交界线)。
//!
//! 22 篇 §4 R0 交付 7:实现本体在 [`vb_common::i18n`](Fluent 底座 + 双语
//! catalog + 204 条命令标签)。此处 re-export,让 `vb_kit`/`vb_shell` 的界面
//! 文案统一经 `vb_session::i18n::t(key)` 取——依赖图上 UI 侧只需要认识
//! vb_session,不直接 import vb_common 的取词内部。
//!
//! 新宿主从第一行代码起禁裸文案(G-UI3;全量双语收口在 R7)。

/// 取词 API 条目级再导出:UI 侧统一经 `vb_session::i18n::t(key)` 取词
/// (本模块文档声明的路径;只再导出条目而非整个模块,`vb_session::i18n`
/// 即取词门面,不产生 `i18n::i18n` 双层路径)。
pub use vb_common::i18n::{
    init, language, set_language, t, t_args, t_args_in, t_in, try_t, try_t_args, try_t_in, Lang,
};

/// `t_args` 实参类型再导出:UI 侧(`vb_kit`/`vb_shell`)按依赖纪律只认识
/// `vb_session`,不必为拼一条插值文案直依赖 `fluent`(vb_common 已是
/// 唯一的 fluent 消费者)。
pub use fluent::FluentValue;

/// 实例级语言覆盖层(COUP-08,2026-10-05)。
///
/// 背景:进程级语言态是 `vb_common::i18n` 的 `AtomicU8` 单例 —— 多窗口/
/// 多会话共享一个语言,改一处全局变。评估过的替代方案:把 FluentBundle
/// 目录做成每实例一份 —— **否决**:bundle 是 `OnceLock` 进程级惰性构建的
/// 只读结构,双份只有内存与构建成本,零收益;语言选择本身只是一个枚举值,
/// per-实例化的正确粒度是「覆盖值」而非「资源」。
///
/// 因此本层是**纯覆盖**:`None`(默认)= 跟随进程级语言(`language()`),
/// `Some(lang)` = 本实例覆盖。取词走 [`vb_common::i18n::t_in`](显式语言
/// 出口),不读也不写进程级 LANG —— 两实例不同语言互不干扰。
///
/// 零成本:一个 `Option<Lang>`(1 字节),`Default` 零分配;挂在
/// vb_session 的会话/窗口态上即可,进程级默认与既有 `set_language`
/// 热切换语义不变。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LangPref {
    /// 实例覆盖(None = 跟随进程默认)。
    overlay: Option<Lang>,
}

impl LangPref {
    /// 跟随进程默认语言(等价 [`Default::default`])。
    pub fn new() -> Self {
        Self { overlay: None }
    }

    /// 设置/清除实例覆盖(`None` = 回归进程默认)。
    pub fn set_override(&mut self, lang: Option<Lang>) {
        self.overlay = lang;
    }

    /// 当前实例覆盖(只读;诊断/设置面板回显用)。
    pub fn override_lang(&self) -> Option<Lang> {
        self.overlay
    }

    /// 本实例的**有效语言** = 覆盖 ?? 进程默认。
    pub fn lang(&self) -> Lang {
        self.overlay.unwrap_or_else(vb_common::i18n::language)
    }

    /// 按本实例有效语言取词(缺词语义与全局 `t` 一致)。
    pub fn t(&self, key: &str) -> String {
        vb_common::i18n::t_in(self.lang(), key)
    }

    /// 按本实例有效语言带插值取词。
    pub fn t_args<'a>(&self, key: &str, args: &[(&'a str, FluentValue<'a>)]) -> String {
        vb_common::i18n::t_args_in(self.lang(), key, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// 全局语言互斥夹具(进程级 LANG 是单例,触它的测试串行化)。
    static LANG_LOCK: Mutex<()> = Mutex::new(());

    /// COUP-08 验收:两实例不同语言覆盖互不干扰,且都不动进程默认。
    #[test]
    fn two_instances_with_different_langs_are_independent() {
        let _g = LANG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 基线:进程默认中文
        vb_common::i18n::set_language(Lang::Zh);

        let mut a = LangPref::new();
        let mut b = LangPref::new();
        assert_eq!(a.lang(), Lang::Zh, "无覆盖 → 跟随进程默认");
        assert_eq!(b.lang(), Lang::Zh);
        assert_eq!(a.t("prefs.ui-language"), "界面语言");

        a.set_override(Some(Lang::En));
        assert_eq!(a.lang(), Lang::En);
        assert_eq!(a.t("prefs.ui-language"), "UI language");
        // b 未覆盖:不受 a 影响,也不受全局影响
        assert_eq!(b.lang(), Lang::Zh);
        assert_eq!(b.t("prefs.ui-language"), "界面语言");
        // 进程默认未被实例覆盖污染
        assert_eq!(vb_common::i18n::language(), Lang::Zh);

        b.set_override(Some(Lang::En));
        b.set_override(Some(Lang::Zh));
        assert_eq!(b.t("prefs.ui-language"), "界面语言");
        assert_eq!(a.t("prefs.ui-language"), "UI language");

        // 清覆盖 → 回归进程默认;覆盖缺失 key 回退链仍生效(en 缺 → 中文)
        a.set_override(None);
        assert_eq!(a.lang(), Lang::Zh);
        assert_eq!(a.t("prefs.ui-language"), "界面语言");
        // 默认构造与 new 等价
        assert_eq!(LangPref::default(), LangPref::new());
        assert_eq!(LangPref::new().override_lang(), None);

        vb_common::i18n::set_language(Lang::Zh);
    }
}
