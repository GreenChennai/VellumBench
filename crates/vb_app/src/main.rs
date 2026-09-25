//! 入口:窗口 + wgpu 后端(设计文档 05 篇 §十:Vulkan 优先,自动回退)。
//!
//! 阶段 2(副文档 02-1):启动流程改造 —— 外壳(shell::ShellApp)统一管理
//! 主页与多窗口;`--project <dir>` / 位置参数直达项目窗口,无参数开主页,
//! `--help` / `--version` 保留(脚本/CI 用法不破坏)。

fn main() -> eframe::Result<()> {
    env_logger::init();
    let argv: Vec<String> = std::env::args().collect();
    match vb_app::shell::parse_launch(&argv) {
        vb_app::shell::Launch::Help(text) | vb_app::shell::Launch::Version(text) => {
            print!("{text}");
            Ok(())
        }
        vb_app::shell::Launch::Error(text) => {
            eprintln!("{text}");
            std::process::exit(2);
        }
        launch => vb_app::shell::run_native(launch),
    }
}
