//! 浏览器校对面板(03-4 / 能力台账 X-6)。
//!
//! 「视图 → 浏览器校对…」:左侧画布渲染、右侧系统浏览器渲染(复用
//! `vb_browser` 的原生 CDP 截图,无新增重依赖;不引入 WPI),支持
//! 并排 / 叠加 / 滑块三种对比模式,显示差异分数与差异热力图,可导出
//! 对比图。浏览器不可用(无系统 Edge/Chrome)时**显式降级**:面板给出
//! 可见警示且不产出分数(不得假绿)。
//!
//! 画布侧 = 真实画布 GPU 纹理读回(Vello 形状层,与 `--canvas-shot`
//! 同一采样;egui 文字近似层不在其中 —— 分数口径如实标注,不冒充
//! 「画布全渲染」)。

use egui::ColorImage;

use crate::app::VellumApp;

/// 校对状态机阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// 相机已排定,等画布用新相机渲染几帧后读回。
    WaitCanvasFrames(u32),
    /// 画布侧已取得;等浏览器后台线程。
    WaitBrowser,
    /// 完成(成功或降级)。
    Done,
}

/// 一次校对任务(纯数据;流程方法在 [`VellumApp`] 上)。
pub(crate) struct ProofreadJob {
    pub target_sid: String,
    pub target_name: String,
    /// 画板声明尺寸(世界 px;浏览器车道取景用)。
    pub aw: f64,
    pub ah: f64,
    stage: Stage,
    /// 相机备份(读回后恢复)。
    saved_camera: (f64, f64, f64),
    /// 排定时算好的画板纹理矩形(读回时裁剪用)。
    crop: [f64; 4],
    /// 画布侧图像(Vello 形状层)。
    canvas_img: Option<image::RgbaImage>,
    /// 浏览器侧图像(线程送回后存放)。
    browser_img: Option<image::RgbaImage>,
    /// 浏览器侧结果通道(未收取时为 Some)。
    browser_rx: Option<std::sync::mpsc::Receiver<(Result<image::RgbaImage, String>, String)>>,
    browser_error: Option<String>,
    /// 差异结果(分数由采样计算生成,禁止手写)。
    score: Option<f64>,
    diff_ratio: Option<f64>,
    diff_heat: Option<image::RgbaImage>,
    last_export: Option<String>,
}

/// egui 贴图(自由函数:不借 &mut self,避免与 job 借用交叠)。
fn make_tex(ctx: &egui::Context, name: String, img: &image::RgbaImage) -> egui::TextureHandle {
    let size = [img.width() as usize, img.height() as usize];
    let rgba = img.pixels().flat_map(|p| p.0).collect::<Vec<u8>>();
    let color = ColorImage::from_rgba_unmultiplied(size, &rgba);
    ctx.load_texture(name, color, egui::TextureOptions::LINEAR)
}

fn blend(a: &image::RgbaImage, b: &image::RgbaImage, t: f32) -> image::RgbaImage {
    let mut out = a.clone();
    let tb = (t.clamp(0.0, 1.0) * 255.0) as u16;
    for (x, y, p) in b.enumerate_pixels() {
        let o = out.get_pixel_mut(x, y);
        for c in 0..3 {
            o.0[c] = ((p.0[c] as u16 * tb + o.0[c] as u16 * (255 - tb)) / 255) as u8;
        }
        o.0[3] = 255;
    }
    out
}

/// 与画布侧同尺寸的浏览器侧(便于对比/混合)。
fn browser_at_canvas_size(job: &ProofreadJob) -> Option<image::RgbaImage> {
    let canvas = job.canvas_img.as_ref()?;
    let browser = job.browser_img.as_ref()?;
    Some(image::imageops::resize(
        browser,
        canvas.width(),
        canvas.height(),
        image::imageops::FilterType::Triangle,
    ))
}

impl VellumApp {
    /// 打开/关闭校对面板(`view.browser_proof`)。
    pub(crate) fn toggle_proofread(&mut self) {
        self.proofread_open = !self.proofread_open;
        if self.proofread_open {
            self.status = "浏览器校对:选择画板后点「开始校对」".into();
        }
    }

    /// 开始一次校对:排定相机 + 起浏览器后台线程。
    pub(crate) fn proofread_begin(&mut self, index: usize) {
        self.proofread_target = index; // 选择器与任务保持一致
        let Some(&ab) = self.doc.artboards.get(index) else {
            return;
        };
        let Some(n) = self.doc.nodes.get(ab) else {
            return;
        };
        let (ax, ay, aw, ah) = (n.geom.x, n.geom.y, n.geom.w, n.geom.h);
        let sid = n.sid.as_str().to_string();
        let name = n.name.clone();

        // 相机对准画板(以当前画布矩形为包络;与 fit 同口径,留 15% 边距)
        let (cw, ch) = self
            .canvas_rect
            .map(|r| (r.width() as f64, r.height() as f64))
            .unwrap_or((1200.0, 800.0));
        let zoom = ((cw * 0.85) / aw.max(1.0))
            .min((ch * 0.85) / ah.max(1.0))
            .clamp(0.05, 4.0);
        let saved = (self.camera.zoom, self.camera.pan_x, self.camera.pan_y);
        let pan_x = cw / 2.0 - (ax + aw / 2.0) * zoom;
        let pan_y = ch / 2.0 - (ay + ah / 2.0) * zoom;
        self.camera.zoom = zoom;
        self.camera.pan_x = pan_x;
        self.camera.pan_y = pan_y;

        // 纹理空间裁剪矩形(画布本地 = pan + world×zoom;DPI≠1 时按
        // 纹理/画布宽度比在读回端换算)
        let crop = [pan_x + ax * zoom, pan_y + ay * zoom, aw * zoom, ah * zoom];

        // 浏览器后台线程(系统 Edge/Chrome;失败显式降级,不假绿)
        let dir = self.project_dir.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = (|| -> Result<image::RgbaImage, String> {
                let Some(dir) = dir else {
                    return Err("项目目录未知(文档未保存到磁盘)".into());
                };
                let req = vb_browser::LaneRequest {
                    format: vb_browser::LaneFormat::Png,
                    width: aw.round() as u32,
                    height: ah.round() as u32,
                    scale: 1,
                    transparent: false,
                    artboard: true,
                    artboard_index: index,
                };
                let outcome = vb_browser::export_source(&dir, &req)?;
                image::load_from_memory(&outcome.bytes)
                    .map(|img| img.to_rgba8())
                    .map_err(|e| format!("浏览器截图解码失败:{e}"))
            })();
            let _ = tx.send((out, String::new()));
        });

        self.proofread = Some(ProofreadJob {
            target_sid: sid,
            target_name: name,
            aw,
            ah,
            stage: Stage::WaitCanvasFrames(3),
            saved_camera: saved,
            crop,
            canvas_img: None,
            browser_img: None,
            browser_rx: Some(rx),
            browser_error: None,
            score: None,
            diff_ratio: None,
            diff_heat: None,
            last_export: None,
        });
        self.status = "浏览器校对:等待画布帧…".into();
    }

    /// 每帧驱动(app.rs `ui` 末尾调用;无任务时零开销)。
    /// 全程 take-处理-放回:读回/相机恢复/差异计算都要 &mut self 全权。
    pub(crate) fn tick_proofread(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if self.proofread.is_none() {
            return;
        }
        let mut job = self.proofread.take().expect("job");
        match job.stage {
            Stage::WaitCanvasFrames(n) => {
                ctx.request_repaint();
                if n == 0 {
                    // 画布已按排定相机渲染过:读回纹理并裁画板矩形
                    match self.readback_canvas_region(frame, job.crop) {
                        Ok(img) => {
                            job.canvas_img = Some(img);
                            job.stage = Stage::WaitBrowser;
                            self.status = "浏览器校对:画布侧已取,浏览器渲染中…".into();
                        }
                        Err(e) => {
                            self.restore_proofread_camera();
                            job.stage = Stage::Done;
                            self.status = format!("浏览器校对:画布侧读取失败({e})");
                        }
                    }
                } else {
                    job.stage = Stage::WaitCanvasFrames(n - 1);
                }
            }
            Stage::WaitBrowser => {
                ctx.request_repaint();
                if let Some(rx) = &job.browser_rx {
                    match rx.try_recv() {
                        Ok((res, _engine)) => {
                            job.browser_rx = None;
                            self.restore_proofread_camera();
                            match res {
                                Ok(img) => {
                                    job.browser_img = Some(img);
                                    job.stage = Stage::Done;
                                    self.proofread = Some(job);
                                    self.finish_proofread_diff();
                                    self.status = "浏览器校对:完成".into();
                                    return;
                                }
                                Err(e) => {
                                    // 显式降级:不产分数,不假绿
                                    job.browser_error = Some(e);
                                    job.stage = Stage::Done;
                                    self.status = "浏览器校对不可用(降级,未产出分数)".into();
                                }
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {}
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            job.browser_rx = None;
                            job.browser_error = Some("浏览器校对线程异常退出".into());
                            job.stage = Stage::Done;
                            self.restore_proofread_camera();
                        }
                    }
                }
            }
            Stage::Done => {}
        }
        self.proofread = Some(job);
    }

    fn restore_proofread_camera(&mut self) {
        if let Some(job) = &self.proofread {
            let (z, px, py) = job.saved_camera;
            self.camera.zoom = z;
            self.camera.pan_x = px;
            self.camera.pan_y = py;
        }
    }

    /// 双侧齐备 → 计算差异分数与热力图(贴图在 UI 层惰性建)。
    fn finish_proofread_diff(&mut self) {
        let Some(job) = self.proofread.as_mut() else {
            return;
        };
        let (Some(canvas), Some(browser)) = (&job.canvas_img, &job.browser_img) else {
            return;
        };
        let browser = image::imageops::resize(
            browser,
            canvas.width(),
            canvas.height(),
            image::imageops::FilterType::Triangle,
        );
        let n = canvas.width() as usize * canvas.height() as usize;
        let mut diff_px = 0usize;
        let mut heat = image::RgbaImage::new(canvas.width(), canvas.height());
        for ((a, b), hp) in canvas.pixels().zip(browser.pixels()).zip(heat.pixels_mut()) {
            let la = (a.0[0] as u32 * 299 + a.0[1] as u32 * 587 + a.0[2] as u32 * 114) / 1000;
            let lb = (b.0[0] as u32 * 299 + b.0[1] as u32 * 587 + b.0[2] as u32 * 114) / 1000;
            let d = la.abs_diff(lb);
            if d > 16 {
                diff_px += 1;
            }
            let v = (d * 3).min(255) as u8;
            *hp = [v, 0, 0, 255].into();
        }
        job.diff_ratio = Some(diff_px as f64 / n as f64);
        job.score = Some(1.0 - diff_px as f64 / n as f64);
        job.diff_heat = Some(heat);
    }

    /// 校对面板 UI(egui Window;app.rs 每帧调用)。
    pub(crate) fn show_proofread_window(&mut self, ctx: &egui::Context) {
        if !self.proofread_open {
            return;
        }
        let mut open = self.proofread_open;
        egui::Window::new("浏览器校对")
            .open(&mut open)
            .default_width(1100.0)
            .default_height(660.0)
            .show(ctx, |ui| {
                self.proofread_ui(ui, ctx);
            });
        self.proofread_open = open;
    }

    fn proofread_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // 画板选择 + 开始
        let labels: Vec<String> = self
            .doc
            .artboards
            .iter()
            .enumerate()
            .map(|(i, &a)| {
                self.doc
                    .nodes
                    .get(a)
                    .map(|n| format!("{}({})", n.name, n.sid.as_str()))
                    .unwrap_or_else(|| format!("画板 {i}"))
            })
            .collect();
        let mut picked = self.proofread_target;
        egui::ComboBox::from_label("画板")
            .selected_text(labels.get(picked).cloned().unwrap_or_else(|| "—".into()))
            .show_ui(ui, |ui| {
                for (i, l) in labels.iter().enumerate() {
                    ui.selectable_value(&mut picked, i, l);
                }
            });
        self.proofread_target = picked;
        ui.end_row();

        let running = self
            .proofread
            .as_ref()
            .map(|j| !matches!(j.stage, Stage::Done))
            .unwrap_or(false);
        ui.add_enabled_ui(!running, |ui| {
            if ui.button("开始校对").clicked() {
                self.proofread_begin(self.proofread_target);
            }
        });
        if running {
            ui.spinner();
        }
        ui.separator();

        let Some(job) = self.proofread.as_ref() else {
            ui.label(
                "把「画布上的渲染」与「真实浏览器渲染」并排对拍:确认画布近似的偏差范围, \
                 以及验证保存后的 HTML 在浏览器里的效果。",
            );
            return;
        };

        // 浏览器侧失败 → 显式降级警示(不假绿);提示级走主题 warn 令牌(07-I 口径)
        if let Some(err) = &job.browser_error {
            ui.colored_label(
                vb_ui::theme::tokens(ui.ctx()).warn,
                format!("⚠ 浏览器校对不可用:{err}。本次校对降级,不产出分数(不假绿)。"),
            );
        }
        if matches!(job.stage, Stage::WaitCanvasFrames(_)) {
            ui.label("等待画布帧(相机对准中)…");
            return;
        }
        if matches!(job.stage, Stage::WaitBrowser) {
            ui.label("画布侧已取;浏览器渲染中(系统 Edge/Chrome,后台)…");
            return;
        }

        // 结果区:分数 + 说明
        if let (Some(score), Some(ratio)) = (job.score, job.diff_ratio) {
            ui.label(format!(
                "差异分数 {score:.4}(灰度不一致像素占比 {:.2}%;分数由本次采样生成)",
                ratio * 100.0
            ));
            ui.small(
                "口径:画布侧为 Vello 形状层(不含画布 egui 文字近似层),\
                 文字区域计入差异 —— 分数偏保守,用于趋势观察。",
            );
        } else if job.browser_error.is_some() {
            ui.label("无分数(浏览器侧降级)。画布侧截图仍可查看。");
        } else if job.canvas_img.is_none() {
            ui.label("画布侧未采样。");
        }
        ui.separator();

        // 贴图(自由函数建,避免 &mut self 与 job 借用交叠)
        let tex_canvas = job
            .canvas_img
            .as_ref()
            .map(|img| make_tex(ctx, format!("proofread-canvas-{}", job.target_sid), img));
        let tex_browser = job
            .browser_img
            .as_ref()
            .map(|img| make_tex(ctx, format!("proofread-browser-{}", job.target_sid), img));
        let tex_heat = job
            .diff_heat
            .as_ref()
            .map(|img| make_tex(ctx, format!("proofread-heat-{}", job.target_sid), img));

        let mut want_export = false;
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.proofread_show_heat, "显示差异热力图");
            ui.separator();
            ui.radio_value(&mut self.proofread_mode, 0, "并排");
            ui.radio_value(&mut self.proofread_mode, 1, "叠加");
            ui.radio_value(&mut self.proofread_mode, 2, "滑块");
            if self.proofread_mode == 2 {
                ui.add(egui::Slider::new(&mut self.proofread_slider, 0.0..=1.0).text("分割"));
            }
            let can_export = job.diff_heat.is_some();
            if ui
                .add_enabled(can_export, egui::Button::new("导出对比图"))
                .clicked()
            {
                want_export = true; // 借还后再执行(需要 &mut self)
            }
        });
        if let Some(p) = &job.last_export {
            ui.small(format!("上次导出:{p}"));
        }

        // 图像展示(克隆所需图像到局部,避开借用)
        let mode = self.proofread_mode;
        let slider = self.proofread_slider;
        let pair = job
            .canvas_img
            .as_ref()
            .zip(job.browser_img.as_ref())
            .map(|(c, b)| {
                (
                    c.clone(),
                    image::imageops::resize(
                        b,
                        c.width(),
                        c.height(),
                        image::imageops::FilterType::Triangle,
                    ),
                )
            });
        let half = egui::vec2((ui.available_width() / 2.0).min(520.0), 330.0);
        let one = egui::vec2(ui.available_width().min(640.0), 380.0);
        let img_ui = |ui: &mut egui::Ui, tex: &egui::TextureHandle, size: egui::Vec2| {
            ui.image((tex.id(), size));
        };
        match mode {
            0 => {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(format!("画布 · {}", job.target_name));
                        if let Some(t) = &tex_canvas {
                            img_ui(ui, t, half);
                        } else {
                            ui.label("(无)");
                        }
                    });
                    ui.vertical(|ui| {
                        ui.label("浏览器");
                        if let Some(t) = &tex_browser {
                            img_ui(ui, t, half);
                        } else {
                            ui.label("(无 —— 浏览器侧降级)");
                        }
                    });
                });
            }
            1 => {
                if let Some((c, b)) = &pair {
                    let blended = blend(c, b, slider);
                    let tex = make_tex(ctx, "proofread-blend".into(), &blended);
                    img_ui(ui, &tex, one);
                    ui.small("叠加:透明度滑块(0 = 画布,1 = 浏览器)");
                }
            }
            _ => {
                if let Some((c, b)) = &pair {
                    let split = (c.width() as f32 * slider) as u32;
                    let mut view = c.clone();
                    for (x, y, p) in b.enumerate_pixels() {
                        if x >= split {
                            *view.get_pixel_mut(x, y) = *p;
                        }
                    }
                    let tex = make_tex(ctx, "proofread-slider".into(), &view);
                    img_ui(ui, &tex, one);
                    ui.small("滑块对比:左侧 = 画布,右侧 = 浏览器");
                }
            }
        }
        if self.proofread_show_heat {
            if let Some(h) = &tex_heat {
                ui.separator();
                ui.label("差异热力图(红 = 不一致)");
                img_ui(ui, h, one);
            }
        }
        // 完成态下允许换画板重跑(tick 不再推进)
        let _ = ctx;
        if want_export {
            self.proofread_export_compare();
        }
    }

    /// 导出对比图:画布 | 浏览器 | 热力图 三联,落到 %TEMP%。
    fn proofread_export_compare(&mut self) {
        let Some(job) = self.proofread.as_ref() else {
            return;
        };
        let out = (|| -> Option<image::RgbaImage> {
            let canvas = job.canvas_img.as_ref()?;
            let browser = browser_at_canvas_size(job).or_else(|| job.browser_img.clone())?;
            let heat = job.diff_heat.as_ref()?;
            let (w, h) = (canvas.width(), canvas.height());
            let mut out = image::RgbaImage::new(w * 3 + 16, h + 24);
            image::imageops::overlay(&mut out, canvas, 4i64, 20i64);
            image::imageops::overlay(&mut out, &browser, (w + 8) as i64, 20i64);
            image::imageops::overlay(&mut out, heat, (w * 2 + 12) as i64, 20i64);
            Some(out)
        })();
        let Some(out) = out else {
            self.status = "对比图导出失败(缺画布/浏览器/热力图之一)".into();
            return;
        };
        let dir = std::env::temp_dir().join("vb-iter").join("proofread");
        if std::fs::create_dir_all(&dir).is_err() {
            self.status = "对比图导出失败:建目录失败".into();
            return;
        }
        let path = dir.join(format!(
            "compare-{}-{}x{}.png",
            job.target_sid, job.aw as u32, job.ah as u32
        ));
        match out.save(&path) {
            Ok(()) => {
                if let Some(job) = self.proofread.as_mut() {
                    job.last_export = Some(path.display().to_string());
                }
                self.status = format!("对比图已导出:{}", path.display());
            }
            Err(e) => self.status = format!("对比图导出失败:{e}"),
        }
    }
}
