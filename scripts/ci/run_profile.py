"""Run catalogued Cargo checks without constructing shell commands."""

import argparse
import json
import os
import re
import subprocess

from profiles import PROFILES, QUALIFICATION, matrix_entry


def verify_revision(expected: str) -> None:
    if not re.fullmatch(r"[0-9a-f]{40}", expected):
        raise ValueError("Validation requires an immutable 40-character commit SHA")
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    if actual != expected:
        raise ValueError(f"Checked out {actual}, expected {expected}")


def commands(name: str) -> list[list[str]]:
    profile = PROFILES[name]
    options = profile.cargo_options()
    if profile.compile_only:
        return [["cargo", "check", *options, "--all-targets"]]
    result = []
    if profile.companion:
        standalone = ["--locked", "-p", profile.companion]
        result.append(
            ["cargo", "clippy", *standalone, "--all-targets", "--", "-D", "warnings"]
        )
        # Qualify features by package when selecting multiple workspace members.
        for i in range(len(options)):
            if i and options[i - 1] == "--features":
                options[i] = ",".join(
                    f"{profile.package}/{feature}"
                    for feature in profile.features.split(",")
                )
        options.extend(["-p", profile.companion])
    if profile.harness:
        options.extend(
            [
                "-p",
                "acton-service-integration-tests",
                "--features",
                f"acton-service-integration-tests/{profile.harness}",
            ]
        )
    result.append(
        ["cargo", "clippy", *options, "--all-targets", "--", "-D", "warnings"]
    )
    if profile.tests:
        test_options = ["--no-fail-fast"]
        if name == "windows-saml":
            test_options.extend(["-E", "test(signed) | test(signature)"])
        if name == "tls-ring":
            test_options.extend(
                [
                    "-E",
                    "test(tls) | test(crypto) | "
                    "binary(~tls) | binary(~metrics_exporter)",
                ]
            )
        result.append(["cargo", "nextest", "run", *test_options, *options])
    if profile.doctests:
        result.append(["cargo", "test", "--doc", *options])
    elif profile.companion:
        result.append(["cargo", "test", "--doc", "--locked", "-p", profile.companion])
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
        if profile is None or entry != matrix_entry(entry["profile"]):
            raise ValueError(f"Unexpected validation matrix entry: {entry}")
    if exhaustive and set(names) != QUALIFICATION:
        raise ValueError(
            "Release qualification must run every supported service profile"
        )


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
