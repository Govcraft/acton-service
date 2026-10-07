"""Portable download archives must hydrate a different Cargo home safely."""

import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from registry_cache import PREFIX, ROOT, SNAPSHOT, cargo_home, hydrate, identity, stage


class RegistryTests(unittest.TestCase):
    def test_lockfile_identity_is_equal_across_checkout_line_endings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            lock = root / "Cargo.lock"
            lock.write_bytes(b"version = 4\n[[package]]\n")
            linux = identity(root)
            lock.write_bytes(b"version = 4\r\n[[package]]\r\n")
            self.assertEqual(identity(root), linux)
            self.assertTrue(linux.startswith(PREFIX))
            self.assertFalse(linux.startswith("ci-v2-registry--old"))
            lock.write_bytes(b"version = 4\n# dependency changed\n")
            self.assertNotEqual(identity(root), linux)

    def test_relative_archive_restores_into_another_home_without_tools_or_secrets(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "linux-home"
            checkout = root / "linux-workspace"
            checkout.mkdir()
            payload = {
                "registry/cache/index/serde.crate": b"crate download",
                "registry/index/index/.cache/se/rd/serde": b"index metadata",
                "git/db/repository/objects/pack/data.pack": b"git objects",
            }
            excluded = {
                "registry/src/index/serde/lib.rs": b"extracted source",
                "bin/cargo": b"toolchain executable",
                "config.toml": b"runner config",
                "credentials.toml": b"secret",
            }
            for name, contents in (payload | excluded).items():
                file = source / name
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_bytes(contents)
            stage(source, checkout)
            archive = shutil.make_archive(
                str(root / "downloads"), "tar", root_dir=checkout, base_dir=SNAPSHOT
            )
            # Remove the original home, making an absolute-path restore unusable.
            shutil.rmtree(source)
            windows_checkout = root / "windows-workspace"
            windows_checkout.mkdir()
            shutil.unpack_archive(archive, windows_checkout)
            destination = root / "windows-home"
            destination.mkdir()
            (destination / "config.toml").write_text("keep runner config")
            hydrate(destination, windows_checkout)
            for name, contents in payload.items():
                self.assertEqual((destination / name).read_bytes(), contents)
            for name in excluded.keys() - {"config.toml"}:
                self.assertFalse((destination / name).exists())
            self.assertEqual(
                (destination / "config.toml").read_text(), "keep runner config"
            )

    def test_staging_removes_stale_downloads_and_hydration_preserves_existing_ones(
        self,
    ):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "cargo-home"
            cache = home / "registry/cache/index"
            cache.mkdir(parents=True)
            (cache / "first.crate").write_bytes(b"first")
            stage(home, root)
            (cache / "first.crate").unlink()
            (cache / "second.crate").write_bytes(b"second")
            stage(home, root)
            self.assertFalse(
                (root / SNAPSHOT / "registry/cache/index/first.crate").exists()
            )
            target = root / "another-home"
            (target / "registry/cache/index").mkdir(parents=True)
            (target / "registry/cache/index/existing.crate").write_bytes(b"existing")
            hydrate(target, root)
            self.assertTrue((target / "registry/cache/index/existing.crate").exists())
            self.assertEqual(
                (target / "registry/cache/index/second.crate").read_bytes(), b"second"
            )

    def test_cache_misses_and_explicit_cargo_home_do_not_change_runner_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / "custom-home"
            hydrate(home, root)
            self.assertFalse(home.exists())
            self.assertEqual(cargo_home({"CARGO_HOME": str(home)}), home)
            self.assertEqual(cargo_home({}), Path.home() / ".cargo")

    def test_every_registry_consumer_uses_the_portable_action(self):
        action = (ROOT / ".github/actions/registry-cache/action.yml").read_text()
        self.assertIn(f"path: {SNAPSHOT}", action)
        self.assertIn("enableCrossOsArchive: true", action)
        self.assertNotIn("~", action)
        for workflow in (
            "build.yml",
            "qualification.yml",
            "rust-validation.yml",
            "cache-maintenance.yml",
        ):
            content = (ROOT / ".github/workflows" / workflow).read_text()
            self.assertIn("./.github/actions/registry-cache", content)
            self.assertNotIn("~/.cargo/registry", content)
            self.assertNotIn("hashFiles('Cargo.lock')", content)


if __name__ == "__main__":
    unittest.main()
