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
//!
//! ## 随包(UI-14,审查 2026-10-04 §8.4 动作 1)
//!
//! `assets/fonts/` 内置 **Inter Regular/Medium/SemiBold**(SIL OFL 1.1,见
//! `LICENSE-OFL.txt`)与 **MiSans Regular**(小米免费商用许可,见
//! `LICENSE-MiSans.txt`),fallback 链从此有前两级,中英混排的拉丁侧
//! 不再依赖系统字体;中文侧有 MiSans 时优先于系统 CJK(消除
//! "每台机器字体不一样"的观感漂移)。

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

/// egui 的 `TextStyle` 只给了 5 个槽位，但设计有 7 档字号(§8.4)。
/// 多出来的档用具名 style 表达。
pub fn style_label() -> TextStyle {
    TextStyle::Name("vb-label".into())
}

/// 多出来的档(`--vb-font-body-strong`)。
pub fn style_body_strong() -> TextStyle {
    TextStyle::Name("vb-body-strong".into())
}

/// display 档(24/600,启动器标题/空态主标题;§8.4 新增)。
pub fn style_display() -> TextStyle {
    TextStyle::Name("vb-display".into())
}

/// 字体加载结果 —— 如实回报**实际用上了什么**，供"关于"对话框与状态栏显示。
///
/// 这是"界面不说谎"的一部分：如果拉丁字体没打包进来，界面要能自己说出来，
/// 而不是假装用了 Inter。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontReport {
    /// 拉丁字体来源（"Inter" / "egui 内置"）。
    pub latin: String,
    /// 中文字体来源（"MiSans" / "Microsoft YaHei" / "无(中文将显示为占位)"）。
    pub cjk: String,
    /// 等宽字体来源。
    pub mono: String,
    /// 是否使用了随包字体（false = 全靠系统字体）。
    pub bundled: bool,
    /// 实际加载成功的拉丁字重清单(["Inter Regular", "Inter Medium", …];
    /// 空 = 无随包拉丁,族退化到 egui 内置)。UI-14 体检扩展:三档字重
    /// 缺任何一档,「关于」对话框都能如实点名,而不是假装字重齐全。
    pub latin_weights: Vec<String>,
    /// 中文是否走随包 MiSans(false = 系统 CJK 回退)。
    pub cjk_bundled: bool,
}

impl FontReport {
    /// 一行摘要，直接喂给"关于"对话框。
    pub fn summary(&self) -> String {
        let weights = if self.latin_weights.is_empty() {
            String::new()
        } else {
            format!("({})", self.latin_weights.join("/"))
        };
        format!(
            "拉丁 {}{} · 中文 {}{} · 等宽 {}{}",
            self.latin,
            weights,
            self.cjk,
            if self.cjk_bundled { "(随包)" } else { "" },
            self.mono,
            if self.bundled { "" } else { "（未随包）" }
        )
    }
}

/// 随包字体目录（按顺序探测，取第一个存在者）：
///
/// 1. `VB_FONTS_DIR` 环境变量（用户/分发者显式指定）；
/// 2. `<exe目录>/assets/fonts/` —— 发行布局（字体与 exe 一起分发）；
/// 3. `<exe目录>/../assets/fonts/` —— 便携版布局（assets 与 bin 同级）；
/// 4. `<repo>/assets/fonts/` —— 开发布局（编译期路径，仅开发机存在）。
///
/// 不能只用 `CARGO_MANIFEST_DIR`：那是编译期常量，发布到用户机器后指向
/// 不存在的路径，随包字体会全体静默失效（vb_ui/fonts.rs 发布阻断项）。
fn bundled_dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = std::env::var_os("VB_FONTS_DIR") {
        v.push(std::path::PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.parent() {
            v.push(root.join("assets/fonts"));
            if let Some(parent) = root.parent() {
                v.push(parent.join("assets/fonts"));
            }
        }
    }
    v.push(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts"));
    v
}

/// 第一个含任一候选字体文件的目录；都不存在则返回开发布局路径
/// （让 `read_first_bundled` 照常落空，行为与旧版一致）。
fn bundled_dir() -> std::path::PathBuf {
    let candidates = bundled_dirs();
    for dir in &candidates {
        if dir.is_dir() {
            return dir.clone();
        }
    }
    candidates
        .into_iter()
        .next_back()
        .unwrap_or_else(|| std::path::PathBuf::from("assets/fonts"))
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

/// 系统中文回退候选（相对 `%WINDIR%\Fonts` 解析）。
///
/// 随包 MiSans(UI-14)就位时**不会走到这里**;此回退链服务于"字体目录
/// 被删/分发不完整"的场景:微软雅黑已随 Windows 分发,直接用它不存在
/// 再分发问题。候选含等线/黑体作兜底,避免老系统缺 msyh 时中文全灭。
fn system_cjk_candidates() -> Vec<(std::path::PathBuf, &'static str)> {
    let windir = std::env::var_os("WINDIR")
        .or_else(|| std::env::var_os("SystemRoot"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("C:\\Windows"));
    let fonts = windir.join("Fonts");
    let rel = [
        ("msyh.ttc", "Microsoft YaHei"),
        ("msyh.ttf", "Microsoft YaHei"),
        ("Deng.ttf", "DengXian"),
        ("simhei.ttf", "SimHei"),
        ("simsun.ttc", "SimSun"),
    ];
    rel.into_iter()
        .map(|(f, label)| (fonts.join(f), label))
        .collect()
}

/// 随包等宽候选。
const MONO_FILES: [(&str, &str); 2] = [
    ("JetBrainsMono-Regular.ttf", "JetBrains Mono"),
    ("JetBrainsMono-Regular.otf", "JetBrains Mono"),
];

/// CJK 字形微调(UI-13,审查 2026-10-04 §8.4 动作 2)。
///
/// egui 按各字体自身的度量排基线,中英混排(如「宽度 W 320 px」)可能出现
/// 中文整体偏高/偏低。`FontTweak` 就是为此准备的旋钮 —— 但它**只影响绘制、
/// 不参与布局**(见 epaint `FontTweak` 文档)。
///
/// **实测结论(2026-10-05,随包 Inter 4.1 + MiSans,探针见
/// `cjk_baseline_probe`)**:egui 0.35 对混排行做**居中补偿**
/// (`glyph.pos.y = face_ascent + 0.5×(font_height − face_height)`,见
/// epaint text_layout.rs),Inter 与 MiSans 的字形 baseline_y 实测完全一致
/// (13.00 @13px)—— **布局层基线已齐**;残余只可能是 face 度量声明与实际
/// 轮廓之间的光学差,只能靠真机渲染像素判断。
///
/// 因此当前保持 0(恒等)是**有依据的结论**而非未做:任何非零值都必须
/// 先有像素级证据。真机复核程序:跑应用目检「宽度 W 320 px」同一行,
/// 若确认跳变,截图量差值 px → `y_offset = −差值`(正值下移),回填
/// 下面常量并删除 TODO。
// TODO(UI-13/S5):真机像素走查后决定是否需要非零校准;0 = 布局层已实测对齐
const CJK_TWEAK_Y_OFFSET_FACTOR: f32 = 0.0;
/// 见 [`CJK_TWEAK_Y_OFFSET_FACTOR`]。
const CJK_TWEAK_Y_OFFSET: f32 = 0.0;

fn cjk_tweak() -> FontTweak {
    FontTweak {
        y_offset_factor: CJK_TWEAK_Y_OFFSET_FACTOR,
        y_offset: CJK_TWEAK_Y_OFFSET,
        ..FontTweak::default()
    }
}

/// 注册全部字体族。
///
/// 在 app 启动时调用一次（`ctx.set_fonts` 之后建议紧跟
/// [`crate::theme::apply`]，因为样式里的字号依赖族名）。
pub fn install(ctx: &egui::Context) -> FontReport {
    let mut fonts = FontDefinitions::default();
    let mut bundled = false;
    let mut latin_weights = Vec::new();

    // ── 拉丁：随包 Inter(缺失则退化为 egui 内置) ──
    let mut latin = "egui 内置".to_string();
    for (id, candidates, label) in LATIN_FILES {
        if let Some((bytes, _)) = read_first_bundled(&bundled_dir(), candidates) {
            fonts
                .font_data
                .insert(id.into(), Arc::new(FontData::from_owned(bytes)));
            bundled = true;
            latin = "Inter".to_string();
            latin_weights.push(label.to_string());
        }
    }

    // ── 中文：优先随包 MiSans，其次系统 ──
    let mut cjk = "无(中文将显示为占位)".to_string();
    let mut cjk_id: Option<&'static str> = None;
    let mut cjk_bundled = false;
    if let Some((bytes, _)) = read_first_bundled(&bundled_dir(), &CJK_FILES.map(|(f, _)| f)[..]) {
        fonts.font_data.insert(
            "vb-cjk".into(),
            Arc::new(FontData::from_owned(bytes).tweak(cjk_tweak())),
        );
        bundled = true;
        cjk_bundled = true;
        cjk = CJK_FILES[0].1.to_string();
        cjk_id = Some("vb-cjk");
    }
    if cjk_id.is_none() {
        for (path, label) in system_cjk_candidates() {
            if let Ok(bytes) = std::fs::read(&path) {
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
        latin_weights,
        cjk_bundled,
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
        assert_eq!(style_display(), TextStyle::Name("vb-display".into()));
    }

    /// UI-14 随包回归门禁:开发布局(`assets/fonts/`)必须带齐
    /// Inter 三档 + MiSans Regular。字体文件是普通 git 内容(非 LFS),
    /// 缺文件 = 有人误删/误 gitignore,随包字体全体静默失效 —— 直接红。
    #[test]
    fn bundled_font_files_are_present() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/fonts");
        for name in [
            "Inter-Regular.otf",
            "Inter-Medium.otf",
            "Inter-SemiBold.otf",
            "MiSans-Regular.ttf",
            "LICENSE-OFL.txt",
            "LICENSE-MiSans.txt",
        ] {
            assert!(
                dir.join(name).is_file(),
                "随包字体缺失:{},UI-14 回归",
                dir.join(name).display()
            );
        }
    }

    /// FontReport 摘要必须如实反映随包状态(三档字重点名 + CJK 随包标记)。
    #[test]
    fn font_report_summary_reflects_bundling() {
        let bundled = FontReport {
            latin: "Inter".into(),
            cjk: "MiSans".into(),
            mono: "JetBrains Mono".into(),
            bundled: true,
            latin_weights: vec![
                "Inter Regular".into(),
                "Inter Medium".into(),
                "Inter SemiBold".into(),
            ],
            cjk_bundled: true,
        };
        let s = bundled.summary();
        assert!(
            s.contains("Inter(Inter Regular/Inter Medium/Inter SemiBold)"),
            "{s}"
        );
        assert!(s.contains("MiSans(随包)"), "{s}");
        assert!(!s.contains("未随包"), "{s}");
        let degenerated = FontReport {
            latin: "egui 内置".into(),
            cjk: "Microsoft YaHei".into(),
            mono: "egui 内置等宽".into(),
            bundled: false,
            latin_weights: Vec::new(),
            cjk_bundled: false,
        };
        let s = degenerated.summary();
        assert!(s.contains("egui 内置 ·"), "{s}");
        assert!(s.contains("未随包"), "{s}");
    }

    /// UI-13 校准探针(**手工运行**,默认忽略):用随包 Inter + MiSans
    /// 实测混排行内拉丁/CJK 字形的 ascent 差,给 `cjk_tweak` 的
    /// `y_offset_factor` 提供标定输入。
    ///
    /// 运行:`cargo test -p vb_ui cjk_baseline_probe -- --ignored --nocapture`
    ///
    /// 限制(诚实声明):这里量的是**布局层度量**(egui Glyph 的
    /// ascent/face_ascent),不是渲染像素;「宽度 W 320 px 不跳基线」的
    /// 最终验收仍需真机肉眼复核(见 `cjk_tweak` 文档)。
    #[test]
    #[ignore = "手工校准探针(UI-13):输出量测数据供 cjk_tweak 标定,不做断言"]
    fn cjk_baseline_probe() {
        let ctx = egui::Context::default();
        let report = install(&ctx);
        ctx.begin_pass(egui::RawInput::default());
        println!("FontReport: {}", report.summary());
        for (text, label) in [("W字W", "混排"), ("W", "纯拉丁"), ("字", "纯 CJK")] {
            let galley = ctx.fonts_mut(|f| {
                f.layout_no_wrap(
                    text.to_owned(),
                    font(13.0, Weight::Regular),
                    egui::Color32::WHITE,
                )
            });
            println!("— {label}「{text}」 row_h={:?}", galley.size().y);
            for row in &galley.rows {
                for g in &row.glyphs {
                    println!(
                        "'{}' baseline_y={:.2} font_ascent={:.2} face_ascent={:.2} font_h={:.2}",
                        g.chr, g.pos.y, g.font_ascent, g.font_face_ascent, g.font_height
                    );
                }
            }
        }
        println!(
            "egui 0.35 混排定位:pos.y = face_ascent + 0.5×(font_height − face_height)\n\
             (居中补偿;两字体 baseline_y 相同 = 布局层基线已齐。\n\
              若真机仍见光学跳变:y_offset = −像素差值,填入 CJK_TWEAK_Y_OFFSET)"
        );
    }
}
