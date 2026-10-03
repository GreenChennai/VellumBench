//! `canvas-spike` — 画布上屏通道三路对比 spike(22 篇 §4 R0 交付 3,ADR-0047)。
//!
//! 四条可切换通道(运行期键位 1/2/3/4,启动期 `--route`):
//! - **(a)** `1` — vb_render `encode_scene` → vello GPU `render_to_texture`
//!   → GPU→CPU 读回 → `RenderImage` → `paint_image`(离屏纹理路线的现实
//!   形态:gpui 0.2.2 无外部纹理导入,零拷贝不可行);
//! - **(b)** `2` — DrawList → sable-paint `VelloSink`(PaintSink 抽象)→
//!   同上读回上屏(sable 抽象 + GPU 后端);
//! - **(b2)** `3` — 同一翻译 → sable-paint `CpuRenderer`(vello_cpu 软
//!   光栅,= sable-canvas `gpui_element` 现状上屏路径);
//! - **(c)** `4` — DrawList → gpui 原生 paint(quad/path/text),不经 vello。
//!
//! 判据(22 篇):1080p 下 1 万节点 60fps;缩放无糊(0.25x/1x/4x,键位
//! `-` / `0` / `=`);帧时间 overlay(翻译/编码、上屏管线、整帧 EMA)。
//!
//! 自动化:`--frames N` 跑满 N 帧自动退出,`--csv f.csv` 逐帧记录
//! (route,nodes,zoom,encode_ms,present_ms,total_ms_ema),供 ADR 取数。
//!
//! 注:spike 是开发期工具,不接 dock/gpui-component 主题,overlay 用 gpui
//! 原生色(dev-stats 风格);窗口标题实时标注当前通道。

mod gpu_frame;
mod route_gpui;
mod route_sable;
mod scene;

use std::io::Write as _;
use std::sync::Arc;
use std::time::Instant;

use sable::gpui::{
    canvas, div, px, rgb, size, white, App, AppContext as _, Application, Bounds, Corners,
    DispatchPhase, IntoElement, KeyDownEvent, ParentElement as _, Pixels, Render, RenderImage,
    Rgba, Styled as _, Window, WindowBounds, WindowOptions,
};
use vb_render::DrawList;

use crate::gpu_frame::GpuFrame;
use crate::route_gpui::Prim;

/// 上屏通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Route {
    /// (a) vello GPU 离屏 + 读回。
    A,
    /// (b) sable-paint VelloSink(GPU)+ 读回。
    B,
    /// (b2) sable-paint CpuRenderer(vello_cpu,= sable-canvas 现状)。
    B2,
    /// (c) gpui 原生翻译层。
    C,
}

impl Route {
    fn label(self) -> &'static str {
        match self {
            Route::A => "a-vello离屏+读回",
            Route::B => "b-sableVelloSink(GPU)+读回",
            Route::B2 => "b2-sableCpuRenderer(vello_cpu现状)",
            Route::C => "c-gpui原生(quad/path/text)",
        }
    }
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "1" => Some(Route::A),
            "2" => Some(Route::B),
            "3" => Some(Route::B2),
            "4" => Some(Route::C),
            _ => None,
        }
    }
}

/// 命令行参数(手写解析,不引 clap)。
struct Args {
    route: Route,
    nodes: usize,
    frames: u64, // 0 = 无限(关窗退出)
    zoom: f64,
    csv: Option<String>,
    /// 非窗口模式:渲一帧直接落 PNG(通道 parity 证据用;路线 c 不支持)。
    dump: Option<String>,
}

impl Args {
    fn parse() -> Self {
        let mut args = Args {
            route: Route::B2,
            nodes: 10_000,
            frames: 0,
            zoom: 1.0,
            csv: None,
            dump: None,
        };
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            match a.as_str() {
                "--route" => {
                    let v = it.next().unwrap_or_else(|| panic!("--route 缺值"));
                    args.route = match v.as_str() {
                        "a" => Route::A,
                        "b" => Route::B,
                        "b2" => Route::B2,
                        "c" => Route::C,
                        other => panic!("未知路线 {other}(可选 a/b/b2/c)"),
                    };
                }
                "--nodes" => {
                    let v = it.next().unwrap_or_else(|| panic!("--nodes 缺值"));
                    args.nodes = v.parse().expect("--nodes 非整数");
                }
                "--frames" => {
                    let v = it.next().unwrap_or_else(|| panic!("--frames 缺值"));
                    args.frames = v.parse().expect("--frames 非整数");
                }
                "--zoom" => {
                    let v = it.next().unwrap_or_else(|| panic!("--zoom 缺值"));
                    args.zoom = v.parse().expect("--zoom 非数字");
                }
                "--csv" => args.csv = Some(it.next().unwrap_or_else(|| panic!("--csv 缺值"))),
                "--dump" => args.dump = Some(it.next().unwrap_or_else(|| panic!("--dump 缺值"))),
                other => panic!("未知参数 {other}(--route/--nodes/--frames/--zoom/--csv)"),
            }
        }
        args
    }
}

/// 一帧的可提交产物(Arc 化:render 期构建,paint 期只消费)。
enum FrameData {
    Image(Arc<RenderImage>),
    Prims(Arc<Vec<Prim>>),
}

struct CanvasSpikeApp {
    route: Route,
    zoom: f64,
    list: DrawList,
    gpu: Option<GpuFrame>,
    frame: Option<Arc<FrameData>>,
    /// 整帧周期 EMA(帧起点 → 下一次帧起点,含全部工作 + 调度间隙;
    /// FPS 由此推出)。
    frame_delta_ms_ema: f64,
    encode_ms: f64,
    present_ms: f64,
    last_frame: Option<Instant>,
    frames_done: u64,
    frames_target: u64,
    text_cache_hits: u64,
    text_cache_misses: u64,
    csv: Option<std::fs::File>,
    samples: Vec<f64>,
    /// 稳态(排除首帧 warmup)累计。
    steady_encode_ms: f64,
    steady_present_ms: f64,
    steady_frames: u64,
    /// 首帧 warmup 耗时(字体发现/shader 编译等一次性成本)。
    warmup_encode_ms: f64,
    warmup_present_ms: f64,
    finished: bool,
}

/// 设备像素尺寸(逻辑 × scale_factor,夹到 [1, 8192];8192 = 观测防御上限)。
fn device_size(vw: f64, vh: f64, scale: f32) -> (u32, u32) {
    let clamp = |v: f64| (v * f64::from(scale)).round().clamp(1.0, 8192.0) as u32;
    (clamp(vw), clamp(vh))
}

fn to_render_image(rgba: Vec<u8>, w: u32, h: u32) -> Arc<RenderImage> {
    let buffer = image::RgbaImage::from_raw(w, h, rgba).expect("读回缓冲尺寸匹配");
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

impl CanvasSpikeApp {
    fn title(&self) -> String {
        format!(
            "canvas-spike [{}] 节点={} 缩放={:.2}x",
            self.route.label(),
            self.list.items.len(),
            self.zoom
        )
    }

    fn render_one_frame(&mut self, window: &mut Window, cx: &mut sable::gpui::Context<Self>) {
        let start = Instant::now();
        let viewport = window.viewport_size();
        let scale = window.scale_factor();
        let (dw, dh) = device_size(f64::from(viewport.width), f64::from(viewport.height), scale);
        let tf = gpu_frame::viewport_transform(
            scene::ARTBOARD,
            [f64::from(dw), f64::from(dh)],
            self.zoom,
        );

        let mut hits = 0u64;
        let mut misses = 0u64;
        let (frame, encode_ms, present_ms) = match self.route {
            Route::A => {
                let gpu = self.gpu.as_mut().expect("路线 (a) 需要 GPU(启动时已校验)");
                let t0 = Instant::now();
                let mut inner = vello::Scene::new();
                vb_render::gpu::encode_scene(&mut inner, &self.list);
                let mut out = vello::Scene::new();
                out.append(&inner, Some(tf));
                let encode = t0.elapsed().as_secs_f64() * 1e3;
                let (rgba, present) = gpu
                    .render_to_rgba(&out, dw, dh)
                    .expect("vello GPU 渲染失败");
                (
                    FrameData::Image(to_render_image(rgba, dw, dh)),
                    encode,
                    present.as_secs_f64() * 1e3,
                )
            }
            Route::B => {
                let gpu = self.gpu.as_mut().expect("路线 (b) 需要 GPU(启动时已校验)");
                let t0 = Instant::now();
                let mut sink = sable::paint::gpu::VelloSink::new();
                route_sable::translate_into(&self.list, tf, &mut sink, &mut hits, &mut misses);
                let part = sink.into_inner();
                let mut out = vello::Scene::new();
                out.append(&part, Some(tf));
                let encode = t0.elapsed().as_secs_f64() * 1e3;
                let (rgba, present) = gpu
                    .render_to_rgba(&out, dw, dh)
                    .expect("vello GPU 渲染失败");
                (
                    FrameData::Image(to_render_image(rgba, dw, dh)),
                    encode,
                    present.as_secs_f64() * 1e3,
                )
            }
            Route::B2 => {
                // CpuRenderer 全程 CPU(翻译 + vello_cpu 光栅一体);目标尺寸 =
                // 设备像素(与 (a)/(b) 同屏同口径)。present 记整段,encode 记 0。
                let t0 = Instant::now();
                let mut renderer = sable::paint::cpu::CpuRenderer::new(
                    dw as u16,
                    dh as u16,
                    [0x14, 0x16, 0x1a, 0xff],
                );
                route_sable::translate_into(
                    &self.list,
                    tf,
                    renderer.sink(),
                    &mut hits,
                    &mut misses,
                );
                let rgba = renderer.finish();
                let total = t0.elapsed().as_secs_f64() * 1e3;
                (FrameData::Image(to_render_image(rgba, dw, dh)), 0.0, total)
            }
            Route::C => {
                let (prims, encode) = route_gpui::translate(
                    &self.list,
                    self.zoom,
                    [f64::from(viewport.width), f64::from(viewport.height)],
                    window.text_system(),
                );
                (
                    FrameData::Prims(Arc::new(prims)),
                    encode.as_secs_f64() * 1e3,
                    0.0,
                )
            }
        };
        self.text_cache_hits = hits;
        self.text_cache_misses = misses;

        // 整帧周期(起点到起点;上一帧的全部工作 + 空闲,直接反映管线成本)。
        if let Some(last_start) = self.last_frame {
            let delta = start.duration_since(last_start).as_secs_f64() * 1e3;
            self.frame_delta_ms_ema = if self.frame_delta_ms_ema <= 0.0 {
                delta
            } else {
                self.frame_delta_ms_ema * 0.9 + delta * 0.1
            };
            self.samples.push(delta);
        }
        self.last_frame = Some(start);
        self.encode_ms = encode_ms;
        self.present_ms = present_ms;
        if self.frames_done == 0 {
            self.warmup_encode_ms = encode_ms;
            self.warmup_present_ms = present_ms;
        } else {
            self.steady_encode_ms += encode_ms;
            self.steady_present_ms += present_ms;
            self.steady_frames += 1;
        }
        self.frames_done += 1;

        if let Some(file) = self.csv.as_mut() {
            let _ = writeln!(
                file,
                "{:?},{},{},{:.3},{:.3},{:.3}",
                self.route,
                self.list.items.len(),
                self.zoom,
                encode_ms,
                present_ms,
                self.frame_delta_ms_ema
            );
        }
        if self.frames_done.is_multiple_of(60) {
            println!(
                "[frame {:>5}] fps={:>5.1} encode={:.2}ms present={:.2}ms",
                self.frames_done,
                1000.0 / self.frame_delta_ms_ema.max(1e-6),
                encode_ms,
                present_ms
            );
        }
        self.frame = Some(Arc::new(frame));

        // --frames N:跑满即出汇总并退出(Drop 兜底 flush)。
        if self.frames_target > 0 && self.frames_done >= self.frames_target {
            self.flush_and_finish();
            cx.quit();
        }
    }

    fn flush_and_finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        let mut sorted = self.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let pick = |p: f64| {
            sorted
                .get(((sorted.len() as f64) * p) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        println!(
            "SUMMARY route={:?} nodes={} zoom={} frames={} fps_ema={:.1} total_ms p50={:.2} p95={:.2} max={:.2} encode_mean_ms={:.2} present_mean_ms={:.2}",
            self.route,
            self.list.items.len(),
            self.zoom,
            self.frames_done,
            1000.0 / self.frame_delta_ms_ema.max(1e-6),
            pick(0.50),
            pick(0.95),
            sorted.last().copied().unwrap_or(0.0),
            if self.steady_frames > 0 {
                self.steady_encode_ms / self.steady_frames as f64
            } else {
                0.0
            },
            if self.steady_frames > 0 {
                self.steady_present_ms / self.steady_frames as f64
            } else {
                0.0
            }
        );
        println!(
            "WARMUP(首帧一次性成本) encode_ms={:.2} present_ms={:.2}",
            self.warmup_encode_ms, self.warmup_present_ms
        );
        if let Some(file) = self.csv.as_mut() {
            let _ = file.flush();
        }
    }
}

impl Drop for CanvasSpikeApp {
    fn drop(&mut self) {
        self.flush_and_finish();
    }
}

/// paint 期自检:零尺寸 bounds 意味着 canvas 元素没参与布局。
fn self_zero_check(b: Bounds<Pixels>) -> bool {
    f64::from(b.size.width) <= 0.0 || f64::from(b.size.height) <= 0.0
}

/// overlay 底衬(半透明黑;dev-stats 风格,spike 不接主题)。
fn scrim() -> Rgba {
    Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.6,
    }
}

impl Render for CanvasSpikeApp {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut sable::gpui::Context<Self>,
    ) -> impl IntoElement {
        self.render_one_frame(window, cx);

        let frame = self
            .frame
            .clone()
            .expect("render_one_frame 刚产出 FrameData");
        let entity = cx.entity();
        let stats = format!(
            "路线 {} | 节点 {} | 缩放 {:.2}x | FPS {:.1} | 翻译/编码 {:.2} ms | 上屏管线 {:.2} ms | 整帧EMA {:.2} ms | 文本缓存 命中{} 未命中{}",
            self.route.label(),
            self.list.items.len(),
            self.zoom,
            1000.0 / self.frame_delta_ms_ema.max(1e-6),
            self.encode_ms,
            self.present_ms,
            self.frame_delta_ms_ema,
            self.text_cache_hits,
            self.text_cache_misses,
        );

        div()
            .size_full()
            .relative()
            .bg(rgb(0x14161a))
            .child(
                canvas(
                    // prepaint:无状态(目标尺寸每帧由 viewport 现算)。
                    |_bounds: Bounds<Pixels>, _window, _cx| {},
                    move |paint_bounds: Bounds<Pixels>,
                          (),
                          window: &mut Window,
                          cx: &mut App| {
                        if self_zero_check(paint_bounds) {
                            eprintln!("paint_bounds 异常:{paint_bounds:?}");
                        }
                        match &*frame {
                            FrameData::Image(image) => {
                                if let Err(e) = window.paint_image(
                                    paint_bounds,
                                    Corners::all(px(0.0)),
                                    image.clone(),
                                    0,
                                    false,
                                ) {
                                    eprintln!("paint_image 失败:{e}");
                                }
                            }
                            FrameData::Prims(prims) => {
                                route_gpui::paint_prims(prims, window, cx);
                            }
                        }
                        // 连续重绘(FPS 计量的驱动源)。
                        window.request_animation_frame();
                        // 键位:1/2/3/4 切路线,-/0/= 切缩放(窗口级监听,免焦点;
                        // paint 期注册,下一帧自动失效,逐帧重挂)。
                        let entity = entity.clone();
                        window.on_key_event(
                            move |event: &KeyDownEvent,
                                  _phase: DispatchPhase,
                                  window: &mut Window,
                                  cx: &mut App| {
                                let key = event.keystroke.key.clone();
                                let mut new_title = None;
                                entity.update(cx, |app, cx| {
                                    let zoom = match key.as_str() {
                                        "-" => Some(0.25),
                                        "0" => Some(1.0),
                                        "=" => Some(4.0),
                                        _ => None,
                                    };
                                    if let Some(z) = zoom {
                                        app.zoom = z;
                                        new_title = Some(app.title());
                                        cx.notify();
                                        return;
                                    }
                                    if let Some(route) = Route::from_key(&key) {
                                        // GPU 路线在无 GPU 启动时禁切(保持现状路线)。
                                        if matches!(route, Route::A | Route::B) && app.gpu.is_none()
                                        {
                                            return;
                                        }
                                        app.route = route;
                                        new_title = Some(app.title());
                                        cx.notify();
                                    }
                                });
                                if let Some(title) = new_title {
                                    window.set_window_title(&title);
                                }
                            },
                        );
                    },
                )
                // canvas 无内性尺寸,必须显式撑满(否则布局高度为 0,
                // paint_bounds 为零矩形,paint_image 静默不画 —— spike 实测踩坑)。
                .size_full())
            .child(
                div()
                    .absolute()
                    .top(px(8.0))
                    .left(px(8.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .bg(scrim())
                    .rounded_sm()
                    .text_size(px(12.0))
                    .text_color(white())
                    .font_family(".SystemUIFont")
                    .child(stats),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(8.0))
                    .left(px(8.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .bg(scrim())
                    .rounded_sm()
                    .text_size(px(11.0))
                    .text_color(rgb(0x9aa4b0))
                    .font_family(".SystemUIFont")
                    .child("键位:1/2/3/4 = 路线(a 离屏读回 / b sableGPU / b2 sableCPU现状 / c gpui原生)· - / 0 / = = 缩放 0.25x / 1x / 4x"),
            )
    }
}

/// 离屏单帧落 PNG(1280×800;ADR 通道 parity 证据;路线 c 需要窗口,跳过)。
fn dump_scene(args: &Args, list: &DrawList, out: &str) {
    let (w, h) = (1280u32, 800u32);
    let tf =
        gpu_frame::viewport_transform(scene::ARTBOARD, [f64::from(w), f64::from(h)], args.zoom);
    let (mut hits, mut misses) = (0u64, 0u64);
    let rgba: Vec<u8> = match args.route {
        Route::A => {
            let mut gpu = GpuFrame::new().expect("GPU");
            let mut inner = vello::Scene::new();
            vb_render::gpu::encode_scene(&mut inner, list);
            let mut out_scene = vello::Scene::new();
            out_scene.append(&inner, Some(tf));
            gpu.render_to_rgba(&out_scene, w, h).expect("render").0
        }
        Route::B => {
            let mut gpu = GpuFrame::new().expect("GPU");
            let mut sink = sable::paint::gpu::VelloSink::new();
            route_sable::translate_into(list, tf, &mut sink, &mut hits, &mut misses);
            let part = sink.into_inner();
            let mut out_scene = vello::Scene::new();
            out_scene.append(&part, Some(tf));
            gpu.render_to_rgba(&out_scene, w, h).expect("render").0
        }
        Route::B2 => {
            let mut renderer =
                sable::paint::cpu::CpuRenderer::new(w as u16, h as u16, [0x14, 0x16, 0x1a, 0xff]);
            route_sable::translate_into(list, tf, renderer.sink(), &mut hits, &mut misses);
            renderer.finish()
        }
        Route::C => {
            eprintln!("路线 (c) 依赖 gpui 窗口 paint,不支持 --dump(用窗口截图目检)");
            std::process::exit(3);
        }
    };
    image::RgbaImage::from_raw(w, h, rgba)
        .expect("尺寸匹配")
        .save(out)
        .expect("PNG 写出失败");
    println!(
        "dump: {out}(route={:?} nodes={} zoom={})",
        args.route,
        list.items.len(),
        args.zoom
    );
}

fn main() {
    // gpui 平台层会安装默认 logger;parley 的 ICU4X CJK 分词缺内置数据,
    // 每个 layout miss 打一条 warn(不影响单行短文本的字形输出),压到
    // error 级避免 stderr 反压污染帧计时(ADR 记录此 sable 文本管线观察)。
    std::env::set_var("RUST_LOG", "error");
    let args = Args::parse();

    if let Some(dump) = args.dump.clone() {
        let list = scene::build_draw_list(args.nodes);
        dump_scene(&args, &list, &dump);
        return;
    }

    // 路线 (a)/(b) 依赖独立 wgpu 设备(启动期一次,阻塞式;失败 fail-fast)。
    let gpu = if matches!(args.route, Route::A | Route::B) {
        match GpuFrame::new() {
            Ok(g) => {
                println!("GPU adapter: {}", g.adapter_info());
                Some(g)
            }
            Err(e) => {
                eprintln!("GPU 初始化失败,路线 {} 不可用:{e}", args.route.label());
                std::process::exit(2);
            }
        }
    } else {
        None
    };

    let list = scene::build_draw_list(args.nodes);
    println!("场景:节点={} 画板={:?}", list.items.len(), scene::ARTBOARD);

    let csv = args.csv.as_ref().map(|path| {
        let mut file = std::fs::File::create(path).expect("CSV 创建失败");
        let _ = writeln!(file, "route,nodes,zoom,encode_ms,present_ms,total_ms_ema");
        file
    });
    let frames_target = args.frames;
    let initial_route = args.route;
    let initial_zoom = args.zoom;

    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.0), px(800.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(sable::gpui::TitlebarOptions {
                title: Some("canvas-spike".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let app = CanvasSpikeApp {
                route: initial_route,
                zoom: initial_zoom,
                list,
                gpu,
                frame: None,
                frame_delta_ms_ema: 0.0,
                encode_ms: 0.0,
                present_ms: 0.0,
                last_frame: None,
                frames_done: 0,
                frames_target,
                text_cache_hits: 0,
                text_cache_misses: 0,
                csv,
                samples: Vec::new(),
                steady_encode_ms: 0.0,
                steady_present_ms: 0.0,
                steady_frames: 0,
                warmup_encode_ms: 0.0,
                warmup_present_ms: 0.0,
                finished: false,
            };
            let title = app.title();
            let entity = cx.new(|_| app);
            window.set_window_title(&title);
            entity
        })
        .expect("canvas-spike 开窗失败");
        cx.activate(true);
    });
}
