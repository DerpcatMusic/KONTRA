"""Portable shipping coverage tests: synthetic Cargo JSON and isolated Git; no Cargo."""
import copy
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import MagicMock, patch

import shipping_tests as shipping

SOURCE = "a" * 40
STAMP = dict(version="0.1.0-nightly.20231114.gaaaaaaaaaaaa", source_date_epoch=1700000000,
             cargo_toml_sha256="1" * 64, cargo_lock_sha256="2" * 64)
FIXTURE_ROOT = Path.cwd().resolve() / "synthetic-shipping-workspace"
CRATES = " ".join("-p " + name for name in shipping.PACKAGES)


def fixture(root=FIXTURE_ROOT):
    packages = []
    for index, name in enumerate(shipping.PACKAGES):
        directory = root / ("crates/" + name)
        features = {"default": ["enabled"], "enabled": [], "optional": []}
        target_names = ["case_" + str(index)]
        if index < 2:
            target_names.append("collision")
        targets = [dict(name=target, kind=["test"], crate_types=["bin"],
                        src_path=str(directory / "tests" / (target + ".rs")),
                        test=True, doctest=False) for target in target_names]
        targets.append(dict(name=name.replace("-", "_"), kind=["lib"], crate_types=["lib"],
                            src_path=str(directory / "src/lib.rs"), test=True, doctest=True))
        targets.append(dict(name="example", kind=["example"], crate_types=["bin"],
                            src_path=str(directory / "examples/example.rs"), test=False, doctest=False))
        packages.append(dict(name=name, id="opaque:" + name + ":" + str(root),
                             version=STAMP["version"] if name == "kontakto" else "0.1.0",
                             source=None, manifest_path=str(directory / "Cargo.toml"),
                             features=features, dependencies=[], targets=targets))
    return dict(version=1, workspace_root=str(root), packages=packages,
                workspace_members=[p["id"] for p in packages], resolve=None)


def contract_fixture():
    # Deliberately synthetic reviewed arrays; production values are never guessed.
    return dict(schema=1, packages={name: ["default", "enabled"] for name in shipping.PACKAGES},
                evidence=[dict(source=SOURCE, log_sha256="e" * 64,
                               command=["cargo", "test", "--profile", "release", "--locked"] +
                               [v for name in shipping.PACKAGES for v in ("-p", name)] +
                               ["--test", "source", "--test", "inspect_seed"])])


def prepared(metadata=None, contract=None, stamp=None, source=SOURCE):
    metadata = metadata or fixture()
    selected = shipping.packages_from_args(CRATES)
    items, opaque = shipping.inventory(metadata, selected)
    return shipping.make_plan(items, selected, source, contract_fixture() if contract is None else contract,
                              STAMP if stamp is None else stamp), opaque


def actual_for(plan, shard):
    return [dict(target, features=plan["feature_contract"]["packages"][target["package"]])
            for target in shard["targets"]]


def messages(plan, shard, opaque, root=FIXTURE_ROOT):
    package_ids = {name: key for key, name in opaque.items()}
    result = []
    for actual in actual_for(plan, shard):
        result.append(dict(reason="compiler-artifact", package_id=package_ids[actual["package"]],
                           target=dict(kind=["test"], name=actual["name"], src_path=str(root / actual["src_path"])),
                           profile=dict(test=True, opt_level="3"), features=actual["features"],
                           executable=str(root / "target/release/deps" / actual["name"])))
    result.append(dict(reason="build-finished", success=True))
    return result


class Plans(unittest.TestCase):
    def test_exact_union_and_no_duplicate_targets(self):
        plan, _ = prepared()
        expected = shipping.integration_targets(plan["inventory"])
        assigned = [t for shard in plan["shards"] for t in shard["targets"]]
        key = lambda t: (t["package"], t["name"])
        self.assertEqual(sorted(assigned, key=key), expected)
        self.assertEqual(len({key(t) for t in assigned}), len(expected))
        self.assertEqual(len(plan["shards"]), 8)
        self.assertTrue(all(shard["targets"] and shard["selectors"] for shard in plan["shards"]))

    def test_collision_names_stay_together(self):
        plan, _ = prepared()
        shards = [s for s in plan["shards"] if "collision" in s["selectors"]]
        self.assertEqual(len(shards), 1)
        self.assertEqual(sum(t["name"] == "collision" for t in shards[0]["targets"]), 2)

    def test_plan_deterministic_across_metadata_order_and_checkout_paths(self):
        plan, _ = prepared()
        other = fixture(FIXTURE_ROOT.parent / "different-runner-checkout")
        other["packages"].reverse()
        other["workspace_members"].reverse()
        for package in other["packages"]:
            package["targets"].reverse()
        self.assertEqual(prepared(other)[0], plan)

    def test_empty_inventory_and_empty_shards_refused(self):
        for keep in (0, 1, 7):
            metadata = fixture()
            for index, package in enumerate(metadata["packages"]):
                package["targets"] = [t for t in package["targets"] if t["kind"] != ["test"]]
                if index < keep:
                    package["targets"].append(dict(name="kept_" + str(index), kind=["test"], crate_types=["bin"],
                                                    src_path=str(FIXTURE_ROOT / "tests" / ("kept_" + str(index) + ".rs")),
                                                    test=True, doctest=False))
            with self.subTest(keep=keep), self.assertRaises(shipping.Invalid):
                prepared(metadata)

    def test_duplicate_metadata_targets_and_packages_refused(self):
        for mutation in ("target", "package", "missing"):
            metadata = fixture()
            if mutation == "target":
                metadata["packages"][0]["targets"].append(copy.deepcopy(metadata["packages"][0]["targets"][0]))
            elif mutation == "package":
                metadata["packages"].append(copy.deepcopy(metadata["packages"][0]))
            else:
                metadata["packages"].pop()
            with self.subTest(mutation=mutation), self.assertRaises(shipping.Invalid):
                prepared(metadata)

    def test_changed_inventory_features_and_source_refused(self):
        plan, _ = prepared()
        for mutation in ("new-target", "feature", "required-feature", "default", "source"):
            metadata = fixture()
            source = SOURCE
            if mutation == "new-target":
                target = copy.deepcopy(metadata["packages"][0]["targets"][0])
                target["name"] = "new_target"
                metadata["packages"][0]["targets"].append(target)
            elif mutation == "feature":
                metadata["packages"][0]["features"]["new"] = []
            elif mutation == "required-feature":
                metadata["packages"][0]["targets"][0]["required-features"] = ["enabled"]
            elif mutation == "default":
                metadata["packages"][0]["features"]["default"] = []
            else:
                source = "b" * 40
            with self.subTest(mutation=mutation), self.assertRaises(shipping.Invalid):
                shipping.validate_current(plan, metadata, list(shipping.PACKAGES), source,
                                          plan["feature_contract"], plan["stamp"])

    def test_plan_tampering_refused(self):
        original, _ = prepared()
        for mutation in ("missing", "duplicate", "wrong-shard", "empty", "id"):
            plan = copy.deepcopy(original)
            if mutation == "missing":
                plan["shards"][0]["targets"].pop()
            elif mutation == "duplicate":
                plan["shards"][1]["targets"].append(plan["shards"][0]["targets"][0])
            elif mutation == "wrong-shard":
                plan["shards"][0]["targets"], plan["shards"][1]["targets"] = plan["shards"][1]["targets"], plan["shards"][0]["targets"]
            elif mutation == "empty":
                plan["shards"][0]["selectors"] = []
            else:
                plan["shards"][0]["id"] = "0" * 64
            with self.subTest(mutation=mutation), self.assertRaises(shipping.Invalid):
                shipping.validate_plan(plan)

    def test_feature_gate_requires_known_local_default_activation(self):
        metadata = fixture()
        target = metadata["packages"][0]["targets"][0]
        target["required-features"] = ["enabled"]
        prepared(metadata)  # Exact default closure is a sufficient proof.
        for feature in ("optional", "unknown", "dependency/feature"):
            target["required-features"] = [feature]
            with self.subTest(feature=feature), self.assertRaises(shipping.Invalid):
                prepared(metadata)
        metadata["packages"][0]["features"]["default"] = ["unknown"]
        with self.assertRaises(shipping.Invalid):
            prepared(metadata)

    def test_unsupported_target_settings_fail_closed(self):
        for mutation in ("integration-test-false", "example-test-true", "bench-test-true", "lib-test-false",
                         "lib-doctest-false", "glob", "kind", "mixed-kind", "outside-root"):
            metadata = fixture()
            target = metadata["packages"][0]["targets"][0]
            if mutation == "integration-test-false":
                target["test"] = False
            elif mutation == "example-test-true":
                metadata["packages"][0]["targets"][-1]["test"] = True
            elif mutation == "bench-test-true":
                target["kind"] = ["bench"]
            elif mutation == "lib-test-false":
                metadata["packages"][0]["targets"][-2]["test"] = False
            elif mutation == "lib-doctest-false":
                metadata["packages"][0]["targets"][-2]["doctest"] = False
            elif mutation == "glob":
                target["name"] = "case*"
            elif mutation == "kind":
                target["kind"] = ["future-kind"]
            elif mutation == "mixed-kind":
                target["kind"] = ["bin", "test"]
            else:
                target["src_path"] = str(FIXTURE_ROOT.parent / "outside/test.rs")
            with self.subTest(mutation=mutation), self.assertRaises(shipping.Invalid):
                prepared(metadata)

    def test_exact_fourteen_package_selection_and_no_feature_overrides(self):
        self.assertEqual(shipping.packages_from_args(CRATES), list(shipping.PACKAGES))
        for value in ("", CRATES + " --no-default-features", CRATES + " --features optional",
                      CRATES.replace("sampler-ir", "sampler-core"), CRATES + " -p vendor"):
            with self.subTest(value=value), self.assertRaises(shipping.Invalid):
                shipping.packages_from_args(value)

    def test_command_preserves_release_defaults_and_all_packages(self):
        plan, _ = prepared()
        for shard in plan["shards"]:
            cmd = shipping.command(plan, shard)
            self.assertEqual(cmd[:6], ["cargo", "test", "--profile", "release", "--locked", "--message-format=json-render-diagnostics"])
            self.assertEqual([cmd[i + 1] for i, value in enumerate(cmd) if value == "-p"], list(shipping.PACKAGES))
            self.assertEqual([cmd[i + 1] for i, value in enumerate(cmd) if value == "--test"], shard["selectors"])
            for excluded in ("--", "--no-run", "--ignored", "--features", "--no-default-features", "--all-features"):
                self.assertNotIn(excluded, cmd)


class Artifacts(unittest.TestCase):
    def setUp(self):
        self.plan, self.opaque = prepared()
        self.shard = self.plan["shards"][0]
        self.messages = messages(self.plan, self.shard, self.opaque)

    def inspect(self, data=None, code=0):
        return shipping.inspect_messages((json.dumps(m) for m in (self.messages if data is None else data)),
                                         code, self.plan, self.shard, self.opaque, FIXTURE_ROOT)

    def test_actual_artifact_set_including_fresh_cache_hits(self):
        self.messages[0]["fresh"] = True
        self.assertEqual(self.inspect(), actual_for(self.plan, self.shard))

    def test_dependency_artifacts_and_harness_output_are_not_targets(self):
        self.messages.insert(0, dict(reason="compiler-artifact", profile=dict(test=False)))
        self.messages.append(dict(reason="compiler-artifact", profile=dict(test=True)))
        self.assertEqual(self.inspect(), actual_for(self.plan, self.shard))

    def test_missing_duplicate_wrong_executable_and_default_features(self):
        for mutation in ("missing", "duplicate", "wrong-name", "wrong-package", "wrong-kind",
                         "wrong-path", "no-executable", "no-default", "unknown-feature"):
            data = copy.deepcopy(self.messages)
            if mutation == "missing":
                data.pop(0)
            elif mutation == "duplicate":
                data.insert(0, copy.deepcopy(data[0]))
            elif mutation == "wrong-name":
                data[0]["target"]["name"] = "not_assigned"
            elif mutation == "wrong-package":
                data[0]["package_id"] = "unknown-package"
            elif mutation == "wrong-kind":
                data[0]["target"]["kind"] = ["lib"]
            elif mutation == "wrong-path":
                data[0]["target"]["src_path"] = str(FIXTURE_ROOT / "wrong.rs")
            elif mutation == "no-executable":
                data[0]["executable"] = None
            elif mutation == "no-default":
                data[0]["features"] = []
            else:
                data[0]["features"].append("unknown")
            with self.subTest(mutation=mutation), self.assertRaises(shipping.Invalid):
                self.inspect(data)

    def test_build_and_test_failures_and_zero_output_success_rejected(self):
        for data, code in ((self.messages, 101), ([], 0),
                           ([dict(reason="build-finished", success=True)], 0),
                           (self.messages[:-1], 0),
                           (self.messages[:-1] + [dict(reason="build-finished", success=False)], 0)):
            with self.subTest(data=data, code=code), self.assertRaises(shipping.Invalid):
                self.inspect(data, code)


class Aggregate(unittest.TestCase):
    def setUp(self):
        self.plan, _ = prepared()
        self.receipts = [shipping.receipt(self.plan, shard, actual_for(self.plan, shard)) for shard in self.plan["shards"]]

    def test_all_eight_receipts_exact_union(self):
        result = shipping.aggregate(self.plan, self.receipts, SOURCE)
        self.assertEqual(result["shards"], 8)
        self.assertEqual(result["executables"], len(shipping.integration_targets(self.plan["inventory"])))

    def test_feature_unification_consistent_across_shards(self):
        # A known extra feature on only one executable must not pass aggregation.
        self.receipts[0]["actual"][0]["features"] = ["default", "enabled", "optional"]
        with self.assertRaises(shipping.Invalid):
            shipping.aggregate(self.plan, self.receipts, SOURCE)

    def test_missing_duplicate_failed_stale_or_modified_receipts_rejected(self):
        for mutation in ("missing", "duplicate", "failure", "cancelled", "skipped", "source", "inventory", "shard-id",
                         "exit", "bool-exit", "build-finished", "command", "empty", "missing-artifact"):
            receipts = copy.deepcopy(self.receipts)
            if mutation == "missing":
                receipts.pop()
            elif mutation == "duplicate":
                receipts[1] = copy.deepcopy(receipts[0])
            elif mutation in ("failure", "cancelled", "skipped"):
                receipts[0]["status"] = mutation
            elif mutation == "source":
                receipts[0]["source"] = "b" * 40
            elif mutation == "inventory":
                receipts[0]["inventory_id"] = "0" * 64
            elif mutation == "shard-id":
                receipts[0]["shard_id"] = "0" * 64
            elif mutation == "exit":
                receipts[0]["cargo_returncode"] = 101
            elif mutation == "bool-exit":
                receipts[0]["cargo_returncode"] = False
            elif mutation == "build-finished":
                receipts[0]["build_finished"] = False
            elif mutation == "command":
                receipts[0]["command"].append("--no-run")
            elif mutation == "empty":
                receipts[0]["actual"] = []
            else:
                receipts[0]["actual"].pop()
            with self.subTest(mutation=mutation), self.assertRaises(shipping.Invalid):
                shipping.aggregate(self.plan, receipts, SOURCE)
        with self.assertRaises(shipping.Invalid):
            shipping.aggregate(self.plan, self.receipts, "b" * 40)


class Runner(unittest.TestCase):
    def test_cli_streams_cargo_and_writes_receipt_only_after_success(self):
        # Mock Cargo, not the coverage validator; this exercises the finite CLI.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            metadata = fixture(root)
            plan, opaque = prepared(metadata)
            shard = plan["shards"][0]
            plan_path, metadata_path = root / "plan.json", root / "metadata.json"
            shipping.write(plan_path, plan)
            shipping.write(metadata_path, metadata)
            contract_path = root / "features.json"
            shipping.write(contract_path, contract_fixture())
            receipt_path, log_path = root / "receipt.json", root / "run.log"
            argv = ["shipping_tests.py", "run", "--source", SOURCE, "--plan", str(plan_path),
                    "--metadata", str(metadata_path), "--crates", CRATES, "--shard", "0",
                    "--shard-id", shard["id"], "--receipt", str(receipt_path), "--log", str(log_path),
                    "--feature-contract", str(contract_path)]
            good = "\n".join(json.dumps(m) for m in messages(plan, shard, opaque, root)) + "\nrunning tests\n"
            for output, code in ((good, 0), (good, 101), ("", 0)):
                shipping.write(receipt_path, {"stale": "success"})
                process = MagicMock()
                process.__enter__.return_value = process
                process.stdout = io.StringIO(output)
                process.wait.return_value = code
                with patch.object(sys, "argv", argv), patch.object(shipping, "check_source", return_value=STAMP), \
                     patch.object(shipping.subprocess, "Popen", return_value=process) as popen, \
                     patch("sys.stdout", new_callable=io.StringIO):
                    if code == 0 and output:
                        shipping.main()
                        self.assertEqual(shipping.load(receipt_path), shipping.receipt(plan, shard, actual_for(plan, shard)))
                    else:
                        with self.assertRaises(shipping.Invalid):
                            shipping.main()
                        self.assertFalse(receipt_path.exists())
                    self.assertEqual(popen.call_args.args[0], shipping.command(plan, shard))
                    header, cargo_log = log_path.read_text().split("\n", 1)
                    self.assertEqual(cargo_log, output)
                    self.assertEqual(json.loads(header)["shard_id"], shard["id"])

            shipping.write(receipt_path, {"stale": "success"})
            with patch.object(sys, "argv", argv), \
                 patch.object(shipping, "check_source", side_effect=shipping.Invalid("wrong source")), \
                 patch.object(shipping.subprocess, "Popen") as popen:
                with self.assertRaises(shipping.Invalid):
                    shipping.main()
                self.assertFalse(receipt_path.exists())
                popen.assert_not_called()


def unit_messages(plan, opaque, root):
    ids = {name: key for key, name in opaque.items()}
    return [dict(reason="compiler-artifact", package_id=ids[t["package"]],
                 target=dict(kind=t["kind"], name=t["name"], src_path=str(root / t["src_path"])),
                 profile=dict(test=True, opt_level="3"), executable=str(root / "target" / t["name"]),
                 features=plan["feature_contract"]["packages"][t["package"]])
            for t in shipping.unit_targets(plan)] + [dict(reason="build-finished", success=True)]


class ReviewedFeatures(unittest.TestCase):
    def test_unfilled_contract_and_missing_packages_refused(self):
        # These null values are intentional production blockers, never [] defaults.
        for mutation in ("null-package", "missing-package", "no-evidence", "all-empty"):
            contract = contract_fixture()
            if mutation == "null-package":
                contract["packages"]["sampler-ksp"] = None
            elif mutation == "missing-package":
                contract["packages"].pop("sampler-perf")
            elif mutation == "no-evidence":
                contract["evidence"] = None
            else:
                contract["packages"] = {name: None for name in shipping.PACKAGES}
            with self.subTest(mutation=mutation), self.assertRaises(shipping.Invalid):
                prepared(contract=contract)

    def test_exact_observed_implicit_dependency_feature_names_accepted(self):
        metadata, contract = fixture(), contract_fixture()
        package = next(p for p in metadata["packages"] if p["name"] == "sampler-ir")
        package["features"] = {"cache": ["dep:serde"]}
        # Cargo's reviewed emitted array is authoritative, including implicit
        # dependency names; metadata declarations are not an emitted-name filter.
        contract["packages"]["sampler-ir"] = ["cache", "serde"]
        plan, opaque = prepared(metadata, contract)
        for shard in plan["shards"]:
            self.assertEqual(shipping.inspect_messages(
                map(json.dumps, messages(plan, shard, opaque)), 0, plan, shard, opaque, FIXTURE_ROOT),
                actual_for(plan, shard))

    def test_uniform_cache_scan_native_ui_loss_is_rejected_across_all_receipts(self):
        metadata, contract = fixture(), contract_fixture()
        additions = {"sampler-core": ["cache"], "sampler-ir": ["cache"],
                     "sampler-ksp": ["cache", "scan"], "sampler-kontakt": ["scan", "native-ui"],
                     "sampler-uvi": ["scan"]}
        for package in metadata["packages"]:
            for name in additions.get(package["name"], []):
                package["features"][name] = []
                contract["packages"][package["name"]].append(name)
        plan, _ = prepared(metadata, contract)
        receipts = [shipping.receipt(plan, shard, actual_for(plan, shard)) for shard in plan["shards"]]
        shipping.aggregate(plan, receipts, SOURCE)
        changed = set()
        for item in receipts:
            for artifact in item["actual"]:
                before = artifact["features"]
                artifact["features"] = [f for f in before if f not in {"cache", "scan", "native-ui"}]
                if before != artifact["features"]:
                    changed.add(artifact["package"])
        self.assertEqual(changed, set(additions))
        # Equality alone still holds for each package after this uniform loss.
        for name in additions:
            arrays = [tuple(a["features"]) for r in receipts for a in r["actual"] if a["package"] == name]
            self.assertEqual(len(set(arrays)), 1)
        with self.assertRaises(shipping.Invalid):
            shipping.aggregate(plan, receipts, SOURCE)

    def test_uniform_unexpected_root_shots_or_standalone_rejected(self):
        plan, _ = prepared()
        for extra in ("shots", "standalone"):
            receipts = [shipping.receipt(plan, shard, actual_for(plan, shard)) for shard in plan["shards"]]
            for item in receipts:
                for artifact in item["actual"]:
                    if artifact["package"] == "kontakto":
                        artifact["features"] = artifact["features"] + [extra]
            with self.subTest(extra=extra), self.assertRaises(shipping.Invalid):
                shipping.aggregate(plan, receipts, SOURCE)

    def test_contract_identity_is_bound_to_plan_shards_and_receipts(self):
        plan, _ = prepared()
        changed = contract_fixture()
        changed["packages"]["sampler-ksp"].append("scan")
        other, _ = prepared(contract=changed)
        self.assertNotEqual(plan["id"], other["id"])
        self.assertTrue(all(a["id"] != b["id"] for a, b in zip(plan["shards"], other["shards"])))
        receipt = shipping.receipt(plan, plan["shards"][0], actual_for(plan, plan["shards"][0]))
        self.assertEqual(receipt["feature_contract_id"], plan["feature_contract_id"])
        with self.assertRaises(shipping.Invalid):
            shipping.validate_current(plan, fixture(), list(shipping.PACKAGES), SOURCE, changed, STAMP)

    def test_receipt_version_epoch_hash_and_contract_tampering_rejected(self):
        plan, _ = prepared()
        original = [shipping.receipt(plan, shard, actual_for(plan, shard)) for shard in plan["shards"]]
        for mutation in ("version", "epoch", "lock-hash", "contract"):
            receipts = copy.deepcopy(original)
            if mutation == "version":
                receipts[0]["stamp"]["version"] = "0.1.0-nightly.wrong"
            elif mutation == "epoch":
                receipts[0]["stamp"]["source_date_epoch"] += 1
            elif mutation == "lock-hash":
                receipts[0]["stamp"]["cargo_lock_sha256"] = "0" * 64
            else:
                receipts[0]["feature_contract_id"] = "0" * 64
            with self.subTest(mutation=mutation), self.assertRaises(shipping.Invalid):
                shipping.aggregate(plan, receipts, SOURCE)

    def test_emitted_non_test_first_party_units_also_match_contract(self):
        plan, opaque = prepared()
        shard = plan["shards"][0]
        data = messages(plan, shard, opaque)
        package_id = next(key for key, value in opaque.items() if value == "kontakto")
        data.insert(0, dict(reason="compiler-artifact", package_id=package_id,
                            target=dict(kind=["cdylib", "staticlib", "rlib"]), profile=dict(test=False),
                            features=plan["feature_contract"]["packages"]["kontakto"]))
        shipping.inspect_messages(map(json.dumps, data), 0, plan, shard, opaque, FIXTURE_ROOT)
        data[0]["features"] = data[0]["features"] + ["shots"]
        with self.assertRaises(shipping.Invalid):
            shipping.inspect_messages(map(json.dumps, data), 0, plan, shard, opaque, FIXTURE_ROOT)

    def test_shards_do_not_require_all_fourteen_artifacts(self):
        metadata = fixture()
        absent = {"sampler-native", "sampler-perf", "corpus-health"}
        for package in metadata["packages"]:
            if package["name"] in absent:
                package["targets"] = [t for t in package["targets"] if t["kind"] != ["test"]]
        plan, opaque = prepared(metadata)
        receipts = []
        for shard in plan["shards"]:
            actual = shipping.inspect_messages(map(json.dumps, messages(plan, shard, opaque)),
                                               0, plan, shard, opaque, FIXTURE_ROOT)
            self.assertTrue(absent.isdisjoint(a["package"] for a in actual))
            receipts.append(shipping.receipt(plan, shard, actual))
        shipping.aggregate(plan, receipts, SOURCE)
        # Those roots still receive unit-target feature/coverage validation.
        self.assertTrue(absent <= {t["package"] for t in shipping.unit_targets(plan)})


class NonIntegration(unittest.TestCase):
    def test_commands_retain_all_fourteen_lib_bins_doc_and_build_examples(self):
        plan, _ = prepared()
        commands = shipping.non_integration_commands(plan)
        packages = [v for name in shipping.PACKAGES for v in ("-p", name)]
        self.assertEqual(commands, [
            ["cargo", "test", "--locked", "--profile", "release"] + packages +
            ["--lib", "--bins", "--message-format=json-render-diagnostics"],
            ["cargo", "test", "--locked", "--profile", "release"] + packages + ["--doc"],
            ["cargo", "build", "--locked", "--profile", "ci"] + packages + ["--examples"]])

    def test_missing_bin_root_and_inactive_standalone_target(self):
        metadata = fixture()
        root = metadata["packages"][0]
        root["features"]["standalone"] = []
        binary = dict(name="kontakto", kind=["bin"], crate_types=["bin"],
                      src_path=str(FIXTURE_ROOT / "src/main.rs"), test=True, doctest=False)
        root["targets"].append(binary)
        root["targets"].append(dict(binary, name="kontakto-standalone", **{"required-features": ["standalone"]}))
        native = next(p for p in metadata["packages"] if p["name"] == "sampler-native")
        native["targets"] = [dict(binary, name="sampler-native", src_path=str(FIXTURE_ROOT / "native.rs"))]
        plan, opaque = prepared(metadata)
        self.assertNotIn("kontakto-standalone", [t["name"] for t in shipping.unit_targets(plan)])
        data = unit_messages(plan, opaque, FIXTURE_ROOT)
        actual = shipping.inspect_messages(map(json.dumps, data), 0, plan, None, opaque, FIXTURE_ROOT)
        self.assertEqual(len(actual), len(shipping.unit_targets(plan)))
        native_id = next(key for key, name in opaque.items() if name == "sampler-native")
        data = [m for m in data if m.get("package_id") != native_id]
        with self.assertRaises(shipping.Invalid):
            shipping.inspect_messages(map(json.dumps, data), 0, plan, None, opaque, FIXTURE_ROOT)

    def test_units_cli_requires_all_three_processes_before_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            metadata = fixture(root)
            plan, opaque = prepared(metadata)
            for name, value in (("plan.json", plan), ("metadata.json", metadata), ("features.json", contract_fixture())):
                shipping.write(root / name, value)
            receipt = root / "receipt.json"
            argv = ["shipping_tests.py", "units", "--source", SOURCE, "--plan", str(root / "plan.json"),
                    "--metadata", str(root / "metadata.json"), "--crates", CRATES,
                    "--feature-contract", str(root / "features.json"),
                    "--receipt", str(receipt), "--log", str(root / "run.log")]
            for fail_at in (None, 0, 1, 2):
                calls = []
                def execute(command, log):
                    index = len(calls)
                    calls.append(command)
                    if index == 0:
                        log.write("\n".join(map(json.dumps, unit_messages(plan, opaque, root))) + "\n")
                    else:
                        log.write("completed documentation or example command\n")
                    return 101 if index == fail_at else 0
                shipping.write(receipt, {"stale": "success"})
                with self.subTest(fail_at=fail_at), patch.object(sys, "argv", argv), \
                     patch.object(shipping, "check_source", return_value=STAMP), \
                     patch.object(shipping, "execute", side_effect=execute), patch("sys.stdout", new_callable=io.StringIO):
                    if fail_at is None:
                        shipping.main()
                        result = shipping.load(receipt)
                        self.assertEqual(result["scope"], "lib_bins_doc_examples")
                        self.assertEqual(result["stamp"], STAMP)
                        self.assertEqual(result["feature_contract_id"], plan["feature_contract_id"])
                        self.assertEqual(calls, shipping.non_integration_commands(plan))
                    else:
                        with self.assertRaises(shipping.Invalid):
                            shipping.main()
                        self.assertFalse(receipt.exists())
                        self.assertEqual(len(calls), fail_at + 1)


class SourceAcceptance(unittest.TestCase):
    """Real isolated Git fixtures and real stamp validation; Cargo stays mocked."""
    def setUp(self):
        import subprocess
        self.subprocess = subprocess
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.git("init", "-q", "-b", "shipping-source-fixture")
        self.git("config", "user.name", "Shipping fixture")
        self.git("config", "user.email", "shipping@example.invalid")
        version_source = Path(__file__).resolve().parents[2] / "tools/version.py"
        (self.root / "tools").mkdir()
        self.base = {
            "Cargo.toml": '[package]\nname = "kontakto"\nversion = "0.1.0"\n\n[profile.release]\nlto = "thin"\n',
            "Cargo.lock": '[[package]]\nname = "kontakto"\nversion = "0.1.0"\n\n[[package]]\nname = "other"\nversion = "1.0.0"\n',
            "source.rs": "// reviewed Rust source\n",
            "tools/version.py": version_source.read_text(),
        }
        for name, text in self.base.items():
            (self.root / name).write_text(text)
        self.git("add", ".")
        self.git("commit", "-qm", "reviewed source fixture")
        self.source = self.git("rev-parse", "HEAD")
        self.epoch = self.git("show", "-s", "--format=%ct", "HEAD")
        version = "0.1.0-nightly.20231114.g" + self.source[:12]
        for name in ("Cargo.toml", "Cargo.lock"):
            (self.root / name).write_text(self.base[name].replace('version = "0.1.0"', 'version = "' + version + '"', 1))
        self.stamped = {name: (self.root / name).read_bytes() for name in self.base}
        self.addCleanup(patch.stopall)
        patch.object(shipping, "ROOT", self.root).start()
        patch.dict(os.environ, SOURCE_DATE_EPOCH=self.epoch, KONTRA_BUILD_REVISION=self.source,
                   KONTRA_SOURCE_REVISION=self.source).start()
        self.stamp = shipping.check_source(self.source)

    def git(self, *args):
        env = dict(os.environ, GIT_AUTHOR_DATE="1700000000 +0000", GIT_COMMITTER_DATE="1700000000 +0000")
        return self.subprocess.check_output(["git", "-C", str(self.root), *args], text=True, env=env).strip()

    def restore_fixture(self):
        for name, data in self.stamped.items():
            (self.root / name).write_bytes(data)
        os.environ["SOURCE_DATE_EPOCH"] = self.epoch
        os.environ["KONTRA_BUILD_REVISION"] = self.source
        os.environ["KONTRA_SOURCE_REVISION"] = self.source

    def mutate(self, kind):
        if kind == "epoch":
            os.environ["SOURCE_DATE_EPOCH"] = str(int(self.epoch) + 1)
        elif kind in ("build-revision", "source-revision"):
            os.environ["KONTRA_BUILD_REVISION" if kind == "build-revision" else "KONTRA_SOURCE_REVISION"] = "b" * 40
        elif kind == "rust":
            (self.root / "source.rs").write_text("// unreviewed Rust change\n")
        elif kind == "profile":
            path = self.root / "Cargo.toml"
            path.write_text(path.read_text().replace('lto = "thin"', 'lto = false'))
        elif kind == "lock":
            path = self.root / "Cargo.lock"
            path.write_text(path.read_text().replace('version = "1.0.0"', 'version = "1.0.1"'))
        elif kind in ("manifest-version", "lock-version"):
            path = self.root / ("Cargo.toml" if kind == "manifest-version" else "Cargo.lock")
            path.write_text(path.read_text().replace(self.stamp["version"], "0.1.0-nightly.wrong"))
        elif kind == "version-helper":
            (self.root / "tools/version.py").write_text("def nightly_dirty(): return False\n")
        else:
            raise AssertionError(kind)

    def cli_fixture(self):
        metadata = fixture(self.root)
        metadata["packages"][0]["version"] = self.stamp["version"]
        plan, opaque = prepared(metadata, stamp=self.stamp, source=self.source)
        for name, value in (("plan.json", plan), ("metadata.json", metadata), ("features.json", contract_fixture())):
            shipping.write(self.root / name, value)
        shard = plan["shards"][0]
        receipt = self.root / "receipt.json"
        argv = ["shipping_tests.py", "run", "--source", self.source, "--plan", str(self.root / "plan.json"),
                "--metadata", str(self.root / "metadata.json"), "--crates", CRATES,
                "--feature-contract", str(self.root / "features.json"), "--shard", "0", "--shard-id", shard["id"],
                "--receipt", str(receipt), "--log", str(self.root / "run.log")]
        output = "\n".join(map(json.dumps, messages(plan, shard, opaque, self.root))) + "\n"
        return argv, receipt, output, plan

    def test_exact_stamp_permitted_and_binds_actual_version_sha_epoch_and_bytes(self):
        self.assertEqual(self.stamp["version"], "0.1.0-nightly.20231114.g" + self.source[:12])
        self.assertEqual(self.stamp["source_date_epoch"], int(self.epoch))
        for filename, key in (("Cargo.toml", "cargo_toml_sha256"), ("Cargo.lock", "cargo_lock_sha256")):
            self.assertEqual(self.stamp[key], hashlib.sha256((self.root / filename).read_bytes()).hexdigest())
        argv, receipt, output, plan = self.cli_fixture()
        def execute(command, log):
            log.write(output)
            return 0
        with patch.object(sys, "argv", argv), patch.object(shipping, "execute", side_effect=execute), \
             patch("sys.stdout", new_callable=io.StringIO):
            shipping.main()
        accepted = shipping.load(receipt)
        self.assertEqual(accepted["source"], self.source)
        self.assertEqual(accepted["stamp"], self.stamp)
        self.assertEqual(accepted["feature_contract_id"], plan["feature_contract_id"])
        with self.assertRaises(shipping.Invalid):
            shipping.check_source("b" * 40)

    def test_mutations_refused_before_planning_and_before_cargo_without_receipt(self):
        argv, receipt, _, _ = self.cli_fixture()
        plan_argv = ["shipping_tests.py", "plan", "--source", self.source,
                     "--metadata", str(self.root / "metadata.json"), "--crates", CRATES,
                     "--feature-contract", str(self.root / "features.json"), "--plan", str(self.root / "new-plan.json")]
        for kind in ("rust", "profile", "lock", "manifest-version", "lock-version", "epoch",
                     "build-revision", "source-revision", "version-helper"):
            self.restore_fixture()
            self.mutate(kind)
            shipping.write(receipt, {"stale": "success"})
            with self.subTest(kind=kind), patch.object(sys, "argv", argv), \
                 patch.object(shipping, "execute") as execute:
                with self.assertRaises(shipping.Invalid):
                    shipping.main()
                execute.assert_not_called()
                self.assertFalse(receipt.exists())
            with patch.object(sys, "argv", plan_argv), self.assertRaises(shipping.Invalid):
                shipping.main()
            self.assertFalse((self.root / "new-plan.json").exists())

    def test_post_cargo_mutations_refused_without_success_receipt(self):
        argv, receipt, output, _ = self.cli_fixture()
        for kind in ("rust", "profile", "lock", "manifest-version", "lock-version", "epoch",
                     "build-revision", "source-revision", "version-helper"):
            self.restore_fixture()
            shipping.write(receipt, {"stale": "success"})
            def execute(command, log):
                log.write(output)
                self.mutate(kind)
                return 0
            with self.subTest(kind=kind), patch.object(sys, "argv", argv), \
                 patch.object(shipping, "execute", side_effect=execute), patch("sys.stdout", new_callable=io.StringIO):
                with self.assertRaises(shipping.Invalid):
                    shipping.main()
                self.assertFalse(receipt.exists())

    def test_external_contract_change_after_cargo_is_refused(self):
        argv, receipt, output, _ = self.cli_fixture()
        def execute(command, log):
            log.write(output)
            contract = contract_fixture()
            contract["packages"]["sampler-ksp"].append("scan")
            shipping.write(self.root / "features.json", contract)
            return 0
        with patch.object(sys, "argv", argv), patch.object(shipping, "execute", side_effect=execute), \
             patch("sys.stdout", new_callable=io.StringIO), self.assertRaises(shipping.Invalid):
            shipping.main()
        self.assertFalse(receipt.exists())

    def test_aggregate_verifies_stamp_identity_on_its_unstamped_checkout(self):
        _, _, _, plan = self.cli_fixture()
        receipts = self.root / "receipts"
        receipts.mkdir()
        for shard in plan["shards"]:
            shipping.write(receipts / ("receipt-" + str(shard["index"]) + ".json"),
                           shipping.receipt(plan, shard, actual_for(plan, shard)))
        for name in ("Cargo.toml", "Cargo.lock"):
            (self.root / name).write_text(self.base[name])
        os.environ.pop("SOURCE_DATE_EPOCH")
        argv = ["shipping_tests.py", "aggregate", "--source", self.source,
                "--plan", str(self.root / "plan.json"), "--receipts", str(receipts),
                "--feature-contract", str(self.root / "features.json")]
        with patch.object(sys, "argv", argv), patch("sys.stdout", new_callable=io.StringIO) as output:
            shipping.main()
        result = json.loads(output.getvalue())
        self.assertEqual(result["status"], "success")
        self.assertEqual(result["stamp"], self.stamp)


class Workflow(unittest.TestCase):
    def test_shipping_workflow_contract_and_ordinary_ci_preserved(self):
        ci = Path(__file__).resolve().parents[1].joinpath("workflows/ci.yml").read_text()
        def job(name, following):
            return ci.split("\n  " + name + ":\n", 1)[1].split("\n  " + following + ":\n", 1)[0]
        linux = job("linux", "shipping-plan")
        plan = job("shipping-plan", "shipping-tests")
        tests = job("shipping-tests", "shipping-coverage")
        coverage = job("shipping-coverage", "editor")
        self.assertIn('if: ${{ !inputs.release_validation }}', linux)
        self.assertIn('run: cargo test --locked --profile "$CI_PROFILE" $CRATES\n', linux)
        self.assertIn('shipping_tests.py units', linux)
        self.assertIn('--receipt shipping-unit-receipt.json --log shipping-unit.log', linux)
        self.assertIn('shipping_tests.py plan', linux)
        for shipping_job in (plan, tests):
            self.assertIn("if: inputs.release_validation && needs.changes.outputs.code == 'true'", shipping_job)
            self.assertIn('ref: ${{ github.sha }}', shipping_job)
            self.assertIn('cargo metadata --locked --no-deps --format-version 1', shipping_job)
            self.assertIn('python3 tools/version.py nightly --write', shipping_job)
            self.assertIn('toolchain: 1.99.0', shipping_job)
        for shipping_job in (plan, tests):
            self.assertIn('test "$(python3 tools/version.py nightly-dirty)" = false', shipping_job)
        self.assertIn('fail-fast: false', tests)
        self.assertIn('matrix: ${{ fromJSON(needs.shipping-plan.outputs.matrix) }}', tests)
        self.assertIn('timeout-minutes: 180', tests)
        self.assertIn('key: linux-release-integration-${{ matrix.shard }}', tests)
        self.assertIn('if-no-files-found: error', tests)
        self.assertIn('shipping_tests.py run', tests)
        self.assertIn('needs: [changes, shipping-plan, shipping-tests]', coverage)
        self.assertIn('test "$PLAN" = success\n          test "$TESTS" = success', coverage)
        self.assertIn('shipping_tests.py aggregate', coverage)
        # The package/feature selection is centralized; every invocation validates it.
        self.assertEqual(ci.count('CRATES: >-'), 1)
        self.assertNotIn('cargo test --examples', linux)
        self.assertIn('-p kontra-native-host --lib -- --ignored --test-threads=1', linux)
        self.assertIn('-p kontra-native-host --test x11_visibility -- --ignored --test-threads=1', linux)
        self.assertIn('cargo build --locked --profile "$CI_PROFILE" --lib', linux)


if __name__ == "__main__":
    unittest.main()
