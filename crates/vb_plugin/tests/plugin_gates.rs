//! 05-10-7 门禁测试(09-J):进程级端到端,全部打真实示例插件子进程。
//!
//! ① 示例插件 install→enable→run→面板可见;
//! ② 越权调用被拒 + 记录;
//! ③ 插件崩溃宿主存活(转 Crashed → 可重启);
//! ④ manifest schema 校验(单测部分在 manifest/host 模块内);
//! ⑤ JSON-RPC 编解码与超时(编解码单测在 protocol 模块内;此处为
//!    进程级握手超时)。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use vb_plugin::host::{HostServices, PluginHost, PluginState};
use vb_plugin::projection;

/// 示例插件可执行(cargo 为集成测试注入的绝对路径)。
fn plugin_exe() -> PathBuf {
    let p = env!("CARGO_BIN_EXE_example-stats-plugin");
    PathBuf::from(p)
}

/// 造一个临时 manifest 目录,entry 指向真实示例插件可执行
/// (夹具参数经 `start_with_args` 传,manifest.entry 只放纯路径)。
fn make_plugin_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vb-plugin-gate-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let entry = plugin_exe().display().to_string();
    let manifest = json!({
        "id": "example-stats",
        "name": "统计元素(示例)",
        "version": "0.1.0",
        "entry": entry,
        "commands": ["edit.select_all"],
        "panels": [{"id": "stats", "title": "元素统计"}],
        "exports": [{"id": "png2x", "title": "导出 PNG @2x", "format": "png"}],
    });
    std::fs::write(
        dir.join("plugin.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    dir
}

/// 记账型 mock 宿主服务(授权命令放行并记账;文档投影返回固定结构)。
struct MockServices {
    executed: Arc<Mutex<Vec<String>>>,
    has_doc: bool,
}

impl MockServices {
    fn new(has_doc: bool) -> (Self, Arc<Mutex<Vec<String>>>) {
        let executed = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                executed: executed.clone(),
                has_doc,
            },
            executed,
        )
    }
}

impl HostServices for MockServices {
    fn run_command(&mut self, plugin: &str, command: &str) -> Result<Value, String> {
        self.executed
            .lock()
            .unwrap()
            .push(format!("{plugin}/{command}"));
        Ok(json!({"ok": true, "command": command, "via": plugin}))
    }

    fn doc_projection(&self) -> Result<Value, String> {
        if !self.has_doc {
            return Err("无打开文档".into());
        }
        // 与 vb_doc 真投影同构的迷你夹具(计数由 projection 模块单测保证)
        Ok(json!({
            "rev": 3,
            "artboardCount": 1,
            "counts": {"nodes": 5, "text": 2, "image": 1, "group": 1, "vector": 0, "other": 0},
            "artboards": [],
        }))
    }

    fn run_export(
        &mut self,
        plugin: &str,
        export_id: &str,
        format: &str,
        dir: &Path,
    ) -> Result<Value, String> {
        Ok(json!({
            "ok": true, "plugin": plugin, "export": export_id, "format": format,
            "dir": dir.display().to_string(),
        }))
    }
}

/// 轮询到目标状态(带超时;poll 驱动状态机与入站处理)。
fn wait_state(
    host: &mut PluginHost,
    services: &mut dyn HostServices,
    id: &str,
    want: PluginState,
    timeout: Duration,
) -> PluginState {
    let deadline = Instant::now() + timeout;
    loop {
        host.poll(services);
        let st = host
            .list()
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.state)
            .unwrap_or(PluginState::Crashed);
        if st == want || Instant::now() >= deadline {
            return st;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// ① 示例插件 install→enable→run→面板可见(冒烟全链)。
#[test]
fn gate1_install_enable_run_panel_visible() {
    let dir = make_plugin_dir("g1");
    let mut host = PluginHost::new(None); // None = 不持久化(测试隔离)
    let id = host
        .install_dir(&dir, &|c| c == "edit.select_all")
        .expect("安装必须成功");
    assert_eq!(id, "example-stats");
    // 未授权时启动必须被拒
    assert!(host.start(&id).is_err(), "未授权不得启动");
    assert_eq!(host.list()[0].state, PluginState::Unauthorized);
    // 授权(= 授权弹窗的「启用」)→ 启动 → 握手 → Running
    host.authorize(&id).unwrap();
    host.start(&id).expect("授权后启动成功");
    let (mut services, executed) = MockServices::new(true);
    let st = wait_state(
        &mut host,
        &mut services,
        &id,
        PluginState::Running,
        Duration::from_secs(10),
    );
    assert_eq!(st, PluginState::Running, "握手必须完成:{:?}", host.list());
    // event/started → 插件拉投影 → panel/setUI → 面板可见(轮询到出现)
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut widgets_found = false;
    while Instant::now() < deadline {
        host.poll(&mut services);
        if let Some(ui) = host.panel_ui(&id, "stats") {
            let has_metric = ui.widgets.iter().any(|w| {
                matches!(w, vb_plugin::host::Widget::Metric { label, value }
                    if label == "节点总数" && value == "5")
            });
            let has_text_metric = ui.widgets.iter().any(|w| {
                matches!(w, vb_plugin::host::Widget::Metric { label, value }
                    if label == "文本数" && value == "2")
            });
            let has_image_metric = ui.widgets.iter().any(|w| {
                matches!(w, vb_plugin::host::Widget::Metric { label, value }
                    if label == "图片数" && value == "1")
            });
            if has_metric && has_text_metric && has_image_metric {
                widgets_found = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(widgets_found, "面板必须出现统计读数(节点/文本/图片)");
    // 面板按钮点击 → 插件回调白名单内命令 edit.select_all → mock 记账
    host.send_button(&id, "stats", "select_all").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if !executed.lock().unwrap().is_empty() {
            break;
        }
        host.poll(&mut services);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        executed.lock().unwrap().as_slice(),
        ["example-stats/edit.select_all"],
        "面板按钮必须经 runCommand 落到宿主命令路径"
    );
    // 导出动作:目录必须来自用户选择(None → 拒绝)
    assert!(host.run_export(&mut services, &id, "png2x", None).is_err());
    let out_dir = std::env::temp_dir().join(format!("vb-plugin-gate-out-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&out_dir);
    let r = host
        .run_export(&mut services, &id, "png2x", Some(&out_dir))
        .unwrap();
    assert_eq!(r["dir"], out_dir.display().to_string());
    // 收尾:停止 → Stopped
    host.stop(&id);
    assert_eq!(host.list()[0].state, PluginState::Stopped);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&out_dir);
}

/// ② 越权调用被拒 + 记录(--probe 编辑 undo 之外的白名单外命令)。
#[test]
fn gate2_unauthorized_call_denied_and_logged() {
    let dir = make_plugin_dir("g2");
    let mut host = PluginHost::new(None).with_handshake_timeout(Duration::from_secs(8));
    let id = host.install_dir(&dir, &|c| c == "edit.select_all").unwrap();
    host.authorize(&id).unwrap();
    host.start_with_args(&id, &["--probe".into(), "edit.undo".into()])
        .unwrap();
    let (mut services, executed) = MockServices::new(true);
    // 插件握手成功 → Running → 发 probe(edit.undo 不在白名单)→ 宿主
    // 拒绝(-32001)→ 插件收到错误应答后退出。Running → Crashed 同样
    // 可能落在两次 poll 之间,断言落在**日志环**(越权记录)与 mock
    // 记账(命令没执行)这两个非瞬态事实上。
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut denied_logged = false;
    while Instant::now() < deadline {
        host.poll(&mut services);
        if host
            .logs(&id)
            .iter()
            .any(|l| l.text.contains("越权调用被拒") && l.text.contains("edit.undo"))
        {
            denied_logged = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(denied_logged, "越权必须记录进日志环:{:?}", host.logs(&id));
    // 白名单外命令**没有**被执行(mock 记账为空)
    assert!(
        executed.lock().unwrap().is_empty(),
        "越权命令不得执行:{:?}",
        executed.lock().unwrap()
    );
    // 插件收到拒绝应答后自行退出 → 宿主检出(状态落 Crashed,宿主无恙)
    let st = wait_state(
        &mut host,
        &mut services,
        &id,
        PluginState::Crashed,
        Duration::from_secs(10),
    );
    assert_eq!(st, PluginState::Crashed, "probe 完成后插件退出,宿主应检出");
    let _ = std::fs::remove_dir_all(&dir);
}

/// ③ 插件崩溃宿主存活(--crash 自杀 → Crashed → 重启回 Running)。
#[test]
fn gate3_crash_isolates_host_and_restart_works() {
    let dir = make_plugin_dir("g3");
    let mut host = PluginHost::new(None).with_handshake_timeout(Duration::from_secs(8));
    let id = host.install_dir(&dir, &|c| c == "edit.select_all").unwrap();
    host.authorize(&id).unwrap();
    host.start_with_args(&id, &["--crash".into()]).unwrap();
    let (mut services, _executed) = MockServices::new(true);
    // 插件先到 Running(握手完成),随后自杀 → poll 检出 → Crashed。
    // Running → Crashed 可能落在两次 poll 之间(不外显),所以 Running
    // 的证据取日志环的「已就绪(Running)」(握手线程只在该态写它)。
    let st = wait_state(
        &mut host,
        &mut services,
        &id,
        PluginState::Crashed,
        Duration::from_secs(10),
    );
    assert_eq!(st, PluginState::Crashed, "进程自杀后宿主必须检出崩溃");
    let logs: Vec<String> = host.logs(&id).iter().map(|l| l.text.clone()).collect();
    assert!(
        logs.iter().any(|t| t.contains("已就绪")),
        "崩溃前必须先到过 Running(日志环为证):{logs:?}"
    );
    assert!(
        host.logs(&id)
            .iter()
            .any(|l| l.text.contains("插件进程已退出")),
        "崩溃必须落日志:{:?}",
        host.logs(&id)
    );
    // 宿主存活:其他 API 照常工作
    assert_eq!(host.list().len(), 1);
    // 重启(无夹具参数)→ 回 Running
    host.restart(&id).expect("重启成功");
    let st = wait_state(
        &mut host,
        &mut services,
        &id,
        PluginState::Running,
        Duration::from_secs(10),
    );
    assert_eq!(st, PluginState::Running, "重启必须回到 Running");
    host.stop(&id);
    let _ = std::fs::remove_dir_all(&dir);
}

/// ⑤(进程级)JSON-RPC 超时:插件拒绝握手 → 超时 → Crashed,宿主存活。
#[test]
fn gate5_handshake_timeout_kills_and_marks_crashed() {
    let dir = make_plugin_dir("g5");
    let mut host = PluginHost::new(None).with_handshake_timeout(Duration::from_millis(400));
    let id = host.install_dir(&dir, &|c| c == "edit.select_all").unwrap();
    host.authorize(&id).unwrap();
    host.start_with_args(&id, &["--no-handshake".into()])
        .unwrap();
    let (mut services, _executed) = MockServices::new(true);
    let st = wait_state(
        &mut host,
        &mut services,
        &id,
        PluginState::Crashed,
        Duration::from_secs(6),
    );
    assert_eq!(st, PluginState::Crashed, "握手超时必须转 Crashed");
    assert!(
        host.logs(&id).iter().any(|l| l.text.contains("超时")),
        "超时必须落日志:{:?}",
        host.logs(&id)
    );
    // 宿主自身完全可用(崩溃隔离的另一半:超时路径也不拖垮宿主)
    assert_eq!(host.list().len(), 1);
    host.stop(&id);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 只读投影的结构稳定性(05-10-4 ②:投影是插件 ABI 的一部分)。
#[test]
fn projection_shape_is_stable() {
    let doc = vb_doc::model::Document::new_default();
    let p = projection::build(&doc);
    for key in ["rev", "artboardCount", "counts", "artboards"] {
        assert!(p.get(key).is_some(), "投影缺字段 {key}");
    }
    for key in ["nodes", "text", "image", "group", "vector", "other"] {
        assert!(p["counts"].get(key).is_some(), "counts 缺 {key}");
    }
}
