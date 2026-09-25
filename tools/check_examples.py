#!/usr/bin/env python3
"""示例体检门禁(W9,副文档 03 §4 / 03-2-3)。

对 examples/ 下每个示例断言:
  ① `vellum-cli tree --json` 可读出(文档可被引擎解析);
  ② 画板数 / 元素数 / 文字数符合基线(基线值由本脚本下方 BASELINES 写死,
     生成方式:修复完成后跑 `vellum-cli --doc examples/<name> tree --json`
     统计 kind 计数,人工核对语义后落入本表;改动示例内容须同步更新基线);
  ③ 同层级(同一父节点)元素几何覆叠检出 —— 读 tree 的 box 矩形做相交检测;
     "有意叠放"(背景板、侧栏装饰条、层叠山体等)经 ALLOW 覆叠白名单豁免;
  ④ 画板原点不重叠(纵向堆叠语义)。

背景:P0-④ —— examples/landing 第 1/3 张卡缺 `feature-card-1/3` 类,
两卡都落回 x=0 互相覆叠、右侧 960px 空白。本门禁保证"随仓库的示例"
不再无人验证;注入缺陷(删掉 -1 类)会被 ③ 检出。

用法:python tools/check_examples.py [--cli <vellum-cli 路径>]
退出码:0 = PASS,1 = FAIL。
"""
import argparse
import json
import subprocess
import sys
import pathlib

REPO = pathlib.Path(__file__).resolve().parent.parent

# 每示例基线:name → (画板数, 元素总数(含画板), 文字节点数)。
# 生成方式见文件头注释;landing 为修复 feature-card-1/3 类之后的正确形态。
BASELINES = {
    "landing": (2, 21, 10),
    "poster": (1, 10, 4),
    "resume": (1, 14, 8),
}

# 画板直接子层的几何基线:示例 → 画板名 → 节点名 → [x, y, w, h](画板本地)。
# 生成方式:修复完成后跑 `vellum-cli --doc examples/<name> tree --json`,
# 取画板直接子节点的 box(与 CSS 声明一致,与平台字体度量无关,稳定)。
# 作用:数量基线抓不住"单类删除"(如删掉 feature-card-1 只会让卡片 1
# 从 x=120 漂到 x=0,不产生覆叠),几何基线才能锁死定位意图。
CHILD_GEOM = {
    "landing": {
        "Hero": {
            "Hero 背景": [0, 0, 1440, 600],
            "顶部徽章": [120, 96, 132, 36],
            "主标题": [120, 200, 760, 96],
            "副标题": [120, 320, 520, 40],
            "CTA 按钮": [120, 420, 180, 56],
            "装饰圆点": [1020, 140, 280, 280],
        },
        "Features": {
            "卡片 产地": [120, 96, 360, 300],
            "卡片 烘焙": [540, 96, 360, 300],
            "卡片 配送": [960, 96, 360, 300],
        },
    },
    "poster": {
        "海报": {
            "背景": [0, 0, 750, 1334],
            "落日": [275, 500, 200, 200],
            "山一": [-80, 900, 500, 400],
            "山二": [330, 950, 560, 420],
            "主标题": [60, 140, 630, 110],
            "日期": [62, 270, 500, 44],
            "地点": [62, 330, 500, 40],
            "购票": [62, 430, 240, 64],
        },
    },
    "resume": {
        "简历": {
            "侧栏": [0, 0, 260, 1123],
            "姓名": [300, 80, 400, 60],
            "职位": [300, 150, 400, 36],
            "联系方式": [300, 200, 420, 30],
            "经历标题": [300, 280, 300, 34],
            "经历一": [300, 340, 420, 120],
            "经历二": [300, 480, 420, 120],
            "侧栏点缀": [0, 0, 8, 1123],
        },
    },
}

# 有意叠放白名单:按 (节点名A, 节点名B) 无序对豁免(仅同层级判定用)。
ALLOW_OVERLAP = {
    "landing": {
        # Hero 背景渐变板垫底,徽章/标题/文案/圆点都画在它上面
        ("Hero 背景", "顶部徽章"),
        ("Hero 背景", "主标题"),
        ("Hero 背景", "副标题"),
        ("Hero 背景", "CTA 按钮"),
        ("Hero 背景", "装饰圆点"),
    },
    "poster": {
        # 夜空渐变垫底 + 山体互相层叠(海报设计语义)
        ("背景", "落日"),
        ("背景", "山一"),
        ("背景", "山二"),
        ("背景", "主标题"),
        ("背景", "日期"),
        ("背景", "地点"),
        ("背景", "购票"),
        ("山一", "山二"),
    },
    "resume": {
        # 侧栏底色 + 8px 品牌竖条贴边
        ("侧栏", "侧栏点缀"),
    },
}

# 相交判定阈值:两轴重叠都超过该值(px)才算覆叠(贴合边/1px 抖动不算)
OVERLAP_EPS = 2.0


def load_tree(cli: str, example: str):
    """跑 vellum-cli tree --json,返回 (文档字典, 错误信息)。"""
    doc_dir = REPO / "examples" / example
    try:
        proc = subprocess.run(
            [cli, "--doc", str(doc_dir), "tree", "--json"],
            capture_output=True,
            text=True,
            timeout=120,
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        return None, f"无法执行 {cli}: {e}"
    if proc.returncode != 0:
        return None, f"tree --json 退出码 {proc.returncode}: {proc.stderr.strip()[:300]}"
    try:
        return json.loads(proc.stdout), None
    except json.JSONDecodeError as e:
        return None, f"tree --json 输出不是合法 JSON: {e}"


def walk_nodes(node, parent_name, depth, out):
    """先序收集 (节点, 父节点名, 深度)。depth=0 是画板。"""
    out.append((node, parent_name, depth))
    for c in node.get("children", []) or []:
        walk_nodes(c, node.get("name", "?"), depth + 1, out)


def rect_of(node):
    b = node.get("box") or {}
    return (float(b.get("x", 0)), float(b.get("y", 0)),
            float(b.get("w", 0)), float(b.get("h", 0)))


def intersect(a, b):
    """两矩形 [x,y,w,h] 的相交 (w, h);不相交返回 (0, 0)。"""
    iw = min(a[0] + a[2], b[0] + b[2]) - max(a[0], b[0])
    ih = min(a[1] + a[3], b[1] + b[3]) - max(a[1], b[1])
    return (max(0.0, iw), max(0.0, ih))


def check_example(cli: str, name: str, fails: list):
    doc, err = load_tree(cli, name)
    if doc:
        check_tree(name, doc, fails)
    else:
        fails.append(f"[{name}] ① tree --json 不可读:{err}")


def check_tree(name: str, doc: dict, fails: list):
    boards = doc.get("artboards") or []
    nodes = []
    for ab in boards:
        walk_nodes(ab, None, 0, nodes)

    # ② 基线:画板 / 元素 / 文字
    n_ab = len(boards)
    n_all = len(nodes)
    n_text = sum(1 for n, _, _ in nodes if n.get("kind") == "text")
    base = BASELINES.get(name)
    if base is None:
        fails.append(f"[{name}] ② BASELINES 缺少该示例的基线登记")
    else:
        if (n_ab, n_all, n_text) != base:
            fails.append(
                f"[{name}] ② 基线不符:画板/元素/文字 = {(n_ab, n_all, n_text)},"
                f"期望 {base}(示例内容变更后请核对并更新 tools/check_examples.py)"
            )

    # ②-b 画板直接子层几何基线(抓"类名被删导致单节点漂位"的注入缺陷)
    geom_base = CHILD_GEOM.get(name, {})
    for ab in boards:
        ab_name = ab.get("name", "?")
        expected = geom_base.get(ab_name)
        if expected is None:
            continue
        seen = {}
        for c in ab.get("children", []) or []:
            seen[c.get("name", "?")] = rect_of(c)
        for node_name, want in expected.items():
            got = seen.get(node_name)
            if got is None:
                fails.append(f"[{name}] ②-b 画板 `{ab_name}` 缺少子节点 `{node_name}`")
                continue
            got_r = [round(v) for v in got]
            if any(abs(g - w) > 1 for g, w in zip(got_r, want)):
                fails.append(
                    f"[{name}] ②-b `{ab_name}/{node_name}` 几何漂移:"
                    f"实际 {got_r},期望 {want}"
                    f"(常见原因:HTML 类名与 CSS 规则不匹配)"
                )

    # ③ 同层级覆叠(画板直接子层与各嵌套层都查;白名单豁免有意叠放)
    allow = ALLOW_OVERLAP.get(name, set())
    by_parent = {}
    for n, parent, depth in nodes:
        if depth == 0:
            continue  # 画板之间由 ④ 检查
        by_parent.setdefault(parent, []).append(n)
    for parent, sibs in by_parent.items():
        for i in range(len(sibs)):
            for j in range(i + 1, len(sibs)):
                a, b = sibs[i], sibs[j]
                ra, rb = rect_of(a), rect_of(b)
                iw, ih = intersect(ra, rb)
                if iw > OVERLAP_EPS and ih > OVERLAP_EPS:
                    pair = {(a.get("name", "?"), b.get("name", "?"))}
                    if pair & allow:
                        continue
                    fails.append(
                        f"[{name}] ③ 覆叠:`{a.get('name')}` 与 `{b.get('name')}`"
                        f"(父:{parent or 'body'})相交 {iw:.0f}x{ih:.0f}px;"
                        f"若为有意叠放请在 ALLOW_OVERLAP 登记豁免"
                    )

    # ④ 画板矩形互不重叠(纵向堆叠;间距不限)
    for i in range(len(boards)):
        for j in range(i + 1, len(boards)):
            iw, ih = intersect(rect_of(boards[i]), rect_of(boards[j]))
            if iw > OVERLAP_EPS and ih > OVERLAP_EPS:
                fails.append(
                    f"[{name}] ④ 画板矩形重叠:`{boards[i].get('name')}` 与 "
                    f"`{boards[j].get('name')}` 相交 {iw:.0f}x{ih:.0f}px"
                )

    # 补充:画板尺寸须为正(tree 可读但空板会导致导出白版)
    for ab in boards:
        _, _, w, h = rect_of(ab)
        if w <= 0 or h <= 0:
            fails.append(f"[{name}] 画板 `{ab.get('name')}` 尺寸非正:{w}x{h}")


def main() -> int:
    ap = argparse.ArgumentParser(description="示例体检门禁(W9)")
    ap.add_argument("--cli", default=str(REPO / "target" / "debug" / "vellum-cli.exe"),
                    help="vellum-cli 可执行文件路径")
    ap.add_argument("--examples", nargs="*", default=sorted(BASELINES),
                    help="要体检的示例目录名(默认 BASELINES 全部)")
    args = ap.parse_args()

    fails: list = []
    for name in args.examples:
        if not (REPO / "examples" / name / "index.html").exists():
            fails.append(f"[{name}] examples/{name}/index.html 不存在")
            continue
        check_example(args.cli, name, fails)

    for f in fails:
        print(f"[examples] {f}")
    print(f"check_examples: {'FAIL' if fails else 'PASS'}"
          f"({len(args.examples)} 示例,{len(fails)} 处不合格)")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
