//! 入口:窗口 + wgpu 后端(设计文档 05 篇 §十:Vulkan 优先,自动回退)。

use vb_app::VellumApp;

fn main() -> eframe::Result<()> {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let project = args.get(1).map(std::path::PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1680.0, 1000.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_icon(load_icon()),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Vellum Bench · 绘台",
        options,
        Box::new(move |cc| Ok(Box::new(VellumApp::new(cc, project)))),
    )
}

fn load_icon() -> egui::IconData {
    // 16x16 简单橙色圆点占位图标
    let (w, h) = (16u32, 16u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - 7.5;
            let dy = y as f32 - 7.5;
            let inside = dx * dx + dy * dy <= 36.0;
            let (r, g, b, a) = if inside {
                (255, 90, 31, 255)
            } else {
                (0, 0, 0, 0)
            };
            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }
    egui::IconData {
        width: w,
        height: h,
        rgba,
    }
}
