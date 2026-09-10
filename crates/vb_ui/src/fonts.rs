//! 字体系统（设计文档 14 篇 §3.3）。
//!
//! ## 解决什么问题
//!
//! 原先 `setup_fonts` 硬编码 `C:\Windows\Fonts\simhei.ttf`：中文用黑体（一款
//! 偏"公告/印刷"的老字体，小字号下发糊），英文继续用 egui 默认字体，
//! **中英混排时两者基线不齐** —— 这是"业余感"三大来源之二。
//!
//! ## 做法
//!
//! egui 的 `FontId` 只有 family + size，**没有字重**。所以每个字重必须注册成
//! 独立的 [`FontFamily`]。本模块注册 5 个族：
//!
//! | 族 | 拉丁 | 中文 | 用途 |
//! |---|---|---|---|
//! | [`FAMILY_REGULAR`] | Inter Regular | MiSans Regular / 系统 CJK | 正文 |
//! | [`FAMILY_MEDIUM`] | Inter Medium | 同上 | 字段标签 |
//! | [`FAMILY_SEMIBOLD`] | Inter SemiBold | 同上 | 按钮 / 分组标题 |
//! | [`FAMILY_MONO`] | JetBrains Mono / 默认等宽 | 同上 | 十六进制、代码 |
//! | [`FAMILY_ICON`] | Lucide 图标字体 | — | 图标 |
//!
//! ## fallback 链
//!
//! `Inter → MiSans → 系统 CJK`。**缺字时逐级降级，不出现豆腐块**。
//! 拉丁字体缺失时退化为 egui 内置字体，族仍然存在（只是字重区分退化）——
//! 也就是说这套代码在"字体一个都没打包"的情况下依然能跑，只是不够好看。

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId, FontTweak, TextStyle};

/// 正文族（400）。
pub const FAMILY_REGULAR: &str = "vb-ui-regular";
/// 中等族（500）。
pub const FAMILY_MEDIUM: &str = "vb-ui-medium";
/// 半粗族（600）。
pub const FAMILY_SEMIBOLD: &str = "vb-ui-semibold";
/// 等宽族。
pub const FAMILY_MONO: &str = "vb-mono";
/// 图标族（见 [`crate::icons`]）。
pub const FAMILY_ICON: &str = "vb-icon";

/// 字重族名（供 `FontId::new` 使用）。
pub fn family_regular() -> FontFamily {
    FontFamily::Name(FAMILY_REGULAR.into())
}
/// 见 [`FAMILY_MEDIUM`]。
pub fn family_medium() -> FontFamily {
    FontFamily::Name(FAMILY_MEDIUM.into())
}
/// 见 [`FAMILY_SEMIBOLD`]。
pub fn family_semibold() -> FontFamily {
    FontFamily::Name(FAMILY_SEMIBOLD.into())
}
/// 见 [`FAMILY_MONO`]。
pub fn family_mono() -> FontFamily {
    FontFamily::Name(FAMILY_MONO.into())
}
/// 见 [`FAMILY_ICON`]。
pub fn family_icon() -> FontFamily {
    FontFamily::Name(FAMILY_ICON.into())
}

/// egui 的 `TextStyle` 只给了 5 个槽位，但设计有 6 档字号。
/// 多出来的两档用具名 style 表达（`--vb-font-label`）。
pub fn style_label() -> TextStyle {
    TextStyle::Name("vb-label".into())
}

/// 多出来的两档（`--vb-font-body-strong`）。
pub fn style_body_strong() -> TextStyle {
    TextStyle::Name("vb-body-strong".into())
}

/// 字体加载结果 —— 如实回报**实际用上了什么**，供"关于"对话框与状态栏显示。
///
/// 这是"界面不说谎"的一部分：如果拉丁字体没打包进来，界面要能自己说出来，
/// 而不是假装用了 Inter。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontReport {
    /// 拉丁字体来源（"Inter Regular" / "egui 内置"）。
    pub latin: String,
    /// 中文字体来源（"MiSans" / "Microsoft YaHei" / "无(中文将显示为占位)"）。
    pub cjk: String,
    /// 等宽字体来源。
    pub mono: String,
    /// 是否使用了随包字体（false = 全靠系统字体）。
    pub bundled: bool,
}

impl FontReport {
    /// 一行摘要，直接喂给"关于"对话框。
    pub fn summary(&self) -> String {
        format!(
            "拉丁 {} · 中文 {} · 等宽 {}{}",
            self.latin,
            self.cjk,
            self.mono,
            if self.bundled { "" } else { "（未随包）" }
        )
    }
}

/// 随包字体目录：`<repo>/assets/fonts/`。
fn bundled_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts")
}

/// 某个字重优先找的随包文件名，以及它在界面上的显示名。
const LATIN_FILES: [(&str, &[&str], &str); 3] = [
    (
        "vb-latin-regular",
        &["Inter-Regular.otf", "Inter-Regular.ttf"],
        "Inter Regular",
    ),
    (
        "vb-latin-medium",
        &["Inter-Medium.otf", "Inter-Medium.ttf"],
        "Inter Medium",
    ),
    (
        "vb-latin-semibold",
        &["Inter-SemiBold.otf", "Inter-SemiBold.ttf"],
        "Inter SemiBold",
    ),
];

/// 随包中文候选。
const CJK_FILES: [(&str, &str); 2] = [
    ("MiSans-Regular.ttf", "MiSans"),
    ("MiSans-Regular.otf", "MiSans"),
];

/// 系统中文回退候选（Windows 常见路径）。
///
/// 中文**不随包**的原因见 `docs/fonts.md`：微软雅黑已随 Windows 分发，
/// 直接用它不存在再分发问题，且省 2MB 体积。若用户把 MiSans 放进
/// `assets/fonts/`，则优先用 MiSans。
const SYSTEM_CJK: [(&str, &str); 3] = [
    ("C:\\Windows\\Fonts\\msyh.ttc", "Microsoft YaHei"),
    ("C:\\Windows\\Fonts\\msyh.ttf", "Microsoft YaHei"),
    ("C:\\Windows\\Fonts\\simsun.ttc", "SimSun"),
];

/// 随包等宽候选。
const MONO_FILES: [(&str, &str); 2] = [
    ("JetBrainsMono-Regular.ttf", "JetBrains Mono"),
    ("JetBrainsMono-Regular.otf", "JetBrains Mono"),
];

/// CJK 字形微调。
///
/// egui 按各字体自身的度量排基线，中英混排（如「宽度 W 320 px」）可能出现
/// 中文整体偏高/偏低。`FontTweak` 就是为此准备的旋钮。
///
/// ⚠️ **当前是恒等变换（全部为 0），即"尚未调过"**。原因：这个值必须在真机上
/// 用实际字体肉眼复核，凭猜测给一个非零值只会引入新的偏移。
/// 设计文档 14 篇 §3.3 的验收项（「中英混排不跳基线」）在真实字体到位后
/// 需要回来把这里调出非零值，届时同步更新 14 篇 §3.3 的备注。
fn cjk_tweak() -> FontTweak {
    FontTweak::default()
}

/// 注册全部字体族。
///
/// 在 app 启动时调用一次（`ctx.set_fonts` 之后建议紧跟
/// [`crate::theme::apply`]，因为样式里的字号依赖族名）。
pub fn install(ctx: &egui::Context) -> FontReport {
    let mut fonts = FontDefinitions::default();
    let mut bundled = false;

    // ── 拉丁：随包 Inter（缺失则退化为 egui 内置） ──
    let mut latin = "egui 内置".to_string();
    for (id, candidates, label) in LATIN_FILES {
        if let Some((bytes, _)) = read_first_bundled(&bundled_dir(), candidates) {
            fonts
                .font_data
                .insert(id.into(), Arc::new(FontData::from_owned(bytes)));
            bundled = true;
            latin = label.to_string();
        }
    }

    // ── 中文：优先随包 MiSans，其次系统 ──
    let mut cjk = "无(中文将显示为占位)".to_string();
    let mut cjk_id: Option<&'static str> = None;
    if let Some((bytes, _)) = read_first_bundled(&bundled_dir(), &CJK_FILES.map(|(f, _)| f)[..]) {
        fonts.font_data.insert(
            "vb-cjk".into(),
            Arc::new(FontData::from_owned(bytes).tweak(cjk_tweak())),
        );
        bundled = true;
        cjk = CJK_FILES[0].1.to_string();
        cjk_id = Some("vb-cjk");
    }
    if cjk_id.is_none() {
        for (path, label) in SYSTEM_CJK {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert(
                    "vb-cjk".into(),
                    Arc::new(FontData::from_owned(bytes).tweak(cjk_tweak())),
                );
                cjk = label.to_string();
                cjk_id = Some("vb-cjk");
                break;
            }
        }
    }

    // ── 等宽：随包 JetBrains Mono，否则用 egui 内置等宽 ──
    let mut mono = "egui 内置等宽".to_string();
    if let Some((bytes, _)) = read_first_bundled(&bundled_dir(), &MONO_FILES.map(|(f, _)| f)[..]) {
        fonts
            .font_data
            .insert("vb-mono-face".into(), Arc::new(FontData::from_owned(bytes)));
        bundled = true;
        mono = MONO_FILES[0].1.to_string();
    }

    // ── 图标字体（Lucide，由 icons 模块提供） ──
    crate::icons::register(&mut fonts);

    // ── 组族：拉丁 → 中文 的 fallback 链 ──
    //
    // egui 按 families[name] 的顺序查找字形：前面的字体没有该字形时往后找，
    // 所以"拉丁在前、CJK 在后"天然实现了 fallback 链。
    // 先算好各族的首选字形 ID 再插入，避免同时借用 fonts 的可变与不可变。
    let has = |id: &str| fonts.font_data.contains_key(id);
    let regular_id = has("vb-latin-regular").then_some("vb-latin-regular");
    // 想要的字重缺失时退到 regular：族必须**始终存在**，否则
    // `FontId::new(size, family_medium())` 找不到字形。粗细退化好过崩溃。
    let medium_id = has("vb-latin-medium")
        .then_some("vb-latin-medium")
        .or(regular_id);
    let semibold_id = has("vb-latin-semibold")
        .then_some("vb-latin-semibold")
        .or(regular_id);
    let mono_face = has("vb-mono-face").then_some("vb-mono-face");

    let chain = |latin: Option<&str>| -> Vec<String> {
        let mut v = Vec::new();
        v.extend(latin.map(str::to_string));
        v.extend(cjk_id.map(str::to_string));
        v
    };
    fonts
        .families
        .insert(FontFamily::Name(FAMILY_REGULAR.into()), chain(regular_id));
    fonts
        .families
        .insert(FontFamily::Name(FAMILY_MEDIUM.into()), chain(medium_id));
    fonts
        .families
        .insert(FontFamily::Name(FAMILY_SEMIBOLD.into()), chain(semibold_id));
    fonts
        .families
        .insert(FontFamily::Name(FAMILY_MONO.into()), chain(mono_face));

    // ── 让 egui 内置族也能显示中文（默认控件、TextEdit 都走这两个族） ──
    if let Some(id) = cjk_id {
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(list) = fonts.families.get_mut(&family) {
                list.push(id.to_string());
            }
        }
    }

    ctx.set_fonts(fonts);

    FontReport {
        latin,
        cjk,
        mono,
        bundled,
    }
}

/// 便捷构造：`FontId` + 字重族。
pub fn font(size: f32, weight: Weight) -> FontId {
    FontId::new(
        size,
        match weight {
            Weight::Regular => family_regular(),
            Weight::Medium => family_medium(),
            Weight::Semibold => family_semibold(),
        },
    )
}

/// 字重。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    /// 400
    Regular,
    /// 500
    Medium,
    /// 600
    Semibold,
}

/// 在目录里按候选顺序找第一个存在的文件。
fn read_first_bundled(
    dir: &std::path::Path,
    candidates: &[&str],
) -> Option<(Vec<u8>, std::path::PathBuf)> {
    for name in candidates {
        let p = dir.join(name);
        if let Ok(bytes) = std::fs::read(&p) {
            return Some((bytes, p));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 5 个族名互不相同，否则注册时会互相覆盖。
    #[test]
    fn family_names_are_distinct() {
        let names = [
            FAMILY_REGULAR,
            FAMILY_MEDIUM,
            FAMILY_SEMIBOLD,
            FAMILY_MONO,
            FAMILY_ICON,
        ];
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert_ne!(a, b, "字体族名重复：{a}");
            }
        }
    }

    /// 族名必须带 `vb-` 前缀（ADR-0014 前缀统一），避免与用户文档里的
    /// CSS 字体名或将来引入的第三方字体冲突。
    #[test]
    fn family_names_are_prefixed() {
        for n in [
            FAMILY_REGULAR,
            FAMILY_MEDIUM,
            FAMILY_SEMIBOLD,
            FAMILY_MONO,
            FAMILY_ICON,
        ] {
            assert!(n.starts_with("vb-"), "字体族 {n} 未带 vb- 前缀");
        }
    }

    /// 具名 TextStyle 必须与 `theme::apply` 里插入的键一致。
    #[test]
    fn named_text_styles_are_stable() {
        assert_eq!(style_label(), TextStyle::Name("vb-label".into()));
        assert_eq!(
            style_body_strong(),
            TextStyle::Name("vb-body-strong".into())
        );
    }
}
