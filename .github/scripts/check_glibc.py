#!/usr/bin/env python3
"""Reject Linux release artifacts requiring a newer glibc than the baseline."""
import argparse
import re
import subprocess


def required_version(output):
    if "GLIBC_PRIVATE" in output:
        raise ValueError("artifact depends on GLIBC_PRIVATE")
    return max((tuple(map(int, match.split(".")))
                for match in re.findall(r"GLIBC_([0-9]+(?:\.[0-9]+)+)", output)),
               default=(0, 0))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", nargs="+")
    args = parser.parse_args()
    for path in args.artifacts:
        output = subprocess.check_output(["readelf", "--version-info", path], text=True)
        required = required_version(output)
        if required > (2, 35):
            parser.error(f"{path}: requires GLIBC_{'.'.join(map(str, required))}; maximum is 2.35")
        print(f"{path}: maximum GLIBC_{'.'.join(map(str, required))} <= 2.35")


if __name__ == "__main__":
    main()
