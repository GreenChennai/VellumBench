//! dompaint(ADR-0021):PaintList JSON(浏览器 DOM 快照)→ vb_doc::Document。
//!
//! 每个绘制项一个节点;文本按「行」建 Point 文本节点(整行字符串 →
//! 写入器一行一个 CID Tj,根除逐字断字);双图层用 Layer 节点组织。

use std::path::{Path, PathBuf};

use serde_json::Value;
use vb_doc::model::{Document, Geom, Node, NodeId, NodeKind, SegStyle, TextMode, TextSeg};

/// 采集侧元数据(结构门禁 A2/A3 对拍用)。
#[derive(Debug, Default, Clone)]
pub struct DomPaintMeta {
    pub line_count: u32,
    pub clip_demand: u32,
    pub raster_count: u32,
    pub warnings: Vec<String>,
}

pub struct DomPaint {
    pub doc: Document,
    pub artboard: NodeId,
    pub meta: DomPaintMeta,
    /// 副产物目录(系统临时目录;导出结束由调用方清理;None = 本轮无副产物)。
    pub raster_dir: Option<PathBuf>,
}

fn color_css(c: &[f64]) -> String {
    format!(
        "rgba({}, {}, {}, {})",
        (c[0] * 255.0).round() as u32,
        (c[1] * 255.0).round() as u32,
        (c[2] * 255.0).round() as u32,
        c[3]
    )
}

fn json_f64s(v: Option<&Value>) -> Option<Vec<f64>> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
}

fn rect_of(v: &Value) -> [f64; 4] {
    let a = v.as_array().cloned().unwrap_or_default();
    [
        a.first().and_then(Value::as_f64).unwrap_or(0.0),
        a.get(1).and_then(Value::as_f64).unwrap_or(0.0),
        a.get(2).and_then(Value::as_f64).unwrap_or(0.0),
        a.get(3).and_then(Value::as_f64).unwrap_or(0.0),
    ]
}

fn attach(doc: &mut Document, parent: NodeId, child: NodeId) {
    doc.nodes.get_mut(parent).unwrap().children.push(child);
    doc.nodes.get_mut(child).unwrap().parent = Some(parent);
}

fn base_node(doc: &mut Document, name: &str, kind: NodeKind, rect: [f64; 4]) -> NodeId {
    let sid = doc.alloc_sid();
    let mut n = Node::new(kind, name, sid);
    n.geom = Geom {
        x: rect[0],
        y: rect[1],
        w: rect[2].max(1.0),
        h: rect[3].max(1.0),
    };
    n.authored = [true, true, true, true];
    doc.nodes.insert(n)
}

/// 构造 Document。`page_png` 为整页截图(位图降级裁剪源),None 时降级项画占位框。
/// `declared_width`(P0-3):画板声明宽(>0 时为唯一真相)——透明容器
/// (无背景的 `.poster` 等)不产生绘制项,「最大绘制右边界」启发式会取到
/// 内层元素的右缘而低估画板宽(1920 → 1824);尺寸门要求与声明严格相等。
pub fn paintlist_to_document(
    list: &Value,
    project_dir: &Path,
    page_png: Option<&[u8]>,
    url_prefix: &str,
    capture_scale: f64,
    height_lock: f64,
    declared_width: f64,
) -> Result<DomPaint, String> {
    let vp = list
        .get("viewport")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let vw = vp
        .first()
        .and_then(Value::as_f64)
        .unwrap_or(1240.0)
        .max(1.0);
    // 高度锁定(--height,动画卡 100vh 型):画板高取锁定值;否则取内容高
    let h = if height_lock > 0.0 {
        height_lock
    } else {
        vp.get(1).and_then(Value::as_f64).unwrap_or(1754.0).max(1.0)
    };
    // 画板宽 = 全部绘制项的最大右边界(含背景层:`.page`/BODY 这类满页
    // 容器正是画板真实宽度;按视口宽拉伸的封面盒会得到与视口同值的结果,
    // 落到下面的 `else { vw }` 分支,无副作用)
    let items = list
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let content_right = items
        .iter()
        .filter_map(|it| it.get("rect").and_then(Value::as_array))
        .filter_map(|r| {
            let a: Vec<f64> = r.iter().filter_map(Value::as_f64).collect();
            (a.len() == 4).then(|| a[0] + a[2])
        })
        .fold(0.0f64, f64::max);
    // 优先级:声明宽(P0-3 尺寸门)> body 实宽(非视口拉伸)> 内容层右边界 > 视口宽
    let body_w = list.get("bodyWidth").and_then(Value::as_f64).unwrap_or(0.0);
    let w = if declared_width > 0.0 {
        declared_width
    } else if body_w > 200.0 && body_w < vw - 0.5 {
        body_w
    } else if (200.0..=vw).contains(&content_right) && content_right < vw - 0.5 {
        content_right.round()
    } else {
        vw
    };
    let mut doc = Document::new_empty("Kiln DOM snapshot", "zh-CN");
    let ab = doc.new_artboard("画板 1", w, h);
    // 真实两层容器：避免 Illustrator 把每个 PDF 对象提升为剪切组。
    let bg_layer = base_node(&mut doc, "背景", NodeKind::Layer, [0.0, 0.0, w, h]);
    let content_layer = base_node(&mut doc, "内容", NodeKind::Layer, [0.0, 0.0, w, h]);
    attach(&mut doc, ab, bg_layer);
    attach(&mut doc, ab, content_layer);
    let mut meta = DomPaintMeta {
        line_count: list
            .get("textLineCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32,
        clip_demand: list.get("clipDemand").and_then(Value::as_u64).unwrap_or(0) as u32,
        ..Default::default()
    };
    // 图层容器固定为背景→内容；同层内仍保持 DOM 绘制顺序。

    // 副产物目录(K4 修复):系统临时目录按进程+时间戳隔离,绝不写源工程;
    // 懒创建,无降级项/无 data: 资源时不产生任何目录
    let mut raster_dir: Option<PathBuf> = None;

    let mut raster_idx = 0u32;
    for item in &items {
        let kind = item.get("kind").and_then(Value::as_str).unwrap_or("");
        let rect = item.get("rect").map(rect_of).unwrap_or([0.0, 0.0, w, h]);
        // 盒/图按采集分类;文本恒内容层(用户语义:背景一层、其余一层)
        let layer = match kind {
            "text" => 1u32,
            _ => item.get("layer").and_then(Value::as_u64).unwrap_or(1) as u32,
        };
        let opacity = item.get("opacity").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        match kind {
            "box" => {
                let id = base_node(&mut doc, "背景盒", NodeKind::Box, rect);
                if let Some(bg) = json_f64s(item.get("bg")) {
                    if bg.len() == 4 {
                        doc.nodes
                            .get_mut(id)
                            .unwrap()
                            .style_set("background-color", &color_css(&bg));
                    }
                }
                if let Some(grad) = item.get("gradient").and_then(Value::as_str) {
                    if !grad.is_empty() {
                        doc.nodes.get_mut(id).unwrap().style_set("background", grad);
                    }
                }
                if let Some(bu) = item.get("bgUrl").and_then(Value::as_str) {
                    if !bu.is_empty() {
                        let rel =
                            relativize(bu, url_prefix, &mut raster_dir, &mut raster_idx, None);
                        let n = doc.nodes.get_mut(id).unwrap();
                        n.kind = NodeKind::Image { src: rel };
                    }
                }
                if let Some(rad) = item.get("radii").and_then(Value::as_array) {
                    let r: Vec<f64> = rad.iter().filter_map(Value::as_f64).collect();
                    if r.len() == 4 && r.iter().any(|v| *v > 0.5) {
                        let uniform = r
                            .iter()
                            .cloned()
                            .fold(f64::MAX, f64::min)
                            .max(r.iter().cloned().fold(f64::MIN, f64::max) - 0.75);
                        if uniform > 0.5 {
                            doc.nodes
                                .get_mut(id)
                                .unwrap()
                                .style_set("border-radius", &format!("{uniform:.2}px"));
                        }
                    }
                }
                if let Some(b) = item.get("border").and_then(Value::as_object) {
                    let c = b.get("color").and_then(|v| json_f64s(Some(v)));
                    if let (Some(wv), Some(c)) = (b.get("width").and_then(Value::as_f64), c) {
                        if c.len() == 4 {
                            // S1(21 篇):写 longhand——border 简写的空白切分
                            // 会把 rgba(255, 207, 77, 1) 切碎 → 颜色回退黑
                            let n = doc.nodes.get_mut(id).unwrap();
                            n.style_set("border-width", &format!("{}px", wv));
                            n.style_set("border-style", "solid");
                            n.style_set("border-color", &color_css(&c));
                        }
                    }
                }
                if opacity < 0.999 {
                    doc.nodes
                        .get_mut(id)
                        .unwrap()
                        .style_set("opacity", &format!("{opacity:.3}"));
                }
                // box-shadow → 辉光近似:外扩盒 + 径向衰减渐变(中心高亮区
                // 被盒体自身盖住,露出环形光晕;glow/外阴影均适用)
                // G2(21 篇):辉光一律挂背景层——画在内容层会把邻近文字
                // 蒙出一层「半透明色块」(褪色感);并限幅不越画板
                if let Some(sh) = item.get("shadow").and_then(Value::as_str) {
                    let glows = parse_box_shadows(sh, &rect, radii_of(item));
                    if std::env::var("KILN_DEBUG_GLOW").is_ok() {
                        eprintln!("[glow] sh={sh:?} -> {} 层", glows.len());
                    }
                    for glow in glows {
                        let g_rect = clamp_rect(glow.0, w, h);
                        let sid2 = base_node(&mut doc, "辉光", NodeKind::Box, g_rect);
                        doc.nodes
                            .get_mut(sid2)
                            .unwrap()
                            .style_set("background", &glow.1);
                        attach(&mut doc, bg_layer, sid2);
                    }
                }
                if layer > 0 {
                    doc.nodes.get_mut(id).unwrap().style_set("vb-layer", "1");
                }
                attach(
                    &mut doc,
                    if layer == 0 { bg_layer } else { content_layer },
                    id,
                );
            }
            "image" => {
                let Some(src) = item.get("src").and_then(Value::as_str) else {
                    continue;
                };
                let rel = relativize(src, url_prefix, &mut raster_dir, &mut raster_idx, None);
                let id = base_node(&mut doc, "图片", NodeKind::Image { src: rel }, rect);
                if opacity < 0.999 {
                    doc.nodes
                        .get_mut(id)
                        .unwrap()
                        .style_set("opacity", &format!("{opacity:.3}"));
                }
                if layer > 0 {
                    doc.nodes.get_mut(id).unwrap().style_set("vb-layer", "1");
                }
                attach(
                    &mut doc,
                    if layer == 0 { bg_layer } else { content_layer },
                    id,
                );
            }
            "raster" | "svg" => {
                // S2(21 篇):svg 资产优先 usvg 矢量导入(真矢量、无截图
                // 裁剪的邻内容污染);失败回退整页截图裁剪
                let reason = item.get("reason").and_then(Value::as_str).unwrap_or("");
                let src = item.get("src").and_then(Value::as_str).unwrap_or("");
                if reason == "svg-image"
                    && !src.is_empty()
                    && try_svg_vector(src, url_prefix, project_dir, &rect, &mut doc, content_layer)
                {
                    continue;
                }
                meta.raster_count += 1;
                let id = match page_png {
                    Some(png) => {
                        let name = format!("r{raster_idx}.png");
                        // 目录创建失败时 path 不存在 → crop_png 报错走占位分支
                        let path = ensure_raster_dir(&mut raster_dir).map(|d| d.join(&name));
                        match path.and_then(|p| crop_png(png, rect, &p, capture_scale).map(|_| p)) {
                            Ok(path) => {
                                raster_idx += 1;
                                base_node(
                                    &mut doc,
                                    "位图降级",
                                    NodeKind::Image {
                                        src: path.to_string_lossy().to_string(),
                                    },
                                    rect,
                                )
                            }
                            Err(e) => {
                                meta.warnings.push(format!("位图裁剪失败({e}):跳过一项"));
                                base_node(&mut doc, "占位", NodeKind::Box, rect)
                            }
                        }
                    }
                    None => base_node(&mut doc, "占位", NodeKind::Box, rect),
                };
                if layer > 0 {
                    doc.nodes.get_mut(id).unwrap().style_set("vb-layer", "1");
                }
                attach(
                    &mut doc,
                    if layer == 0 { bg_layer } else { content_layer },
                    id,
                );
            }
            "text" => {
                let Some(runs) = item.get("runs").and_then(Value::as_array) else {
                    continue;
                };
                // 一个绘制项的全部视觉行合并为一个 Text 节点；换行用真实
                // '\n' 保存；样式变化用字节区间保存（T1/T2：行内 segs 优先，
                // 跨 inline 的重点字号/字重/颜色不再被首段吞掉），避免逐字断裂。
                let mut text = String::new();
                let mut segments = Vec::<TextSeg>::new();
                let mut min_x = f64::MAX;
                let mut min_y = f64::MAX;
                let mut max_r: f64 = 0.0;
                let mut max_b: f64 = 0.0;
                let mut base_family = String::from("sans-serif");
                let mut base_size = 16.0;
                let mut base_weight = 400u16;
                let mut base_color = String::from("rgba(0,0,0,1)");
                let mut base_ls = 0.0;
                let mut line_tops: Vec<f64> = Vec::new();
                let mut base_set = false;
                for run in runs.iter() {
                    let rr = run.get("rect").map(rect_of).unwrap_or(rect);
                    // segs 优先：行文本由 segs 拼接（与字节区间严格同源）
                    let segs = run.get("segs").and_then(Value::as_array);
                    let run_text = match segs {
                        Some(ss) => ss
                            .iter()
                            .filter_map(|s| s.get("t").and_then(Value::as_str))
                            .collect::<String>(),
                        None => run
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    };
                    if run_text.is_empty() {
                        continue;
                    }
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    let line_start = text.len();
                    text.push_str(&run_text);
                    let line_end = text.len();
                    line_tops.push(rr[1]);
                    min_x = min_x.min(rr[0]);
                    min_y = min_y.min(rr[1]);
                    max_r = max_r.max(rr[0] + rr[2]);
                    max_b = max_b.max(rr[1] + rr[3]);
                    let run_family = run
                        .get("family")
                        .and_then(Value::as_str)
                        .unwrap_or("sans-serif");
                    let run_size = run.get("size").and_then(Value::as_f64).unwrap_or(16.0);
                    let run_weight =
                        run.get("weight").and_then(Value::as_u64).unwrap_or(400) as u16;
                    let run_color = run.get("color").and_then(Value::as_array).map(|c| {
                        color_css(&c.iter().filter_map(Value::as_f64).collect::<Vec<_>>())
                    });
                    if !base_set {
                        base_set = true;
                        base_family = run_family.to_string();
                        base_size = run_size;
                        base_weight = run_weight;
                        if let Some(c) = &run_color {
                            base_color = c.clone();
                        }
                        base_ls = run.get("ls").and_then(Value::as_f64).unwrap_or(0.0);
                    }
                    match segs {
                        Some(ss) if !ss.is_empty() => {
                            // 行内分段：字节区间 + 段样式（相对基准的差量）
                            let mut off = line_start;
                            for sg in ss.iter() {
                                let t = sg.get("t").and_then(Value::as_str).unwrap_or("");
                                let start = off;
                                let end = off + t.len();
                                off = end;
                                if t.is_empty() {
                                    continue;
                                }
                                let family = sg
                                    .get("family")
                                    .and_then(Value::as_str)
                                    .unwrap_or(run_family);
                                let size =
                                    sg.get("size").and_then(Value::as_f64).unwrap_or(run_size);
                                let weight = sg
                                    .get("weight")
                                    .and_then(Value::as_u64)
                                    .unwrap_or(run_weight as u64)
                                    as u16;
                                let color = sg
                                    .get("color")
                                    .and_then(Value::as_array)
                                    .map(|c| {
                                        color_css(
                                            &c.iter().filter_map(Value::as_f64).collect::<Vec<_>>(),
                                        )
                                    })
                                    .or_else(|| run_color.clone());
                                let mut style = SegStyle::default();
                                if family != base_family {
                                    style.font_family = Some(family.to_string());
                                }
                                if (size - base_size).abs() > 0.01 {
                                    style.font_size = Some(size);
                                }
                                if weight != base_weight {
                                    style.bold = Some(weight >= 600);
                                }
                                if let Some(c) = &color {
                                    if *c != base_color {
                                        style.color = Some(c.clone());
                                    }
                                }
                                if style != SegStyle::default() {
                                    segments.push(TextSeg { start, end, style });
                                }
                            }
                        }
                        _ => {
                            // 兼容旧采集（无 segs）：run 级样式差
                            let start = line_start;
                            let end = line_end;
                            let mut style = SegStyle::default();
                            if run_family != base_family {
                                style.font_family = Some(run_family.to_string());
                            }
                            if (run_size - base_size).abs() > 0.01 {
                                style.font_size = Some(run_size);
                            }
                            if run_weight != base_weight {
                                style.bold = Some(run_weight >= 600);
                            }
                            if let Some(c) = &run_color {
                                if *c != base_color {
                                    style.color = Some(c.clone());
                                }
                            }
                            if style != SegStyle::default() {
                                segments.push(TextSeg { start, end, style });
                            }
                        }
                    }
                }
                if text.is_empty() {
                    continue;
                }
                let x = if min_x.is_finite() { min_x } else { rect[0] };
                let y = if min_y.is_finite() { min_y } else { rect[1] };
                // 行距 = 相邻视觉行 top 之差(浏览器实测)。此前用「整块高度
                // 当行距」(max_b - y),两行标题的行距直接翻倍 → 第二行被推到
                // 150px 之外(A4 标题实测);单行时退回块高。
                let line_h = if line_tops.len() >= 2 {
                    let d = line_tops[1] - line_tops[0];
                    d.max(base_size * 1.05)
                } else {
                    (max_b - y).max(rect[3]).max(base_size * 1.05)
                };
                // 宽度拉到画板右缘，避免 Rust 与 Blink 的 advance 微差再次断行。
                let n_rect = [x, y, (w - x).max((max_r - x).max(base_size)), line_h];
                let id = base_node(
                    &mut doc,
                    "文本",
                    NodeKind::Text {
                        text,
                        mode: TextMode::Point,
                        segments,
                    },
                    n_rect,
                );
                {
                    let n = doc.nodes.get_mut(id).unwrap();
                    n.style_set("font-family", &format!("\"{base_family}\""));
                    n.style_set("font-size", &format!("{base_size:.2}px"));
                    n.style_set("font-weight", &base_weight.to_string());
                    n.style_set("color", &base_color);
                    n.style_set("line-height", &format!("{line_h:.2}px"));
                    if base_ls > 0.01 {
                        n.style_set("letter-spacing", &format!("{base_ls:.2}px"));
                    }
                    if opacity < 0.999 {
                        n.style_set("opacity", &format!("{opacity:.3}"));
                    }
                }
                doc.nodes.get_mut(id).unwrap().style_set("vb-layer", "1");
                attach(&mut doc, content_layer, id);
            }
            _ => {}
        }
    }
    Ok(DomPaint {
        doc,
        artboard: ab,
        meta,
        raster_dir,
    })
}

/// 副产物目录(懒创建):系统临时目录按「进程号+纳秒」隔离,绝不写源工程
/// (K4 修复:此前 `.kiln-raster/` 落在源 HTML 工程内且全仓无清理逻辑)。
fn ensure_raster_dir(slot: &mut Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(d) = slot {
        return Ok(d.clone());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|t| t.as_nanos())
        .unwrap_or(0);
    let d = std::env::temp_dir().join(format!("kiln-raster-{}-{}", std::process::id(), nanos));
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    *slot = Some(d.clone());
    Ok(d)
}

/// URL → 可加载路径;data: URI 解码落盘到副产物目录(绝对路径,
/// `Path::join` 遇绝对路径整体替换,加载器无需特判)。
fn relativize(
    url: &str,
    prefix: &str,
    raster_dir: &mut Option<PathBuf>,
    idx: &mut u32,
    _page: Option<&[u8]>,
) -> String {
    if let Some(rest) = url.strip_prefix(prefix) {
        return rest.trim_start_matches('/').to_string();
    }
    if let Some(b64) = url.strip_prefix("data:image/") {
        let comma = b64.find(',').unwrap_or(0);
        let (meta, data) = b64.split_at(comma);
        let data = data.trim_start_matches(',');
        let ext = if meta.starts_with("png") {
            "png"
        } else if meta.starts_with("jpeg") || meta.starts_with("jpg") {
            "jpg"
        } else if meta.starts_with("webp") {
            "webp"
        } else {
            "png"
        };
        let name = format!("d{idx}.{ext}");
        *idx += 1;
        if let Ok(bytes) = b64_decode(data) {
            if let Ok(dir) = ensure_raster_dir(raster_dir) {
                let path = dir.join(&name);
                if std::fs::write(&path, bytes).is_ok() {
                    return path.to_string_lossy().to_string();
                }
            }
        }
        return String::new();
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        // 外链:原样返回(attach_images 失败时发 ImageMissing 警告)
        return url.to_string();
    }
    url.to_string()
}

fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        if c.is_ascii_whitespace() || c == b'=' {
            continue;
        }
        let v = TBL.iter().position(|&t| t == c).ok_or("bad b64")? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Ok(out)
}

fn crop_png(png: &[u8], rect: [f64; 4], out: &Path, scale: f64) -> Result<(), String> {
    let img = image::load_from_memory(png).map_err(|e| e.to_string())?;
    let (iw, ih) = (img.width() as f64, img.height() as f64);
    if iw < 1.0 || ih < 1.0 {
        return Err("空图".into());
    }
    // 整页截图的物理像素 = 采集 rect(CSS px) × 采集 DSF
    let s = scale.max(1.0);
    let x0 = (rect[0] * s).floor().max(0.0) as u32;
    let y0 = (rect[1] * s).floor().max(0.0) as u32;
    let x1 = ((rect[0] + rect[2]) * s).ceil().min(iw) as u32;
    let y1 = ((rect[1] + rect[3]) * s).ceil().min(ih) as u32;
    if x1 <= x0 || y1 <= y0 {
        return Err("空裁剪区".into());
    }
    let cropped = img.crop_imm(x0, y0, x1 - x0, y1 - y0);
    cropped.save(out).map_err(|e| e.to_string())
}

fn radii_of(item: &Value) -> f64 {
    item.get("radii")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_f64).fold(0.0f64, f64::max))
        .unwrap_or(0.0)
}

/// 限幅到画板(G2:辉光不得越出画板边界)。
fn clamp_rect(r: [f64; 4], aw: f64, ah: f64) -> [f64; 4] {
    let x0 = r[0].clamp(0.0, aw);
    let y0 = r[1].clamp(0.0, ah);
    let x1 = (r[0] + r[2]).clamp(0.0, aw);
    let y1 = (r[1] + r[3]).clamp(0.0, ah);
    [x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0)]
}

/// S2:`<img src=*.svg>` → usvg 矢量导入,子文档节点带映射移植到内容层。
/// 映射:viewBox 尺寸 → 采集矩形(x/y 平移 + 等比缩放)。成功返回 true
/// (节点已挂载);任何一步失败返回 false 走截图裁剪兜底。
fn try_svg_vector(
    src: &str,
    prefix: &str,
    mount_dir: &Path,
    rect: &[f64; 4],
    doc: &mut Document,
    parent: NodeId,
) -> bool {
    let Some(rest) = src.strip_prefix(prefix) else {
        return false;
    };
    let rel = rest.trim_start_matches('/');
    let path = mount_dir.join(rel);
    if !path.is_file() {
        return false;
    }
    let Ok((sub, warnings)) = crate::import_svg::import_svg_to_doc(&path, None) else {
        return false;
    };
    // v1 边界守卫:自由路径会被包围盒矩形近似(视觉劣化),此类 svg
    // 整体回退截图裁剪;纯图元(矩形/圆/椭圆)svg 才走矢量导入
    if warnings.iter().any(|w| w.contains("包围盒矩形近似")) {
        return false;
    }
    let Some(sub_ab) = sub.artboards.first().copied() else {
        return false;
    };
    let (vb_w, vb_h) = match sub.nodes.get(sub_ab) {
        Some(n) => (n.geom.w.max(1.0), n.geom.h.max(1.0)),
        None => return false,
    };
    let sx = rect[2] / vb_w;
    let sy = rect[3] / vb_h;
    let mut placed = 0usize;
    // 从子画板的孩子逐棵移植(几何:子文档画板内绝对坐标 → 画板绝对坐标)
    let roots: Vec<NodeId> = sub
        .nodes
        .get(sub_ab)
        .map(|n| n.children.clone())
        .unwrap_or_default();
    for c in roots {
        if graft_svg_node(doc, &sub, c, parent, rect[0], rect[1], sx, sy, &mut placed) {
            placed += 1;
        }
    }
    placed > 0
}

/// 递归移植一个 SVG 子树(克隆节点,sid 重新分配,几何做仿射映射)。
#[allow(clippy::too_many_arguments)]
fn graft_svg_node(
    doc: &mut Document,
    sub: &Document,
    nid: NodeId,
    parent: NodeId,
    ox: f64,
    oy: f64,
    sx: f64,
    sy: f64,
    placed: &mut usize,
) -> bool {
    let Some(src) = sub.nodes.get(nid) else {
        return false;
    };
    let mut n = src.clone();
    n.sid = doc.alloc_sid();
    n.geom = Geom {
        x: ox + n.geom.x * sx,
        y: oy + n.geom.y * sy,
        w: n.geom.w * sx,
        h: n.geom.h * sy,
    };
    n.children.clear();
    n.parent = Some(parent);
    let new_id = doc.nodes.insert(n);
    doc.nodes.get_mut(parent).unwrap().children.push(new_id);
    *placed += 1;
    let kids: Vec<NodeId> = sub
        .nodes
        .get(nid)
        .map(|p| p.children.clone())
        .unwrap_or_default();
    for c in kids {
        graft_svg_node(doc, sub, c, new_id, ox, oy, sx, sy, placed);
    }
    true
}

/// computed box-shadow 串 → [(外扩矩形, radial-gradient 背景)] 逐层。
/// 形如 `rgba(0, 255, 209, 0.15) 0px 0px 40px 0px, ...`;inset 跳过。
fn parse_box_shadows(sh: &str, rect: &[f64; 4], radius: f64) -> Vec<([f64; 4], String)> {
    let mut out = Vec::new();
    for layer in split_top_level(sh, ',') {
        let t = layer.trim();
        if t.contains("inset") {
            continue;
        }
        let Some(c) = extract_rgba(t) else {
            if std::env::var("KILN_DEBUG_GLOW").is_ok() {
                eprintln!("[glow] rgba 解析失败: {t:?}");
            }
            continue;
        };
        // 去掉 rgba(...) 后再取 4 个长度(x y blur spread)
        let tail = match t.find("rgba(") {
            Some(start) => {
                let end = t[start..]
                    .find(')')
                    .map(|e| start + e + 1)
                    .unwrap_or(t.len());
                &t[end..]
            }
            None => t,
        };
        let nums: Vec<f64> = tail
            .split_whitespace()
            .filter_map(|tok| tok.trim_end_matches("px").parse::<f64>().ok())
            .collect();
        if std::env::var("KILN_DEBUG_GLOW").is_ok() {
            eprintln!("[glow] tail={tail:?} nums={nums:?}");
        }
        if nums.len() < 3 {
            continue;
        }
        let (dx, dy, blur) = (nums[0], nums[1], nums[2]);
        let spread = nums.get(3).copied().unwrap_or(0.0);
        if blur <= 0.5 {
            continue;
        }
        let grow = blur + spread;
        let g_rect = [
            rect[0] + dx - grow,
            rect[1] + dy - grow,
            rect[2] + grow * 2.0,
            rect[3] + grow * 2.0,
        ];
        // 内边界(盒半径+内缩)→ 渐变保持不透明的百分比:中心实心区被
        // 盒体盖住,外部按半径衰减到 0
        let half_min = g_rect[2].min(g_rect[3]) / 2.0;
        let solid = ((radius.max(4.0) + 2.0) / half_min.max(1.0) * 100.0).min(85.0) as i64;
        let grad = format!(
            "radial-gradient(rgba({}, {}, {}, {:.3}) {}%, rgba({}, {}, {}, 0) 100%)",
            c.0, c.1, c.2, c.3, solid, c.0, c.1, c.2,
        );
        out.push((g_rect, grad));
    }
    out
}

fn extract_rgba(s: &str) -> Option<(u32, u32, u32, f64)> {
    let start = s.find("rgba(")? + 5;
    let end = s[start..].find(')').map(|e| start + e)?;
    let p: Vec<f64> = s[start..end]
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect();
    if p.len() < 4 {
        return None;
    }
    Some((p[0] as u32, p[1] as u32, p[2] as u32, p[3]))
}

fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth -= 1;
                cur.push(ch);
            }
            c if c == sep && depth == 0 => {
                out.push(cur.clone());
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}
