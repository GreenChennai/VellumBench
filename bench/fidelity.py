#!/usr/bin/env python3
"""保真度评分(与 BENCHMARK.md §八口径一致):

  基线 = WPI(Playwright + 系统 Edge 无头浏览器)同源 HTML 渲染
  score = 100 × (1 − MAD/255),MAD = 全像素 RGB 平均绝对差

用法:
  python fidelity.py --wpi <WPI仓库> --kiln <kiln-cli.exe> [--out <目录>]

产物:bench/suite/baseline/<case>.png(浏览器基线,入库固化)、
      bench/suite/kiln/<case>.png、控制台逐例得分与平均分。
"""
import argparse
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageChops

SUITE = Path(__file__).resolve().parent / "suite"


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        print("  cmd failed:", " ".join(str(c) for c in cmd[-4:]))
    return r


def render_baseline(wpi: Path, case: Path, out: Path, width: int):
    r = run([
        sys.executable, str(wpi / "src" / "main.py"),
        "--export", "--source", str(case), "--output", str(out),
        "--format", "PNG", "--width", str(width), "--scale", "1",
    ])
    return r.returncode == 0 and out.exists()


def render_kiln(kiln: Path, case: Path, out: Path):
    r = run([
        str(kiln), "export", "--source", str(case), "--output", str(out),
        "--format", "PNG", "--scale", "1",
    ])
    return r.returncode == 0 and out.exists()


def mad_score(a: Path, b: Path):
    ia = Image.open(a).convert("RGB")
    ib = Image.open(b).convert("RGB")
    if ia.size != ib.size:
        # 尺寸不一致:裁到共同区域(左上对齐)
        w = min(ia.width, ib.width)
        h = min(ia.height, ib.height)
        ia = ia.crop((0, 0, w, h))
        ib = ib.crop((0, 0, w, h))
    diff = ImageChops.difference(ia, ib)
    hist = diff.histogram()
    total = ia.width * ia.height * 3
    mad = sum(i % 256 * c for i, c in enumerate(hist)) / total
    return 100.0 * (1.0 - mad / 255.0), mad


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--wpi", default=r"E:\平日资料\GitHub\WPI")
    ap.add_argument("--kiln", default=r"E:\平日资料\GitHub\VellumBench\target\release\kiln-cli.exe")
    ap.add_argument("--out", default=str(SUITE))
    ap.add_argument("--skip-baseline", action="store_true")
    args = ap.parse_args()
    out = Path(args.out)
    base_dir = out / "baseline"
    kiln_dir = out / "kiln"
    base_dir.mkdir(parents=True, exist_ok=True)
    kiln_dir.mkdir(parents=True, exist_ok=True)

    cases = sorted(SUITE.glob("c*.html"))
    if not cases:
        print("无用例", file=sys.stderr)
        return 1

    scores = []
    for case in cases:
        name = case.stem
        base = base_dir / f"{name}.png"
        kil = kiln_dir / f"{name}.png"
        if not args.skip_baseline or not base.exists():
            if not render_baseline(Path(args.wpi), case, base, 1280):
                print(f"{name}: 基线渲染失败")
                continue
        if not render_kiln(Path(args.kiln), case, kil):
            print(f"{name}: Kiln 渲染失败")
            continue
        score, mad = mad_score(base, kil)
        scores.append((name, score, mad))
        print(f"{name}: score={score:.2f} mad={mad:.2f}")

    if not scores:
        print("无有效得分", file=sys.stderr)
        return 1
    avg = sum(s for _, s, _ in scores) / len(scores)
    print(f"=== 平均还原度 {avg:.2f} / 100(达标线 97)===")
    return 0 if avg >= 97.0 else 2


if __name__ == "__main__":
    sys.exit(main())
