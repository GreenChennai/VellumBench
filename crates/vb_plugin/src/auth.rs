//! 授权与安装登记持久化(05-10-3):`plugins.json`。
//!
//! 路径解析顺序(与 workspace.json 同款):`VB_PLUGINS` 环境变量(测试/
//! 便携用)→ 配置目录(`%APPDATA%\VellumBench\` / XDG)下 `plugins.json`。
//!
//! 与 recent.json 同款纪律:
//! - **原子写**:先写 `.tmp` 再改名,避免半截 JSON;
//! - **损坏回退 + 告警**:解析失败 / 版本不符 → 回退默认,中文原因交回
//!   调用方;首次运行无文件**不是错误**(不告警)。
//!
//! schema v2(PLG-02):授权除命令白名单快照外,还绑定**入口可执行的
//! 解析路径 + SHA-256 文件哈希**——「先授权 A 再把 manifest/二进制换成 B」
//! 的整条链路(清单与可执行)都在防线上。v1 文件读取时迁移:v1 授权
//! 无入口绑定 → 一律视为未授权(fail-safe),用户重新确认一次。
//!
//! schema:
//! ```json
//! {
//!   "schema_version": 2,
//!   "installed": [{ "dir": "…\\plugins\\example-stats" }],
//!   "grants": { "example-stats": { "authorized": true, "commands": ["edit.select_all"],
//!               "granted_at": 0, "entry": "…\\example-stats-plugin.exe",
//!               "entry_sha256": "…" } }
//! }
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::digest::sha256_file;

/// 当前 schema 版本。**加字段必须同时抬版本 + 写迁移**(同 recent.json)。
pub const SCHEMA_VERSION: u32 = 2;

/// 一条已安装插件登记(目录即安装单位;目录内必须有 plugin.json)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstalledEntry {
    /// 插件目录(含 plugin.json;规范化为绝对路径)。
    pub dir: String,
}

/// 授权时绑定的入口可执行指纹(PLG-02)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryBinding {
    /// 入口可执行的解析路径(与启动用同一解析器,保证可比)。
    #[serde(default)]
    pub path: String,
    /// 文件 SHA-256(小写 hex);`""` = 授权当时不可哈希(无效绑定)。
    #[serde(default)]
    pub sha256: String,
}

impl EntryBinding {
    /// 对当前文件取指纹。读盘失败 → sha256 为空串(调用方把它当
    /// 「不匹配」处理,fail-safe)。
    pub fn of(path: &Path) -> EntryBinding {
        EntryBinding {
            path: path.to_string_lossy().to_string(),
            sha256: sha256_file(path).unwrap_or_default(),
        }
    }

    /// 空绑定(未绑定;匹配恒 false)。
    pub fn empty() -> EntryBinding {
        EntryBinding {
            path: String::new(),
            sha256: String::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.path.is_empty() && self.sha256.is_empty()
    }
}

/// 一次授权记录(**授权持久化**:用户确认过一次,重启不再弹)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    /// 是否已授权(用户在授权弹窗点了「启用」)。
    pub authorized: bool,
    /// 授权时 manifest 声明的命令白名单快照(审计用;运行时以当前
    /// manifest 为准 —— manifest 变了必须重新走授权)。
    #[serde(default)]
    pub commands: Vec<String>,
    /// 授权时刻(Unix 秒;0 = 未知)。
    #[serde(default)]
    pub granted_at: i64,
    /// 授权时绑定的入口可执行(v2;v1 迁移记录为空 = 须重新授权)。
    #[serde(default)]
    pub entry: EntryBinding,
}

/// `plugins.json` 内存态(宿主持有;单写入者)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuthStore {
    pub schema_version: u32,
    /// 已安装插件(保存序 = 安装序)。
    #[serde(default)]
    pub installed: Vec<InstalledEntry>,
    /// 插件 id → 授权记录。
    #[serde(default)]
    pub grants: BTreeMap<String, Grant>,
}

impl Default for AuthStore {
    fn default() -> Self {
        AuthStore {
            schema_version: SCHEMA_VERSION,
            installed: Vec::new(),
            grants: BTreeMap::new(),
        }
    }
}

impl AuthStore {
    /// 读取(损坏 / 版本不符 → 回退默认 + 中文告警;无文件 = 默认,不告警)。
    ///
    /// v1 → v2 迁移:结构兼容(serde default 补新字段),但 v1 授权没有
    /// 入口绑定,按 PLG-02 一律视为未授权(fail-safe,重授权一次)。
    pub fn load_from(path: &Path) -> (AuthStore, Option<String>) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return (AuthStore::default(), None);
        };
        let version = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("schema_version").and_then(|x| x.as_u64()));
        match (version, serde_json::from_str::<AuthStore>(&text)) {
            (Some(v), Ok(s)) if v == SCHEMA_VERSION as u64 => (s, None),
            // v1:结构可读但授权无入口绑定 → 清空授权(重授权),保留安装登记
            (Some(1), Ok(mut s)) => {
                let migrated = s.grants.len();
                s.grants.clear();
                s.schema_version = SCHEMA_VERSION;
                (
                    s,
                    Some(format!(
                        "plugins.json 从 v1 迁移:{migrated} 条旧授权未绑定入口可执行,已按未授权处理(须重新确认;PLG-02)"
                    )),
                )
            }
            // 缺 schema_version 的"成功解析"不可达(schema_version 无
            // default),防御性兜底:回默认 + 告警
            (None, Ok(_)) => (
                AuthStore::default(),
                Some("plugins.json 缺 schema_version 字段,已回退默认授权态".to_string()),
            ),
            (Some(v), Ok(_)) => (
                AuthStore::default(),
                Some(format!(
                    "plugins.json schema 版本不符(文件 {v} / 程序 {SCHEMA_VERSION}),已回退默认授权态"
                )),
            ),
            (_, Err(e)) => (
                AuthStore::default(),
                Some(format!("plugins.json 解析失败({e}),已回退默认授权态")),
            ),
        }
    }

    /// 原子写(.tmp → rename)。
    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败:{e}"))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| format!("序列化失败:{e}"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("写临时文件失败:{e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("替换 plugins.json 失败:{e}"))?;
        Ok(())
    }

    /// 登记安装(已存在则不重复;返回是否新插入了)。
    pub fn install(&mut self, dir: &Path) -> bool {
        let key = dir.to_string_lossy().to_string();
        if self.installed.iter().any(|e| e.dir == key) {
            return false;
        }
        self.installed.push(InstalledEntry { dir: key });
        true
    }

    /// 移除安装登记(授权记录一并清)。
    pub fn uninstall(&mut self, dir: &Path) -> bool {
        let key = dir.to_string_lossy().to_string();
        let before = self.installed.len();
        self.installed.retain(|e| e.dir != key);
        self.installed.len() != before
    }

    /// 读授权态。
    pub fn grant_of(&self, plugin_id: &str) -> Option<&Grant> {
        self.grants.get(plugin_id)
    }

    /// 写授权(启用)。manifest 命令白名单快照 + 入口可执行指纹一并记
    /// (PLG-02:授权绑定到「这套命令 + 这个二进制」)。
    pub fn grant(&mut self, plugin_id: &str, commands: &[String], entry: EntryBinding) {
        self.grants.insert(
            plugin_id.to_string(),
            Grant {
                authorized: true,
                commands: commands.to_vec(),
                granted_at: now_secs(),
                entry,
            },
        );
    }

    /// 撤销授权(停用/拒绝)。
    pub fn revoke(&mut self, plugin_id: &str) {
        self.grants.remove(plugin_id);
    }

    /// 授权是否仍匹配当前声明(PLG-02 完整判据):
    /// ① 已授权;② 命令白名单与快照一致(防改 manifest);
    /// ③ 入口绑定有效且路径 + 文件哈希与当前一致(防换二进制)。
    /// 任一不满足 → 须重新授权。
    pub fn grant_matches(
        &self,
        plugin_id: &str,
        commands: &[String],
        current_entry: &EntryBinding,
    ) -> bool {
        match self.grants.get(plugin_id) {
            Some(g) => {
                g.authorized
                    && g.commands == commands
                    && !g.entry.is_empty()
                    && !current_entry.is_empty()
                    && g.entry.path == current_entry.path
                    && g.entry.sha256 == current_entry.sha256
            }
            None => false,
        }
    }
}

/// Unix 秒(测试同款口径)。
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 默认授权文件路径:`VB_PLUGINS` 环境变量 → 配置目录下 `plugins.json`。
/// (与 workspace.json 的 `VB_WORKSPACE` 同款;测试指临时文件。)
pub fn default_auth_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VB_PLUGINS") {
        if !p.trim().is_empty() {
            return Some(PathBuf::from(p));
        }
    }
    Some(config_dir()?.join("plugins.json"))
}

/// 配置目录(与 vb_app::dock_layout::config_dir 同口径;此处复制实现,
/// 避免 UI crate 依赖倒挂)。
fn config_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var("APPDATA").ok().map(PathBuf::from)
    } else {
        std::env::var("XDG_CONFIG_HOME")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".config"))
            })
    }?;
    Some(base.join("VellumBench"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("vb-plugin-auth-{}-{tag}.json", std::process::id()))
    }

    #[test]
    fn missing_file_is_not_an_error() {
        let p = tmp_path("missing");
        let _ = std::fs::remove_file(&p);
        let (s, warn) = AuthStore::load_from(&p);
        assert!(warn.is_none(), "首次运行无文件不告警");
        assert!(s.installed.is_empty());
    }

    #[test]
    fn corrupt_file_falls_back_with_warning() {
        let p = tmp_path("corrupt");
        std::fs::write(&p, "{ not json").unwrap();
        let (_s, warn) = AuthStore::load_from(&p);
        assert!(warn.is_some(), "损坏必须中文告警:{warn:?}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn version_mismatch_falls_back_with_warning() {
        let p = tmp_path("ver");
        std::fs::write(&p, r#"{"schema_version":99,"installed":[]}"#).unwrap();
        let (_s, warn) = AuthStore::load_from(&p);
        assert!(warn.is_some(), "{warn:?}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn install_grant_uninstall_roundtrip() {
        let p = tmp_path("roundtrip");
        let _ = std::fs::remove_file(&p);
        let exe = tmp_path("entry.exe");
        std::fs::write(&exe, b"plugin-binary-v1").unwrap();
        let mut s = AuthStore::default();
        assert!(s.install(Path::new("E:/x/plugins/example-stats")));
        assert!(
            !s.install(Path::new("E:/x/plugins/example-stats")),
            "重复安装幂等"
        );
        let binding = EntryBinding::of(&exe);
        s.grant(
            "example-stats",
            &["edit.select_all".to_string()],
            binding.clone(),
        );
        assert!(s.grant_of("example-stats").unwrap().authorized);
        assert!(s.grant_matches("example-stats", &["edit.select_all".to_string()], &binding));
        // manifest 变了 → 授权不再匹配(须重新走授权弹窗)
        assert!(!s.grant_matches("example-stats", &[], &binding));
        assert!(!s.grant_matches("other", &["edit.select_all".to_string()], &binding));
        // PLG-02:二进制被换 → 授权失配(篡改检测)
        std::fs::write(&exe, b"plugin-binary-TAMPERED").unwrap();
        let tampered = EntryBinding::of(&exe);
        assert!(
            !s.grant_matches("example-stats", &["edit.select_all".to_string()], &tampered),
            "入口哈希不一致必须判定未授权"
        );
        // 路径不同(即使哈希恰好同)也不匹配
        let mut other_path = binding.clone();
        other_path.path = "E:/elsewhere/plugin.exe".into();
        assert!(!s.grant_matches(
            "example-stats",
            &["edit.select_all".to_string()],
            &other_path
        ));
        // 恢复内容后重新匹配
        std::fs::write(&exe, b"plugin-binary-v1").unwrap();
        assert!(s.grant_matches(
            "example-stats",
            &["edit.select_all".to_string()],
            &EntryBinding::of(&exe)
        ));
        // 原子写 + 读回
        s.save_to(&p).unwrap();
        let (mut s2, warn) = AuthStore::load_from(&p);
        assert!(warn.is_none(), "{warn:?}");
        assert_eq!(s2.installed.len(), 1);
        assert!(s2.grant_matches(
            "example-stats",
            &["edit.select_all".to_string()],
            &EntryBinding::of(&exe)
        ));
        // 撤销 + 卸载
        s2.revoke("example-stats");
        assert!(s2.grant_of("example-stats").is_none());
        assert!(s2.uninstall(Path::new("E:/x/plugins/example-stats")));
        assert!(s2.installed.is_empty());
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(&exe);
    }

    /// PLG-02 fail-safe:入口文件消失 / 空绑定 / 空哈希 → 一律未授权。
    #[test]
    fn grant_without_valid_entry_binding_is_rejected() {
        let mut s = AuthStore::default();
        // 授权当时拿不到指纹(空绑定)→ 永不匹配
        s.grant("a", &[], EntryBinding::empty());
        assert!(!s.grant_matches("a", &[], &EntryBinding::empty()));
        // 当前文件读不到(哈希为空)→ 不匹配
        let missing = EntryBinding {
            path: "Z:/no/such/file.exe".into(),
            sha256: String::new(),
        };
        s.grant("b", &[], EntryBinding::of(Path::new("Z:/was/there.exe")));
        assert!(!s.grant_matches("b", &[], &missing));
    }

    /// v1 → v2 迁移:旧授权(无入口绑定)被清空并告警,安装登记保留。
    #[test]
    fn v1_grants_migrate_to_unauthorized() {
        let p = tmp_path("v1migration");
        std::fs::write(
            &p,
            r#"{"schema_version":1,"installed":[{"dir":"E:/x/plugins/example-stats"}],
                "grants":{"example-stats":{"authorized":true,"commands":["edit.select_all"],"granted_at":42}}}"#,
        )
        .unwrap();
        let (s, warn) = AuthStore::load_from(&p);
        assert!(warn.is_some(), "v1 迁移必须中文告警:{warn:?}");
        assert_eq!(s.schema_version, SCHEMA_VERSION);
        assert_eq!(s.installed.len(), 1, "安装登记保留");
        assert!(
            s.grants.is_empty(),
            "v1 授权无入口绑定,必须按未授权处理(fail-safe)"
        );
        assert!(s.grant_of("example-stats").is_none());
        let _ = std::fs::remove_file(&p);
    }
}
