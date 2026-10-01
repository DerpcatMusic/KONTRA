#!/usr/bin/env python3
"""Conservative code-change classifier shared by CI and scheduled snapshots."""
import os
from pathlib import Path
import subprocess


def build_relevant(path):
    # This file is copied into downloadable packages, so it must refresh them.
    if path == "THIRD_PARTY.md":
        return True
    # Unknown/new paths are code by default. Do not maintain a fragile source
    # allowlist that would miss a newly introduced build input.
    return not (path.endswith(".md") or path.startswith(("docs/", "audits/")))


def changed_code(before, after, force=False, cwd=None):
    if force or not before or set(before) == {"0"}:
        return True
    try:
        result = subprocess.run(
            ["git", "diff", "--name-only", "--no-renames", "-z", before, after, "--"],
            cwd=cwd, check=True, capture_output=True,
        )
    except subprocess.CalledProcessError:
        # First snapshot / missing history must run verification, never skip it.
        return True
    return any(build_relevant(os.fsdecode(path)) for path in result.stdout.split(b"\0") if path)


def main():
    relevant = changed_code(os.environ.get("BEFORE", ""), os.environ["AFTER"],
                            os.environ.get("FORCE") == "true")
    output = f"code={str(relevant).lower()}\n"
    print(output, end="")
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as stream:
        stream.write(output)


if __name__ == "__main__":
    main()
