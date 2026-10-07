"""Reuse completed trusted development validation only for an identical source tree."""

import argparse
import io
import json
import os
import re
import subprocess
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

ARTIFACT = "development-evidence"


def compatible(record: dict, plan: dict, tree: str, repository: str) -> bool:
    required = {entry["profile"] for entry in plan["matrix"]["include"]}
    return (
        record.get("version") == 1
        and record.get("repository") == repository
        and record.get("tree") == tree
        and re.fullmatch(r"[0-9a-f]{40}", record.get("sha", "")) is not None
        and isinstance(record.get("profiles"), list)
        and all(isinstance(name, str) for name in record["profiles"])
        and required <= set(record["profiles"])
        and all(
            not plan[key] or record.get(key) is True
            for key in ("docs", "security", "tooling")
        )
    )


class ArtifactRedirect(urllib.request.HTTPRedirectHandler):
    """Signed artifact URLs need no GitHub credential on their storage host."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        redirected = super().redirect_request(req, fp, code, msg, headers, newurl)
        if (
            redirected
            and urllib.parse.urlsplit(req.full_url).netloc
            != urllib.parse.urlsplit(newurl).netloc
        ):
            redirected.remove_header("Authorization")
        return redirected


def request(path: str, raw: bool = False):
    # Paths originate in fixed endpoint templates, never in downloaded evidence.
    url = (
        f"{os.environ['GITHUB_API_URL']}/repos/{os.environ['GITHUB_REPOSITORY']}/{path}"
    )
    req = urllib.request.Request(
        url,
        headers={
            "Authorization": f"Bearer {os.environ['GH_TOKEN']}",
            "Accept": "application/vnd.github+json",
        },
    )
    with urllib.request.build_opener(ArtifactRedirect()).open(
        req, timeout=20
    ) as response:
        data = response.read(1024**2 + 1)
        if len(data) > 1024**2:
            raise ValueError("Oversized evidence response")
        return data if raw else json.loads(data)


def find_evidence(plan: dict, tree: str) -> tuple[dict | None, str]:
    repository = os.environ["GITHUB_REPOSITORY"]
    runs = request("actions/workflows/build.yml/runs?status=success&per_page=50")[
        "workflow_runs"
    ]
    for run in runs:
        if (
            run["event"] != "pull_request"
            or run["head_repository"]["full_name"] != repository
        ):
            continue
        # The trusted API commit tree must agree with artifact evidence.
        commit = request(f"git/commits/{run['head_sha']}")
        if commit["tree"]["sha"] != tree:
            continue
        artifacts = request(f"actions/runs/{run['id']}/artifacts")["artifacts"]
        artifact = next(
            (a for a in artifacts if a["name"] == ARTIFACT and not a["expired"]), None
        )
        if artifact is None:
            continue
        with zipfile.ZipFile(
            io.BytesIO(request(f"actions/artifacts/{artifact['id']}/zip", raw=True))
        ) as archive:
            if archive.getinfo("evidence.json").file_size > 65536:
                raise ValueError("Oversized evidence document")
            record = json.loads(archive.read("evidence.json"))
        if compatible(record, plan, tree, repository):
            # Docs reuse additionally requires the validated site to remain available.
            if plan["docs"] and not any(
                a["name"] == "documentation-site" and not a["expired"]
                for a in artifacts
            ):
                continue
            return record, str(run["id"])
    return None, ""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["select", "record"])
    args = parser.parse_args()
    plan = json.loads(Path("ci-plan.json").read_text())
    tree = subprocess.check_output(
        ["git", "rev-parse", "HEAD^{tree}"], text=True
    ).strip()
    if args.command == "record":
        record = {
            "version": 1,
            "repository": os.environ["GITHUB_REPOSITORY"],
            "tree": tree,
            "sha": subprocess.check_output(
                ["git", "rev-parse", "HEAD"], text=True
            ).strip(),
            "profiles": [e["profile"] for e in plan["matrix"]["include"]],
            **{key: plan[key] for key in ("docs", "security", "tooling")},
        }
        Path("evidence.json").write_text(json.dumps(record, indent=2) + "\n")
        return
    record, run_id = None, ""
    if (
        os.environ.get("GITHUB_EVENT_NAME") == "push"
        and os.environ.get("GITHUB_REF") == "refs/heads/main"
        and os.environ.get("PUSH_FORCED") != "true"
    ):
        try:
            record, run_id = find_evidence(plan, tree)
        except (
            urllib.error.URLError,
            ValueError,
            KeyError,
            TypeError,
            zipfile.BadZipFile,
        ) as error:
            print(f"No reusable evidence ({error}); validating the merge.")
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as stream:
        for name in ("matrix", "code", "docs", "security", "tooling"):
            value = plan[name] if name == "matrix" or record is None else False
            stream.write(f"{name}={json.dumps(value, separators=(',', ':'))}\n")
        stream.write(
            f"deploy_docs={str(plan['docs']).lower()}\nevidence_run={run_id}\n"
        )
    if record:
        print(f"Identical tree {tree} already validated by successful PR run {run_id}.")
    print(json.dumps(plan, indent=2))


if __name__ == "__main__":
    main()
