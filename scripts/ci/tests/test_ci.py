"""Behavioral regression tests for selection and release/merge safeguards."""

import hashlib
import io
import json
import os
import sys
import tarfile
import tempfile
import tomllib
import unittest
import urllib.error
import zipfile
from contextlib import redirect_stdout
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from cache import obsolete, trim_target
from evidence import compatible, find_evidence
from gate import development_expectations, verify_results
from plan import BACKEND_PROFILES, EXAMPLE_PROFILES, changed_paths, select
from profiles import CACHE_WRITERS, PROFILES, QUALIFICATION, cache_family, matrix
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
            {"oauth-no-cache", "oauth-with-cache"},
        )

    def test_grpc_change_checks_tls_absent_present_and_examples(self):
        self.assertEqual(
            selected("acton-service/src/grpc/server.rs"),
            {
                "windows-grpc",
                "grpc-no-tls",
                "grpc-tls",
                "grpc-examples",
                "grpc-integration",
                "grpc-integration-ring",
            },
        )

    def test_every_backend_gets_its_unit_tests_and_facade_integration(self):
        for backend, profiles in BACKEND_PROFILES.items():
            with self.subTest(backend=backend):
                self.assertEqual(
                    selected(f"acton-service-{backend}/src/storage.rs"),
                    profiles,
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
                "oauth-no-cache",
                "oauth-with-cache",
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
                    {p["profile"] for p in plan["matrix"]["include"]},
                    QUALIFICATION | {"cli"},
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
                self.assertEqual(selected(path), QUALIFICATION | {"cli"})

    def test_unknown_paths_fail_safe(self):
        for path in (
            "new-subsystem/src/lib.rs",
            "acton-service/src/new.rs",
            "acton-service/src/include.md",
            "acton-service/proto/hello.proto",
        ):
            with self.subTest(path=path):
                self.assertEqual(selected(path), QUALIFICATION | {"cli"})

    def test_build_workflow_changes_test_the_build_workflow(self):
        self.assertEqual(
            selected(".github/workflows/build.yml"), QUALIFICATION | {"cli"}
        )
        self.assertEqual(selected("scripts/ci/plan.py"), QUALIFICATION | {"cli"})

    def test_private_harness_proto_changes_validate_transport_without_databases(self):
        self.assertEqual(
            selected("acton-service-integration-tests/proto/hello.proto"),
            {
                "windows-grpc",
                "grpc-examples",
                "grpc-integration",
                "grpc-integration-ring",
            },
        )

    def test_cli_changes_are_checked(self):
        self.assertEqual(selected("acton-cli/src/main.rs"), {"cli"})

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

    def test_backend_fixture_changes_select_only_their_live_scenarios(self):
        self.assertEqual(
            selected("acton-service-integration-tests/tests/mssql_integration.rs"),
            {"mssql-integration"},
        )
        self.assertEqual(
            selected("acton-service-integration-tests/tests/postgres_integration.rs"),
            {"postgres-integration", "postgres-integration-ring"},
        )

    def test_workflow_only_docs_change_still_validates_workflow_and_builds_site(self):
        plan = select([".github/workflows/deploy-docs.yml"])
        self.assertTrue(plan["tooling"])
        self.assertTrue(plan["docs"])
        self.assertFalse(plan["code"])

    def test_qualification_excludes_deprecated_cli_but_development_checks_it(self):
        names = {
            entry["profile"] for entry in select([], full=True)["matrix"]["include"]
        }
        self.assertEqual(names, QUALIFICATION)
        self.assertNotIn("cli", names)
        self.assertIn("cli", selected("acton-service/src/lib.rs"))

    def test_every_declared_framework_example_has_executable_feature_coverage(self):
        root = Path(__file__).resolve().parents[3]
        manifest = tomllib.loads((root / "acton-service/Cargo.toml").read_text())

        def features(profile):
            enabled = set(profile.features.split(",")) - {""}
            if profile.defaults:
                enabled.add("default")
            pending = list(enabled)
            while pending:
                for feature in manifest["features"].get(pending.pop(), []):
                    if feature not in enabled:
                        enabled.add(feature)
                        pending.append(feature)
            return enabled

        for target in manifest["example"]:
            with self.subTest(example=target["name"]):
                self.assertIn(target["path"], EXAMPLE_PROFILES)
                names = selected("acton-service/" + target["path"])
                self.assertTrue(
                    any(
                        set(target.get("required-features", []))
                        <= features(PROFILES[name])
                        for name in names
                    )
                )
                self.assertNotEqual(names, set(PROFILES))


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
        for name in ("windows", "default", "minimal", "audit-surrealdb"):
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

    def test_ring_runtime_uses_narrow_binaries_and_broad_compile_stays_isolated(self):
        self.assertFalse(any("nextest" in command for command in commands("ring")))
        self.assertTrue(any("nextest" in command for command in commands("tls-ring")))
        self.assertIn("--no-default-features", commands("ring")[0])

    def test_windows_does_not_serially_compile_unrelated_configurations(self):
        self.assertEqual(len(commands("windows")), 2)
        self.assertTrue(PROFILES["windows-saml"].tests)
        self.assertIn("test(signed) | test(signature)", commands("windows-saml")[1])

    def test_combined_adapter_job_retains_standalone_and_adapter_runtime_evidence(self):
        checks = commands("audit-surrealdb")
        self.assertIn("acton-service-surrealdb", checks[0])
        runtime = next(command for command in checks if "nextest" in command)
        self.assertIn("acton-service", runtime)
        self.assertIn("acton-service-surrealdb", runtime)
        self.assertIn("acton-service-integration-tests", runtime)
        self.assertIn("acton-service-integration-tests/surrealdb", runtime)
        self.assertIn(
            "acton-service/surrealdb", runtime[runtime.index("--features") + 1]
        )

    def test_qualification_live_backend_coverage_is_combined_without_duplicates(self):
        for feature, name in (
            ("mssql", "mssql"),
            ("surrealdb", "audit-surrealdb"),
            ("clickhouse", "audit-clickhouse"),
        ):
            self.assertIn(name, QUALIFICATION)
            self.assertEqual(PROFILES[name].harness, feature)
            self.assertNotIn(feature + "-integration", QUALIFICATION)
            self.assertTrue(
                any(
                    "nextest" in command
                    and "acton-service-integration-tests/" + feature in command
                    for command in commands(name)
                )
            )


class CacheTests(unittest.TestCase):
    def entry(
        self, identifier, key, size, accessed="2026-01-01", ref="refs/heads/main"
    ):
        return {
            "id": identifier,
            "key": key,
            "size_in_bytes": size,
            "last_accessed_at": accessed,
            "ref": ref,
        }

    def test_only_six_distinct_families_have_writers(self):
        self.assertEqual(len(CACHE_WRITERS), 6)
        self.assertEqual(len({cache_family(name) for name in CACHE_WRITERS}), 6)

    def test_retention_removes_superseded_cache_and_enforces_budget(self):
        entries = [
            self.entry(1, "ci-v2-full--new", 60, "2026-01-03"),
            self.entry(2, "ci-v2-full--old", 60, "2026-01-02"),
            self.entry(3, "ci-v2-default--new", 60),
            self.entry(4, "pnpm-unrelated", 1000),
            self.entry(5, "v0-rust-profile", 1000),
        ]
        self.assertEqual(set(obsolete(entries, budget=100)), {2, 3, 5})

    def test_trim_keeps_library_fingerprints_but_removes_test_executables(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for relative in (
                "debug/deps/libservice.rlib",
                "debug/deps/test-binary",
                "debug/.fingerprint/contract",
                "debug/incremental/unused",
                "doc/index.html",
            ):
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("artifact")
            self.assertTrue(trim_target(root))
            self.assertTrue((root / "debug/deps/libservice.rlib").exists())
            self.assertTrue((root / "debug/.fingerprint/contract").exists())
            self.assertFalse((root / "debug/deps/test-binary").exists())
            self.assertFalse((root / "doc").exists())


class EvidenceTests(unittest.TestCase):
    def record(self):
        return {
            "version": 1,
            "repository": "Govcraft/acton-service",
            "tree": "a" * 40,
            "sha": "b" * 40,
            "profiles": ["oauth-no-cache", "oauth-with-cache"],
            "docs": False,
            "security": False,
            "tooling": False,
        }

    def test_identical_tree_with_complete_coverage_can_be_reused(self):
        self.assertTrue(
            compatible(
                self.record(),
                select(["acton-service/src/auth/oauth/state.rs"]),
                "a" * 40,
                "Govcraft/acton-service",
            )
        )

    def test_different_tree_missing_coverage_or_untrusted_record_cannot_be_reused(self):
        plan = select(["acton-service/src/auth/oauth/state.rs"])
        for change in (
            {"tree": "c" * 40},
            {"profiles": ["oauth-no-cache"]},
            {"profiles": None},
            {"version": 0},
            {"repository": "fork/repo"},
            {"sha": "main"},
            {"sha": 123},
        ):
            with self.subTest(change=change):
                self.assertFalse(
                    compatible(
                        {**self.record(), **change},
                        plan,
                        "a" * 40,
                        "Govcraft/acton-service",
                    )
                )
        for key in ("docs", "security", "tooling"):
            self.assertFalse(
                compatible(
                    self.record(),
                    {**plan, key: True},
                    "a" * 40,
                    "Govcraft/acton-service",
                )
            )

    @patch.dict(os.environ, {"GITHUB_REPOSITORY": "Govcraft/acton-service"})
    @patch("evidence.request")
    def test_reuse_requires_completed_trusted_pr_and_api_tree_agreement(self, request):
        run = {
            "event": "pull_request",
            "status": "completed",
            "conclusion": "success",
            "head_repository": {"full_name": "Govcraft/acton-service"},
            "head_sha": "b" * 40,
            "id": 123,
        }
        request.side_effect = [
            {
                "workflow_runs": [
                    {**run, "event": "push"},
                    {**run, "head_repository": {"full_name": "fork/repo"}},
                    run,
                ]
            },
            {"tree": {"sha": "c" * 40}},
        ]
        self.assertEqual(
            find_evidence(select(["acton-service/src/auth/oauth/state.rs"]), "a" * 40),
            (None, ""),
        )
        self.assertEqual(request.call_count, 2)

    @patch.dict(os.environ, {"GITHUB_REPOSITORY": "Govcraft/acton-service"})
    @patch("evidence.request")
    def test_successful_same_tree_run_artifact_is_reused(self, request):
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("evidence.json", json.dumps(self.record()))
        request.side_effect = [
            {
                "workflow_runs": [
                    {
                        "event": "pull_request",
                        "status": "completed",
                        "conclusion": "success",
                        "head_repository": {"full_name": "Govcraft/acton-service"},
                        "head_sha": "b" * 40,
                        "id": 123,
                    }
                ]
            },
            {"tree": {"sha": "a" * 40}},
            {
                "artifacts": [
                    {"name": "development-evidence", "expired": False, "id": 456}
                ]
            },
            buffer.getvalue(),
        ]
        record, run_id = find_evidence(
            select(["acton-service/src/auth/oauth/state.rs"]), "a" * 40
        )
        self.assertEqual(record, self.record())
        self.assertEqual(run_id, "123")


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
