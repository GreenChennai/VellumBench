"""一次性:E0308 expected `&str` found `String` → 在 t()/t_args() 表达式前插 `&`。

用法:python tools/_fix_borrow.py   (跑 cargo check --message-format=json,
收集诊断,右到左插 &,重复到收敛。)
"""
import json
import pathlib
import subprocess
import sys

SRC = pathlib.Path("crates/vb_app/src")
ENV = {"TEMP": r"D:\Temp", "TMP": r"D:\Temp", "CARGO_BUILD_JOBS": "1", "CARGO_INCREMENTAL": "0"}

import os

env = dict(os.environ)
env.update(ENV)


def collect():
    p = subprocess.run(
        ["cargo", "check", "-p", "vb_app", "--message-format=json"],
        cwd=".", env=env, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    hits = {}  # path -> [(line, col)]
    for line in p.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        m = msg.get("message", {})
        if (m.get("code") or {}).get("code") != "E0308":
            continue
        text = m.get("message", "") + " ".join(
            c.get("message", "") for c in m.get("children", [])
        ) + " ".join(sp.get("label") or "" for sp in m.get("spans", []))
        if "found `String`" not in text or "consider borrowing" not in text:
            continue
        for sp in m.get("spans", []):
            if sp.get("is_primary"):
                f = sp["file_name"].replace("\\", "/")
                if not f.startswith("crates/vb_app/src"):
                    continue
                hits.setdefault(f, []).append((sp["line_start"], sp["column_start"]))
                break
    return hits


def fix(hits):
    n = 0
    for f, spots in hits.items():
        p = pathlib.Path(f)
        lines = p.read_text(encoding="utf-8").split("\n")
        for line_no, col in sorted(set(spots), reverse=True):
            line = lines[line_no - 1]
            at = col - 1
            # 确认该位置是 vb_session::i18n::t( 的起点
            if line[at:].startswith("vb_session::i18n::t"):
                lines[line_no - 1] = line[:at] + "&" + line[at:]
                n += 1
            else:
                # 表达式起点可能在诊断列之后(外层调用):本行从该列向右
                # 找第一个 t( 调用起点
                idx = line.find("vb_session::i18n::t", at)
                if idx >= 0 and idx - at < 120:
                    lines[line_no - 1] = line[:idx] + "&" + line[idx:]
                    n += 1
                else:
                    print(f"SKIP {f}:{line_no}:{col} {line.strip()[:90]}")
        p.write_text("\n".join(lines), encoding="utf-8", newline="")
    return n


for it in range(8):
    hits = collect()
    total = sum(len(v) for v in hits.values())
    print(f"iter {it}: {total} sites")
    if not total:
        sys.exit(0)
    fixed = fix(hits)
    print(f"  fixed {fixed}")
    if not fixed:
        print("no progress")
        sys.exit(1)
