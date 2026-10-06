"""Reject failures, cancellations, and unexpected skips in required checks."""

import argparse
import json
import os


def verify_results(results: dict, expected: dict[str, bool]) -> None:
    if results.keys() != expected.keys():
        raise ValueError(
            "Unexpected gate dependencies: "
            f"{sorted(results)}; expected {sorted(expected)}"
        )
    for name, required in expected.items():
        result = results[name].get("result")
        wanted = "success" if required else "skipped"
        if result != wanted:
            raise ValueError(f"{name}: expected {wanted}, received {result}")


def development_expectations(results: dict) -> dict[str, bool]:
    outputs = results.get("changes", {}).get("outputs", {})
    for key in ("code", "docs", "security"):
        if outputs.get(key) not in {"true", "false"}:
            raise ValueError(f"Invalid or missing change-selection output: {key}")
    return {
        "changes": True,
        "rust": outputs["code"] == "true",
        "docs-build": outputs["docs"] == "true",
        "security": outputs["security"] == "true",
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=["development", "rust", "qualification"])
    args = parser.parse_args()
    results = json.loads(os.environ["RESULTS"])
    expected = {
        "development": development_expectations,
        "rust": lambda _: {"format": True, "profiles": True},
        "qualification": lambda _: {
            "plan": True,
            "rust": True,
            "docs": True,
            "security": True,
            "package": True,
        },
    }[args.kind](results)
    verify_results(results, expected)
    print("Every selected validation job succeeded; all skips were intentional.")


if __name__ == "__main__":
    main()
