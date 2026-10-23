import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace

MODULE_PATH = Path(__file__).resolve().parents[1] / "test_artifact_guard.py"
SPEC = importlib.util.spec_from_file_location("test_artifact_guard", MODULE_PATH)
guard = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(guard)


class TestArtifactGuard(unittest.TestCase):
    def test_target_is_isolated_per_checkout(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp) / "cargo-target"
            first = guard.test_target_dir(Path(temp) / "one", base)
            second = guard.test_target_dir(Path(temp) / "two", base)
            self.assertNotEqual(first, second)
            self.assertEqual(first.parent, base)
            self.assertRegex(first.name, r"^vellumbench-tests-[0-9a-f]{12}$")

    def test_under_limit_is_left_untouched(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "test-target"
            target.mkdir()
            artifact = target / "small.bin"
            artifact.write_bytes(b"123456789")

            result = guard.cleanup_test_target(target, limit_bytes=10)

            self.assertEqual(result["after"], 9)
            self.assertFalse(result["full"])
            self.assertTrue(artifact.exists())

    def test_over_limit_prunes_incremental_cache_first(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "test-target"
            incremental = target / "debug" / "incremental"
            deps = target / "debug" / "deps"
            incremental.mkdir(parents=True)
            deps.mkdir(parents=True)
            (incremental / "cache.bin").write_bytes(b"x" * 12)
            kept = deps / "current.rlib"
            kept.write_bytes(b"12345678")

            result = guard.cleanup_test_target(target, limit_bytes=10)

            self.assertEqual(result["after"], 8)
            self.assertFalse(result["full"])
            self.assertGreater(result["incremental"], 0)
            self.assertTrue(kept.exists())

    def test_over_limit_falls_back_to_isolated_target_reset(self):
        with tempfile.TemporaryDirectory() as temp:
            parent = Path(temp)
            target = parent / "test-target"
            sibling = parent / "other-project"
            target.mkdir()
            sibling.mkdir()
            (target / "artifact.bin").write_bytes(b"x" * 20)
            sentinel = sibling / "keep.txt"
            sentinel.write_text("untouched", encoding="utf-8")

            result = guard.cleanup_test_target(target, limit_bytes=10)

            self.assertTrue(result["full"])
            self.assertEqual(result["after"], 0)
            self.assertTrue(sentinel.exists())

    def test_symlink_target_is_never_cleaned(self):
        with tempfile.TemporaryDirectory() as temp:
            parent = Path(temp)
            external = parent / "external"
            external.mkdir()
            sentinel = external / "keep.txt"
            sentinel.write_text("untouched", encoding="utf-8")
            link = parent / "test-target-link"
            try:
                link.symlink_to(external, target_is_directory=True)
            except (OSError, NotImplementedError):
                self.skipTest("symlink creation is unavailable")

            with self.assertRaises(RuntimeError):
                guard.cleanup_test_target(link, force=True)
            self.assertTrue(sentinel.exists())

    def test_only_stale_kiln_temp_directories_are_removed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            now = 2_000_000_000.0
            stale = root / "kiln-stale"
            recent = root / "kiln-recent"
            other = root / "other-stale"
            stale.mkdir()
            recent.mkdir()
            other.mkdir()
            os.utime(stale, (now - 86_401, now - 86_401))
            os.utime(recent, (now - 86_399, now - 86_399))
            os.utime(other, (now - 100_000, now - 100_000))

            removed, errors = guard.cleanup_kiln_temp(root, now=now)

            self.assertEqual([path.name for path in removed], ["kiln-stale"])
            self.assertFalse(stale.exists())
            self.assertTrue(recent.exists())
            self.assertTrue(other.exists())
            self.assertEqual(errors, [])

    def test_failed_test_still_enforces_cap_and_preserves_exit_code(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / "test-target"

            def fake_runner(command, *, cwd, env, check):
                output = Path(env["CARGO_TARGET_DIR"]) / "debug" / "deps" / "large.bin"
                output.parent.mkdir(parents=True)
                output.write_bytes(b"x" * 20)
                return SimpleNamespace(returncode=7)

            exit_code = guard.run_guarded(
                ["fake-test"], target, root=Path(temp), limit_bytes=10,
                runner=fake_runner, kiln_temp_root=Path(temp) / "kiln-temp",
            )

            self.assertEqual(exit_code, 7)
            self.assertEqual(guard.directory_size(target), 0)


if __name__ == "__main__":
    unittest.main()
