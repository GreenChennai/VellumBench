//! CLI 取消路径进程级 E2E(硬骨头 #6,R0)。
//!
//! 拉起**真实 kiln-cli 子进程**导出一段长 GIF,4 秒后经 stdin 写入单独
//! 一行 `c`,断言三件事:
//! 1. 退出码 = 130(成功 0 / 失败非 0 / 取消 130 三态);
//! 2. stderr 有「已请求取消」提示与 `{"ok":false,"cancelled":true,…}` 收口;
//! 3. **不留半截产物**:输出文件不存在,工作目录只剩源文件。
//!
//! 依赖系统 Edge/Chrome(与 size_gate_browser 同前提);无浏览器时跳过。

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PAGE: &str = r#"<!DOCTYPE html>
<html lang="zh-CN"><head><meta charset="utf-8">
<style>
  body { margin: 0; background: #ffffff; }
  .box {
    position: absolute; left: 40px; top: 40px;
    width: 120px; height: 80px; border-radius: 8px;
    background: #10b981;
    animation: drift 6s linear infinite alternate;
  }
  @keyframes drift { from { transform: translateX(0); } to { transform: translateX(400px); } }
</style></head>
<body><div class="box"></div></body></html>"#;

/// 看门狗:子进程超过 120s 视为挂死,强杀并 fail(取消必须尽快生效)。
fn spawn_export_and_cancel(args: &[&str]) -> (i32, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kiln-cli"));
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("kiln-cli 子进程启动失败");
    // 4s 后注入取消指令(浏览器冷启动 + 若干帧之后,命中帧边界检查)
    let mut stdin = child.stdin.take().expect("stdin");
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(4));
        let _ = stdin.write_all(b"c\n");
        let _ = stdin.flush();
        // 不主动 close:让进程退出时自然回收,避免竞态提前 EOF
    });
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = child.wait_with_output().expect("收尾读取");
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                return (status.code().unwrap_or(-1), stderr);
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("kiln-cli 120s 未退出:取消未生效(挂死)");
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => panic!("等待子进程失败:{e}"),
        }
    }
}

#[test]
fn cli_cancel_via_stdin_returns_130_without_artifact() {
    // 无浏览器环境直接跳过(动画浏览器车道的前提;与 size_gate 一致)
    if vb_browser::discover_browser(None).is_none() {
        eprintln!("未发现系统浏览器(Edge/Chrome),跳过 CLI 取消 E2E");
        return;
    }
    let tmp = tempfile::tempdir().expect("临时目录");
    let src = tmp.path().join("index.html");
    std::fs::write(&src, PAGE).expect("写源页");
    let out = tmp.path().join("out.gif");

    // fps 5 × 120s = 600 帧,远超取消注入点(4s),导出不可能先行完成
    let (code, stderr) = spawn_export_and_cancel(&[
        "export",
        "--source",
        src.to_str().unwrap(),
        "--output",
        out.to_str().unwrap(),
        "--format",
        "gif",
        "--fps",
        "5",
        "--duration",
        "120",
    ]);

    let stderr_tail: String = {
        let chars: Vec<char> = stderr.chars().collect();
        let start = chars.len().saturating_sub(400);
        chars[start..].iter().collect()
    };
    assert_eq!(
        code, 130,
        "取消退出码必须是 130(实得 {code});stderr 尾部:{stderr_tail}"
    );
    assert!(
        stderr.contains("已请求取消"),
        "stderr 必须有取消提示;尾部:{stderr_tail}"
    );
    assert!(
        stderr.contains("\"cancelled\":true"),
        "stderr 必须有结构化 cancelled 收口;尾部:{stderr_tail}"
    );
    assert!(
        !out.exists(),
        "取消后不得留下半截产物文件:{}",
        out.display()
    );
    let leftovers: Vec<String> = std::fs::read_dir(tmp.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(
        leftovers,
        vec!["index.html".to_string()],
        "工作目录应只剩源文件:{leftovers:?}"
    );
}
