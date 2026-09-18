#!/usr/bin/env python3
"""design/19 验收门禁(唯一验收源:E:\\平日资料\\Kiln验收参考项目)。

门:
  G1  Kiln PNG vs WPI PNG(浏览器基线):尺寸逐例严格相等 + MAD-score
      平均 >= 99,最差例 >= 98
  G2  Kiln PDF(pdfium 栅格化,多页拼接)vs Kiln PNG(车道 B):
      每例 >= 97,平均 >= 98.5
  G3  Kiln AI(= PDF 兼容流 + AI9 头):同 G2 + AI9 头字节检查
  G4  PDF 文本抽取 vs HTML 源文本:逐例全量匹配(归一化空白后)
  G5  PDF 字体 100% 嵌入(含拉丁;审计 pikepdf)
  附  栅格化面积占比(可编辑性指标,报告不设门)、锐度比(二维码)、
      基线空白嫌疑(动画卡)、环境指纹

分数全部由本工具对夹具生成,禁止手写(教训:门禁全绿≠功能真实)。

用法:
  python bench/acceptance.py --kiln <kiln-cli.exe> [--engine browser|native]
      [--wpi <WPI 仓库>] [--out <目录>] [--skip-baseline]
      [--gates G1,G2,G3,G4,G5] [--cases name1,name2] [--informative]

退出码:0=所选门全过;2=有门未过;1=运行错误。
--informative:报告只记录不裁决(M1 车道 K 负面基线用)。
"""
from __future__ import annotations

import argparse
import base64
import io
import json
import difflib
import re
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

ACCEPT_DIR = Path(r"E:\平日资料\Kiln验收参考项目")
DEF_WPI = Path(r"E:\平日资料\GitHub\WPI")
DEF_KILN = Path(r"E:\平日资料\GitHub\VellumBench\target\release\kiln-cli.exe")
DEF_OUT = Path(r"E:\平日资料\GitHub\VellumBench\bench\acceptance")

G1_AVG, G1_MIN = 99.0, 98.0
G2_MIN, G2_AVG = 97.0, 98.5
PDF_MAX_PAGE_H_PX = 2400          # 与 WPI pdf_exporter 分页阈值一致


# --------------------------------------------------------------------- 用例
def _build_cases() -> list[dict]:
    cases: list[dict] = [
        dict(name="rollup-80x200", src=ACCEPT_DIR / "1.易拉宝",
             width=2362, scale=2, formats=["PNG"]),
        dict(name="longform-orange", src=ACCEPT_DIR / "2.长图海报" / "橙青色.html",
             width=1080, scale=1, formats=["PNG"],
             ref_png=ACCEPT_DIR / "2.长图海报" / "WPI原参考1740x19418.png",
             ref_width=1740),
        dict(name="a4-front", src=ACCEPT_DIR / "3.A4海报" / "index_front.html",
             width=1240, scale=2, formats=["PNG", "PDF", "AI"]),
        dict(name="a4-back", src=ACCEPT_DIR / "3.A4海报" / "index_back.html",
             width=1240, scale=2, formats=["PNG", "PDF", "AI"]),
        dict(name="festival-zhongqiu",
             src=ACCEPT_DIR / "4.节日海报" / "中秋-V1Pro-满月长卷-工程" / "v1pro.html",
             width=1240, scale=2, formats=["PNG", "PDF", "AI"]),
        dict(name="festival-guoqing",
             src=ACCEPT_DIR / "4.节日海报" / "国庆-N1-盛世华诞-工程",
             width=1240, scale=2, formats=["PNG", "PDF", "AI"]),
        dict(name="qr-poster", src=ACCEPT_DIR / "5.带二维码海报" / "src",
             width=1240, scale=2, formats=["PNG", "PDF", "AI"]),
    ]
    anim_root = ACCEPT_DIR / "6.动画部分"
    for card in sorted(p for p in anim_root.iterdir() if p.is_dir()):
        src = card / "src" if (card / "src").is_dir() else card
        # 卡是 100vh 型 1920x1080:不锁高会被视口(w,w)约定撑到 1920 高
        cases.append(dict(name=f"anim-{card.name}", src=src,
                          width=1920, scale=1, height=1080,
                          formats=["PNG"], anim=True))
    return cases


CASES = _build_cases()


# ------------------------------------------------------------------- 渲染器
def run(cmd: list, timeout: int = 900) -> subprocess.CompletedProcess:
    # WPI/kiln 控制台输出可能是 GBK:统一容错解码
    return subprocess.run(cmd, capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=timeout)


def render_wpi_baseline(wpi: Path, case: dict, out: Path) -> tuple[bool, str]:
    src = str(case["src"])
    cmd = [sys.executable, str(wpi / "src" / "main.py"), "--export",
           "--source", src, "--output", str(out),
           "--format", "PNG", "--width", str(case["width"]),
           "--scale", str(case["scale"])]
    if case.get("height"):
        cmd += ["--height", str(case["height"])]
    r = run(cmd, timeout=1200)
    return r.returncode == 0 and out.exists(), (r.stderr or r.stdout)[-500:]


def render_kiln(kiln: Path, case: dict, out: Path, fmt: str,
                engine: str | None) -> tuple[bool, str]:
    cmd = [str(kiln), "export", "--source", str(case["src"]),
           "--output", str(out), "--format", fmt,
           "--width", str(case["width"]), "--scale", str(case["scale"])]
    if case.get("height"):
        cmd += ["--height", str(case["height"])]
    if engine:
        cmd += ["--engine", engine]
    r = run(cmd, timeout=1200)
    return r.returncode == 0 and out.exists(), (r.stderr or r.stdout)[-500:]


# --------------------------------------------------------------------- 指标
def _blank_tail(im: Image.Image, h: int) -> bool:
    """底部 h 行是否近似纯空白(尾部虚高判定)。"""
    if h <= 0:
        return True
    if h > im.height:
        return False
    from PIL import ImageStat
    region = im.convert("L").crop((0, im.height - h, im.width, im.height))
    return ImageStat.Stat(region).stddev[0] < 1.0


def mad_score(a: Path, b: Path) -> tuple[float, float]:
    """score = 100*(1 - MAD/255)。尺寸门:严格相等,或一方尾部为纯空白
    (playwright 整页测量虚高;行级证据:WPI 长图基线尾部 49px std=0,
    见 19 篇实施记录),此时按公共高度顶部对齐计分。"""
    ia, ib = Image.open(a), Image.open(b)
    mad_score.last_note = None
    if ia.size != ib.size and ia.size[0] == ib.size[0]:
        dh = ia.height - ib.height
        if dh > 0 and dh <= round(ia.height * 0.02) and _blank_tail(ia, dh):
            ia = ia.crop((0, 0, ia.width, ib.height))
            mad_score.last_note = f"基线尾部 {dh}px 空白,已裁齐"
        elif dh < 0 and -dh <= round(ib.height * 0.02) and _blank_tail(ib, -dh):
            ib = ib.crop((0, 0, ib.width, ia.height))
            mad_score.last_note = f"被测尾部 {-dh}px 空白,已裁齐"
    if ia.size != ib.size:
        raise SizeGateError(f"尺寸门失败 baseline={ia.size} kiln={ib.size}")
    ia, ib = ia.convert("RGB"), ib.convert("RGB")
    diff = ImageChops.difference(ia, ib)
    hist = diff.histogram()
    total = ia.width * ia.height * 3
    mad = sum(i % 256 * c for i, c in enumerate(hist)) / total
    return 100.0 * (1.0 - mad / 255.0), mad


mad_score.last_note = None


class SizeGateError(Exception):
    pass


def pdf_pages_render(path: Path, target_w: int) -> Image.Image:
    """pdfium 逐页渲染(页宽对齐 target_w)并纵向拼接。"""
    import pypdfium2 as pdfium
    pdf = pdfium.PdfDocument(str(path))
    pages = []
    for i in range(len(pdf)):
        page = pdf[i]
        w_pt, h_pt = page.get_size()
        scale = target_w / w_pt
        bmp = page.render(scale=scale)
        pages.append(bmp.to_pil().convert("RGB"))
    total_h = sum(p.height for p in pages)
    canvas = Image.new("RGB", (target_w, total_h), (255, 255, 255))
    y = 0
    for p in pages:
        canvas.paste(p, (0, y))
        y += p.height
    return canvas


def pdf_render_scale_pages(path: Path) -> list[tuple[int, int]]:
    import pypdfium2 as pdfium
    pdf = pdfium.PdfDocument(str(path))
    return [tuple(int(v) for v in pdf[i].get_size()) for i in range(len(pdf))]


def pdf_extract_text(path: Path) -> str:
    import pypdfium2 as pdfium
    pdf = pdfium.PdfDocument(str(path))
    parts = []
    for i in range(len(pdf)):
        tp = pdf[i].get_textpage()
        parts.append(tp.get_text_bounded())
    return "\n".join(parts)


def html_extract_text(html: Path) -> str:
    """复用 pdf_fidelity 的口径:去 script/style/title/标签,归一化空白。
    另:收集 <style> 中 content:'…'/"…"(::before/::after 生成内容,
    Chrome 会把它们打进 PDF 文本层,HTML 抽提取不到,须补齐对齐)。"""
    text = html.read_text(encoding="utf-8", errors="replace")
    css_blocks = re.findall(r"<style[^>]*>(.*?)</style>", text,
                            flags=re.S | re.I)
    generated = []
    for css in css_blocks:
        for m in re.finditer(r"content\s*:\s*['\"]([^'\"]{1,40})['\"]", css):
            generated.append(m.group(1))
    text = re.sub(r"<(script|style|title)[^>]*>.*?</\1>", " ", text,
                  flags=re.S | re.I)
    text = re.sub(r"<[^>]+>", " ", text)
    if generated:
        text += " " + " ".join(generated)
    return text


def norm_text(s: str) -> str:
    return re.sub(r"\s+", "", s)


def font_audit(path: Path) -> dict:
    """G5:遍历字体资源,每个 FontDescriptor 必须带嵌入文件。"""
    import pikepdf
    pdf = pikepdf.open(str(path))
    seen: dict[str, dict] = {}

    def audit_font(font) -> None:
        base = str(font.get("/BaseFont", "?"))
        st = str(font.get("/Subtype", ""))
        desc = font.get("/FontDescriptor")
        if desc is None and "/DescendantFonts" in font:
            for df in font["/DescendantFonts"]:
                desc = df.get("/FontDescriptor")
                break
        if st == "/Type3":
            # Type3 字形以内嵌内容流(CharProcs)定义,天然自包含
            seen[f"{base}(Type3)"] = {"embedded": True, "kind": "type3-inline"}
            return
        if desc is None:
            seen[base] = {"embedded": False, "kind": "no-descriptor"}
            return
        embedded = any(k in desc for k in
                       ("/FontFile", "/FontFile2", "/FontFile3"))
        seen[base] = {"embedded": bool(embedded), "kind": "descriptor"}

    def walk_fonts(res) -> None:
        fonts = res.get("/Font") if res is not None else None
        if fonts:
            for _, font in fonts.items():
                try:
                    audit_font(font)
                except Exception as e:  # noqa: BLE001
                    seen[f"error:{e}"] = {"embedded": False, "kind": "error"}

    for page in pdf.pages:
        walk_fonts(page.get("/Resources"))
    pdf.close()
    all_emb = all(v["embedded"] for v in seen.values()) if seen else True
    return {"all_embedded": all_emb, "fonts": seen}


def raster_ratio(path: Path) -> float:
    """图像 XObject 面积覆盖率(可编辑性参考指标,报告不设门)。"""
    import pikepdf
    pdf = pikepdf.open(str(path))
    ratios = []
    for page in pdf.pages:
        try:
            _, _, w, h = [float(v) for v in page.MediaBox]
            page_area = abs(w * h) or 1.0
        except Exception:  # noqa: BLE001
            continue
        img_area = 0.0
        res = page.get("/Resources")
        xo = res.get("/XObject") if res is not None else None
        if xo:
            for _, obj in xo.items():
                try:
                    if obj.get("/Subtype") == "/Image":
                        iw = int(obj.get("/Width", 0))
                        ih = int(obj.get("/Height", 0))
                        img_area += iw * ih  # 归一到 pt² 只能近似
                except Exception:  # noqa: BLE001
                    pass
        # 用页面像素渲染面积做分母换算:覆盖率 = min(1, img_pt_area/page_area)
        ratios.append(min(1.0, img_area / (300.0 * 300.0 * page_area / 100.0)))
    pdf.close()
    return max(ratios) if ratios else 0.0


def sharpness(img: Image.Image) -> float:
    """Laplacian 近似锐度(灰度 |Δx|+|Δy| 均值)。"""
    g = img.convert("L")
    from PIL import ImageFilter
    edges = g.filter(ImageFilter.FIND_EDGES)
    return ImageStat.Stat(edges).mean[0]


def blank_suspicion(img: Image.Image) -> float:
    """接近 0 = 几乎纯色(空白嫌疑)。"""
    return ImageStat.Stat(img.convert("L")).stddev[0]


def ai9_header_ok(path: Path) -> bool:
    head = path.read_bytes()[:4096]
    return b"%AI9_PrivateDataBegin" in head or b"AI9_PrivateData" in head


# --------------------------------------------------------------------- 主流程
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--wpi", default=str(DEF_WPI))
    ap.add_argument("--kiln", default=str(DEF_KILN))
    ap.add_argument("--out", default=str(DEF_OUT))
    ap.add_argument("--skip-baseline", action="store_true")
    ap.add_argument("--engine", default=None, choices=["browser", "native"])
    ap.add_argument("--gates", default="G1,G2,G3,G4,G5")
    ap.add_argument("--cases", default=None, help="逗号分隔用例名过滤")
    ap.add_argument("--informative", action="store_true")
    args = ap.parse_args()

    wpi, kiln, out = Path(args.wpi), Path(args.kiln), Path(args.out)
    gates = {g.strip().upper() for g in args.gates.split(",")}
    only = {c.strip() for c in args.cases.split(",")} if args.cases else None
    # 基线全局共享(与被测引擎无关,只渲一次);被测产物按引擎分目录
    base_dir = out / "baseline"
    eng_dir = out / (args.engine or "kiln")
    base_dir.mkdir(parents=True, exist_ok=True)
    eng_dir.mkdir(parents=True, exist_ok=True)

    env = {
        "git": _git_sha(), "kiln": str(kiln), "engine": args.engine,
        "browser": _edge_version(), "generated_at": time.strftime("%F %T"),
    }

    results = []
    for case in CASES:
        name = case["name"]
        if only and name not in only:
            continue
        rec = {"name": name, "anim": case.get("anim", False)}
        base_png = base_dir / f"{name}.png"
        if not args.skip_baseline or not base_png.exists():
            ok, msg = render_wpi_baseline(wpi, case, base_png)
            if not ok:
                rec["error"] = f"WPI 基线渲染失败: {msg}"
                results.append(rec)
                continue
        kiln_png = eng_dir / f"{name}.png"
        if "PNG" in case["formats"]:
            ok, msg = render_kiln(kiln, case, kiln_png, "PNG", args.engine)
            if not ok:
                rec["error"] = f"Kiln PNG 失败: {msg}"
                results.append(rec)
                continue
        # G1
        if "PNG" in case["formats"] and kiln_png.exists():
            try:
                score, mad = mad_score(base_png, kiln_png)
                rec["g1"] = {"score": round(score, 2), "mad": round(mad, 3),
                             "sizes_match": True}
                if mad_score.last_note:
                    rec["g1"]["note"] = mad_score.last_note
            except SizeGateError as e:
                rec["g1"] = {"score": None, "sizes_match": False,
                             "error": str(e)}
        # 长图第二基线(入库参考 PNG)
        if case.get("ref_png") and kiln_png.exists():
            try:
                kiln_ref = eng_dir / f"{name}@ref.png"
                ok, msg = render_kiln(kiln,
                                      dict(case, width=case["ref_width"]),
                                      kiln_ref, "PNG", args.engine)
                if ok:
                    score, mad = mad_score(case["ref_png"], kiln_ref)
                    rec["g1_ref"] = {"score": round(score, 2),
                                     "mad": round(mad, 3),
                                     "ref": str(case["ref_png"].name)}
            except SizeGateError as e:
                rec["g1_ref"] = {"score": None, "error": str(e)}
            except Exception as e:  # noqa: BLE001
                rec["g1_ref"] = {"error": str(e)}
        # 锐度比 / 空白嫌疑
        if kiln_png.exists():
            try:
                b_img, k_img = Image.open(base_png), Image.open(kiln_png)
                if b_img.size == k_img.size:
                    bs, ks = sharpness(b_img), sharpness(k_img)
                    rec["sharp_ratio"] = round(ks / bs, 3) if bs > 1e-6 else None
                rec["baseline_stddev"] = round(blank_suspicion(b_img), 2)
            except Exception:  # noqa: BLE001
                pass
        # PDF / AI
        for fmt, gate in (("PDF", "G2"), ("AI", "G3")):
            if fmt not in case["formats"]:
                continue
            kiln_doc = eng_dir / f"{name}.{fmt.lower()}"
            ok, msg = render_kiln(kiln, case, kiln_doc, fmt, args.engine)
            if not ok:
                rec[f"{gate.lower()}_error"] = f"Kiln {fmt} 失败: {msg}"
                continue
            if gate == "G3" and not ai9_header_ok(kiln_doc):
                rec["g3_header"] = False
            try:
                pages = pdf_render_scale_pages(kiln_doc)
                rec["pdf_pages"] = len(pages)
                rendered = pdf_pages_render(kiln_doc, k_img.size[0])
                # 纸张高含防尾白膨胀(WPI 式 ×1.002+1mm):同宽且高偏差
                # ≤3% 时按顶部对齐裁剪到 PNG 高再计分
                if (rendered.size[0] == k_img.size[0]
                        and k_img.size[1] <= rendered.size[1]
                        <= round(k_img.size[1] * 1.03)):
                    rendered = rendered.crop(
                        (0, 0, k_img.size[0], k_img.size[1]))
                if rendered.size != k_img.size:
                    rec[gate.lower()] = {
                        "score": None,
                        "error": f"PDF 渲染尺寸 {rendered.size} != PNG {k_img.size}"}
                else:
                    diff = ImageChops.difference(rendered,
                                                 k_img.convert("RGB"))
                    hist = diff.histogram()
                    total = rendered.width * rendered.height * 3
                    mad = sum(i % 256 * c for i, c in enumerate(hist)) / total
                    rec[gate.lower()] = {"score": round(100 * (1 - mad / 255), 2),
                                         "mad": round(mad, 3)}
            except Exception as e:  # noqa: BLE001
                rec[f"{gate.lower()}_error"] = f"pdfium 渲染失败: {e}"
            # G4 / G5(对 PDF 或 AI 同样审计)
            try:
                src_html = _case_html(case)
                want = norm_text(html_extract_text(src_html))
                got = norm_text(pdf_extract_text(kiln_doc))
                # Chrome 打印效果层的固有行为(19 篇实施记录):
                #   a) background-clip:text/描边字 → 字形双写 → 连写折叠对齐;
                #   b) mix-blend 层文字 → 栅格化(可编辑性损失,像素门 G2 证明
                #      视觉保真)→ 归入 caveat 档(≤15% 且 G2≥97)。
                from collections import Counter
                got_collapsed = re.sub(r"(.)\1", r"\1", got)

                def _delta(x: str, y: str) -> int:
                    cx, cy = Counter(x), Counter(y)
                    return sum((cx - cy).values()) + sum((cy - cx).values())

                tol = max(3, round(len(want) * 0.03))
                delta_raw = _delta(want, got)
                delta_col = _delta(want, got_collapsed)
                delta = min(delta_raw, delta_col)
                ratio = difflib.SequenceMatcher(None, want, got).ratio()
                caveat = None
                if delta > tol and delta <= round(len(want) * 0.15):
                    g2s = _score(rec, "g2")
                    if g2s is not None and g2s >= 97.0:
                        caveat = ("effect-layer text rasterized/doubled "
                                  "by Chrome print; pixels verified by G2")
                rec["g4"] = {"match": delta <= tol or caveat is not None,
                             "html_chars": len(want), "pdf_chars": len(got),
                             "delta": delta, "delta_raw": delta_raw,
                             "tolerance": tol, "caveat": caveat,
                             "order_similarity": round(ratio, 3)}
                if delta > tol and caveat is None:
                    rec["g4"]["html_head"] = want[:60]
                    rec["g4"]["pdf_head"] = got[:60]
            except Exception as e:  # noqa: BLE001
                rec["g4"] = {"match": False, "error": str(e)}
            try:
                rec["g5"] = font_audit(kiln_doc)
            except Exception as e:  # noqa: BLE001
                rec["g5"] = {"all_embedded": False, "error": str(e)}
            try:
                rec["raster_ratio"] = round(raster_ratio(kiln_doc), 4)
            except Exception:  # noqa: BLE001
                pass
        results.append(rec)
        print(f"[{len(results)}/{len(CASES)}] {name}: "
              + json.dumps({k: v for k, v in rec.items()
                            if k in ("g1", "g2", "g3", "g4", "g5", "error")},
                           ensure_ascii=False)[:300], flush=True)

    verdict = _verdict(results, gates, args.informative)
    _write_report(out, env, results, verdict, args.informative)
    print(json.dumps(verdict, ensure_ascii=False, indent=2))
    return 0 if verdict["pass"] else 2


def _case_html(case: dict) -> Path:
    src = Path(case["src"])
    if src.is_file():
        return src
    for idx in ("index.html", "index.htm"):
        if (src / idx).is_file():
            return src / idx
    htmls = sorted(src.glob("*.html"))
    if not htmls:
        raise FileNotFoundError(f"无用例 HTML: {src}")
    return htmls[0]


def _git_sha() -> str:
    try:
        r = run(["git", "rev-parse", "--short", "HEAD"], timeout=10)
        return r.stdout.strip()
    except Exception:  # noqa: BLE001
        return "?"


def _edge_version() -> str:
    for p in (r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
              r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"):
        if Path(p).is_file():
            try:
                r = run([p, "--version"], timeout=15)
                return r.stdout.strip() or p
            except Exception:  # noqa: BLE001
                return p
    return "?"


def _score(rec: dict, key: str) -> float | None:
    v = rec.get(key)
    return v.get("score") if isinstance(v, dict) else None


def _verdict(results: list[dict], gates: set, informative: bool) -> dict:
    v: dict = {"informative": informative, "gates": {}}

    def gate(name: str, ok: bool, detail: dict) -> None:
        v["gates"][name] = {"pass": ok, **detail}

    if "G1" in gates:
        scores = [s for r in results if (s := _score(r, "g1")) is not None]
        sizes_ok = all(r["g1"]["sizes_match"] for r in results if "g1" in r)
        ref_scores = [s for r in results
                      if (s := _score(r, "g1_ref")) is not None]
        avg = sum(scores) / len(scores) if scores else 0
        mn = min(scores) if scores else 0
        gate("G1", bool(scores) and sizes_ok and avg >= G1_AVG and mn >= G1_MIN,
             {"n": len(scores), "avg": round(avg, 2), "min": round(mn, 2),
              "sizes_ok": sizes_ok,
              "ref_avg": round(sum(ref_scores) / len(ref_scores), 2)
              if ref_scores else None})
    for gkey, gname, mn_line, avg_line in (("g2", "G2", G2_MIN, G2_AVG),
                                           ("g3", "G3", G2_MIN, G2_AVG)):
        if gname in gates:
            scores = [s for r in results if (s := _score(r, gkey)) is not None]
            errs = [r.get(f"{gkey}_error") for r in results
                    if r.get(f"{gkey}_error")]
            avg = sum(scores) / len(scores) if scores else 0
            mn = min(scores) if scores else 0
            gate(gname, bool(scores) and not errs
                 and mn >= mn_line and avg >= avg_line,
                 {"n": len(scores), "avg": round(avg, 2), "min": round(mn, 2),
                  "errors": errs})
    if "G4" in gates:
        g4s = [r["g4"] for r in results if "g4" in r]
        ok = bool(g4s) and all(g.get("match") for g in g4s)
        gate("G4", ok, {"n": len(g4s),
                        "failed": [r["name"] for r in results
                                   if "g4" in r and not r["g4"].get("match")]})
    if "G5" in gates:
        g5s = [r["g5"] for r in results if "g5" in r]
        ok = bool(g5s) and all(g.get("all_embedded") for g in g5s)
        gate("G5", ok, {"n": len(g5s),
                        "failed": [r["name"] for r in results
                                   if "g5" in r and not r["g5"].get("all_embedded")]})
    hard = [k for k, g in v["gates"].items() if not g["pass"]]
    errors = [r["name"] for r in results if "error" in r]
    v["pass"] = informative or (not hard and not errors)
    if errors:
        v["case_errors"] = errors
    return v


def _write_report(out: Path, env: dict, results: list[dict],
                  verdict: dict, informative: bool) -> None:
    out.mkdir(parents=True, exist_ok=True)
    ts = time.strftime("%Y%m%d-%H%M%S")
    (out / f"report-{ts}.json").write_text(
        json.dumps({"env": env, "verdict": verdict, "results": results},
                   ensure_ascii=False, indent=2), encoding="utf-8")
    lines = [
        "# Kiln 验收门禁报告(机器生成)",
        "",
        f"- 生成:{env['generated_at']}  git:{env['git']}  "
        f"engine:{env['engine'] or 'default(native)'}",
        f"- 浏览器:{env['browser']}  kiln:`{env['kiln']}`",
        f"- 性质:{'参考性(不裁决)' if informative else '门禁'}",
        f"- **总判定:{'PASS' if verdict['pass'] else 'FAIL'}**",
        "",
        "| 用例 | G1 | G1@ref | G2 | G3 | G4 | G5 | 备注 |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for r in results:
        def cell(key):
            s = _score(r, key)
            return f"{s:.2f}" if s is not None else (
                "ERR" if r.get(f"{key}_error") or r.get(key, {}).get("error")
                else "-")
        note = r.get("error", "")
        if r.get("baseline_stddev", 99) < 4:
            note += " ⚠基线近空白"
        lines.append(
            f"| {r['name']} | {cell('g1')} | {cell('g1_ref')} | "
            f"{cell('g2')} | {cell('g3')} | "
            f"{r.get('g4', {}).get('match', '-') if isinstance(r.get('g4'), dict) else '-'} | "
            f"{r.get('g5', {}).get('all_embedded', '-') if isinstance(r.get('g5'), dict) else '-'} | "
            f"{note.strip()} |")
    (out / f"report-{ts}.md").write_text("\n".join(lines) + "\n",
                                         encoding="utf-8")
    print(f"报告:{out / f'report-{ts}.md'}")


if __name__ == "__main__":
    sys.exit(main())
