#!/usr/bin/env python3
"""Offline release safety check: python3 .github/workflows/check_nightly.py."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap

workflow = Path(__file__).with_name("nightly.yml").read_text()
publish = textwrap.dedent(workflow.rsplit("        run: |\n", 1)[1])
platforms = ("linux-x86_64", "windows-x86_64", "macos-arm64", "macos-x86_64")
readme = Path(__file__).resolve().parents[2].joinpath("README.md").read_text()
for platform in platforms:
    assert f"name: {platform}" in workflow
    assert f"releases/download/nightly/KONTRA-nightly-{platform}.zip" in readme
assert "nightly-build-${{ matrix.name }}\n      cancel-in-progress: true" in workflow
assert "nightly-publish\n      cancel-in-progress: false" in workflow

mock_gh = '''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
path = Path("state.json")
state = json.loads(path.read_text())
args = sys.argv[1:]
state["calls"].append(args)
output, status = "", 0
if args[0] == "api":
    if any("git/ref/heads/main" in arg for arg in args):
        state["heads"] += 1
        stale = state["case"] == "stale-before" or (state["case"] == "stale-after" and state["heads"] == 2)
        output = "b" * 40 if stale else os.environ["GITHUB_SHA"]
    elif "nightly-staging" in args[-1]:
        output = "2" if state["draft"] else ""
    else:
        output = "1" if state["nightly"] else ""
elif args[:2] == ["release", "create"]:
    assert "--draft" in args and args[2] == "nightly-staging"
    assert state["nightly"] == (state["case"] != "first")
    state["draft"] = True
    status = 1 if state["case"] == "upload-fails" else 0
    state["uploaded"] = status == 0
elif args[:2] == ["release", "delete"]:
    if args[2] == "nightly":
        assert state["uploaded"] and state["heads"] == 2
        state["nightly"] = False
    else:
        state["draft"] = False
elif args[:2] == ["release", "edit"]:
    assert state["uploaded"] and not state["nightly"] and state["draft"]
    assert args[args.index("--tag") + 1] == "nightly"
    assert args[args.index("--target") + 1] == os.environ["GITHUB_SHA"]
    assert "--draft=false" in args
    state["nightly"], state["draft"], state["promoted"] = True, False, True
else:
    raise AssertionError(args)
path.write_text(json.dumps(state))
print(output)
sys.exit(status)
'''

for case in ("current", "first", "leftover", "stale-before", "stale-after", "upload-fails", "missing-asset"):
    with tempfile.TemporaryDirectory(prefix="kontra-nightly-check-") as directory:
        root = Path(directory)
        root.joinpath("gh").write_text(mock_gh)
        root.joinpath("gh").chmod(0o755)
        root.joinpath("dist").mkdir()
        for platform in platforms:
            if case != "missing-asset" or platform != platforms[-1]:
                root.joinpath(f"dist/KONTRA-nightly-{platform}.zip").write_bytes(b"archive")
        initial = dict(case=case, nightly=case != "first", draft=case == "leftover", heads=0, uploaded=False, promoted=False, calls=[])
        root.joinpath("state.json").write_text(json.dumps(initial))
        env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}", GITHUB_SHA="a" * 40, GH_REPO="example/KONTRA")
        result = subprocess.run(["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", publish], cwd=root, env=env, capture_output=True, text=True)
        state = json.loads(root.joinpath("state.json").read_text())
        assert result.returncode == (1 if case in ("upload-fails", "missing-asset") else 0), (case, result.stderr)
        assert state["promoted"] == (case in ("current", "first", "leftover")), case
        assert state["nightly"] and not state["draft"], (case, state)
print("Nightly checks passed: 7 release scenarios and all four README asset links.")
