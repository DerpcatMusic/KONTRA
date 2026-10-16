#!/usr/bin/env python3
"""Shipping-only Cargo integration target shards; no individual test filtering.

Metadata is supplied by the caller (cargo metadata --locked --no-deps
--format-version 1). All paths/IDs in the published plan are workspace-relative;
opaque Cargo package IDs are mapped only using the current runner's metadata.
This module never invokes metadata, changes features, or executes binaries itself.
"""
import argparse
from collections import defaultdict
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tomllib

SCHEMA = 2
SHARDS = 8
ROOT = Path(__file__).resolve().parents[2]
CONTRACT = ROOT / ".github/shipping-test-features.json"
PACKAGES = (
    "kontakto", "sampler-core", "sampler-ir", "sampler-simd", "sampler-pool",
    "sampler-ksp", "sampler-kontakt", "sampler-uvi", "sampler-midi",
    "sampler-native", "sampler-ui-ir", "sampler-perf", "sampler-parity", "corpus-health",
)
KINDS = {"lib", "rlib", "staticlib", "cdylib", "dylib", "proc-macro", "bin",
         "test", "bench", "example", "custom-build"}
LIB_KINDS = {"lib", "rlib", "staticlib", "cdylib", "dylib", "proc-macro"}


class Invalid(ValueError):
    """A missing or unsupported coverage proof (always fatal)."""


def require(condition, message):
    if not condition:
        raise Invalid(message)


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def strings(value, description):
    require(isinstance(value, list) and all(isinstance(v, str) and v for v in value), description)
    require(len(value) == len(set(value)), "duplicate " + description)
    return sorted(value)


def packages_from_args(value):
    args = shlex.split(value)
    require(len(args) == 2 * len(PACKAGES) and args[::2] == ["-p"] * len(PACKAGES),
            "CRATES must contain only the fourteen explicit -p selections")
    selected = args[1::2]
    require(len(set(selected)) == len(PACKAGES) and set(selected) == set(PACKAGES),
            "CRATES changed: explicitly review the shipping package inventory")
    return selected


def relative(path, root):
    require(isinstance(path, str) and Path(path).is_absolute(), "metadata path must be absolute")
    require(".." not in Path(path).parts, "metadata path must not traverse outside workspace")
    try:
        return Path(path).relative_to(root).as_posix()
    except ValueError as error:
        raise Invalid("metadata path outside workspace: " + path) from error


def default_features(features):
    """Prove package-local default activation, without guessing Cargo's resolver.

    Dependency feature edges do not prove any extra local feature. A target whose
    required features rely on cross-package unification is deliberately refused.
    No workspace-wide metadata resolve.features set is used as a false proof for
    this narrower fourteen-package command.
    """
    active = set()
    pending = ["default"] if "default" in features else []
    while pending:
        feature = pending.pop()
        if feature in active:
            continue
        require(feature in features, "unknown local default feature: " + feature)
        active.add(feature)
        for edge in features[feature]:
            if edge.startswith("dep:") or "/" in edge:
                continue
            require(edge in features, "unknown feature activation: " + edge)
            pending.append(edge)
    return sorted(active)


def inventory(metadata, selected):
    require(metadata.get("version") == 1, "expected Cargo metadata format version 1")
    root = Path(metadata["workspace_root"])
    require(root.is_absolute(), "workspace root must be absolute")
    members = set(metadata["workspace_members"])
    by_name = defaultdict(list)
    for package in metadata["packages"]:
        if package["id"] in members:
            by_name[package["name"]].append(package)
    result, opaque = [], {}
    for name in selected:
        require(len(by_name[name]) == 1, "missing/ambiguous workspace package: " + name)
        package = by_name[name][0]
        require(package.get("source") is None, "first-party package must be local: " + name)
        require(package["id"] not in opaque, "duplicate Cargo package ID")
        opaque[package["id"]] = name
        features = {key: strings(value, "feature edges") for key, value in sorted(package["features"].items())}
        defaults = default_features(features)
        targets, target_keys = [], set()
        for target in package["targets"]:
            kinds = strings(target["kind"], "target kinds")
            require(kinds and set(kinds) <= KINDS, "unknown Cargo target kind")
            require(len(kinds) == 1 or set(kinds) <= LIB_KINDS, "unsupported mixed Cargo target kind")
            target_name = target["name"]
            require(isinstance(target_name, str) and re.fullmatch(r"[A-Za-z0-9_-]+", target_name),
                    "target name is not a literal Cargo selector")
            key = (tuple(kinds), target_name)
            require(key not in target_keys, "duplicate package/target: " + name + "/" + target_name)
            target_keys.add(key)
            required = strings(target.get("required-features", []), "required features")
            require(set(required) <= set(features), "unknown required feature: " + name + "/" + target_name)
            require(type(target["test"]) is bool and type(target["doctest"]) is bool,
                    "missing Cargo test/doctest flags")
            if kinds == ["test"]:
                require(target["test"], "test=false integration target needs an explicit coverage policy")
                require(set(required) <= set(defaults),
                        "cannot prove integration target required features active by default: " + name + "/" + target_name)
            if kinds == ["example"]:
                require(not target["test"], "test=true example needs execution coverage, not build-only")
            if kinds == ["bench"]:
                require(not target["test"], "test=true benchmark needs explicit default-test coverage")
            if kinds == ["bin"] or set(kinds) <= LIB_KINDS:
                require(target["test"], "test=false library/binary needs an explicit target-selection policy")
            if set(kinds) <= LIB_KINDS:
                require(target["doctest"], "doctest=false library needs an explicit documentation-test policy")
            targets.append(dict(name=target_name, kind=kinds,
                                crate_types=strings(target["crate_types"], "crate types"),
                                src_path=relative(target["src_path"], root),
                                required_features=required, test=target["test"], doctest=target["doctest"]))
        dependencies = []
        for dependency in package["dependencies"]:
            item = {key: dependency[key] for key in (
                "name", "rename", "kind", "req", "source", "optional", "uses_default_features", "target")}
            item["features"] = strings(dependency["features"], "dependency features")
            item["path"] = relative(dependency["path"], root) if dependency.get("path") else None
            dependencies.append(item)
        result.append(dict(name=name, version=package["version"],
                           manifest_path=relative(package["manifest_path"], root),
                           features=features, default_features=defaults,
                           dependencies=sorted(dependencies, key=lambda d: json.dumps(d, sort_keys=True)),
                           targets=sorted(targets, key=lambda t: (t["kind"], t["name"]))))
    return result, opaque


def integration_targets(items):
    return sorted((dict(package=p["name"], name=t["name"], src_path=t["src_path"])
                   for p in items for t in p["targets"] if t["kind"] == ["test"]),
                  key=lambda t: (t["package"], t["name"]))


def feature_contract(value):
    """Only a checked-in, explicitly reviewed emitted contract is authoritative.

    Feature declarations/default closures are not resolver output. In particular,
    implicit dependency feature names in Cargo's real arrays are accepted here.
    A null entry is deliberately unfilled, never an empty enabled feature set.
    """
    require(value.get("schema") == 1, "unsupported shipping feature contract")
    packages = value.get("packages", {})
    require(set(packages) == set(PACKAGES), "feature contract must cover exactly all fourteen packages")
    for name in PACKAGES:
        require(packages[name] is not None, "unfilled reviewed emitted feature contract: " + name)
    normalized = {name: strings(packages[name], "reviewed artifact features for " + name) for name in PACKAGES}
    evidence = value.get("evidence")
    require(isinstance(evidence, list) and evidence, "feature contract needs reviewed Cargo proof references")
    for proof in evidence:
        require(isinstance(proof.get("source"), str) and re.fullmatch(r"[0-9a-f]{40}", proof["source"]),
                "feature proof needs its exact source SHA")
        require(isinstance(proof.get("log_sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", proof["log_sha256"]),
                "feature proof needs its completed Cargo log hash")
        require(isinstance(proof.get("command"), list) and proof["command"] and
                all(isinstance(v, str) and v for v in proof["command"]), "feature proof needs its exact argv")
    return dict(schema=1, packages=normalized, evidence=evidence)


def validate_features(plan, package, features):
    observed = strings(features, "artifact features")
    expected = plan["feature_contract"]["packages"][package]
    require(observed == expected,
            "emitted feature contract differs for " + package + ": missing=" +
            repr(sorted(set(expected) - set(observed))) + " unexpected=" +
            repr(sorted(set(observed) - set(expected))))


def make_plan(items, selected, source, contract, stamp, count=SHARDS):
    require(re.fullmatch(r"[0-9a-f]{40}", source) is not None, "source must be an exact commit SHA")
    require(type(count) is int and count == SHARDS, "shipping requires exactly eight shards")
    require([p["name"] for p in items] == selected, "package selection/order differs from inventory")
    contract = feature_contract(contract)
    for package in items:
        require(set(package["default_features"]) <= set(contract["packages"][package["name"]]),
                "reviewed emitted contract omits package-local defaults: " + package["name"])
    require(type(stamp.get("source_date_epoch")) is int and stamp["source_date_epoch"] >= 0,
            "stamp needs the commit-derived source epoch")
    require(next(p for p in items if p["name"] == "kontakto")["version"] == stamp["version"],
            "metadata root version differs from actual Nightly stamp")
    for name in ("cargo_toml_sha256", "cargo_lock_sha256"):
        require(isinstance(stamp.get(name), str) and re.fullmatch(r"[0-9a-f]{64}", stamp[name]),
                "stamp needs actual manifest/lock hashes")
    targets = integration_targets(items)
    require(targets, "empty integration inventory")
    require(len({(t["package"], t["name"]) for t in targets}) == len(targets), "duplicate integration target")
    groups = defaultdict(list)
    for target in targets:
        groups[target["name"]].append(target)
    require(len(groups) >= count, "not enough target-name groups: empty shards are forbidden")
    shards = [dict(index=i, selectors=[], targets=[]) for i in range(count)]
    # Keep collisions together. Stable greedy placement balances executable counts,
    # with no timing guesses, target renaming, or individual libtest filters.
    for name in sorted(groups, key=lambda n: (-len(groups[n]), n)):
        shard = min(shards, key=lambda s: (len(s["targets"]), s["index"]))
        shard["selectors"].append(name)
        shard["targets"].extend(groups[name])
    inventory_id = digest(items)
    contract_id = digest(contract)
    for shard in shards:
        shard["selectors"].sort()
        shard["targets"].sort(key=lambda t: (t["package"], t["name"]))
        shard["id"] = digest(dict(source=source, stamp=stamp, feature_contract_id=contract_id,
                                  inventory_id=inventory_id, shard=shard))
    result = dict(schema=SCHEMA, source=source, profile="release", packages=selected,
                  stamp=stamp, feature_contract=contract, feature_contract_id=contract_id,
                  inventory=items, inventory_id=inventory_id, shards=shards)
    result["id"] = digest(result)
    return result


def validate_plan(plan):
    packages_from_args(shlex.join([v for name in plan["packages"] for v in ("-p", name)]))
    require(digest(plan) == digest(make_plan(plan["inventory"], plan["packages"], plan["source"],
                                           plan["feature_contract"], plan["stamp"])),
            "plan identity/assignment changed")


def validate_current(plan, metadata, selected, source, contract, stamp):
    validate_plan(plan)
    require(plan["source"] == source and plan["packages"] == selected, "source/package selection changed")
    require(stamp == plan["stamp"], "actual stamped source/version/epoch changed since planning")
    require(digest(feature_contract(contract)) == plan["feature_contract_id"], "reviewed feature contract changed")
    current, opaque = inventory(metadata, selected)
    require(digest(current) == plan["inventory_id"], "Cargo inventory changed since planning")
    return opaque


def command(plan, shard):
    return (["cargo", "test", "--profile", "release", "--locked", "--message-format=json-render-diagnostics"]
            + [value for name in plan["packages"] for value in ("-p", name)]
            + [value for name in shard["selectors"] for value in ("--test", name)])


def validate_actual(plan, shard, actual):
    require(actual, "no test executable artifacts")
    targets = unit_targets(plan) if shard is None else shard["targets"]
    def key_for(target):
        key = (target["package"], target["name"])
        return key + (tuple(target["kind"]),) if shard is None else key
    expected = {key_for(t): t for t in targets}
    seen = set()
    for artifact in actual:
        key = key_for(artifact)
        require(key in expected, "wrong package/test executable: " + repr(key))
        require(key not in seen, "duplicate test executable: " + repr(key))
        seen.add(key)
        require(artifact["src_path"] == expected[key]["src_path"], "wrong test source path")
        validate_features(plan, artifact["package"], artifact["features"])
    require(seen == set(expected), "missing assigned test executables: " + repr(sorted(set(expected) - seen)))


def inspect_messages(lines, returncode, plan, shard, opaque, root):
    require(returncode == 0, "cargo test failed with exit " + str(returncode))
    actual, finished = [], False
    for line in lines:
        # After Cargo finishes compiling, stdout belongs to the unmodified tests.
        if finished:
            continue
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if not isinstance(message, dict):
            continue
        if message.get("reason") == "build-finished":
            require(message.get("success") is True, "Cargo build-finished reported failure")
            finished = True
        elif message.get("reason") == "compiler-artifact":
            package = opaque.get(message.get("package_id"))
            if package and message["target"]["kind"] != ["custom-build"]:
                # Validate relevant emitted first-party libraries/binaries too,
                # without requiring all fourteen roots in each integration shard.
                validate_features(plan, package, message["features"])
            if not message.get("profile", {}).get("test"):
                continue
            target = message["target"]
            if shard is not None:
                require(target["kind"] == ["test"], "unexpected non-integration test executable")
            else:
                require(target["kind"] == ["bin"] or set(target["kind"]) <= LIB_KINDS,
                        "unexpected non-library/binary test executable")
            require(message.get("executable"), "test artifact missing executable")
            require(message["package_id"] in opaque, "test artifact from unselected package")
            artifact = dict(package=opaque[message["package_id"]], name=target["name"],
                            src_path=relative(target["src_path"], root), features=message["features"])
            if shard is None:
                artifact["kind"] = sorted(target["kind"])
            actual.append(artifact)
    require(finished, "Cargo produced no successful build-finished message")
    validate_actual(plan, shard, actual)
    return sorted(actual, key=lambda a: (a["package"], a["name"]))


def receipt(plan, shard, actual):
    validate_actual(plan, shard, actual)
    return dict(schema=SCHEMA, status="success", source=plan["source"], plan_id=plan["id"],
                stamp=plan["stamp"], feature_contract_id=plan["feature_contract_id"],
                inventory_id=plan["inventory_id"], shard_index=shard["index"], shard_id=shard["id"],
                cargo_returncode=0, build_finished=True, command=command(plan, shard), actual=actual)


def aggregate(plan, receipts, source):
    validate_plan(plan)
    require(source == plan["source"], "aggregate source mismatch")
    require(len(receipts) == SHARDS, "exactly eight successful receipts are required")
    seen, actual_union, package_features = set(), set(), {}
    for item in receipts:
        index = item["shard_index"]
        require(type(index) is int and 0 <= index < SHARDS and index not in seen,
                "duplicate/invalid receipt shard")
        seen.add(index)
        shard = plan["shards"][index]
        require(digest(item) == digest(receipt(plan, shard, item["actual"])), "failed/stale/modified shard receipt")
        for target in item["actual"]:
            key = (target["package"], target["name"])
            require(key not in actual_union, "duplicate executable across receipts")
            actual_union.add(key)
            features = sorted(target["features"])
            previous = package_features.setdefault(target["package"], features)
            require(previous == features, "package features differ across integration executables/shards: " + target["package"])
    require(actual_union == {(t["package"], t["name"]) for t in integration_targets(plan["inventory"])},
            "aggregate integration target union differs from inventory")
    return dict(source=source, plan_id=plan["id"], inventory_id=plan["inventory_id"],
                stamp=plan["stamp"], feature_contract_id=plan["feature_contract_id"],
                shards=len(seen), executables=len(actual_union), status="success")


def unit_targets(plan):
    """All selected eligible unit targets, including roots absent from shards."""
    return [dict(package=p["name"], name=t["name"], kind=t["kind"], src_path=t["src_path"])
            for p in plan["inventory"] for t in p["targets"]
            if (t["kind"] == ["bin"] or set(t["kind"]) <= LIB_KINDS)
            and set(t["required_features"]) <= set(plan["feature_contract"]["packages"][p["name"]])]


def non_integration_commands(plan):
    packages = [v for name in plan["packages"] for v in ("-p", name)]
    return [["cargo", "test", "--locked", "--profile", "release"] + packages +
            ["--lib", "--bins", "--message-format=json-render-diagnostics"],
            ["cargo", "test", "--locked", "--profile", "release"] + packages + ["--doc"],
            ["cargo", "build", "--locked", "--profile", "release"] + packages + ["--examples"]]


def check_source(source, stamped=True):
    def git(*args):
        return subprocess.check_output(["git", "-C", str(ROOT), *args])
    head = git("rev-parse", "HEAD").decode().strip()
    require(head == source, "checkout HEAD differs from requested source SHA")
    # Verify the version helper before importing it, so an edited validator cannot
    # authorize its own dirty source. Execute the pinned bytes, not cached pyc.
    path = ROOT / "tools/version.py"
    trusted = git("show", source + ":tools/version.py")
    require(not path.is_symlink() and path.read_bytes() == trusted, "version validator differs from reviewed source")
    spec = importlib.util.spec_from_file_location("shipping_version", path)
    version = importlib.util.module_from_spec(spec)
    exec(compile(trusted, str(path), "exec"), version.__dict__)
    epoch = int(git("show", "-s", "--format=%ct", source))
    originals = {name: git("show", source + ":" + name).decode() for name, _ in version.MANIFESTS}
    package = tomllib.loads(originals["Cargo.toml"])["package"]
    expected_version = version.nightly(package["version"], source, epoch)
    expected = {name: version.version_text(originals[name], header, package["name"], expected_version).encode()
                for name, header in version.MANIFESTS}
    if stamped:
        require(os.environ.get("SOURCE_DATE_EPOCH") == str(epoch), "SOURCE_DATE_EPOCH differs from commit epoch")
        for variable in ("KONTRA_BUILD_REVISION", "KONTRA_SOURCE_REVISION"):
            require(os.environ.get(variable, source) == source, variable + " differs from actual source SHA")
        require(not version.nightly_dirty(), "source changes exceed the exact deterministic Nightly stamp")
        for name in expected:
            require((ROOT / name).read_bytes() == expected[name], "actual stamped bytes differ: " + name)
        actual_package = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]
        lock_versions = [p["version"] for p in tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
                         if p["name"] == package["name"] and "source" not in p]
        require(actual_package["version"] == expected_version and lock_versions == [expected_version],
                "actual Cargo.toml/Cargo.lock versions differ from deterministic stamp")
    return dict(version=expected_version, source_date_epoch=epoch,
                cargo_toml_sha256=hashlib.sha256(expected["Cargo.toml"]).hexdigest(),
                cargo_lock_sha256=hashlib.sha256(expected["Cargo.lock"]).hexdigest())


def execute(argv, log):
    with subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                          text=True, encoding="utf-8", errors="replace") as process:
        for line in process.stdout:
            log.write(line)
            print(line, end="", flush=True)
        return process.wait()


def load(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    for operation in ("plan", "run", "units", "aggregate"):
        cli = sub.add_parser(operation)
        cli.add_argument("--source", required=True)
        cli.add_argument("--plan", required=True)
        cli.add_argument("--feature-contract", default=str(CONTRACT))
        if operation != "aggregate":
            cli.add_argument("--metadata", required=True)
            cli.add_argument("--crates", required=True)
        if operation == "plan":
            cli.add_argument("--github-output")
        elif operation in ("run", "units"):
            if operation == "run":
                cli.add_argument("--shard", required=True, type=int)
                cli.add_argument("--shard-id", required=True)
            cli.add_argument("--receipt", required=True)
            cli.add_argument("--log", required=True)
        else:
            cli.add_argument("--receipts", required=True, help="directory containing receipt-*.json")
    args = parser.parse_args()
    if args.operation in ("run", "units"):
        # Remove stale success even when source validation fails before Cargo.
        Path(args.receipt).unlink(missing_ok=True)
    stamp = check_source(args.source, stamped=args.operation != "aggregate")
    contract = feature_contract(load(args.feature_contract))
    if args.operation == "plan":
        selected = packages_from_args(args.crates)
        items, _ = inventory(load(args.metadata), selected)
        plan = make_plan(items, selected, args.source, contract, stamp)
        require(check_source(args.source) == stamp, "source identity changed during planning")
        write(args.plan, plan)
        matrix = dict(include=[dict(shard=s["index"], shard_id=s["id"]) for s in plan["shards"]])
        if args.github_output:
            with Path(args.github_output).open("a") as output:
                output.write("matrix=" + json.dumps(matrix, separators=(",", ":")) + "\n")
        print(json.dumps(dict(source=args.source, inventory_id=plan["inventory_id"], plan_id=plan["id"],
                              stamp=stamp, feature_contract_id=plan["feature_contract_id"],
                              shards=[dict(index=s["index"], id=s["id"], targets=len(s["targets"])) for s in plan["shards"]])))
    elif args.operation in ("run", "units"):
        plan, metadata = load(args.plan), load(args.metadata)
        selected = packages_from_args(args.crates)
        opaque = validate_current(plan, metadata, selected, args.source, contract, stamp)
        shard = None
        if args.operation == "run":
            require(0 <= args.shard < SHARDS, "invalid shard index")
            shard = plan["shards"][args.shard]
            require(shard["id"] == args.shard_id, "matrix shard ID differs from plan")
        commands = [command(plan, shard)] if shard is not None else non_integration_commands(plan)
        header = json.dumps(dict(source=args.source, plan_id=plan["id"], inventory_id=plan["inventory_id"],
                                 stamp=stamp, feature_contract_id=plan["feature_contract_id"],
                                 shard_id=shard["id"] if shard is not None else None, commands=commands))
        print(header, flush=True)
        with Path(args.log).open("w") as log:
            log.write(header + "\n")
            code = execute(commands[0], log)
        with Path(args.log).open() as log:
            actual = inspect_messages(log, code, plan, shard, opaque, Path(metadata["workspace_root"]))
        def verify_after_cargo():
            # A successful process does not authorize changed source or identity.
            require(check_source(args.source) == stamp, "source identity changed during Cargo")
            require(digest(feature_contract(load(args.feature_contract))) == plan["feature_contract_id"],
                    "reviewed feature contract changed during Cargo")
        verify_after_cargo()
        with Path(args.log).open("a") as log:
            for argv in commands[1:]:
                require(execute(argv, log) == 0, "shipping documentation/example Cargo process failed")
                verify_after_cargo()
        if shard is not None:
            result = receipt(plan, shard, actual)
        else:
            result = dict(schema=SCHEMA, status="success", scope="lib_bins_doc_examples", source=args.source,
                          stamp=stamp, feature_contract_id=plan["feature_contract_id"], plan_id=plan["id"],
                          commands=commands, actual=actual, cargo_returncode=0, build_finished=True)
        write(args.receipt, result)
    else:
        plan = load(args.plan)
        require(plan["stamp"] == stamp and plan["feature_contract_id"] == digest(contract),
                "aggregate source stamp/feature contract mismatch")
        receipts = [load(path) for path in sorted(Path(args.receipts).glob("receipt-*.json"))]
        print(json.dumps(aggregate(plan, receipts, args.source), sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (Invalid, KeyError, TypeError, ValueError, OSError, subprocess.CalledProcessError) as error:
        print("Shipping test coverage rejected: " + str(error), file=sys.stderr)
        sys.exit(1)
