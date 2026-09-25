//! `plugin.json` manifest(05-10-2):严格 schema 解析。
//!
//! 纪律:
//! - **未知字段拒绝**(与 workspace.json 的宽容策略相反 —— manifest 是
//!   权限声明,拼错的字段名等于静默放宽权限,必须报错);
//! - **中文报错**:所有校验失败给出可读中文原因(含字段名);
//! - **entry 三种形态**:`bin:<名>`(与宿主同目录的 workspace 内置插件
//!   可执行,冒烟/测试用)、绝对路径、相对 manifest 目录的路径。
//!
//! schema:
//!
//! ```json
//! {
//!   "id": "example-stats",
//!   "name": "统计元素",
//!   "version": "0.1.0",
//!   "entry": "bin:example-stats-plugin",
//!   "commands": ["edit.select_all"],
//!   "panels": [{ "id": "stats", "title": "元素统计" }],
//!   "exports": [{ "id": "png2x", "title": "导出 PNG @2x", "format": "png" }]
//! }
//! ```

use std::path::{Path, PathBuf};

use serde_json::Value;

/// 面板声明(manifest `panels` 元素)。
#[derive(Debug, Clone, PartialEq)]
pub struct PanelDecl {
    /// 面板 id(插件侧 `panel/setUI` 引用;`[a-z0-9_-]{1,63}`)。
    pub id: String,
    /// 面板标题(插件坞 Tab 文本,≤40 字符)。
    pub title: String,
}

/// 导出动作声明(manifest `exports` 元素)。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDecl {
    /// 动作 id(≤64 字符)。
    pub id: String,
    /// 按钮标题(≤40 字符)。
    pub title: String,
    /// 输出格式(当前支持 png / svg;宿主执行,输出只落用户选择的目录)。
    pub format: String,
}

/// 插件 manifest(解析并校验后的内存态)。
#[derive(Debug, Clone, PartialEq)]
pub struct PluginManifest {
    /// 插件 id(`[a-z0-9][a-z0-9_-]{1,63}`;授权登记的主键)。
    pub id: String,
    /// 显示名(1–60 字符)。
    pub name: String,
    /// 语义化版本(`x.y.z`)。
    pub version: String,
    /// 入口可执行(manifest 原文;解析规则见模块头)。
    pub entry: String,
    /// **宿主命令白名单**(权限;插件只能调用其中列出的命令)。
    pub commands: Vec<String>,
    /// 注册的面板(受控 UI;渲染进「插件」次级坞组)。
    pub panels: Vec<PanelDecl>,
    /// 注册的导出动作(宿主执行,输出只落用户选择的目录)。
    pub exports: Vec<ExportDecl>,
}

impl PluginManifest {
    /// 严格解析(未知字段 / 缺字段 / 字段非法 → 中文 Err)。
    pub fn parse(text: &str) -> Result<PluginManifest, String> {
        let v: Value =
            serde_json::from_str(text).map_err(|e| format!("plugin.json 不是合法 JSON:{e}"))?;
        let obj = v
            .as_object()
            .ok_or_else(|| "plugin.json 顶层必须是对象".to_string())?;
        const KNOWN: &[&str] = &[
            "id", "name", "version", "entry", "commands", "panels", "exports",
        ];
        for key in obj.keys() {
            if !KNOWN.contains(&key.as_str()) {
                return Err(format!(
                    "plugin.json 存在未知字段「{key}」(manifest 是权限声明,拼写错误的字段一律拒绝)"
                ));
            }
        }
        let id = str_field(obj, "id")?;
        if id.len() < 2
            || id.len() > 64
            || !id
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            || !id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(format!(
                "plugin.json 的 id「{id}」非法:须为 2–64 位小写字母/数字/中划线/下划线,且以字母或数字开头"
            ));
        }
        let name = str_field(obj, "name")?;
        if name.trim().is_empty() || name.chars().count() > 60 {
            return Err("plugin.json 的 name 须为 1–60 字符的非空文本".into());
        }
        let version = str_field(obj, "version")?;
        if !is_semver(&version) {
            return Err(format!(
                "plugin.json 的 version「{version}」非法:须为 x.y.z 三段数字"
            ));
        }
        let entry = str_field(obj, "entry")?;
        if entry.trim().is_empty() || entry.chars().count() > 500 {
            return Err("plugin.json 的 entry 须为非空的入口可执行路径(或 bin:<名>)".into());
        }
        // commands:必填数组(可为空 = 零权限;**默认零权限**必须显式声明)
        let commands = match obj.get("commands") {
            None => {
                return Err("plugin.json 缺少字段「commands」(宿主命令白名单,可为空数组)".into())
            }
            Some(Value::Array(a)) => {
                let mut out = Vec::new();
                for it in a {
                    let s = it.as_str().ok_or_else(|| {
                        "plugin.json 的 commands 元素必须是字符串(命令 id)".to_string()
                    })?;
                    if !s.contains('.') || s.len() > 128 {
                        return Err(format!(
                            "plugin.json 的命令 id「{s}」非法:须为「域.动作」形态(如 edit.undo)"
                        ));
                    }
                    if out.contains(&s.to_string()) {
                        return Err(format!("plugin.json 的 commands 重复列出「{s}」"));
                    }
                    out.push(s.to_string());
                }
                out
            }
            Some(_) => return Err("plugin.json 的 commands 必须是数组".into()),
        };
        // panels:可选数组
        let panels = match obj.get("panels") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) => {
                let mut out = Vec::new();
                for it in a {
                    let o = it.as_object().ok_or_else(|| {
                        "plugin.json 的 panels 元素必须是对象 {id,title}".to_string()
                    })?;
                    for key in o.keys() {
                        if key != "id" && key != "title" {
                            return Err(format!("plugin.json 的 panels 条目存在未知字段「{key}」"));
                        }
                    }
                    let pid = str_field(o, "id")?;
                    if pid.is_empty()
                        || pid.len() > 64
                        || !pid
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                    {
                        return Err(format!(
                            "plugin.json 的面板 id「{pid}」非法:须为 1–64 位字母/数字/中划线/下划线"
                        ));
                    }
                    let title = str_field(o, "title")?;
                    if title.trim().is_empty() || title.chars().count() > 40 {
                        return Err("plugin.json 的面板 title 须为 1–40 字符".into());
                    }
                    if out.iter().any(|p: &PanelDecl| p.id == pid) {
                        return Err(format!("plugin.json 的 panels 重复声明「{pid}」"));
                    }
                    out.push(PanelDecl { id: pid, title });
                }
                out
            }
            Some(_) => return Err("plugin.json 的 panels 必须是数组".into()),
        };
        // exports:可选数组
        let exports = match obj.get("exports") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) => {
                let mut out = Vec::new();
                for it in a {
                    let o = it.as_object().ok_or_else(|| {
                        "plugin.json 的 exports 元素必须是对象 {id,title,format}".to_string()
                    })?;
                    for key in o.keys() {
                        if !matches!(key.as_str(), "id" | "title" | "format") {
                            return Err(format!(
                                "plugin.json 的 exports 条目存在未知字段「{key}」"
                            ));
                        }
                    }
                    let eid = str_field(o, "id")?;
                    if eid.is_empty() || eid.len() > 64 {
                        return Err("plugin.json 的导出动作 id 须为 1–64 字符".into());
                    }
                    let title = str_field(o, "title")?;
                    if title.trim().is_empty() || title.chars().count() > 40 {
                        return Err("plugin.json 的导出动作 title 须为 1–40 字符".into());
                    }
                    let format = str_field(o, "format")?.to_ascii_lowercase();
                    if !matches!(format.as_str(), "png" | "svg") {
                        return Err(format!(
                            "plugin.json 的导出格式「{format}」不支持:当前仅 png / svg"
                        ));
                    }
                    if out.iter().any(|e: &ExportDecl| e.id == eid) {
                        return Err(format!("plugin.json 的 exports 重复声明「{eid}」"));
                    }
                    out.push(ExportDecl {
                        id: eid,
                        title,
                        format,
                    });
                }
                out
            }
            Some(_) => return Err("plugin.json 的 exports 必须是数组".into()),
        };
        Ok(PluginManifest {
            id,
            name,
            version,
            entry,
            commands,
            panels,
            exports,
        })
    }

    /// 从文件加载(UTF-8;错误带文件路径)。
    pub fn load_file(path: &Path) -> Result<PluginManifest, String> {
        let text =
            std::fs::read_to_string(path).map_err(|e| format!("读取 {}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}:{e}", path.display()))
    }

    /// 解析 entry → 可执行文件绝对路径:
    /// - `bin:<名>` → 宿主可执行同目录下的 `<名>(.exe)`(workspace 内置
    ///   插件;cargo 运行时宿主与插件 bin 同在 target/debug);
    /// - 绝对路径 → 原样;
    /// - 相对路径 → 相对 manifest 所在目录。
    pub fn resolve_entry(&self, manifest_dir: &Path, host_exe_dir: Option<&Path>) -> PathBuf {
        let exe_ext = if cfg!(windows) { ".exe" } else { "" };
        if let Some(name) = self.entry.strip_prefix("bin:") {
            if let Some(dir) = host_exe_dir {
                return dir.join(format!("{name}{exe_ext}"));
            }
            return PathBuf::from(format!("{name}{exe_ext}"));
        }
        let p = Path::new(&self.entry);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            manifest_dir.join(p)
        }
    }
}

/// 字符串字段读取(缺字段 / 非字符串 → 中文 Err)。
fn str_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    match obj.get(key) {
        None => Err(format!("plugin.json 缺少字段「{key}」")),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!("plugin.json 的字段「{key}」必须是字符串")),
    }
}

/// 宽松 semver:`x.y.z`,每段 1–4 位数字(x/y/z 位置)。
fn is_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 4 && p.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{
        "id": "example-stats",
        "name": "统计元素",
        "version": "0.1.0",
        "entry": "bin:example-stats-plugin",
        "commands": ["edit.select_all"],
        "panels": [{"id": "stats", "title": "元素统计"}],
        "exports": [{"id": "png2x", "title": "导出 PNG @2x", "format": "png"}]
    }"#;

    #[test]
    fn parses_good_manifest() {
        let m = PluginManifest::parse(GOOD).expect("合法 manifest 必须通过");
        assert_eq!(m.id, "example-stats");
        assert_eq!(m.commands, vec!["edit.select_all".to_string()]);
        assert_eq!(m.panels.len(), 1);
        assert_eq!(m.panels[0].title, "元素统计");
        assert_eq!(m.exports[0].format, "png");
    }

    #[test]
    fn empty_commands_is_zero_privilege() {
        let m = PluginManifest::parse(
            r#"{"id":"a-b","name":"n","version":"0.1.0","entry":"e","commands":[]}"#,
        )
        .expect("空白名单 = 零权限,合法");
        assert!(m.commands.is_empty());
        assert!(m.panels.is_empty());
    }

    #[test]
    fn missing_required_fields_are_rejected() {
        for (text, field) in [
            (
                r#"{"name":"n","version":"0.1.0","entry":"e","commands":[]}"#,
                "id",
            ),
            (
                r#"{"id":"ab","version":"0.1.0","entry":"e","commands":[]}"#,
                "name",
            ),
            (
                r#"{"id":"ab","name":"n","entry":"e","commands":[]}"#,
                "version",
            ),
            (
                r#"{"id":"ab","name":"n","version":"0.1.0","commands":[]}"#,
                "entry",
            ),
            (
                r#"{"id":"ab","name":"n","version":"0.1.0","entry":"e"}"#,
                "commands",
            ),
        ] {
            let err = PluginManifest::parse(text).unwrap_err();
            assert!(err.contains(field), "缺 {field} 的报错必须点名字段:{err}");
        }
    }

    /// 安全红线:未知字段 = 权限声明面,拼错即拒绝。
    #[test]
    fn unknown_fields_are_rejected() {
        let err = PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"0.1.0","entry":"e","commands":[],"commandz":["*"]}"#,
        )
        .unwrap_err();
        assert!(err.contains("未知字段"), "{err}");
    }

    #[test]
    fn bad_id_version_entry_are_rejected() {
        // 大写 id / 非法字符
        assert!(PluginManifest::parse(
            r#"{"id":"Ab","name":"n","version":"0.1.0","entry":"e","commands":[]}"#
        )
        .unwrap_err()
        .contains("id"));
        // 非三段版本
        assert!(PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"1.0","entry":"e","commands":[]}"#
        )
        .unwrap_err()
        .contains("version"));
        // 空 entry
        assert!(PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"1.0.0","entry":"  ","commands":[]}"#
        )
        .unwrap_err()
        .contains("entry"));
    }

    #[test]
    fn command_without_dot_is_rejected_with_hint() {
        let err = PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"1.0.0","entry":"e","commands":["undo"]}"#,
        )
        .unwrap_err();
        assert!(err.contains("undo"), "报错要含原命令名:{err}");
    }

    #[test]
    fn panel_and_export_validation() {
        // 面板未知字段
        assert!(PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"1.0.0","entry":"e","commands":[],"panels":[{"id":"a","title":"t","onclick":"evil()"}]}"#
        )
        .unwrap_err()
        .contains("onclick"));
        // 面板重复
        assert!(PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"1.0.0","entry":"e","commands":[],"panels":[{"id":"a","title":"t"},{"id":"a","title":"u"}]}"#
        )
        .unwrap_err()
        .contains("重复"));
        // 导出格式不支持
        assert!(PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"1.0.0","entry":"e","commands":[],"exports":[{"id":"x","title":"t","format":"exe"}]}"#
        )
        .unwrap_err()
        .contains("exe"));
        // 非 JSON
        assert!(PluginManifest::parse("not json")
            .unwrap_err()
            .contains("JSON"));
    }

    #[test]
    fn entry_resolution() {
        let m = PluginManifest::parse(GOOD).unwrap();
        // bin: → 宿主 exe 同目录
        let host = Path::new("C:/target/debug");
        let p = m.resolve_entry(Path::new("plugins/example-stats"), Some(host));
        let s = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) {
            assert!(s.ends_with("target/debug/example-stats-plugin.exe"), "{s}");
        } else {
            assert!(s.ends_with("target/debug/example-stats-plugin"), "{s}");
        }
        // 绝对路径原样
        let m2 = PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"1.0.0","entry":"C:/x/p.exe","commands":[]}"#,
        )
        .unwrap();
        assert_eq!(
            m2.resolve_entry(Path::new("d"), None),
            PathBuf::from("C:/x/p.exe")
        );
        // 相对路径 → 相对 manifest 目录
        let m3 = PluginManifest::parse(
            r#"{"id":"ab","name":"n","version":"1.0.0","entry":"run.py","commands":[]}"#,
        )
        .unwrap();
        assert_eq!(
            m3.resolve_entry(Path::new("plugins/x"), None),
            PathBuf::from("plugins/x/run.py")
        );
    }
}
