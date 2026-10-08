"""Verify a sidecar worker witness against a deliberately different note plan."""
import json
import sys

result = json.loads(open(sys.argv[1]).read())
plan = json.loads(open(sys.argv[2]).read())["programs"]["0"]
expected = [plan["key"], plan["velocity"]]
assert result["native_preferred_note"] != expected, "use a plan distinct from native preference"
assert result["pick"] == result["programs"][0]["pick"] == expected
assert result["pick_source"] == result["programs"][0]["pick_source"] == "shared-note-plan"
assert result["loads"] == result["plays_note"] == "yes"
assert result["nonfinite"] == 0 and result["peak"] > 1e-5
assert result["ui"] == "original-ok"
assert result["programs"][0]["views"] == result["views"]
assert result["programs"][0]["sample_resident_bytes"] == result["sample_resident_bytes"]
print("UVI production worker honours shared note plan; Original paint and finite PCM pass")
