#!/usr/bin/env python3
"""UI 文案抽取器(UI-01;审查 2026-10-04 §8.12 G-UI-C 配套)。

从 `crates/vb_app/src` 渲染路径机械抽取中文文案字面量:

- 判定口径与 `crates/vb_app/tests/ui_literals.rs`(G-UI-C 门禁)一致:
  跳过注释 / `#[cfg(test)]` / 诊断出口(panic/assert/print/log 族)/
  `vb-literal-ok:` 行豁免 / 数据表(catalog.rs, menus.rs);
- key 规范:`ui-<文件路径连字符>-<序号>`(机械、稳定、可审计);跨文件
  复用与人工语义化的串走 COMMON 表(`ui-common-*` 等);
- zh 值**逐字**取自源码字面量(解码转义后原样写入,禁止润色);
- en 值由 `ui_i18n_dict`(EN_EXACT 精确表 + PHRASES 分词组合)生成,
  遵守 CONTEXT.md 术语表(禁 Frame/frame/smart object);全部单行;
- `format!`/`write!`/`anyhow!`/`bail!` 的格式串转换为 Fluent 占位符
  `{ $aN }` / `{ $name }`,调用点改写为
  `vb_session::i18n::t_args(key, &[("aN",
  vb_session::i18n::FluentValue::from((expr).to_string()))])`;
  含精度/宽度/Debug 规格(`{:.1}`/`{:?}`)的串不动(列手动清单);
- 产物:`i18n/zh.ftl` / `en.ftl` 的 BEGIN/END `ui-extract` 段 +
  `tools/ui_i18n_map.json`(key 分配 sidecar,保证重跑 key 稳定)。

用法:
  python tools/gen_ui_ftl.py --gen               # 生成 ftl 段 + sidecar
  python tools/gen_ui_ftl.py --apply --files foo,bar
                                                 # 只改写路径含子串的文件
  python tools/gen_ui_ftl.py --report            # 分类统计 + 手动清单

调用点改写必须逐处编译验证:每批 --apply 后 `cargo check -p vb_app`,
类型不符(&'static str 形参/const 初始化/match 模式)按报告逐个手动处理。
"""
import argparse
import collections
import json
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from ui_i18n_dict import BANNED_EN, COMMON, EN_EXACT, PHRASES

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "crates/vb_app/src"
ZH = ROOT / "i18n/zh.ftl"
EN = ROOT / "i18n/en.ftl"
SIDECAR = ROOT / "tools/ui_i18n_map.json"
BEGIN = "# ── BEGIN ui-extract(由 tools/gen_ui_ftl.py 生成;勿手改)──"
END = "# ── END ui-extract ──"

# 与 G-UI-C 门禁同一份口径
DATA_TABLES = {"shortcuts/catalog.rs", "shortcuts/menus.rs"}
DIAG = {
    "panic", "expect", "assert", "assert_eq", "assert_ne", "debug_assert",
    "debug_assert_eq", "todo", "unreachable", "print", "println", "eprint",
    "eprintln", "dbg", "info", "warn", "error", "debug", "trace",
}
CJK = re.compile(r"[\u4e00-\u9fff]")
LIT = re.compile(r'"((?:[^"\\\n]|\\.)*)"')

T_CALL = 'vb_session::i18n::t("'
TARGS_CALL = "vb_session::i18n::t_args("
FV = "vb_session::i18n::FluentValue::from"

_ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", "'": "'", '"': '"'}


def decode_rust(lit: str) -> str:
    """解码 Rust 字符串转义(\\u{…} 按 Unicode 码点;未知转义即中止)。"""

    def sub(m):
        e = m.group(1)
        if e[0] in _ESCAPES:
            return _ESCAPES[e[0]]
        if e[0] == "u":
            return chr(int(e[3:-1], 16))
        if e[0] == "x":
            return chr(int(e[1:], 16))
        raise SystemExit(f"未知转义 \\{e}")

    return re.sub(r"\\(.)", sub, lit)


def strip_to_blanked(src: str) -> str:
    """注释与字符串内容抹空(保长度,保换行)——供调用名回溯与 cfg 定位。

    与门禁同源的微型词法:行/块注释、普通与原始字符串、字节串、字符
    字面量(含转义)、生命周期('a 不吃后续内容)。
    """
    b = src
    n = len(b)
    out = []
    i = 0

    def is_id(ch):
        return ch.isalnum() or ch == "_"

    while i < n:
        c = b[i]
        if c == "/" and i + 1 < n and b[i + 1] == "/":
            while i < n and b[i] != "\n":
                out.append(" " if b[i] != "\n" else "\n")
                i += 1
            continue
        if c == "/" and i + 1 < n and b[i + 1] == "*":
            start = i
            i += 2
            while i + 1 < n and not (b[i] == "*" and b[i + 1] == "/"):
                i += 1
            i = min(i + 2, n)
            for x in b[start:i]:
                out.append(" " if x != "\n" else "\n")
            continue
        if c == '"':
            start = i
            i += 1
            while i < n:
                if b[i] == "\\":
                    i += 2
                    continue
                if b[i] == '"':
                    i += 1
                    break
                i += 1
            i = min(i, n)
            for x in b[start:i]:
                out.append(" " if x != "\n" else "\n")
            continue
        if c == "r" and (i == 0 or not is_id(b[i - 1])) and i + 1 < n and (b[i + 1] == '"' or b[i + 1] == "#"):
            start = i
            j = i + 1
            hashes = 0
            while j < n and b[j] == "#":
                hashes += 1
                j += 1
            if j < n and b[j] == '"':
                j += 1
                while j < n:
                    if b[j] == '"':
                        k = j + 1
                        h = 0
                        while h < hashes and k + h < n and b[k + h] == "#":
                            h += 1
                        if h == hashes:
                            j = k + hashes
                            break
                    j += 1
                i = min(j, n)
                for x in b[start:i]:
                    out.append(" " if x != "\n" else "\n")
                continue
            out.append(c)
            i += 1
            continue
        if c == "b" and (i == 0 or not is_id(b[i - 1])) and i + 1 < n and b[i + 1] == '"':
            start = i
            j = i + 2
            while j < n:
                if b[j] == "\\":
                    j += 2
                    continue
                if b[j] == '"':
                    j += 1
                    break
                j += 1
            i = min(j, n)
            for x in b[start:i]:
                out.append(" " if x != "\n" else "\n")
            continue
        if c == "'":
            if i + 1 < n and b[i + 1] == "\\":
                # 字符转义字面量('\n' '\'' …):吃到收引号
                s0 = i
                j = i + 2
                while j < n and b[j] != "'":
                    j += 1
                i = min(j + 1, n)
                for x in b[s0:i]:
                    out.append(" " if x != "\n" else "\n")
                continue
            if i + 2 < n and b[i + 2] == "'":
                # 单字符字面量 'x'
                out.append("   ")
                i += 3
                continue
            out.append(c)  # 生命周期 'a 等,单字符推进
            i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def find_matching_paren(src: str, open_pos: int) -> int:
    """open_pos 指向 `(`;返回配对 `)` 偏移(字符串感知)。"""
    depth = 0
    i = open_pos
    n = len(src)
    instr = False
    esc = False
    while i < n:
        c = src[i]
        if instr:
            if esc:
                esc = False
            elif c == "\\":
                esc = True
            elif c == '"':
                instr = False
        elif c == '"':
            instr = True
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    raise SystemExit(f"{src[open_pos:open_pos+40]!r} 括号不配对")


def split_top_args(src: str, open_pos: int, close_pos: int):
    """把 (open_pos, close_pos) 之间的实参按顶层逗号切开,返回 [(start, end)]。"""
    parts = []
    depth = 0
    last = open_pos + 1
    i = open_pos + 1
    instr = False
    esc = False
    while i < close_pos:
        c = src[i]
        if instr:
            if esc:
                esc = False
            elif c == "\\":
                esc = True
            elif c == '"':
                instr = False
            i += 1
            continue
        if c == '"':
            instr = True
        elif c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif c == "," and depth == 0:
            parts.append((last, i))
            last = i + 1
        i += 1
    parts.append((last, close_pos))
    return parts


def callee_name(blanked: str, pos: int):
    i = pos
    depth = 0
    while i > 0:
        c = blanked[i - 1] if i - 1 < len(blanked) else ""
        if c in " \t\r\n":
            i -= 1
            continue
        if c == "(" and depth == 0:
            i -= 1
            while i > 0 and blanked[i - 1] in " \t\r\n":
                i -= 1
            if i > 0 and blanked[i - 1] == "!":
                i -= 1
            end = i
            while i > 0 and (blanked[i - 1].isalnum() or blanked[i - 1] == "_"):
                i -= 1
            name = blanked[i:end]
            return name.split("::")[-1] if name else None
        if c in ")]":
            depth += 1
            i -= 1
            continue
        if c in "([":
            if depth == 0:
                return None
            depth -= 1
            i -= 1
            continue
        if c in ";{}":
            return None
        i -= 1
    return None


def cfg_test_regions(blanked: str):
    regions = []
    for m in re.finditer(r"#\[cfg\(test\)\]", blanked):
        k = blanked.find("{", m.end())
        if k < 0:
            continue
        depth = 0
        j = k
        while j < len(blanked):
            if blanked[j] == "{":
                depth += 1
            elif blanked[j] == "}":
                depth -= 1
                if depth == 0:
                    regions.append((m.start(), j + 1))
                    break
            j += 1
    return regions


class Site:
    __slots__ = (
        "rel", "start", "end", "raw", "text", "kind", "line", "note",
        "open_pos", "close_pos", "macro_start",
    )

    def __init__(self, rel, start, end, raw, text, line):
        self.rel = rel
        self.start = start        # 字面量内容起点(开引号后)
        self.end = end            # 字面量内容终点(收引号前)
        self.raw = raw            # 源码里的字面量内容(含转义序列)
        self.text = text          # 解码后的文本
        self.kind = "plain"       # plain|format|write|anyhow|bail|manual:*
        self.line = line
        self.note = ""
        self.open_pos = -1        # format 族:调用开括号
        self.close_pos = -1       # format 族:调用配对括号
        self.macro_start = -1     # format 族:宏名起点

    @property
    def manual(self):
        return self.kind.startswith("manual")


def scan_sites(path: pathlib.Path, rel: str):
    """扫描一份源码,产出分类后的 Site 列表(口径 = G-UI-C 门禁)。"""
    src = path.read_text(encoding="utf-8")
    blanked = strip_to_blanked(src)
    regions = cfg_test_regions(blanked)
    lines = []
    pos = 0
    for ln in src.split("\n"):
        lines.append((pos, pos + len(ln)))
        pos += len(ln) + 1

    def line_of(off):
        for k, (a, b) in enumerate(lines, 1):
            if a <= off <= b:
                return k, src[a:b]
        return len(lines), ""

    out = []
    for m in LIT.finditer(src):
        s, e, raw = m.start(1), m.end(1), m.group(1)
        if not CJK.search(raw):
            continue
        if any(a <= s < b for (a, b) in regions):
            continue
        no, line = line_of(s)
        if "vb-literal-ok:" in line:
            continue
        if callee_name(blanked, s) in DIAG:
            continue
        site = Site(rel, s, e, raw, decode_rust(raw), no)
        out.append(_classify(src, blanked, site))
    return out, src


def _classify(src: str, blanked: str, site: Site) -> Site:
    # 常量/静态初始化(无法在 const 上下文调用 fn)
    line_start = src.rfind("\n", 0, site.start) + 1
    line_head = src[line_start : site.start].strip()
    if re.match(r"(pub\s+)?(const|static)\b", line_head):
        site.kind = "manual:const"
        site.note = "const/static 初始化器不能调用 fn"
        return site
    # match 模式位置(后随 `=>`)
    after = blanked[site.end :].lstrip()
    if after.startswith("=>"):
        site.kind = "manual:pattern"
        site.note = "match 模式位,不能改为表达式"
        return site
    # 解码后含控制字符(\n 等)——ftl 单行放不下
    if any(ord(ch) < 32 for ch in site.text):
        site.kind = "manual:ctl"
        site.note = "解码后含控制字符,ftl 单行放不下"
        return site
    prefix = blanked[: site.start].rstrip()
    m = re.search(r"\b(format|write|writeln|anyhow|bail)!\($", prefix)
    if not m:
        return site
    name = m.group(1)
    open_pos = prefix.rfind("(")
    if open_pos < 0:
        return site
    close_pos = find_matching_paren(src, open_pos)
    args = split_top_args(src, open_pos, close_pos)
    # 字面量(连引号)必须整体落在某个实参内
    lit_idx = None
    for idx, (a, b) in enumerate(args):
        if a <= site.start - 1 and b >= site.end + 1:
            lit_idx = idx
            break
    first_fmt = 1 if name in ("write", "writeln") else 0
    if lit_idx != first_fmt:
        return site  # 不是格式串(普通实参),按 plain 处理
    site.open_pos = open_pos
    site.close_pos = close_pos
    # 宏名起点
    head = blanked[:open_pos].rstrip()
    k = len(head)
    while k > 0 and (head[k - 1].isalnum() or head[k - 1] in "_!:"):
        k -= 1
    site.macro_start = k
    site.kind = {"format": "format", "write": "write", "writeln": "write",
                 "anyhow": "anyhow", "bail": "bail"}[name]
    return site


SPEC_BAD = re.compile(r"[<>^#:]")

def convert_format_value(text: str):
    """格式串 → (fluent 值, [(argname, positional_index_or_None)])。

    返回 None 表示含不支持的规格(精度/宽度/Debug),列手动。
    """
    out = []
    args = []
    seen = set()
    pos_seen = 0
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "{" and i + 1 < n and text[i + 1] == "{":
            out.append('{"{"}')
            i += 2
            continue
        if c == "}" and i + 1 < n and text[i + 1] == "}":
            out.append('{"}"}')
            i += 2
            continue
        if c == "{":
            j = text.find("}", i)
            if j < 0:
                return None
            spec = text[i + 1 : j].strip()
            if SPEC_BAD.search(spec):
                return None
            if spec == "":
                k = pos_seen
                pos_seen += 1
                an = f"a{k + 1}"
            elif spec.isdigit():
                k = int(spec)
                pos_seen = max(pos_seen, k + 1)
                an = f"a{k + 1}"
            elif re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", spec):
                k = None
                an = spec
            else:
                return None
            if an not in seen:
                seen.add(an)
                args.append((an, k))
            out.append("{ $" + an + " }")
            i = j + 1
            continue
        out.append(c)
        i += 1
    return "".join(out), args


# ────────────────────────── 数据表 key(menus.rs)──────────────────────────
def data_table_rows():
    """解析 shortcuts/menus.rs 的四张表,产出 (key, zh) 机械镜像。

    表是 zh 文案基准(数据表豁免,见门禁注释);渲染点经
    menu_label_t/planned_reason_t/pathfinder_tip_t/menu_title_t 取词。
    """
    text = (SRC / "shortcuts/menus.rs").read_text(encoding="utf-8")
    rows = []
    # MenuItem { id: "x", label: "y" }
    for m in re.finditer(r'id: "([^"]+)",\n\s*label: "([^"]*)"', text):
        mid, label = m.group(1), decode_rust(m.group(2))
        rows.append((f"ui-menu-{mid.replace('.', '-')}", label))
    # PLANNED / PATHFINDER_TIPS:("id", "text") 二元组
    for m in re.finditer(r'\(\s*"([a-z_.]+)",\s*\n?\s*"([^"]*)"\s*,?\s*\)', text):
        mid, tip = m.group(1), decode_rust(m.group(2))
        key = f"ui-pf-{mid.replace('.', '-')}" if "path." in mid else f"ui-planned-{mid.replace('.', '-')}"
        rows.append((key, tip))
    # MENU_TITLES
    m = re.search(r"MENU_TITLES:\s*\[&str;\s*9\]\s*=\s*\[(.*?)\]", text, re.S)
    if m:
        for i, lit in enumerate(re.findall(r'"([^"]*)"', m.group(1))):
            rows.append((f"ui-menu-title-{i}", decode_rust(lit)))
    return rows


# ────────────────────────── key 分配 ──────────────────────────
def key_stem(rel: str) -> str:
    stem = rel[: -len(".rs")]
    stem = stem.replace("/", "-").replace("_", "-")
    # 术语门禁禁 frame(含 key 行);frame.rs 的词干换名
    stem = stem.replace("app-frame", "app-chrome")
    if stem.endswith("-mod"):
        stem += "-panels"
    return stem


class Assigner:
    def __init__(self, sidecar: dict):
        self.keys = dict(sidecar.get("keys", {}))
        self.counter = collections.Counter(sidecar.get("counters", {}))

    def key_for(self, rel: str, text: str) -> str:
        if text in self.keys:
            return self.keys[text]
        if text in COMMON:
            k = COMMON[text]
        else:
            stem = key_stem(rel)
            n = self.counter[stem] + 1
            self.counter[stem] = n
            k = f"ui-{stem}-{n:03d}"
        self.keys[text] = k
        return k

    def dump(self):
        return {"keys": self.keys, "counters": dict(self.counter)}


# ────────────────────────── en 翻译 ──────────────────────────
# zh 全角标点折叠(仅用于 EN_EXACT 查询键;zh 值本身逐字保留全角)。
FOLD = str.maketrans({
    ",": ",", ":": ":", ";": ";", "!": "!", "?": "?",
    "(": "(", ")": ")",
})
EN_FOLDED = {k.translate(FOLD): v for k, v in EN_EXACT.items()}


def en_translate(text: str) -> str:
    """en 取词:精确表 → 分词组合 → 空串(调用方记 TODO)。"""
    if text in EN_EXACT:
        return EN_EXACT[text]
    folded = text.translate(FOLD)
    if folded in EN_FOLDED:
        return EN_FOLDED[folded]
    # 占位符哨兵化,保护 { $x } / {"{"} / {"}"}
    prot = re.sub(r"\{ \$[A-Za-z0-9_]+ \}", lambda m: "\x00" + m.group(0) + "\x03", text)
    prot = prot.replace('{"{"}', "\x01").replace('{"}"}', "\x02")
    words = []
    i = 0
    while i < len(prot):
        c = prot[i]
        if c in "\x00\x01\x02":
            j = prot.find("\x03", i)
            if j < 0:
                return ""
            words.append(prot[i : j + 1])
            i = j + 1
            continue
        if ord(c) < 128:
            j = i
            while j < len(prot) and ord(prot[j]) < 128:
                j += 1
            words.append(prot[i:j])
            i = j
            continue
        hit = None
        for L in range(min(10, len(prot) - i), 0, -1):
            seg = prot[i : i + L]
            if seg in PHRASES:
                hit = seg
                break
        if not hit:
            return ""
        words.append(PHRASES[hit])
        i += len(hit)
    out = []
    for w in words:
        if w.startswith("\x00"):
            out.append(w[1:-1])
        elif w.startswith("\x01"):
            out.append("{")
        elif w.startswith("\x02"):
            out.append("}")
        else:
            out.append(w)
    s = " ".join(out)
    s = re.sub(r"\s+([,:;!?])", r"\1", s)
    return s


def check_banned(en: str) -> bool:
    low = en.lower()
    return not any(re.search(rf"\b{re.escape(b)}\b", low) for b in BANNED_EN)


# ────────────────────────── ftl 写出 ──────────────────────────
def without_section(path: pathlib.Path) -> str:
    text = path.read_text(encoding="utf-8")
    b = text.find(BEGIN)
    e = text.find(END)
    if 0 <= b < e:
        return text[:b].rstrip("\n") + "\n" + text[e + len(END) :].lstrip("\n")
    return text.rstrip("\n") + "\n"


def write_sections(zh_body: str, en_body: str):
    zt = without_section(ZH).rstrip("\n")
    et = without_section(EN).rstrip("\n")
    ZH.write_text(
        zt + "\n\n" + BEGIN + "\n" + zh_body.rstrip("\n") + "\n" + END + "\n",
        encoding="utf-8", newline="\n",
    )
    EN.write_text(
        et + "\n\n" + BEGIN + "\n" + en_body.rstrip("\n") + "\n" + END + "\n",
        encoding="utf-8", newline="\n",
    )


# ────────────────────────── 改写 ──────────────────────────
def targs_rust(key: str, args, src: str, open_pos: int, close_pos: int):
    """生成 t_args(…) 调用文本;args = [(name, positional_index_or_None)]。"""
    parts = []
    fa_cache = None
    for an, k in args:
        if k is None:
            expr = an  # 行内捕获:标识符本身
        else:
            if fa_cache is None:
                fa_cache = split_top_args(src, open_pos, close_pos)
            idx = 1 + k  # 第 0 参是格式串
            if idx >= len(fa_cache):
                return None
            expr = src[fa_cache[idx][0] : fa_cache[idx][1]].strip()
            if not expr:
                return None
        parts.append(f'("{an}", {FV}(({expr}).to_string()))')
    inner = ", ".join(parts)
    return f"{TARGS_CALL}\"{key}\", &[{inner}])"


def rewrite(src: str, sites, keys):
    """从右向左改写;返回(新源码, 改写数, 跳过数)。"""
    edits = []
    skipped = 0
    for s in sites:
        if s.manual:
            skipped += 1
            continue
        key = keys.get(s.text)
        if not key:
            skipped += 1
            continue
        if s.kind == "plain":
            edits.append((s.start - 1, s.end + 1, f'{T_CALL}{key}")'))
            continue
        conv = convert_format_value(s.text)
        if conv is None:
            skipped += 1
            continue
        _, fargs = conv
        call = (
            targs_rust(key, fargs, src, s.open_pos, s.close_pos) if fargs else None
        )
        if call is None:
            skipped += 1
            continue
        if not fargs:
            call = f'{T_CALL}{key}")'
        if s.kind == "format":
            edits.append((s.macro_start, s.close_pos + 1, call))
        elif s.kind == "write":
            fa = split_top_args(src, s.open_pos, s.close_pos)
            recv = src[fa[0][0] : fa[0][1]].strip()
            edits.append(
                (s.macro_start, s.close_pos + 1, f'write!({recv}, "{{}}", {call})')
            )
        else:  # anyhow / bail
            edits.append(
                (s.macro_start, s.close_pos + 1, f'{s.kind}!("{{}}", {call})')
            )
    out = src
    for a, b, rep in sorted(edits, reverse=True):
        out = out[:a] + rep + out[b:]
    return out, len(edits), skipped


# ────────────────────────── 主流程 ──────────────────────────
def all_sites():
    sites = []
    for f in sorted(SRC.rglob("*.rs")):
        rel = f.relative_to(SRC).as_posix()
        if rel in DATA_TABLES:
            continue
        # i18n 定义文件(词干 i18n)与 tests.rs 模块文件(经
        # `#[cfg(test)] mod tests;` 挂载,整文件即测试)——与门禁同口径跳过
        if f.stem == "i18n" or f.stem == "tests":
            continue
        ss, _ = scan_sites(f, rel)
        sites.extend(ss)
    return sites


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--gen", action="store_true", help="生成 ftl 段 + sidecar")
    ap.add_argument("--apply", action="store_true", help="改写调用点")
    ap.add_argument("--files", default="", help="apply 子串过滤(逗号分隔)")
    ap.add_argument("--report", action="store_true", help="统计与手动清单")
    a = ap.parse_args()

    sidecar = json.loads(SIDECAR.read_text(encoding="utf-8")) if SIDECAR.exists() else {}

    if a.report or not (a.gen or a.apply):
        sites = all_sites()
        by_kind = collections.Counter(s.kind for s in sites)
        print("=== 分类统计 ===")
        for k, v in by_kind.most_common():
            print(f"{v:5d}  {k}")
        manual = [s for s in sites if s.manual]
        if manual:
            print("=== 手动清单 ===")
            for s in manual:
                print(f"{s.rel}:{s.line} [{s.kind}] {s.note} | {s.text[:56]}")
        sys.exit(0)

    sites = all_sites()

    if a.gen:
        asg = Assigner(sidecar)
        zh_rows = {}
        kind_of = {}
        for s in sites:
            if s.manual:
                continue
            key = asg.key_for(s.rel, s.text)
            if s.kind == "plain":
                zh_rows[key] = s.text
            else:
                conv = convert_format_value(s.text)
                if conv is None:
                    s.kind = "manual:spec"
                    s.note = "format 规格含精度/宽度/Debug,需手动"
                    continue
                zh_rows[key] = conv[0]
            kind_of[key] = s.kind
        for key, zh in data_table_rows():
            asg.keys[zh] = key  # 数据表 key 固定(语义名),不占序号
            zh_rows[key] = zh
        en_rows = {}
        todo = []
        for key, zh in sorted(zh_rows.items()):
            en = en_translate(zh)
            if not en or not check_banned(en):
                todo.append((key, zh))
                en_rows[key] = f"TODO-EN {key}"
            else:
                en_rows[key] = en
        write_sections(
            "\n".join(f"{k} = {zh_rows[k]}" for k in sorted(zh_rows)),
            "\n".join(f"{k} = {en_rows[k]}" for k in sorted(zh_rows)),
        )
        SIDECAR.write_text(
            json.dumps(asg.dump(), ensure_ascii=False, indent=1),
            encoding="utf-8", newline="\n",
        )
        print(f"gen:key {len(zh_rows)} 条(manual 另计);en TODO {len(todo)} 条")
        for k, zh in todo[:80]:
            print(f"  TODO {k} = {zh[:56]}")
        sys.exit(0)

    if a.apply:
        keys = sidecar.get("keys", {})
        subs = [x for x in a.files.split(",") if x]
        rels = sorted({s.rel for s in sites if any(x in s.rel for x in subs)})
        if not rels:
            print("无匹配文件:", subs)
            sys.exit(1)
        tot_e = tot_s = 0
        for rel in rels:
            ss, src = scan_sites(SRC / rel, rel)
            new, n_edit, n_skip = rewrite(src, ss, keys)
            if n_edit:
                (SRC / rel).write_text(new, encoding="utf-8", newline="")
            tot_e += n_edit
            tot_s += n_skip
            print(f"{rel}: 改写 {n_edit},跳过 {n_skip}")
        print(f"合计:改写 {tot_e},跳过 {tot_s}")
        sys.exit(0)


if __name__ == "__main__":
    main()
