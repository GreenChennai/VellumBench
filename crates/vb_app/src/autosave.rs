//! 自动保存与崩溃恢复(阶段 7 / 副文档 07-1:07-A / 07-B;ADR-VB-L08 Z1)。
//!
//! **落盘位置**(Z1 已确认):项目内 `.vb-autosave/` —— 与项目一起备份/迁移,
//! **绝不覆盖 `index.html`**。快照内容 = 当前文档的 canonical 序列化
//! ([`vb_doc::export::render_project`],与手动保存同一条序列化路径),
//! 外面包一层 JSON 信封(schema 版本 + 保存时刻 + 文件表)。
//!
//! 纪律(与 `recent.rs` / `dock_layout.rs` 同款):
//! - **原子写**:先写 `<名>.tmp` 再改名,防半截 JSON;
//! - **滚动快照**:保留最近 [`MAX_SNAPSHOTS`] 份(`doc.json` 最新,
//!   `doc.1.json` / `doc.2.json` 渐旧)—— 防"快照本身写坏";
//! - **损坏回退**:读最新份解析失败 → 逐份回退更旧的,损坏份记日志不静默;
//! - **可见性**:目录里放 `README.txt` 说明(可安全删除 + git 忽略建议)。
//!
//! 恢复语义:快照文件**只读不写回项目** —— 「恢复快照」把快照内容载入为
//! 当前文档(磁盘 `index.html` 不动,由用户 Ctrl+S 决定何时写回);
//! 「丢弃」删除全部快照;「暂不处理」保留快照继续编辑。
//!
//! 本文件的纯数据/纯函数(快照读写、滚动、LCS 行 diff)带单测;
//! GUI(恢复对话框 / 差异视图 / 自动保存节拍)在 `app/recover.rs`
//! (`app` 的子模块 —— 要摸 `VellumApp` 的私有会话字段,与 `panel_dock` 同款分层)。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 快照目录名(项目内;ADR-VB-L08 Z1)。
pub const DIR_NAME: &str = ".vb-autosave";
/// 滚动快照保留份数(含最新一份)。
pub const MAX_SNAPSHOTS: usize = 3;
/// 快照信封 schema 版本。
pub const SCHEMA_VERSION: u32 = 1;
/// 快照文件名基础名(`doc.json` / `doc.1.json` / `doc.2.json`)。
const BASE: &str = "doc";
/// README 说明文件名(目录可见性;ADR-VB-L08「.gitignore 提示」的落地)。
const README: &str = "README.txt";
/// 恢复中转目录(import 需要目录形态;用完即删,不入滚动序列)。
const RESTORE_DIR: &str = ".restore";

/// 自动保存间隔档位(秒;`0` = 关)。默认 60s(副文档 07 §2.1)。
pub const INTERVAL_STEPS: [u32; 5] = [0, 30, 60, 120, 300];
/// 默认间隔(秒)。
pub const DEFAULT_INTERVAL_SECS: u32 = 60;

/// 恢复提示(打开项目时检测到快照残留;`VellumApp.recover` 会话态)。
#[derive(Debug, Clone)]
pub struct RecoverPrompt {
    /// 最新可读快照(已做过损坏回退)。
    pub snapshot: Snapshot,
    /// 快照文件路径(状态栏/日志定位用)。
    pub path: PathBuf,
}

/// 快照信封:canonical 序列化的文件表 + 元信息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema_version: u32,
    /// 保存时刻(Unix 秒;恢复对话框展示用)。
    pub saved_at_unix: i64,
    /// 相对路径 → 内容(`index.html` 必有;外部 CSS 模式另有 `styles/main.css`)。
    pub files: BTreeMap<String, String>,
}

impl Snapshot {
    /// 取 `index.html` 内容(差异视图主对比面)。
    pub fn index_html(&self) -> Option<&str> {
        self.files.get("index.html").map(String::as_str)
    }
}

// ─────────────────────────── 路径与文件名 ───────────────────────────

/// 项目的快照目录。
pub fn autosave_dir(project: &Path) -> PathBuf {
    project.join(DIR_NAME)
}

/// 滚动序列:下标 0 = 最新,`MAX_SNAPSHOTS-1` = 最旧。
fn slot_name(i: usize) -> String {
    if i == 0 {
        format!("{BASE}.json")
    } else {
        format!("{BASE}.{i}.json")
    }
}

/// 目录内是否残留可读快照(启动检测入口;不落任何新文件)。
pub fn has_snapshots(project: &Path) -> bool {
    read_newest(project).is_some()
}

// ─────────────────────────── 写入(07-A) ───────────────────────────

/// 写一份新快照(原子写 + 滚动 + README 保证)。
///
/// `doc` 经 [`vb_doc::export::render_project`] 做 canonical 序列化
/// (与手动保存同路径),产物只进 `.vb-autosave/`,不触碰项目源文件。
/// 返回保存时刻(Unix 秒;状态栏印记用)。
pub fn write_snapshot(project: &Path, doc: &vb_doc::model::Document) -> Result<i64, String> {
    write_snapshot_keep(project, doc, MAX_SNAPSHOTS)
}

/// 同 [`write_snapshot`],但滚动保留份数由调用方给出
/// (05-4-A2 首选项「数据」页:快照保留数可配,1–[`MAX_SNAPSHOTS`])。
pub fn write_snapshot_keep(
    project: &Path,
    doc: &vb_doc::model::Document,
    keep: usize,
) -> Result<i64, String> {
    let keep = keep.clamp(1, MAX_SNAPSHOTS);
    let dir = autosave_dir(project);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建快照目录失败:{e}"))?;
    ensure_readme(&dir);
    let saved_at = crate::recent::now_secs();
    let res = vb_doc::export::render_project(doc);
    let snap = Snapshot {
        schema_version: SCHEMA_VERSION,
        saved_at_unix: saved_at,
        files: res.files.into_iter().collect(),
    };
    let text = serde_json::to_string(&snap).map_err(|e| format!("序列化快照失败:{e}"))?;

    // 滚动:最旧出列,其余后移一位(Windows 下 rename 不覆盖,先删目标)。
    // 槽位表仍按 MAX_SNAPSHOTS 布置;实际滚动只在前 `keep` 个槽内进行,
    // 缩减保留数后把多出的旧槽清掉(防读侧回退读到超出保留数的陈旧份)。
    for i in keep..MAX_SNAPSHOTS {
        let _ = std::fs::remove_file(dir.join(slot_name(i)));
    }
    for i in (1..keep).rev() {
        let from = dir.join(slot_name(i - 1));
        let to = dir.join(slot_name(i));
        if from.exists() {
            let _ = std::fs::remove_file(&to);
            std::fs::rename(&from, &to).map_err(|e| format!("滚动快照失败:{e}"))?;
        }
    }
    // 原子写:tmp → rename
    let final_path = dir.join(slot_name(0));
    let tmp = dir.join(format!("{BASE}.json.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("写快照失败:{e}"))?;
    let _ = std::fs::remove_file(&final_path);
    std::fs::rename(&tmp, &final_path).map_err(|e| format!("提交快照失败:{e}"))?;
    Ok(saved_at)
}

/// 目录说明文件(幂等):快照可安全删除 + git 忽略建议。
/// ADR-VB-L08「随项目生成 .gitignore 提示」的落地:若项目根没有
/// `.gitignore`,这份说明是用户唯一能看到的提醒,必须写清楚。
fn ensure_readme(dir: &Path) {
    let p = dir.join(README);
    if p.exists() {
        return;
    }
    let text = "这是 Vellum Bench 的自动保存快照目录(.vb-autosave/)。\n\
                \n\
                · 内容是编辑过程的滚动快照,可以安全删除;删除后崩溃恢复不可用。\n\
                · 快照不会覆盖项目里的 index.html / styles/main.css。\n\
                · 若本项目纳入 git,请把下面一行加入项目根的 .gitignore:\n\
                \n\
                \x20   .vb-autosave/\n\
                \n\
                (Vellum Bench 阶段 7 · 数据安全)\n";
    let _ = std::fs::write(&p, text);
}

// ─────────────────────────── 读取与回退(07-B) ───────────────────────────

/// 读指定快照文件(损坏 → 中文原因;供单测与回退循环共用)。
pub fn read_file(path: &Path) -> Result<Snapshot, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("读快照失败:{e}"))?;
    let snap: Snapshot =
        serde_json::from_str(&text).map_err(|e| format!("快照解析失败(疑似写坏):{e}"))?;
    if snap.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "快照版本 {} 与当前 {SCHEMA_VERSION} 不符",
            snap.schema_version
        ));
    }
    Ok(snap)
}

/// 读最新可读快照(损坏回退:`doc.json` → `doc.1.json` → `doc.2.json`;
/// 损坏份逐条记日志,全坏 → `None`)。
pub fn read_newest(project: &Path) -> Option<(Snapshot, PathBuf)> {
    let dir = autosave_dir(project);
    for i in 0..MAX_SNAPSHOTS {
        let p = dir.join(slot_name(i));
        if !p.exists() {
            continue;
        }
        match read_file(&p) {
            Ok(snap) => return Some((snap, p)),
            Err(e) => log::warn!("自动快照回退({}):{e}", p.display()),
        }
    }
    None
}

/// 删除全部快照与中转目录(保存成功 / 丢弃 / 恢复完成后调用)。
pub fn clear(project: &Path) {
    let dir = autosave_dir(project);
    for i in 0..MAX_SNAPSHOTS {
        let _ = std::fs::remove_file(dir.join(slot_name(i)));
    }
    let _ = std::fs::remove_file(dir.join(format!("{BASE}.json.tmp")));
    let _ = std::fs::remove_dir_all(dir.join(RESTORE_DIR));
}

/// 把快照文件表落到中转目录(恢复用;import 需要目录形态)。
fn materialize(project: &Path, snap: &Snapshot) -> Result<PathBuf, String> {
    let dir = autosave_dir(project).join(RESTORE_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建恢复中转目录失败:{e}"))?;
    for (rel, content) in &snap.files {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建快照子目录失败:{e}"))?;
        }
        std::fs::write(&p, content).map_err(|e| format!("写快照文件 {rel} 失败:{e}"))?;
    }
    Ok(dir)
}

/// 快照 → 文档(恢复路径;与 `import_with_layout` 同一条导入/求值链:
/// `import_project` 解析 + `apply_import_layout` 以**真实项目根**求值几何,
/// 因此快照中转目录缺 assets 不影响图像固有尺寸)。
pub fn restore_doc(project: &Path, snap: &Snapshot) -> Result<vb_doc::model::Document, String> {
    let dir = materialize(project, snap)?;
    let r = vb_doc::import::import_project(&dir).map_err(|e| e.to_string())?;
    for w in &r.warnings {
        log::warn!("快照导入:{w}");
    }
    let synthetic = r.synthetic_artboard;
    let mut doc = r.doc;
    for w in vb_layout::apply_import_layout(&mut doc, Some(project), synthetic) {
        log::warn!("快照求值:{w}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(doc)
}

// ─────────────────────────── LCS 行 diff(差异视图) ───────────────────────────

/// diff 行类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// 两方相同。
    Same,
    /// 只在旧方(磁盘)。
    Del,
    /// 只在新方(快照)。
    Add,
}

/// 一行 diff(`kind` + 文本;统一视图按序渲染)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: DiffKind,
    pub text: String,
}

/// LCS 中段规模上限(超限退化为整块替换,防 O(n·m) 爆内存)。
const LCS_CELL_CAP: usize = 2_000_000;

/// 行级 LCS diff(公共前/后缀裁剪 + 中段 DP;统一序输出)。
/// 纯函数,单测直接打这里。
pub fn line_diff(a: &str, b: &str) -> Vec<DiffRow> {
    let la: Vec<&str> = a.lines().collect();
    let lb: Vec<&str> = b.lines().collect();

    // 公共前缀
    let mut pre = 0usize;
    while pre < la.len() && pre < lb.len() && la[pre] == lb[pre] {
        pre += 1;
    }
    // 公共后缀(不与前缀重叠)
    let mut suf = 0usize;
    while suf < la.len() - pre
        && suf < lb.len() - pre
        && la[la.len() - 1 - suf] == lb[lb.len() - 1 - suf]
    {
        suf += 1;
    }

    let mut out = Vec::new();
    for l in &la[..pre] {
        out.push(DiffRow {
            kind: DiffKind::Same,
            text: (*l).to_string(),
        });
    }
    let ma = &la[pre..la.len() - suf];
    let mb = &lb[pre..lb.len() - suf];
    out.extend(mid_diff(ma, mb));
    for l in &la[la.len() - suf..] {
        out.push(DiffRow {
            kind: DiffKind::Same,
            text: (*l).to_string(),
        });
    }
    out
}

/// 中段 diff:规模允许 → LCS;超限 → 整块 Del+Add(并给省略标注行)。
fn mid_diff(ma: &[&str], mb: &[&str]) -> Vec<DiffRow> {
    let mut out = Vec::new();
    if ma.is_empty() && mb.is_empty() {
        return out;
    }
    let cells = (ma.len() + 1).saturating_mul(mb.len() + 1);
    if cells > LCS_CELL_CAP {
        if ma.len() + mb.len() > 0 {
            out.push(DiffRow {
                kind: DiffKind::Same,
                text: format!("…(中段 {}+{} 行过长,整块省略)…", ma.len(), mb.len()),
            });
        }
        for l in ma {
            out.push(DiffRow {
                kind: DiffKind::Del,
                text: (*l).to_string(),
            });
        }
        for l in mb {
            out.push(DiffRow {
                kind: DiffKind::Add,
                text: (*l).to_string(),
            });
        }
        return out;
    }
    // DP:LCS 长度表
    let (h, w) = (ma.len() + 1, mb.len() + 1);
    let mut dp = vec![0u32; h * w];
    for i in (0..ma.len()).rev() {
        for j in (0..mb.len()).rev() {
            dp[i * w + j] = if ma[i] == mb[j] {
                dp[(i + 1) * w + j + 1] + 1
            } else {
                dp[(i + 1) * w + j].max(dp[i * w + j + 1])
            };
        }
    }
    // 回溯产出统一序
    let (mut i, mut j) = (0usize, 0usize);
    while i < ma.len() && j < mb.len() {
        if ma[i] == mb[j] {
            out.push(DiffRow {
                kind: DiffKind::Same,
                text: ma[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if dp[(i + 1) * w + j] >= dp[i * w + j + 1] {
            out.push(DiffRow {
                kind: DiffKind::Del,
                text: ma[i].to_string(),
            });
            i += 1;
        } else {
            out.push(DiffRow {
                kind: DiffKind::Add,
                text: mb[j].to_string(),
            });
            j += 1;
        }
    }
    for l in &ma[i..] {
        out.push(DiffRow {
            kind: DiffKind::Del,
            text: (*l).to_string(),
        });
    }
    for l in &mb[j..] {
        out.push(DiffRow {
            kind: DiffKind::Add,
            text: (*l).to_string(),
        });
    }
    out
}

// ─────────────────────── 单测(快照写入 / 滚动 / 清理 / 回退 / diff) ───────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 独立临时项目目录(测试间互不串扰)。
    fn tmp_project(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vb-autosave-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_then_read_roundtrip_and_readme_visible() {
        let proj = tmp_project("rt");
        let mut doc = vb_doc::model::Document::new_default();
        doc.meta.title = "往返".into();
        write_snapshot(&proj, &doc).unwrap();

        // README 必须真实存在且说明"可安全删除 + gitignore 建议"(用户可见性)
        let readme = autosave_dir(&proj).join(README);
        let text = std::fs::read_to_string(&readme).unwrap();
        assert!(text.contains("安全删除"), "README 必须说明可安全删除");
        assert!(text.contains(".gitignore"), "README 必须带 git 忽略建议");

        let (snap, path) = read_newest(&proj).expect("刚写的快照必须可读");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "doc.json");
        assert!(
            snap.index_html().unwrap().contains("<html"),
            "快照必须是 canonical HTML"
        );
        assert_eq!(
            snap.files.get("index.html").unwrap(),
            &vb_doc::export::render_project(&doc).files[0].1
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn snapshots_rotate_and_clear() {
        let proj = tmp_project("rot");
        for i in 0..5 {
            let mut doc = vb_doc::model::Document::new_default();
            doc.meta.title = format!("第{i}版");
            write_snapshot(&proj, &doc).unwrap();
        }
        let dir = autosave_dir(&proj);
        // 滚动上限:只保留最近 3 份
        for i in 0..MAX_SNAPSHOTS {
            assert!(dir.join(slot_name(i)).exists(), "doc.{i}.json 应存在");
        }
        assert!(!dir.join(slot_name(MAX_SNAPSHOTS)).exists());
        // 最新份 = 第 5 版;更旧一份 = 第 4 版
        let (newest, _) = read_newest(&proj).unwrap();
        assert!(newest.index_html().is_some());
        let older = read_file(&dir.join(slot_name(1))).unwrap();
        assert_ne!(newest, older, "滚动后两份快照内容必须不同");
        // 清理:一个不剩
        clear(&proj);
        assert!(read_newest(&proj).is_none(), "clear 后不得再检出快照");
        assert!(!dir.join(slot_name(0)).exists());
        std::fs::remove_dir_all(&proj).unwrap();
    }

    /// 05-4-A2(首选项「数据」页):快照保留数可配 —— keep=1 只留最新,
    /// 缩减后多出的旧槽被清掉;keep 超上限按上限收敛。
    #[test]
    fn keep_count_controls_rotation_width() {
        let proj = tmp_project("keep");
        let mut doc = vb_doc::model::Document::new_default();
        doc.meta.title = "保留一".into();
        write_snapshot_keep(&proj, &doc, 3).unwrap();
        doc.meta.title = "保留二".into();
        write_snapshot_keep(&proj, &doc, 3).unwrap();
        // 收缩到 1:旧槽(doc.1.json)必须被清掉
        doc.meta.title = "保留三".into();
        write_snapshot_keep(&proj, &doc, 1).unwrap();
        let dir = autosave_dir(&proj);
        assert!(dir.join(slot_name(0)).exists());
        assert!(!dir.join(slot_name(1)).exists(), "keep=1 不得保留旧槽");
        assert!(!dir.join(slot_name(2)).exists());
        let (newest, _) = read_newest(&proj).unwrap();
        assert!(newest.index_html().unwrap().contains("保留三"));
        // keep 超上限:clamp 到 MAX,不 panic、不多写(此前 keep=1 时
        // 旧槽已清,滚动只在现存槽位间移动,补不出第三份)
        doc.meta.title = "超限收敛".into();
        write_snapshot_keep(&proj, &doc, 99).unwrap();
        assert!(dir.join(slot_name(0)).exists());
        assert!(
            dir.join(slot_name(1)).exists(),
            "keep=99 收敛为 MAX:现存槽参与滚动"
        );
        assert!(!dir.join(slot_name(MAX_SNAPSHOTS)).exists(), "不得超出上限");
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn corrupt_newest_falls_back_to_older() {
        let proj = tmp_project("bad");
        let mut doc = vb_doc::model::Document::new_default();
        doc.meta.title = "好快照".into();
        write_snapshot(&proj, &doc).unwrap();
        write_snapshot(&proj, &doc).unwrap();
        // 把最新份写坏(半截 JSON = 断电/强杀的真实形态)
        let dir = autosave_dir(&proj);
        std::fs::write(
            dir.join("doc.json"),
            "{\"schema_version\":1,\"files\":{\"ind",
        )
        .unwrap();
        let (snap, path) = read_newest(&proj).expect("坏最新份必须回退到 doc.1.json");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "doc.1.json");
        assert!(snap.index_html().is_some());
        // 全坏 → None(不 panic、不误报)
        std::fs::write(dir.join("doc.1.json"), "这不是 JSON").unwrap();
        std::fs::write(dir.join("doc.2.json"), "{{{").unwrap();
        assert!(read_newest(&proj).is_none());
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn restore_doc_rebuilds_document_content() {
        let proj = tmp_project("restore");
        // 磁盘项目(带 index.html):模拟"磁盘上有旧版"
        let mut disk = vb_doc::model::Document::new_default();
        disk.meta.title = "磁盘版".into();
        vb_doc::export::write_project(&disk, &proj).unwrap();
        // 快照 = 新版(改了标题)
        let mut mem = vb_doc::model::Document::new_default();
        mem.meta.title = "崩溃前未保存的标题".into();
        write_snapshot(&proj, &mem).unwrap();
        let (snap, _) = read_newest(&proj).unwrap();
        // 恢复:得到的是快照内容,磁盘文件保持旧版
        let doc = restore_doc(&proj, &snap).unwrap();
        assert_eq!(doc.meta.title, "崩溃前未保存的标题");
        let on_disk = std::fs::read_to_string(proj.join("index.html")).unwrap();
        assert!(on_disk.contains("磁盘版"), "恢复不得写回磁盘 index.html");
        assert!(
            !autosave_dir(&proj).join(RESTORE_DIR).exists(),
            "中转目录用完即删"
        );
        std::fs::remove_dir_all(&proj).unwrap();
    }

    #[test]
    fn line_diff_marks_del_add_and_same() {
        let a = "相同1\n删除行\n相同2\n都变的\n";
        let b = "相同1\n相同2\n都变了的\n新增行\n";
        let rows = line_diff(a, b);
        let kinds: Vec<(DiffKind, String)> =
            rows.iter().map(|r| (r.kind, r.text.clone())).collect();
        assert!(kinds.contains(&(DiffKind::Same, "相同1".into())));
        assert!(kinds.contains(&(DiffKind::Del, "删除行".into())));
        assert!(kinds.contains(&(DiffKind::Add, "新增行".into())));
        assert!(kinds.contains(&(DiffKind::Add, "都变了的".into())));
        assert!(kinds.contains(&(DiffKind::Del, "都变的".into())));
        // 顺序:Same 在最前,Del/Add 保持各自文档内的相对顺序
        let first_del = kinds.iter().position(|(k, _)| *k == DiffKind::Del).unwrap();
        let first_add = kinds.iter().position(|(k, _)| *k == DiffKind::Add).unwrap();
        assert!(first_del < first_add || kinds[..first_del.min(first_add)].is_empty());
        // 完全相同 → 全 Same;一方为空 → 全 Add/Del
        assert!(line_diff("x\ny", "x\ny")
            .iter()
            .all(|r| r.kind == DiffKind::Same));
        assert!(line_diff("", "a\nb")
            .iter()
            .all(|r| r.kind == DiffKind::Add));
        assert!(line_diff("a\nb", "")
            .iter()
            .all(|r| r.kind == DiffKind::Del));
    }

    #[test]
    fn line_diff_survives_huge_input_without_blowing_memory() {
        // 超 LCS 规模上限:退化为整块替换,产出仍可用(不 panic/不卡死)
        let a: String = (0..3000).map(|i| format!("旧{i}\n")).collect();
        let b: String = (0..3000).map(|i| format!("新{i}\n")).collect();
        let rows = line_diff(&a, &b);
        assert!(rows.iter().any(|r| r.kind == DiffKind::Del));
        assert!(rows.iter().any(|r| r.kind == DiffKind::Add));
        assert!(
            rows.iter().any(|r| r.text.contains("过长")),
            "超限必须给省略标注"
        );
    }
}
