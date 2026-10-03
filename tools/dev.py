#!/usr/bin/env python3
"""Incremental checks and explicitly named tests; shipping validation stays separate."""

import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent


def arguments(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--toolchain", default="1.98.1")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--profile", choices=["dev", "dsp-dev"], default="dev")
    parser.add_argument("--print", action="store_true", help="print the plan without running Cargo")
    sub = parser.add_subparsers(dest="action", required=True)
    check = sub.add_parser("check", help="type-check the affected target without linking")
    check.add_argument("--target", choices=["lib", "cli", "standalone", "all"], default="lib")
    check.add_argument("--core", action="store_true", help="lib only: omit plugin/UI features")
    test = sub.add_parser("test", help="build once, verify names, run only those exact tests")
    test.add_argument("--target", choices=["lib", "playback"], default="lib")
    test.add_argument("--ignored", action="store_true", help="explicitly run named ignored tests")
    test.add_argument("names", nargs="+")
    args = parser.parse_args(argv)
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    if args.action == "check" and args.core and args.target != "lib":
        parser.error("--core is limited to --target lib")
    if args.action == "test" and any(not name or name.startswith("-") for name in args.names):
        parser.error("test names must be nonempty exact names")
    return args


def command(args):
    cmd = ["cargo", "+" + args.toolchain, args.action, "--locked", "-p", "kontakto",
           "--profile", args.profile, "-j", str(args.jobs)]
    if args.action == "check":
        targets = {"lib": ["--lib"], "cli": ["--bin", "kontakto"],
                   "standalone": ["--bin", "kontakto-standalone", "--features", "standalone"],
                   "all": ["--all-targets", "--all-features"]}
        cmd += targets[args.target]
        if args.core:
            cmd += ["--no-default-features", "--features", "library-access"]
    else:
        cmd += ["--lib"] if args.target == "lib" else ["--test", "playback"]
        cmd += ["--no-run", "--message-format=json"]
    return cmd


def environment(environ):
    env = dict(environ)
    # Do not inherit the release/probe target: its dependency identities may
    # currently be frozen. This directory persists across local edit cycles.
    target = Path(env.get("KONTRA_DEV_TARGET_DIR", ROOT / "artifacts" / "dev-target"))
    env["CARGO_TARGET_DIR"] = str(target.resolve())
    env["CARGO_INCREMENTAL"] = "1"
    return env


def test_binary(output, target):
    name = "kontakto" if target == "lib" else "playback"
    paths = set()
    for line in output.splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if (row.get("reason") == "compiler-artifact" and row["target"]["name"] == name
                and row["profile"]["test"] and row.get("executable")):
            paths.add(row["executable"])
    if len(paths) != 1:
        raise ValueError(f"expected one Cargo-produced {name} test executable, found {len(paths)}")
    return next(iter(paths))


def listed_tests(output):
    return {line.removesuffix(": test") for line in output.splitlines() if line.endswith(": test")}


def validate_names(names, available, ignored, run_ignored):
    for name in names:
        if name not in available:
            raise ValueError(f"exact test not found: {name}")
        if (name in ignored) != run_ignored:
            raise ValueError(f"{name}: choose {'--ignored' if name in ignored else 'without --ignored'}")


def main(argv=None):
    args = arguments(argv)
    env = environment(os.environ)
    cmd = command(args)
    print(f"Development {args.profile} only; release/native/full-suite gates remain required.", flush=True)
    print("CARGO_TARGET_DIR=" + env["CARGO_TARGET_DIR"], flush=True)
    print(shlex.join(cmd), flush=True)
    if args.print:
        if args.action == "test":
            print("Verify exact names against the Cargo-produced executable before running:")
            for name in args.names:
                print("  " + name)
        return 0
    if args.action == "check":
        return subprocess.run(cmd, cwd=ROOT, env=env).returncode
    built = subprocess.run(cmd, cwd=ROOT, env=env, stdout=subprocess.PIPE, text=True)
    for line in built.stdout.splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if row.get("reason") == "compiler-message" and row.get("message", {}).get("rendered"):
            print(row["message"]["rendered"], end="", file=sys.stderr)
    if built.returncode:
        return built.returncode
    binary = test_binary(built.stdout, args.target)
    print("Cargo verified test executable: " + binary, flush=True)
    def listing(extra):
        return listed_tests(subprocess.check_output([binary, "--list", *extra], cwd=ROOT, env=env, text=True))
    validate_names(args.names, listing([]), listing(["--ignored"]), args.ignored)
    for name in dict.fromkeys(args.names):
        run = [binary, name, "--exact", "--nocapture", "--test-threads=1"]
        if args.ignored:
            run.append("--ignored")
        print(shlex.join(run), flush=True)
        result = subprocess.run(run, cwd=ROOT, env=env)
        if result.returncode:
            return result.returncode
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"dev: {error}", file=sys.stderr)
        raise SystemExit(1)
