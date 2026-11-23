#!/usr/bin/env python3
"""Find the cargo-moose revision matching the public plugin runtime."""

import argparse
from pathlib import Path
import re
import tomllib


def locked_revision(lock):
    sources = [package.get("source", "") for package in lock.get("package", [])
               if package["name"] == "moose"]
    if len(sources) != 1:
        raise ValueError("Cargo.lock must select exactly one moose package")
    match = re.fullmatch(
        r"git\+https://github\.com/Matari-Audio/moose(?:\.git)?(?:\?[^#]+)?#([0-9a-f]{40})",
        sources[0],
    )
    if match is None:
        raise ValueError("Cargo.lock has no public Moose Git commit. Refresh the standalone lockfile after the framework mirror.")
    return match.group(1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lockfile", type=Path)
    args = parser.parse_args()
    try:
        with args.lockfile.open("rb") as lockfile:
            print(locked_revision(tomllib.load(lockfile)))
    except (OSError, ValueError) as error:
        parser.error(str(error))


if __name__ == "__main__":
    main()
