//! 用户自定义键位(`keymap.json`,阶段 5 / 05-4-A2;台账 09-L)。
//!
//! **范式与 `recent.json` 同款**(02-2-6 纪律):
//! - 与 `workspace.json` / `recent.json` 同目录(`config_dir`);
//! - **原子写**:先写 `.tmp` 再改名,避免半截 JSON;
//! - **损坏回退 + 告警**:解析失败 / 版本不符 → 回退默认键位,中文原因交回调用方;
//! - 首次运行无文件**不是错误**(不告警);
//! - 解析顺序:`VB_KEYMAP` 环境变量(测试/便携用)→ 配置目录 `keymap.json`。
//!
//! **叠加语义**:用户键位是**覆盖层**,不改 `shortcuts::SHORTCUTS` 静态表
//! (门禁自检仍然打在静态表上)。运行时解析顺序([`resolve_effective`]):
//! 1. 用户绑定命中该组合键 → 采用用户绑定;
//! 2. 静态表命中该组合键 → 采用,**除非**该命令已被用户重绑(旧键位必须死);
//! 3. 都没有 → 无绑定。
//!
//! **冲突纪律**:编辑器「保存方案」时检测有效键位集内的重复绑定,
//! 有冲突则**拒绝保存**;加载手改文件时检测到冲突 → 保留首份、丢弃后续并告警。
//! 默认表(不加载任何用户文件)必须与静态表完全一致(门禁测试锁定)。

use std::path::PathBuf;

use egui::Key;
use serde::{Deserialize, Serialize};

use crate::shortcuts::{self, CtxSet, ModMatch, Shortcut};

/// 当前 schema 版本。
pub const SCHEMA_VERSION: u32 = 1;

/// 一条用户绑定:命令 id + 组合键文本(如 `"Ctrl+Shift+P"`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct KeyBinding {
    pub id: String,
    pub combo: String,
}

/// `keymap.json` 的内存态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeymapStore {
    pub schema_version: u32,
    /// 用户覆盖绑定(保存序即优先序;冲突时**首份胜**)。
    #[serde(default)]
    pub bindings: Vec<KeyBinding>,
}

impl Default for KeymapStore {
    /// 默认 = 当前 schema 版本 + 空覆盖表(不用 derive:
    /// u32 的 derive 默认是 0,写盘/比对会与版本门禁打架)。
    fn default() -> Self {
        KeymapStore {
            schema_version: SCHEMA_VERSION,
            bindings: Vec::new(),
        }
    }
}

// ─────────────────────────── 组合键文本编解码 ───────────────────────────

/// 组合键文本(与 `Shortcut::key_text` 同一格式:`Ctrl+Shift+Esc`)。
pub fn combo_text(key: Key, ctrl: bool, shift: bool, alt: bool) -> String {
    let mut s = String::new();
    if ctrl {
        s.push_str("Ctrl+");
    }
    if shift {
        s.push_str("Shift+");
    }
    if alt {
        s.push_str("Alt+");
    }
    s.push_str(shortcuts::key_label(key));
    s
}

/// 解析组合键文本(`Shortcut::key_text` 格式;大小写不敏感)。
/// 修饰键 token:`Ctrl` / `Shift` / `Alt`;末段 = 主键
/// (接受 `shortcuts::key_label` 的全部写法与 egui `Key::from_name` 的写法)。
pub fn parse_combo(text: &str) -> Option<(Key, bool, bool, bool)> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut main: Option<Key> = None;
    // `+` 本身作主键时(如 "Ctrl++"),按 '+' 切分会产生空尾 token:
    // 尾部空 token = 主键是 "+",先把它消费掉再扫修饰键。
    let mut toks: Vec<&str> = text.split('+').map(str::trim).collect();
    let plus_main = toks.len() >= 2 && toks.last() == Some(&"");
    if plus_main {
        toks.pop();
        if toks.last() == Some(&"") {
            toks.pop();
        }
        main = Some(Key::Plus);
    }
    for tok in toks.into_iter().filter(|t| !t.is_empty()) {
        match tok.to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" | "opt" => alt = true,
            _ => {
                // 主键只允许出现一次;`key_label` 的自造名(如 Del/箭头符号)优先
                if main.is_some() {
                    return None;
                }
                main = Some(parse_key_label(tok)?);
            }
        }
    }
    let key = main?;
    // 纯修饰键不能当组合键主键
    if matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    ) {
        return None;
    }
    Some((key, ctrl, shift, alt))
}

/// 解析 `shortcuts::key_label` 的自定义写法(egui `from_name` 覆盖不到的部分),
/// 其余交给 egui 的 `Key::from_name`。
fn parse_key_label(tok: &str) -> Option<Key> {
    match tok {
        "Del" => Some(Key::Delete),
        "←" => Some(Key::ArrowLeft),
        "→" => Some(Key::ArrowRight),
        "↑" => Some(Key::ArrowUp),
        "↓" => Some(Key::ArrowDown),
        _ => Key::from_name(tok),
    }
}

/// 该键是否为纯修饰键(录制新键时忽略,等真正的主键落下)。
pub fn is_modifier_key(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    )
}

// ─────────────────────────── 有效键位集(叠加解析) ───────────────────────────

/// 一条**有效**绑定(静态表或用户覆盖,已经过解析;派发与编辑器共用)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveBinding {
    pub id: String,
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// 生效上下文(继承该命令静态绑定的上下文;无静态绑定 → 除文本编辑外)。
    pub ctx: CtxSet,
}

impl LiveBinding {
    /// 组合键文本(菜单/编辑器展示)。
    pub fn combo_text(&self) -> String {
        combo_text(self.key, self.ctrl, self.shift, self.alt)
    }

    fn hits(&self, key: Key, ctrl: bool, shift: bool, alt: bool) -> bool {
        self.key == key && self.ctrl == ctrl && self.shift == shift && self.alt == alt
    }
}

/// 命令 id 的生效上下文:静态表有绑定 → 继承;否则除文本编辑外全局
/// (面板/画布可用,文本输入不被劫持)。
fn ctx_for(id: &str) -> CtxSet {
    shortcuts::SHORTCUTS
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.ctx)
        .unwrap_or(shortcuts::CTX_NO_TEXT)
}

/// 由静态表构建默认有效键位集(无用户文件时的运行时形态)。
pub fn default_live() -> Vec<LiveBinding> {
    shortcuts::SHORTCUTS
        .iter()
        .map(|s| LiveBinding {
            id: s.id.to_string(),
            key: s.key,
            ctrl: s.ctrl == ModMatch::On,
            shift: s.shift == ModMatch::On,
            alt: s.alt == ModMatch::On,
            ctx: s.ctx,
        })
        .collect()
}

/// 从 store 构建有效键位集。返回 `(有效集, 告警)`:
/// 解析失败的绑定与冲突的后续份被丢弃(首份胜),逐条中文告警。
pub fn build_live(store: &KeymapStore) -> (Vec<LiveBinding>, Vec<String>) {
    let mut warns = Vec::new();
    let mut live: Vec<LiveBinding> = Vec::new();
    for b in &store.bindings {
        let Some((key, ctrl, shift, alt)) = parse_combo(&b.combo) else {
            warns.push(format!("键位方案:无法识别组合键「{}」(已跳过)", b.combo));
            continue;
        };
        let entry = LiveBinding {
            id: b.id.clone(),
            key,
            ctrl,
            shift,
            alt,
            ctx: ctx_for(&b.id),
        };
        // 冲突(同组合键已有绑定):首份胜,后续丢弃 —— 与「保存时拒绝」
        // 同一判定,手改文件由此兜底。
        if let Some(prev) = live.iter().find(|l| hits_same(l, key, ctrl, shift, alt)) {
            warns.push(format!(
                "键位方案:{} 与 {} 同时绑定「{}」,已保留前者",
                prev.id, b.id, b.combo
            ));
            continue;
        }
        live.push(entry);
    }
    (live, warns)
}

fn hits_same(l: &LiveBinding, key: Key, ctrl: bool, shift: bool, alt: bool) -> bool {
    l.key == key && l.ctrl == ctrl && l.shift == shift && l.alt == alt
}

/// 该命令是否已被用户重绑(旧默认键位必须失效的判据)。
pub fn is_overridden(store: &KeymapStore, id: &str) -> bool {
    store.bindings.iter().any(|b| b.id == id)
}

/// 运行时解析(`handle_shortcuts` 唯一入口):
/// 1. 用户绑定命中组合键 → `(命令, 上下文)`;
/// 2. 静态表命中 → `(命令, 上下文)`,**除非**该命令已被覆盖;
/// 3. 无 → None。
pub fn resolve_effective(
    store: &KeymapStore,
    live: &[LiveBinding],
    key: Key,
    ctrl: bool,
    shift: bool,
    alt: bool,
) -> Option<(String, CtxSet)> {
    if let Some(l) = live.iter().find(|l| l.hits(key, ctrl, shift, alt)) {
        return Some((l.id.clone(), l.ctx));
    }
    // 静态表:最具体的一条(与 shortcuts::lookup 同法)
    let mut best: Option<(&Shortcut, u8)> = None;
    for s in shortcuts::SHORTCUTS {
        if s.key != key || !s.ctrl.hit(ctrl) || !s.shift.hit(shift) || !s.alt.hit(alt) {
            continue;
        }
        let spec = s.ctrl.specificity() + s.shift.specificity() + s.alt.specificity();
        if best.map(|(_, b)| spec > b).unwrap_or(true) {
            best = Some((s, spec));
        }
    }
    best.map(|(s, _)| s)
        .filter(|s| !is_overridden(store, s.id))
        .map(|s| (s.id.to_string(), s.ctx))
}

/// 某命令当前的组合键文本(菜单/命令面板展示):用户覆盖优先,其次静态表。
pub fn key_text_for(store: &KeymapStore, id: &str) -> Option<String> {
    if let Some(b) = store.bindings.iter().find(|b| b.id == id) {
        return Some(b.combo.clone());
    }
    shortcuts::key_text_for(id)
}

/// 在**有效键位集**(用户绑定 + 未被覆盖的默认绑定)中找同一组合键的
/// 其它命令(编辑器冲突红标与保存拒绝共用)。
/// 返回冲突命令 id 列表(不含 `except_id` 自身;`except_id` 传正在编辑的命令)。
pub fn combo_conflicts(
    store: &KeymapStore,
    live: &[LiveBinding],
    combo: &(Key, bool, bool, bool),
    except_id: &str,
) -> Vec<String> {
    let (key, ctrl, shift, alt) = *combo;
    let mut out: Vec<String> = live
        .iter()
        .filter(|l| l.hits(key, ctrl, shift, alt) && l.id != except_id)
        .map(|l| l.id.clone())
        .collect();
    if let Some(s) = shortcuts::SHORTCUTS.iter().find(|s| {
        s.id != except_id && !is_overridden(store, s.id) && {
            let dc = s.ctrl == ModMatch::On;
            let ds = s.shift == ModMatch::On;
            let da = s.alt == ModMatch::On;
            s.key == key && dc == ctrl && ds == shift && da == alt
        }
    }) {
        out.push(s.id.to_string());
    }
    out.sort();
    out.dedup();
    out
}

// ─────────────────────────── 落盘(与 recent.rs 同范式) ───────────────────────────

/// `keymap.json` 路径。解析顺序:`VB_KEYMAP` 环境变量(测试/便携用)→
/// 配置目录(与 workspace.json **同一目录解析函数**)下的 `keymap.json`。
pub fn keymap_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VB_KEYMAP") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    Some(crate::app::dock_layout::config_dir()?.join("keymap.json"))
}

/// 读取(真实路径)。返回 `(store, 告警)`;损坏/版本不符 → 回退默认 + 中文告警。
pub fn load() -> (KeymapStore, Option<String>) {
    match keymap_path() {
        Some(p) => load_from(&p),
        None => (
            KeymapStore::default(),
            Some("找不到配置目录,自定义键位不会持久化(可用 VB_KEYMAP 指定)".into()),
        ),
    }
}

/// 读取指定路径。**损坏不静默**;首次运行无文件不是错误。
pub fn load_from(path: &std::path::Path) -> (KeymapStore, Option<String>) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return (KeymapStore::default(), None),
    };
    match serde_json::from_str::<KeymapStore>(&text) {
        Ok(mut st) => {
            if st.schema_version != SCHEMA_VERSION {
                return (
                    KeymapStore::default(),
                    Some(format!(
                        "keymap.json 版本 {} 与当前 {SCHEMA_VERSION} 不符,已回退默认键位",
                        st.schema_version
                    )),
                );
            }
            st.schema_version = SCHEMA_VERSION;
            (st, None)
        }
        Err(e) => (
            KeymapStore::default(),
            Some(format!("keymap.json 解析失败,已回退默认键位:{e}")),
        ),
    }
}

/// 写指定路径(原子性:先写 `.tmp` 再改名)。
pub fn save_to(path: &std::path::Path, st: &KeymapStore) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败:{e}"))?;
    }
    let mut st = st.clone();
    st.schema_version = SCHEMA_VERSION;
    let text = serde_json::to_string_pretty(&st).map_err(|e| format!("序列化键位方案失败:{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("写键位方案失败:{e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("提交键位方案失败:{e}"))
}

/// 写真实路径(无路径 → 报错说明,不静默)。
pub fn save(st: &KeymapStore) -> Result<(), String> {
    let p = keymap_path().ok_or_else(|| "找不到配置目录,键位方案未持久化".to_string())?;
    save_to(&p, st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcuts::InputContext;

    /// 加载守门:默认表(无用户文件)的有效键位集必须与静态表逐一等值
    /// ——「加速键一致」门禁:菜单显示什么,派发就触发什么。
    #[test]
    fn default_live_matches_static_registry() {
        let store = KeymapStore::default();
        let live = default_live();
        assert_eq!(live.len(), shortcuts::SHORTCUTS.len(), "条数一致");
        for s in shortcuts::SHORTCUTS {
            let (id, ctx) = resolve_effective(
                &store,
                &live,
                s.key,
                s.ctrl == ModMatch::On,
                s.shift == ModMatch::On,
                s.alt == ModMatch::On,
            )
            .unwrap_or_else(|| panic!("静态键位 {} 必须仍可解析", s.key_text()));
            assert_eq!(id, s.id, "组合键 {} 的命令漂移", s.key_text());
            assert_eq!(ctx, s.ctx, "{} 的上下文漂移", s.key_text());
        }
        // 静态表本身无冲突(既有门禁已在 shortcuts.rs;此处锁叠加层同态)
        assert!(shortcuts::conflicts().is_empty());
    }

    /// 叠加语义:重绑后旧键位死、新键位活;清除覆盖即恢复默认。
    #[test]
    fn overlay_rebind_kills_old_key_and_keeps_others() {
        let mut store = KeymapStore::default();
        // 把「保存」从 Ctrl+S 挪到 Ctrl+P
        store.bindings.push(KeyBinding {
            id: "file.save".into(),
            combo: "Ctrl+P".into(),
        });
        let (live, warns) = build_live(&store);
        assert!(warns.is_empty(), "{warns:?}");
        // 旧键位死
        assert!(resolve_effective(&store, &live, Key::S, true, false, false).is_none());
        // 新键位活
        let (id, _) = resolve_effective(&store, &live, Key::P, true, false, false).unwrap();
        assert_eq!(id, "file.save");
        // 其它命令不受影响(Ctrl+O 打开)
        let (id, _) = resolve_effective(&store, &live, Key::O, true, false, false).unwrap();
        assert_eq!(id, "file.open");
        // 覆盖展示优先
        assert_eq!(key_text_for(&store, "file.save").as_deref(), Some("Ctrl+P"));
    }

    /// 覆盖层可**清除**默认键位?——不允许:覆盖必然给出新组合键
    /// (编辑器只产 New 绑定);但手写空 combo 必须被解析拒绝。
    #[test]
    fn empty_combo_is_rejected() {
        let mut store = KeymapStore::default();
        store.bindings.push(KeyBinding {
            id: "file.save".into(),
            combo: String::new(),
        });
        let (_, warns) = build_live(&store);
        assert!(!warns.is_empty(), "空组合键必须告警并跳过");
    }

    /// 冲突检测:把「撤销」绑到 Ctrl+S(与未被覆盖的「保存」同键)→
    /// `combo_conflicts` 必须报 file.save;把「保存」也挪走后不再报。
    #[test]
    fn combo_conflict_detection_in_effective_set() {
        let mut store = KeymapStore::default();
        store.bindings.push(KeyBinding {
            id: "edit.undo".into(),
            combo: "Ctrl+S".into(),
        });
        let (live, warns) = build_live(&store);
        assert!(warns.is_empty());
        let s = parse_combo("Ctrl+S").unwrap();
        let hits = combo_conflicts(&store, &live, &s, "edit.undo");
        assert_eq!(hits, vec!["file.save".to_string()], "必须检出与保存的冲突");
        // 同一命令自身不算冲突
        assert!(combo_conflicts(&store, &live, &s, "file.save").contains(&"edit.undo".into()));
        // 保存也挪走 → 无冲突
        store.bindings.push(KeyBinding {
            id: "file.save".into(),
            combo: "Ctrl+Shift+S".into(),
        });
        let (live, _) = build_live(&store);
        assert!(combo_conflicts(&store, &live, &s, "edit.undo").is_empty());
    }

    /// 加载守门:手改文件的重复绑定 → 首份胜 + 中文告警;坏组合键跳过。
    #[test]
    fn hand_edited_store_conflicts_keep_first_with_warning() {
        let store = KeymapStore {
            schema_version: SCHEMA_VERSION,
            bindings: vec![
                KeyBinding {
                    id: "file.save".into(),
                    combo: "Ctrl+J".into(),
                },
                KeyBinding {
                    id: "edit.undo".into(),
                    combo: "Ctrl+J".into(),
                },
                KeyBinding {
                    id: "file.open".into(),
                    combo: "不存在的键".into(),
                },
            ],
        };
        let (live, warns) = build_live(&store);
        assert_eq!(live.len(), 1, "冲突首份胜,坏组合键跳过");
        assert_eq!(live[0].id, "file.save");
        assert_eq!(warns.len(), 2, "冲突与坏组合键各一条告警");
    }

    /// 组合键文本编解码往返:key_label 的全部写法(含 Esc/Del/箭头)。
    #[test]
    fn combo_text_parse_roundtrip() {
        for (key, c, s, a) in [
            (Key::S, true, false, false),
            (Key::Escape, false, false, false),
            (Key::Delete, true, true, false),
            (Key::ArrowLeft, false, false, true),
            (Key::Num0, true, false, false),
            (Key::Plus, true, true, false),
            (Key::F2, false, true, false),
        ] {
            let text = combo_text(key, c, s, a);
            let (k2, c2, s2, a2) =
                parse_combo(&text).unwrap_or_else(|| panic!("组合键 {text} 必须可解析"));
            assert_eq!((k2, c2, s2, a2), (key, c, s, a), "往返失败:{text}");
        }
        // 纯修饰键 / 空串 / 双主键都不是合法组合
        assert!(parse_combo("Ctrl+Shift").is_none());
        assert!(parse_combo("").is_none());
        assert!(parse_combo("Ctrl+S+D").is_none());
        // 大小写不敏感
        assert_eq!(parse_combo("ctrl+shift+p"), parse_combo("Ctrl+Shift+P"));
    }

    /// 落盘往返 + 损坏回退(与 recent.rs 同范式)。
    #[test]
    fn save_load_roundtrip_and_corrupt_fallback() {
        let p = std::env::temp_dir().join(format!("vb-keymap-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&p);
        // 首次运行:无文件不是错误
        let (st, warn) = load_from(&p);
        assert_eq!(st, KeymapStore::default());
        assert!(warn.is_none());

        let mut st = KeymapStore::default();
        st.bindings.push(KeyBinding {
            id: "tool.rect".into(),
            combo: "Ctrl+R".into(),
        });
        save_to(&p, &st).unwrap();
        let (back, warn) = load_from(&p);
        assert!(warn.is_none());
        assert_eq!(back, st, "逐字段还原");
        assert!(!p.with_extension("json.tmp").exists(), "无 .tmp 残留");

        // 损坏 → 回退默认 + 告警
        std::fs::write(&p, "{ 这不是 JSON").unwrap();
        let (st2, warn) = load_from(&p);
        assert_eq!(st2, KeymapStore::default());
        assert!(warn.unwrap().contains("解析失败"));
        // 版本不符 → 回退 + 告警
        std::fs::write(&p, r#"{"schema_version":99,"bindings":[]}"#).unwrap();
        let (_, warn) = load_from(&p);
        assert!(warn.unwrap().contains("版本"));
        let _ = std::fs::remove_file(&p);
    }

    /// 上下文继承:无静态绑定的命令得到「除文本编辑外」上下文;
    /// 有静态绑定的命令继承原上下文(文本编辑态仍可 Ctrl+S)。
    #[test]
    fn ctx_inheritance_for_rebound_ids() {
        assert_eq!(ctx_for("file.save"), shortcuts::CTX_ALL);
        let with_default = ctx_for("file.save");
        assert!(with_default.contains(InputContext::TextEdit));
        let custom = ctx_for("__不存在_的自定义命令__");
        assert!(!custom.contains(InputContext::TextEdit));
        assert!(custom.contains(InputContext::Canvas));
    }
}
