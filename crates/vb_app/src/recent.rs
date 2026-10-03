//! 最近项目(MRU)与最近会话持久化(阶段 2 / 副文档 02-2、02-6)。
//!
//! **R0 数据下沉(22 篇 §4 R0 交付 1/4)**:实现整体迁至
//! [`vb_session::mru`](宿主无关纯数据模块,零 UI 依赖;持久化 JSON 格式
//! 不变,v0.14 旧文件可读,兼容用例见 `vb_session::mru::tests`)。
//!
//! 本模块保留原模块路径 re-export:`vb_app` 内既有调用点(`shell.rs` /
//! `launcher.rs` / `autosave.rs` / `recover.rs` 等)行为零变化;目录解析
//! 单一真相(`config_dir`)同批下沉,`dock_layout::config_dir` 委托至此。
//!
//! 多窗口写竞争的缓解不变 = **单写入者**:只有外壳(`crate::shell::ShellApp`)
//! 持有内存态并落盘;项目窗口的"保存/打开"经外壳转发,不直接写。

pub use vb_session::mru::*;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// 委托链路冒烟:经 `vb_app::recent` 原路径访问的类型/函数与
    /// `vb_session::mru` 同源(re-export 不断线)。
    #[test]
    fn re_export_wiring_is_the_same_source() {
        assert_eq!(SCHEMA_VERSION, vb_session::mru::SCHEMA_VERSION);
        assert_eq!(MAX_ITEMS, vb_session::mru::MAX_ITEMS);
        let mut st = RecentStore::default();
        st.touch(Path::new("examples/landing"));
        assert_eq!(st.items.len(), 1);
        assert_eq!(st.items[0].name, "landing");
        assert_eq!(
            path_key(Path::new("examples/landing")),
            st.items[0].path.to_lowercase()
        );
        assert_eq!(
            relative_time(1_800_000_000, 1_800_000_030),
            vb_session::mru::relative_time(1_800_000_000, 1_800_000_030)
        );
    }
}
