"""Dependency isolation and semantic release-bump selection regressions."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from graph import DRIVERS, verify_graph
from plan import select
from versions import satisfies, version_only


def fixture(version="0.4.0", requirement=None, external="1.0.0"):
    requirement = requirement or version
    return {
        "Cargo.toml": f'''[workspace]
members = ["consumer", "core"]
[workspace.package]
version = "{version}"
[workspace.dependencies]
serde = "1"
''',
        "consumer/Cargo.toml": f'''[package]
name = "consumer"
version.workspace = true
[dependencies]
[dependencies.project-core]
path = "../core"
version = "{requirement}"
default-features = false
''',
        "core/Cargo.toml": (
            '[package]\nname = "project-core"\nversion.workspace = true\n'
        ),
        "Cargo.lock": f'''version = 4
[[package]]
name = "consumer"
version = "{version}"
dependencies = ["project-core {version}", "serde"]
[[package]]
name = "project-core"
version = "{version}"
[[package]]
name = "serde"
version = "{external}"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "unchanged"
''',
    }


def only_versions(before, after):
    paths = [
        name
        for name in before.keys() | after.keys()
        if before.get(name) != after.get(name)
    ]
    return version_only(paths, before, after)


class VersionBoundaryTests(unittest.TestCase):
    def test_inherited_versions_and_updated_local_references_use_metadata_path(self):
        self.assertTrue(only_versions(fixture(), fixture("0.5.0")))
        plan = select(["Cargo.toml", "Cargo.lock"], versions_only=True)
        self.assertTrue(plan["metadata"])
        self.assertTrue(plan["docs"])
        self.assertFalse(plan["code"])
        self.assertFalse(plan["security"])
        self.assertEqual(plan["matrix"]["include"], [])

    def test_patch_bump_can_preserve_compatible_caret_reference(self):
        self.assertTrue(only_versions(fixture(), fixture("0.4.1", "0.4.0")))

    def test_external_changes_cannot_use_fast_path(self):
        before = fixture()
        after = fixture("0.5.0", external="1.1.0")
        self.assertFalse(only_versions(before, after))
        for old, new in [
            ('serde = "1"', 'serde = "2"'),
            ('checksum = "unchanged"', 'checksum = "changed"'),
        ]:
            after = fixture("0.5.0")
            path = "Cargo.toml" if "serde =" in old else "Cargo.lock"
            after[path] = after[path].replace(old, new)
            self.assertFalse(only_versions(before, after))

    def test_feature_dependency_flags_and_root_configuration_cannot_be_hidden(self):
        before = fixture()
        for path, addition in (
            ("consumer/Cargo.toml", "\n[features]\nnew-feature = []\n"),
            ("Cargo.toml", "\n[profile.dev]\nopt-level = 2\n"),
        ):
            after = fixture("0.5.0")
            after[path] += addition
            self.assertFalse(only_versions(before, after))
        after = fixture("0.5.0")
        after["consumer/Cargo.toml"] = after["consumer/Cargo.toml"].replace(
            "default-features = false", "default-features = true"
        )
        self.assertFalse(only_versions(before, after))

    def test_invalid_requirements_stale_lock_and_prereleases_fail_broad(self):
        before = fixture()
        for requirement in ("0.4.0", "garbage"):
            self.assertFalse(only_versions(before, fixture("0.5.0", requirement)))
        after = fixture("0.5.0")
        after["Cargo.lock"] = before["Cargo.lock"]
        self.assertFalse(only_versions(before, after))
        self.assertFalse(only_versions(before, fixture("0.5.0-beta.1")))
        after = fixture("0.5.0")
        after["consumer/Cargo.toml"] = after["consumer/Cargo.toml"].replace(
            'version = "0.5.0"', ""
        )
        self.assertFalse(only_versions(before, after))

    def test_deleted_or_added_packages_and_source_edits_cannot_be_hidden(self):
        before, after = fixture(), fixture("0.5.0")
        del after["core/Cargo.toml"]
        self.assertFalse(only_versions(before, after))
        after = fixture("0.5.0")
        after["consumer/src/lib.rs"] = "changed source"
        self.assertFalse(only_versions(before, after))

    def test_ambiguous_registry_name_does_not_normalize_wrong_dependency(self):
        before, after = fixture(), fixture("0.5.0")
        collision = """
[[package]]
name = "project-core"
version = "0.4.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "unchanged"
"""
        before["Cargo.lock"] += collision
        after["Cargo.lock"] += collision
        self.assertFalse(only_versions(before, after))

    def test_supported_stable_constraints_keep_cargo_caret_semantics(self):
        for version, requirement, expected in (
            ("0.4.1", "0.4.0", True),
            ("0.5.0", "^0.4.0", False),
            ("1.2.0", "1.0.0", True),
            ("0.0.4", "0.0.3", False),
            ("0.4.1", "=0.4.0", False),
            ("0.4.1", "~0.4.0", True),
        ):
            self.assertEqual(satisfies(version, requirement), expected)


class GraphBoundaryTests(unittest.TestCase):
    def graph(self, backend, ring=False):
        lines = [
            "acton-service v0.47.0 (/workspace/facade)|http",
            "[dev-dependencies]",
            "serde v1.0.0|derive (*)",
        ]
        if backend:
            lines.append(DRIVERS[backend] + " v1.0.0|default")
        if backend == "postgres":
            provider = "_tls-rustls-ring-webpki" if ring else "_tls-rustls-aws-lc-rs"
            lines.append("sqlx-core v0.8.0|" + provider)
        return "\n".join(lines)

    def test_each_backend_and_no_database_graph_are_independent(self):
        for backend in (None, *DRIVERS):
            verify_graph(self.graph(backend), backend)
        verify_graph(self.graph("postgres", ring=True), "postgres", ring=True)

    def test_another_driver_or_missing_selected_driver_cannot_pass(self):
        for selected in DRIVERS:
            for unexpected in DRIVERS.keys() - {selected}:
                with (
                    self.subTest(selected=selected, unexpected=unexpected),
                    self.assertRaises(ValueError),
                ):
                    verify_graph(
                        self.graph(selected) + "\n" + DRIVERS[unexpected] + " v1.0.0|",
                        selected,
                    )
            with self.assertRaises(ValueError):
                verify_graph(self.graph(None), selected)

    def test_sqlx_must_compile_exactly_the_requested_provider(self):
        with self.assertRaises(ValueError):
            verify_graph(self.graph("postgres"), "postgres", ring=True)
        with self.assertRaises(ValueError):
            verify_graph(
                self.graph("postgres")
                + "\nsqlx-core v0.8.0|_tls-rustls-ring-webpki (*)",
                "postgres",
            )

    def test_empty_and_invalid_graphs_fail_closed(self):
        for text in ("", "cargo tree failed", "[dev-dependencies]"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                verify_graph(text, None)

    def test_unused_sqlx_drivers_cannot_leak_into_any_graph(self):
        for driver in ("sqlx-mysql", "sqlx-sqlite"):
            with self.assertRaises(ValueError):
                verify_graph(self.graph("postgres") + f"\n{driver} v0.8.0|", "postgres")

    def test_remote_surrealdb_rejects_embedded_engine_and_default_parser(self):
        remote = self.graph("surrealdb")
        for package in ("surrealdb-core", "surrealdb-engine-local", "surrealdb-kvs"):
            with self.subTest(package=package), self.assertRaises(ValueError):
                verify_graph(remote + f"\n{package} v3.3.0|", "surrealdb")
        with self.assertRaises(ValueError):
            verify_graph(remote + "\nsurrealdb v3.3.0|parse", "surrealdb")
