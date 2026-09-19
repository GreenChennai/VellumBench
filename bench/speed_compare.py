#!/usr/bin/env python3
"""WPI vs Kiln(车道 B)PNG 导出速度对比 + Kiln 额外导出可编辑 .AI。

输出:E:\平日资料\Kiln验收参考项目\_验收导出\
  WPI\{case}.png  Kiln-PNG\{case}.png  Kiln-AI\{case}.ai  速度对比.md
计时口径:子进程从启动到退出(即用户命令行单次导出的真实体感,含浏览器启动)。
"""
from __future__ import annotations

import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from acceptance import CASES, DEF_KILN, DEF_WPI, render_kiln, render_wpi_baseline  # noqa: E402

OUT = Path(r"E:\平日资料\Kiln验收参考项目\_验收导出")
WPI_DIR = OUT / "WPI"
KILN_PNG_DIR = OUT / "Kiln-PNG"
KILN_AI_DIR = OUT / "Kiln-AI"


def main() -> int:
    for d in (WPI_DIR, KILN_PNG_DIR, KILN_AI_DIR):
        d.mkdir(parents=True, exist_ok=True)
    rows = []
    for case in CASES:
        name = case["name"]
        row = {"case": name}

        wpi_out = WPI_DIR / f"{name}.png"
        t0 = time.perf_counter()
        ok, msg = render_wpi_baseline(Path(DEF_WPI), case, wpi_out)
        row["wpi_s"] = round(time.perf_counter() - t0, 1)
        row["wpi_ok"] = ok

        kiln_png = KILN_PNG_DIR / f"{name}.png"
        t0 = time.perf_counter()
        ok, msg = render_kiln(Path(DEF_KILN), case, kiln_png, "PNG", "browser")
        row["kiln_s"] = round(time.perf_counter() - t0, 1)
        row["kiln_ok"] = ok

        # AI:海报 5 案例已在 formats 里;动画卡单页可编辑,补导。
        # 易拉宝/长图高度超 2400px(打印会分页),不适合单页 AI,跳过。
        want_ai = ("AI" in case["formats"]) or case.get("anim")
        if want_ai:
            ai_out = KILN_AI_DIR / f"{name}.ai"
            t0 = time.perf_counter()
            ok, msg = render_kiln(Path(DEF_KILN), case, ai_out, "AI", "browser")
            row["ai_s"] = round(time.perf_counter() - t0, 1)
            row["ai_ok"] = ok

        rows.append(row)
        print(f"[done] {name}: WPI {row['wpi_s']}s / Kiln {row['kiln_s']}s"
              + (f" / AI {row.get('ai_s')}s" if "ai_s" in row else ""),
              flush=True)

    wpi_total = sum(r["wpi_s"] for r in rows)
    kiln_total = sum(r["kiln_s"] for r in rows)
    ai_total = sum(r.get("ai_s", 0) for r in rows)
    lines = [
        "# WPI vs Kiln(车道 B)PNG 导出速度对比",
        "",
        "计时 = 命令行单次导出全流程(含浏览器/Python 启动);单位秒。",
        "",
        "| 案例 | WPI PNG | Kiln PNG | Kiln AI | 备注 |",
        "|---|---|---|---|---|",
    ]
    for r in rows:
        note = []
        if not r["wpi_ok"]:
            note.append("WPI失败")
        if not r["kiln_ok"]:
            note.append("Kiln失败")
        if not r.get("ai_ok", True):
            note.append("AI失败")
        lines.append(f"| {r['case']} | {r['wpi_s']} | {r['kiln_s']} | "
                     f"{r.get('ai_s', '-')} | {','.join(note)} |")
    lines += [
        "",
        f"| **合计** | **{wpi_total:.1f}** | **{kiln_total:.1f}** | "
        f"**{ai_total:.1f}(24 例)** | Kiln PNG 为 WPI 的 "
        f"{kiln_total / wpi_total * 100:.0f}% 耗时 |",
        "",
        "易拉宝/长图未导 AI:CSS 高度 >2400px,printToPDF 会按页切分,"
        "不适合单页可编辑文件(如需可改走 PDF 分页版)。",
    ]
    (OUT / "速度对比.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"合计:WPI {wpi_total:.1f}s / Kiln PNG {kiln_total:.1f}s / AI {ai_total:.1f}s",
          flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
