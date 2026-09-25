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
//! schema:
//! ```json
//! {
//!   "schema_version": 1,
//!   "installed": [{ "dir": "…\\plugins\\example-stats" }],
//!   "grants": { "example-stats": { "authorized": true, "commands": ["edit.select_all"], "granted_at": 0 } }
//! }
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 当前 schema 版本。**加字段必须同时抬版本 + 写迁移**(同 recent.json)。
pub const SCHEMA_VERSION: u32 = 1;

/// 一条已安装插件登记(目录即安装单位;目录内必须有 plugin.json)。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstalledEntry {
    /// 插件目录(含 plugin.json;规范化为绝对路径)。
    pub dir: String,
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
    pub fn load_from(path: &Path) -> (AuthStore, Option<String>) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return (AuthStore::default(), None);
        };
        match serde_json::from_str::<AuthStore>(&text) {
            Ok(s) if s.schema_version == SCHEMA_VERSION => (s, None),
            Ok(s) => (
                AuthStore::default(),
                Some(format!(
                    "plugins.json schema 版本不符(文件 {} / 程序 {SCHEMA_VERSION}),已回退默认授权态",
                    s.schema_version
                )),
            ),
            Err(e) => (
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

    /// 写授权(启用)。manifest 命令白名单快照一并记(审计)。
    pub fn grant(&mut self, plugin_id: &str, commands: &[String]) {
        self.grants.insert(
            plugin_id.to_string(),
            Grant {
                authorized: true,
                commands: commands.to_vec(),
                granted_at: now_secs(),
            },
        );
    }

    /// 撤销授权(停用/拒绝)。
    pub fn revoke(&mut self, plugin_id: &str) {
        self.grants.remove(plugin_id);
    }

    /// manifest 是否被用户改过(授权快照 ≠ 当前白名单 → 须重新授权;
    /// 防止"先授权 A,再偷偷把 manifest 改成 B")。
    pub fn grant_matches(&self, plugin_id: &str, commands: &[String]) -> bool {
        match self.grants.get(plugin_id) {
            Some(g) => g.authorized && g.commands == commands,
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
        let mut s = AuthStore::default();
        assert!(s.install(Path::new("E:/x/plugins/example-stats")));
        assert!(
            !s.install(Path::new("E:/x/plugins/example-stats")),
            "重复安装幂等"
        );
        s.grant("example-stats", &["edit.select_all".to_string()]);
        assert!(s.grant_of("example-stats").unwrap().authorized);
        assert!(s.grant_matches("example-stats", &["edit.select_all".to_string()]));
        // manifest 变了 → 授权不再匹配(须重新走授权弹窗)
        assert!(!s.grant_matches("example-stats", &[]));
        assert!(!s.grant_matches("other", &[]));
        // 原子写 + 读回
        s.save_to(&p).unwrap();
        let (mut s2, warn) = AuthStore::load_from(&p);
        assert!(warn.is_none(), "{warn:?}");
        assert_eq!(s2.installed.len(), 1);
        assert!(s2.grant_matches("example-stats", &["edit.select_all".to_string()]));
        // 撤销 + 卸载
        s2.revoke("example-stats");
        assert!(s2.grant_of("example-stats").is_none());
        assert!(s2.uninstall(Path::new("E:/x/plugins/example-stats")));
        assert!(s2.installed.is_empty());
        let _ = std::fs::remove_file(&p);
    }
}
