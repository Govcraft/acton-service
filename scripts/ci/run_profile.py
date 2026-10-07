"""Run catalogued Cargo checks without constructing shell commands."""

import argparse
import json
import os
import re
import subprocess

from profiles import PROFILES


def verify_revision(expected: str) -> None:
    if not re.fullmatch(r"[0-9a-f]{40}", expected):
        raise ValueError("Validation requires an immutable 40-character commit SHA")
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if actual != expected:
        raise ValueError(f"Checked out {actual}, expected {expected}")


def commands(name: str) -> list[list[str]]:
    profile = PROFILES[name]
    options = profile.cargo_options()
    result = [["cargo", "clippy", *options, "--all-targets", "--", "-D", "warnings"]]
    if profile.tests:
        result.append(["cargo", "nextest", "run", "--no-fail-fast", *options])
    if profile.doctests:
        result.append(["cargo", "test", "--doc", *options])
    if name == "windows":
        for other in ("full", "mssql", "windows-auth"):
            result.append(
                ["cargo", "check", *PROFILES[other].cargo_options(), "--all-targets"]
            )
        result.append(
            [
                "cargo",
                "check",
                *PROFILES["grpc-integration"].cargo_options(),
                "--features",
                "cedar-authz",
                "--all-targets",
            ]
        )
    return result


def validate_matrix(value: dict, exhaustive: bool) -> None:
    entries = value.get("include")
    if not isinstance(entries, list) or not entries:
        raise ValueError("Validation matrix must contain at least one profile")
    names = [entry.get("profile") for entry in entries]
    if len(names) != len(set(names)):
        raise ValueError("Validation matrix contains duplicate profiles")
    for entry in entries:
        profile = PROFILES.get(entry.get("profile"))
        if profile is None or entry != {
            "profile": entry["profile"],
            "runner": profile.runner,
            "protoc": profile.protoc,
            "kerberos": profile.kerberos,
        }:
            raise ValueError(f"Unexpected validation matrix entry: {entry}")
    if exhaustive and set(names) != PROFILES.keys():
        raise ValueError("Release qualification must run every supported profile")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile", nargs="?", choices=sorted(PROFILES))
    parser.add_argument("--check-matrix", action="store_true")
    parser.add_argument("--exhaustive", action="store_true")
    parser.add_argument("--print", dest="print_only", action="store_true")
    args = parser.parse_args()
    if os.environ.get("EXPECTED_SHA"):
        verify_revision(os.environ["EXPECTED_SHA"])
    if args.check_matrix:
        validate_matrix(json.loads(os.environ["PROFILE_MATRIX"]), args.exhaustive)
        return
    if not args.profile:
        parser.error("A profile is required unless --check-matrix is selected")
    for command in commands(args.profile):
        print(json.dumps(command), flush=True)
        if not args.print_only:
            subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
