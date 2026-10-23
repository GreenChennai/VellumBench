//! 安全解析项目内相对资源路径,供布局探测与渲染加载共用。

use std::path::{Component, Path, PathBuf};

const MAX_SOURCE_PATH_BYTES: usize = 8 * 1024;

/// 将 HTML/CSS 项目资源路径解析为项目根目录内的现存普通文件。
///
/// 支持项目根相对路径、URL 百分号转义和 query/fragment 后缀。远程/协议路径、
/// Windows 驱动器与 UNC、NUL、父目录穿越、越界 symlink、目录和过长路径均拒绝。
pub fn resolve_project_asset(project_root: &Path, src: &str) -> Option<PathBuf> {
    let src = src.trim();
    if src.is_empty()
        || src.len() > MAX_SOURCE_PATH_BYTES
        || src.starts_with("//")
        || src.starts_with("\\\\")
        || src.contains("://")
    {
        return None;
    }

    let path_part = src.split(['?', '#']).next()?;
    let decoded = percent_decode(path_part)?.replace('\\', "/");
    if decoded.is_empty() || decoded.contains('\0') || decoded.contains(':') {
        return None;
    }

    // Web-root-relative assets are relative to this project, never the host filesystem.
    let relative = decoded.trim_start_matches('/');
    if relative.is_empty() {
        return None;
    }
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }

    let root = project_root.canonicalize().ok()?;
    let resolved = root.join(relative_path).canonicalize().ok()?;
    (resolved.starts_with(&root) && resolved.is_file()).then_some(resolved)
}

fn percent_decode(src: &str) -> Option<String> {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let bytes = src.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

#[cfg(test)]
mod tests {
    use super::resolve_project_asset;
    use std::path::PathBuf;

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new() -> Self {
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "vellumbench-project-assets-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(root.join("project/assets"))
                .expect("create temporary project root");
            std::fs::write(root.join("project/assets/中.png"), b"image")
                .expect("write fixture asset");
            std::fs::write(root.join("outside.png"), b"outside").expect("write external fixture");
            Self(root)
        }

        fn project(&self) -> PathBuf {
            self.0.join("project")
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolves_project_relative_root_relative_and_url_encoded_paths() {
        let temp = TempRoot::new();
        let project = temp.project();
        let expected = project.join("assets/中.png").canonicalize().unwrap();
        assert_eq!(
            resolve_project_asset(&project, "assets/%E4%B8%AD.png?rev=2"),
            Some(expected.clone())
        );
        assert_eq!(
            resolve_project_asset(&project, "/assets/中.png#preview"),
            Some(expected)
        );
    }

    #[test]
    fn rejects_remote_absolute_and_traversal_paths() {
        let temp = TempRoot::new();
        let project = temp.project();
        let outside = temp.0.join("outside.png");
        for src in [
            "https://example.invalid/image.png",
            "//example.invalid/image.png",
            "\\\\server\\share\\image.png",
            "../outside.png",
            "%2e%2e/outside.png",
            "data:image/png;base64,AA==",
            "C:/outside.png",
        ] {
            assert!(
                resolve_project_asset(&project, src).is_none(),
                "unexpectedly accepted {src}"
            );
        }
        assert!(resolve_project_asset(&project, outside.to_str().unwrap()).is_none());
        assert!(resolve_project_asset(&project, "assets/%00.png").is_none());
        assert!(
            resolve_project_asset(&project, &"a".repeat(super::MAX_SOURCE_PATH_BYTES + 1))
                .is_none()
        );
    }
}
