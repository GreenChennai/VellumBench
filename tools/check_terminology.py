#!/usr/bin/env python3
"""门禁 6(简版):扫描 i18n 文案文件中的禁用术语(01 篇 §七)。

范围:i18n/*.ftl(UI 文案单一来源,05-7 起**中英两套都扫**)。源代码中的
标识符(如 egui Frame 类型)不属于 UI 文案,不在扫描范围。CONTEXT.md 的
禁用表本身是文档,也排除。

词表(05-7 扩展):中文禁用词按子串匹配;英文对应词按整词正则匹配
(大小写不敏感),避免「Frame」误伤复合词以外的一般英文文本。
含「禁用」字样的行跳过(词表文档自举的逃生口,保持既有约定)。
"""
import re
import sys
import pathlib

# (词, 原因, 匹配方式):word = 子串;regex = 整词、大小写不敏感
BANNED = [
    ("Frame", "用「编组 / 画板」", "regex"),
    ("frame", "用「编组 / 画板」(Frame 为禁用词)", "regex"),
    ("智能对象", "Photoshop 词,不用", "word"),
    ("smart object", "「智能对象」的英文对应词,不用", "regex"),
]

i18n = pathlib.Path("i18n")
if not i18n.exists():
    print("terminology: PASS (i18n/ not created yet, P5)")
    sys.exit(0)

files = sorted(i18n.glob("*.ftl"))
if not files:
    print("terminology: FAIL (i18n/ exists but no .ftl resources)")
    sys.exit(1)

fails = 0
for f in files:
    for i, line in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
        if "禁用" in line:
            continue
        for word, why, mode in BANNED:
            hit = (
                re.search(rf"\b{re.escape(word)}\b", line, re.IGNORECASE)
                if mode == "regex"
                else word in line
            )
            if hit:
                print(f"[term] {f}:{i}: banned word {word!r} ({why})")
                fails += 1

print(f"terminology: {'FAIL' if fails else 'PASS'}")
sys.exit(1 if fails else 0)
