//! 分步探针:定位浏览器车道挂点(临时调试用)。
//! 用法:cargo run -p vb_browser --example probe [url]
//! 无参 = 内置 data: URL 自检。

use std::time::Duration;
use std::time::Instant;

fn main() {
    let t0 = Instant::now();
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(90));
        eprintln!("[watchdog] 90s 强制退出");
        std::process::exit(9);
    });
    fn step(t0: &Instant, msg: String) {
        println!("[{:>6}ms] {msg}", t0.elapsed().as_millis());
    }
    let url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "data:text/html,<h1>hi</h1>".into());
    step(&t0, format!("目标: {url}"));
    let exe = vb_browser::discover_browser(None).expect("无浏览器");
    let proc = vb_browser::browser::BrowserProcess::launch(&exe).expect("启动失败");
    step(&t0, format!("启动成功, DevTools 端口 {}", proc.port));
    let mut page = vb_browser::page::PageSession::attach(&proc).expect("attach 失败");
    step(&t0, format!("浏览器版本 {}", proc.version()));
    page.set_device_metrics(1240, 1240, 1).expect("metrics 失败");
    step(&t0, "视口已设".into());

    page.emulate_static().expect("emulate 失败");
    let rm = page.evaluate("(() => matchMedia('(prefers-reduced-motion: reduce)').matches)()", false);
    step(&t0, format!("reduced-motion 生效: {:?}", rm));
    match page.navigate(&url) {
        Ok(()) => step(&t0, "load 事件到达".into()),
        Err(e) => {
            step(&t0, format!("导航失败: {e}"));
            return;
        }
    }
    step(&t0, "开始 networkidle 等待".into());
    page.wait_network_idle(Duration::from_secs(5));
    step(&t0, "networkidle 返回".into());
    page.sleep(300);
    step(&t0, "300ms 落定完成".into());
    if let Err(e) = page.ensure_alive() {
        step(&t0, format!("加载阶段渲染器崩溃: {e}"));
        return;
    }
    step(&t0, "networkidle+存活检查通过".into());
    match page.content_size() {
        Ok((w, h)) => step(&t0, format!("内容 {w}x{h}")),
        Err(e) => step(&t0, format!("content_size 失败: {e}")),
    }
    // 整页截图(beyond + clip 全内容)
    let (w, h) = page.content_size().unwrap_or((1240, 1754));
    match page.screenshot("png", None, Some((0.0, 0.0, w as f64, h as f64)), true, false) {
        Ok(bytes) => step(&t0, format!("整页截图 {} 字节", bytes.len())),
        Err(e) => step(&t0, format!("整页截图失败: {e}")),
    }
    if let Err(e) = page.ensure_alive() {
        step(&t0, format!("截图后渲染器崩溃: {e}"));
        return;
    }
    // 动画状态诊断
    let anims = page.evaluate(
        "(() => { const a = document.getAnimations(); return { total: a.length, running: a.filter(x => x.playState === 'running').length, infinite: a.filter(x => { try { return x.effect.getTiming().iterations === Infinity } catch(e) { return false } }).length } })()",
        false,
    );
    step(&t0, format!("动画状态: {:?}", anims));
    let h1 = page.content_size().unwrap_or((0, 0));
    let _ = page.evaluate("(() => { for (const a of document.getAnimations()) { try { a.finish() } catch(e) {} } })()", false);
    page.sleep(500);
    let h2 = page.content_size().unwrap_or((0, 0));
    step(&t0, format!("finish 前高 {:?} / finish 后高 {:?}", h1, h2));
    step(&t0, "全部通过".into());
}
