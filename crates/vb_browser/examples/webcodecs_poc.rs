//! WebCodecs 全 GPU 渲染路径 PoC:在真实 SEEK 页面上实测
//! `SEEK → VideoFrame(canvas) → VideoEncoder` 的单帧成本与硬编可用性。
//! 用法:cargo run -p vb_browser --example webcodecs_poc <url> [帧数]
//! 环境变量:VB_GPU=1 开 GPU 光栅(对比软件光栅)。

use std::time::{Duration, Instant};

fn main() {
    let t0 = Instant::now();
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(240));
        eprintln!("[watchdog] 240s 强制退出");
        std::process::exit(9);
    });
    fn step(t0: &Instant, msg: String) {
        println!("[{:>7}ms] {msg}", t0.elapsed().as_millis());
    }
    let url = std::env::args()
        .nth(1)
        .expect("用法: webcodecs_poc <url|目录> [帧数]");
    let frames: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let gpu = std::env::var("VB_GPU").map(|v| v == "1").unwrap_or(false);

    let path = std::path::Path::new(&url);
    let (mount_dir, html) = if path.is_dir() {
        (path.to_path_buf(), None)
    } else {
        (path.parent().unwrap().to_path_buf(), Some(path.clone()))
    };
    let srv = vb_browser::staticsrv::StaticServer::start(&mount_dir).expect("静态服务失败");
    let page_url = match &html {
        Some(h) => format!(
            "http://127.0.0.1:{}/{}",
            srv.port(),
            h.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("index.html")
        ),
        None => srv.url_for_dir().expect("无入口"),
    };
    step(&t0, format!("目标: {page_url} (GPU={gpu})"));

    let exe = vb_browser::discover_browser(None).expect("无浏览器");
    let proc = vb_browser::browser::BrowserProcess::launch_with(
        &exe,
        vb_browser::browser::LaunchOptions { gpu },
    )
    .expect("启动失败");
    let mut page = vb_browser::page::PageSession::attach(&proc).expect("attach 失败");
    step(&t0, format!("浏览器 {}", proc.version()));
    page.set_device_metrics(1920, 1080, 1)
        .expect("metrics 失败");
    page.navigate(&page_url).expect("导航失败");
    page.wait_network_idle(Duration::from_secs(5));
    page.sleep(500);

    let script = format!(
        r#"(async () => {{
  const canvas = document.querySelector('canvas');
  if (!canvas) return JSON.stringify({{err: 'no-canvas'}});
  if (typeof VideoEncoder === 'undefined') return JSON.stringify({{err: 'no-webcodecs'}});
  if (typeof window.SEEK !== 'function') return JSON.stringify({{err: 'no-SEEK'}});
  const cfg = {{
    codec: 'avc1.42002A',
    width: canvas.width,
    height: canvas.height,
    bitrate: 8_000_000,
    framerate: 30,
    hardwareAcceleration: 'prefer-hardware',
  }};
  let sup;
  try {{ sup = await VideoEncoder.isConfigSupported(cfg); }}
  catch (e) {{ return JSON.stringify({{err: 'cfg: ' + e.message}}); }}
  if (!sup.supported) return JSON.stringify({{err: 'unsupported'}});
  let bytes = 0, chunks = 0, keyframes = 0;
  const enc = new VideoEncoder({{
    output: (c, m) => {{ bytes += c.byteLength; chunks++; if (m && m.decoderConfig) keyframes++; }},
    error: (e) => {{ throw e; }},
  }});
  enc.configure(cfg);
  const N = {frames};
  let drawMs = 0, frameMs = 0, waitMs = 0;
  const t0 = performance.now();
  for (let i = 0; i < N; i++) {{
    const a = performance.now();
    window.SEEK(i / 30);
    const frame = new VideoFrame(canvas, {{
      timestamp: Math.round(i * 1e6 / 30),
      duration: Math.round(1e6 / 30),
    }});
    const b = performance.now();
    enc.encode(frame, {{ keyFrame: i % 150 === 0 }});
    frame.close();
    const c = performance.now();
    drawMs += b - a;
    frameMs += c - b;
    while (enc.encodeQueueSize > 8) {{
      await new Promise((r) => setTimeout(r, 0));
      waitMs += 1;
    }}
  }}
  await enc.flush();
  enc.close();
  const total = performance.now() - t0;
  return JSON.stringify({{
    fps: +(N / (total / 1000)).toFixed(1),
    per_frame_ms: +(total / N).toFixed(1),
    draw_per_frame_ms: +(drawMs / N).toFixed(1),
    videoframe_ms: +(frameMs / N).toFixed(1),
    encode_bytes: bytes,
    chunks,
    keyframes,
    hw: sup.config && sup.config.hardwareAcceleration || 'default',
    canvas: canvas.width + 'x' + canvas.height,
  }});
}})()"#
    );

    match page.evaluate(&script, true) {
        Ok(v) => step(&t0, format!("PoC 结果: {v}")),
        Err(e) => step(&t0, format!("PoC 失败: {e}")),
    }
    step(&t0, format!("总耗时(GPU={gpu})见上"));
}
