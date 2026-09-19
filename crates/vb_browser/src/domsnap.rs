//! DOM 快照采集(ADR-0021):settle 后注入 domsnap.js,产出 PaintList JSON。
//!
//! 采集原则:浏览器只给「布局与测量」——元素矩形/样式/逐行文本 run;
//! 矢量结构与文本发射完全交给 vb_kiln 写入器(根除 Type3/逐字 Tj/clip 泛滥)。

use serde_json::Value;

use crate::page::PageSession;

/// 采集脚本(IIFE,returnByValue 返回 PaintList JSON)。
pub const DOMSNAP_JS: &str = r#"(() => {
const sx = window.scrollX || 0, sy = window.scrollY || 0;
const doc = document;
const W = Math.max(doc.documentElement.scrollWidth, doc.body ? doc.body.scrollWidth : 0);
const H = Math.max(doc.documentElement.scrollHeight, doc.body ? doc.body.scrollHeight : 0);
const items = [];
let textLineCount = 0;
let clipDemand = 0;   // 写入侧可能需要 clip 的项(渐变/圆角图片)
const rasterRects = [];  // 需位图降级的区域(采集后在 Rust 侧截图裁剪)

const SKIP = new Set(['SCRIPT','STYLE','LINK','META','TITLE','HEAD','NOSCRIPT','TEMPLATE']);

function rect(r) { return [+(r.left + sx).toFixed(2), +(r.top + sy).toFixed(2),
                           +r.width.toFixed(2), +r.height.toFixed(2)]; }
function alphaColor(c) {
  const m = /rgba?\(([^)]+)\)/.exec(c || '');
  if (!m) return null;
  const p = m[1].split(/[,\/\s]+/).filter(Boolean).map(Number);
  if (p.length < 3) return null;
  const a = p.length > 3 ? p[3] : 1;
  if (a <= 0.01) return null;
  return [p[0]/255, p[1]/255, p[2]/255, +a.toFixed(3)];
}
function radii4(cs, w, h) {
  const tl = parseFloat(cs.borderTopLeftRadius) || 0;
  const out = [tl, parseFloat(cs.borderTopRightRadius) || 0,
               parseFloat(cs.borderBottomRightRadius) || 0,
               parseFloat(cs.borderBottomLeftRadius) || 0];
  // % 半径换算(圆角圆形按钮)
  return out.map((v, i) => v > 0 && String(cs.borderTopLeftRadius).includes('%')
      ? v * (i % 2 === 0 ? h : w) / 100 : v).map(v => +v.toFixed(2));
}
function clipAncestor(el) {
  let p = el.parentElement;
  while (p && p !== doc.body) {
    const cs = getComputedStyle(p);
    if (cs.overflow !== 'visible' || cs.overflowX !== 'visible' || cs.overflowY !== 'visible') {
      const r = p.getBoundingClientRect();
      return rect(r);
    }
    p = p.parentElement;
  }
  return null;
}
function hasEffect(cs) {
  const f = cs.filter || '';
  return (cs.mixBlendMode && cs.mixBlendMode !== 'normal')
      || (f && f !== 'none')
      || (cs.backdropFilter && cs.backdropFilter !== 'none')
      || (cs.maskImage && cs.maskImage !== 'none')
      || (cs.webkitMaskImage && cs.webkitMaskImage !== 'none')
      || (cs.clipPath && cs.clipPath !== 'none');
}
function matrix(cs) {
  const t = cs.transform;
  if (!t || t === 'none') return null;
  const m = /matrix\(([^)]+)\)/.exec(t);
  if (!m) return null;
  return m[1].split(',').map(v => +v.trim());
}

// ---- background-clip:text 的取色:color 多为透明,用渐变中点近似 ----
function gradientMid(bi) {
  if (!bi || !/gradient\(/.test(bi)) return null;
  const cols = [...bi.matchAll(/rgba?\(([^)]+)\)/g)].map(m => alphaColor('rgba(' + m[1] + ')')).filter(Boolean);
  if (!cols.length) return null;
  const c = cols[Math.floor(cols.length / 2)];
  return c;
}
function textColorOf(pcs) {
  const c = alphaColor(pcs.color);
  const clippedText = (pcs.webkitBackgroundClip === 'text' || pcs.backgroundClip === 'text');
  if (c && c[3] > 0.05 && !clippedText) return c;
  if (clippedText) { const g = gradientMid(pcs.backgroundImage); if (g) return g; }
  if (c && c[3] > 0.05) return c;
  // 透明字:同元素渐变兜底,再退黑
  return gradientMid(pcs.backgroundImage) || [0, 0, 0, 1];
}

// ---- 文本行采集:整块容器 + 逐行切分 + 跨 inline 子元素合并 ----
function textBase(pcs) {
  const fs = parseFloat(pcs.fontSize);
  return {
    family: pcs.fontFamily.split(',')[0].replace(/["']/g, '').trim(),
    size: +fs.toFixed(2), weight: parseInt(pcs.fontWeight) || 400,
    color: textColorOf(pcs),
    ls: pcs.letterSpacing === 'normal' ? 0 : +(parseFloat(pcs.letterSpacing) || 0),
    lh: pcs.lineHeight === 'normal' ? 0 : +parseFloat(pcs.lineHeight).toFixed(2),
    opacity: +pcs.opacity, blend: pcs.mixBlendMode !== 'normal' ? pcs.mixBlendMode : null,
    matrix: matrix(pcs),
  };
}
// 单个文本节点 → 逐视觉行 run(与原逻辑同口径)
function nodeRuns(node) {
  const out = [];
  const raw = node.nodeValue;
  if (!raw || !raw.trim()) return out;
  const pcs = getComputedStyle(node.parentElement);
  if (pcs.display === 'none' || pcs.visibility === 'hidden') return out;
  const base = textBase(pcs);
  const fs = base.size;
  const range = doc.createRange();
  range.selectNodeContents(node);
  const whole = range.getClientRects();
  const emit = (text, chars) => {
    if (!text) return;
    const L = Math.min(...chars.map(c => c.left)), R = Math.max(...chars.map(c => c.right));
    const T = Math.min(...chars.map(c => c.top)), B = Math.max(...chars.map(c => c.bottom));
    out.push(Object.assign({}, base, {
      text, rect: [+(L + sx).toFixed(2), +(T + sy).toFixed(2), +(R - L).toFixed(2), +(B - T).toFixed(2)],
    }));
  };
  if (whole.length <= 1) {
    const r = whole[0] || range.getBoundingClientRect();
    if (r.width > 0 && r.height > 0) {
      emit(raw.replace(/\s+/g, ' ').trim(),
           [{left: r.left, top: r.top, right: r.right, bottom: r.bottom}]);
    }
    return out;
  }
  // 多行:逐字符 Range 行分组
  const lines = [];
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (/\s/.test(ch)) { if (lines.length) lines[lines.length-1].text += ch; continue; }
    range.setStart(node, i); range.setEnd(node, i + 1);
    const r = range.getBoundingClientRect();
    if (!r.width && !r.height) continue;
    let ln = lines.find(l => Math.abs(l.top - r.top) < Math.max(r.height * 0.5, fs * 0.5)
        && Math.abs((l.fs || fs) - fs) < Math.max(1, fs * 0.12));
    if (!ln) { ln = {top: r.top, fs: fs, text: '', chars: []}; lines.push(ln); }
    ln.text += ch; ln.chars.push({left: r.left, top: r.top, right: r.right, bottom: r.bottom});
  }
  for (const ln of lines) emit(ln.text.replace(/\s+/g, ' ').trim(), ln.chars);
  return out;
}
// 整棵子树文本 → 逐行 run,再把同一视觉行的多个 run 合成一条字符串
// (根因:collectText 原只取「直接子文本节点」,`覆盖<b>17+</b>AI 流量入口`
//  被拆成两条独立 run → 写入器按硬换行排成两行,整行字符串被打断)
function collectText(nodes) {
  const pieces = [];
  for (const n of nodes) {
    const p = n.parentElement;
    if (!p || SKIP.has(p.tagName)) continue;
    for (const r of nodeRuns(n)) pieces.push(r);
  }
  const merged = [];
  for (const r of pieces) {
    const m = merged[merged.length - 1];
    if (m) {
      const ov = Math.min(m.rect[1] + m.rect[3], r.rect[1] + r.rect[3])
               - Math.max(m.rect[1], r.rect[1]);
      // 垂直重叠过半 → 同一视觉行(跨 inline 元素);否则是新行
      if (ov > Math.min(m.rect[3], r.rect[3]) * 0.5) {
        const gap = r.rect[0] - (m.rect[0] + m.rect[2]);
        if (gap > r.size * 0.15) m.text += ' ';   // 原本的空白已被 trim,按间距补回
        m.text += r.text;
        const L = Math.min(m.rect[0], r.rect[0]), T = Math.min(m.rect[1], r.rect[1]);
        const R = Math.max(m.rect[0] + m.rect[2], r.rect[0] + r.rect[2]);
        const B = Math.max(m.rect[1] + m.rect[3], r.rect[1] + r.rect[3]);
        m.rect = [L, T, R - L, B - T];
        continue;
      }
    }
    merged.push(Object.assign({}, r));
  }
  textLineCount += merged.length;
  return merged;
}
function isBlockLevel(cs) {
  const d = cs.display;
  return d !== 'inline' && d !== 'inline-block' && d !== 'inline-flex' && d !== 'contents';
}
// 「本块自己的」文本节点:向下穿过 inline 子元素,遇到块级子元素即停
// (那些文字归那个块自己发射)。于是每个文本节点的最近块级祖先**恰好一个**,
// 全局发射一次:既不重复(实测五连印),也不丢(块内还有块时,块自身的
// 直接文本节点曾整段丢失——二维码海报丢了一句)
function ownTextNodes(el, out) {
  for (const n of el.childNodes) {
    if (n.nodeType === 3) {
      if (n.nodeValue && n.nodeValue.trim()) out.push(n);
      continue;
    }
    if (n.nodeType !== 1) continue;
    if (SKIP.has(n.tagName)) continue;
    const cs = getComputedStyle(n);
    if (cs.display === 'none' || cs.visibility === 'hidden') continue;
    if (isBlockLevel(cs)) continue;
    ownTextNodes(n, out);
  }
  return out;
}

// ---- 深度优先遍历(绘制序) ----
const area = W * H;
function walk(el, layerOfParent) {
  if (SKIP.has(el.tagName)) return;
  const cs = getComputedStyle(el);
  if (cs.display === 'none' || cs.visibility === 'hidden') return;
  const r = el.getBoundingClientRect();
  const visible = r.width > 0.5 && r.height > 0.5
      && r.bottom + sy > 0 && r.right + sx > 0;
  if (!visible) return;
  const rectDoc = rect(r);
  // 图层分类:覆盖 ≥60% 画板的封面盒 → 背景(0);BODY/HTML 恒背景
  // (此前 BODY 被排除在外,`html,body{background:#fff}` 的整页白底落到
  //  内容层并被最后绘制,直接把背景图/主渐变整块盖掉)
  const cover = (r.width * r.height) >= area * 0.6
      || el.tagName === 'BODY' || el.tagName === 'HTML';
  const layer = cover ? 0 : 1;
  const _lp = layerOfParent; void _lp;

  const common = { rect: rectDoc, layer, opacity: +cs.opacity, matrix: matrix(cs),
                   clip: clipAncestor(el) };
  const effect = hasEffect(cs);
  const isImg = el.tagName === 'IMG';
  const isSvg = el.tagName === 'svg' || el.querySelector?.('svg') === null && false;

  if (el.tagName === 'svg') {
    items.push(Object.assign({}, common, { kind: 'svg', markup: el.outerHTML.slice(0, 200000) }));
    if (effect) clipDemand++;
    return; // svg 子树不深入
  }
  if (isImg) {
    const rad = radii4(cs, r.width, r.height);
    const rawSrc = el.currentSrc || el.src || '';
    // SVG 资产:<img src=*.svg> 无法用位图解码器读入,按整页截图裁剪保留
    // 真实外观(反而比「缺失占位」保真;真矢量化列 carry-forward)
    const svgSrc = /\.svg(\?|#|$)/i.test(rawSrc) || /^data:image\/svg/i.test(rawSrc);
    if (rad.some(v => v > 0.5) || effect || svgSrc) {
      items.push(Object.assign({}, common, { kind: 'raster',
        reason: effect ? 'effect-image' : (svgSrc ? 'svg-image' : 'rounded-image') }));
      rasterRects.push(rectDoc); clipDemand++;
    } else {
      items.push(Object.assign({}, common, { kind: 'image',
        src: rawSrc, natural: [el.naturalWidth, el.naturalHeight] }));
    }
    return;
  }
  // 背景盒(effect 元素的背景省略:纯图形走位图化,含文字靠救活近似,均防重影)
  const bg = alphaColor(cs.backgroundColor);
  const bi = cs.backgroundImage;
  const grad = /gradient\(/.test(bi) ? bi : null;
  const bgUrl = !grad && bi && bi !== 'none' ? (/url\(["']?([^"')]+)["']?\)/.exec(bi) || [])[1] : null;
  const border = (() => {
    const w = parseFloat(cs.borderTopWidth) || 0;
    if (w <= 0) return null;
    const same = [cs.borderRightWidth, cs.borderBottomWidth, cs.borderLeftWidth]
        .every(v => Math.abs(parseFloat(v) - w) < 0.51)
      && [cs.borderRightColor, cs.borderBottomColor, cs.borderLeftColor].every(c => c === cs.borderTopColor);
    const c = alphaColor(cs.borderTopColor);
    if (!c) return null;
    return same ? { width: +w.toFixed(2), color: c } : { uneven: true };
  })();
  const rad = radii4(cs, r.width, r.height);
  const shadow = cs.boxShadow && cs.boxShadow !== 'none' ? cs.boxShadow : null;
  // effect 分档(实测教训:整块省略背景会丢 CTA 按钮等大视觉):
  //   blend/filter/backdrop → 背景照画(混合/滤镜效果丢弃,视觉近似);
  //   mask/clipPath 且无文本 → 整体位图化(区域裁剪最保真)
  const heavyEffect = (cs.maskImage && cs.maskImage !== 'none')
      || (cs.webkitMaskImage && cs.webkitMaskImage !== 'none')
      || (cs.clipPath && cs.clipPath !== 'none');
  if (heavyEffect) {
    const hasText = [...el.childNodes].some(n => n.nodeType === 3 && n.nodeValue.trim())
      || [...el.querySelectorAll('*')].some(c =>
           [...c.childNodes].some(n => n.nodeType === 3 && n.nodeValue.trim()));
    if (!hasText) {
      items.push(Object.assign({}, common, { kind: 'raster', reason: 'mask-clip' }));
      rasterRects.push(rectDoc); clipDemand++;
      return; // 子树被区域位图覆盖
    }
  }
  if (bg || grad || bgUrl || border) {
    if (grad) clipDemand++;
    items.push(Object.assign({}, common, {
      kind: 'box', bg: bg, gradient: grad, bgUrl: bgUrl || null,
      border: border, radii: rad, shadow: shadow,
    }));
  }
  // 文本(块级容器发射「自己的」文本节点一次;整行字符串跨 inline 合并)
  if (isBlockLevel(cs)) {
    const own = ownTextNodes(el, []);
    if (own.length) {
      const runs = collectText(own);
      if (runs.length) {
        items.push(Object.assign({}, common, { kind: 'text', runs: runs }));
      }
    }
  }
  for (const child of el.children) walk(child, layer);
}
walk(doc.body, 1);
if (!items.length && doc.body) {
  const cs = getComputedStyle(doc.body);
  items.push({ kind: 'box', rect: [0, 0, W, H], layer: 0, opacity: 1,
    bg: alphaColor(cs.backgroundColor), gradient: null, bgUrl: null,
    border: null, radii: [0,0,0,0], shadow: null, matrix: null, clip: null });
}
const bodyW = doc.body ? doc.body.getBoundingClientRect().width : 0;
return { viewport: [W, H], bodyWidth: +bodyW.toFixed(2), layers: ['背景', '内容'], items,
         textLineCount, clipDemand, rasterRects };
})()"#;

/// 执行采集,返回 PaintList JSON。
pub fn snapshot(page: &mut PageSession) -> Result<Value, String> {
    page.evaluate(DOMSNAP_JS, false)
}
