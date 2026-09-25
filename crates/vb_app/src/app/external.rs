//! 项目开存与外部监听:保存、文件监视(热重载)、外部改动印记、
//! 项目就地打开与导入布局求值。
//!
//! 06-1 自 `app.rs` 按「生命周期 / 派发 / 外部监听 / 导航拾取」拆出
//! (纯搬移,零行为变化);`ExternalChange` 路径经 `app.rs` 的
//! `pub use` 保持不变。

use vb_doc::model::Document;
use vb_doc::undo::UndoStack;

use super::{Drag, Tool, VellumApp};

/// 07-R:外部改动印记(哪一步是 Agent/其他进程改的)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalChange {
    /// 触发时刻(Unix 秒)。
    pub at_unix: i64,
    /// 触发时刻(单调钟;信息窗的"多久前"用)。
    pub at: std::time::Instant,
    /// 触发文件(相对项目根;最多记 [`EXTERNAL_FILES_MAX`] 个防刷屏)。
    pub files: Vec<String>,
    /// `true` = 已自动重载;`false` = 本地有未保存编辑未采用。
    pub adopted: bool,
}

/// 07-R:印记记录的触发文件上限(超出部分在信息窗里以"…等 N 个"归纳)。
pub const EXTERNAL_FILES_MAX: usize = 8;

impl VellumApp {
    /// 保存项目(无目录时弹目录选择器)。返回是否真的保存成功
    /// (用户取消选目录 → false;外壳的「保存并关闭」据此留在窗口)。
    pub(crate) fn save_project(&mut self) -> bool {
        if self.project_dir.is_none() {
            let picked = rfd::FileDialog::new()
                .set_title("选择项目保存目录")
                .pick_folder();
            self.project_dir = picked;
        }
        let Some(dir) = self.project_dir.clone() else {
            self.status = "已取消保存".into();
            return false;
        };
        match vb_doc::export::write_project(&self.doc, &dir) {
            Ok(files) => {
                self.doc.rev += 1;
                self.saved_rev = self.doc.rev;
                self.last_self_write = Some(std::time::Instant::now());
                self.status = format!(
                    "已保存 {} → {}",
                    files
                        .iter()
                        .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(", "),
                    dir.display()
                );
                // 02-2-2:保存也记最近(经外壳单点写 recent.json)
                if let Some(tx) = &self.shell_tx {
                    let _ = tx.send(crate::shell::ShellRequest::TouchRecent(dir.clone()));
                }
                // 07-A:保存成功 → 清理对应快照(磁盘已与新内容一致,
                // 快照无意义;残留会误导下次启动的恢复检测)
                crate::autosave::clear(&dir);
                self.autosave_last = None;
                self.autosave_at = None;
                true
            }
            Err(e) => {
                self.toast_error(format!("保存失败:{e}"));
                false
            }
        }
    }

    /// 轮询文件监听(去抖 300ms;Agent 场景默认自动采用外部修改)。
    pub(super) fn poll_watcher(&mut self) {
        let Some(rx) = &self.watcher_rx else {
            return;
        };
        let (mut hit, mut paths) = (false, Vec::new());
        while let Ok(ev) = rx.try_recv() {
            hit = true;
            paths.extend(ev);
        }
        if !hit {
            return;
        }
        // 阶段 2(02-2-5):应用自己生成的 `.vb-cache/`(缩略图缓存)与
        // `.vb-autosave/`(07-A 自动保存快照)都不是外部修改 —— 自产写盘
        // 不该触发热重载提示
        if !paths.is_empty()
            && paths.iter().all(|p| {
                p.components()
                    .any(|c| c.as_os_str() == ".vb-cache" || c.as_os_str() == ".vb-autosave")
            })
        {
            return;
        }
        self.handle_external_event(paths);
    }

    /// 外部改动事件的统一处理(07-R 拆出为独立方法,可单测直打):
    /// 热重载 / 未采用判定 + 外部改动印记(时间与触发文件)。
    pub(super) fn handle_external_event(&mut self, paths: Vec<std::path::PathBuf>) {
        // 文档变更:位图缓存整体失效(B3;文件内容可能已被外部替换)
        self.image_cache.clear();
        if self
            .last_self_write
            .map(|t| t.elapsed() < std::time::Duration::from_millis(800))
            .unwrap_or(false)
        {
            return; // 自己刚写盘,不算外部修改
        }
        // 07-R:触发文件记相对项目根的路径(展示友好;最多 8 个防刷屏)
        let stamp_files = |dir: &std::path::Path| -> Vec<String> {
            paths
                .iter()
                .map(|p| {
                    p.strip_prefix(dir)
                        .unwrap_or(p)
                        .to_string_lossy()
                        .to_string()
                })
                .take(EXTERNAL_FILES_MAX)
                .collect()
        };
        let Some(dir) = self.project_dir.clone() else {
            return;
        };
        if self.doc.rev == self.saved_rev {
            match import_with_layout(&dir) {
                Ok(r) => {
                    let n = r.doc.artboards.len();
                    self.doc = r.doc;
                    self.undo = UndoStack::new();
                    self.selection.clear();
                    // 新 arena 的 NodeId 与旧文档无对应关系,全部悬空引用作废
                    self.isolate_stack.clear();
                    self.pen_points.clear();
                    self.ds_vertex = None;
                    self.editing_text = None;
                    self.drag = Drag::None;
                    self.layer_drag = None;
                    self.editing_layer = None;
                    // 04-5-2:文档整体换血 → 工具态一并回选择(避免残留创建类工具)
                    self.set_tool(Tool::Select);
                    self.saved_rev = self.doc.rev;
                    // 07-R:外部改动印记(已重载)
                    self.external_change = Some(ExternalChange {
                        at_unix: crate::recent::now_secs(),
                        at: std::time::Instant::now(),
                        files: stamp_files(&dir),
                        adopted: true,
                    });
                    self.status = format!("检测到外部修改,已自动采用(Agent 热重载,{n} 画板)");
                }
                Err(e) => self.toast_error(format!("热重载失败:{e}")),
            }
        } else {
            // 07-R:外部改动印记(未采用 —— 本地有未保存编辑)
            self.external_change = Some(ExternalChange {
                at_unix: crate::recent::now_secs(),
                at: std::time::Instant::now(),
                files: stamp_files(&dir),
                adopted: false,
            });
            self.toast_warn("检测到磁盘修改,但本地有未保存编辑(未自动采用;先 Ctrl+S 或撤销)");
        }
    }

    /// 就地打开(旧式路径:**仅无外壳的独立构造兜底**使用)。多窗口模式下
    /// 「打开项目…」经外壳开新窗口(02-5-1 一项目一窗口),不走这里。
    pub(super) fn open_project_inplace(&mut self) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title("打开项目目录(含 index.html)")
            .pick_folder()
        {
            match import_with_layout(&dir) {
                Ok(r) => {
                    let n = r.doc.artboards.len();
                    self.doc = r.doc;
                    self.undo = UndoStack::new();
                    self.selection.clear();
                    self.isolate_stack.clear();
                    self.pen_points.clear();
                    self.ds_vertex = None;
                    self.editing_text = None;
                    self.drag = Drag::None;
                    self.layer_drag = None;
                    self.editing_layer = None;
                    self.project_dir = Some(r.project_dir);
                    self.saved_rev = self.doc.rev;
                    // 04-5-1/2:打开项目 → 自动适合窗口 + 强制回选择工具
                    self.set_tool(Tool::Select);
                    self.fit_pending = true;
                    self.status = format!("已打开 {}(画板 {n})", dir.display());
                }
                Err(e) => self.toast_error(format!("打开失败:{e}")),
            }
        }
    }
}

/// 菜单项按钮:标签 + 右侧键位文本(键位一律查 `shortcuts` 注册表)。
/// 导入 + 内存布局求值(P0-1):打开/热重载/打开目录三条路共用。
/// 声明几何(百分比锚/inset/right|bottom/流式)经 taffy 解析为具体几何供画布
/// 使用;声明本身保留在 style,保存不烤入。缺标记/布局降级告警走 log(不静默)。
pub(super) fn import_with_layout(
    path: &std::path::Path,
) -> Result<vb_doc::import::ImportResult, vb_doc::VbError> {
    let mut r = vb_doc::import::import_project(path)?;
    let dir = r.project_dir.clone();
    for w in &r.warnings {
        log::warn!("{w}");
    }
    let synthetic = r.synthetic_artboard;
    for w in vb_layout::apply_import_layout(&mut r.doc, Some(&dir), synthetic) {
        log::warn!("{w}");
    }
    Ok(r)
}

/// 启动项目目录文件监听(v0.6:Agent/外部编辑改 HTML → 画布热重载)。
pub(super) fn start_watcher(
    project: Option<&std::path::Path>,
) -> Option<std::sync::mpsc::Receiver<Vec<std::path::PathBuf>>> {
    use notify::Watcher;
    let dir = project?;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            if let Ok(ev) = res {
                // 去抖由主循环做(200ms 窗口);带上路径供调用方过滤自产事件
                let _ = tx.send(ev.paths.clone());
            }
        })
        .ok()?;
    watcher
        .watch(dir, notify::RecursiveMode::NonRecursive)
        .ok()?;
    std::mem::forget(watcher); // v0.1:与 App 同生命周期
    Some(rx)
}

/// 递归给子树分配全新 sid(粘贴用:副本是新元素,必须有自己的稳定 id)。
pub(super) fn re_sid_tree(tree: &mut vb_doc::model::NodeTree, doc: &mut Document) {
    tree.node.sid = doc.alloc_sid();
    for c in &mut tree.children {
        re_sid_tree(c, doc);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::assemble::tests::app_fresh;
    use std::path::PathBuf;
    use vb_doc::commands::Command;

    /// 临时单文件项目(最小合法 index.html)。
    fn ext_fixture(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vb-ext-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.html"),
            "<html><body><section class=\"vb-artboard\"><p>a</p></section></body></html>",
        )
        .unwrap();
        dir
    }

    /// 07-R:干净文档收到外部改动 → 自动采用并记印记(时间 + 触发文件);
    /// 本地有未保存编辑 → 判「未采用」但同样留印记;自产目录(autosave/
    /// cache)不进印记(过滤在 poll_watcher,已由 02-2-5 覆盖)。
    #[test]
    fn external_change_stamp_records_files_and_adoption() {
        let _env = crate::ENV_LOCK.lock();
        let dir = ext_fixture("stamp");
        let mut app = app_fresh(Some(dir.clone()));
        assert!(app.external_change.is_none(), "初始无印记");
        // ① 外部改盘 → 热重载 + 已采用印记
        std::fs::write(
            dir.join("index.html"),
            "<html><body><section class=\"vb-artboard\"><p>b</p></section></body></html>",
        )
        .unwrap();
        app.handle_external_event(vec![dir.join("index.html")]);
        let stamp = app.external_change.clone().expect("外部改动必须留印记");
        assert!(stamp.adopted, "干净文档必须自动采用");
        assert_eq!(
            stamp.files,
            vec!["index.html".to_string()],
            "触发文件按项目相对路径记录:{:?}",
            stamp.files
        );
        // ② 本地有未保存编辑 → 未采用,但印记同样记录
        app.exec(Command::SetMetaTitle {
            new: "本地编辑".into(),
            old: None,
        });
        assert!(app.is_dirty());
        app.handle_external_event(vec![dir.join("index.html")]);
        let stamp = app.external_change.clone().unwrap();
        assert!(!stamp.adopted, "脏文档必须判未采用");
        assert_eq!(stamp.files, vec!["index.html".to_string()]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
