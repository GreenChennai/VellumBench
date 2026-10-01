//! WebCodecs 车道:canvas+SEEK 页面的**全 GPU** 渲染编码(0.13)。
//!
//! 与截图车道(animlane)的本质区别:像素不过 CPU。SEEK 绘制在
//! GPU 加速的 canvas2D 上,`new VideoFrame(canvas)` 是 GPU 位图引用
//! (零拷贝),`VideoEncoder` 走 D3D11VideoEncoder(AMF/NVENC/QSV 硬件),
//! 页内 AVCC→AnnexB 转换后经本地回环 POST 给静态服务,
//! ffmpeg `-c copy` 无损封装 MP4(纯 remux,秒级)。
//!
//! 资格与回退:仅当页面有 `SEEK` 类驱动函数 **且** 全屏 `<canvas>` 时
//! 尝试;`VideoEncoder.isConfigSupported` 探针失败、编码中途报错、上传
//! 失败一律 Err,由调用方回退截图车道(auto 语义)。
//!
//! 限制:H.264 MP4(无音频);RGB→YUV 的矩阵由浏览器编码器决定
//! (通常 BT.601 video range),输出码流统一打 BT.709/tv 标记
//! (h264_metadata bsf,与截图车道同口径)——同一车道产物自洽,
//! 与老素材混剪时的色度口径见 docs/gpu-full-acceleration-research.md。

use std::path::Path;
use std::time::Duration;

use super::animlane::{style_blocks_of_html, AnimLaneResult, AnimPipeOpts};

/// WebCodecs 车道资格探针 + 单帧节奏(供 auto 决策与告警)。
/// 返回 Err = 页面不符合资格(非 canvas/无驱动/编码器不可用)。
struct ProbeOutcome {
    seek_fn: String,
    canvas_w: u32,
    canvas_h: u32,
}

fn probe_candidate(
    page: &mut vb_browser::page::PageSession,
    seek_fn: &Option<String>,
) -> Result<ProbeOutcome, String> {
    let names = match seek_fn {
        Some(f) => format!("'{}'", f.replace('\'', "")),
        None => "'SEEK','__SEEK__','seek','VB_SEEK'".to_string(),
    };
    let js = format!(
        "(() => {{
            const names = [{names}];
            let fn = null;
            for (const n of names) {{ if (typeof window[n] === 'function') {{ fn = n; break; }} }}
            if (!fn) return JSON.stringify({{err:'no-seek-fn'}});
            const canvas = document.querySelector('canvas');
            if (!canvas || canvas.width < 2 || canvas.height < 2)
                return JSON.stringify({{err:'no-canvas'}});
            if (typeof VideoEncoder === 'undefined')
                return JSON.stringify({{err:'no-webcodecs'}});
            // 只编 canvas ⇒ 页面可见内容必须就是这一个 canvas(覆盖视口、
            // 无其他可见元素)。DOM+canvas 混合页(歌词/终端是 DOM 文本)
            // 只抓 canvas 会产出黑屏残影,必须回退截图车道整页合成。
            const de = document.documentElement;
            if (canvas.width < de.clientWidth * 0.95 || canvas.height < de.clientHeight * 0.95)
                return JSON.stringify({{err:'canvas-not-fullscreen'}});
            let others = 0;
            for (const el of document.body.querySelectorAll('*')) {{
                if (el === canvas || el.contains(canvas) || canvas.contains(el)) continue;
                const tag = el.tagName;
                if (tag === 'SCRIPT' || tag === 'STYLE' || tag === 'LINK' || tag === 'BR') continue;
                const st = getComputedStyle(el);
                if (st.display === 'none' || st.visibility === 'hidden' || parseFloat(st.opacity) === 0) continue;
                const r = el.getBoundingClientRect();
                if (r.width < 2 || r.height < 2) continue;
                others++;
            }}
            if (others > 0)
                return JSON.stringify({{err:'mixed-dom:' + others}});
            return JSON.stringify({{fn, w: canvas.width, h: canvas.height}});
        }})()"
    );
    let v = page.evaluate(&js, false)?;
    let parsed: serde_json::Value =
        serde_json::from_str(v.as_str().unwrap_or("")).map_err(|e| e.to_string())?;
    if let Some(err) = parsed.get("err").and_then(|x| x.as_str()) {
        return Err(format!("WebCodecs 资格不符: {err}"));
    }
    Ok(ProbeOutcome {
        seek_fn: parsed
            .get("fn")
            .and_then(|x| x.as_str())
            .unwrap_or("SEEK")
            .to_string(),
        canvas_w: parsed.get("w").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
        canvas_h: parsed.get("h").and_then(|x| x.as_u64()).unwrap_or(0) as u32,
    })
}

/// AVCC(body = [4B BE 描述长][avcC][拼接块流])→ Annex-B。
/// avcC:byte4 低 2 位 = 长度前缀字节数-1;随后 SPS/PPS 各自带 2 字节长度;
/// 块流:每个 NAL 带同宽长度前缀。全部换成 00 00 00 01 起始码。
fn avcc_to_annexb(body: &[u8]) -> Result<Vec<u8>, &'static str> {
    let desc_len = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let desc_end = 4usize.checked_add(desc_len).ok_or("描述长度溢出")?;
    if body.len() < desc_end {
        return Err("描述长度超出 body");
    }
    let desc = &body[4..desc_end];
    if desc.len() < 5 {
        return Err("avcC 过短");
    }
    let len_size = (desc[4] & 0x03) as usize + 1;
    let mut out = Vec::with_capacity(body.len() + 64);
    const SC: [u8; 4] = [0, 0, 0, 1];
    // avcC:byte5 = (reserved<<5)|(numSPS & 0x1f);随后逐个 2B 长 + NAL;
    // 然后 numPPS(1B)+ 逐个 2B 长 + NAL
    let mut p = 5usize;
    for k in 0..2 {
        if p >= desc.len() {
            return Err("avcC 截断(NAL 计数)");
        }
        let cnt = if k == 0 { desc[p] & 0x1f } else { desc[p] };
        p += 1;
        for _ in 0..cnt {
            if p + 2 > desc.len() {
                return Err("avcC 截断(NAL 长度)");
            }
            let l = u16::from_be_bytes([desc[p], desc[p + 1]]) as usize;
            p += 2;
            if p + l > desc.len() {
                return Err("avcC 截断(NAL 数据)");
            }
            out.extend_from_slice(&SC);
            out.extend_from_slice(&desc[p..p + l]);
            p += l;
        }
    }
    // 块流:按 len_size 前缀逐 NAL
    let mut off = desc_end;
    let read_len = |off: usize| -> Option<usize> {
        if off + len_size > body.len() {
            return None;
        }
        let mut l = 0usize;
        for k in 0..len_size {
            l = (l << 8) | body[off + k] as usize;
        }
        Some(l)
    };
    let mut nal_count = 0usize;
    while off < body.len() {
        let l = read_len(off).ok_or("块流截断(长度前缀)")?;
        off += len_size;
        if l == 0 || off + l > body.len() {
            return Err("块流截断");
        }
        out.extend_from_slice(&SC);
        out.extend_from_slice(&body[off..off + l]);
        off += l;
        nal_count += 1;
    }
    eprintln!(
        "kiln: AVCC→AnnexB 完成:{nal_count} 个 NAL,{} 字节",
        out.len()
    );
    Ok(out)
}

/// WebCodecs 车道导出(MP4)。`opts` 复用 AnimPipeOpts 的宽高/fps/码率等
/// 字段;workers/截屏相关字段忽略(单实例页内编码)。
pub fn export_anim_webcodecs(source: &Path, opts: &AnimPipeOpts) -> Result<AnimLaneResult, String> {
    let t_start = std::time::Instant::now();
    let (mount_dir, html_path) = crate::domexport::resolve_source(source)?;
    let sink = std::env::temp_dir().join(format!(
        "kiln-wc-{}-{}.h264",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    let srv = vb_browser::staticsrv::StaticServer::start_with_upload(&mount_dir, sink.clone())?;
    let url = if source.is_dir() {
        srv.url_for_dir()?
    } else {
        let name = html_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("源文件名非法")?;
        format!(
            "http://127.0.0.1:{}/{}",
            srv.port(),
            crate::domexport::url_encode(name)
        )
    };
    let exe = vb_browser::discover_browser(None)
        .ok_or("未发现系统浏览器(Edge/Chrome);WebCodecs 车道不可用")?;

    let fps = opts.fps.clamp(1, 60);
    let n = ((fps as f32 * opts.duration_s.max(0.1)).ceil().max(1.0)) as usize;
    let vw = if opts.width > 0 { opts.width } else { 1080 };
    let vh = if opts.height > 0 { opts.height } else { vw };
    let dsf = opts.scale.clamp(1, 8);

    let proc = vb_browser::browser::BrowserProcess::launch_with(
        &exe,
        vb_browser::browser::LaunchOptions { gpu: opts.gpu },
    )?;
    let browser = proc.version();
    let mut page = vb_browser::page::PageSession::attach(&proc)?;
    page.set_device_metrics(vw, vh, dsf)?;
    page.navigate(&url)?;
    page.wait_network_idle(Duration::from_secs(5));
    vb_browser::capture::wait_assets(&mut page);
    page.sleep(300);

    let mut warnings: Vec<String> = Vec::new();
    let probe =
        probe_candidate(&mut page, &opts.seek_fn).map_err(|e| format!("WebCodecs 车道: {e}"))?;
    if probe.canvas_w != vw || probe.canvas_h != vh {
        warnings.push(format!(
            "canvas {}x{} 与请求 {}x{} 不同,按 canvas 尺寸编码",
            probe.canvas_w, probe.canvas_h, vw, vh
        ));
    }

    // 分批驱动:每批 BATCH 帧在一次 evaluate 内完成(SEEK→VideoFrame→
    // encode),批间回 Rust 控制超时与存活检查 —— 单次 evaluate 不会
    // 超过 cdp 死亡线,长片也能推进。
    const BATCH: usize = 60;
    let bitrate = opts.bitrate_kbps.max(500) * 1000;
    let keyframe_interval = (fps as usize * 5).max(30);
    for start in (0..n).step_by(BATCH) {
        let end = (start + BATCH).min(n);
        // 批内脚本:Seek 编码 [start, end);首帧初始化解码器配置,
        // 末帧把累计 AnnexB 追加进 window.__vbAnnexB
        let js = format!(
            r#"(async () => {{
  if (!window.__vbWc) {{
    const canvas = document.querySelector('canvas');
    const cfg = {{
      codec: 'avc1.42002A',
      width: canvas.width,
      height: canvas.height,
      bitrate: {bitrate},
      framerate: {fps},
      hardwareAcceleration: 'prefer-hardware',
    }};
    let sup;
    try {{ sup = await VideoEncoder.isConfigSupported(cfg); }}
    catch (e) {{ return JSON.stringify({{err:'cfg:' + e.message}}); }}
    if (!sup.supported) return JSON.stringify({{err:'encoder-unsupported'}});
    window.__vbWc = {{ cfg, canvas, bytes: [], n: 0, desc: null, drawMs: 0, encErr: null }};
    window.__vbEnc = new VideoEncoder({{
      output: (chunk, meta) => {{
        if (meta && meta.decoderConfig && !window.__vbWc.desc) {{
          window.__vbWc.desc = meta.decoderConfig.description;
        }}
        const u8 = new Uint8Array(chunk.byteLength);
        chunk.copyTo(u8);
        window.__vbWc.bytes.push(u8);
        window.__vbWc.n++;
      }},
      error: (e) => {{ window.__vbWc.encErr = String(e); }},
    }});
    window.__vbEnc.configure(cfg);
    window.__vbWc.enc = window.__vbEnc;
    // GPU 同步探针:加速 canvas 的绘制命令是异步提交,VideoFrame 抓取
    // 可能早于 GPU 执行(实测产物全黑残影)。读 1px 强制管线 flush。
    let c2 = canvas.getContext('2d');
    let gl = c2 ? null : (canvas.getContext('webgl') || canvas.getContext('experimental-webgl'));
    if (c2) {{
      window.__vbFlush = () => {{ c2.getImageData(0, 0, 1, 1); }};
    }} else if (gl) {{
      window.__vbFlush = () => {{ gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array(4)); }};
    }}
  }}
  const st = window.__vbWc;
  if (st.encErr) return JSON.stringify({{err:'enc:' + st.encErr}});
  const a = performance.now();
  const fn = {seek};
  for (let i = {start}; i < {end}; i++) {{
    fn(i / {fps});
    if (window.__vbFlush) window.__vbFlush();
    const frame = new VideoFrame(st.canvas, {{
      timestamp: Math.round(i * 1e6 / {fps}),
      duration: Math.round(1e6 / {fps}),
    }});
    st.enc.encode(frame, {{ keyFrame: i % {kf} === 0 }});
    frame.close();
    while (window.__vbEnc.encodeQueueSize > 8) {{
      await new Promise((r) => setTimeout(r, 0));
    }}
    if (st.encErr) return JSON.stringify({{err:'enc:' + st.encErr}});
  }}
  st.drawMs += performance.now() - a;
  return JSON.stringify({{done: {end}, queued: window.__vbEnc.encodeQueueSize}});
}})()"#,
            seek = probe.seek_fn,
            start = start,
            end = end,
            fps = fps,
            bitrate = bitrate,
            kf = keyframe_interval,
        );
        let v = page.evaluate(&js, true)?;
        let parsed: serde_json::Value =
            serde_json::from_str(v.as_str().unwrap_or("{}")).map_err(|e| e.to_string())?;
        if let Some(err) = parsed.get("err").and_then(|x| x.as_str()) {
            return Err(format!("WebCodecs 编码失败(帧 {start}):{err}"));
        }
        let _ = &parsed;
    }
    // flush + 上传(一次 evaluate):body = [4 字节 BE 描述长度][avcC 描述]
    // [拼接的 AVCC 块流];AVCC→AnnexB 转换在 Rust 侧做(DataView 逐字节
    // 解析在页内 RangeError 过,且 Rust 侧可调试)。
    let flush_js = format!(
        r#"(async () => {{
  try {{
    const st = window.__vbWc;
    if (st.encErr) return JSON.stringify({{err:'enc:' + st.encErr}});
    await window.__vbEnc.flush();
    window.__vbEnc.close();
    if (!st.desc) return JSON.stringify({{err:'no-decoder-config'}});
    const parts = [new Uint8Array(4)];
    new DataView(parts[0].buffer).setUint32(0, st.desc.byteLength);
    parts.push(new Uint8Array(st.desc));
    let total = st.desc.byteLength;
    for (const u8 of st.bytes) {{
      parts.push(u8);
      total += u8.byteLength;
    }}
    const blob = new Blob(parts);
    const t0 = performance.now();
    const up = await fetch('/__kiln-upload', {{ method: 'POST', body: blob }});
    if (!up.ok) return JSON.stringify({{err: 'upload ' + up.status}});
    return JSON.stringify({{
      bytes: total,
      chunks: st.n,
      draw_per_frame_ms: +(st.drawMs / {n}).toFixed(1),
      upload_ms: +(performance.now() - t0).toFixed(0),
    }});
  }} catch (e) {{
    return JSON.stringify({{err:'page:' + (e && e.message || e)}});
  }}
}})()"#,
        n = n,
    );
    let v = page.evaluate(&flush_js, true)?;
    let parsed: serde_json::Value =
        serde_json::from_str(v.as_str().unwrap_or("{}")).map_err(|e| e.to_string())?;
    if let Some(err) = parsed.get("err").and_then(|x| x.as_str()) {
        return Err(format!("WebCodecs 收尾失败:{err}"));
    }
    let total_bytes = parsed.get("bytes").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
    let total_chunks = parsed.get("chunks").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
    let per_frame_ms = parsed
        .get("draw_per_frame_ms")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    warnings.push(format!(
        "WebCodecs 页内硬编:{n} 帧,绘制约 {per_frame_ms}ms/帧,产物 {total_bytes} 字节/{total_chunks} chunks"
    ));

    page.close();
    drop(proc);

    // AVCC → Annex-B(读取上传 body:[4B BE 描述长][avcC][AVCC 块流])。
    // 封装段整体进闭包:此前往何一步 Err 都把 sink/半截 mp4 留在 %TEMP%
    // (sweep 兜底前它们是永久残留),现在无论成败一律清干净。
    let mp4_path = sink.with_extension("mp4");
    let encode_result = (|| -> Result<Vec<u8>, String> {
        let raw = std::fs::read(&sink).map_err(|e| format!("读取上传产物失败:{e}"))?;
        if raw.len() < 4 {
            return Err("WebCodecs 上传产物过短".into());
        }
        // 上传完整性对账:页面自报 bytes 与落盘字节数应严格相等。
        // 不等 = 中途截断(服务端已拦短传,这里是双保险),按警告放行
        // 让 ffmpeg 给出最终裁决,数字进 warnings 方便定位。
        if total_bytes > 0 && raw.len() != total_bytes {
            warnings.push(format!(
                "WebCodecs 上传对账:页面自报 {total_bytes} 字节,落盘 {} 字节(可能截断)",
                raw.len()
            ));
        }
        let annexb = avcc_to_annexb(&raw).map_err(|e| format!("WebCodecs 码流转换失败:{e}"))?;
        std::fs::write(&sink, &annexb).map_err(|e| format!("写 Annex-B 失败:{e}"))?;

        // ffmpeg -c copy 无损封装(纯 remux;VUI 标记与截图车道同口径)
        if !crate::frames::ffmpeg_available() {
            return Err("WebCodecs 产物封装需要 ffmpeg(-c copy);未找到 ffmpeg".into());
        }
        let output = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "h264",
                "-framerate",
                &fps.to_string(),
                "-i",
                sink.to_str().unwrap_or("in.h264"),
                "-c:v",
                "copy",
                "-movflags",
                "+faststart",
                mp4_path.to_str().unwrap_or("out.mp4"),
            ])
            .output()
            .map_err(|e| format!("ffmpeg 封装启动失败:{e}"))?;
        if !output.status.success() {
            return Err(format!(
                "WebCodecs 产物封装失败: {}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(300)
                    .collect::<String>()
            ));
        }
        std::fs::read(&mp4_path).map_err(|e| format!("读取成片失败:{e}"))
    })();
    let _ = std::fs::remove_file(&sink);
    let _ = std::fs::remove_file(&mp4_path);
    let bytes = encode_result?;

    warnings.extend(
        srv.take_not_found()
            .iter()
            .map(|s| vb_browser::staticsrv::asset_not_found_message(s)),
    );
    warnings.push(format!(
        "动画流水线:{n} 帧 @ {fps}fps,总耗时 {:.1}s({:.1} 帧/s)",
        t_start.elapsed().as_secs_f32(),
        n as f32 / t_start.elapsed().as_secs_f32().max(1e-3),
    ));
    let anim_coverage = crate::anim::AnimCoverage::from_keyframes(
        &crate::anim::parse_keyframes(&style_blocks_of_html(&html_path)),
        "browser",
    );
    Ok(AnimLaneResult {
        bytes,
        frames: n,
        browser,
        encoder_used: Some("h264(WebCodecs 硬编)".into()),
        warnings,
        anim_coverage,
    })
}
