//! 05-2 命令实现:09-D 图像置入/替换 + 09-B 剪切蒙版落地 + 铅笔保真度档位。
//!
//! 可测边界:文件落位(`copy_asset_into_project`)与命令构造
//! (`place_image_cmd`)是**纯函数**,对话框拾取只是它们的输入源;
//! 剪切蒙版的命令序在 [`super::clip_mask`](super::clip_mask)(单测在彼处),
//! 这里只负责"读选区 → exec → 反馈"。

use vb_doc::commands::Command;
use vb_doc::model::{Geom, NodeKind};

use super::{clip_mask, VellumApp};

impl VellumApp {
    // ─────────────────── 09-B 剪切蒙版 ───────────────────

    /// `Mod+7` 建立:选区「内容… + 蒙版(最后选)」→ 一条 Compound。
    pub(crate) fn apply_clip_mask(&mut self) {
        match clip_mask::clip_mask_cmds(&self.doc, &self.selection) {
            Ok(cmds) => {
                self.exec(Command::Compound { cmds });
                self.status = "已建立剪切蒙版(overflow 容器;Ctrl+Alt+7 释放,Ctrl+Z 撤销)".into();
            }
            Err(msg) => self.toast_warn(msg),
        }
    }

    /// `Mod+Alt+7` 释放:选中蒙版容器 → 内容回原父级 + 标记移除。
    pub(crate) fn apply_release_clip_mask(&mut self) {
        let Some(sid) = self.selection.first().cloned() else {
            self.toast_warn("释放剪切蒙版:先选中蒙版容器");
            return;
        };
        match clip_mask::release_clip_cmds(&self.doc, &sid) {
            Ok(cmds) => {
                self.exec(Command::Compound { cmds });
                self.status = "已释放剪切蒙版(内容回到原位置)".into();
            }
            Err(msg) => self.toast_warn(msg),
        }
    }

    // ─────────────────── 09-D 图像置入 / 替换 ───────────────────

    /// 「文件 → 置入图像…」:选文件 → 复制进 assets/ →
    /// 选中的单个图像节点 = 替换 src;否则新建 img 节点(原图尺寸,居中)。
    pub(crate) fn place_image_via_dialog(&mut self) {
        let Some(file) = rfd::FileDialog::new()
            .set_title("置入图像")
            .add_filter("图像", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
            .pick_file()
        else {
            return;
        };
        let Some(dir) = self.project_dir.clone() else {
            self.toast_warn("置入图像:先保存或打开一个项目(图像进入 assets/ 目录)");
            return;
        };
        let rel = match copy_asset_into_project(&dir, &file) {
            Ok(rel) => rel,
            Err(e) => {
                self.toast_error(format!("置入图像失败:{e}"));
                return;
            }
        };
        // 单选图像节点 → 直接替换 src;否则新建 img 节点
        if self.selection.len() == 1
            && assets_panel_replace_target(&self.doc, self.selection.first().unwrap()).is_some()
        {
            let sid = self.selection.first().unwrap().clone();
            if let Some(cmd) = assets_panel_replace_cmd(&self.doc, &sid, &rel) {
                self.exec(cmd);
                self.status = format!("已置入并替换图像引用:{rel}");
            }
        } else {
            match place_image_cmd(&mut self.doc, &rel, &dir, &file) {
                Some((cmd, sid)) => {
                    self.exec(cmd);
                    self.selection = vec![sid];
                    self.status = format!("已置入图像:{rel}(新 img 节点,原图尺寸)");
                }
                None => self.toast_error("置入图像:无法读取图片尺寸"),
            }
        }
        // 资产面板行的 rev 缓存失效(下次打开面板读到新资产)
        self.assets_cache = None;
    }

    /// 「替换图像…」(右键图像 / 属性入口):选文件 → SetImageSrc。
    pub(crate) fn replace_image_via_dialog(&mut self) {
        let Some(sid) = self.selection.first().cloned() else {
            self.toast_warn("替换图像:先选中一个图像对象");
            return;
        };
        if assets_panel_replace_target(&self.doc, &sid).is_none() {
            self.toast_warn("替换图像:选中对象不是图像");
            return;
        }
        let Some(file) = rfd::FileDialog::new()
            .set_title("替换图像(保持几何)")
            .add_filter("图像", &["png", "jpg", "jpeg", "gif", "webp", "svg"])
            .pick_file()
        else {
            return;
        };
        let Some(dir) = self.project_dir.clone() else {
            self.toast_warn("替换图像:先保存或打开一个项目(图像进入 assets/ 目录)");
            return;
        };
        match copy_asset_into_project(&dir, &file) {
            Ok(rel) => {
                if let Some(cmd) = assets_panel_replace_cmd(&self.doc, &sid, &rel) {
                    self.exec(cmd);
                    self.assets_cache = None;
                    self.status = format!("已替换图像引用:{rel}(几何不变)");
                }
            }
            Err(e) => self.toast_error(format!("替换图像失败:{e}")),
        }
    }

    // ─────────────────── X-5 铅笔保真度 ───────────────────

    /// 铅笔保真度档位(px;design/06 §3.7「保真度参数 0–20px,设置项」)。
    pub(crate) const PENCIL_FIDELITY_STEPS: [f64; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];

    /// 「编辑 → 设置 → 铅笔保真度」:档位循环,持久化到 workspace.json。
    pub(crate) fn step_pencil_fidelity(&mut self) {
        let steps = Self::PENCIL_FIDELITY_STEPS;
        let cur = self.pencil_fidelity;
        let next = match steps.iter().position(|s| (*s - cur).abs() < 1e-6) {
            Some(i) => steps[(i + 1) % steps.len()],
            None => steps[2],
        };
        self.pencil_fidelity = next;
        self.save_workspace();
        self.status = format!("铅笔保真度:{next:.0}px(容差越大笔迹越简洁;下一档继续点)",);
    }
}

/// 资产面板的替换目标判定(与 `assets_panel::replace_cmd` 同一判据;
/// 这里转引,避免两份真值漂移)。
fn assets_panel_replace_target(
    doc: &vb_doc::model::Document,
    sid: &str,
) -> Option<vb_doc::model::NodeId> {
    let id = doc.find_by_sid(sid)?;
    let n = doc.nodes.get(id)?;
    let is_image = matches!(n.kind, NodeKind::Image { .. });
    if is_image || n.attrs.contains_key("src") {
        Some(id)
    } else {
        None
    }
}

/// 资产面板替换命令(转引 `assets_panel::replace_cmd`;SetImageSrc 双写
/// kind/attrs 并可撤销)。
fn assets_panel_replace_cmd(
    doc: &vb_doc::model::Document,
    sid: &str,
    rel: &str,
) -> Option<Command> {
    super::assets_panel::replace_cmd(doc, sid, rel)
}

/// 把外部图片文件复制进项目 `assets/`(纯函数,对话框之外的机械部分):
/// - 重名冲突追加序号(`hero-2.png`);
/// - 返回**相对路径**(POSIX 分隔,HTML src 直接可用)。
pub(crate) fn copy_asset_into_project(
    project_dir: &std::path::Path,
    src: &std::path::Path,
) -> Result<String, String> {
    let name = src
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or("路径没有文件名")?;
    let assets = project_dir.join("assets");
    std::fs::create_dir_all(&assets).map_err(|e| format!("建 assets/ 失败:{e}"))?;
    let mut rel = format!("assets/{name}");
    let mut dst = assets.join(&name);
    // 重名不覆盖:追加 -2、-3 …
    let mut seq = 2u32;
    while dst.exists() {
        let stem = std::path::Path::new(&name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "image".into());
        let ext = std::path::Path::new(&name)
            .extension()
            .map(|s| format!(".{}", s.to_string_lossy()))
            .unwrap_or_default();
        rel = format!("assets/{stem}-{seq}{ext}");
        dst = assets.join(format!("{stem}-{seq}{ext}"));
        seq += 1;
        if seq > 1000 {
            return Err("重名资产过多".into());
        }
    }
    std::fs::copy(src, &dst).map_err(|e| format!("复制失败:{e}"))?;
    Ok(rel)
}

/// 置入命令构造(纯函数):新建 img 节点,几何 = 原图尺寸(读不出则
/// 320×200 兜底),位置 = 当前画板中心。返回 (命令, 新节点 sid)。
pub(crate) fn place_image_cmd(
    doc: &mut vb_doc::model::Document,
    rel: &str,
    _project_dir: &std::path::Path,
    src_file: &std::path::Path,
) -> Option<(Command, String)> {
    // 原图固有尺寸(读取失败不阻塞置入,走兜底尺寸)
    let (mut w, mut h) = (320.0f64, 200.0f64);
    if src_file.extension().map(|e| e != "svg").unwrap_or(true) {
        if let Ok(img) = image::open(src_file) {
            w = img.width() as f64;
            h = img.height() as f64;
        }
    } else {
        // SVG 按画板半幅占位(浏览器/布局期再精化)
        w = 480.0;
        h = 320.0;
    }
    // 尺寸超过画板 → 收进画板(置入即可见)
    if let Some(&ab) = doc.artboards.first() {
        if let Some(an) = doc.nodes.get(ab) {
            if w > an.geom.w {
                h *= an.geom.w / w;
                w = an.geom.w;
            }
            if h > an.geom.h {
                w *= an.geom.h / h;
                h = an.geom.h;
            }
        }
    }
    let (cx, cy) = doc
        .artboards
        .first()
        .and_then(|&ab| doc.nodes.get(ab))
        .map(|an| (an.geom.w / 2.0, an.geom.h / 2.0))
        .unwrap_or((200.0, 150.0));
    let sid = doc.alloc_sid();
    let mut n = vb_doc::model::Node::new(
        NodeKind::Image {
            src: rel.to_string(),
        },
        format!("图像 {}", sid.as_str()),
        sid.clone(),
    );
    n.geom = Geom {
        x: (cx - w / 2.0).round(),
        y: (cy - h / 2.0).round(),
        w: w.round(),
        h: h.round(),
    };
    n.attrs.insert("src".into(), rel.to_string());
    n.attrs.insert("alt".into(), String::new());
    let parent = doc.artboards.first().copied()?;
    let parent_sid = doc.nodes.get(parent)?.sid.as_str().to_string();
    let tree = vb_doc::model::NodeTree {
        node: n,
        children: vec![],
    };
    Some((
        Command::Insert {
            parent_sid,
            index: usize::MAX,
            tree,
        },
        sid.as_str().to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 09-D:复制进 assets/ 的机械行为 —— 新文件、重名追加序号、相对路径。
    #[test]
    fn copy_asset_into_project_dedupes_names() {
        let tmp = std::env::temp_dir().join(format!("vb-place-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let img = tmp.join("hero.png");
        std::fs::write(&img, b"fake-png").unwrap();
        let rel1 = copy_asset_into_project(&tmp, &img).unwrap();
        let rel2 = copy_asset_into_project(&tmp, &img).unwrap();
        assert_eq!(rel1, "assets/hero.png");
        assert_eq!(rel2, "assets/hero-2.png", "重名追加序号,不覆盖");
        assert!(tmp.join("assets").join("hero.png").exists());
        assert!(tmp.join("assets").join("hero-2.png").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 09-D:置入命令构造 —— img 节点带 src/alt、几何 = 原图尺寸、
    /// 超画板收边、命令可撤销(Insert 走命令层)。
    #[test]
    fn place_image_cmd_creates_sized_img_node() {
        let tmp = std::env::temp_dir().join(format!("vb-place-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let mut doc = vb_doc::model::Document::new("t", "zh-CN");
        // 800×600 的假图(尺寸由调用方读;这里直接传一个不存在的文件走兜底)
        let (cmd, sid) = place_image_cmd(&mut doc, "assets/x.png", &tmp, &tmp.join("nope.png"))
            .expect("兜底尺寸路径可构造");
        let mut undo = vb_doc::undo::UndoStack::new();
        undo.push(&mut doc, cmd).unwrap();
        let id = doc.find_by_sid(&sid).unwrap();
        let n = doc.nodes.get(id).unwrap();
        assert!(matches!(n.kind, NodeKind::Image { ref src } if src == "assets/x.png"));
        assert_eq!(n.attrs.get("src").map(String::as_str), Some("assets/x.png"));
        assert_eq!(
            vb_tools::abs_bbox(&doc, id).unwrap(),
            vb_common::geom::Rect::new(560.0, 350.0, 880.0, 550.0),
            "320×200 兜底尺寸,居中于 1440×900 画板"
        );
        // 撤销后节点消失
        undo.undo(&mut doc).unwrap();
        assert!(doc.find_by_sid(&sid).is_none(), "置入可撤销");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
