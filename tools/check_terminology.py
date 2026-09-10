#!/usr/bin/env python3
"""门禁 6(简版):扫描 i18n 文案文件中的禁用术语(01 篇 §七)。

范围:i18n/*.ftl(UI 文案单一来源)。源代码中的标识符(如 egui Frame 类型)
不属于 UI 文案,不在扫描范围。CONTEXT.md 的禁用表本身是文档,也排除。
"""
import sys, pathlib

BANNED = [
    ("Frame", "用「编组 / 画板」"),
    ("智能对象", "Photoshop 词,不用"),
]

i18n = pathlib.Path("i18n")
if not i18n.exists():
    print("术语扫描: PASS (i18n/ 尚未建立,P5 落地)")
    sys.exit(0)

fails = 0
for f in sorted(i18n.glob("*.ftl")):
    for i, line in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
        for word, why in BANNED:
            if word in line and "禁用" not in line:
                print(f"[术语] {f}:{i}: 禁用词「{word}」({why})")
                fails += 1

print(f"术语扫描: {'FAIL' if fails else 'PASS'}")
sys.exit(1 if fails else 0)
