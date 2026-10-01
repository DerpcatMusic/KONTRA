#!/usr/bin/env python3
"""Offline release safety check: python3 .github/workflows/check_nightly.py."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap

workflow = Path(__file__).with_name("nightly.yml").read_text()
publish_step, cleanup_step = workflow.split("      - name: Publish the nightly pre-release\n", 1)[1].split("      - name: Remove released Actions artifacts\n", 1)
publish = textwrap.dedent(publish_step.split("        run: |\n", 1)[1])
cleanup = textwrap.dedent(cleanup_step.split("        run: |\n", 1)[1])
platforms = ("linux-x86_64", "windows-x86_64", "macos-arm64", "macos-x86_64")
readme = Path(__file__).resolve().parents[2].joinpath("README.md").read_text()
for platform in platforms:
    assert f"name: {platform}" in workflow
    assert f"releases/download/nightly/KONTRA-nightly-{platform}.zip" in readme
assert "nightly-build-${{ matrix.name }}\n      cancel-in-progress: true" in workflow
assert "nightly-publish\n      cancel-in-progress: false" in workflow
assert "retention-days: 1" in workflow
assert "if: steps.publish.outputs.published == 'true'" in cleanup_step
assert "continue-on-error: true" in cleanup_step

mock_gh = '''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
path = Path("state.json")
state = json.loads(path.read_text())
args = sys.argv[1:]
state["calls"].append(args)
output, status = "", 0
if args[0] == "api":
    if "--method" in args:
        assert args[args.index("--method") + 1] == "DELETE" and state["promoted"]
        assert args[-1].startswith("repos/example/KONTRA/actions/artifacts/")
        state["deleted"].append(int(args[-1].rsplit("/", 1)[1]))
        status = 1 if state["case"] == "cleanup-fails" else 0
    elif any("/actions/runs/" in arg for arg in args):
        assert "repos/example/KONTRA/actions/runs/7/artifacts" in args and state["promoted"]
        assert 'startswith("KONTRA-nightly-")' in args[-1]
        output = "100\\n101\\n102\\n103"
    elif any("git/ref/heads/main" in arg for arg in args):
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

for case in ("current", "first", "leftover", "stale-before", "stale-after", "upload-fails", "missing-asset", "bad-checksum", "cleanup-fails"):
    with tempfile.TemporaryDirectory(prefix="kontra-nightly-check-") as directory:
        root = Path(directory)
        root.joinpath("gh").write_text(mock_gh)
        root.joinpath("gh").chmod(0o755)
        root.joinpath("dist").mkdir()
        for platform in platforms:
            if case != "missing-asset" or platform != platforms[-1]:
                filename = f"KONTRA-nightly-{platform}.zip"
                root.joinpath(f"dist/{filename}").write_bytes(b"archive")
                digest = hashlib.sha256(b"corrupt" if case == "bad-checksum" else b"archive").hexdigest()
                root.joinpath(f"dist/{filename}.sha256").write_text(f"{digest}  {filename}\n")
        initial = dict(case=case, nightly=case != "first", draft=case == "leftover", heads=0, uploaded=False, promoted=False, calls=[], deleted=[])
        root.joinpath("state.json").write_text(json.dumps(initial))
        output = root / "outputs"
        output.touch()
        env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}", GITHUB_SHA="a" * 40, GH_REPO="example/KONTRA", GITHUB_RUN_ID="7", GITHUB_OUTPUT=str(output))
        result = subprocess.run(["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", publish], cwd=root, env=env, capture_output=True, text=True)
        state = json.loads(root.joinpath("state.json").read_text())
        assert result.returncode == (1 if case in ("upload-fails", "missing-asset", "bad-checksum") else 0), (case, result.stderr)
        assert state["promoted"] == (case in ("current", "first", "leftover", "cleanup-fails")), case
        assert state["nightly"] and not state["draft"], (case, state)
        assert ("published=true" in output.read_text()) == state["promoted"], case
        if state["promoted"]:
            result = subprocess.run(["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", cleanup], cwd=root, env=env, capture_output=True, text=True)
            state = json.loads(root.joinpath("state.json").read_text())
            assert result.returncode == (1 if case == "cleanup-fails" else 0), (case, result.stderr)
            assert state["deleted"] == ([100] if case == "cleanup-fails" else [100, 101, 102, 103])
            assert state["nightly"] and state["promoted"] and not state["draft"]
        else:
            assert not state["deleted"]
print("Nightly checks passed: 9 release/cleanup scenarios and all four README asset links.")
