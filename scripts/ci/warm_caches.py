"""Warm only missing shared dependency graphs, independently of validation."""

import argparse
import json
import os
import re
import subprocess
import tomllib
from pathlib import Path

from cache import ROOT, compiler_identity, inventory
from profiles import CACHE_WRITERS, PROFILES, cache_family
from run_profile import commands as validation_commands


def missing_matrix(entries: list[dict], root=ROOT) -> dict:
    existing = {entry["key"] for entry in entries if entry["ref"] == "refs/heads/main"}
    selected = []
    for name in sorted(CACHE_WRITERS):
        profile = PROFILES[name]
        platform = "Windows" if profile.runner == "windows" else "Linux"
        family = cache_family(name)
        key, _ = compiler_identity(family, platform, "X64", root)
        if key not in existing:
            selected.append(
                {
                    "profile": name,
                    "family": family,
                    "key": key,
                    "runner": "windows-latest"
                    if platform == "Windows"
                    else "ubuntu-latest",
                    "protoc": profile.protoc,
                    "kerberos": profile.kerberos,
                }
            )
    return {"include": selected}


def commands(name: str, recipe: Path) -> list[list[str]]:
    if name not in CACHE_WRITERS:
        raise ValueError("Only representative cache graphs may be warmed")
    profile = PROFILES[name]
    windows = profile.runner == "windows"
    result = (
        [] if windows else [["cargo", "chef", "prepare", "--recipe-path", str(recipe)]]
    )
    for command in validation_commands(name):
        if command[1] not in {"clippy", "check"}:
            continue
        options = command[2 : command.index("--")] if "--" in command else command[2:]
        # The standalone adapter and unified facade graph keep separate features.
        if windows:
            result.append(["cargo", "check", *options])
        else:
            result.append(
                [
                    "cargo",
                    "chef",
                    "cook",
                    "--check",
                    "--recipe-path",
                    str(recipe),
                    *options,
                ]
            )
    if profile.tests:
        # Build the unified lint graph for Nextest artifacts without executing tests.
        result.append(
            ["cargo", "build", *options]
            if windows
            else ["cargo", "chef", "cook", "--recipe-path", str(recipe), *options]
        )
    return result


def strip_target_editions(contents: str) -> str:
    edition = tomllib.loads(contents).get("package", {}).get("edition")
    section = ""
    lines = []
    for line in contents.splitlines(keepends=True):
        heading = re.fullmatch(r"\s*\[+(.*?)\]+\s*", line.strip())
        if heading:
            section = heading[1]
        if section in {"lib", "bin", "test", "bench", "example"} and re.match(
            r"\s*edition\s*=", line
        ):
            if tomllib.loads(line)["edition"] == edition:
                continue
        lines.append(line)
    return "".join(lines)


def clean_recipe(recipe: Path) -> None:
    data = json.loads(recipe.read_text())
    for manifest in data["skeleton"]["manifests"]:
        manifest["contents"] = strip_target_editions(manifest["contents"])
    recipe.write_text(json.dumps(data))


def warm(name: str, environment: dict, run=subprocess.run) -> None:
    if not (
        environment.get("GITHUB_ACTIONS") == "true"
        and environment.get("RUNNER_ENVIRONMENT") == "github-hosted"
        and environment.get("GITHUB_REF") == "refs/heads/main"
    ):
        raise ValueError("Warming requires a disposable GitHub-hosted main checkout")
    recipe = Path(environment["RUNNER_TEMP"]) / "dependency-cache-recipe.json"
    # Record real workspace identities before chef replaces manifests with stubs.
    metadata = run(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        check=True,
        capture_output=True,
        text=True,
    )
    packages = json.loads(metadata.stdout)
    workspace = set(packages["workspace_members"])
    names = [p["name"] for p in packages["packages"] if p["id"] in workspace]
    for command in commands(name, recipe):
        print(json.dumps(command), flush=True)
        run(command, check=True)
        if command[1:3] == ["chef", "prepare"]:
            clean_recipe(recipe)
    # Never cache dummy workspace libraries or executables as candidate code.
    run(
        ["cargo", "clean", *[option for name in names for option in ("-p", name)]],
        check=True,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["plan", "warm", "print"])
    parser.add_argument("--profile", choices=sorted(CACHE_WRITERS))
    args = parser.parse_args()
    if args.mode == "plan":
        entries = inventory()
        selected = missing_matrix(entries)
        registry_key = os.environ["REGISTRY_KEY"]
        registry_missing = not any(
            entry["key"] == registry_key and entry["ref"] == "refs/heads/main"
            for entry in entries
        )
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
            stream.write(
                f"matrix={json.dumps(selected)}\nmissing={str(bool(selected['include'])).lower()}\n"
            )
            stream.write(
                f"registry-key={registry_key}\nregistry-missing={str(registry_missing).lower()}\n"
            )
    elif args.profile:
        if args.mode == "print":
            print(json.dumps(commands(args.profile, Path("recipe.json")), indent=2))
        else:
            warm(args.profile, os.environ)
    else:
        parser.error("A profile is required")


if __name__ == "__main__":
    main()
