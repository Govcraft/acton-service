"""Behavioral regression tests for selection and release/merge safeguards."""

import hashlib
import io
import json
import os
import sys
import tarfile
import unittest
import urllib.error
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from gate import development_expectations, verify_results
from plan import BACKEND_PROFILES, changed_paths, select
from profiles import PROFILES, matrix
from release import main as release_main
from release import (
    publication_order,
    publish_command,
    remaining_packages,
    validate_published_archive,
    validate_tag,
)
from run_profile import commands, validate_matrix, verify_revision


def selected(*paths: str) -> set[str]:
    return {entry["profile"] for entry in select(list(paths))["matrix"]["include"]}


class SelectionTests(unittest.TestCase):
    def test_documentation_can_merge_without_rust_checks(self):
        plan = select(["README.md", "docs/ci.md", "acton-docs/src/app/page.mdoc"])
        self.assertFalse(plan["code"])
        self.assertTrue(plan["docs"])
        self.assertFalse(plan["security"])
        self.assertEqual(plan["matrix"]["include"], [])

    def test_package_and_example_readmes_do_not_recompile_rust(self):
        self.assertFalse(
            select(
                [
                    "acton-service-turso/README.md",
                    "acton-service/examples/grpc/README.md",
                ]
            )["code"]
        )

    def test_oauth_change_exercises_both_state_backends_without_other_databases(self):
        self.assertEqual(
            selected("acton-service/src/auth/oauth/state.rs"),
            {"default", "minimal", "oauth-no-cache", "oauth-with-cache"},
        )

    def test_grpc_change_checks_tls_absent_present_and_examples(self):
        self.assertEqual(
            selected("acton-service/src/grpc/server.rs"),
            {"default", "minimal", "grpc-no-tls", "grpc-tls", "grpc-examples"},
        )

    def test_every_backend_gets_its_unit_tests_and_facade_integration(self):
        for backend, profiles in BACKEND_PROFILES.items():
            with self.subTest(backend=backend):
                self.assertEqual(
                    selected(f"acton-service-{backend}/src/storage.rs"),
                    profiles | {"default", "minimal"},
                )

    def test_unrelated_adapters_are_not_selected_for_turso(self):
        names = selected("acton-service-turso/src/lib.rs")
        self.assertNotIn("surrealdb-adapter", names)
        self.assertNotIn("ring", names)
        self.assertNotIn("windows", names)

    def test_multiple_changes_combine_required_profiles(self):
        self.assertEqual(
            selected(
                "acton-service/src/auth/oauth/state.rs",
                "acton-service-turso/src/lib.rs",
            ),
            {
                "default",
                "minimal",
                "oauth-no-cache",
                "oauth-with-cache",
                "turso-adapter",
                "audit-turso",
            },
        )

    def test_dependency_changes_receive_security_and_exhaustive_checks(self):
        for path in (
            "Cargo.toml",
            "Cargo.lock",
            "acton-service/Cargo.toml",
            "acton-service-postgres/Cargo.toml",
            "deny.toml",
            ".cargo/audit.toml",
        ):
            with self.subTest(path=path):
                plan = select([path])
                self.assertTrue(plan["security"])
                self.assertEqual(
                    {p["profile"] for p in plan["matrix"]["include"]}, PROFILES.keys()
                )

    def test_shared_source_and_contracts_require_all_profiles(self):
        for path in (
            "acton-service/src/service_builder.rs",
            "acton-service/src/state.rs",
            "acton-service/src/config.rs",
            "acton-service/src/lib.rs",
            "acton-service-core/src/error.rs",
            "acton-service-audit/src/storage.rs",
        ):
            with self.subTest(path=path):
                self.assertEqual(selected(path), PROFILES.keys())

    def test_unknown_paths_fail_safe(self):
        for path in (
            "new-subsystem/src/lib.rs",
            "acton-service/src/new.rs",
            "acton-service/src/include.md",
            "acton-service/proto/hello.proto",
        ):
            with self.subTest(path=path):
                self.assertEqual(selected(path), PROFILES.keys())

    def test_build_workflow_changes_test_the_build_workflow(self):
        self.assertEqual(selected(".github/workflows/build.yml"), PROFILES.keys())
        self.assertEqual(selected("scripts/ci/plan.py"), PROFILES.keys())

    def test_private_harness_proto_changes_validate_examples_and_container_tests(self):
        self.assertEqual(
            selected("acton-service-integration-tests/proto/hello.proto"),
            {"default", "minimal", "grpc-examples", "mssql-integration"},
        )

    def test_cli_changes_are_checked(self):
        self.assertEqual(
            selected("acton-cli/src/main.rs"), {"cli", "default", "minimal"}
        )

    def test_manual_full_ci_cannot_skip_docs_or_security(self):
        plan = select([], full=True)
        self.assertTrue(all(plan[name] for name in ("code", "docs", "security")))
        validate_matrix(plan["matrix"], exhaustive=True)

    @patch("plan.subprocess.check_output")
    def test_pull_requests_diff_from_merge_base_and_keep_unusual_filenames(
        self, output
    ):
        base, head, ancestor = "a" * 40, "b" * 40, "c" * 40
        output.side_effect = [ancestor + "\n", b"a\nfile.rs\0deleted.rs\0"]
        self.assertEqual(
            changed_paths("pull_request", base, head, False),
            ["a\nfile.rs", "deleted.rs"],
        )
        self.assertEqual(
            output.call_args_list[0].args[0], ["git", "merge-base", base, head]
        )
        self.assertEqual(output.call_args_list[1].args[0][-2:], [ancestor, head])
        self.assertIn("--no-renames", output.call_args_list[1].args[0])

    def test_unreliable_diffs_are_rejected(self):
        for event, base, head, forced in (
            ("push", "0" * 40, "b" * 40, False),
            ("push", "a" * 40, "b" * 40, True),
            ("workflow_dispatch", "a" * 40, "b" * 40, False),
            ("pull_request", "--output=/tmp/example", "b" * 40, False),
        ):
            with (
                self.subTest(event=event, base=base, forced=forced),
                self.assertRaises(ValueError),
            ):
                changed_paths(event, base, head, forced)


class GateTests(unittest.TestCase):
    def development_results(self, code="true", docs="false", security="false"):
        return {
            "changes": {
                "result": "success",
                "outputs": {"code": code, "docs": docs, "security": security},
            },
            "rust": {"result": "success" if code == "true" else "skipped"},
            "docs-build": {"result": "success" if docs == "true" else "skipped"},
            "security": {"result": "success" if security == "true" else "skipped"},
        }

    def test_intentional_documentation_only_skips_pass(self):
        results = self.development_results(code="false", docs="true")
        verify_results(results, development_expectations(results))

    def test_unexpected_skip_failure_and_cancellation_block_merging(self):
        for outcome in ("skipped", "failure", "cancelled", None):
            results = self.development_results()
            results["rust"]["result"] = outcome
            with self.subTest(outcome=outcome), self.assertRaises(ValueError):
                verify_results(results, development_expectations(results))

    def test_missing_selection_output_cannot_silently_skip_checks(self):
        results = self.development_results()
        del results["changes"]["outputs"]["code"]
        with self.assertRaises(ValueError):
            development_expectations(results)

    def test_selection_failure_blocks_even_if_rust_is_skipped(self):
        results = self.development_results(code="false")
        results["changes"]["result"] = "failure"
        with self.assertRaises(ValueError):
            verify_results(results, development_expectations(results))

    def test_missing_gate_dependency_is_an_error(self):
        with self.assertRaises(ValueError):
            verify_results(
                {"format": {"result": "success"}}, {"format": True, "profiles": True}
            )

    def test_every_release_qualification_dependency_is_required(self):
        names = ("plan", "rust", "docs", "security", "package")
        expected = dict.fromkeys(names, True)
        results = {name: {"result": "success"} for name in names}
        verify_results(results, expected)
        for name in names:
            with self.subTest(name=name), self.assertRaises(ValueError):
                verify_results({**results, name: {"result": "skipped"}}, expected)


class ProfileTests(unittest.TestCase):
    def test_exhaustive_matrix_cannot_omit_a_supported_profile(self):
        with self.assertRaises(ValueError):
            validate_matrix(matrix({"default", "minimal"}), exhaustive=True)

    def test_empty_duplicate_or_injected_matrices_are_rejected(self):
        entry = matrix({"default"})["include"][0]
        for value in (
            {"include": []},
            {"include": [entry, entry]},
            {"include": [{**entry, "runner": "self-hosted"}]},
            {"include": [{**entry, "profile": "unknown"}]},
        ):
            with self.subTest(value=value), self.assertRaises(ValueError):
                validate_matrix(value, exhaustive=False)

    def test_every_cargo_validation_command_uses_the_lockfile(self):
        for name in PROFILES:
            with self.subTest(name=name):
                self.assertTrue(
                    all("--locked" in command for command in commands(name))
                )

    def test_minimal_and_ring_do_not_accidentally_enable_defaults(self):
        for name in ("minimal", "ring", "postgres-ring"):
            with self.subTest(name=name):
                self.assertTrue(
                    all(
                        "--no-default-features" in command for command in commands(name)
                    )
                )

    def test_nextest_is_used_on_windows_and_linux(self):
        for name in ("windows", "default", "minimal", "surrealdb-adapter"):
            with self.subTest(name=name):
                self.assertTrue(
                    any(
                        command[:3] == ["cargo", "nextest", "run"]
                        for command in commands(name)
                    )
                )

    def test_full_qualification_includes_doctests(self):
        self.assertTrue(
            any(
                command[:3] == ["cargo", "test", "--doc"]
                for command in commands("full")
            )
        )

    @patch("run_profile.subprocess.check_output", return_value="a" * 40 + "\n")
    def test_revision_mismatch_and_symbolic_refs_are_rejected(self, _output):
        verify_revision("a" * 40)
        for revision in ("main", "b" * 40, "", "--help"):
            with self.subTest(revision=revision), self.assertRaises(ValueError):
                verify_revision(revision)


def metadata_for(packages: list[tuple[str, list[str], bool]]) -> dict:
    return {
        "workspace_members": [name for name, _, _ in packages],
        "packages": [
            {
                "id": name,
                "name": name,
                "version": "0.47.0",
                "publish": None if public else [],
                "dependencies": [
                    {
                        "name": dep,
                        "path": f"/workspace/{dep}",
                        "req": "^0.47.0",
                        "kind": None,
                    }
                    for dep in dependencies
                ],
            }
            for name, dependencies, public in packages
        ],
    }


class ReleaseTests(unittest.TestCase):
    def archive(self, sha="a" * 40, dirty=False):
        buffer = io.BytesIO()
        data = json.dumps({"git": {"sha1": sha, "dirty": dirty}}).encode()
        with tarfile.open(fileobj=buffer, mode="w:gz") as archive:
            info = tarfile.TarInfo("component-0.47.0/.cargo_vcs_info.json")
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
        return buffer.getvalue()

    def test_adapters_are_published_after_contracts_and_before_facade(self):
        graph = metadata_for(
            [
                (
                    "acton-service",
                    ["acton-service-postgres", "acton-service-turso"],
                    True,
                ),
                ("acton-service-postgres", ["acton-service-audit"], True),
                ("acton-service-turso", ["acton-service-audit"], True),
                ("acton-service-audit", ["acton-service-core"], True),
                ("acton-service-core", [], True),
                ("acton-cli", ["acton-service"], False),
                ("acton-service-integration-tests", ["acton-service"], False),
            ]
        )
        order = publication_order(graph)
        self.assertEqual(order[0], "acton-service-core")
        self.assertEqual(order[1], "acton-service-audit")
        self.assertEqual(order[-1], "acton-service")
        self.assertNotIn("acton-cli", order)
        self.assertNotIn("acton-service-integration-tests", order)

    def test_local_dependency_cycles_or_unpublishable_dependencies_are_rejected(self):
        for graph in (
            metadata_for(
                [
                    ("acton-service", ["adapter"], True),
                    ("adapter", ["acton-service"], True),
                ]
            ),
            metadata_for(
                [("acton-service", ["private"], True), ("private", [], False)]
            ),
        ):
            with self.subTest(graph=graph), self.assertRaises(ValueError):
                publication_order(graph)

    def test_local_dependencies_need_registry_version_requirements(self):
        graph = metadata_for([("acton-service", ["core"], True), ("core", [], True)])
        graph["packages"][0]["dependencies"][0]["req"] = "*"
        with self.assertRaises(ValueError):
            publication_order(graph)

    def test_dry_run_builds_packaged_crates_without_uploading(self):
        command = publish_command(["core", "acton-service"], dry_run=True)
        self.assertIn("--dry-run", command)
        self.assertNotIn("--no-verify", command)
        self.assertIn("--locked", command)
        self.assertEqual(command[-4:], ["-p", "core", "-p", "acton-service"])

    def test_publishing_qualified_packages_avoids_rebuilding_after_authentication(self):
        command = publish_command(["core", "acton-service"], dry_run=False)
        self.assertNotIn("--dry-run", command)
        self.assertIn("--no-verify", command)

    def test_tag_must_match_the_workspace_version(self):
        validate_tag("acton-service-v0.47.0", "0.47.0")
        for tag in (
            "v0.47.0",
            "acton-service-v0.46.0",
            "acton-service-v0.47.0;echo bad",
            "acton-service-v0.47.0\n",
        ):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                validate_tag(tag, "0.47.0")

    def test_dirty_source_cannot_be_published(self):
        with self.assertRaises(ValueError):
            publish_command(["acton-service"], dry_run=False, allow_dirty=True)

    def test_retry_accepts_only_verified_archive_from_same_clean_commit(self):
        archive = self.archive()
        checksum = hashlib.sha256(archive).hexdigest()
        dirty_archive = self.archive(dirty=True)
        validate_published_archive(archive, checksum, "component", "0.47.0", "a" * 40)
        for data, digest, sha in (
            (archive, "0" * 64, "a" * 40),
            (archive, checksum, "b" * 40),
            (dirty_archive, hashlib.sha256(dirty_archive).hexdigest(), "a" * 40),
        ):
            with self.subTest(sha=sha, digest=digest), self.assertRaises(ValueError):
                validate_published_archive(data, digest, "component", "0.47.0", sha)

    def test_already_published_qualified_version_is_skipped_on_retry(self):
        archive = self.archive()
        checksum = hashlib.sha256(archive).hexdigest()

        def fetch(url):
            return (
                archive
                if url.endswith("/download")
                else json.dumps({"version": {"checksum": checksum}}).encode()
            )

        with redirect_stdout(io.StringIO()):
            self.assertEqual(
                remaining_packages(
                    ["component"], {"component": "0.47.0"}, "a" * 40, fetch
                ),
                [],
            )

    def test_registry_outage_is_not_mistaken_for_unpublished_package(self):
        for code in (404, 403, 503):

            def fetch(url, response_code=code):
                raise urllib.error.HTTPError(url, response_code, "fixture", {}, None)

            with self.subTest(code=code):
                if code == 404:
                    self.assertEqual(
                        remaining_packages(
                            ["component"], {"component": "0.47.0"}, "a" * 40, fetch
                        ),
                        ["component"],
                    )
                else:
                    with self.assertRaises(urllib.error.HTTPError):
                        remaining_packages(
                            ["component"], {"component": "0.47.0"}, "a" * 40, fetch
                        )

    def test_bootstrap_token_never_publishes_the_facade(self):
        graph = metadata_for(
            [("acton-service", ["component"], True), ("component", [], True)]
        )
        with (
            patch.object(sys, "argv", ["release.py", "bootstrap"]),
            patch.dict(os.environ, {"EXPECTED_SHA": "a" * 40}, clear=True),
            patch("release.verify_revision"),
            patch("release.subprocess.check_output", return_value=json.dumps(graph)),
            patch(
                "release.remaining_packages",
                return_value=["component", "acton-service"],
            ),
            patch("release.subprocess.run") as run,
            redirect_stdout(io.StringIO()),
        ):
            release_main()
        command = run.call_args.args[0]
        self.assertIn("component", command)
        self.assertNotIn("acton-service", command)

    def test_publishing_requires_an_exact_qualified_revision(self):
        with (
            patch.object(sys, "argv", ["release.py", "publish"]),
            patch.dict(os.environ, {}, clear=True),
            self.assertRaises(ValueError),
        ):
            release_main()


if __name__ == "__main__":
    unittest.main()
