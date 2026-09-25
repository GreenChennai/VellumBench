// VellumBench Web 壳(K2,05-11-2)—— 纯静态页,无框架无构建步骤。
// 职责:文件进出(fetch/下载/文件选择器)、状态渲染;文档语义全部在
// Rust 侧(vb_doc 命令路径),JS 不自己改 HTML。
import init, { VbWebApp, register_font } from "./pkg/vb_web.js";

const $ = (id) => document.getElementById(id);
const status = (msg, cls = "") => {
  const line = document.createElement("div");
  if (cls) line.className = cls;
  line.textContent = msg;
  $("status").appendChild(line);
};

let app = null;
let artboards = [];
let textNodes = [];
let activeSid = null;
let dirty = false;

async function boot() {
  await init();
  app = new VbWebApp();
  $("btn-apply").disabled = false;
  $("file-input").addEventListener("change", onPickFiles);
  $("font-input").addEventListener("change", onPickFonts);
  $("btn-example").addEventListener("click", loadExample);
  $("btn-apply").addEventListener("click", applyText);
  $("btn-export").addEventListener("click", exportFiles);
  $("btn-png").addEventListener("click", exportPng);
  status("wasm 已就绪。", "ok");
  const q = new URLSearchParams(location.search);
  if (q.get("autotest") === "1") await autotest();
}

// ---- 载入 ----

async function loadExample() {
  const html = await (await fetch("./example/index.html")).text();
  const cssResp = await fetch("./example/styles/main.css");
  const css = cssResp.ok ? await cssResp.text() : null;
  loadInto(html, css, "内置示例");
}

async function onPickFiles(ev) {
  const files = [...ev.target.files];
  const html = files.find((f) => f.name === "index.html");
  if (!html) return status("未选择 index.html", "err");
  const cssFile =
    files.find((f) => f.name.endsWith(".css") && f.webkitRelativePath.includes("styles")) ??
    files.find((f) => f.name.endsWith(".css"));
  const css = cssFile ? await cssFile.text() : null;
  loadInto(await html.text(), css, html.name);
}

async function loadInto(html, css, tag) {
  const summary = JSON.parse(app.load_project(html, css));
  artboards = summary.artboards;
  textNodes = JSON.parse(app.text_nodes_json());
  dirty = false;
  status(
    `[${tag}] 已载入「${summary.title}」:画板 ${artboards.length},文本 ${textNodes.length}` +
      (summary.warnings.length ? `\n告警:${summary.warnings.join("; ")}` : ""),
    summary.warnings.length ? "warn" : "ok"
  );
  renderArtboards();
  renderTexts();
  $("btn-export").disabled = false;
  $("btn-png").disabled = artboards.length === 0;
  activeSid = artboards[0]?.sid ?? null;
  await refreshPreview();
}

// ---- 展示 ----

function renderArtboards() {
  const box = $("artboards");
  box.innerHTML = "";
  for (const ab of artboards) {
    const b = document.createElement("button");
    b.textContent = ab.name;
    b.onclick = () => {
      activeSid = ab.sid;
      refreshPreview();
    };
    box.appendChild(b);
  }
  if (!artboards.length) box.textContent = "— 无画板 —";
}

function renderTexts() {
  const sel = $("text-select");
  sel.innerHTML = "";
  for (const t of textNodes) {
    const o = document.createElement("option");
    o.value = t.sid;
    o.textContent = `${t.name || "文本"} (${t.sid})`;
    o.dataset.text = t.text;
    sel.appendChild(o);
  }
  sel.onchange = () => {
    $("text-editor").value = sel.selectedOptions[0]?.dataset.text ?? "";
  };
  if (textNodes.length) sel.onchange();
}

async function refreshPreview() {
  if (!app) return;
  $("preview").srcdoc = app.preview_html(activeSid);
}

// ---- 轻编辑 / 导出 ----

function applyText() {
  const sid = $("text-select").value;
  if (!sid) return status("先选中文本节点", "warn");
  const text = $("text-editor").value;
  try {
    app.set_text(sid, text);
    const node = textNodes.find((t) => t.sid === sid);
    if (node) node.text = text;
    // 更新选项里的快照,重选时不丢
    $("text-select").selectedOptions[0].dataset.text = text;
    dirty = true;
    status(`已应用 SetText(${sid});rev = ${app.rev()}`, "ok");
    refreshPreview();
  } catch (e) {
    status(`SetText 失败:${e}`, "err");
  }
}

function download(name, data, mime) {
  const blob = data instanceof Blob ? data : new Blob([data], { type: mime });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name;
  a.click();
  URL.revokeObjectURL(a.href);
}

function exportFiles() {
  const files = JSON.parse(app.export_files_json());
  for (const f of files) {
    download(f.path.split("/").pop(), f.content, "text/html");
  }
  status(`已导出 ${files.map((f) => f.path).join(" + ")}${dirty ? "(含未保存编辑)" : ""}`, "ok");
}

async function onPickFonts(ev) {
  for (const f of ev.target.files) {
    const bytes = new Uint8Array(await f.arrayBuffer());
    const family = f.name.replace(/\.[^.]+$/, "");
    register_font(family, 400, bytes); // 简化:统一按 400;粗体边界见能力文档
    status(`字体已注册:${family}(${bytes.length} 字节)`, "ok");
  }
}

async function exportPng() {
  if (!activeSid) return status("先载入项目", "warn");
  try {
    const png = app.raster_png(activeSid, 2.0);
    download(`artboard-${activeSid}.png`, new Blob([png], { type: "image/png" }));
    status(`PNG 已导出(画板 ${activeSid} @2x)`, "ok");
  } catch (e) {
    status(`光栅失败:${e}`, "err");
  }
}

// ---- 自动验收(?autotest=1):载入 → 切画板 → 改文字 → 导出含编辑。
// 无头浏览器验证以 #status 的机械结果 + 截图为准。

async function autotest() {
  const mark = (ok, name) => status(`[autotest] ${ok ? "PASS" : "FAIL"} ${name}`, ok ? "ok" : "err");
  try {
    const html = await (await fetch("./example/index.html")).text();
    const cssResp = await fetch("./example/styles/main.css");
    const css = cssResp.ok ? await cssResp.text() : null;
    await loadInto(html, css, "autotest"); // 走同一条 UI 载入路径(面板同步填充)
    const summary = JSON.parse(app.load_project(html, css)); // 断言用摘要
    mark(summary.artboards.length > 0, "载入示例(画板 > 0)");
    const abs = JSON.parse(app.artboards_json());
    activeSid = abs[0].sid;
    await refreshPreview();
    const shown = app.preview_html(activeSid);
    mark(
      shown.includes(`data-vb-id="${activeSid}"`) && shown.includes("display:none"),
      "切换画板(只显规则注入)"
    );
    const texts = JSON.parse(app.text_nodes_json());
    const target = texts[0];
    const edited = target.text + "·改";
    app.set_text(target.sid, edited);
    mark(app.preview_html(null).includes(edited), "轻编辑进预览(SetText 生效)");
    const files = JSON.parse(app.export_files_json());
    mark(files.some((f) => f.content.includes(edited)), "导出产物含编辑(字节同源)");
    mark(app.rev() > 0, "rev 随编辑推进");
    window.__AUTOTEST_DONE__ = true;
  } catch (e) {
    status(`[autotest] FAIL 异常:${e}`, "err");
  }
}

boot();
