#!/usr/bin/env python3
"""Count raw NCKP names in memory for a single witness; never save resources."""
import json
import sys


def raw_nckp_names(root):
    names, leaves = set(), set()
    def walk(controls, parent=""):
        for control in controls:
            value = control.get("value", {})
            leaf = value.get("common", {}).get("id", "")
            path = f"{parent}_{leaf}" if parent else leaf
            names.add(path.casefold())
            leaves.add(leaf.casefold())
            walk(value.get("controls", []), path)
    walk(root["value"]["performanceView"].get("controls", []))
    return names, leaves


def nckp_stdin():
    # Decrypted resource bytes stay in the pipe/process; print counts only.
    count = int(sys.stdin.readline())
    sought = {sys.stdin.readline().strip()[1:].casefold() for _ in range(count)}
    root = json.load(sys.stdin)
    names, leaves = raw_nckp_names(root)
    strings = set()
    def visit(value):
        if isinstance(value, str):
            strings.add(value.lstrip("$%@").casefold())
        elif isinstance(value, dict):
            for item in value.values():
                visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)
    visit(root)
    print(json.dumps({"raw_control_paths": len(names), "sought": len(sought), "path_matches": len(sought & names), "leaf_matches": len(sought & leaves), "any_string_matches": len(sought & strings)}))


def main():
    if sys.argv[1:] == ["--self-check"]:
        root = {"value": {"performanceView": {"controls": [{"value": {"common": {"id": "Panel"}, "controls": [{"value": {"common": {"id": "Knob"}}}]}}]}}}
        assert raw_nckp_names(root) == ({"panel", "panel_knob"}, {"panel", "knob"})
        print("raw NCKP path check passed")
        return
    if sys.argv[1:] == ["--nckp-stdin"]:
        nckp_stdin()
        return
    raise SystemExit("usage: widget-audit.py --self-check | --nckp-stdin")


if __name__ == "__main__":
    main()
