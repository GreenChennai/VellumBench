use std::time::Duration;
fn main() {
    let url = r"D:\视频工程\DeepSeek4.1F-world.execute(me);\04_影片工程\src";
    let path = std::path::Path::new(url);
    let srv = vb_browser::staticsrv::StaticServer::start(path.parent().unwrap()).unwrap();
    let name = path.file_name().and_then(|n| n.to_str()).unwrap();
    let page_url = format!("http://127.0.0.1:{}/{}", srv.port(), name);
    let exe = vb_browser::discover_browser(None).unwrap();
    let proc = vb_browser::browser::BrowserProcess::launch_with(
        &exe,
        vb_browser::browser::LaunchOptions { gpu: true },
    )
    .unwrap();
    let mut page = vb_browser::page::PageSession::attach(&proc).unwrap();
    page.set_device_metrics(1920, 1080, 1).unwrap();
    page.navigate(&page_url).unwrap();
    page.wait_network_idle(Duration::from_secs(5));
    page.sleep(800);
    let js = r#"(() => {
      const cs = [...document.querySelectorAll('canvas')];
      const info = cs.map((c,i) => ({i, w: c.width, h: c.height, id: c.id, cls: String(c.className).slice(0,30)}));
      return JSON.stringify({count: cs.length, info, hasSeek: typeof window.SEEK});
    })()"#;
    let v = page.evaluate(js, false).unwrap();
    println!("诊断: {v}");
}
