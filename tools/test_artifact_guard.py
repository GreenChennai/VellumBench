#!/usr/bin/env python3
"""Run Cargo tests in an isolated, capped artifact directory.

The cap is 10 GB (decimal, 10,000,000,000 bytes). On each managed test run,
artifacts are pruned before execution if already over the cap and again after
the process exits, including failure/interrupt paths. Only this workspace's
isolated Cargo target directory is eligible for deletion.
"""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
MAX_BYTES = 10_000_000_000


def cargo_target_base(root: Path = ROOT) -> Path:
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=root,
        check=False,
        capture_output=True,
    )
    if result.returncode != 0:
        error = result.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(f"cargo metadata exited {result.returncode}: {error}")
    metadata = json.loads(result.stdout.decode("utf-8-sig"))
    target = metadata.get("target_directory")
    if not target:
        raise RuntimeError("cargo metadata did not return target_directory")
    return Path(target).expanduser().resolve()


def test_target_dir(root: Path = ROOT, target_base: Path | None = None) -> Path:
    """Return a per-checkout target directory, isolated from other projects."""
    normalized_root = os.path.normcase(str(root.resolve()))
    key = hashlib.sha256(normalized_root.encode("utf-8")).hexdigest()[:12]
    base = target_base.resolve() if target_base else cargo_target_base(root)
    return base / f"vellumbench-tests-{key}"


def directory_size(path: Path) -> int:
    if not path.exists():
        return 0
    if path.is_symlink() or not path.is_dir():
        raise RuntimeError(f"refusing to size non-directory or symlink: {path}")

    total = 0

    def raise_walk_error(error: OSError) -> None:
        raise error

    for current, directories, files in os.walk(
        path, topdown=True, onerror=raise_walk_error, followlinks=False
    ):
        current_path = Path(current)
        directories[:] = [name for name in directories
                          if not (current_path / name).is_symlink()]
        for name in files:
            file_path = current_path / name
            if not file_path.is_symlink():
                total += file_path.stat().st_size
    return total


def _remove_path(path: Path) -> None:
    if path.is_symlink() or path.is_file():
        path.unlink()
    elif path.exists():
        shutil.rmtree(path)


def cleanup_test_target(
    target: Path, *, force: bool = False, limit_bytes: int = MAX_BYTES
) -> dict[str, int | bool]:
    """Prune incremental caches, then clear only this isolated target if needed."""
    target = Path(target)
    if target.is_symlink():
        raise RuntimeError(f"refusing to clean symlink target: {target}")
    target = target.resolve()
    if target.exists() and not target.is_dir():
        raise RuntimeError(f"refusing to clean non-directory target: {target}")

    before = directory_size(target)
    if not force and before <= limit_bytes:
        return {"before": before, "after": before, "incremental": 0, "full": False}
    if not target.exists():
        return {"before": 0, "after": 0, "incremental": 0, "full": False}

    incremental_dirs: list[Path] = []
    for current, directories, _ in os.walk(target, topdown=True, followlinks=False):
        current_path = Path(current)
        directories[:] = [name for name in directories
                          if not (current_path / name).is_symlink()]
        if current_path.name.casefold() == "incremental":
            incremental_dirs.append(current_path)
            directories.clear()
    for directory in sorted(incremental_dirs, key=lambda item: len(item.parts), reverse=True):
        _remove_path(directory)

    after_incremental = directory_size(target)
    full = force or after_incremental > limit_bytes
    if full:
        for child in target.iterdir():
            _remove_path(child)

    after = directory_size(target)
    if after > limit_bytes:
        raise RuntimeError(
            f"test target remains over limit after cleanup: {after} > {limit_bytes} bytes"
        )
    return {
        "before": before,
        "after": after,
        "incremental": before - after_incremental,
        "full": full,
    }


def cleanup_kiln_temp(
    temp_root: Path | None = None,
    *,
    max_age_seconds: int = 24 * 60 * 60,
    now: float | None = None,
) -> tuple[list[Path], list[str]]:
    """Preserve the legacy cleanup of stale, top-level kiln-* temp folders."""
    root = Path(temp_root or tempfile.gettempdir())
    if not root.exists() or root.is_symlink() or not root.is_dir():
        return [], []

    cutoff = (time.time() if now is None else now) - max_age_seconds
    removed: list[Path] = []
    errors: list[str] = []
    for child in root.iterdir():
        if not child.name.startswith("kiln-") or child.is_symlink() or not child.is_dir():
            continue
        try:
            if child.lstat().st_mtime < cutoff:
                shutil.rmtree(child)
                removed.append(child)
        except OSError as error:
            errors.append(f"{child}: {error}")
    return removed, errors


def _report_kiln_cleanup(temp_root: Path | None = None) -> None:
    removed, errors = cleanup_kiln_temp(temp_root)
    if removed:
        print(f"removed {len(removed)} stale kiln-* temp directories (>24 hours)")
    for error in errors:
        print(f"warning: could not remove stale kiln temp directory: {error}", file=sys.stderr)


@contextlib.contextmanager
def target_lock(target: Path, *, blocking: bool):
    """Serialize managed test/cleanup operations across processes and platforms."""
    target.parent.mkdir(parents=True, exist_ok=True)
    lock_path = target.parent / f".{target.name}.lock"
    with lock_path.open("a+b") as handle:
        locked = False
        try:
            if os.name == "nt":
                import msvcrt

                handle.seek(0, os.SEEK_END)
                if handle.tell() == 0:
                    handle.write(b"\0")
                    handle.flush()
                handle.seek(0)
                while True:
                    try:
                        msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                        break
                    except OSError:
                        if not blocking:
                            raise
                        time.sleep(0.25)
            else:
                import fcntl

                flags = fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB)
                fcntl.flock(handle.fileno(), flags)
            locked = True
            yield
        except OSError as error:
            if not blocking:
                raise RuntimeError("test artifacts are in use by a managed test run") from error
            raise
        finally:
            if locked:
                if os.name == "nt":
                    import msvcrt

                    handle.seek(0)
                    msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
                else:
                    import fcntl

                    fcntl.flock(handle.fileno(), fcntl.LOCK_UN)


def format_gb(size: int) -> str:
    return f"{size / 1_000_000_000:.2f} GB"


def _report_cleanup(
    result: dict[str, int | bool], label: str, limit_bytes: int = MAX_BYTES
) -> None:
    before = int(result["before"])
    after = int(result["after"])
    if before == after and not result["full"]:
        print(f"{label}: {format_gb(after)} / {format_gb(limit_bytes)}")
        return
    print(
        f"{label}: {format_gb(before)} -> {format_gb(after)}; "
        f"reclaimed {format_gb(before - after)}"
        + (" (isolated test target reset)" if result["full"] else " (incremental cache pruned)")
    )


def run_guarded(
    command: list[str],
    target: Path,
    *,
    root: Path = ROOT,
    limit_bytes: int = MAX_BYTES,
    runner=subprocess.run,
    kiln_temp_root: Path | None = None,
) -> int:
    if not command:
        print("error: provide a command after `run --`", file=sys.stderr)
        return 2

    with target_lock(target, blocking=True):
        try:
            before = cleanup_test_target(target, limit_bytes=limit_bytes)
            _report_cleanup(before, "pre-test artifacts", limit_bytes)
        except (OSError, RuntimeError) as error:
            print(f"error: cannot enforce test artifact cap before tests: {error}", file=sys.stderr)
            return 3

        environment = os.environ.copy()
        environment["CARGO_TARGET_DIR"] = str(target)
        print(f"test artifact target: {target}")
        try:
            completed = runner(command, cwd=root, env=environment, check=False)
            test_exit = completed.returncode
        except KeyboardInterrupt:
            test_exit = 130
        except OSError as error:
            print(f"error: failed to start test command: {error}", file=sys.stderr)
            test_exit = 127

        try:
            after = cleanup_test_target(target, limit_bytes=limit_bytes)
            _report_cleanup(after, "post-test artifacts", limit_bytes)
            _report_kiln_cleanup(kiln_temp_root)
        except (OSError, RuntimeError) as error:
            print(f"error: cannot enforce test artifact cap after tests: {error}", file=sys.stderr)
            return test_exit if test_exit != 0 else 3
        return test_exit


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="action", required=True)
    run_parser = commands.add_parser("run", help="run a command with pre/post cap enforcement")
    run_parser.add_argument("command", nargs=argparse.REMAINDER)
    clean_parser = commands.add_parser("clean", help="clean this workspace's test target if over cap")
    clean_parser.add_argument("--full", action="store_true", help="clear the isolated test target")
    args = parser.parse_args(argv)

    try:
        target = test_target_dir()
    except (OSError, subprocess.CalledProcessError, RuntimeError, json.JSONDecodeError) as error:
        print(f"error: cannot resolve Cargo test target directory: {error}", file=sys.stderr)
        return 3

    if args.action == "run":
        command = args.command[1:] if args.command[:1] == ["--"] else args.command
        try:
            return run_guarded(command, target)
        except (OSError, RuntimeError) as error:
            print(f"error: managed test run failed: {error}", file=sys.stderr)
            return 3

    try:
        with target_lock(target, blocking=False):
            result = cleanup_test_target(target, force=args.full)
        _report_cleanup(result, "test artifacts")
        _report_kiln_cleanup()
    except (OSError, RuntimeError) as error:
        print(f"error: test artifact cleanup failed: {error}", file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
