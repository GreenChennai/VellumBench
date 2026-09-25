//! `vb_web` — VellumBench Web 壳(阶段 5-G / 05-11-2,台账 09-K,ADR-0032)。
//!
//! **定位(诚实边界)**:这不是「vb_app 搬进浏览器」,而是分册允许的
//! **裁剪版 web 壳** —— 桌面全功能应用的 wasm 化被原生依赖阻碍(评估报告
//! `docs/k2-web-capability.md`:rfd 原生文件对话框 / eframe `run_native` /
//! notify 文件监视 / pdfium 动态绑定均无 wasm32 形态)。本壳只承诺:
//!
//! 1. **只读预览**:canonical HTML 本身就是预览(iframe srcdoc)——
//!    「文件真相源 + HTML 可 diff」原则在 Web 端原样成立,预览即真相;
//! 2. **画板切换**:按 `data-vb-id` 注入一条 CSS 规则,只显示选中画板;
//! 3. **文字轻编辑**:模型命令路径(`Command::SetText`,与桌面同一 apply,
//!    rev 递增、捕获旧快照可逆),不是 contenteditable 的野路子;
//! 4. **导出**:`vb_doc::export::render_project` 同一 canonical 序列化,
//!    产物 = index.html + styles/main.css(与桌面 Ctrl+S 字节同源);
//!    另提供 vb_kiln CPU 光栅 PNG(文字需先注册字体字节,见下)。
//!
//! **已知边界**:字体(vb_render 真文本管线在浏览器里没有系统字体可枚举,
//! 光栅前需经 `register_font_bytes` 喂字体字节;HTML 预览不受此限,浏览器
//! 自己排字);图像节点引用相对路径时由 JS 侧改写或容忍破图。
//! 运行方式见 `crates/vb_web/web/`(index.html + index.js,纯静态页,
//! `--target wasm32-unknown-unknown` 构建 + wasm-bindgen --target web)。
//!
//! **分层**:全部业务在宿主可测的 `WebCore`(纯 Rust,`Result<_, String>`);
//! `#[wasm_bindgen]` 的 `VbWebApp` 只是 String→JsValue 的转接板 ——
//! wasm-bindgen 导入符号在宿主上不可调用,门禁 `cargo test --workspace`
//! 跑在 Windows,测试必须打在 WebCore 层。

use vb_doc::commands::TextSnapshot;
use vb_doc::model::{Document, NodeId, NodeKind};
use vb_doc::Command;

use wasm_bindgen::prelude::*;

/// 主样式表约定路径(项目契约,ADR-0028;import 内联与预览重组共用)。
pub const CSS_LINK_PATH: &str = "styles/main.css";

// ─────────────────────────── WebCore:宿主可测的业务层 ───────────────────────────

/// Web 壳会话核心:内存中的文档模型(与桌面同一 `vb_doc::model::Document`)。
#[derive(Debug, Clone)]
pub struct WebCore {
    pub doc: Document,
    /// 最近一次导入的告警(原样透给 UI,不静默)。
    pub warnings: Vec<String>,
}

impl Default for WebCore {
    fn default() -> Self {
        Self::new()
    }
}

impl WebCore {
    /// 新会话:空白文档(含默认画板,与桌面「新建」一致)。
    pub fn new() -> WebCore {
        WebCore {
            doc: Document::new_default(),
            warnings: Vec::new(),
        }
    }

    /// 从 HTML 导入项目。`main_css` = 外部样式表内容(项目用
    /// `<link href="styles/main.css">` 时必传):import 侧走 `std::fs` 读盘,
    /// wasm 无文件系统,壳先把 CSS 内联为 `<style>` 再导入 —— 等价信息,
    /// 不引入第二套解析。告警(缺表/降级)原样返回,不静默。
    /// 返回 JSON 摘要 `{title, artboards[], warnings[]}`。
    pub fn load_project(
        &mut self,
        index_html: &str,
        main_css: Option<&str>,
    ) -> Result<String, String> {
        let html = match main_css {
            Some(css) => inline_main_css(index_html, css),
            None => index_html.to_string(),
        };
        // project_dir 在 wasm 无意义;给 "." 让相对解析走默认分支
        let r = vb_doc::import::import_html(&html, std::path::Path::new("."))
            .map_err(|e| format!("导入失败:{e}"))?;
        let mut doc = r.doc;
        self.warnings = r.warnings.clone();
        // 布局求值:声明几何 → 内存 geom(与桌面「打开项目」同一条链)
        let synthetic = r.synthetic_artboard;
        for w in vb_layout::apply_import_layout(&mut doc, None, synthetic) {
            self.warnings.push(w);
        }
        self.doc = doc;
        serde_json::to_string(&serde_json::json!({
            "title": self.doc.meta.title,
            "artboards": self.artboard_rows(),
            "warnings": self.warnings,
        }))
        .map_err(|e| e.to_string())
    }

    /// 画板列表 JSON:`[{sid, name, w, h}]`(导出顺序)。
    pub fn artboards_json(&self) -> String {
        serde_json::to_string(&self.artboard_rows()).unwrap_or_else(|_| "[]".into())
    }

    /// 文本节点列表 JSON:`[{sid, name, text}]`(文档序;root 子树覆盖
    /// 全部画板 —— 画板本身挂在 root 下)。
    pub fn text_nodes_json(&self) -> String {
        let mut rows = Vec::new();
        collect_text_nodes(&self.doc, self.doc.root, &mut rows);
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
    }

    /// 文字轻编辑:走 `Command::SetText`(捕获旧快照,可逆路径),
    /// apply 后 rev 递增 —— 与桌面同一命令底座,不绕过模型。
    pub fn set_text(&mut self, sid: &str, text: &str) -> Result<(), String> {
        let id = self
            .doc
            .find_by_sid(sid)
            .ok_or_else(|| format!("sid 不存在:{sid}"))?;
        let old = match self.doc.nodes.get(id).map(|n| &n.kind) {
            Some(NodeKind::Text { text, segments, .. }) => Some(TextSnapshot {
                text: text.clone(),
                segs: segments.clone(),
            }),
            Some(_) => return Err("目标不是文本节点".into()),
            None => return Err("节点缺失".into()),
        };
        Command::SetText {
            sid: sid.to_string(),
            new: text.to_string(),
            old,
        }
        .apply(&mut self.doc)
        .map_err(|e| format!("SetText 失败:{e}"))?;
        // rev 与桌面 exec 同规:命令成功应用后递增(乐观锁可见性)
        self.doc.rev += 1;
        Ok(())
    }

    /// 预览 HTML(canonical index.html,main.css 内联)。
    /// `active_sid` = Some 时注入「只显示该画板」的 CSS 规则(画板切换)。
    /// 注入只发生在预览串上,文档模型与导出产物不受影响。
    pub fn preview_html(&self, active_sid: Option<&str>) -> Result<String, String> {
        let files = vb_doc::export::render_project(&self.doc);
        let mut html = files
            .files
            .iter()
            .find(|(p, _)| p == "index.html")
            .map(|(_, c)| c.clone())
            .unwrap_or_default();
        if let Some((_, css)) = files.files.iter().find(|(p, _)| p == CSS_LINK_PATH) {
            html = inline_main_css(&html, css);
        }
        if let Some(sid) = active_sid {
            // data-vb-id 由导出侧原样输出,sid 是受限 base36(无引号注入面)
            let rule = format!(
                "<style>section.vb-artboard{{display:none}}\
                 section.vb-artboard[data-vb-id=\"{sid}\"]{{display:block}}</style>"
            );
            html = html.replace("</head>", &format!("{rule}</head>"));
        }
        Ok(html)
    }

    /// 导出文件表:`(相对路径, 内容)`(index.html / styles/main.css)。
    /// 与桌面 Ctrl+S 同一 `render_project` 序列化路径 —— 字节同源。
    pub fn export_files(&self) -> Vec<(String, String)> {
        vb_doc::export::render_project(&self.doc).files
    }

    /// 导出文件 JSON:`[{path, content}]`。
    pub fn export_files_json(&self) -> String {
        let rows: Vec<serde_json::Value> = self
            .export_files()
            .iter()
            .map(|(p, c)| serde_json::json!({ "path": p, "content": c }))
            .collect();
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".into())
    }

    /// 画板 → PNG 字节(vb_kiln CPU 光栅;与 CLI/CI 同一渲染真相)。
    /// **文字需要字体**:浏览器无系统字体可枚举,先 [`register_font_bytes`]
    /// 注册;未注册时文字不画出(不 panic,与 CLI 无字体的降级一致)。
    pub fn raster_png(&self, sid: &str, scale: f32) -> Result<Vec<u8>, String> {
        let id = self
            .doc
            .find_by_sid(sid)
            .ok_or_else(|| format!("sid 不存在:{sid}"))?;
        let list = vb_render::encode::encode_artboard(&self.doc, id)
            .map_err(|e| format!("编码失败:{e}"))?;
        vb_kiln::raster::rasterize_png_bytes(&list, scale, false)
            .map_err(|e| format!("光栅失败:{e}"))
    }

    /// 当前 rev(轻编辑也走命令路径,rev 每次编辑递增;展示用)。
    pub fn rev(&self) -> u64 {
        self.doc.rev
    }

    fn artboard_rows(&self) -> Vec<serde_json::Value> {
        self.doc
            .artboards
            .iter()
            .filter_map(|&id| self.doc.nodes.get(id))
            .map(|n| {
                serde_json::json!({
                    "sid": n.sid.as_str(),
                    "name": n.name,
                    "w": n.geom.w,
                    "h": n.geom.h,
                })
            })
            .collect()
    }
}

// ─────────────────────────── wasm-bindgen 转接板(String → JsValue) ───────────────────────────

/// 注册字体字节(家庭/字重 → 字体数据;K2 webfont 边界的补法)。
/// wasm 侧 fetch / 文件选择器拿到字节后调用;桌面端不必使用。
#[wasm_bindgen]
pub fn register_font(family: &str, weight: u16, bytes: Vec<u8>) {
    vb_render::text::register_font_bytes(family, weight, bytes);
}

/// Web 壳 wasm 门面:每个方法只是 `WebCore` 同名方法的 JsValue 转接
/// (wasm-bindgen 导入符号在宿主不可调用,故业务不入此层)。
#[wasm_bindgen]
pub struct VbWebApp {
    core: WebCore,
}

impl Default for VbWebApp {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl VbWebApp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> VbWebApp {
        VbWebApp {
            core: WebCore::new(),
        }
    }

    pub fn load_project(
        &mut self,
        index_html: &str,
        main_css: Option<String>,
    ) -> Result<JsValue, JsValue> {
        self.core
            .load_project(index_html, main_css.as_deref())
            .map(|s| JsValue::from_str(&s))
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn artboards_json(&self) -> String {
        self.core.artboards_json()
    }

    pub fn text_nodes_json(&self) -> String {
        self.core.text_nodes_json()
    }

    pub fn set_text(&mut self, sid: &str, text: &str) -> Result<(), JsValue> {
        self.core
            .set_text(sid, text)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn preview_html(&self, active_sid: Option<String>) -> Result<String, JsValue> {
        self.core
            .preview_html(active_sid.as_deref())
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn export_files_json(&self) -> String {
        self.core.export_files_json()
    }

    pub fn raster_png(&self, sid: &str, scale: f32) -> Result<Vec<u8>, JsValue> {
        self.core
            .raster_png(sid, scale)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn rev(&self) -> u64 {
        self.core.rev()
    }
}

// ─────────────────────────── 内部纯函数(单测直打) ───────────────────────────

/// 把 `<link rel="stylesheet" href="styles/main.css">` 内联为 `<style>`。
/// 只匹配 canonical 导出自身产生的形态(壳的承诺范围);其余外链不动,
/// 内联不成立时原样返回(导入侧按「样式表缺失」告警降级,不静默)。
fn inline_main_css(html: &str, css: &str) -> String {
    let needle = format!(r#"<link rel="stylesheet" href="{CSS_LINK_PATH}">"#);
    let style = format!("<style>\n{css}</style>");
    if html.contains(&needle) {
        html.replace(&needle, &style)
    } else {
        html.to_string()
    }
}

/// 先序收集文本节点(sid/name/text;非 Text 节点无文字,跳过)。
fn collect_text_nodes(doc: &Document, root: NodeId, rows: &mut Vec<serde_json::Value>) {
    let Some(n) = doc.nodes.get(root) else { return };
    if let NodeKind::Text { text, .. } = &n.kind {
        rows.push(serde_json::json!({
            "sid": n.sid.as_str(),
            "name": n.name,
            "text": text,
        }));
    }
    for &c in &n.children {
        collect_text_nodes(doc, c, rows);
    }
}

// ─────────────────────────── 单测(宿主跑;纯函数 + 模型路径) ───────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小项目 HTML:两个画板、两段文本,ExternalCss 形态。
    fn fixture_html() -> String {
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>Web 壳夹具</title>
<link rel="stylesheet" href="styles/main.css">
</head>
<body>
<section class="vb-artboard" data-vb-id="1" data-vb-name="画板 1">
<p class="t1" data-vb-id="2">标题甲</p>
</section>
<section class="vb-artboard" data-vb-id="3" data-vb-name="画板 2">
<p class="t2" data-vb-id="4">标题乙</p>
</section>
</body>
</html>"#
            .to_string()
    }

    fn fixture_css() -> String {
        ".t1 { position: absolute; left: 10px; top: 20px; width: 100px; height: 30px; }\n\
         .t2 { position: absolute; left: 30px; top: 40px; width: 100px; height: 30px; }\n"
            .to_string()
    }

    /// 内联:link 换成 style;无 link 时原样(不伪装成功)。
    #[test]
    fn inline_main_css_replaces_link() {
        let out = inline_main_css(&fixture_html(), ".t1{}");
        assert!(
            !out.contains(r#"<link rel="stylesheet""#),
            "link 必须被替换"
        );
        assert!(out.contains("<style>\n.t1{}</style>"));
        // 没有 link 的 HTML 原样返回
        assert_eq!(inline_main_css("<html></html>", ".x{}"), "<html></html>");
    }

    /// 加载 → 画板/文本清单 → 改文字 → 预览含新文字 + 切画板规则。
    #[test]
    fn load_edit_preview_round() {
        let mut core = WebCore::new();
        let summary = core
            .load_project(&fixture_html(), Some(&fixture_css()))
            .expect("导入成功");
        let s: serde_json::Value = serde_json::from_str(&summary).unwrap();
        assert_eq!(s["title"], "Web 壳夹具");
        let abs = s["artboards"].as_array().unwrap();
        assert_eq!(abs.len(), 2, "两个画板:{s}");

        let rows: Vec<serde_json::Value> = serde_json::from_str(&core.text_nodes_json()).unwrap();
        assert_eq!(rows.len(), 2);
        let t1_sid = rows[0]["sid"].as_str().unwrap().to_string();

        let rev0 = core.rev();
        core.set_text(&t1_sid, "改过的标题").expect("轻编辑成功");
        assert_eq!(core.rev(), rev0 + 1, "命令 apply 必须推进 rev");

        // 预览:含新文本;未编辑画板文字原样;main.css 已内联
        let html_all = core.preview_html(None).unwrap();
        assert!(html_all.contains("改过的标题"));
        assert!(html_all.contains("标题乙"), "未编辑的画板文字原样");
        assert!(html_all.contains("<style>"), "main.css 已内联进预览");
        assert!(
            !html_all.contains(r#"<link rel="stylesheet""#),
            "预览不引用外链"
        );
        // 画板切换:注入只显规则,且预览只是显示层 —— 导出产物不受影响
        let ab_sid = abs[0]["sid"].as_str().unwrap();
        let html_one = core.preview_html(Some(ab_sid)).unwrap();
        assert!(
            html_one.contains(&format!(
                r#"section.vb-artboard[data-vb-id="{ab_sid}"]{{display:block}}"#
            )),
            "必须注入只显规则"
        );
        let exported = core.export_files()[0].1.clone();
        assert!(
            !exported.contains("display:none"),
            "切换规则不得泄漏进导出产物"
        );
    }

    /// 导出:render_project 同源;导出产物再导入 → 编辑存活(往返闭环)。
    #[test]
    fn export_files_and_reimport() {
        let mut core = WebCore::new();
        core.load_project(&fixture_html(), Some(&fixture_css()))
            .unwrap();
        let rows: Vec<serde_json::Value> = serde_json::from_str(&core.text_nodes_json()).unwrap();
        core.set_text(rows[0]["sid"].as_str().unwrap(), "导出甲")
            .unwrap();

        let files = core.export_files();
        assert_eq!(files.len(), 2, "ExternalCss 默认:html + css");
        assert_eq!(files[0].0, "index.html");
        assert_eq!(files[1].0, "styles/main.css");
        let html = &files[0].1;
        assert!(html.contains("导出甲"));
        assert!(
            html.contains(r#"href="styles/main.css""#),
            "canonical 导出仍走外链表(与桌面一致)"
        );

        // 再导入导出产物 → 编辑存活(证明 Web 导出不是一次性字符串)
        let css = files[1].1.clone();
        let mut core2 = WebCore::new();
        core2.load_project(html, Some(&css)).unwrap();
        let rows2: Vec<serde_json::Value> = serde_json::from_str(&core2.text_nodes_json()).unwrap();
        assert_eq!(rows2[0]["text"], "导出甲");
    }

    /// 错误面:不存在的 sid / 非文本节点,显式报错不 panic。
    #[test]
    fn set_text_errors_are_honest() {
        let mut core = WebCore::new();
        core.load_project(&fixture_html(), Some(&fixture_css()))
            .unwrap();
        assert!(core.set_text("zzz", "x").is_err(), "不存在 sid 必须报错");
    }
}
