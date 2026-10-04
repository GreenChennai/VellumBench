"""一次性:词法状态追踪,定位把解析状态弄翻的行。"""
import pathlib
import re

src = pathlib.Path("crates/vb_app/src/app/commands.rs").read_text(encoding="utf-8")
lines = src.split("\n")
BS = chr(92)
state = None
limit = int(sys_line) if (sys_line := 1470) else 1470
for idx, line in enumerate(lines, 1):
    j = 0
    n = len(line)
    while j < n:
        c = line[j]
        if state is None:
            if c == '"':
                state = "str"
            elif c == "'":
                m = re.match("'" + "(" + BS + ".|[^" + BS + "'])'", line[j:])
                if m:
                    j += m.end() - 1
            elif line[j : j + 2] == "//":
                break
            elif line[j : j + 2] == "/*":
                state = "block"
                j += 1
        elif state == "str":
            if c == BS:
                j += 1
            elif c == '"':
                state = None
        elif state == "block":
            if line[j : j + 2] == "*/":
                state = None
                j += 1
        j += 1
    if state and 1300 <= idx <= 1470:
        print(f"line {idx}: state={state} | {line[:90]!r}")
    if idx > limit:
        break
print("final:", state)
