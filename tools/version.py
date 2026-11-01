#!/usr/bin/env python3
"""Derive the v3 nightly version from the checked-out Git commit."""
import argparse
import datetime as dt
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def nightly():
    epoch = int(git('show', '-s', '--format=%ct', 'HEAD'))
    date = dt.datetime.fromtimestamp(epoch, dt.timezone.utc).strftime('%Y%m%d')
    return f"3.0.0-nightly.{date}.{git('rev-list', '--count', 'HEAD')}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['v3-nightly'])
    parser.parse_args()
    print(nightly())


if __name__ == '__main__':
    main()
