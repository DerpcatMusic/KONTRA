import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

CHECK = Path(__file__).with_name("check.py")
MUI = "dcf0796082feec053af1418e3a38a302ee61da0a"
MOOSE = "bffa4677d0b82119d38566ce7e932dc5c463d497"


class StackCheck(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=Path(__file__).parent)
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        (self.repo / "Cargo.toml").write_text(f'''[package]
name = "example"
version = "0.1.0"
[dependencies]
mui = {{ git = "https://github.com/Matari-Audio/MUI", rev = "{MUI}" }}
''')
        self.lock(f'''[[package]]
name = "mui"
version = "1.0.0"
source = "git+https://github.com/Matari-Audio/MUI?rev={MUI}#{MUI}"
''')

    def lock(self, content):
        (self.repo / "Cargo.lock").write_text('version = 4\n' + content)

    def run_check(self):
        result = subprocess.run([sys.executable, str(CHECK), str(self.repo)], text=True, capture_output=True)
        self.assertIn(result.returncode, (0, 1), result.stderr)
        return result.returncode, json.loads(result.stdout)["repositories"][0]

    def test_coherent_fixture_passes_and_does_not_mutate(self):
        before = {p.name: p.read_bytes() for p in self.repo.iterdir()}
        code, report = self.run_check()
        self.assertEqual(code, 0)
        self.assertEqual(report["issues"], [])
        self.assertEqual(report["resolved"][0]["package"], "mui")
        self.assertEqual(before, {p.name: p.read_bytes() for p in self.repo.iterdir()})

    def test_forks_sources_and_tool_revisions_fail(self):
        self.lock(f'''[[package]]
name = "kontra-native-host"
version = "0.1.0"
[[package]]
name = "moose-baseview"
version = "1.0.0"
source = "git+https://github.com/Matari-Audio/MUI?rev={MUI}#{MUI}"
[[package]]
name = "moose-baseview"
version = "1.0.0"
source = "git+https://github.com/Matari-Audio/moose?rev={MOOSE}#{MOOSE}"
''')
        with (self.repo / "Cargo.toml").open("a") as f:
            f.write('\n[patch."https://github.com/Matari-Audio/MUI"]\nmui = { path = "vendor/mui" }\n')
        workflows = self.repo / ".github" / "workflows"
        workflows.mkdir(parents=True)
        (workflows / "release.yml").write_text(f'''env:
  MOOSE_REV: {MOOSE}
jobs:
  build:
    steps:
      - run: cargo install --git https://github.com/Matari-Audio/moose --rev "$MOOSE_REV" cargo-moose
      - run: cargo install --git https://github.com/Matari-Audio/moose --rev {'a' * 40} cargo-moose
''')
        code, report = self.run_check()
        self.assertEqual(code, 1)
        kinds = [issue["kind"] for issue in report["issues"]]
        for kind in ("custom-alias", "unregistered-source", "multiple-sources", "local-patch", "tooling-drift"):
            self.assertIn(kind, kinds)
        self.assertEqual([t["revision"] for t in report["tooling"]], [MOOSE, 'a' * 40])
        self.assertEqual(len([i for i in report["issues"] if i["kind"] == "tooling-drift"]), 1)

    def test_unpinned_declaration_and_unknown_source_fail(self):
        (self.repo / "Cargo.toml").write_text('[dependencies]\nmui = { git = "https://github.com/Matari-Audio/MUI" }\n')
        self.lock('''[[package]]
name = "mui"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
''')
        code, report = self.run_check()
        self.assertEqual(code, 1)
        self.assertIn("declaration-drift", [i["kind"] for i in report["issues"]])
        self.assertIn("unregistered-source", [i["kind"] for i in report["issues"]])


if __name__ == "__main__":
    unittest.main()
