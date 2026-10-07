"""Shared-cache scope, feature identity, and destructive warming boundaries."""

import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cache import ROOT, closed_pr_caches, compiler_identity, inventory, obsolete
from profiles import CACHE_WRITERS, cache_family
from run_profile import commands as validation_commands
from warm_caches import commands, missing_matrix, strip_target_editions, warm


class IdentityTests(unittest.TestCase):
    def fixture(self, root):
        (root / "Cargo.toml").write_text('[workspace]\nmembers = ["facade"]\n')
        (root / "facade/src").mkdir(parents=True)
        (root / "facade/Cargo.toml").write_text(
            '[package]\nname = "facade"\nversion = "1.0.0"\n[features]\noptional = []\n'
        )
        (root / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.99.0"\n')
        (root / "Cargo.lock").write_text("version = 4\n")

    def test_feature_changes_invalidate_immutable_keys_but_source_edits_do_not(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            initial = compiler_identity("full", "Linux", "X64", root)
            (root / "facade/src/lib.rs").write_text("changed source")
            self.assertEqual(compiler_identity("full", "Linux", "X64", root), initial)
            with (root / "facade/Cargo.toml").open("a") as stream:
                stream.write("new = []\n")
            changed = compiler_identity("full", "Linux", "X64", root)
            self.assertNotEqual(changed[0], initial[0])
            self.assertEqual(changed[1], initial[1])

    def test_recipe_build_settings_lockfile_and_compiler_change_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            for path in (
                "scripts/ci/warm_caches.py",
                ".github/workflows/rust-validation.yml",
                ".cargo/config.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
            ):
                before = compiler_identity("full", "Linux", "X64", root)[0]
                file = root / path
                file.parent.mkdir(parents=True, exist_ok=True)
                with file.open("a") as stream:
                    stream.write("\n# changed\n")
                self.assertNotEqual(
                    compiler_identity("full", "Linux", "X64", root)[0], before
                )

    def test_platform_architecture_and_family_cannot_share_compiler_artifacts(self):
        identities = {
            compiler_identity(*values)[0]
            for values in [
                ("full", "Linux", "X64"),
                ("full", "Windows", "X64"),
                ("full", "Linux", "ARM64"),
                ("mssql", "Linux", "X64"),
            ]
        }
        self.assertEqual(len(identities), 4)
        with self.assertRaises(ValueError):
            compiler_identity("../full", "Linux", "X64")

    def test_globbed_workspace_member_features_are_hashed(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            (root / "Cargo.toml").write_text('[workspace]\nmembers = ["fac*"]\n')
            before = compiler_identity("full", "Linux", "X64", root)[0]
            with (root / "facade/Cargo.toml").open("a") as stream:
                stream.write("extra = []\n")
            self.assertNotEqual(
                compiler_identity("full", "Linux", "X64", root)[0], before
            )

    def test_windows_checkout_line_endings_preserve_planned_identity(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.fixture(root)
            before = compiler_identity("windows", "Windows", "X64", root)[0]
            for file in root.rglob("*"):
                if file.is_file():
                    file.write_bytes(file.read_bytes().replace(b"\n", b"\r\n"))
            self.assertEqual(
                compiler_identity("windows", "Windows", "X64", root)[0], before
            )


class PlanningTests(unittest.TestCase):
    def entries(self):
        return [
            {"key": row["key"], "ref": "refs/heads/main"}
            for row in missing_matrix([])["include"]
        ]

    def test_fully_warmed_main_skips_the_entire_compile_matrix(self):
        self.assertEqual(missing_matrix(self.entries()), {"include": []})

    def test_pr_and_tag_keys_are_not_shared_main_hits(self):
        entries = self.entries()
        for ref in ("refs/pull/178/merge", "refs/tags/acton-service-v0.47.0"):
            self.assertEqual(
                len(missing_matrix([{**e, "ref": ref} for e in entries])["include"]), 8
            )

    def test_only_one_missing_graph_is_selected(self):
        entries = self.entries()
        missing = entries.pop()
        rows = missing_matrix(entries)["include"]
        self.assertEqual(len(rows), 1)
        self.assertEqual(rows[0]["key"], missing["key"])

    def test_sql_server_has_a_separate_shared_family(self):
        self.assertEqual(cache_family("mssql"), "mssql")
        self.assertEqual(cache_family("mssql-integration"), "mssql")
        self.assertEqual(cache_family("windows-mssql"), "windows")
        self.assertEqual(cache_family("audit-turso"), "turso")


class WarmingTests(unittest.TestCase):
    def test_windows_warms_exact_default_full_and_saml_graphs_without_feature_union(
        self,
    ):
        expected = []
        for name in ("windows", "windows-full", "windows-saml"):
            lint = validation_commands(name)[0]
            options = lint[2 : lint.index("--")] if "--" in lint else lint[2:]
            expected.append(["cargo", "check", *options])
            if name != "windows-full":
                expected.append(["cargo", "build", *options])
        self.assertEqual(commands("windows", Path("recipe.json")), expected)
        self.assertEqual(
            [c[1] for c in expected], ["check", "build", "check", "check", "build"]
        )

    def test_cooks_match_standalone_and_combined_mssql_feature_graphs(self):
        cooks = commands("mssql", Path("recipe.json"))[1:]
        self.assertEqual(
            cooks[0][6:], ["--locked", "-p", "acton-service-mssql", "--all-targets"]
        )
        check, build = cooks[1:]
        self.assertEqual(check[6:], build[5:])
        self.assertIn("acton-service-integration-tests/mssql", build)
        self.assertIn("--no-default-features", build)
        self.assertIn(
            "acton-service/crypto-aws-lc-rs", build[build.index("--features") + 1]
        )

    def test_warming_cannot_execute_test_binaries_or_services(self):
        for name in CACHE_WRITERS:
            for command in commands(name, Path("recipe.json")):
                self.assertEqual(command[0], "cargo")
                self.assertIn(command[1], {"chef", "check", "build"})
                self.assertNotIn("--release", command)
                if command[1:] != ["chef", "prepare", "--recipe-path", "recipe.json"]:
                    self.assertIn("--locked", command)
            self.assertFalse(
                any("chef" in c for c in commands("windows", Path("recipe.json")))
            )

    def test_local_self_hosted_and_pr_checkouts_are_rejected_before_mutation(self):
        hosted = {
            "GITHUB_ACTIONS": "true",
            "RUNNER_ENVIRONMENT": "github-hosted",
            "GITHUB_REF": "refs/heads/main",
        }
        for env in (
            {},
            {**hosted, "RUNNER_ENVIRONMENT": "self-hosted"},
            {**hosted, "GITHUB_REF": "refs/pull/178/merge"},
        ):
            run = Mock()
            with self.assertRaises(ValueError):
                warm("mssql", env, run)
            run.assert_not_called()

    def test_warmer_removes_workspace_stub_artifacts_before_upload(self):
        metadata = {
            "workspace_members": ["service"],
            "packages": [
                {"id": "service", "name": "acton-service"},
                {"id": "dependency", "name": "serde"},
            ],
        }
        run = Mock(return_value=Mock(stdout=json.dumps(metadata)))
        with tempfile.TemporaryDirectory() as directory:
            warm(
                "windows",
                {
                    "GITHUB_ACTIONS": "true",
                    "RUNNER_ENVIRONMENT": "github-hosted",
                    "GITHUB_REF": "refs/heads/main",
                    "RUNNER_TEMP": directory,
                },
                run,
            )
        self.assertEqual(
            run.call_args_list[-1].args[0], ["cargo", "clean", "-p", "acton-service"]
        )

    def test_redundant_target_editions_are_removed_without_changing_package(self):
        contents = (
            '[package]\nedition = "2021"\n[[bin]]\nname = "server"\nedition = "2021"\n'
        )
        result = strip_target_editions(contents)
        self.assertEqual(result.count("edition"), 1)
        self.assertIn('name = "server"', result)


class CleanupTests(unittest.TestCase):
    def test_all_pages_are_read_before_planning(self):
        request = Mock(
            side_effect=[
                {"actions_caches": [{}] * 100},
                {"actions_caches": [{"last": True}]},
            ]
        )
        self.assertEqual(len(inventory(request)), 101)
        self.assertEqual(
            request.call_args_list[-1].args[0], "actions/caches?per_page=100&page=2"
        )

    def test_only_live_confirmed_closed_pr_ids_can_be_deleted(self):
        entries = [
            {"id": 1, "ref": "refs/pull/178/merge"},
            {"id": 2, "ref": "refs/pull/179/merge"},
            {"id": 3, "ref": "refs/heads/main"},
            {"id": 4, "ref": "refs/tags/v1"},
            {"id": True, "ref": "refs/pull/178/merge"},
            {"id": 5, "ref": None},
        ]
        request = Mock(side_effect=[{"state": "closed"}, {"state": "open"}])
        self.assertEqual(closed_pr_caches(entries, request), [1])
        self.assertEqual(
            [c.args[0] for c in request.call_args_list], ["pulls/178", "pulls/179"]
        )

    def test_unavailable_pr_state_cannot_fall_back_to_payload_state(self):
        with self.assertRaises(RuntimeError):
            closed_pr_caches(
                [{"id": 1, "ref": "refs/pull/178/merge"}],
                Mock(side_effect=RuntimeError("API unavailable")),
            )

    def test_budget_reserves_unrelated_and_open_pr_storage_without_deleting_them(self):
        entries = [
            {
                "id": i,
                "key": key,
                "ref": ref,
                "size_in_bytes": size,
                "last_accessed_at": "2026-10-06",
            }
            for i, key, ref, size in [
                (1, "ci-v2-full--new", "refs/heads/main", 30),
                (2, "ci-v2-full--pr", "refs/pull/178/merge", 50),
                (3, "pnpm", "refs/heads/main", 40),
            ]
        ]
        self.assertEqual(obsolete(entries, budget=100), [1])

    def test_validation_is_restore_only_and_maintenance_cannot_supply_evidence(self):
        validation = (ROOT / ".github/workflows/rust-validation.yml").read_text()
        self.assertNotIn("actions/cache/save", validation)
        maintenance = (ROOT / ".github/workflows/cache-maintenance.yml").read_text()
        self.assertNotIn("development-evidence", maintenance)
        self.assertNotIn("nextest", maintenance)
        self.assertIn(
            "70ef940ef90d04d122f0176fdb8d6c39069191b484a1eaa29b327370c2e1c3c0",
            maintenance,
        )


if __name__ == "__main__":
    unittest.main()
