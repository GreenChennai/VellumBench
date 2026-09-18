#!/usr/bin/env python3
"""验收参考项目环境准备(design/19 §5.4)。

1. 扫描验收集全部 HTML 的 @font-face url(fonts/...) 相对引用;
   目标文件缺失时从 artboard 技能字体库(或 2.长图海报/fonts)做**硬链接**补齐。
   只新增文件,不修改任何 HTML/工程文件;跨设备或硬链接失败时退回复制。
2. 修正 6.动画部分/manifest.json 的 size 字段倒置([1080,1920] → 以 HTML 为准)。

用法:python bench/acceptance_env.py [--apply]
默认只读报告(--dry-run);--apply 才落盘。
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
from pathlib import Path

ACCEPT = Path(r"E:\平日资料\Kiln验收参考项目")
ARTBOARD_FONTS = Path(r"E:\平日资料\GitHub\.agents\skills\artboard\fonts")
LONGFORM_FONTS = ACCEPT / "2.长图海报" / "fonts"
MANIFEST = ACCEPT / "6.动画部分" / "manifest.json"

URL_RE = re.compile(r"url\(\s*['\"]?([^'\")]+?)['\"]?\s*\)")


def font_refs(html: Path) -> list[str]:
    try:
        text = html.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
    out = []
    for m in URL_RE.finditer(text):
        ref = m.group(1).strip()
        if ref.startswith(("data:", "http://", "https://", "/", "#")):
            continue
        if "fonts/" in ref:
            out.append(ref)
    return out


def resolve_source(ref: str) -> Path | None:
    """把 HTML 里的 fonts/ 相对引用映射到供应方字体库。"""
    rel = ref.replace("\\", "/").lstrip("./")
    # fonts/<sub>/<file> → artboard/fonts/<sub>/<file>
    cand = ARTBOARD_FONTS / rel[len("fonts/"):] if rel.startswith("fonts/") else None
    if cand and cand.is_file():
        return cand
    # 裸 fonts/MiSans-*.woff2(无子目录)→ 2.长图海报/fonts/<file>
    name = Path(rel).name
    cand = LONGFORM_FONTS / name
    if cand.is_file():
        return cand
    # 裸文件名在 artboard 库里递归找一个同名
    hits = sorted(ARTBOARD_FONTS.rglob(name))
    return hits[0] if hits else None


def link(src: Path, dst: Path) -> str:
    dst.parent.mkdir(parents=True, exist_ok=True)
    try:
        os_link = getattr(__import__("os"), "link")
        os_link(src, dst)
        return "hardlink"
    except OSError:
        shutil.copy2(src, dst)
        return "copy"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="落盘(默认只读报告)")
    args = ap.parse_args()

    htmls = sorted(ACCEPT.rglob("*.html"))
    need: dict[Path, Path] = {}   # dst -> src
    missing_src: list[tuple[str, str]] = []
    for html in htmls:
        for ref in font_refs(html):
            dst = (html.parent / ref).resolve()
            if dst.exists():
                continue
            src = resolve_source(ref)
            if src is None:
                missing_src.append((str(html.relative_to(ACCEPT)), ref))
            else:
                need[dst] = src

    print(f"扫描 {len(htmls)} 个 HTML")
    print(f"缺失字体引用 {len(need) + len(missing_src)} 项:"
          f"可补齐 {len(need)},无供应方 {len(missing_src)}")
    for dst, src in sorted(need.items()):
        action = link(src, dst) if args.apply else "will-hardlink"
        print(f"  [{action}] {dst.relative_to(ACCEPT)}  <-  {src}")
    for html_rel, ref in missing_src:
        print(f"  [UNRESOLVED] {html_rel}: {ref}")

    # manifest 尺寸修正(逐项;HTML/project.json 真值 1920x1080)
    if MANIFEST.is_file():
        data = json.loads(MANIFEST.read_text(encoding="utf-8"))
        fixed = 0
        for item in data.get("items", []):
            if item.get("size") == [1080, 1920]:
                if args.apply:
                    item["size"] = [1920, 1080]
                fixed += 1
        print(f"manifest.json 倒置尺寸 {fixed} 项"
              + ("" if args.apply else "(dry-run 未落盘)"))
        if args.apply and fixed:
            MANIFEST.write_text(
                json.dumps(data, ensure_ascii=False, indent=1), encoding="utf-8")
            print("  [fixed] 全部 -> [1920, 1080]")
    else:
        print("manifest.json 不存在")

    if not args.apply:
        print("\n(dry-run,未落盘;--apply 执行)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
