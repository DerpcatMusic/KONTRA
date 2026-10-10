"""Offline static custody checks. Needs only this sealed directory and pinned Git objects."""
import json
import re
from collect import BASE, TIP, HERE, PACKET, data, diff, delta, git, pin, replay, sha, tree

plan = json.loads((HERE / "source-ready.json").read_bytes())
assert plan["base"]["commit"] == BASE and plan["candidate"]["commit"] == TIP
for commit in [BASE, TIP]:
    assert git("rev-parse", commit+"^{tree}").decode().strip() == (
        plan["base"] if commit == BASE else plan["candidate"])["tree"]
for item in plan["immutable_inputs"]:
    body = data(item["commit"], item["path"]) if "commit" in item else (HERE/item["path"]).read_bytes()
    assert sha(body) == item.get("sha256", item.get("file_sha256"))
for item in plan["primary_spec_only"]:
    assert sha(item["excerpt"].encode()) == item["excerpt_sha256"]
for item in plan["original_cherries"]:
    assert delta(diff(item["original"])) == delta(diff(item["assembled"]))
for item in plan["ordered_stack"]:
    assert sha(diff(item["commit"])) == item["patch_sha256"]
    assert git("rev-parse", item["commit"]+"^").decode().strip() == item["parent"]
    for path in item["paths"]:
        replay(item["commit"], path)
for item in plan["changed_files"]:
    assert pin(TIP, item["path"])["file_sha256"] == item["file_sha256"]
for item in plan["caller_trace"]:
    e = item["evidence"]
    assert pin(TIP, e["path"], *e["lines"]) == e
    # All these production callers are unchanged by the three-fix stack.
    assert pin(BASE, e["path"])["blob"] == e["blob"]
for item in plan["tests"]:
    e = item["evidence"]
    assert pin(TIP, e["path"], *e["lines"]) == e
    assert item["status"] == "NOT_RUN_NOT_COMPILED"

# Independently compare source-declared test names, not an executed harness list.
new_names = set()
for path in ["crates/sampler-ksp/tests/compile.rs", "crates/sampler-ksp/tests/functions.rs",
             "crates/sampler-ksp/tests/strings.rs", "crates/sampler-ksp/src/lower_display_ext_tests.rs"]:
    def names(commit):
        body = data(commit, path) if path in tree(commit) else b""
        return set(re.findall(rb"#\[test\]\s*fn (\w+)\(", body))
    added = names(TIP) - names(BASE)
    new_names.update(n.decode() for n in added)
assert len(new_names) == 13
legacy = "nested_function_branches_loops_and_exit_keep_polyphonic_state_and_caller_targets"
assert {t["filter"].split("::")[-1] for t in plan["tests"]} == new_names | {legacy}

# Authenticate all original committed source/test spans and all review table rows.
manifest = json.loads(data(TIP, PACKET))
for e in manifest["source_spans"] + [t["evidence"] for t in manifest["tests"]]:
    actual = pin(e["commit"], e["path"], e["start_line"], e["end_line"])
    assert actual["blob"] == e["git_blob"]
    assert actual["file_sha256"] == e["file_sha256"]
    assert actual["span_sha256"] == e["span_sha256"]
count = 0
review = (HERE / "review-snapshot.md").read_text()
pattern = r"\| (?:`([a-f0-9]{40})` \| )?`([^`]+):(\d+)–(\d+)` \| `([a-f0-9]{40})` \| `([a-f0-9]{64})` \|"
for commit, path, start, end, blob, digest in re.findall(pattern, review):
    actual = pin(commit or TIP, path, int(start), int(end))
    assert actual["blob"] == blob
    assert actual["span_sha256"] == digest
    count += 1
assert count == 120, count
assert "SOURCE_REVIEW_CLEAR_ONLY" in review and "COMPOSED-EXT-TEST-001 / F1" in review
assert "RETIRED_IN_SOURCE_ONLY" in review
assert "NOT_RUN_NOT_COMPILED" in review
print(json.dumps({"static_custody": "VERIFIED_ONLY", "review_table_spans": count,
                  "committed_manifest_spans": 55, "new_test_declarations": len(new_names),
                  "mapped_tests": 14, "Rust_KSP_native_execution": "NONE"}))
