//! K3 多写者故障注入 e2e(阶段 5-G / 05-11-3,台账 09-K)。
//!
//! 把「另一进程」当作外部写者:每个 [`Writer`] 是一个独立会话(自己的
//! 文档副本 + saved_rev),共享同一个项目目录 —— 与 `app/external.rs` 的
//! 真实策略一一对应:干净(rev == saved_rev)→ 自动采用重载;脏 →
//! 冲突印记(未采用),由 09-N 三方对比对话框显式裁决。
//!
//! 覆盖三类故障注入(分册 05-11-3):
//! 1. **双进程交替编辑**(不同节点/属性)→ 断言零丢改动;
//! 2. **冲突注入**(同节点同属性,双方都脏)→ 断言冲突被检出、
//!    三方材料(磁盘/快照/内存)齐备、两个方向的裁决都显式生效;
//! 3. **崩溃重放**(B 写快照后写盘半截崩溃)→ 断言损坏可检出/可容忍、
//!    `.vb-autosave` 快照恢复 B 的编辑、恢复后写盘重开无损。
//!
//! GUI 触发链(印记点击 → 对话框 → 裁决动作)由 `app/conflict_dialog.rs`
//! 的单测覆盖(真 VellumApp 实例);本文件只做进程级语义,不启 GUI。

use std::path::{Path, PathBuf};

use vb_app::autosave;
use vb_doc::commands::Command;
use vb_doc::model::{Document, Geom};
use vb_doc::undo::UndoStack;

/// 一个模拟进程:自己的内存文档 + 保存基线 + 撤销栈(与 GUI 同构)。
struct Writer {
    dir: PathBuf,
    doc: Document,
    saved_rev: u64,
    undo: UndoStack,
}

enum ExternalOutcome {
    /// 干净 → 自动采用(重载,零丢失)。
    Adopted,
    /// 脏 → 冲突(未采用;待 09-N 裁决)。
    Conflict,
}

impl Writer {
    /// 打开项目(import_with_layout 同链;布局求值对几何断言必要)。
    fn open(dir: &Path) -> Writer {
        let mut r = vb_doc::import::import_project(dir).expect("打开项目失败");
        let synthetic = r.synthetic_artboard;
        for w in vb_layout::apply_import_layout(&mut r.doc, Some(dir), synthetic) {
            log::warn!("{w}");
        }
        let saved_rev = r.doc.rev;
        Writer {
            dir: dir.to_path_buf(),
            doc: r.doc,
            saved_rev,
            undo: UndoStack::new(),
        }
    }

    /// 本地编辑(经撤销栈,rev 随 apply 递增 —— 与 GUI exec 同规)。
    fn exec(&mut self, cmd: Command) {
        self.undo.push(&mut self.doc, cmd).expect("命令应用失败");
    }

    fn is_dirty(&self) -> bool {
        self.doc.rev != self.saved_rev
    }

    /// 收到外部改动事件(watcher 确认文件已变)—— external.rs 判定复刻。
    fn on_external_change(&mut self) -> ExternalOutcome {
        if self.doc.rev == self.saved_rev {
            let mut r = vb_doc::import::import_project(&self.dir).expect("热重载失败");
            let synthetic = r.synthetic_artboard;
            for w in vb_layout::apply_import_layout(&mut r.doc, Some(&self.dir), synthetic) {
                log::warn!("{w}");
            }
            self.doc = r.doc;
            self.undo = UndoStack::new();
            self.saved_rev = self.doc.rev;
            ExternalOutcome::Adopted
        } else {
            ExternalOutcome::Conflict
        }
    }

    /// 「以磁盘为准重载」(09-N 裁决方向一)。
    fn take_disk(&mut self) {
        assert!(matches!(
            self.on_external_change(),
            ExternalOutcome::Conflict
        ));
        let mut r = vb_doc::import::import_project(&self.dir).expect("重载失败");
        let synthetic = r.synthetic_artboard;
        for w in vb_layout::apply_import_layout(&mut r.doc, Some(&self.dir), synthetic) {
            log::warn!("{w}");
        }
        self.doc = r.doc;
        self.undo = UndoStack::new();
        self.saved_rev = self.doc.rev;
    }

    /// 「以内存为准存回」(09-N 裁决方向二)= Ctrl+S 同路径。
    fn take_memory(&mut self) {
        vb_doc::export::write_project(&self.doc, &self.dir).expect("存回失败");
        self.doc.rev += 1;
        self.saved_rev = self.doc.rev;
    }

    /// 正常保存(磁盘此刻无别人新写入时安全;有 → 先 on_external_change)。
    fn save(&mut self) {
        vb_doc::export::write_project(&self.doc, &self.dir).expect("保存失败");
        self.doc.rev += 1;
        self.saved_rev = self.doc.rev;
    }

    fn disk_html(&self) -> String {
        std::fs::read_to_string(self.dir.join("index.html")).expect("磁盘 index.html 缺失")
    }

    fn node_text(&self, sid: &str) -> String {
        let nid = self.doc.find_by_sid(sid).expect("sid 缺失");
        match &self.doc.nodes.get(nid).expect("节点缺失").kind {
            vb_doc::model::NodeKind::Text { text, .. } => text.clone(),
            k => panic!("不是文本节点 {sid}:{k:?}"),
        }
    }

    fn node_geom(&self, sid: &str) -> Geom {
        let nid = self.doc.find_by_sid(sid).expect("sid 缺失");
        self.doc.nodes.get(nid).expect("节点缺失").geom
    }
}

/// 最小双节点项目:A、B 两个文本节点 + 一个盒节点(几何冲突面)。
fn fixture_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vb-k3-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>K3 夹具</title>
<style>
.ab { position: absolute; left: 10px; top: 20px; width: 100px; height: 40px; }
.cd { position: absolute; left: 200px; top: 20px; width: 100px; height: 40px; }
.bx { position: absolute; left: 30px; top: 90px; width: 120px; height: 60px; background: #346; }
</style>
</head>
<body>
<section class="vb-artboard" data-vb-id="ab0" data-vb-name="画板 1">
<p class="ab" data-vb-id="ta">节点甲</p>
<p class="cd" data-vb-id="tb">节点乙</p>
<div class="bx" data-vb-id="bx"></div>
</section>
</body>
</html>"#,
    )
    .unwrap();
    dir
}

/// 场景 1:双进程交替编辑(不同节点)——断言零丢改动。
/// A 改甲存盘 → B 改乙存盘 → A 改盒几何存盘(先自动采用 B)→ B 收尾。
/// 最终磁盘必须同时含 A 与 B 的全部编辑,且 L1 幂等不被多写者破坏。
#[test]
fn alternation_zero_loss() {
    let dir = fixture_dir("alt");
    let mut a = Writer::open(&dir);

    // A:改甲 → 存盘
    a.exec(Command::SetText {
        sid: "ta".into(),
        new: "甲-A1".into(),
        old: None,
    });
    a.save();
    assert!(a.disk_html().contains("甲-A1"));

    // B:打开(已含 A1)→ 改乙 → 存盘(交错后开的进程自然看到 A 的盘)
    let mut b = Writer::open(&dir);
    assert_eq!(b.node_text("ta"), "甲-A1", "B 必须看到 A 的存盘");
    b.exec(Command::SetText {
        sid: "tb".into(),
        new: "乙-B1".into(),
        old: None,
    });
    b.save();

    // A:干净 → 外部改动自动采用(B 的乙-B1 进来,零丢失)→ 再改盒几何 → 存盘
    assert!(!a.is_dirty(), "A 已存盘,必须是干净态");
    assert!(matches!(a.on_external_change(), ExternalOutcome::Adopted));
    assert_eq!(a.node_text("tb"), "乙-B1", "A 重载后必须含 B 的编辑");
    let g = a.node_geom("bx");
    a.exec(Command::SetGeom {
        sid: "bx".into(),
        new: Geom { x: g.x + 50.0, ..g },
        old: None,
        old_declared: None,
    });
    a.save();

    // B:干净 → 自动采用(A 的几何)→ 收尾改乙 → 存盘
    assert!(matches!(b.on_external_change(), ExternalOutcome::Adopted));
    assert_eq!(
        b.node_geom("bx").x,
        g.x + 50.0,
        "B 重载后必须含 A 的几何编辑"
    );
    b.exec(Command::SetText {
        sid: "tb".into(),
        new: "乙-B2".into(),
        old: None,
    });
    b.save();

    // 终局:第三方重开 → 三段编辑全部在盘(零丢改动)
    let c = Writer::open(&dir);
    assert_eq!(c.node_text("ta"), "甲-A1");
    assert_eq!(c.node_text("tb"), "乙-B2");
    assert_eq!(c.node_geom("bx").x, g.x + 50.0);
    // 多写者逐轮写盘不破坏 canonical 幂等(L1):重导出字节稳定
    let html1 = c.disk_html();
    let reread = Writer::open(&dir);
    vb_doc::export::write_project(&reread.doc, &dir).expect("重写失败");
    let html2 = std::fs::read_to_string(dir.join("index.html")).unwrap();
    assert_eq!(html1, html2, "L1:重导入→重导出必须字节幂等");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 场景 2:冲突注入(同节点同属性,双方都脏)——
/// 冲突必须被检出(不能静默覆盖),三方材料齐备,两个裁决方向都显式生效。
#[test]
fn same_node_conflict_detected_and_resolved() {
    let dir = fixture_dir("conflict");
    let mut a = Writer::open(&dir);
    let mut b = Writer::open(&dir);

    // 双方同时(基于同一基线)改同一节点同一属性 → 都脏
    a.exec(Command::SetText {
        sid: "ta".into(),
        new: "甲-版本A".into(),
        old: None,
    });
    b.exec(Command::SetText {
        sid: "ta".into(),
        new: "甲-版本B".into(),
        old: None,
    });
    assert!(a.is_dirty() && b.is_dirty());

    // B 先存盘;A 的 watcher 随后触发 → 必须判「冲突(未采用)」,
    // 而不是自动重载丢掉 A 的编辑,也不是 A 存盘时静默覆盖 B
    b.save();
    assert!(matches!(a.on_external_change(), ExternalOutcome::Conflict));

    // 09-N 三方材料齐备:磁盘(B 版)/ 内存(A 版)/ 自动快照可读
    assert!(a.disk_html().contains("甲-版本B"), "磁盘 = B 的版本");
    assert_eq!(a.node_text("ta"), "甲-版本A", "内存 = A 的版本");
    autosave::write_snapshot(&dir, &a.doc).expect("快照写入失败");
    let (snap, _) = autosave::read_newest(&dir).expect("快照可读");
    assert!(snap.index_html().unwrap().contains("甲-版本A"));

    // 裁决方向一:以内存为准存回 —— A 显式胜出,B 的版本被覆盖(用户决断,非静默)
    a.take_memory();
    assert!(!a.is_dirty());
    assert!(Writer::open(&dir).node_text("ta") == "甲-版本A");

    // 裁决方向二(重演):以磁盘为准重载 —— 弃本地编辑,磁盘内容胜出
    // (B 的内存态是「甲-版本B」,其存盘后磁盘 ta = 甲-版本B;a2 的
    // 「甲-重演A」被显式放弃 —— 裁决是用户决断,不是静默丢改)
    let mut a2 = Writer::open(&dir);
    a2.exec(Command::SetText {
        sid: "ta".into(),
        new: "甲-重演A".into(),
        old: None,
    });
    b.exec(Command::SetText {
        sid: "tb".into(),
        new: "乙-并存".into(),
        old: None,
    });
    b.save();
    a2.take_disk();
    assert_eq!(a2.node_text("ta"), "甲-版本B", "重载 = 磁盘内容");
    assert_eq!(a2.node_text("tb"), "乙-并存", "B 的并存编辑也在盘");
    assert!(!a2.is_dirty(), "重载后 = 磁盘基线");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 场景 3:崩溃重放 —— B 写快照后写盘半截崩溃;A 可检出损坏/陈旧态,
/// `.vb-autosave` 恢复 B 的编辑;恢复后写盘重开无损。
#[test]
fn crash_midwrite_recovered_by_autosave() {
    let dir = fixture_dir("crash");
    let mut a = Writer::open(&dir);
    let mut b = Writer::open(&dir);

    // B:编辑 → 自动保存快照(autosave 节拍)→ 崩溃:写盘写了一半(截断 HTML)
    b.exec(Command::SetText {
        sid: "tb".into(),
        new: "乙-崩溃前".into(),
        old: None,
    });
    autosave::write_snapshot(&dir, &b.doc).expect("快照写入失败");
    let full = vb_doc::export::render_project(&b.doc);
    let html = full.files.iter().find(|(p, _)| p == "index.html").unwrap();
    let truncated: String = html.1.chars().take(html.1.len() * 2 / 3).collect();
    std::fs::write(dir.join("index.html"), &truncated).unwrap();

    // A:干净态收到外部事件 → 重载不得 panic;结果二选一(诚实):
    //   Ok(宽容解析出残缺文档)或 Err(解析失败)—— 都不吞异常不装没事
    let outcome = vb_doc::import::import_project(&dir);
    match outcome {
        Ok(r) => {
            // 残缺文档也被 canonical 化(A 采用后可以安全工作)
            let mut degraded = r.doc;
            for w in vb_layout::apply_import_layout(&mut degraded, Some(&dir), r.synthetic_artboard)
            {
                log::warn!("{w}");
            }
            let files = vb_doc::export::render_project(&degraded);
            assert!(files.files.iter().any(|(p, _)| p == "index.html"));
        }
        Err(e) => log::warn!("崩溃盘重载报错(合法):{e}"),
    }
    assert!(matches!(a.on_external_change(), ExternalOutcome::Adopted));

    // 恢复:最新快照 = B 崩溃前的完整状态 → restore_doc 取回 B 的编辑
    let (snap, path) = autosave::read_newest(&dir).expect("必须有快照可恢复");
    assert!(
        snap.index_html().unwrap().contains("乙-崩溃前"),
        "快照必须含 B 崩溃前的编辑(路径 {path:?})"
    );
    let recovered = autosave::restore_doc(&dir, &snap).expect("快照恢复失败");
    a.doc = recovered;
    a.saved_rev = a.doc.rev;
    assert_eq!(a.node_text("tb"), "乙-崩溃前", "恢复必须取回 B 的编辑");

    // A 把恢复态写回磁盘 → 第三方(崩溃后重启的 B)重开无损
    a.save();
    let c = Writer::open(&dir);
    assert_eq!(c.node_text("tb"), "乙-崩溃前");
    assert_eq!(c.node_text("ta"), "节点甲", "未卷入崩溃的节点原样");
    let _ = std::fs::remove_dir_all(&dir);
}
