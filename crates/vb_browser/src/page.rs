//! 页面会话:CDP 高层操作 + WPI 捕获协议十要素移植(design/19 附录 A)。
//!
//! 十要素映射(WPI 行号 → 本文件):
//!  1 load+networkidle      → `navigate_and_settle_load`(`capture_engine.py:89-94`)
//!  2 视口只定宽不撑高      → `set_device_metrics`(`controller.py:156-157`)
//!  3 DSF 原生倍率          → `set_device_metrics`(`browser_host.py:99-103`)
//!  4 单拍/分块滚动拼接     → `capture.rs`(`capture_engine.py:232-336`)
//!  5 fonts.ready 资源等待  → `WAIT_ASSETS_JS`(`:394-433`)
//!  6 滚动触发 reveal       → `trigger_scroll_reveals`(`:465-510`)
//!  7 有限动画 finish 定格  → `freeze_animations`(`:435-463`)
//!  8 视觉稳定哈希兜底      → `wait_visual_stability`(`:541-577`)
//!  9 透明/白底展平         → `capture.rs`(`png_exporter.py:15-22`)
//! 10 rAF 节流+reduced-motion → `RAF_THROTTLE_JS` + `emulate_static`(`:45-52`,`:86`)

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::b64;
use crate::browser::BrowserProcess;
use crate::cdp::Cdp;

/// rAF 节流注入(页面脚本执行前;WPI `_RENDER_THROTTLE_JS`,16ms)。
pub const RAF_THROTTLE_JS: &str = r#"(() => {
    if (window.__wpiRafThrottled) return;
    window.__wpiRafThrottled = true;
    window.requestAnimationFrame = (cb) =>
        setTimeout(() => cb(performance.now()), 16);
    window.cancelAnimationFrame = (id) => clearTimeout(id);
})();"#;

/// 字体/图片资源等待(WPI `wait_assets` JS 原样移植)。
pub const WAIT_ASSETS_JS: &str = r#"(async () => {
    try {
        if (document.fonts && document.fonts.ready) {
            await Promise.race([
                document.fonts.ready,
                new Promise(r => setTimeout(r, 3000)),
            ]);
        }
    } catch (e) {}
    const lazy = document.querySelectorAll('img[loading="lazy"]');
    for (const i of lazy) { try { i.loading = 'eager'; } catch (e) {} }
    const imgs = Array.from(document.images);
    const pending = imgs.filter(i => !i.complete || i.naturalWidth === 0);
    if (pending.length) {
        await Promise.race([
            Promise.all(pending.map(i => new Promise(res => {
                if (i.complete && i.naturalWidth) return res();
                const done = () => res();
                i.addEventListener('load', done, {once: true});
                i.addEventListener('error', done, {once: true});
            }))),
            new Promise(r => setTimeout(r, 5000)),
        ]);
    }
    return true;
})()"#;

/// 有限动画 finish(WPI `freeze_animations` JS;返回仍在跑的无限动画数)。
pub const FREEZE_ANIMATIONS_JS: &str = r#"(() => {
    if (typeof document.getAnimations !== 'function') return 0;
    let inf = 0;
    for (const a of document.getAnimations()) {
        if (a.playState !== 'running') continue;
        let isInf = false;
        try {
            const eff = a.effect;
            const t = eff && typeof eff.getTiming === 'function' ? eff.getTiming() : null;
            isInf = !!(t && (t.iterations === Infinity || t.duration === Infinity));
        } catch (e) {}
        if (isInf) { inf += 1; continue; }
        try { a.finish(); } catch (e) {}
    }
    return inf;
})()"#;

/// 内容尺寸(WPI `content_size` JS)。
pub const CONTENT_SIZE_JS: &str = r#"(() => [
    Math.max(document.documentElement.scrollWidth,
             document.body ? document.body.scrollWidth : 0),
    Math.max(document.documentElement.scrollHeight,
             document.body ? document.body.scrollHeight : 0),
])()"#;

/// 顶部小条隐藏/恢复(WPI `_toggle_fixed_topbar` JS)。
pub const TOGGLE_FIXED_TOPBAR_JS: &str = r#"(hide) => {
    for (const el of document.querySelectorAll('*')) {
        try {
            if (getComputedStyle(el).position !== 'fixed') continue;
            const r = el.getBoundingClientRect();
            if (r.top <= 120 && r.height <= 200) {
                el.style.setProperty('visibility', hide ? 'hidden' : '', 'important');
            }
        } catch (e) {}
    }
}"#;

/// 视口外 canvas 探测(WPI `has_below_fold_canvas` JS)。
pub const BELOW_FOLD_CANVAS_JS: &str = r#"(vh) => {
    for (const c of document.querySelectorAll('canvas')) {
        const r = c.getBoundingClientRect();
        if (r.width > 0 && r.top >= vh) return true;
    }
    return false;
}"#;

/// 外链资源失败收集(WPI `collect_resource_warnings` JS)。
pub const RESOURCE_WARNINGS_JS: &str = r#"() => {
    const out = [];
    for (const el of document.querySelectorAll('img,video,audio,source,link,script,iframe')) {
        const src = el.currentSrc || el.src || el.href;
        if (!src) continue;
        const tag = el.tagName.toLowerCase();
        if (tag === 'img' && el.complete && el.naturalWidth === 0) {
            out.push(src);
        } else if (tag === 'link' && !el.sheet) {
            out.push(src);
        }
    }
    return out;
}"#;

/// 画板取景:重置 body 页边距(P0-3)。页边距属页面 chrome 而非画板画布,
/// 与 native 车道「画板即画布」同语义;仅在画板声明尺寸取景时注入。
pub const BODY_MARGIN_RESET_JS: &str = r#"(() => {
    const s = document.createElement('style');
    s.textContent = 'body { margin: 0 !important; }';
    document.head.appendChild(s);
    return true;
})()"#;

/// 画板矩形定位(P0-3):优先显式 `vb-artboard` 标记(含旧前缀),否则在
/// body 顶层元素里找边界盒与声明尺寸一致者(与 vb_doc 导入器的启发式同
/// 口径的采集侧镜像)。返回视口相对坐标(调用方保证 scrollY=0,即文档坐标)。
pub const ARTBOARD_RECT_JS: &str = r#"(w, h) => {
    const SELS = ['.vb-artboard', '.vs-artboard', '.vsm-artboard'];
    let el = null;
    for (const s of SELS) {
        el = document.querySelector(s);
        if (el) break;
    }
    if (!el && document.body) {
        for (const c of document.body.children) {
            const r = c.getBoundingClientRect();
            if (r.width > 0 && Math.abs(r.width - w) < 1 && (h <= 0 || Math.abs(r.height - h) < 1)) {
                el = c;
                break;
            }
        }
    }
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return [r.x, r.y, r.width, r.height];
}"#;

/// 页面会话:一条 CDP 连接 + 协议级状态(load/网络活动)。
pub struct PageSession {
    pub cdp: Cdp,
    load_fired: bool,
    last_net_activity: Instant,
    net_event_count: u64,
    pub crashed: bool,
}

impl PageSession {
    /// 连接到 BrowserProcess 新开的标签页。
    pub fn attach(browser: &BrowserProcess) -> Result<Self, String> {
        let ws_path = browser.new_tab("about:blank")?;
        let ws = crate::ws::WsConn::connect(
            "127.0.0.1",
            browser.port,
            &ws_path,
            Duration::from_secs(10),
        )?;
        let mut page = PageSession {
            cdp: Cdp::new(ws),
            load_fired: false,
            last_net_activity: Instant::now(),
            net_event_count: 0,
            crashed: false,
        };
        page.cdp
            .call("Page.enable", json!({}), Duration::from_secs(5))?;
        page.cdp
            .call("Network.enable", json!({}), Duration::from_secs(5))?;
        page.cdp
            .call("Runtime.enable", json!({}), Duration::from_secs(5))?;
        Ok(page)
    }

    /// 消费事件,维护 load / 网络活动状态。
    fn sync_events(&mut self) {
        for (method, _params) in self.cdp.drain_events() {
            if method.starts_with("Network.") {
                self.net_event_count += 1;
                self.last_net_activity = Instant::now();
            } else if method == "Page.loadEventFired" {
                self.load_fired = true;
            } else if method == "Inspector.targetCrashed" {
                self.crashed = true;
            }
        }
    }

    /// 渲染器崩溃检测(各等待点轮询;崩溃后继续等待只会超时)。
    pub fn ensure_alive(&mut self) -> Result<(), String> {
        self.sync_events();
        if self.crashed {
            return Err("渲染器崩溃(Inspector.targetCrashed)".into());
        }
        Ok(())
    }

    /// 页面脚本执行前注入(rAF 节流)。
    pub fn add_init_script(&mut self, source: &str) -> Result<(), String> {
        self.cdp.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": source }),
            Duration::from_secs(5),
        )?;
        Ok(())
    }

    /// 视口 + 原生倍率(要素 2/3;只定宽,高用占位值防 100vh 撑爆)。
    pub fn set_device_metrics(&mut self, width: u32, height: u32, dsf: u32) -> Result<(), String> {
        self.cdp.call(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": width,
                "height": height,
                "deviceScaleFactor": dsf,
                "mobile": false
            }),
            Duration::from_secs(5),
        )?;
        Ok(())
    }

    /// screen 媒体 + prefers-reduced-motion:reduce(要素 10)。
    pub fn emulate_static(&mut self) -> Result<(), String> {
        self.cdp.call(
            "Emulation.setEmulatedMedia",
            json!({
                "media": "screen",
                "features": [ { "name": "prefers-reduced-motion", "value": "reduce" } ]
            }),
            Duration::from_secs(5),
        )?;
        Ok(())
    }

    /// 仅 screen 媒体(PDF 用,保持动画状态不变)。
    pub fn emulate_media_screen(&mut self) -> Result<(), String> {
        self.cdp.call(
            "Emulation.setEmulatedMedia",
            json!({ "media": "screen" }),
            Duration::from_secs(5),
        )?;
        Ok(())
    }

    /// 导航并等待 load(要素 1 前半;30s 上限)。
    pub fn navigate(&mut self, url: &str) -> Result<(), String> {
        self.cdp.call(
            "Page.navigate",
            json!({ "url": url }),
            Duration::from_secs(35),
        )?;
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.load_fired {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("页面加载超时(load 事件未触发)".into());
            }
            self.cdp.pump(remaining.min(Duration::from_millis(50)))?;
            self.sync_events();
            self.ensure_alive()?;
        }
        Ok(())
    }

    /// networkidle(500ms 静默;3s 上限后放弃,与 WPI 容错一致)。
    pub fn wait_network_idle(&mut self, cap: Duration) {
        let deadline = Instant::now() + cap;
        loop {
            self.sync_events();
            if self.last_net_activity.elapsed() >= Duration::from_millis(500) {
                return;
            }
            if Instant::now() >= deadline {
                return;
            }
            let _ = self.cdp.pump(Duration::from_millis(50));
            self.sync_events();
        }
    }

    pub fn sleep(&mut self, ms: u64) {
        let deadline = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < deadline {
            let _ = self.cdp.pump(Duration::from_millis(20));
            self.sync_events();
        }
    }

    /// evaluate:表达式求值(可 await Promise),返回 returnByValue 结果。
    pub fn evaluate(&mut self, expression: &str, await_promise: bool) -> Result<Value, String> {
        let res = self.cdp.call(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": await_promise,
            }),
            Duration::from_secs(30),
        )?;
        if let Some(detail) = res.get("exceptionDetails") {
            return Err(format!(
                "页面执行异常: {}",
                detail.get("text").and_then(Value::as_str).unwrap_or("?")
            ));
        }
        // cdp.call 返回 CDP result 层:内层 result.value 才是 returnByValue 的值
        Ok(res
            .get("result")
            .and_then(|r| r.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    // ------------------------------------------------------------ 页面查询
    pub fn content_size(&mut self) -> Result<(u32, u32), String> {
        let v = self.evaluate(CONTENT_SIZE_JS, false)?;
        let arr = v.as_array().ok_or("content_size 返回非数组")?;
        let w = arr.first().and_then(Value::as_f64).unwrap_or(1.0);
        let h = arr.get(1).and_then(Value::as_f64).unwrap_or(1.0);
        Ok((w.max(1.0) as u32, h.max(1.0) as u32))
    }

    pub fn inner_height(&mut self) -> Result<u32, String> {
        let v = self.evaluate("(() => window.innerHeight || 600)()", false)?;
        Ok(v.as_f64().unwrap_or(600.0) as u32)
    }

    pub fn scroll_y(&mut self) -> u32 {
        self.evaluate("(() => window.scrollY || 0)()", false)
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u32
    }

    pub fn scroll_to(&mut self, y: u32) {
        let _ = self.evaluate(&format!("(() => {{ window.scrollTo(0, {y}); }})()"), false);
    }

    pub fn wait_two_raf(&mut self) {
        let _ = self.evaluate(
            "(() => new Promise(r => requestAnimationFrame(() => requestAnimationFrame(r))))()",
            true,
        );
    }

    pub fn has_below_fold_canvas(&mut self) -> bool {
        let vh = self.inner_height().unwrap_or(600);
        self.evaluate(&format!("({BELOW_FOLD_CANVAS_JS})({vh})"), false)
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    pub fn collect_resource_warnings(&mut self) -> Vec<String> {
        match self.evaluate(&format!("({RESOURCE_WARNINGS_JS})()"), false) {
            Ok(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(|s| format!("外部资源加载失败: {s}"))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// 顶部小条显隐(要素 4 子步骤)。
    pub fn toggle_fixed_topbar(&mut self, hide: bool) {
        // TOGGLE_FIXED_TOPBAR_JS 为 `(hide) => {...}`,此处带参直调
        let js = format!("({})({})", TOGGLE_FIXED_TOPBAR_JS, hide);
        let _ = self.evaluate(&js, false);
    }

    // ----------------------------------------------------------- 截图/打印
    /// Page.captureScreenshot。clip 为 CSS px(视口相对;beyond=true 时为文档坐标)。
    pub fn screenshot(
        &mut self,
        format: &str,
        quality: Option<u8>,
        clip: Option<(f64, f64, f64, f64)>,
        beyond: bool,
        omit_background: bool,
    ) -> Result<Vec<u8>, String> {
        let mut params = json!({
            "format": format,
            "captureBeyondViewport": beyond,
            "omitBackground": omit_background,
        });
        if let Some(quality) = quality {
            params["quality"] = json!(quality);
        }
        if let Some((x, y, w, h)) = clip {
            params["clip"] = json!({ "x": x, "y": y, "width": w, "height": h, "scale": 1 });
        }
        let res = self
            .cdp
            .call("Page.captureScreenshot", params, Duration::from_secs(180))?;
        let data = res
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| "截图响应缺少 data".to_string())?;
        b64::decode(data).map_err(|e| format!("截图数据解码失败: {e}"))
    }

    /// Page.printToPDF。
    pub fn print_to_pdf(
        &mut self,
        paper_width_in: f64,
        paper_height_in: f64,
        prefer_css_page_size: bool,
    ) -> Result<Vec<u8>, String> {
        let params = json!({
            "printBackground": true,
            "paperWidth": paper_width_in,
            "paperHeight": paper_height_in,
            "marginTop": 0.0,
            "marginBottom": 0.0,
            "marginLeft": 0.0,
            "marginRight": 0.0,
            "scale": 1.0,
            "preferCSSPageSize": prefer_css_page_size,
        });
        let res = self
            .cdp
            .call("Page.printToPDF", params, Duration::from_secs(300))?;
        let data = res
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| "printToPDF 响应缺少 data".to_string())?;
        b64::decode(data).map_err(|e| format!("PDF 数据解码失败: {e}"))
    }

    pub fn close(&mut self) {
        self.cdp.close();
    }
}
