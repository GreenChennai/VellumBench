//! `vb_app` — Vellum Bench 桌面应用(ADR-0015:eframe 宿主 + Vello 画布纹理合成)。

pub mod app;
/// 自动保存与崩溃恢复的文件层(阶段 7 / 07-A·07-B:快照信封、滚动、
/// 损坏回退、LCS 行 diff;GUI 投影在 `app::recover`)。
pub mod autosave;
/// 画布出图通道(03-3 canvas_parity 门禁的画布侧采样;隐藏启动参数)。
pub mod canvas_shot;
/// 能力台账(副文档 09-3:能力/状态/命令 ID 的**单一真相**)。
pub mod capabilities;
/// i18n 骨架(阶段 5 / 05-7:ftl 资源 + t() + 语言切换;资源在仓库根
/// `i18n/*.ftl`,禁用词门禁由 `tools/check_terminology.py` 扫描同一文件)。
pub mod i18n;
/// 用户自定义键位(阶段 5 / 05-4-A2:`keymap.json` 覆盖层,叠加在
/// 静态绑定表之上;编辑器 GUI 在 `app::keymap_dialog`)。
pub mod keymap;
/// 启动主页(阶段 2 / 副文档 02-3:最近项目 + 动作区 + 搜索 + 键盘导航)。
pub mod launcher;
/// 新建项目(阶段 2 / 副文档 02-4:对话框规格、项目生成与模板复制)。
pub mod new_project;
/// 最近项目与最近会话持久化(阶段 2 / 副文档 02-2、02-6:`recent.json`)。
pub mod recent;
/// 外壳(阶段 2 / 副文档 02-1、02-5:启动流程 + 多窗口管理)。
pub mod shell;
pub mod shortcuts;

pub use app::{Tool, VellumApp};

/// 测试专用:串行化 `VB_WORKSPACE` 环境变量的改写(阶段 7 起,多个模块的
/// 门禁测试都要以独立 workspace 文件隔离持久化;并发 `set_var` 与其它
/// 线程的 `get_var` 是数据竞争,曾致 ui_scale 持久化断言偶发红)。
/// 所有"设 env + 触发 workspace 读写"的测试都应持锁运行。
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
