//! 字体缺失专项对话框(阶段 5 / 05-4-A2;台账 09-O)。
//!
//! 打开文档时扫描全部 `font-family` 声明(节点样式 + 富文本段注记),
//! 对不在「可用字体集」内的字体名弹专项对话框:缺失清单 + 每项
//! 「替换为系统字体」(候选按名称相似度排序)+ 跳过。替换经
//! `SetStyle` 命令层(Compound,可撤销)。
//!
//! **检测口径(诚实标注)**:真正的系统字体枚举需要注册表/字体库,
//! 本版按「可用集合 = CSS 通用族 + 随包/系统回退已装字体(启动时的
//! `FontReport`)+ 常见 Windows 字体族清单」判定;对话框里如实写明,
//! 不冒充逐字体枚举。可用集合之外的一律按缺失提示(宁报缺不漏报)。

use std::collections::BTreeSet;

use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeKind};

use super::VellumApp;

/// CSS 通用族(浏览器自行解析,永远可用)。
const GENERIC_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-sans-serif",
    "ui-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "inherit",
    "initial",
    "unset",
];

/// 已随环境就绪的字体族(随包 + 系统回退;与 `vb_ui::fonts` 的
/// 安装候选一致)。
const BUNDLED_FAMILIES: &[&str] = &[
    "Inter",
    "Inter Regular",
    "Inter Medium",
    "Inter SemiBold",
    "MiSans",
    "Microsoft YaHei",
    "微软雅黑",
    "SimSun",
    "宋体",
    "JetBrains Mono",
];

/// 常见 Windows 字体族(随系统分发;检测按名单判定,诚实标注非枚举)。
const KNOWN_WINDOWS_FAMILIES: &[&str] = &[
    "Arial",
    "Arial Black",
    "Bahnschrift",
    "Calibri",
    "Cambria",
    "Candara",
    "Comic Sans MS",
    "Consolas",
    "Constantia",
    "Corbel",
    "Courier New",
    "Ebrima",
    "Franklin Gothic Medium",
    "Gabriola",
    "Gadugi",
    "Georgia",
    "Impact",
    "Ink Free",
    "Javanese Text",
    "Leelawadee UI",
    "Lucida Console",
    "Lucida Sans Unicode",
    "Malgun Gothic",
    "Marlett",
    "Microsoft Himalaya",
    "Microsoft JhengHei",
    "Microsoft New Tai Lue",
    "Microsoft PhagsPa",
    "Microsoft Sans Serif",
    "Microsoft Tai Le",
    "Microsoft YaHei UI",
    "Mongolian Baiti",
    "MS Gothic",
    "MS Mincho",
    "MV Boli",
    "Myanmar Text",
    "Nirmala UI",
    "Noto Sans",
    "Noto Serif",
    "Palatino Linotype",
    "Segoe MDL2 Assets",
    "Segoe Print",
    "Segoe Script",
    "Segoe UI",
    "Segoe UI Emoji",
    "Segoe UI Historic",
    "Segoe UI Symbol",
    "SimHei",
    "黑体",
    "SimSun-ExtB",
    "Sitka",
    "Sylfaen",
    "Symbol",
    "Tahoma",
    "Times New Roman",
    "Trebuchet MS",
    "Verdana",
    "Webdings",
    "Wingdings",
    "楷体",
    "KaiTi",
    "仿宋",
    "FangSong",
    "等线",
    "DengXian",
    "华文黑体",
    "PingFang SC",
    "Hiragino Sans GB",
    "Source Han Sans SC",
    "思源黑体",
    "Noto Sans SC",
    "Noto Serif SC",
    "Alibaba PuHuiTi",
    "HarmonyOS Sans SC",
];

/// 一条缺失字体(含使用它的节点 sid 集合;替换命令按 sid 构建)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFont {
    /// 缺失的字体名(font-family 声明里的单个名字,已去引号)。
    pub family: String,
    /// 使用该字体的节点 sid(替换的作用域)。
    pub sids: Vec<String>,
}

/// 字体名归一(比较用):去引号、去空白、小写。
fn norm_family(s: &str) -> String {
    s.trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 单个 font-family 声明值 → 字体名列表(按逗号切,逐个去引号;
/// `!important` 后缀剥离)。
pub fn parse_family_list(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_end_matches("!important")
        .split(',')
        .map(|s| s.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 名字是否可用(通用族 / 已装 / 常见系统族)。
pub fn is_available(family: &str) -> bool {
    let n = norm_family(family);
    GENERIC_FAMILIES.iter().any(|g| norm_family(g) == n)
        || BUNDLED_FAMILIES.iter().any(|g| norm_family(g) == n)
        || KNOWN_WINDOWS_FAMILIES.iter().any(|g| norm_family(g) == n)
}

/// 相似度(编辑距离;越小越相似)。候选排序用,无需精确。
fn edit_distance(a: &str, b: &str) -> usize {
    let a = a.to_lowercase();
    let b = b.to_lowercase();
    let (m, n) = (a.len(), b.len());
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur = vec![0usize; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = usize::from(a.as_bytes()[i - 1] != b.as_bytes()[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}

/// 替换候选:通用族 + 已装 + 常见系统族,按与缺失名的相似度升序。
pub fn replacement_candidates(missing: &str) -> Vec<&'static str> {
    let mut all: Vec<&'static str> = Vec::new();
    all.extend(BUNDLED_FAMILIES.iter().copied());
    all.extend(KNOWN_WINDOWS_FAMILIES.iter().copied());
    let key = norm_family(missing);
    let mut scored: Vec<(usize, &'static str)> = all
        .into_iter()
        .map(|f| (edit_distance(&key, &norm_family(f)), f))
        .collect();
    scored.sort();
    scored.dedup_by(|a, b| a.1 == b.1);
    let mut out: Vec<&'static str> = scored.into_iter().map(|(_, f)| f).collect();
    // 通用族永远兜底可用,放在相似度榜之后(sans-serif/serif/monospace)
    out.extend_from_slice(&["sans-serif", "serif", "monospace"]);
    out
}

/// 扫描文档:返回缺失字体清单(按名字稳定排序;每项带使用它的 sid)。
pub fn scan_missing_fonts(doc: &Document) -> Vec<MissingFont> {
    // family(norm) → sid 集
    let mut usage: std::collections::BTreeMap<String, BTreeSet<String>> =
        std::collections::BTreeMap::new();
    for (_, n) in doc.nodes.iter() {
        // 节点级 font-family 声明
        for decl in &n.style {
            if decl.prop == "font-family" {
                for fam in parse_family_list(&decl.value) {
                    if !is_available(&fam) {
                        usage
                            .entry(norm_family(&fam))
                            .or_default()
                            .insert(n.sid.as_str().to_string());
                    }
                }
            }
        }
        // 富文本段注记的字体覆盖
        if let NodeKind::Text { segments, .. } = &n.kind {
            for seg in segments {
                if let Some(fam) = &seg.style.font_family {
                    if !is_available(fam) {
                        usage
                            .entry(norm_family(fam))
                            .or_default()
                            .insert(n.sid.as_str().to_string());
                    }
                }
            }
        }
    }
    usage
        .into_iter()
        .map(|(family, sids)| MissingFont {
            family: display_name_of(&family),
            sids: sids.into_iter().collect(),
        })
        .collect()
}

/// 展示名:扫描时丢失了原大小写,从声明值里找回一个原样的名字。
/// (简化:直接存 norm 名;CSS 字体名大小写不敏感,展示不致歧义。)
fn display_name_of(norm: &str) -> String {
    norm.to_string()
}

// ─────────────────────────── 对话框(渲染层) ───────────────────────────

/// 对话框会话态(每条缺失字体的替换选择)。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FontDialogState {
    pub entries: Vec<MissingFont>,
    /// 每条的用户选择(候选下标;与 entries 同长,默认 0)。
    pub picks: Vec<usize>,
    /// 已跳过的条目下标集。
    pub skipped: Vec<bool>,
}

impl FontDialogState {
    pub fn new(entries: Vec<MissingFont>) -> Self {
        let n = entries.len();
        FontDialogState {
            entries,
            picks: vec![0; n],
            skipped: vec![false; n],
        }
    }
}

impl VellumApp {
    /// 字体缺失专项窗口(09-O;打开时自动弹出,「文字 → 查找字体…」复用)。
    pub(crate) fn show_font_window(&mut self, ui: &mut egui::Ui) {
        let Some(dlg) = self.font_dialog.clone() else {
            return;
        };
        // 待处理条目 = 未跳过且确有作用域对象;全部处理完 → 收窗
        let pending = dlg
            .entries
            .iter()
            .zip(&dlg.skipped)
            .any(|(e, skip)| !skip && !e.sids.is_empty());
        if dlg.entries.is_empty() || !pending {
            self.font_dialog = None;
            return;
        }
        let mut open = true;
        let mut apply: Vec<usize> = Vec::new();
        egui::Window::new("缺失字体")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.weak(format!(
                    "检测到 {} 种文档使用的字体不在本机可用集合内(判定口径:通用族 + 随包/系统回退 + 常见系统字体;非逐字体枚举)。",
                    dlg.entries.len()
                ));
                ui.separator();
                let candidates: Vec<Vec<&'static str>> = dlg
                    .entries
                    .iter()
                    .map(|e| replacement_candidates(&e.family))
                    .collect();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, entry) in dlg.entries.iter().enumerate() {
                        if dlg.skipped[i] || entry.sids.is_empty() {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                vb_ui::theme::tokens(ui.ctx()).warn,
                                format!("「{}」", entry.family),
                            );
                            ui.weak(format!("{} 个对象", entry.sids.len()));
                            let mut pick = dlg.picks[i];
                            egui::ComboBox::from_id_salt(format!("vb-font-swap-{i}"))
                                .selected_text(
                                    candidates[i].get(pick).copied().unwrap_or("sans-serif"),
                                )
                                .show_ui(ui, |ui| {
                                    for (j, cand) in candidates[i].iter().enumerate() {
                                        ui.selectable_value(&mut pick, j, *cand);
                                    }
                                });
                            self.font_dialog.as_mut().unwrap().picks[i] = pick;
                            if ui.button("替换").clicked() {
                                apply.push(i);
                            }
                            if ui.button("跳过").clicked() {
                                self.font_dialog.as_mut().unwrap().skipped[i] = true;
                            }
                        });
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("全部跳过").clicked() {
                        if let Some(d) = self.font_dialog.as_mut() {
                            d.skipped = vec![true; d.entries.len()];
                        }
                    }
                    if ui.button("全部替换(各自首选候选)").clicked() {
                        for i in 0..dlg.entries.len() {
                            if !dlg.skipped[i] && !dlg.entries[i].sids.is_empty() {
                                apply.push(i);
                            }
                        }
                    }
                });
            });
        // 执行替换(命令层,可撤销)
        for i in apply {
            self.font_replace_one(i);
        }
        if !open {
            self.font_dialog = None;
        }
    }

    /// 执行一条替换:`family` → 所选候选,作用域 = 使用它的全部节点
    /// (Compound/SetStyle,可撤销);完成后条目标记跳过(收窗条件)。
    fn font_replace_one(&mut self, index: usize) {
        let Some(dlg) = self.font_dialog.clone() else {
            return;
        };
        let Some(entry) = dlg.entries.get(index) else {
            return;
        };
        let candidates = replacement_candidates(&entry.family);
        let target = candidates
            .get(dlg.picks.get(index).copied().unwrap_or(0))
            .copied()
            .unwrap_or("sans-serif");
        let mut cmds: Vec<Command> = Vec::new();
        for sid in &entry.sids {
            if let Some(nid) = self.doc.find_by_sid(sid) {
                let n = self.doc.nodes.get(nid).unwrap();
                let mut new_style = n.style.clone();
                let mut hit = false;
                for decl in new_style.iter_mut() {
                    if decl.prop == "font-family" {
                        let fams = parse_family_list(&decl.value);
                        if fams
                            .iter()
                            .any(|f| norm_family(f) == norm_family(&entry.family))
                        {
                            let replaced: Vec<String> = fams
                                .iter()
                                .map(|f| {
                                    if norm_family(f) == norm_family(&entry.family) {
                                        target.to_string()
                                    } else {
                                        f.clone()
                                    }
                                })
                                .collect();
                            decl.value = replaced.join(", ");
                            hit = true;
                        }
                    }
                }
                if hit {
                    cmds.push(Command::SetStyle {
                        sid: sid.clone(),
                        new: new_style,
                        old: None,
                    });
                }
            }
        }
        let count = cmds.len();
        if count > 0 {
            self.exec(Command::Compound { cmds });
        }
        if let Some(d) = self.font_dialog.as_mut() {
            d.skipped[index] = true;
        }
        self.say(format!(
            "字体替换:「{}」→「{target}」({count} 个对象,可撤销)",
            entry.family
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::assemble::tests::app_fresh;
    use vb_doc::model::Geom;
    use vb_doc::model::{Node, SegStyle, TextSeg};

    /// 可用性判定:通用族/随包/常见系统族可用;生僻名不可用;大小写与
    /// 引号不敏感。
    #[test]
    fn availability_judgement() {
        for ok in [
            "sans-serif",
            "SANS-SERIF",
            "Inter",
            "\"Microsoft YaHei\"",
            "Arial",
            "微软雅黑",
            "Noto Sans SC",
        ] {
            assert!(is_available(ok), "{ok} 应判可用");
        }
        for miss in ["FooBar Sans", "MythicalType", "Noto Sans CJK SC ExtraLight"] {
            assert!(!is_available(miss), "{miss} 应判缺失");
        }
    }

    /// family 列表解析:引号剥离、important 剥离、空段过滤。
    #[test]
    fn family_list_parse() {
        assert_eq!(
            parse_family_list("\"A B\", 'C', sans-serif"),
            vec!["A B", "C", "sans-serif"]
        );
        assert_eq!(parse_family_list("Inter !important"), vec!["Inter"]);
        assert!(parse_family_list(" , ").is_empty());
    }

    /// 扫描:节点样式与段注记都进清单;同一字体聚合 sid;可用字体不报。
    #[test]
    fn scan_collects_missing_from_styles_and_segments() {
        let mut doc = Document::new_empty("t", "zh-CN");
        let ab = doc.new_artboard("A", 100.0, 100.0);
        let mut box_node = Node::new(NodeKind::Box, "b", doc.alloc_sid());
        box_node.parent = Some(ab);
        box_node.geom = Geom {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        box_node.style.push(vb_css::Decl {
            prop: "font-family".into(),
            value: "MythicalType, sans-serif".into(),
            important: false,
        });
        let box_sid = box_node.sid.as_str().to_string();
        let bid = doc.nodes.insert(box_node);
        if let Some(p) = doc.nodes.get_mut(ab) {
            p.children.push(bid);
        }
        // 文本节点:段注记字体缺失
        let mut text = Node::new(
            NodeKind::Text {
                text: "hi".into(),
                mode: vb_doc::model::TextMode::Point,
                segments: vec![TextSeg {
                    start: 0,
                    end: 2,
                    style: SegStyle {
                        font_family: Some("FooBar Sans".into()),
                        ..SegStyle::default()
                    },
                }],
            },
            "t",
            doc.alloc_sid(),
        );
        text.parent = Some(ab);
        text.geom = Geom {
            x: 0.0,
            y: 0.0,
            w: 10.0,
            h: 10.0,
        };
        let text_sid = text.sid.as_str().to_string();
        let tid = doc.nodes.insert(text);
        if let Some(p) = doc.nodes.get_mut(ab) {
            p.children.push(tid);
        }
        let missing = scan_missing_fonts(&doc);
        let names: Vec<&str> = missing.iter().map(|m| m.family.as_str()).collect();
        assert_eq!(names.len(), 2, "两种缺失各一条:{names:?}");
        assert!(names.contains(&"mythicaltype"));
        assert!(names.contains(&"foobar sans"));
        let m = missing.iter().find(|m| m.family == "mythicaltype").unwrap();
        assert_eq!(m.sids, vec![box_sid]);
        // sans-serif 不进清单
        assert!(!names.contains(&"sans-serif"));
        let _ = text_sid;
    }

    /// 候选排序:与缺失名越近越靠前(精确/近名在前,通用族兜底在后)。
    #[test]
    fn candidates_sorted_by_similarity() {
        let c = replacement_candidates("Microsoft YaHeii");
        assert_eq!(c[0], "Microsoft YaHei", "近名应排最前:{c:?}");
        assert_eq!(replacement_candidates("Arial")[0], "Arial");
        for cand in replacement_candidates("完全无关的字体") {
            assert!(!cand.is_empty());
        }
        // 通用族兜底必在
        let c = replacement_candidates("zzz");
        assert!(c.contains(&"sans-serif"));
    }

    /// 替换可逆(命令层):替换 → 撤销回到原 font-family 声明。
    #[test]
    fn replace_is_undoable() {
        let _env = crate::ENV_LOCK.lock();
        let mut app = app_fresh(None);
        // 构造一个用「生僻字体」的 box
        let ab = app.active_artboard().unwrap();
        let sid = {
            let mut n = Node::new(NodeKind::Box, "b", app.doc.alloc_sid());
            n.parent = Some(ab);
            n.geom = Geom {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            };
            n.style.push(vb_css::Decl {
                prop: "font-family".into(),
                value: "MythicalType".into(),
                important: false,
            });
            let sid = n.sid.as_str().to_string();
            let nid = app.doc.nodes.insert(n);
            app.doc.nodes.get_mut(ab).unwrap().children.push(nid);
            sid
        };
        app.font_dialog = Some(FontDialogState::new(scan_missing_fonts(&app.doc)));
        assert!(app.font_dialog.is_some(), "有缺失必须开专项对话框");
        // 执行替换(候选 0 = 相似度最高;MythicalType 无近名,取榜首个可)
        app.font_replace_one(0);
        let v1 = |app: &crate::app::VellumApp| -> String {
            app.doc
                .find_by_sid(&sid)
                .and_then(|id| app.doc.nodes.get(id))
                .and_then(|n| n.style_get("font-family").map(str::to_string))
                .unwrap_or_default()
        };
        let after = v1(&app);
        assert_ne!(after, "MythicalType", "替换必须改写声明:{after}");
        assert!(is_available(&after), "替换目标必须可用:{after}");
        // 撤销回到原值(可逆性)
        let _ = app.undo.undo(&mut app.doc).unwrap();
        let restored = v1(&app);
        assert_eq!(restored, "MythicalType", "撤销必须还原原声明:{restored}");
    }
}
