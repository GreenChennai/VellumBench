//! 阶段 6「布局持久化」验收门(副文档 07-2;判据 3)。
//!
//! 走**真实路径解析**(`VB_WORKSPACE` 环境变量)而不是直接打文件名,
//! 因此覆盖了「配置路径怎么找 → 写 → 读回」这一整条链 —— 也就是
//! 「改停靠位后重启还原」的机械等价物(重启 = 重新 `load()`)。
//!
//! 单测放在独立测试二进制里:`VB_WORKSPACE` 是进程级环境变量,
//! 独立进程可避免与其它测试并行时的相互干扰。

use vb_app::app::dock_layout::{self, DockSide, WorkspaceConfig};

#[test]
fn workspace_roundtrips_through_env_configured_path() {
    let file =
        std::env::temp_dir().join(format!("vb-ws-env-{}-{}.json", std::process::id(), line!()));
    let _ = std::fs::remove_file(&file);
    // SAFETY:本文件只有这一个测试,且在本进程内独占该环境变量。
    unsafe { std::env::set_var("VB_WORKSPACE", &file) };

    assert_eq!(dock_layout::config_path().as_deref(), Some(file.as_path()));

    // 首次运行:无文件 → 默认布局且**不告警**
    let (cfg, warn) = dock_layout::load();
    assert_eq!(cfg, WorkspaceConfig::default());
    assert!(warn.is_none(), "首次运行不该告警:{warn:?}");
    assert_eq!(cfg.toolbar_dock, DockSide::Bottom, "默认停靠 = 底部");

    // 改停靠 + 列数 + 面板坞状态 → 落盘
    let mut changed = cfg.clone();
    changed.toolbar_dock = DockSide::Left;
    changed.toolbar_columns = 2;
    changed.panel_order = vec![3, 1, 0, 2];
    changed.panel_tab = 3;
    changed.dock_collapsed = true;
    dock_layout::save(&changed).expect("写工作区配置");

    // 「重启」= 重新 load
    let (back, warn) = dock_layout::load();
    assert!(warn.is_none());
    assert_eq!(back, changed, "重启后必须逐字段还原");

    // 再次保存(幂等,不产生 .tmp 残留)
    dock_layout::save(&back).unwrap();
    assert!(
        !file.with_extension("json.tmp").exists(),
        "临时文件应已改名"
    );
    let (again, _) = dock_layout::load();
    assert_eq!(again, back);

    let _ = std::fs::remove_file(&file);
    unsafe { std::env::remove_var("VB_WORKSPACE") };
}

/// 损坏文件 → 回退默认并**告警**(07-2-2;判据 3 后半)。
#[test]
fn corrupt_workspace_file_falls_back_and_warns() {
    let file = std::env::temp_dir().join(format!(
        "vb-ws-corrupt-{}-{}.json",
        std::process::id(),
        line!()
    ));
    std::fs::write(&file, "{ this is not json").unwrap();

    let (cfg, warn) = dock_layout::load_from(&file);
    assert_eq!(cfg.toolbar_dock, DockSide::Bottom);
    assert_eq!(cfg.toolbar_columns, 1);
    let msg = warn.expect("损坏必须告警");
    assert!(msg.contains("回退默认"), "告警文案应说明已回退:{msg}");

    let _ = std::fs::remove_file(&file);
}
