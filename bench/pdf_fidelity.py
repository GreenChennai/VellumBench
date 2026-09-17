#!/usr/bin/env python3
"""PDF 可编辑导出保真度对比管线(计划书 18 §三)。

基线 = Kiln CPU 栅格(HTML → Kiln 直接渲染 PNG,即"原生 HTML 导出的 PNG")
候选 = Kiln PDF → PDFium 渲染 PNG(可编辑 PDF → PDFium 渲染)

评分 = 100 × (1 − MAD/255),MAD = 全像素 RGB 平均绝对差。
网格 10×10 分区定位差异区域。
文本提取比对:PDF ToUnicode 提取 vs CPU 栅格(基准为 HTML 文本)。

用法:
  python pdf_fidelity.py --kiln <exe> [--cases <目录>] [--out <目录>]
"""
import argparse
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageChops

SCRIPTS = Path(__file__).resolve().parent.parent / "scripts"
CASES_DIR = SCRIPTS.parent / "assets" / "cases"
OUT_DIR = Path(__file__).resolve().parent / "pdf-fidelity"

GRID = 10  # 10×10 网格
FORBIDDEN_START = "。、！？：；）】》〉」』〕〗〙〛"


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    return r


def render_kiln_png(kiln, source, out):
    r = run([kiln, "export", "--source", source, "--output", str(out),
             "--format", "PNG", "--scale", "1"])
    return r.returncode == 0 and Path(out).exists()


def render_kiln_pdf(kiln, source, out):
    r = run([kiln, "export", "--source", source, "--output", str(out),
             "--format", "PDF", "--scale", "1"])
    return r.returncode == 0 and Path(out).exists()


def render_pdfium(pdf_path, png_path):
    try:
        import pypdfium2 as pdfium
        pdf = pdfium.PdfDocument(str(pdf_path))
        bmp = pdf[0].render(scale=1.0)
        bmp.to_pil().save(str(png_path))
        return True
    except Exception:
        return False


def extract_pdf_text(pdf_path):
    try:
        import pypdfium2 as pdfium
        pdf = pdfium.PdfDocument(str(pdf_path))
        return pdf[0].get_textpage().get_text_range()
    except Exception:
        return ""


def extract_html_text(html_path):
    """从 HTML 提取可见文本(去标签+去空白)。"""
    import re
    html = Path(html_path).read_text(encoding="utf-8", errors="replace")
    # 去 style/script
    html = re.sub(r'<style[^>]*>.*?</style>', '', html, flags=re.S)
    html = re.sub(r'<script[^>]*>.*?</script>', '', html, flags=re.S)
    html = re.sub(r'<!--.*?-->', '', html, flags=re.S)
    # 去标签
    text = re.sub(r'<[^>]+>', ' ', html)
    # 去实体
    text = text.replace('&amp;', '&').replace('&lt;', '<').replace('&gt;', '>')
    text = text.replace('&nbsp;', ' ').replace('&quot;', '"')
    # 折叠空白
    text = re.sub(r'\s+', ' ', text).strip()
    return text


def mad_score(a: Path, b: Path):
    ia = Image.open(a).convert("RGB")
    ib = Image.open(b).convert("RGB")
    if ia.size != ib.size:
        w = min(ia.width, ib.width)
        h = min(ia.height, ib.height)
        ia = ia.crop((0, 0, w, h))
        ib = ib.crop((0, 0, w, h))
    diff = ImageChops.difference(ia, ib)
    hist = diff.histogram()
    total = ia.width * ia.height * 3
    mad = sum(i % 256 * c for i, c in enumerate(hist)) / total
    score = 100.0 * (1.0 - mad / 255.0)
    return score, mad, (ia.width, ia.height)


def grid_analysis(a: Path, b: Path, grid=GRID):
    """10×10 网格分区:返回 MAD > 30 的格子坐标列表。"""
    ia = Image.open(a).convert("RGB")
    ib = Image.open(b).convert("RGB")
    if ia.size != ib.size:
        w = min(ia.width, ib.width)
        h = min(ia.height, ib.height)
        ia = ia.crop((0, 0, w, h))
        ib = ib.crop((0, 0, w, h))
    gw = ia.width // grid
    gh = ia.height // grid
    hot = []
    for gy in range(grid):
        for gx in range(grid):
            box = (gx * gw, gy * gh, (gx + 1) * gw, (gy + 1) * gh)
            ca = ia.crop(box)
            cb = ib.crop(box)
            diff = ImageChops.difference(ca, cb)
            hist = diff.histogram()
            total = ca.width * ca.height * 3
            if total == 0:
                continue
            mad = sum(i % 256 * c for i, c in enumerate(hist)) / total
            if mad > 30:
                hot.append((gx, gy, round(mad, 1)))
    return hot


def text_match_ratio(pdf_text: str, html_text: str) -> float:
    """文本提取一致率:PDF 提取文本中出现在 HTML 文本中的字符占比。"""
    if not html_text:
        return 0.0
    html_set = set(html_text.replace(' ', ''))
    if not html_set:
        return 1.0
    matched = sum(1 for c in pdf_text if c != ' ' and c in html_set)
    total = len([c for c in pdf_text if c != ' '])
    return matched / total if total > 0 else 0.0


def main():
    ap = argparse.ArgumentParser(description="PDF 可编辑导出保真度对比")
    ap.add_argument("--kiln", default=r"E:\平日资料\GitHub\VellumBench\target\release\kiln-cli.exe")
    ap.add_argument("--cases", default=str(CASES_DIR))
    ap.add_argument("--out", default=str(OUT_DIR))
    ap.add_argument("--min-score", type=float, default=90.0, help="单案例最低分(网格分析触发)")
    args = ap.parse_args()

    cases_dir = Path(args.cases)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    case_files = sorted(cases_dir.glob("*.html"))
    if not case_files:
        print("无案例文件", file=sys.stderr)
        return 1

    kiln = args.kiln
    if not Path(kiln).exists():
        print(f"Kiln exe 不存在: {kiln}", file=sys.stderr)
        return 1

    all_scores = []
    all_text_ratios = []
    failures = []

    for case in case_files:
        name = case.stem
        case_out = out_dir / name
        case_out.mkdir(parents=True, exist_ok=True)

        base_png = case_out / "html_render.png"
        pdf_file = case_out / "export.pdf"
        pdf_png = case_out / "pdf_render.png"

        # 1. 基线:HTML → Kiln CPU 栅格 PNG
        if not render_kiln_png(kiln, str(case), str(base_png)):
            print(f"✗ {name}: HTML→PNG 渲染失败")
            failures.append(name)
            continue

        # 2. 候选:HTML → PDF → PDFium PNG
        if not render_kiln_pdf(kiln, str(case), str(pdf_file)):
            print(f"✗ {name}: PDF 导出失败")
            failures.append(name)
            continue
        if not render_pdfium(pdf_file, pdf_png):
            print(f"✗ {name}: PDF→PNG 渲染失败")
            failures.append(name)
            continue

        # 3. 像素对比
        score, mad, size = mad_score(base_png, pdf_png)

        # 4. 网格分区
        hot_zones = grid_analysis(base_png, pdf_png)

        # 5. 文本比对
        pdf_text = extract_pdf_text(pdf_file)
        html_text = extract_html_text(case)
        ratio = text_match_ratio(pdf_text, html_text)

        all_scores.append(score)
        all_text_ratios.append(ratio)

        status = "✓" if score >= 90 else "△" if score >= 80 else "✗"
        hot_str = f" hot={hot_zones}" if hot_zones else ""
        print(f"{status} {name}: score={score:.2f} text_ratio={ratio:.2f} size={size}{hot_str}")

        if score < args.min_score:
            failures.append(f"{name} (score={score:.1f})")

    if not all_scores:
        print("无有效结果", file=sys.stderr)
        return 1

    avg = sum(all_scores) / len(all_scores)
    avg_text = sum(all_text_ratios) / len(all_text_ratios) if all_text_ratios else 0
    print(f"\n{'='*60}")
    print(f"  案例数: {len(all_scores)}")
    print(f"  平均还原度: {avg:.2f} / 100(达标线 97)")
    print(f"  平均文本一致率: {avg_text:.2%}(达标线 95%)")
    print(f"  低于 {args.min_score} 分的案例: {len(failures)}")
    if failures:
        print(f"    {failures}")
    print(f"{'='*60}")
    ok = avg >= 97.0 and len(failures) == 0
    print(f"  {'✅ 全绿' if ok else '❌ 未达标'}")
    return 0 if ok else 2


if __name__ == "__main__":
    sys.exit(main())
