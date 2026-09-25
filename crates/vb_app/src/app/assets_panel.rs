//! 资产面板(阶段 7 / 07-K;副文档 07 §2.3)。
//!
//! 列出项目 `assets/` 的文件与**被引用关系**,支持「定位到引用」与「替换」:
//! - 引用收集**单一来源** = [`super::health::asset_reference_map`](07-E 复用,
//!   不写两套 —— 引用约定漂移会同时破坏体检与面板);
//! - 未使用的资产给「未使用」标记(与 07-E ④「未使用资产」同源同口径);
//! - 「定位到引用」= 点击引用行选中对应图层(与体检报告同一交互);
//! - 「替换」= 把某节点引用指向另一资产,走 [`Command::SetImageSrc`]
//!   命令层(kind 与 attrs 双写同步、可撤销,由命令层保证);
//! - 图片给小预览(纹理缓存在 `VellumApp::asset_thumbs`),字体给名字。
//!
//! 数据面 = 纯函数 [`asset_rows`](文档状态级测试直打);面板正文在文件尾。

use std::path::Path;

use vb_doc::commands::Command;
use vb_doc::model::{Document, NodeKind};
use vb_ui::icons::{self, Name};
use vb_ui::theme;

use super::{health, VellumApp};

/// 一行资产(渲染前折算好的展示模型;纯函数可测)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRow {
    /// 相对项目根路径(正斜杠;显示与替换命令都用它)。
    pub rel: String,
    /// 文件名(含扩展名;字体类以名字即预览)。
    pub name: String,
    /// 类别(图片给缩略图 / 字体给名字 / 其余按扩展名)。
    pub kind: AssetKind,
    /// 文件大小(字节;读不到 = 0)。
    pub bytes: u64,
    /// 引用它的节点(`via` = src / href;CSS `url()` 无节点,只计数)。
    pub refs: Vec<health::RefHit>,
    /// CSS `url()` 引用次数(无节点可定位/替换,只提示)。
    pub css_refs: usize,
}

impl AssetRow {
    /// 是否被任何引用(节点或 CSS)命中。
    pub fn used(&self) -> bool {
        !self.refs.is_empty() || self.css_refs > 0
    }

    /// 引用总数(节点引用 + CSS 引用)。
    pub fn ref_count(&self) -> usize {
        self.refs.len() + self.css_refs
    }
}

/// 资产类别(按扩展名粗分;决定预览形态)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    Image,
    Font,
    Style,
    Other,
}

impl AssetKind {
    pub fn label(self) -> &'static str {
        match self {
            AssetKind::Image => "图片",
            AssetKind::Font => "字体",
            AssetKind::Style => "样式",
            AssetKind::Other => "文件",
        }
    }
}

/// 资产类别判定(按扩展名;大小写不敏感)。
pub fn asset_kind(name: &str) -> AssetKind {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => {
            AssetKind::Image
        }
        "ttf" | "otf" | "woff" | "woff2" | "ttc" => AssetKind::Font,
        "css" => AssetKind::Style,
        _ => AssetKind::Other,
    }
}

/// 资产行折算(纯函数):递归列 `assets/` + 套引用映射(07-E 单一来源)。
///
/// 排序:未使用在前(最需要处理的先看到),其余按引用数降序、路径升序。
pub fn asset_rows(project: &Path, doc: &Document) -> Vec<AssetRow> {
    let refs = health::asset_reference_map(doc);
    let mut files = Vec::new();
    health::list_files(project, Path::new("assets"), &mut files);
    let mut rows: Vec<AssetRow> = files
        .iter()
        .map(|rel| {
            let key = health::rel_key(&rel.to_string_lossy());
            let hits = refs.get(&key).cloned().unwrap_or_default();
            let name = rel
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let kind = asset_kind(&name);
            let bytes = std::fs::metadata(project.join(rel))
                .map(|m| m.len())
                .unwrap_or(0);
            AssetRow {
                rel: rel.to_string_lossy().replace('\\', "/"),
                name,
                kind,
                bytes,
                refs: hits.iter().filter(|h| !h.sid.is_empty()).cloned().collect(),
                css_refs: hits.iter().filter(|h| h.sid.is_empty()).count(),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        a.used()
            .cmp(&b.used())
            .then_with(|| b.ref_count().cmp(&a.ref_count()))
            .then_with(|| a.rel.cmp(&b.rel))
    });
    rows
}

/// 「替换引用」的选择态(会话态;挂在 `VellumApp::asset_replace`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacePick {
    /// 被替换引用的节点 sid。
    pub sid: String,
    /// 原引用路径(展示用)。
    pub from: String,
}

/// 构造替换命令(纯函数):把节点 `sid` 的图像引用指向 `new_rel`。
///
/// 走 [`Command::SetImageSrc`]:kind 与 attrs 双写同步、可撤销都在命令层;
/// 目标节点不带 src 引用(非图像且无 src 属性)时返回 `None`(调用方给
/// 中文提示,不静默)。
pub fn replace_cmd(doc: &Document, sid: &str, new_rel: &str) -> Option<Command> {
    let id = doc.find_by_sid(sid)?;
    let n = doc.nodes.get(id)?;
    let is_image = matches!(n.kind, NodeKind::Image { .. });
    let had_attr = n.attrs.contains_key("src");
    if !is_image && !had_attr {
        return None;
    }
    Some(Command::SetImageSrc {
        sid: sid.to_string(),
        new: new_rel.to_string(),
        old: None,
    })
}

/// 人类可读的文件大小(面板行内展示)。
fn fmt_bytes(b: u64) -> String {
    if b >= 1024 * 1024 {
        format!("{:.1} MB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{:.0} KB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

impl VellumApp {
    /// 资产面板正文(次级坞「资产」Tab;`panel_dock::sec_panel_body` 转发)。
    pub(crate) fn assets_panel_body(&mut self, ui: &mut egui::Ui) {
        let Some(dir) = self.project_dir.clone() else {
            ui.label("当前文档没有项目目录(先保存或打开一个项目)。");
            return;
        };
        ui.horizontal(|ui| {
            ui.label(format!("项目:{}", crate::recent::display_name(&dir)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("刷新").clicked() {
                    self.assets_cache = None;
                }
            });
        });
        ui.separator();

        // 行折算带 rev 缓存:文档没变不重读盘(手动「刷新」强制重算)
        let rev = self.doc.rev;
        if self.assets_cache.as_ref().is_none_or(|(r, _)| *r != rev) {
            self.assets_cache = Some((rev, asset_rows(&dir, &self.doc)));
        }
        let rows = self
            .assets_cache
            .as_ref()
            .map(|(_, r)| r.clone())
            .unwrap_or_default();
        if rows.is_empty() {
            // U-5:资产空态 = 统一「图标 + 一句短话 + 动作按钮」
            let t = vb_ui::theme::tokens(ui.ctx());
            ui.add_space(vb_ui::theme::space::S3);
            ui.horizontal(|ui| {
                ui.add_space(vb_ui::theme::space::S2);
                ui.label(vb_ui::icons::rich(vb_ui::icons::Name::KindImage, 18.0).color(t.text_3));
                ui.label("项目还没有 assets/ 目录 —— 拖入或置入图像后会自动出现。");
            });
            ui.add_space(vb_ui::theme::space::S2);
            ui.horizontal_wrapped(|ui| {
                if ui.button("保存项目(Ctrl+S)").clicked() {
                    self.run_command("file.save", false, false);
                }
                if ui.button("图像置入…").clicked() {
                    self.run_command("file.place_image", false, false);
                }
            });
        } else {
            let unused = rows.iter().filter(|r| !r.used()).count();
            ui.label(format!(
                "{} 个资产,{} 个未被引用(CSS url() 只计数,无可定位节点)",
                rows.len(),
                unused
            ));
        }
        ui.separator();

        // 点击先收集,渲染完再执行(与历史面板同款借用纪律)
        let mut locate: Option<String> = None;
        let mut replace: Option<health::RefHit> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for row in &rows {
                // ── 资产行:缩略图/类别点 | 名字 + 元信息 | 引用/未使用标记 ──
                let t = theme::tokens(ui.ctx());
                ui.horizontal(|ui| {
                    self.asset_thumb(ui, &dir, &row.rel, row.kind);
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(&row.name).strong().size(12.5));
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {} · {}",
                                row.kind.label(),
                                fmt_bytes(row.bytes),
                                row.rel
                            ))
                            .size(11.0)
                            .color(t.text_3),
                        );
                    });
                });
                // ── 引用关系行 ──
                if row.used() {
                    for hit in &row.refs {
                        let name = self
                            .doc
                            .find_by_sid(&hit.sid)
                            .and_then(|id| self.doc.nodes.get(id))
                            .map(|n| n.name.clone())
                            .unwrap_or_else(|| "(已不存在的节点)".into());
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("└ {} → {}", hit.via, hit.sid))
                                    .size(11.5),
                            );
                            let goto = ui
                                .selectable_label(false, format!("「{name}」·点击定位"))
                                .on_hover_text("点击选中该图层(定位到引用)");
                            if goto.clicked() {
                                locate = Some(hit.sid.clone());
                            }
                            if ui.small_button("替换").clicked() {
                                replace = Some(hit.clone());
                            }
                        });
                    }
                    if row.css_refs > 0 {
                        ui.label(
                            egui::RichText::new(format!(
                                "└ css url() ×{}(样式表引用,无节点可定位)",
                                row.css_refs
                            ))
                            .size(11.5)
                            .color(theme::tokens(ui.ctx()).text_3),
                        );
                    }
                } else {
                    // 07-I:未使用标记走主题 warn 令牌(浅色自动加深,不硬编码浅橙)
                    ui.label(
                        egui::RichText::new("└ 未使用 —— 没有任何 src/href/CSS 引用(可归档或删除)")
                            .size(11.5)
                            .color(theme::tokens(ui.ctx()).warn),
                    );
                }
                ui.add_space(theme::space::S2);
            }
        });

        // 定位到引用(与体检报告同款交互:选中 + 状态提示)
        if let Some(sid) = locate {
            if self.doc.find_by_sid(&sid).is_some() {
                self.selection = vec![sid.clone()];
                self.say(format!("资产面板:已定位图层({sid})"));
            } else {
                self.say("该图层已不存在(文档可能已变更;点「刷新」更新面板)");
            }
        }
        // 替换:打开候选资产选择窗(只列图片类;走命令层,可撤销)
        if let Some(pick) = replace {
            let from = row_rel_of(&rows, &pick.sid);
            self.asset_replace = Some(ReplacePick {
                sid: pick.sid,
                from,
            });
        }
        self.show_replace_window(ui.ctx());
    }

    /// 图片缩略图(40×40;首次解码后缓存,失败记占位不逐帧重试)。
    fn asset_thumb(&mut self, ui: &mut egui::Ui, project: &Path, rel: &str, kind: AssetKind) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        let t = theme::tokens(ui.ctx());
        let Some(tex) = self.asset_thumbs.entry(rel.to_string()).or_insert_with(|| {
            if kind != AssetKind::Image {
                return None;
            }
            load_asset_texture(ui.ctx(), &project.join(rel))
        }) else {
            // 非图片 / 解码失败:类别首字占位
            ui.painter()
                .rect_filled(rect, theme::radius::sm(), t.bg_canvas);
            ui.painter().rect_stroke(
                rect,
                theme::radius::sm(),
                egui::Stroke::new(1.0, t.border),
                egui::StrokeKind::Inside,
            );
            let glyph = match kind {
                AssetKind::Image => Name::KindImage.glyph().to_string(),
                AssetKind::Font => "A".to_string(),
                AssetKind::Style => "#".to_string(),
                AssetKind::Other => "•".to_string(),
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                icons::font(15.0),
                t.text_3,
            );
            return;
        };
        let tex = tex.clone();
        ui.painter().image(
            tex.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }

    /// 「替换引用」候选窗:从图片类资产里挑一个新目标,走 SetImageSrc。
    fn show_replace_window(&mut self, ctx: &egui::Context) {
        let Some(pick) = self.asset_replace.clone() else {
            return;
        };
        let mut action: Option<String> = None;
        let mut open = true;
        egui::Window::new("替换图像引用")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "把节点 {} 的引用 {} 指向另一个资产:",
                    pick.sid,
                    pick.from.as_str()
                ));
                ui.separator();
                let Some(dir) = self.project_dir.clone() else {
                    return;
                };
                let rows = asset_rows(&dir, &self.doc);
                let candidates: Vec<&AssetRow> = rows
                    .iter()
                    .filter(|r| r.kind == AssetKind::Image && r.rel != pick.from)
                    .collect();
                if candidates.is_empty() {
                    ui.label("项目里没有其他图片类资产可换(先放一张图进 assets/)。");
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for cand in &candidates {
                            if ui
                                .selectable_label(false, format!("{}({})", cand.name, cand.rel))
                                .clicked()
                            {
                                action = Some(cand.rel.clone());
                            }
                        }
                    });
                ui.weak("替换走命令层(SetImageSrc,kind 与 attrs 双写同步;Ctrl+Z 可撤销)。");
            });
        if let Some(new_rel) = action {
            let cmd = replace_cmd(&self.doc, &pick.sid, &new_rel);
            match cmd {
                Some(c) => {
                    self.exec(c);
                    self.say(format!("已替换图像引用 → {new_rel}(可撤销)"));
                    self.asset_replace = None;
                }
                None => self.toast_warn("该对象不带 src 图像引用,无法替换"),
            }
        }
        if !open {
            self.asset_replace = None;
        }
    }
}

/// 由引用行反查其所在资产的相对路径(展示用;找不到返回空串)。
fn row_rel_of(rows: &[AssetRow], sid: &str) -> String {
    rows.iter()
        .find(|r| r.refs.iter().any(|h| h.sid == sid))
        .map(|r| r.rel.clone())
        .unwrap_or_default()
}

/// 读图片文件 → egui 纹理(失败返回 None,调用方落类别占位)。
fn load_asset_texture(ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [img.width().max(1) as usize, img.height().max(1) as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("vb-asset-thumb", color, egui::TextureOptions::LINEAR))
}

// ─────────────────────── 单测(文档状态级) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use vb_doc::model::{Node, TextMode};

    fn fixture(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("vb-assets-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("assets")).unwrap();
        std::fs::write(d.join("index.html"), "<html><body>ok</body></html>").unwrap();
        std::fs::write(d.join("assets/hero.png"), b"png").unwrap();
        std::fs::write(d.join("assets/old.png"), b"png").unwrap();
        std::fs::write(d.join("assets/logo.svg"), b"<svg/>").unwrap();
        std::fs::write(d.join("assets/main.css"), "body{}".as_bytes()).unwrap();
        std::fs::write(d.join("assets/brand.ttf"), b"font").unwrap();
        d
    }

    /// 画板上挂一张 img(kind + attrs 双写,与导入一致)。
    fn add_img(doc: &mut Document, src: &str) -> String {
        let sid = doc.alloc_sid();
        let mut n = Node::new(
            NodeKind::Image {
                src: src.to_string(),
            },
            "图像",
            sid,
        );
        n.attrs.insert("src".into(), src.into());
        let parent = doc.artboards[0];
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        doc.nodes.get(id).unwrap().sid.as_str().to_string()
    }

    fn add_text(doc: &mut Document, name: &str) -> String {
        let sid = doc.alloc_sid();
        let n = Node::new(
            NodeKind::Text {
                text: "文字".into(),
                mode: TextMode::Point,
                segments: vec![],
            },
            name,
            sid,
        );
        let parent = doc.artboards[0];
        let id = doc.nodes.insert(n);
        doc.nodes.get_mut(id).unwrap().parent = Some(parent);
        doc.nodes.get_mut(parent).unwrap().children.push(id);
        doc.nodes.get(id).unwrap().sid.as_str().to_string()
    }

    #[test]
    fn asset_rows_list_refs_and_mark_unused() {
        let proj = fixture("rows");
        let mut doc = Document::new_default();
        add_img(&mut doc, "assets/hero.png");
        add_text(&mut doc, "无关文字");
        doc.raw_css
            .push(".bg { background: url('assets/logo.svg') }".into());

        let rows = asset_rows(&proj, &doc);
        let hero = rows.iter().find(|r| r.rel == "assets/hero.png").unwrap();
        assert_eq!(hero.kind, AssetKind::Image);
        assert_eq!(hero.refs.len(), 1, "kind+attrs 双写去重后一行引用");
        assert!(hero.used());
        let old = rows.iter().find(|r| r.rel == "assets/old.png").unwrap();
        assert!(!old.used(), "未被引用的资产必须标「未使用」");
        let logo = rows.iter().find(|r| r.rel == "assets/logo.svg").unwrap();
        assert_eq!(logo.css_refs, 1, "css url() 只计数");
        assert!(logo.used(), "css 引用也算使用中");
        // 排序:未使用的排最前(组内按路径升序 → brand.ttf 领先)
        assert_eq!(
            rows[0].rel,
            "assets/brand.ttf",
            "未使用资产置顶:{:?}",
            rows.iter().map(|r| r.rel.clone()).collect::<Vec<_>>()
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn asset_kind_classifies_by_extension() {
        assert_eq!(asset_kind("a.PNG"), AssetKind::Image);
        assert_eq!(asset_kind("b.webp"), AssetKind::Image);
        assert_eq!(asset_kind("c.TTF"), AssetKind::Font);
        assert_eq!(asset_kind("d.woff2"), AssetKind::Font);
        assert_eq!(asset_kind("e.css"), AssetKind::Style);
        assert_eq!(asset_kind("f.json"), AssetKind::Other);
        assert_eq!(asset_kind("noext"), AssetKind::Other);
    }

    #[test]
    fn replace_cmd_applies_and_reverts_via_command_layer() {
        let proj = fixture("replace");
        let mut doc = Document::new_default();
        let sid = add_img(&mut doc, "assets/old.png");
        // 构造替换命令并经 UndoStack 应用(kind 与 attrs 双写同步)
        let cmd = replace_cmd(&doc, &sid, "assets/hero.png").expect("图像节点可替换");
        let mut stack = vb_doc::undo::UndoStack::new();
        stack.push(&mut doc, cmd).unwrap();
        let id = doc.find_by_sid(&sid).unwrap();
        let n = doc.nodes.get(id).unwrap();
        assert!(
            matches!(&n.kind, NodeKind::Image { src } if src == "assets/hero.png"),
            "kind 源必须同步更新"
        );
        assert_eq!(
            n.attrs.get("src").map(String::as_str),
            Some("assets/hero.png"),
            "attrs 源必须同步更新(导出读 attrs)"
        );
        // 替换后引用关系跟着走:hero 被引用、old 变未使用
        let rows = asset_rows(&proj, &doc);
        assert!(rows
            .iter()
            .find(|r| r.rel == "assets/hero.png")
            .unwrap()
            .used());
        assert!(!rows
            .iter()
            .find(|r| r.rel == "assets/old.png")
            .unwrap()
            .used());
        // 撤销精确还原两处源
        stack.undo(&mut doc).unwrap();
        let n = doc.nodes.get(doc.find_by_sid(&sid).unwrap()).unwrap();
        assert!(
            matches!(&n.kind, NodeKind::Image { src } if src == "assets/old.png"),
            "undo 还原 kind 源"
        );
        assert_eq!(
            n.attrs.get("src").map(String::as_str),
            Some("assets/old.png")
        );
        // 非图像且无 src 属性的节点不支持替换(不静默)
        let text = add_text(&mut doc, "纯文字");
        assert!(replace_cmd(&doc, &text, "assets/hero.png").is_none());
        std::fs::remove_dir_all(&proj).unwrap();
    }
}
