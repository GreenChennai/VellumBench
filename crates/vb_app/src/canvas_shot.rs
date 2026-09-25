//! 画布出图通道(P0-③ / 03-3 `canvas_parity` 门禁的画布侧采样)。
//!
//! 隐藏启动参数:`vellumbench --project <dir> --canvas-shot <out.png> --artboard <sid>`
//!
//! 流程(见 [`CanvasShotCfg::tick`]):
//! ① 启动后第 [`Self::STAGE_FRAME`] 帧:把相机**按固定标称框**对准目标画板
//!    (不读当前画布矩形 —— 否则缺陷会被"相机缩 compensate"掩盖),
//!    并把窗口从 1680×1000 调整到 2200×1200。这次**窗口增长**正是探针:
//!    画布矩形若不能随窗口增长(03-1 的 P0-③ 根因),GPU 纹理停在旧尺寸,
//!    画板在目标缩放下必然放不全。
//! ② 第 [`Self::SHOT_FRAME`] 帧:把画布 GPU 纹理(与 on-screen 画布**同一张**,
//!    vello 层;egui 叠加的文字层不在其中,见 manifest 的 `layers` 字段)
//!    读回落盘 PNG,并写 manifest JSON(画板尺寸/缩放/裁剪矩形/是否完整)。
//! ③ 门禁脚本 [`tools/canvas_parity.ps1`] 读 manifest 与导出 PNG 对拍:
//!    硬判据 = 完整可见 + 画板矩形尺寸与导出一致(换算后 ≤1px)。
//! ④ 发送退出,进程自然结束。

use std::path::PathBuf;

use crate::VellumApp;

/// 出图配置(由启动参数解析而来)。
#[derive(Debug, Clone)]
pub struct CanvasShotCfg {
    /// 画布侧 PNG 输出路径。
    pub out_png: PathBuf,
    /// 目标画板:sid(`f0a1b2`)优先,其次按名称,最后按序号(0 起)。
    pub artboard: String,
    frame: u32,
    staged: bool,
}

impl CanvasShotCfg {
    pub fn new(out_png: PathBuf, artboard: String) -> Self {
        Self {
            out_png,
            artboard,
            frame: 0,
            staged: false,
        }
    }
}

/// 相机/窗口调整已排定后等待的帧数(等 OS 真正完成 resize + 重排)。
const SETTLE_FRAMES: u32 = 30;
/// 相机排定发生在第几帧(避开首帧面板未稳 + 工作区恢复)。
const STAGE_FRAME: u32 = 20;
/// 出图时画板目标包络(逻辑 px):缩放 = min(包络/画板, 2.0),与
/// 运行窗口无关(决定性,不随画布矩形浮动 —— 否则缺陷被缩放掩盖)。
const FIT_W: f64 = 1600.0;
const FIT_H: f64 = 900.0;
/// 探针用目标窗口尺寸(逻辑 px;必须与启动默认 1680×1000 不同)。
const RESIZE_TO: (f32, f32) = (2200.0, 1200.0);

impl VellumApp {
    /// 每帧末尾调用(app.rs `ui` 末尾):驱动出图状态机。未启用时零开销。
    pub(crate) fn tick_canvas_shot(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.canvas_shot.is_none() {
            return;
        }
        let cfg = self.canvas_shot.as_mut().expect("cfg");
        cfg.frame += 1;
        ctx.request_repaint();
        if cfg.frame == STAGE_FRAME {
            // 先取出再排相机(stage_camera 要 &mut self 全权;画板键作参传入,
            // 不能从 self.canvas_shot 读 —— 刚被 take 走)
            let mut cfg = self.canvas_shot.take().expect("cfg");
            self.stage_camera(&cfg.artboard, ctx);
            cfg.staged = true;
            self.canvas_shot = Some(cfg);
            return;
        }
        if cfg.staged && cfg.frame >= STAGE_FRAME + SETTLE_FRAMES {
            // 取出 cfg,避免 &mut self 与读回阶段的借用交叠
            let cfg = self.canvas_shot.take().expect("cfg");
            if let Err(e) = self.capture_shot(&cfg, frame) {
                eprintln!("canvas-shot 失败:{}", e);
                write_manifest(
                    &cfg,
                    &ShotManifest {
                        error: Some(e),
                        ..Default::default()
                    },
                );
            }
            // 无论成败,采完即退(shell 的 QuitAll 会自然关掉整个进程)
            if let Some(tx) = &self.shell_tx {
                let _ = tx.send(crate::shell::ShellRequest::QuitAll);
            } else {
                std::process::exit(0);
            }
        }
    }

    /// 读回整张画布纹理(Vello 层),返回 (整幅 RGBA, point→物理px 比)。
    pub(crate) fn readback_canvas_texture(
        &self,
        frame: &eframe::Frame,
    ) -> Result<(image::RgbaImage, f64), String> {
        let Some(rs) = frame.wgpu_render_state() else {
            return Err("无 wgpu 渲染状态".into());
        };
        let Some(g) = self.gpu.as_ref() else {
            return Err("GPU 画布未初始化(vello 不可用?)".into());
        };
        let Some((tex, _, tex_size, _)) = g.tex.as_ref() else {
            return Err("画布纹理不存在".into());
        };
        let (tw, th) = (tex_size[0], tex_size[1]);
        let scale = match self.canvas_rect {
            Some(r) if r.width() > 1.0 => tw as f64 / r.width() as f64,
            _ => 1.0,
        };

        // 纹理 → buffer(bytes_per_row 256 对齐)
        let bpr = align256(tw * 4);
        let buf = rs.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("vb-canvas-readback"),
            size: bpr as u64 * th as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = rs
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("vb-readback"),
            });
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bpr),
                    rows_per_image: Some(th),
                },
            },
            wgpu::Extent3d {
                width: tw,
                height: th,
                depth_or_array_layers: 1,
            },
        );
        rs.queue.submit(Some(enc.finish()));
        let slice = buf.slice(..);
        let (tx_, rx_) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx_.send(r);
        });
        let _ = rs.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        });
        rx_.recv()
            .map_err(|_| "map_async 通道关闭".to_string())?
            .map_err(|e| format!("纹理映射失败:{e}"))?;
        let data = slice.get_mapped_range();
        let mut img = image::RgbaImage::new(tw, th);
        for row in 0..th as usize {
            let start = row * bpr as usize;
            let row_bytes = &data[start..start + (tw as usize) * 4];
            for (col, px) in row_bytes.as_chunks::<4>().0.iter().enumerate() {
                img.put_pixel(
                    col as u32,
                    row as u32,
                    image::Rgba([px[0], px[1], px[2], px[3]]),
                );
            }
        }
        drop(data);
        buf.unmap();
        Ok((img, scale))
    }

    /// 读回画布纹理并裁出矩形(入参为画布本地**点**坐标 [x,y,w,h])。
    pub(crate) fn readback_canvas_region(
        &self,
        frame: &eframe::Frame,
        crop_points: [f64; 4],
    ) -> Result<image::RgbaImage, String> {
        let (full, scale) = self.readback_canvas_texture(frame)?;
        let (tw, th) = (full.width() as f64, full.height() as f64);
        let (cx, cy, cw, ch) = (
            crop_points[0] * scale,
            crop_points[1] * scale,
            crop_points[2] * scale,
            crop_points[3] * scale,
        );
        let x0 = cx.clamp(0.0, tw).round() as u32;
        let y0 = cy.clamp(0.0, th).round() as u32;
        let x1 = (cx + cw).clamp(0.0, tw).round() as u32;
        let y1 = (cy + ch).clamp(0.0, th).round() as u32;
        if x1 <= x0 || y1 <= y0 {
            return Err(format!(
                "画板裁剪矩形为空({x0},{y0})-({x1},{y1});纹理 {tw}x{th}"
            ));
        }
        Ok(image::imageops::crop_imm(&full, x0, y0, x1 - x0, y1 - y0).to_image())
    }

    /// 画布出图(门禁 --canvas-shot):读回 + 裁剪 + 落盘 + manifest。
    fn capture_shot(&self, cfg: &CanvasShotCfg, frame: &eframe::Frame) -> Result<(), String> {
        // 目标画板与当前相机 → 纹理空间裁剪矩形(与 stage_camera 同一变换)
        let (ab_id, ab_name, g) = find_artboard(&self.doc, &cfg.artboard)
            .ok_or_else(|| format!("找不到画板 `{}`", cfg.artboard))?;
        let [ax, ay, aw, ah] = g;
        let (z, pan_x, pan_y) = self.camera_shot_transform(ax, ay, aw, ah);
        let crop = [pan_x + ax * z, pan_y + ay * z, aw * z, ah * z];
        let (img, scale) = self.readback_canvas_texture(frame)?;
        let px = |v: f64| (v * scale).round();
        let (tw, th) = (img.width() as f64, img.height() as f64);
        let (cx, cy) = (px(crop[0]), px(crop[1]));
        let (cw, ch) = (px(crop[2]).max(1.0), px(crop[3]).max(1.0));
        let fully_visible = cx >= 0.0 && cy >= 0.0 && cx + cw <= tw && cy + ch <= th;
        let x0 = cx.clamp(0.0, tw) as u32;
        let y0 = cy.clamp(0.0, th) as u32;
        let x1 = (cx + cw).clamp(0.0, tw) as u32;
        let y1 = (cy + ch).clamp(0.0, th) as u32;
        let cropped = image::imageops::crop(&mut img.clone(), x0, y0, x1 - x0, y1 - y0).to_image();

        if let Some(parent) = cfg.out_png.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("建目录失败:{e}"))?;
        }
        cropped
            .save(&cfg.out_png)
            .map_err(|e| format!("PNG 落盘失败:{e}"))?;

        write_manifest(
            cfg,
            &ShotManifest {
                artboard_sid: self
                    .doc
                    .nodes
                    .get(ab_id)
                    .map(|n| n.sid.as_str().to_string())
                    .unwrap_or_default(),
                artboard_name: ab_name,
                artboard_w: aw,
                artboard_h: ah,
                zoom: z,
                point_to_px: scale,
                texture_w: tw as u32,
                texture_h: th as u32,
                crop: [x0 as f64, y0 as f64, (x1 - x0) as f64, (y1 - y0) as f64],
                fully_visible,
                layers: "vello(形状/图像;egui 文字叠加层不在读回纹理内)",
                canvas_rect_points: self
                    .canvas_rect
                    .map(|r| [r.left(), r.top(), r.right(), r.bottom()]),
                error: None,
            },
        );
        Ok(())
    }

    /// 出图相机的决定性变换(缩放 + 平移)。**只依赖画板几何与标称包络**,
    /// 不读 `canvas_rect` —— 探针的意义就是检验画布矩形能否容纳标称缩放
    /// 下的画板(03-1 根因回归)。平移对准画板的**世界中心**(多画板时
    /// 第二画板有纵向堆叠偏移,必须一起折算,否则裁剪矩形落空)。
    fn camera_shot_transform(&self, ax: f64, ay: f64, aw: f64, ah: f64) -> (f64, f64, f64) {
        let z = (FIT_W / aw.max(1.0))
            .min(FIT_H / ah.max(1.0))
            .clamp(0.05, 2.0);
        let pan_x = RESIZE_TO.0 as f64 / 2.0 - (ax + aw / 2.0) * z;
        let pan_y = RESIZE_TO.1 as f64 / 2.0 - 40.0 - (ay + ah / 2.0) * z;
        (z, pan_x, pan_y)
    }
}

impl VellumApp {
    /// 相机排定:①把相机按标称变换对准目标画板(决定性,见
    /// `camera_shot_transform`;不应用的话画布仍按启动相机渲染,裁剪会错位);
    /// ②把窗口从启动默认 1680×1000 调整到探针尺寸。**这次窗口增长就是探针
    /// 本身**:画布矩形若能跟随增长(修复后),GPU 纹理会随之重建;若停留
    /// 在首帧尺寸(P0-③ 旧实现),按标称缩放置入的画板必然放不全。
    fn stage_camera(&mut self, key: &str, ctx: &egui::Context) {
        let Some([ax, ay, aw, ah]) = find_artboard(&self.doc, key).map(|(_, _, g)| g) else {
            return;
        };
        let (z, pan_x, pan_y) = self.camera_shot_transform(ax, ay, aw, ah);
        self.camera.zoom = z;
        self.camera.pan_x = pan_x;
        self.camera.pan_y = pan_y;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(
            RESIZE_TO.0,
            RESIZE_TO.1,
        )));
    }
}
/// 画板查找(sid 优先 → 名称 → 序号),返回 (id, 名称, [x, y, w, h])。
type AbHit = (vb_doc::model::NodeId, String, [f64; 4]);
fn find_artboard(doc: &vb_doc::model::Document, key: &str) -> Option<AbHit> {
    let geom = |n: &vb_doc::model::Node| [n.geom.x, n.geom.y, n.geom.w, n.geom.h];
    if let Some(id) = doc.find_by_sid(key) {
        if let Some(n) = doc.nodes.get(id) {
            return Some((id, n.name.clone(), geom(n)));
        }
    }
    if let Some((id, n)) = doc
        .artboards
        .iter()
        .filter_map(|&a| doc.nodes.get(a).map(|n| (a, n)))
        .find(|(_, n)| n.name == key)
    {
        return Some((id, n.name.clone(), geom(n)));
    }
    if let Ok(i) = key.parse::<usize>() {
        if let Some(&a) = doc.artboards.get(i) {
            if let Some(n) = doc.nodes.get(a) {
                return Some((a, n.name.clone(), geom(n)));
            }
        }
    }
    None
}

#[derive(Default)]
struct ShotManifest {
    error: Option<String>,
    artboard_sid: String,
    artboard_name: String,
    artboard_w: f64,
    artboard_h: f64,
    zoom: f64,
    point_to_px: f64,
    texture_w: u32,
    texture_h: u32,
    crop: [f64; 4],
    fully_visible: bool,
    layers: &'static str,
    canvas_rect_points: Option<[f32; 4]>,
}

/// manifest 手拼 JSON(字段全为标量,无需引入反序列化层)。
fn write_manifest(cfg: &CanvasShotCfg, m: &ShotManifest) {
    let path = cfg.out_png.with_extension("json");
    let rect = m
        .canvas_rect_points
        .map(|r| format!("[{},{},{},{}]", r[0], r[1], r[2], r[3]))
        .unwrap_or_else(|| "null".into());
    let json = format!(
        "{{\n \
         \"png\": {:?},\n \
         \"error\": {},\n \
         \"artboard_sid\": {:?},\n \
         \"artboard_name\": {:?},\n \
         \"artboard_w\": {:.3},\n \
         \"artboard_h\": {:.3},\n \
         \"zoom\": {:.6},\n \
         \"point_to_px\": {:.6},\n \
         \"texture_w\": {},\n \
         \"texture_h\": {},\n \
         \"crop\": [{:.0},{:.0},{:.0},{:.0}],\n \
         \"fully_visible\": {},\n \
         \"layers\": {:?},\n \
         \"canvas_rect_points\": {}\n}}",
        cfg.out_png
            .file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default(),
        m.error
            .as_ref()
            .map(|e| format!("{e:?}"))
            .unwrap_or_else(|| "null".into()),
        m.artboard_sid,
        m.artboard_name,
        m.artboard_w,
        m.artboard_h,
        m.zoom,
        m.point_to_px,
        m.texture_w,
        m.texture_h,
        m.crop[0],
        m.crop[1],
        m.crop[2],
        m.crop[3],
        m.fully_visible,
        m.layers,
        rect
    );
    if let Err(e) = std::fs::write(&path, json) {
        eprintln!("manifest 写入失败({}):{e}", path.display());
    }
}

fn align256(v: u32) -> u32 {
    v.div_ceil(256) * 256
}
