//! 协同会话合并层(阶段 5-G / 05-11-4,台账 09-K;ADR-0031 兑付 ADR-VB-L13)。
//!
//! **定位**:协同 = 「会话合并层」,**文档真相仍是 canonical HTML**。
//! 不把文档模型换成 Automerge/Yrs —— 那会摧毁「文件真相源 + HTML 可 diff +
//! 零浏览器依赖」三条不可妥协原则。本层只做一件事:
//!
//! 1. **连续属性**(位置 x/y / 尺寸 w/h / 透明度 opacity)用
//!    **LWW-Element-State CRDT** 无冲突合并:元素 = (节点 sid, 属性),
//!    全序 = (lamport, writer_id);合并可交换 / 可结合 / 幂等 →
//!    两端以任意顺序交换任意次,状态必然收敛(有单测断言);
//! 2. **结构性变更**(增删节点/改树)**不走此层** —— 走命令 + 三向对比
//!    + rev 乐观锁(K3 的多写者策略,见 `vb_app::external` 与 09-N 对话框);
//! 3. **落盘**仍是 canonical HTML(会话合并只作用于内存文档,导出走
//!    `export::render_project` 同一序列化);
//! 4. **传输** = 本地共享目录(`.vb-collab/` 内 ops JSON 文件交换),
//!    不引入云账号、不引入长连接。选共享目录而非 localhost TCP:
//!    实现最简(纯 std::fs,无端口协商/生命周期管理),且与
//!    「文件是交换介质」的项目气质一致。
//!
//! **使用纪律(会话节拍)**:本地编辑(命令路径)→ `sync_from_doc` 采集
//! 差分为 op → `write_state` 落盘 → `load_peer_states` 读同伴 →
//! `merge_remote` 合并 → `apply_to_doc` 应用(命令路径,rev 推进)→
//! `refresh_baseline` → 导出。echo 抑制靠基线:apply 与 sync 都会刷新。

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use vb_common::units::fmt_num;
use vb_css::Decl;

use crate::commands::Command;
use crate::model::Document;

/// 会话交换目录名(项目内;与 `.vb-autosave` 同族的辅助目录)。
pub const COLLAB_DIR: &str = ".vb-collab";

// ─────────────────────────── CRDT 核心(纯数据 + 纯函数) ───────────────────────────

/// 可协同的连续属性(CRDT 覆盖面;结构性变更不在内)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CollabProp {
    /// left(x)
    PosX,
    /// top(y)
    PosY,
    /// width
    SizeW,
    /// height
    SizeH,
    /// opacity(CSS 声明)
    Opacity,
}

impl CollabProp {
    /// 属性名(报告与调试用)。
    pub fn name(self) -> &'static str {
        match self {
            CollabProp::PosX => "x",
            CollabProp::PosY => "y",
            CollabProp::SizeW => "w",
            CollabProp::SizeH => "h",
            CollabProp::Opacity => "opacity",
        }
    }
}

/// 一条 LWW 操作:元素 = (节点 sid, 属性);值 = 连续量;全序键 =
/// `(lamport, writer)`(Lamport 钟 + 写者 id 决胜,保证两端全序一致)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollabOp {
    pub sid: String,
    pub prop: CollabProp,
    pub value: f64,
    pub lamport: u64,
    pub writer: String,
}

impl CollabOp {
    /// 全序(LWW 决胜键)。
    fn ord_key(&self) -> (u64, &str) {
        (self.lamport, self.writer.as_str())
    }
}

/// 会话 CRDT 状态:LWW-Element-State 的元素集。
/// 合并 = 逐元素取全序最大 → 可交换 / 可结合 / 幂等(有单测)。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollabState {
    ops: BTreeMap<(String, CollabProp), CollabOp>,
}

impl CollabState {
    pub fn new() -> CollabState {
        CollabState::default()
    }

    /// 收录一条 op(同元素重复时按 LWW 取胜者)。
    pub fn apply_local(&mut self, op: CollabOp) {
        self.ops.insert((op.sid.clone(), op.prop), op);
    }

    /// 合并远端状态(逐元素 LWW)。
    pub fn merge(&mut self, other: &CollabState) {
        for (k, op) in &other.ops {
            match self.ops.get(k) {
                Some(cur) if cur.ord_key() >= op.ord_key() => {} // 本地已胜
                _ => {
                    self.ops.insert(k.clone(), op.clone());
                }
            }
        }
    }

    /// 某元素当前胜出值。
    pub fn value(&self, sid: &str, prop: CollabProp) -> Option<f64> {
        self.ops.get(&(sid.to_string(), prop)).map(|o| o.value)
    }

    /// 全部胜出 op(传输格式:JSON 数组 —— 元组键不能直接做 JSON 对象键,
    /// 线格式用 op 数组,读回走 [`CollabState::from_ops`])。
    pub fn to_ops(&self) -> Vec<CollabOp> {
        self.ops.values().cloned().collect()
    }

    /// 从 op 数组重建状态。
    pub fn from_ops(ops: Vec<CollabOp>) -> CollabState {
        let mut st = CollabState::new();
        for op in ops {
            st.apply_local(op);
        }
        st
    }

    pub fn len(&self) -> usize {
        self.ops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

// ─────────────────────────── 会话写者(钟 + 基线 + 文档桥) ───────────────────────────

/// 一个会话参与者:writer id + Lamport 钟 + CRDT 状态 + 同步基线。
/// 基线 = 「已反映进状态的文档值」—— echo 抑制的依据。
#[derive(Debug, Clone)]
pub struct SessionWriter {
    pub id: String,
    clock: u64,
    pub state: CollabState,
    baseline: BTreeMap<(String, CollabProp), f64>,
}

impl SessionWriter {
    pub fn new(id: impl Into<String>) -> SessionWriter {
        SessionWriter {
            id: id.into(),
            clock: 0,
            state: CollabState::new(),
            baseline: BTreeMap::new(),
        }
    }

    /// 记录一条本地 op(钟先走一步,保证单调)。
    fn record(&mut self, sid: &str, prop: CollabProp, value: f64) -> CollabOp {
        self.clock += 1;
        let op = CollabOp {
            sid: sid.to_string(),
            prop,
            value,
            lamport: self.clock,
            writer: self.id.clone(),
        };
        self.state.apply_local(op.clone());
        op
    }

    /// 本地编辑后的差分采集:文档当前值 vs 基线,变化者记为新 op。
    /// (GUI/命令编辑只改文档;会话层在节拍点扫描差分 —— 编辑与合并解耦。)
    pub fn sync_from_doc(&mut self, doc: &Document) -> Vec<CollabOp> {
        let mut fresh = Vec::new();
        for (sid, values) in capture_doc(doc) {
            for (prop, v) in values {
                let key = (sid.clone(), prop);
                if self.baseline.get(&key) != Some(&v) {
                    fresh.push(self.record(&sid, prop, v));
                }
                self.baseline.insert(key, v);
            }
        }
        fresh
    }

    /// 合并远端状态并推进本地钟(Lamport:钟 = max(本地, 见过最大))。
    pub fn merge_remote(&mut self, remote: &CollabState) {
        self.state.merge(remote);
        for op in remote.ops.values() {
            self.clock = self.clock.max(op.lamport);
        }
    }

    /// 把合并后的状态应用到文档(命令路径:SetGeom / SetStyle,rev 随
    /// 每条成功应用递增 —— 与撤销栈 push 同规)。只写「文档当前值 ≠
    /// CRDT 胜出值」的元素;返回应用的条数。
    pub fn apply_to_doc(&self, doc: &mut Document) -> Result<usize, String> {
        let pending: Vec<CollabOp> = self
            .state
            .to_ops()
            .into_iter()
            .filter(|op| doc_value(doc, &op.sid, op.prop) != Some(op.value))
            .collect();
        let mut n = 0;
        for op in &pending {
            apply_op(doc, op)?;
            n += 1;
        }
        Ok(n)
    }

    /// apply 之后刷新基线(把 CRDT 胜出值认定为「已见文档态」)。
    pub fn refresh_baseline(&mut self, doc: &Document) {
        for (sid, values) in capture_doc(doc) {
            for (prop, v) in values {
                self.baseline.insert((sid.clone(), prop), v);
            }
        }
    }

    /// 本地钟当前值(调试/报告用)。
    pub fn lamport(&self) -> u64 {
        self.clock
    }
}

// ─────────────────────────── 文档桥(采集 / 应用) ───────────────────────────

/// 采集文档全部节点的连续属性值(sid → [(prop, value)])。
fn capture_doc(doc: &Document) -> Vec<(String, Vec<(CollabProp, f64)>)> {
    let mut rows = Vec::new();
    for (_, n) in doc.nodes.iter() {
        let mut values = Vec::with_capacity(5);
        values.push((CollabProp::PosX, n.geom.x));
        values.push((CollabProp::PosY, n.geom.y));
        values.push((CollabProp::SizeW, n.geom.w));
        values.push((CollabProp::SizeH, n.geom.h));
        if let Some(o) = n
            .style_get("opacity")
            .and_then(|v| v.trim().parse::<f64>().ok())
        {
            values.push((CollabProp::Opacity, o));
        }
        rows.push((n.sid.as_str().to_string(), values));
    }
    rows
}

/// 文档某节点某属性的当前值(与 capture 同源,应用过滤用)。
fn doc_value(doc: &Document, sid: &str, prop: CollabProp) -> Option<f64> {
    let nid = doc.find_by_sid(sid)?;
    let n = doc.nodes.get(nid)?;
    match prop {
        CollabProp::PosX => Some(n.geom.x),
        CollabProp::PosY => Some(n.geom.y),
        CollabProp::SizeW => Some(n.geom.w),
        CollabProp::SizeH => Some(n.geom.h),
        CollabProp::Opacity => n
            .style_get("opacity")
            .and_then(|v| v.trim().parse::<f64>().ok()),
    }
}

/// 单条 op 应用到文档(命令路径;rev 与撤销栈 push 同规递增)。
fn apply_op(doc: &mut Document, op: &CollabOp) -> Result<(), String> {
    let nid = doc
        .find_by_sid(&op.sid)
        .ok_or_else(|| format!("sid 不存在:{}", op.sid))?;
    let n = doc.nodes.get(nid).ok_or("节点缺失")?;
    match op.prop {
        CollabProp::PosX | CollabProp::PosY | CollabProp::SizeW | CollabProp::SizeH => {
            let mut g = n.geom;
            match op.prop {
                CollabProp::PosX => g.x = op.value,
                CollabProp::PosY => g.y = op.value,
                CollabProp::SizeW => g.w = op.value,
                CollabProp::SizeH => g.h = op.value,
                _ => unreachable!("上面的 match 已排除 Opacity"),
            }
            // old 捕获当前几何;old_declared = Some(当前)与桌面「显式移动
            // 兑换声明几何(materialize)」同语义,撤销可精确还原
            Command::SetGeom {
                sid: op.sid.clone(),
                new: g,
                old: Some(n.geom),
                old_declared: Some(n.geom_declared),
            }
            .apply(doc)
            .map_err(|e| format!("SetGeom({} {}):{e}", op.sid, op.prop.name()))?;
            doc.rev += 1;
            Ok(())
        }
        CollabProp::Opacity => {
            // opacity 是 style 里的一条声明:SetStyle 全量替换 style,
            // new = 现有声明改写该条(与桌面 set_prop_cmd 同构)
            let val = fmt_num(op.value);
            let mut style = n.style.clone();
            if let Some(d) = style.iter_mut().find(|d| d.prop == "opacity") {
                d.value = val;
            } else {
                style.push(Decl {
                    prop: "opacity".into(),
                    value: val,
                    important: false,
                });
            }
            Command::SetStyle {
                sid: op.sid.clone(),
                new: style,
                old: Some(n.style.clone()),
            }
            .apply(doc)
            .map_err(|e| format!("SetStyle(opacity {}):{e}", op.sid))?;
            doc.rev += 1;
            Ok(())
        }
    }
}

// ─────────────────────────── 传输层(本地共享目录) ───────────────────────────

/// 项目的会话交换目录(`<项目>/.vb-collab/`)。
pub fn session_dir(project: &Path) -> std::path::PathBuf {
    project.join(COLLAB_DIR)
}

/// 把本端状态写到共享目录(`ops-<writer>.json`;tmp+rename 原子替换,
/// 与 autosave 同纪律)。全量状态(非增量)—— LWW 合并幂等,重复收发
/// 无害,实现最简。
pub fn write_state(project: &Path, writer: &SessionWriter) -> Result<std::path::PathBuf, String> {
    let dir = session_dir(project);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建会话目录失败:{e}"))?;
    let ops = writer.state.to_ops();
    let json = serde_json::to_string_pretty(&ops).map_err(|e| e.to_string())?;
    let dst = dir.join(format!("ops-{}.json", writer.id));
    let tmp = dir.join(format!("ops-{}.json.tmp", writer.id));
    std::fs::write(&tmp, json).map_err(|e| format!("会话 op 写入失败:{e}"))?;
    std::fs::rename(&tmp, &dst).map_err(|e| format!("会话 op 换名失败:{e}"))?;
    Ok(dst)
}

/// 从共享目录读所有同伴状态(跳过自己与坏文件 —— 坏文件记日志不静默,
/// 单端损坏不拖垮整场会话)。
pub fn load_peer_states(project: &Path, own_writer: &str) -> Vec<CollabState> {
    let dir = session_dir(project);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut peers = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(writer_id) = name
            .strip_prefix("ops-")
            .and_then(|s| s.strip_suffix(".json"))
        else {
            continue; // 非 op 文件(README 等)不管
        };
        if writer_id == own_writer {
            continue;
        }
        match std::fs::read_to_string(entry.path())
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Vec<CollabOp>>(&t).map_err(|e| e.to_string()))
        {
            Ok(ops) => peers.push(CollabState::from_ops(ops)),
            Err(e) => log::warn!("会话 op 文件不可读,跳过({name}):{e}"),
        }
    }
    peers
}

// ─────────────────────────── 单测 ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Document, Geom, Node, NodeKind, TextMode};

    /// 最小文档:一个画板 + 两个文本节点(甲/乙,固定几何)。
    /// (直接构造;不经 import —— 本层单测只关心模型不变量。)
    fn fixture_doc() -> Document {
        let mut doc = Document::new_empty("会话夹具", "zh-CN");
        let ab = doc.new_artboard("画板 1", 800.0, 600.0);
        for (name, text, geom) in [
            (
                "甲",
                "标题甲",
                Geom {
                    x: 10.0,
                    y: 20.0,
                    w: 100.0,
                    h: 40.0,
                },
            ),
            (
                "乙",
                "标题乙",
                Geom {
                    x: 40.0,
                    y: 50.0,
                    w: 120.0,
                    h: 30.0,
                },
            ),
        ] {
            let n = Node::new(
                NodeKind::Text {
                    text: text.into(),
                    mode: TextMode::Point,
                    segments: Vec::new(),
                },
                name,
                doc.alloc_sid(),
            );
            let id = doc.nodes.insert(n);
            doc.nodes.get_mut(id).unwrap().geom = geom;
            doc.nodes.get_mut(ab).unwrap().children.push(id);
            doc.nodes.get_mut(id).unwrap().parent = Some(ab);
        }
        doc
    }

    /// 按图层名取 sid(夹具辅助)。
    fn sid_of(doc: &Document, name: &str) -> String {
        let (id, _) = doc
            .nodes
            .iter()
            .find(|(_, n)| n.name == name)
            .unwrap_or_else(|| panic!("夹具缺节点 {name}"));
        doc.nodes.get(id).unwrap().sid.as_str().to_string()
    }
    /// LWW 合并三律:可交换 / 可结合 / 幂等 + 同钟 writer 决胜。
    #[test]
    fn lww_merge_is_commutative_associative_idempotent() {
        let op = |lamport: u64, writer: &str, value: f64| CollabOp {
            sid: "n1".into(),
            prop: CollabProp::PosX,
            value,
            lamport,
            writer: writer.into(),
        };
        let s1 = |ops: Vec<CollabOp>| CollabState::from_ops(ops);

        let a = s1(vec![op(3, "A", 10.0)]);
        let b = s1(vec![op(5, "B", 20.0)]);
        let c = s1(vec![op(3, "C", 30.0)]);

        // 幂等
        let mut x = a.clone();
        x.merge(&a);
        assert_eq!(x, a, "自己合自己必须不变");
        // 可交换 + 高钟者胜
        let mut ab = a.clone();
        ab.merge(&b);
        let mut ba = b.clone();
        ba.merge(&a);
        assert_eq!(ab, ba, "合并可交换");
        assert_eq!(ab.value("n1", CollabProp::PosX), Some(20.0), "高钟者胜");
        // 同钟:writer id 决胜(字典序大者胜,两端必然一致)
        let tie = s1(vec![op(5, "Z", 99.0)]);
        let mut t = ab.clone();
        t.merge(&tie);
        let mut t2 = tie.clone();
        t2.merge(&ab);
        assert_eq!(t, t2);
        assert_eq!(
            t.value("n1", CollabProp::PosX),
            Some(99.0),
            "同钟 writer 决胜"
        );
        // 可结合:(a ⊔ b) ⊔ c == a ⊔ (b ⊔ c)
        let mut left = ab.clone();
        left.merge(&c);
        let mut bc = b.clone();
        bc.merge(&c);
        let mut right = a.clone();
        right.merge(&bc);
        assert_eq!(left, right, "合并可结合");
    }

    /// 并发不相交编辑(A 移动 + B 缩放 + 透明度,同一节点不同属性)→
    /// 经共享目录交换后两端收敛,双方编辑全部存活(零丢改动)。
    #[test]
    fn concurrent_disjoint_edits_converge() {
        let base = fixture_doc();
        let sid = sid_of(&base, "乙");

        // A:移动(直接改模型模拟 GUI/命令编辑后的文档态)
        let mut da = base.clone();
        let nid = da.find_by_sid(&sid).unwrap();
        let g0 = da.nodes.get(nid).unwrap().geom;
        da.nodes.get_mut(nid).unwrap().geom = Geom {
            x: g0.x + 50.0,
            ..g0
        };

        // B:缩放 + 透明度(与 A 并发,互不可见)
        let mut db = base.clone();
        let nid = db.find_by_sid(&sid).unwrap();
        let g0 = db.nodes.get(nid).unwrap().geom;
        db.nodes.get_mut(nid).unwrap().geom = Geom {
            w: g0.w + 20.0,
            ..g0
        };
        db.nodes.get_mut(nid).unwrap().style_set("opacity", "0.5");

        let mut wa = SessionWriter::new("A");
        let mut wb = SessionWriter::new("B");
        wa.refresh_baseline(&base);
        wb.refresh_baseline(&base);
        assert!(!wa.sync_from_doc(&da).is_empty(), "移动必须产生 op");
        assert!(wb.sync_from_doc(&db).len() >= 2, "缩放 + 透明度至少两条 op");

        // 交换(共享目录往返):A 收 B 的,B 收 A 的
        let proj = std::env::temp_dir().join(format!("vb-collab-x-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&proj);
        std::fs::create_dir_all(&proj).unwrap();
        write_state(&proj, &wa).unwrap();
        write_state(&proj, &wb).unwrap();
        for peer in load_peer_states(&proj, "A") {
            wa.merge_remote(&peer);
        }
        for peer in load_peer_states(&proj, "B") {
            wb.merge_remote(&peer);
        }
        assert_eq!(wa.state, wb.state, "合并后两端 CRDT 状态必须一致");

        // 各自应用到自己的文档 → 导出 → 两侧文件表字节一致(收敛判据;
        // ExternalCss 默认:几何/样式在 styles/main.css,故对拍整表)
        wa.apply_to_doc(&mut da).unwrap();
        wb.apply_to_doc(&mut db).unwrap();
        wa.refresh_baseline(&da);
        wb.refresh_baseline(&db);
        let fa = crate::export::render_project(&da);
        let fb = crate::export::render_project(&db);
        assert_eq!(fa.files, fb.files, "两端导出文件表必须收敛一致");
        let all: String = fa.files.iter().map(|(_, c)| c.as_str()).collect();
        // 双方编辑全部存活
        assert!(all.contains("left: 90px"), "A 的移动必须存活(x 40+50)");
        assert!(all.contains("width: 140px"), "B 的缩放必须存活(w 120+20)");
        assert!(all.contains("opacity"), "B 的透明度必须存活");
        // 回声抑制:apply + 刷新后再 sync,不得产生新 op
        assert!(
            wa.sync_from_doc(&da).is_empty() && wb.sync_from_doc(&db).is_empty(),
            "回声 op 泄漏"
        );
        let _ = std::fs::remove_dir_all(&proj);
    }

    /// 同属性并发冲突:LWW 全序决胜,两端一致选同赢家;收敛 HTML
    /// 往返 L0(值保留)/ L1(字节幂等)不破。
    #[test]
    fn same_prop_conflict_resolves_identically_and_roundtrips() {
        let base = fixture_doc();
        let sid = sid_of(&base, "甲");

        // 双端并发改同一属性(A 值 10,B 值 20;B 钟更高)
        let mut da = base.clone();
        let mut db = base.clone();
        let nid_a = da.find_by_sid(&sid).unwrap();
        da.nodes.get_mut(nid_a).unwrap().geom.x = 10.0;
        let nid_b = db.find_by_sid(&sid).unwrap();
        db.nodes.get_mut(nid_b).unwrap().geom.x = 20.0;

        let mut wa = SessionWriter::new("A");
        let mut wb = SessionWriter::new("B");
        wa.refresh_baseline(&base);
        wb.refresh_baseline(&base);
        wa.sync_from_doc(&da);
        wb.sync_from_doc(&db);

        // 两个方向的交换顺序(先 A 后 B / 先 B 后 A)必须同果
        for (left, right) in [(&wa, &wb), (&wb, &wa)] {
            let mut m1 = left.clone();
            let mut m2 = right.clone();
            m1.merge_remote(&m2.state);
            m2.merge_remote(&m1.state);
            assert_eq!(m1.state, m2.state, "交换顺序不得影响终态");
            assert_eq!(
                m1.state.value(&sid, CollabProp::PosX),
                Some(20.0),
                "B(高钟)必须稳定胜出"
            );
            let mut fa = da.clone();
            let mut fb = db.clone();
            m1.apply_to_doc(&mut fa).unwrap();
            m2.apply_to_doc(&mut fb).unwrap();
            let ra = crate::export::render_project(&fa);
            let rb = crate::export::render_project(&fb);
            assert_eq!(ra.files, rb.files, "两端导出文件表收敛一致(冲突属性同赢家)");

            // 往返 L0:导出全表落盘再导入 → 胜出值保留(x=20)
            // (ExternalCss:几何/样式在 styles/main.css,必须整表落盘)
            let dir = std::env::temp_dir().join(format!("vb-collab-l0-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            for (p, c) in &ra.files {
                let full = dir.join(p);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, c).unwrap();
            }
            let r = crate::import::import_project(&dir).expect("收敛项目必须可导入");
            let nid = r.doc.find_by_sid(&sid).unwrap();
            assert_eq!(
                r.doc.nodes.get(nid).unwrap().geom.x,
                20.0,
                "L0:胜出值往返保留"
            );
            // L1:再导出字节幂等
            let again = crate::export::render_project(&r.doc);
            assert_eq!(ra.files, again.files, "L1:收敛结果必须幂等");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// echo 抑制:远端 op 应用 + 刷新基线后,再 sync 不得产生回声 op。
    #[test]
    fn no_echo_after_apply_and_refresh() {
        let doc = fixture_doc();
        let sid = sid_of(&doc, "乙");
        let mut w = SessionWriter::new("A");
        w.refresh_baseline(&doc);

        // 远端 op(模拟同伴移动乙)进状态并应用
        let mut remote = CollabState::new();
        remote.apply_local(CollabOp {
            sid: sid.clone(),
            prop: CollabProp::PosX,
            value: 111.0,
            lamport: 7,
            writer: "B".into(),
        });
        w.merge_remote(&remote);
        let mut d = doc;
        assert_eq!(w.apply_to_doc(&mut d).unwrap(), 1, "远端值必须落进文档");
        assert_eq!(w.lamport(), 7, "本地钟必须吸收远端 lamport");
        w.refresh_baseline(&d);
        assert!(
            w.sync_from_doc(&d).is_empty(),
            "应用 + 刷新基线后不得有回声 op"
        );
    }

    /// 传输层:原子写 + 坏文件容忍(坏 JSON 跳过不 panic,好文件照常读回)。
    #[test]
    fn transport_tolerates_broken_files() {
        let proj = std::env::temp_dir().join(format!("vb-collab-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&proj);
        std::fs::create_dir_all(session_dir(&proj)).unwrap();
        std::fs::write(session_dir(&proj).join("ops-B.json"), "{broken").unwrap();
        std::fs::write(session_dir(&proj).join("README.txt"), "说明").unwrap();

        let mut w = SessionWriter::new("C");
        w.record("n1", CollabProp::PosX, 5.0);
        let path = write_state(&proj, &w).unwrap();
        assert!(path.exists());
        // C 自己的文件不被当同伴读回
        assert!(
            load_peer_states(&proj, "C").is_empty(),
            "坏文件跳过 + 自己跳过"
        );
        // 对端(D)能读到 C 的
        let peers = load_peer_states(&proj, "D");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].value("n1", CollabProp::PosX), Some(5.0));
        let _ = std::fs::remove_dir_all(&proj);
    }
}
