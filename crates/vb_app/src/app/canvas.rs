//! 画布渲染(S1-a 自 app.rs 机械搬移,零行为变化):
//! CentralPanel 装配、GPU(Vello)纹理合成、画板/轮廓/覆盖层绘制与网格。

use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Stroke};
use vb_doc::model::NodeKind;
use vb_render::encode::encode_artboard;
use vb_tools::Camera;
use vb_ui::theme::{semantic, Tokens};

use super::{Drag, GpuCanvas, Tool, VellumApp};

impl VellumApp {
    pub(crate) fn canvas(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::canvas(ui.style()).fill(Tokens::get(self.theme_dark).bg_canvas))
            .show(ui, |ui| {
                let Some(rect) = self
                    .canvas_rect
                    .or(Some(ui.max_rect()))
                    .map(|r| r.intersect(ui.max_rect()))
                else {
                    return;
                };
                let rect = rect.expand(0.0);
                self.canvas_rect = Some(rect);
                let response = ui.allocate_rect(rect, Sense::click_and_drag());
                let painter = ui.painter().with_clip_rect(rect);

                // 网格画在最底层
                if self.grid_on {
                    draw_grid(&painter, rect, &self.camera, self.theme_dark);
                }

                // 渲染画布内容(GPU,画板背景会盖住网格)
                if self.outline_mode {
                    // 轮廓模式(Mod+Y):不画实体,只勾勒每个对象的绝对边界
                    self.draw_outline_mode(&painter, rect);
                } else {
                    self.render_canvas_gpu(frame, rect);
                    if let Some(tex_id) = self.tex_id() {
                        painter.image(
                            tex_id,
                            rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                }
                self.draw_artboards(&painter, rect.min.to_vec2());
                self.draw_overlays(&painter, rect);

                self.handle_canvas_input(&response, ui.ctx().clone(), rect);
            });
    }

    fn render_canvas_gpu(&mut self, frame: &eframe::Frame, rect: Rect) {
        let Some(rs) = frame.wgpu_render_state() else {
            return;
        };
        let size = [
            (rect.width().max(1.0) as u32).min(4096),
            (rect.height().max(1.0) as u32).min(4096),
        ];

        // 初始化 vello renderer(一次)
        if self.gpu.is_none() {
            match vello::Renderer::new(
                &rs.device,
                vello::RendererOptions {
                    use_cpu: false,
                    antialiasing_support: vello::AaSupport::all(),
                    num_init_threads: std::num::NonZeroUsize::new(1),
                    pipeline_cache: None,
                },
            ) {
                Ok(r) => {
                    self.gpu = Some(GpuCanvas {
                        renderer: r,
                        tex: None,
                    })
                }
                Err(e) => {
                    eprintln!("Vello 初始化失败(画布将无内容):{e}");
                    return;
                }
            }
        }
        let gpu = self.gpu.as_mut().unwrap();

        // 纹理尺寸管理
        let need_recreate = match &gpu.tex {
            Some((_, _, s, _)) => s != &size,
            None => true,
        };
        if need_recreate {
            if let Some((_, _, _, old_id)) = gpu.tex.take() {
                rs.renderer.write().free_texture(&old_id);
            }
            let tex = rs.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vb-canvas"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let id = rs.renderer.write().register_native_texture(
                &rs.device,
                &view,
                wgpu::FilterMode::Linear,
            );
            gpu.tex = Some((tex, view, size, id));
        }

        // 编码 + 渲染(当前画板;多画板逐个编码)
        let mut scene = vello::Scene::new();
        for &ab in &self.doc.artboards {
            let mut list = match encode_artboard(&self.doc, ab) {
                Ok(l) => l,
                Err(_) => continue,
            };
            if let Some(dir) = self.project_dir.clone() {
                let cache = &mut self.image_cache;
                vb_render::encode::attach_images(&mut list, &mut |src| {
                    if let Some(bmp) = cache.get(src) {
                        return Some(bmp.clone());
                    }
                    let img = image::open(dir.join(src)).ok()?;
                    let rgba = img.to_rgba8();
                    let bmp = vb_render::encode::BitmapData {
                        width: rgba.width(),
                        height: rgba.height(),
                        rgba: std::sync::Arc::new(rgba.into_raw()),
                    };
                    cache.insert(src.to_string(), bmp.clone());
                    Some(bmp)
                });
            }
            {
                let n = self.doc.nodes.get(ab).unwrap();
                let z = self.camera.zoom;
                let tx = self.camera.pan_x + n.geom.x * z;
                let ty = self.camera.pan_y + n.geom.y * z;
                let tf = vello::kurbo::Affine::translate(vello::kurbo::Vec2::new(tx, ty))
                    * vello::kurbo::Affine::scale(z);
                let mut sub = vello::Scene::new();
                vb_render::gpu::encode_scene(&mut sub, &list);
                scene.append(&sub, Some(tf));
            }
        }
        // P4.3 隔离模式:遮罩压暗非隔离内容,隔离子树顶层重绘保持全亮
        // (06 篇 §4.3「淡化 25%」;重绘与所属画板同一变换)
        if let Some(iso) = self.isolate_top() {
            if let Some(ab) = vb_tools::artboard_of(&self.doc, iso) {
                if let Some(abn) = self.doc.nodes.get(ab) {
                    let z = self.camera.zoom;
                    let tf = vello::kurbo::Affine::translate(vello::kurbo::Vec2::new(
                        self.camera.pan_x + abn.geom.x * z,
                        self.camera.pan_y + abn.geom.y * z,
                    )) * vello::kurbo::Affine::scale(z);
                    let c = Tokens::get(self.theme_dark).bg_canvas;
                    let scrim = vello::peniko::Color::from_rgba8(c.r(), c.g(), c.b(), 185);
                    let mut sc = vello::Scene::new();
                    sc.fill(
                        vello::peniko::Fill::NonZero,
                        vello::kurbo::Affine::IDENTITY,
                        scrim,
                        None,
                        &vello::kurbo::Rect::new(
                            -1.0e5,
                            -1.0e5,
                            abn.geom.w + 1.0e5,
                            abn.geom.h + 1.0e5,
                        ),
                    );
                    scene.append(&sc, Some(tf));
                    if let Ok(mut list) = vb_render::encode::encode_subtree(&self.doc, iso) {
                        // 隔离子树同样挂载位图(B3)
                        if let Some(dir) = self.project_dir.clone() {
                            let cache = &mut self.image_cache;
                            vb_render::encode::attach_images(&mut list, &mut |src| {
                                if let Some(bmp) = cache.get(src) {
                                    return Some(bmp.clone());
                                }
                                let img = image::open(dir.join(src)).ok()?;
                                let rgba = img.to_rgba8();
                                let bmp = vb_render::encode::BitmapData {
                                    width: rgba.width(),
                                    height: rgba.height(),
                                    rgba: std::sync::Arc::new(rgba.into_raw()),
                                };
                                cache.insert(src.to_string(), bmp.clone());
                                Some(bmp)
                            });
                        }
                        let mut sub = vello::Scene::new();
                        vb_render::gpu::encode_scene(&mut sub, &list);
                        scene.append(&sub, Some(tf));
                    }
                }
            }
        }
        let gpu = self.gpu.as_mut().unwrap();
        if let Some((_, view, s, _)) = &gpu.tex {
            let params = vello::RenderParams {
                // 画板外留白随主题(此前浅色主题下仍是深色纹理)
                base_color: if self.theme_dark {
                    vello::peniko::Color::from_rgb8(0x14, 0x14, 0x14)
                } else {
                    vello::peniko::Color::from_rgb8(0xE8, 0xE6, 0xE2)
                },
                width: s[0],
                height: s[1],
                antialiasing_method: vello::AaConfig::Area,
            };
            if let Err(e) = gpu
                .renderer
                .render_to_texture(&rs.device, &rs.queue, &scene, view, &params)
            {
                eprintln!("vello 渲染失败:{e}");
            }
        }
    }

    fn tex_id(&self) -> Option<egui::TextureId> {
        self.gpu.as_ref().and_then(|g| g.tex.as_ref().map(|t| t.3))
    }

    fn draw_artboards(&self, painter: &egui::Painter, origin: egui::Vec2) {
        let t = Tokens::get(self.theme_dark);
        for &ab in &self.doc.artboards {
            let Some(n) = self.doc.nodes.get(ab) else {
                continue;
            };
            let (x0, y0) = self.camera.world_to_screen(n.geom.x, n.geom.y);
            let (x1, y1) = self
                .camera
                .world_to_screen(n.geom.x + n.geom.w, n.geom.y + n.geom.h);
            let r = Rect::from_min_max(
                pos2(x0 as f32 + origin.x, y0 as f32 + origin.y),
                pos2(x1 as f32 + origin.x, y1 as f32 + origin.y),
            );
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(1.0, t.border),
                egui::StrokeKind::Outside,
            );
            painter.text(
                pos2(r.left(), r.top() - 16.0),
                Align2::LEFT_BOTTOM,
                &n.name,
                FontId::proportional(11.0),
                t.text_3,
            );
        }
    }

    /// 轮廓模式(线框,`Mod+Y`):不画填充,只勾勒每个对象的绝对边界。
    ///
    /// AI 的轮廓模式用于查看结构关系与重叠顺序;这里是等价的最小实现
    /// (路径级轮廓待 P4 钢笔/矢量落地后替换)。
    fn draw_outline_mode(&self, painter: &egui::Painter, viewport: Rect) {
        let t = Tokens::get(self.theme_dark);
        let origin = viewport.min.to_vec2();
        let ab_stroke = Stroke::new(1.0, t.border_strong);
        let node_stroke = Stroke::new(1.0, t.border);
        let mut ids = Vec::new();
        for &ab in &self.doc.artboards {
            // 画板本身
            if let Some(an) = self.doc.nodes.get(ab) {
                let (x0, y0) = self.camera.world_to_screen(an.geom.x, an.geom.y);
                let (x1, y1) = self
                    .camera
                    .world_to_screen(an.geom.x + an.geom.w, an.geom.y + an.geom.h);
                painter.rect_stroke(
                    Rect::from_min_max(
                        pos2(x0 as f32 + origin.x, y0 as f32 + origin.y),
                        pos2(x1 as f32 + origin.x, y1 as f32 + origin.y),
                    ),
                    0.0,
                    ab_stroke,
                    egui::StrokeKind::Middle,
                );
            }
            self.doc.subtree(ab, &mut ids);
        }
        for id in ids {
            let Some(n) = self.doc.nodes.get(id) else {
                continue;
            };
            if n.hidden || n.tag == "#text" || matches!(n.kind, NodeKind::Artboard) {
                continue;
            }
            let Some(bb) = vb_tools::abs_bbox_world(&self.doc, id) else {
                continue;
            };
            let (sx, sy) = self.camera.world_to_screen(bb.x0, bb.y0);
            let (ex, ey) = self.camera.world_to_screen(bb.x1, bb.y1);
            let r = Rect::from_min_max(
                pos2(sx as f32 + origin.x, sy as f32 + origin.y),
                pos2(ex as f32 + origin.x, ey as f32 + origin.y),
            );
            if !r.intersects(viewport) {
                continue;
            }
            painter.rect_stroke(r, 0.0, node_stroke, egui::StrokeKind::Middle);
        }
    }

    fn draw_overlays(&self, painter: &egui::Painter, viewport: Rect) {
        let t = Tokens::get(self.theme_dark);
        let origin = viewport.min.to_vec2();

        // P4.2 标尺参考线(青色;从选区生成/从标尺拖出,世界坐标)
        if self.guides_visible {
            for &(h, pos) in &self.guides {
                let seg = if h {
                    let (_, sy) = self.camera.world_to_screen(0.0, pos);
                    [
                        pos2(viewport.left(), sy as f32 + origin.y),
                        pos2(viewport.right(), sy as f32 + origin.y),
                    ]
                } else {
                    let (sx, _) = self.camera.world_to_screen(pos, 0.0);
                    [
                        pos2(sx as f32 + origin.x, viewport.top()),
                        pos2(sx as f32 + origin.x, viewport.bottom()),
                    ]
                };
                painter.line_segment(seg, Stroke::new(1.0, semantic::GUIDE_RULER));
            }
        }

        // P4.5 钢笔预览:锚点连线 + 橡皮筋 + 平滑手柄(出柄实线/入柄镜像虚线)
        if self.tool == Tool::Pen && !self.pen_points.is_empty() {
            let to_screen = |(x, y): (f64, f64)| {
                let (sx, sy) = self.camera.world_to_screen(x, y);
                pos2(sx as f32 + origin.x, sy as f32 + origin.y)
            };
            let stroke = Stroke::new(1.2, semantic::SELECT_BOX);
            let mut prev = to_screen(self.pen_points[0].anchor);
            for pt in self.pen_points.iter().skip(1) {
                let cur = to_screen(pt.anchor);
                painter.line_segment([prev, cur], stroke);
                prev = cur;
            }
            // 橡皮筋:最后锚点 → 光标
            let last = self.pen_points.last().unwrap();
            let cursor = to_screen(self.cursor_world);
            painter.line_segment(
                [to_screen(last.anchor), cursor],
                Stroke::new(0.8, semantic::HOVER_BOX),
            );
            // 锚点方块 + 手柄
            for pt in &self.pen_points {
                let a = to_screen(pt.anchor);
                painter.rect_filled(
                    Rect::from_center_size(a, vec2(6.0, 6.0)),
                    1.0,
                    semantic::SELECT_BOX,
                );
                if let Some((hx, hy)) = pt.h_out {
                    let h = to_screen((hx, hy));
                    painter.line_segment([a, h], Stroke::new(0.8, semantic::GUIDE_SMART_DARK));
                    painter.circle_filled(h, 2.5, semantic::GUIDE_SMART_DARK);
                    if let Some((ix, iy)) = pt.h_in() {
                        let i2 = to_screen((ix, iy));
                        painter.line_segment([a, i2], Stroke::new(0.8, semantic::GUIDE_SMART_DARK));
                        painter.circle_filled(i2, 2.5, semantic::GUIDE_SMART_DARK);
                    }
                }
            }
            // 靠近起点 ≥3 锚点:高亮提示可闭合
            if self.pen_points.len() >= 3 {
                let (x0, y0) = self.pen_points[0].anchor;
                if (self.cursor_world.0 - x0).hypot(self.cursor_world.1 - y0)
                    <= 8.0 / self.camera.zoom
                {
                    painter.circle_filled(to_screen((x0, y0)), 5.0, semantic::SELECT_BOX);
                }
            }
        }

        // 智能参考线(品红,与 AI 同色)
        for l in &self.smart_guides {
            let (x0, y0) = self.camera.world_to_screen(l[0], l[1]);
            let (x1, y1) = self.camera.world_to_screen(l[2], l[3]);
            painter.line_segment(
                [
                    pos2(x0 as f32 + origin.x, y0 as f32 + origin.y),
                    pos2(x1 as f32 + origin.x, y1 as f32 + origin.y),
                ],
                // 智能参考线品红是**语义色**(AI 品红,02/03 篇钉死),深/浅主题共用同一个值;
                // vb-token-ok:不参与令牌化,P2 只把它挪进 vb_ui::theme 的 const
                Stroke::new(1.0, Color32::from_rgb(0xff, 0x00, 0xff)),
            );
        }
        if std::env::var("VB_NO_OVERLAY").is_ok() {
            return;
        }
        // 数值浮层(P3.7,14 篇 §4.4):移动 / 缩放 / 旋转时跟随光标显示实时数值
        let drag_label: Option<String> = match &self.drag {
            Drag::MoveObj {
                sid, start_geom, ..
            } => {
                let n = self
                    .doc
                    .find_by_sid(sid)
                    .and_then(|id| self.doc.nodes.get(id));
                n.map(|n| {
                    format!(
                        "X {}\nY {}\nΔX +{}\nΔY +{}",
                        vb_common::units::fmt_num(n.geom.x),
                        vb_common::units::fmt_num(n.geom.y),
                        vb_common::units::fmt_num(n.geom.x - start_geom.x),
                        vb_common::units::fmt_num(n.geom.y - start_geom.y),
                    )
                })
            }
            Drag::Resize { sid, .. } => self.doc.find_by_sid(sid).and_then(|id| {
                self.doc.nodes.get(id).map(|n| {
                    format!(
                        "W {}\nH {}",
                        vb_common::units::fmt_num(n.geom.w),
                        vb_common::units::fmt_num(n.geom.h)
                    )
                })
            }),
            Drag::Rotate { sid, .. } => self.doc.find_by_sid(sid).and_then(|id| {
                self.doc
                    .nodes
                    .get(id)
                    .and_then(|n| n.style_get("transform"))
                    .and_then(vb_render::encode::parse_rotate_deg)
                    .map(|d| format!("旋转 {}°", vb_common::units::fmt_num(d)))
            }),
            // 创建/缩放区域:拖拽中实时显示目标尺寸(P3.7)
            Drag::Create { start, cur } | Drag::ZoomRegion { start, cur } => {
                let w = ((cur.x - start.x).abs() as f64 / self.camera.zoom).round();
                let h = ((cur.y - start.y).abs() as f64 / self.camera.zoom).round();
                Some(format!("{} × {}", w, h))
            }
            _ => None,
        };
        if let Some(label) = drag_label {
            let (cx, cy) = self.cursor_world;
            let (sx, sy) = self.camera.world_to_screen(cx, cy);
            let pos = pos2(sx as f32 + origin.x + 16.0, sy as f32 + origin.y + 16.0);
            let bg = t.bg_raised;
            let fg = t.text;
            painter.rect_filled(
                Rect::from_min_size(
                    pos,
                    egui::vec2(96.0, 16.0 * label.lines().count() as f32 + 10.0),
                ),
                4.0,
                bg,
            );
            painter.text(
                pos2(pos.x + 8.0, pos.y + 5.0),
                Align2::LEFT_TOP,
                label,
                FontId::monospace(11.0),
                fg,
            );
        }
        // 文本近似绘制 + 冻结块占位(ADR-0017)
        let mut ids = Vec::new();
        for &ab in &self.doc.artboards {
            self.doc.subtree(ab, &mut ids);
        }
        for id in ids {
            let Some(n) = self.doc.nodes.get(id) else {
                continue;
            };
            if n.hidden || n.tag == "#text" {
                continue;
            }
            let abs = vb_tools::abs_bbox_world(&self.doc, id);
            let Some(bb) = abs else { continue };
            let (sx, sy) = self.camera.world_to_screen(bb.x0, bb.y0);
            let (ex, ey) = self.camera.world_to_screen(bb.x1, bb.y1);
            let r = Rect::from_min_max(
                pos2(sx as f32 + origin.x, sy as f32 + origin.y),
                pos2(ex as f32 + origin.x, ey as f32 + origin.y),
            );
            if !r.intersects(viewport) {
                continue;
            }
            match &n.kind {
                NodeKind::Text { text, mode, .. } => {
                    let fs = n
                        .style_get("font-size")
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(16.0);
                    let color = n
                        .style_get("color")
                        .and_then(vb_common::color::parse_color)
                        .map(|c| Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a))
                        // HTML 规范的默认文字色(文档内容,非 UI 皮肤)
                        .unwrap_or(Color32::from_rgb(0x20, 0x20, 0x20)); // vb-token-ok: 文档内容默认色,非 UI 皮肤
                    let px_size = (fs * self.camera.zoom) as f32;
                    painter.text(
                        pos2(r.left() + 2.0, r.top() + 2.0),
                        Align2::LEFT_TOP,
                        text,
                        FontId::proportional(px_size.max(6.0)),
                        color,
                    );
                    // 04-2(design/03 §六):区域文本溢出 → 右下角红点。
                    // 溢出判定用导出同款量测(真字形引擎),与画布近似无关。
                    if *mode == vb_doc::model::TextMode::Area {
                        let overflow =
                            crate::app::panels::charpara::area_overflow_px(&self.doc, id);
                        if overflow > 0.5 {
                            painter.circle_filled(
                                pos2(r.right() - 5.0, r.bottom() - 5.0),
                                4.0,
                                semantic::overflow_dot(self.theme_dark),
                            );
                        }
                    }
                }
                NodeKind::Frozen { .. } => {
                    painter.rect_filled(r, 4.0, semantic::frozen_fill(self.theme_dark));
                    painter.text(
                        pos2(r.left() + 6.0, r.top() + 4.0),
                        Align2::LEFT_TOP,
                        "冻结块",
                        FontId::proportional(11.0),
                        t.text_3,
                    );
                }
                _ => {}
            }
        }

        // 选中框 + 手柄
        for sid in &self.selection {
            let Some(nid) = self.doc.find_by_sid(sid) else {
                continue;
            };
            let Some(bb) = vb_tools::abs_bbox_world(&self.doc, nid) else {
                continue;
            };
            let (sx, sy) = self.camera.world_to_screen(bb.x0, bb.y0);
            let (ex, ey) = self.camera.world_to_screen(bb.x1, bb.y1);
            let r = Rect::from_min_max(
                pos2(sx as f32 + origin.x - 0.5, sy as f32 + origin.y - 0.5),
                pos2(ex as f32 + origin.x + 0.5, ey as f32 + origin.y + 0.5),
            );
            // 「对齐到:关键对象」= 最后选中者 → 它的选中框**加粗**(03-5-3),
            // 让用户一眼看出对齐基准(其余选中框保持 1px)
            let is_key_object = self.align_to == super::align_panel::AlignTo::KeyObject
                && self.selection.last().map(|s| s == sid).unwrap_or(false);
            let width = if is_key_object { 2.5 } else { 1.0 };
            painter.rect_stroke(
                r,
                0.0,
                Stroke::new(width, semantic::SELECT_BOX),
                egui::StrokeKind::Outside,
            );
            for hx in [r.left(), r.center().x, r.right()] {
                for hy in [r.top(), r.center().y, r.bottom()] {
                    if (hx == r.center().x) == (hy == r.center().y) {
                        painter.rect_filled(
                            Rect::from_center_size(pos2(hx, hy), vec2(6.0, 6.0)),
                            1.0,
                            semantic::SELECT_BOX,
                        );
                    }
                }
            }
        }

        // 直接选择(A):矢量锚点方块(拖拽中的锚点高亮)
        if self.tool == Tool::DirectSelect {
            if let Some(sid) = self.selection.last() {
                for (i, vx, vy) in self.vector_vertices(sid) {
                    let (sx, sy) = self.camera.world_to_screen(vx, vy);
                    let c = pos2(sx as f32 + origin.x, sy as f32 + origin.y);
                    let active = self
                        .ds_vertex
                        .as_ref()
                        .map(|(s, vi)| s == sid && *vi == i)
                        .unwrap_or(false);
                    let fill = if active {
                        semantic::SELECT_BOX
                    } else {
                        t.bg_panel
                    };
                    painter.rect_filled(Rect::from_center_size(c, vec2(7.0, 7.0)), 1.0, fill);
                    painter.rect_stroke(
                        Rect::from_center_size(c, vec2(7.0, 7.0)),
                        1.0,
                        Stroke::new(1.0, semantic::SELECT_BOX),
                        egui::StrokeKind::Outside,
                    );
                }
            }
        }

        // 框选/创建预览
        match &self.drag {
            Drag::Marquee { start, cur } => {
                // start/cur 是画布本地坐标,painter 是屏幕绝对:补 origin
                // 与真实相交判定同帧(G5,此前预览比实际选区高一个菜单栏)
                let r = Rect::from_two_pos(
                    pos2(start.x + origin.x, start.y + origin.y),
                    pos2(cur.x + origin.x, cur.y + origin.y),
                );
                painter.rect_filled(r, 0.0, semantic::MARQUEE_FILL);
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, semantic::SELECT_BOX),
                    egui::StrokeKind::Middle,
                );
            }
            Drag::Create { start, cur } => {
                let r = Rect::from_two_pos(
                    pos2(start.x + origin.x, start.y + origin.y),
                    pos2(cur.x + origin.x, cur.y + origin.y),
                );
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, t.border_strong),
                    egui::StrokeKind::Middle,
                );
            }
            Drag::GradientAnnotate { start, end, angle } => {
                let (sx, sy) = self.camera.world_to_screen(start.0, start.1);
                let (cx, cy) = self.camera.world_to_screen(end.0, end.1);
                painter.line_segment(
                    [
                        pos2(sx as f32 + origin.x, sy as f32 + origin.y),
                        pos2(cx as f32 + origin.x, cy as f32 + origin.y),
                    ],
                    Stroke::new(1.5, semantic::SELECT_BOX),
                );
                painter.text(
                    pos2(cx as f32 + origin.x + 8.0, cy as f32 + origin.y),
                    Align2::LEFT_CENTER,
                    format!("{angle:.0}°"),
                    FontId::monospace(11.0),
                    semantic::SELECT_BOX,
                );
            }
            _ => {}
        }

        // 渐变批注保留(05-2-3):渐变工具态下显示方向线 + 色标手柄,
        // 双击手柄即可改色(命中判定在 `canvas_input` / `grad_annot_double_click`)。
        if self.tool == crate::app::Tool::Gradient && matches!(self.drag, Drag::None) {
            if let Some((a, b, g)) = self.grad_annot_screen() {
                let p0 = pos2(a.0 + origin.x, a.1 + origin.y);
                let p1 = pos2(b.0 + origin.x, b.1 + origin.y);
                painter.line_segment([p0, p1], Stroke::new(1.5, semantic::SELECT_BOX));
                let tokens = self.doc.tokens.clone();
                for (i, x, y) in vb_ui::gradient::annot_points(a, b, &g) {
                    let fill = vb_ui::gradient::stop_color(&g.stops[i], &tokens)
                        .unwrap_or(semantic::SELECT_BOX);
                    let c = pos2(x + origin.x, y + origin.y);
                    painter.circle_filled(c, 5.0, fill);
                    painter.circle_stroke(c, 5.0, Stroke::new(1.5, semantic::SELECT_BOX));
                }
            }
        }

        // P4.2 标尺(上/左,20px;刻度与网格同一世界节奏,最后画盖在内容上)
        if self.rulers_on {
            const STRIP: f32 = 20.0;
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(viewport.left(), viewport.top()),
                    pos2(viewport.right(), viewport.top() + STRIP),
                ),
                0.0,
                t.bg_panel,
            );
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(viewport.left(), viewport.top()),
                    pos2(viewport.left() + STRIP, viewport.bottom()),
                ),
                0.0,
                t.bg_panel,
            );
            let mut level = 64.0f64;
            while level * self.camera.zoom < 12.0 {
                level *= 4.0;
            }
            let label_step = level * 2.0;
            let tick = Stroke::new(1.0, t.border);
            let wx_left = (0.0 - self.camera.pan_x) / self.camera.zoom;
            let wx_right = (viewport.width() as f64 - self.camera.pan_x) / self.camera.zoom;
            let k0 = (wx_left / level).floor() as i64;
            let k1 = (wx_right / level).ceil() as i64;
            for k in k0..=k1 {
                let wx = k as f64 * level;
                let sx = (wx * self.camera.zoom + self.camera.pan_x) as f32 + origin.x;
                let is_label = (wx / label_step).fract().abs() < 1e-6;
                let len = if is_label { 10.0 } else { 5.0 };
                painter.line_segment(
                    [
                        pos2(sx, viewport.top() + STRIP - len),
                        pos2(sx, viewport.top() + STRIP),
                    ],
                    tick,
                );
                if is_label {
                    painter.text(
                        pos2(sx + 2.0, viewport.top()),
                        Align2::LEFT_TOP,
                        format!("{}", wx as i64),
                        FontId::monospace(9.0),
                        t.text_2,
                    );
                }
            }
            let wy_top = (0.0 - self.camera.pan_y) / self.camera.zoom;
            let wy_bottom = (viewport.height() as f64 - self.camera.pan_y) / self.camera.zoom;
            let j0 = (wy_top / level).floor() as i64;
            let j1 = (wy_bottom / level).ceil() as i64;
            for j in j0..=j1 {
                let wy = j as f64 * level;
                let sy = (wy * self.camera.zoom + self.camera.pan_y) as f32 + origin.y;
                let is_label = (wy / label_step).fract().abs() < 1e-6;
                let len = if is_label { 10.0 } else { 5.0 };
                painter.line_segment(
                    [
                        pos2(viewport.left() + STRIP - len, sy),
                        pos2(viewport.left() + STRIP, sy),
                    ],
                    tick,
                );
                if is_label {
                    painter.text(
                        pos2(viewport.left() + 1.0, sy + 1.0),
                        Align2::LEFT_TOP,
                        format!("{}", wy as i64),
                        FontId::monospace(9.0),
                        t.text_2,
                    );
                }
            }
        }
    }
}

fn draw_grid(painter: &egui::Painter, rect: Rect, cam: &Camera, dark: bool) {
    // 世界锚定:网格线 = 世界 k*level,屏幕位 = k*level*zoom + pan。
    // 此前按屏幕取整导致平移时网格纹丝不动、缩放时相对世界跳动。
    let mut level = 64.0f64;
    while level * cam.zoom < 16.0 {
        level *= 4.0;
    }
    let step = (level * cam.zoom) as f32;
    if step < 6.0 {
        return;
    }
    let color = semantic::guide_grid(dark);
    // painter 是屏幕绝对坐标:内容映射 = rect.min + pan + w*zoom,
    // 网格必须带 rect.min,否则整体错位一个菜单栏高度(G4)
    let (ox, oy) = (rect.min.x as f64, rect.min.y as f64);
    let k0 = ((rect.left() as f64 - ox - cam.pan_x) / (level * cam.zoom)).floor() as i64;
    let k1 = ((rect.right() as f64 - ox - cam.pan_x) / (level * cam.zoom)).ceil() as i64;
    for k in k0..=k1 {
        let x = (k as f64 * level * cam.zoom + cam.pan_x + ox) as f32;
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(0.5, color),
        );
    }
    let j0 = ((rect.top() as f64 - oy - cam.pan_y) / (level * cam.zoom)).floor() as i64;
    let j1 = ((rect.bottom() as f64 - oy - cam.pan_y) / (level * cam.zoom)).ceil() as i64;
    for j in j0..=j1 {
        let y = (j as f64 * level * cam.zoom + cam.pan_y + oy) as f32;
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(0.5, color),
        );
    }
}
