"""Share Cargo downloads across operating systems without archiving home paths."""

import argparse
import hashlib
import os
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SNAPSHOT = ".ci-cargo-cache"
PREFIX = "ci-v2-registry--portable-v1-"
COMPONENTS = ("registry/cache", "registry/index", "git/db")


def identity(root=ROOT):
    lockfile = (root / "Cargo.lock").read_bytes().replace(b"\r\n", b"\n")
    return PREFIX + hashlib.sha256(lockfile).hexdigest()


def cargo_home(environment):
    return Path(environment.get("CARGO_HOME") or Path.home() / ".cargo")


def copy_downloads(source: Path, destination: Path):
    """Copy only archives, indexes and Git databases, preserving compiler paths."""
    for component in COMPONENTS:
        directory = source / component
        if directory.is_dir():
            shutil.copytree(directory, destination / component, dirs_exist_ok=True)


def stage(home: Path, root=ROOT):
    snapshot = root / SNAPSHOT
    shutil.rmtree(snapshot, ignore_errors=True)
    copy_downloads(home, snapshot)


def hydrate(home: Path, root=ROOT):
    copy_downloads(root / SNAPSHOT, home)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["identity", "stage", "hydrate"])
    args = parser.parse_args()
    if args.mode == "identity":
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"key={identity()}\nrestore-prefix={PREFIX}\n")
    elif args.mode == "stage":
        stage(cargo_home(os.environ))
    else:
        hydrate(cargo_home(os.environ))


if __name__ == "__main__":
    main()
