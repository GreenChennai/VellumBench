//! 文档设置对话框(阶段 5 / 05-4-A2;台账 09-M)。
//!
//! 「文件 → 文档设置…」:项目名(标题)/ 默认画板尺寸与预设 /
//! 输出模式(外链 CSS / 单文件)/ 网格与参考线(项目级)。
//!
//! **项目级网格/参考线的落盘位置(定夺)**:`index.html` 的
//! `<meta name="vb-grid" …>` / `<meta name="vb-guides" …>`(经文档模型
//! 既有的 `head_extra` 原样透传通道),**不用**项目内旁路配置文件。
//! 理由:
//! 1. 「HTML 是文档格式」是产品底线(design/00)—— 网格/参考线属于
//!    这份文档的编辑设置,旁路 JSON 会成为第二个真相源,复制/改名/外部
//!    diff 都可能让它与 index.html 脱钩;
//! 2. `head_extra` 本来就为「head 里无法建模的 meta/link」而生,零模型
//!    改动、往返幂等(import 原样捕获 → export 原样写回);
//! 3. 自动保存快照/三方对比(recover)都拿 `index.html` 全文 —— meta
//!    方案让文档设置**同样**被快照与恢复覆盖,旁路文件则不会。
//!
//! 生效时机(**应用后生效**,窗口底部「应用」统一提交):
//! 标题经 `SetMetaTitle`(可撤销);输出模式与网格/参考线 meta 是
//! 文档级设置(无对应命令变体),直改 + `rev += 1` 标脏 —— 由
//! Ctrl+S / 自动保存落盘,Agent 乐观锁语义不变(见各处注释)。

use vb_doc::model::Document;

use super::VellumApp;

// ─────────────────────────── vb-* meta 纯函数(单测直打) ───────────────────────────

/// 从文档 `head_extra` 里取 `name="vb-…"` meta 的 content 值。
/// 容忍单/双引号与属性顺序;找不到返回 None。
pub fn meta_get(doc: &Document, name: &str) -> Option<String> {
    let needle = format!("name=\"{name}\"");
    let needle2 = format!("name='{name}'");
    for raw in &doc.head_extra {
        let lower = raw.to_lowercase();
        let pos = lower.find(&needle).or_else(|| lower.find(&needle2));
        let Some(p) = pos else { continue };
        // 在同一条 meta 里找 content="…"
        let rest = &raw[p..];
        for key in ["content=\"", "content='"] {
            if let Some(cp) = rest.to_lowercase().find(key) {
                let after = &rest[cp + key.len()..];
                let quote = &key[key.len() - 1..];
                if let Some(end) = after.find(quote) {
                    return Some(after[..end].to_string());
                }
            }
        }
    }
    None
}

/// 设置(或删除)`name="vb-…"` meta 的 content;替换既有同名条目,
/// 无则新插到 head_extra 尾部;`content = None` → 整条删除。
/// 直接改文档(文档级设置无命令变体;调用方负责标脏)。
pub fn meta_set(doc: &mut Document, name: &str, content: Option<&str>) {
    doc.head_extra.retain(|raw| {
        !raw.to_lowercase().contains(&format!("name=\"{name}\""))
            && !raw.to_lowercase().contains(&format!("name='{name}'"))
    });
    if let Some(c) = content {
        let esc = c.replace('"', "&quot;");
        doc.head_extra
            .push(format!("<meta name=\"{name}\" content=\"{esc}\">"));
    }
}

/// 项目级网格设置(meta `vb-grid`)。
#[derive(Debug, Clone, PartialEq)]
pub struct GridSettings {
    /// 网格基础间距 px(≥2)。
    pub spacing: f64,
    /// 打开文档时是否显示网格。
    pub show: bool,
}

impl Default for GridSettings {
    fn default() -> Self {
        GridSettings {
            spacing: 64.0,
            show: true,
        }
    }
}

/// 编码:`spacing=16,show=1`(紧凑键值;解析容忍空格/顺序/缺项)。
pub fn grid_encode(g: &GridSettings) -> String {
    format!(
        "spacing={},show={}",
        g.spacing.max(2.0) as i64,
        if g.show { 1 } else { 0 }
    )
}

/// 解析(缺项回默认;非法值逐项回默认,不整条作废)。
pub fn grid_parse(content: &str) -> GridSettings {
    let mut g = GridSettings::default();
    for part in content.split(',') {
        let mut kv = part.splitn(2, '=');
        let k = kv.next().unwrap_or("").trim();
        let v = kv.next().unwrap_or("").trim();
        match k {
            "spacing" => {
                if let Ok(n) = v.parse::<f64>() {
                    if n >= 2.0 && n.is_finite() {
                        g.spacing = n;
                    }
                }
            }
            "show" => g.show = v == "1" || v.eq_ignore_ascii_case("true"),
            _ => {}
        }
    }
    g
}

/// 编码参考线列表(meta `vb-guides`):`h12.5,v3,h20`(h=水平线 y,v=垂直线 x)。
pub fn guides_encode(guides: &[(bool, f64)]) -> String {
    guides
        .iter()
        .map(|&(h, pos)| format!("{}{}", if h { 'h' } else { 'v' }, pos))
        .collect::<Vec<_>>()
        .join(",")
}

/// 解析参考线列表(坏 token 跳过)。
pub fn guides_parse(content: &str) -> Vec<(bool, f64)> {
    content
        .split(',')
        .filter_map(|tok| {
            let tok = tok.trim();
            let (h, num) = match tok.chars().next()? {
                'h' => (true, &tok[1..]),
                'v' => (false, &tok[1..]),
                _ => return None,
            };
            let pos = num.parse::<f64>().ok()?;
            Some((h, pos))
        })
        .collect()
}

// ─────────────────────────── 对话框(渲染层) ───────────────────────────

/// 对话框的编辑态(打开时从文档快照;「应用」统一提交)。
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct DocSettingsState {
    pub title: String,
    pub output_single: bool,
    pub grid: GridSettings,
    pub guides: String,
    /// 05-5:断点清单编辑串(px,逗号分隔;空 = 无)。
    pub breakpoints: String,
}

impl VellumApp {
    /// 「文件 → 文档设置…」窗口(09-M)。
    pub(crate) fn show_doc_settings_window(&mut self, ui: &mut egui::Ui) {
        if !self.doc_settings_open {
            return;
        }
        if self.doc_settings_state.is_none() {
            // 打开时从文档快照(meta + meta 之外的既有字段)
            let grid = meta_get(&self.doc, "vb-grid")
                .map(|c| grid_parse(&c))
                .unwrap_or_default();
            let guides = meta_get(&self.doc, "vb-guides").unwrap_or_default();
            self.doc_settings_state = Some(DocSettingsState {
                title: self.doc.meta.title.clone(),
                output_single: matches!(
                    self.doc.meta.output,
                    vb_doc::model::OutputMode::SingleFile
                ),
                grid,
                guides,
                breakpoints: meta_get(&self.doc, super::breakpoints::META_NAME).unwrap_or_default(),
            });
        }
        let mut open = true;
        let mut applied = false;
        let mut cancel = false;
        egui::Window::new("文档设置")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                let Some(st) = self.doc_settings_state.as_mut() else {
                    return;
                };
                egui::Grid::new("vb-docset-grid")
                    .num_columns(2)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.label("项目名(= 标题)");
                        ui.add_sized(
                            [240.0, vb_ui::theme::row_height(ui.ctx())],
                            egui::TextEdit::singleline(&mut st.title),
                        );
                        ui.end_row();
                        ui.label("输出模式");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut st.output_single, false, "外链 CSS(styles/main.css)");
                            ui.selectable_value(&mut st.output_single, true, "单文件(CSS 内联)");
                        });
                        ui.end_row();
                        ui.label("网格间距");
                        ui.horizontal(|ui| {
                            let mut sp = st.grid.spacing;
                            ui.add(
                                egui::DragValue::new(&mut sp)
                                    .range(2.0..=256.0)
                                    .suffix(" px"),
                            );
                            st.grid.spacing = sp;
                            ui.checkbox(&mut st.grid.show, "显示网格");
                        });
                        ui.end_row();
                        ui.label("参考线");
                        ui.add_sized(
                            [240.0, vb_ui::theme::row_height(ui.ctx())],
                            egui::TextEdit::singleline(&mut st.guides)
                                .hint_text("h=水平线 y,v=垂直线 x,逗号分隔(如 h0,v120)"),
                        );
                        ui.end_row();
                        // 05-5:断点清单(响应式;状态栏切换器与属性面板消费)
                        ui.label(format!("{}({})", crate::i18n::t("bp.switcher"), crate::i18n::t("bp.doc-settings")));
                        ui.vertical(|ui| {
                            ui.add_sized(
                                [240.0, vb_ui::theme::row_height(ui.ctx())],
                                egui::TextEdit::singleline(&mut st.breakpoints)
                                    .hint_text("px 逗号分隔(如 375,750,1080;空 = 无)"),
                            );
                            ui.weak(crate::i18n::t("bp.doc-settings-help"));
                        });
                        ui.end_row();
                    });
                ui.add_space(4.0);
                ui.weak("网格与参考线存项目级(index.html 的 vb-grid / vb-guides meta),随文件走;保存后生效。");
                ui.weak("画板尺寸在「画板」面板逐块调整;默认预设见首选项「画板」页。");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("应用").clicked() {
                        applied = true;
                    }
                    if ui.button("取消").clicked() {
                        self.doc_settings_state = None;
                        cancel = true;
                    }
                });
            });
        if applied {
            self.doc_settings_apply();
            open = false;
        }
        if cancel {
            open = false;
        }
        if !open {
            self.doc_settings_open = false;
            self.doc_settings_state = None;
        }
    }

    /// 「应用」:编辑态 → 文档。标题可撤销(SetMetaTitle);输出模式与
    /// 网格/参考线 meta 直改 + 标脏(文档级设置;Ctrl+S / 自动保存落盘)。
    fn doc_settings_apply(&mut self) {
        let Some(st) = self.doc_settings_state.clone() else {
            return;
        };
        // ① 标题(可撤销命令;空白回退原值)
        let title = st.title.trim();
        if !title.is_empty() && title != self.doc.meta.title {
            self.exec(vb_doc::commands::Command::SetMetaTitle {
                new: title.to_string(),
                old: None,
            });
        }
        // ② 输出模式 + ③ 网格/参考线 meta(直改 + rev 标脏)
        let new_output = if st.output_single {
            vb_doc::model::OutputMode::SingleFile
        } else {
            vb_doc::model::OutputMode::ExternalCss
        };
        self.doc.meta.output = new_output;
        meta_set(&mut self.doc, "vb-grid", Some(&grid_encode(&st.grid)));
        let guides: Vec<(bool, f64)> = guides_parse(&st.guides);
        if guides.is_empty() {
            meta_set(&mut self.doc, "vb-guides", None);
        } else {
            meta_set(&mut self.doc, "vb-guides", Some(&guides_encode(&guides)));
        }
        // ④ 断点清单 meta(05-5;与网格/参考线同款直改 + 标脏)
        let bps = super::breakpoints::parse_meta(&st.breakpoints);
        let entered = st
            .breakpoints
            .split(',')
            .filter(|t| !t.trim().is_empty())
            .count();
        super::breakpoints::set_meta_breakpoints(&mut self.doc, &bps);
        if bps.len() != entered {
            self.say(format!(
                "断点:{} 项中有 {} 项无效已忽略(px 正整数)",
                entered,
                entered - bps.len()
            ));
        }
        if self
            .active_breakpoint
            .map(|w| !bps.contains(&w))
            .unwrap_or(false)
        {
            self.active_breakpoint = None;
        }
        // 参考线同时投进画布(所见即所得)
        self.guides = guides;
        self.grid_size = st.grid.spacing;
        // 文档级设置直改:rev 前进一位标脏(is_dirty 判据;不经 undo 栈,
        // 与「工作区偏好」同级 —— 内容编辑仍全部走命令层)。meta_set 幂等
        // 但无廉价判等,统一按已改处理。
        self.doc.rev += 1;
        self.say("文档设置已应用(Ctrl+S 或自动保存写盘)");
        self.doc_settings_state = None;
        self.doc_settings_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// vb-* meta 的写/读/删往返(单双条目、引号形态)。
    #[test]
    fn meta_set_get_roundtrip() {
        let mut doc = Document::new_default();
        assert_eq!(meta_get(&doc, "vb-grid"), None, "无条目返回 None");
        meta_set(&mut doc, "vb-grid", Some("spacing=32,show=0"));
        assert_eq!(
            meta_get(&doc, "vb-grid").as_deref(),
            Some("spacing=32,show=0")
        );
        // 替换(不得出现两条同名 meta)
        meta_set(&mut doc, "vb-grid", Some("spacing=48,show=1"));
        assert_eq!(
            meta_get(&doc, "vb-grid").as_deref(),
            Some("spacing=48,show=1")
        );
        assert_eq!(
            doc.head_extra
                .iter()
                .filter(|r| r.contains("vb-grid"))
                .count(),
            1,
            "同名 meta 必须替换而非追加"
        );
        // 删除
        meta_set(&mut doc, "vb-grid", None);
        assert_eq!(meta_get(&doc, "vb-grid"), None);
        // head_extra 里与它无关的条目不动
        doc.head_extra
            .push("<link rel=\"icon\" href=\"a.ico\">".into());
        meta_set(&mut doc, "vb-x", Some("1"));
        assert!(doc.head_extra.iter().any(|r| r.contains("icon")));
        meta_set(&mut doc, "vb-x", None);
        assert!(doc.head_extra.iter().all(|r| !r.contains("vb-x")));
    }

    /// 网格设置编解码往返 + 容错解析(缺项/非法值逐项回默认)。
    #[test]
    fn grid_encode_parse_roundtrip_and_tolerant() {
        let g = GridSettings {
            spacing: 32.0,
            show: false,
        };
        assert_eq!(grid_encode(&g), "spacing=32,show=0");
        assert_eq!(grid_parse(&grid_encode(&g)), g);
        // 缺项回默认
        assert_eq!(grid_parse(""), GridSettings::default());
        assert_eq!(grid_parse("show=1"), GridSettings::default());
        // 非法 spacing 逐项回默认,合法 show 保留
        let p = grid_parse("spacing=-5,show=0");
        assert_eq!(p.spacing, 64.0);
        assert!(!p.show);
    }

    /// 参考线列表编解码往返 + 坏 token 跳过。
    #[test]
    fn guides_encode_parse_roundtrip() {
        let gs = vec![(true, 0.0), (false, 120.5), (true, -8.0)];
        let text = guides_encode(&gs);
        assert_eq!(guides_parse(&text), gs, "往返还原(含 0 与负值)");
        assert_eq!(
            guides_parse("h1,x9,v2,bad"),
            vec![(true, 1.0), (false, 2.0)]
        );
        assert!(guides_parse("").is_empty());
    }

    /// 端到端:meta 落进 head_extra 后,导出 HTML 应包含该 meta
    /// (序列化路径合法 —— 靠 index.html 单一真相落盘的前提)。
    #[test]
    fn grid_meta_survives_render() {
        let mut doc = Document::new_default();
        meta_set(&mut doc, "vb-grid", Some("spacing=24,show=1"));
        let rendered = vb_doc::export::render_project(&doc);
        let html = &rendered.files[0].1;
        assert!(
            html.contains("name=\"vb-grid\"") && html.contains("spacing=24"),
            "导出必须携带 vb-grid meta"
        );
        // 再导入仍可读回(head_extra 原样透传)
        let dir = std::env::temp_dir().join(format!("vb-docset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        vb_doc::export::write_project(&doc, &dir).unwrap();
        let r = vb_doc::import::import_project(&dir).unwrap();
        assert_eq!(
            meta_get(&r.doc, "vb-grid").as_deref(),
            Some("spacing=24,show=1"),
            "导入必须原样带回 meta"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
