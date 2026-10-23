from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TRANSLATIONS = {
    "i18n/zh.ftl": {
        "ui-animation-presets": "命名动画预设",
        "ui-animation-preset-name": "预设名称",
        "ui-animation-save-preset": "保存为预设",
        "ui-animation-preset-apply": "应用",
        "ui-animation-preset-delete": "删除预设",
        "ui-animation-preset-saved": "动画预设已保存",
        "ui-animation-preset-applied": "动画预设已应用",
        "ui-animation-preset-deleted": "动画预设已删除",
        "ui-animation-presets-empty": "暂无命名预设",
    },
    "i18n/en.ftl": {
        "ui-animation-presets": "Named Animation Presets",
        "ui-animation-preset-name": "Preset name",
        "ui-animation-save-preset": "Save preset",
        "ui-animation-preset-apply": "Apply",
        "ui-animation-preset-delete": "Delete preset",
        "ui-animation-preset-saved": "Animation preset saved",
        "ui-animation-preset-applied": "Animation preset applied",
        "ui-animation-preset-deleted": "Animation preset deleted",
        "ui-animation-presets-empty": "No named animation presets",
    },
}


def is_ui_key(line: bytes) -> bool:
    return line.lstrip().startswith(b"ui-")


def key_of(line: bytes) -> bytes | None:
    stripped = line.lstrip()
    if b" =" not in stripped:
        return None
    return stripped.split(b" =", 1)[0]


def move_ui_keys(path: Path, additions: dict[str, str]) -> tuple[int, int, str]:
    original = path.read_bytes()
    nul_count = original.count(b"\x00")
    newline = b"\r\n" if b"\r\n" in original else b"\n"
    lines = original.splitlines(keepends=True)
    begin = next(i for i, line in enumerate(lines) if b"BEGIN cmd-catalog" in line)
    end = next(i for i in range(begin + 1, len(lines)) if b"END cmd-catalog" in lines[i])

    catalog_ui = [line for line in lines[begin + 1 : end] if is_ui_key(line)]
    catalog_keep = [line for line in lines[begin + 1 : end] if not is_ui_key(line)]
    catalog = lines[: begin + 1] + catalog_keep + [lines[end]]
    outside = lines[end + 1 :]

    # Keep existing out-of-catalog labels and add only missing animation-preset keys.
    existing: dict[bytes, int] = {}
    for line in catalog + outside:
        key = key_of(line)
        if key is not None and key.startswith(b"ui-"):
            existing[key] = existing.get(key, 0) + 1
    addition_lines: list[bytes] = []
    for key, value in additions.items():
        encoded_key = key.encode("ascii")
        count = existing.get(encoded_key, 0)
        if count > 1:
            raise ValueError(f"duplicate UI key {key} in {path}")
        if count == 0:
            addition_lines.append(f"{key} = {value}".encode("utf-8") + newline)

    shell_marker = next(
        (i for i, line in enumerate(outside) if b"vb_shell(" in line and line.lstrip().startswith(b"#")),
        None,
    )
    insert_at = shell_marker if shell_marker is not None else len(outside)
    moved = catalog_ui + addition_lines
    if moved:
        # Preserve all existing bytes; only move the raw UI lines and insert the new labels.
        prefix = catalog
        suffix = outside[:insert_at]
        if suffix and not suffix[-1].endswith((b"\n", b"\r")):
            suffix[-1] += newline
        separator = []
        if suffix and suffix[-1].strip():
            separator.append(newline)
        section = b"# VellumBench shared animation preset UI labels".decode().encode("ascii") + newline
        result = prefix + suffix + separator + [section] + moved + [newline] + outside[insert_at:]
    else:
        result = lines

    updated = b"".join(result)
    if updated.count(b"\x00") != nul_count:
        raise ValueError(f"NUL count changed in {path}")
    # Exactly one definition of every UI key after relocating the block.
    counts: dict[bytes, int] = {}
    for line in updated.splitlines():
        key = key_of(line)
        if key is not None and key.startswith(b"ui-"):
            counts[key] = counts.get(key, 0) + 1
    for key in additions:
        if counts.get(key.encode("ascii")) != 1:
            raise ValueError(f"expected one {key} definition in {path}, got {counts.get(key.encode('ascii'), 0)}")
    if updated != original:
        path.write_bytes(updated)
    return len(catalog_ui), len(addition_lines), f"NULs={nul_count}"


for relative, additions in TRANSLATIONS.items():
    moved, inserted, integrity = move_ui_keys(ROOT / relative, additions)
    print(f"{relative}: moved={moved}, inserted={inserted}, {integrity}")
