"""一次性:打印当前错误行原文(按文件分组)。"""
import re
import pathlib

seen = set()
for line in open(r"D:/Temp/chk.txt", encoding="utf-8", errors="replace"):
    m = re.search(r"(crates[\\/].*?\.rs):(\d+):\d+: error\[(E[0-9A-Z]+)\]", line)
    if not m:
        continue
    key = (m.group(1), m.group(2), m.group(3))
    if key in seen:
        continue
    seen.add(key)
    p = pathlib.Path(m.group(1).replace("\\", "/"))
    lines = p.read_text(encoding="utf-8").split("\n")
    ln = lines[int(m.group(2)) - 1]
    print(f"{m.group(1)}:{m.group(2)} [{m.group(3)}]")
    print("   ", ln.strip()[:230])
