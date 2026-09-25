#!/usr/bin/env python3
"""门禁 14(06-2-4):许可口径一致性 —— 本仓协议为 ACL-1.0(ADR-0030)。

两向检查:

1. **正向**:四处许可口径必须齐且一致为 ACL-1.0 ——
   ①根 `LICENSE` 存在且首行是 ACL-1.0;
   ②根 `Cargo.toml` 用 `license-file = "LICENSE"`(ACL-1.0 非 SPDX 标识符,
   不得改用 `license = "…"` 字段);
   ③每个 crate `Cargo.toml` 继承 `license-file.workspace = true`;
   ④`README.md` 的 License 段/徽章含 ACL-1.0。

2. **反向(黑名单)**:仓库「活文档」里不得再出现本仓的 "MIT" 许可声明
   (历史遗留口径)。第三方依赖各自的 MIT/Apache 许可**是合法记述**,只允许
   出现在「参考/依赖文档」目录(docs/deps.md、docs/adr/、docs/design/),
   这些目录不在扫描范围(白名单豁免);若新增合法致谢处,把路径加进
   `ALLOWLIST_HINTS` 并在注释里写明理由。

被扫描的"活文档"= 口径可能被新改动写脏的位置:README/CONTEXT/commands.yaml/
dist 打包物/Cargo.toml/全部源码/工具脚本/i18n。docs/design 等历史设计稿只在
引述依赖许可,不是本仓声明,不扫(防误伤,06-2 任务约束)。
"""
import re
import sys
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent

# 反向扫描的文件集合(glob 相对仓库根;均为"本仓自己的许可口径"可能出现的活文件)
SCAN_GLOBS = [
    "README.md",
    "CONTEXT.md",
    "commands.yaml",
    "Cargo.toml",
    "crates/*/Cargo.toml",
    "dist/*.md",
    "dist/*.ps1",
    "dist/*.bat",
    "crates/**/*.rs",
    "i18n/*.ftl",
    "tools/*.py",
    "tools/*.ps1",
]

# MIT 整词(大小写敏感 —— 许可名总是大写缩写,避免误伤 identify/limit 等词)
MIT_RE = re.compile(r"\bMIT\b")

# 允许的合法第三方致谢(白名单豁免;加入必须注明理由):
# - tools/check_license.py:本门禁脚本自身要引用 "MIT" 这个词做匹配规则(自举)。
ALLOWLIST_HINTS = {
    "tools/check_license.py",
}


def iter_scan_files():
    seen = set()
    for g in SCAN_GLOBS:
        for p in sorted(ROOT.glob(g)):
            # 排除 target/ 构建产物(glob 不会进 target,防御式再滤一次)
            if "target" in p.parts:
                continue
            if p.resolve() in seen:
                continue
            seen.add(p.resolve())
            yield p


def check_positive() -> list[str]:
    """四处口径正向核对;返回问题描述列表。"""
    problems: list[str] = []

    lic = ROOT / "LICENSE"
    if not lic.exists():
        problems.append("根 LICENSE 不存在(应为本仓 ACL-1.0 协议全文)")
    else:
        head = lic.read_text(encoding="utf-8", errors="replace").lstrip("\ufeff")
        first = next((l for l in head.splitlines() if l.strip()), "")
        if "ACL-1.0" not in first:
            problems.append(f"LICENSE 首行不是 ACL-1.0 口径:{first!r}")

    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8", errors="replace")
    if 'license-file = "LICENSE"' not in cargo:
        problems.append('根 Cargo.toml 缺 license-file = "LICENSE"(ACL-1.0 非 SPDX,不许用 license = "…")')

    readme = (ROOT / "README.md").read_text(encoding="utf-8", errors="replace")
    if "ACL-1.0" not in readme:
        problems.append("README.md 许可段/徽章未提及 ACL-1.0")

    crates = sorted((ROOT / "crates").glob("*/Cargo.toml"))
    if not crates:
        problems.append("crates/ 下未找到任何 Cargo.toml")
    for c in crates:
        txt = c.read_text(encoding="utf-8", errors="replace")
        if "license-file.workspace = true" not in txt:
            problems.append(f"{c.relative_to(ROOT)} 缺 license-file.workspace = true(应继承工作区口径)")
    return problems


def check_negative() -> list[str]:
    """活文档 MIT 声明黑名单;返回 `路径:行号` 列表。"""
    problems: list[str] = []
    for p in iter_scan_files():
        rel = p.relative_to(ROOT).as_posix()
        if rel in ALLOWLIST_HINTS:
            continue
        try:
            text = p.read_text(encoding="utf-8", errors="replace")
        except OSError as e:
            problems.append(f"{rel}:读取失败({e})")
            continue
        for i, line in enumerate(text.splitlines(), 1):
            if MIT_RE.search(line):
                problems.append(
                    f"{rel}:{i}:出现 \"MIT\" 许可声明文本 —— 本仓协议是 ACL-1.0(ADR-0030);"
                    "第三方依赖的许可记述请放 docs/deps.md(白名单目录)"
                )
    return problems


def main() -> int:
    pos = check_positive()
    neg = check_negative()
    if pos or neg:
        for m in pos + neg:
            print(f"license: FAIL {m}")
        print(
            "license: 补救 —— ①四处口径(README / LICENSE / Cargo.toml / dist 打包说明)"
            "统一为 ACL-1.0(与 Artboard 同款,见 ADR-0030);"
            "②若为第三方致谢等合法引用,请把文件加入 tools/check_license.py 的 ALLOWLIST_HINTS 并注明理由"
        )
        print(f"license: FAIL(正向 {len(pos)} 项 / 黑名单 {len(neg)} 项)")
        return 1
    print("license: PASS(四处口径一致 ACL-1.0;活文档无 MIT 声明残留)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
