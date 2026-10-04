"""一次性:归类 cargo check 短格式错误。"""
import collections
import re

c = collections.Counter()
samples = {}
for line in open(r"D:/Temp/chk.txt", encoding="utf-8", errors="replace"):
    m = re.search(r"error\[?(E[0-9A-Z]+)?", line)
    if m:
        c[m.group(1)] += 1
        samples.setdefault(m.group(1), line.strip()[:150])
for k, v in c.most_common():
    print(v, k, "|", samples[k])
