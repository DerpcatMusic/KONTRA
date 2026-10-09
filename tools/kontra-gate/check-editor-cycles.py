#!/usr/bin/env python3
"""Check retained editor bytes and closed RSS across a four-cycle host receipt."""
import json
import sys
from pathlib import Path

receipt = json.loads(Path(sys.argv[1]).read_text())
assert receipt["status"] == "RAW_MEASURED"
assert receipt["native_selection_verified"] and receipt["viewport_agreement_verified"]
owners = [row for row in receipt["owner_checkpoints"] if row["phase"].startswith("closed")]
assert [row["phase"] for row in owners] == [f"closed{i}" for i in range(4)]
assert len({row["threads"] for row in owners}) == 1
live = [row["allocated_bytes"] for row in owners]
rss = [receipt["rss"][row["phase"]]["rss_mib"] for row in owners]
result = {"live_growth_mib": (max(live) - min(live)) / 1048576,
          "closed_rss_spread_mib": max(rss) - min(rss), "closed_rss_mib": rss}
print(json.dumps(result), flush=True)
assert result["live_growth_mib"] <= 1, result
assert result["closed_rss_spread_mib"] <= 5, result
