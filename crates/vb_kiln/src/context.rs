//! ExportContext:导出上下文 = DrawList + 光栅帧 + 动画参数 + 元数据。
//!
//! 单一构建入口 `build`:尺寸守门(防 OOM)、scale 钳制、动画参数校验、
//! 透明度语义归一;所有 writer 只读 ctx,不重复做守门。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use vb_doc::model::{Document, NodeId};
use vb_render::encode::{attach_images, encode_artboard_opts, DrawList};

use crate::error::{KilnError, KilnResult, KilnWarning};
use crate::{ExportRequest, Format, MAX_CANVAS_EDGE, MAX_CANVAS_PIXELS};

/// 一帧:RGBA8 像素(动画格式多帧;静态格式恒 1 帧)。
#[derive(Debug, Clone)]
pub struct Frame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// 帧延迟(毫秒)。
    pub delay_ms: u32,
}

#[derive(Debug, Clone)]
pub struct ExportContext {
    pub artboard_name: String,
    pub doc_title: String,
    /// 逻辑画布尺寸(px)。
    pub logical_w: f64,
    pub logical_h: f64,
    /// 输出像素尺寸。
    pub out_w: u32,
    pub out_h: u32,
    /// 实际生效倍率(钳制后)。
    pub scale: u32,
    /// 透明底(writer 决定是否可用;JPG 等强制 false 并告警)。
    pub transparent: bool,
    /// 用户原始透明请求(JPG 告警判定用;归一化前的值)。
    pub requested_transparent: bool,
    /// 源 DrawList(矢量 writer 用)。
    pub list: DrawList,
    /// 光栅帧序列。
    pub frames: Vec<Frame>,
    pub fps: u32,
    pub duration_s: f32,
    pub gif_loops: u16,
    pub mp4_bitrate_kbps: u32,
    pub jpeg_quality: u8,
    /// 构建期收集的告警(scale 钳制等)。
    pub build_warnings: Vec<KilnWarning>,
    pub project_dir: Option<PathBuf>,
}

impl ExportContext {
    pub fn build(
        doc: &Document,
        artboard: NodeId,
        req: &ExportRequest,
        project_dir: Option<&Path>,
    ) -> KilnResult<ExportContext> {
        let ab = doc
            .nodes
            .get(artboard)
            .ok_or_else(|| KilnError::Encode("画板节点不存在".into()))?;
        let artboard_name = ab.name.clone();
        let doc_title = doc.meta.title.clone();

        let mut build_warnings = Vec::new();
        let scale = if req.scale > 8 {
            build_warnings.push(KilnWarning::ScaleClamped(req.scale));
            8
        } else {
            req.scale.max(1)
        };
        let logical_w = ab.geom.w;
        let logical_h = ab.geom.h;

        let out_w = ((logical_w * scale as f64).round() as u32).max(1);
        let out_h = ((logical_h * scale as f64).round() as u32).max(1);

        // 尺寸守门:异常输入不 OOM
        if out_w > MAX_CANVAS_EDGE || out_h > MAX_CANVAS_EDGE {
            return Err(KilnError::CanvasTooLarge {
                w: out_w,
                h: out_h,
                max: MAX_CANVAS_EDGE,
            });
        }
        let pixels = out_w as u64 * out_h as u64;
        if pixels > MAX_CANVAS_PIXELS {
            return Err(KilnError::CanvasAreaTooLarge {
                w: out_w,
                h: out_h,
                pixels,
                max: MAX_CANVAS_PIXELS,
            });
        }

        // 动画参数守门
        let fps = req.fps.clamp(1, 60);
        if !(0.04..=3600.0).contains(&req.duration_s) {
            return Err(KilnError::BadAnimation(format!(
                "duration_s = {}",
                req.duration_s
            )));
        }

        // DrawList 编码(矢量源)
        let mut list = encode_artboard_opts(doc, artboard, req.transparent)
            .map_err(|e| KilnError::Encode(e.to_string()))?;
        if let Some(dir) = project_dir {
            // 先挂载再查缺:missing 检查若在 attach 之前,每个图片项都会
            // 恒报 ImageMissing(此前无真实图片路径的用例掩盖了这一点)
            let mut loader = |src: &str| load_bitmap(dir, src);
            attach_images(&mut list, &mut loader);
            let missing: Vec<String> = list
                .items
                .iter()
                .filter(|it| it.kind == vb_render::encode::DrawKind::Image && it.image.is_none())
                .filter_map(|it| it.src.clone())
                .collect();
            for src in missing {
                build_warnings.push(KilnWarning::ImageMissing { src });
            }
        }

        // 动画解析(M2):raw_css @keyframes + 节点 animation 声明
        let keyframes = crate::anim::parse_keyframes(&doc.raw_css);
        let mut anims: HashMap<String, crate::anim::NodeAnim> = HashMap::new();
        if !keyframes.is_empty() {
            let mut stack = vec![artboard];
            while let Some(nid) = stack.pop() {
                if let Some(node) = doc.node(nid) {
                    if let Some(na) = crate::anim::resolve_node_anim(doc, nid, &keyframes) {
                        anims.insert(node.sid.as_str().to_string(), na);
                    }
                    stack.extend(node.children.iter().copied());
                }
            }
        }

        // 光栅帧:动画格式且存在动画实例 → 逐帧求值;否则单帧
        let animated = matches!(req.format, Format::Gif | Format::Mp4) && !anims.is_empty();
        let mut frames = Vec::new();
        if animated {
            let n = (fps as f32 * req.duration_s).ceil().max(1.0) as usize;
            for i in 0..n {
                let t = i as f64 / fps as f64;
                let mut f_list = list.clone();
                for item in &mut f_list.items {
                    if let Some(na) = anims.get(&item.sid) {
                        let st = crate::anim::eval_node(na, t);
                        if let Some(o) = st.opacity {
                            item.opacity *= o as f32;
                        }
                        if let Some(tr) = &st.transform {
                            crate::anim::apply_transform(item, tr);
                        }
                        let [_, _, iw, ih] = item.rect;
                        if let Some(cp) = &st.clip_path {
                            item.clip = vb_render::encode::parse_clip_path(cp, iw, ih);
                        }
                        if let Some(fl) = &st.filter {
                            item.filter = vb_render::encode::parse_filter(fl);
                        }
                    }
                }
                let rgba = crate::raster::rasterize_rgba(&f_list, scale as f64)?;
                frames.push(Frame {
                    rgba,
                    width: out_w,
                    height: out_h,
                    delay_ms: (1000.0 / fps as f32).round() as u32,
                });
            }
        } else {
            let rgba = crate::raster::rasterize_rgba(&list, scale as f64)?;
            frames.push(Frame {
                rgba,
                width: out_w,
                height: out_h,
                delay_ms: 100,
            });
        }

        // 透明度语义:JPG/EPS/Ai/PPTX 不支持透明
        let transparent = req.transparent
            && matches!(
                req.format,
                Format::Png | Format::Gif | Format::Svg | Format::Pdf
            );

        Ok(ExportContext {
            artboard_name,
            doc_title,
            logical_w,
            logical_h,
            out_w,
            out_h,
            scale,
            transparent,
            requested_transparent: req.transparent,
            list,
            frames,
            fps,
            duration_s: req.duration_s,
            gif_loops: req.gif_loops,
            mp4_bitrate_kbps: req.mp4_bitrate_kbps,
            jpeg_quality: req.jpeg_quality.clamp(1, 100),
            build_warnings,
            project_dir: project_dir.map(|p| p.to_path_buf()),
        })
    }

    /// 静态画布判定(动画格式告警用)。
    pub fn is_static(&self) -> bool {
        self.frames.len() <= 1
    }
}

/// 项目目录下解析位图 → RGBA8(缺失返回 None,writer 画占位兜底)。
///
/// 采集侧拿到的是 URL 路径:带 `%E8%B5%84...` 百分号转义与 `?query`/`#hash`
/// 后缀。此前直接 join 落到文件系统,中文名资产一律「缺失」丢图(实测
/// A4 海报 img/资源 1.png)。
pub fn load_bitmap(dir: &Path, src: &str) -> Option<vb_render::encode::BitmapData> {
    let cleaned = src.split(['?', '#']).next().unwrap_or(src);
    let decoded = percent_decode(cleaned);
    let mut candidates = vec![dir.join(&decoded)];
    if decoded != cleaned {
        candidates.push(dir.join(cleaned));
    }
    for path in candidates {
        let Ok(img) = image::open(&path) else {
            continue;
        };
        let rgba = img.to_rgba8();
        return Some(vb_render::encode::BitmapData {
            width: rgba.width(),
            height: rgba.height(),
            rgba: std::sync::Arc::new(rgba.into_raw()),
        });
    }
    None
}

/// URL 百分号转义 → 原始字节 → UTF-8 路径(仅处理合法序列,其余原样保留)。
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    let hex = |b: u8| -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    };
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}
