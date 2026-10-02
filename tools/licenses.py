#!/usr/bin/env python3
"""Bundle dependency notices and exact MPL source archives; never grant ni-file rights."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def render(data):
    covered = {u["crate"]["id"] for license in data["licenses"]
               if license["text"].strip() for u in license["used_by"]}
    lines = ["KONTRA dependency notices", "Generated with cargo-about 0.9.2, Cargo.lock, all features/platforms.",
             "Includes build/development dependencies; not a per-binary inventory.",
             "ni-file 0.0.1: NO EXPLICIT REDISTRIBUTION LICENSE FOUND.",
             "This bundle does not resolve that permission or the issues in docs/LEGAL.md.",
             "MOOSE: full custom license and notices in licenses/MOOSE/.",
             "MUI: upstream copyright and license in licenses/MUI/LICENSE.",
             "Font: assets/OFL.txt. MPL covered source: licenses/sources/*.crate.", "", "Package inventory:"]
    for crate in data["crates"]:
        p = crate["package"]
        known_gap = ((p["name"], p["version"], crate["license"]) == ("ni-file", "0.0.1", "Unknown")
                     or p["name"].startswith("moose") and crate["license"] == "LicenseRef-TruceLicense-1.0")
        if p["id"] not in covered and not known_gap:
            raise ValueError(f'Missing license text: {p["name"]} {p["version"]} ({crate["license"]})')
        lines.append(f'{p["name"]} {p["version"]}: {crate["license"]}')
    for license in data["licenses"]:
        lines.extend(["", "=" * 72, license["id"], "Used by: " + ", ".join(
            f'{u["crate"]["name"]} {u["crate"]["version"]}' for u in license["used_by"]),
            license["text"]])
    return "\n".join(lines) + "\n"


def generate(output):
    with tempfile.TemporaryDirectory(prefix="kontra-notices-") as directory:
        raw = Path(directory) / "licenses.json"
        subprocess.run(["cargo-about", "generate", "--locked", "--all-features", "--format", "json",
                        "--config", str(ROOT / "about.toml"), "--output-file", str(raw)],
                       cwd=ROOT, check=True)
        data = json.loads(raw.read_text())
    notices = render(data)  # Validate before creating output.
    output.mkdir(parents=True, exist_ok=True)
    for name in ("MOOSE", "MUI"):
        shutil.copytree(ROOT / "licenses" / name, output / name, dirs_exist_ok=True)
    sources = output / "sources"
    sources.mkdir(exist_ok=True)
    for crate in data["crates"]:
        p = crate["package"]
        if "MPL-2.0" not in crate["license"]:
            continue
        manifest = Path(p["manifest_path"])
        registry = manifest.parents[3]
        archive = registry / "cache" / manifest.parents[1].name / f'{p["name"]}-{p["version"]}.crate'
        shutil.copyfile(archive, sources / archive.name)
    (output / "THIRD_PARTY_NOTICES.txt").write_text(notices, encoding="utf-8")
    print(f'Bundled {len(data["crates"])} package entries; ni-file permission remains unresolved.')


def self_test():
    package = dict(id="test", name="test", version="1.0")
    data = dict(crates=[dict(package=package, license="MIT")],
                licenses=[dict(id="MIT", text="Copyright Example\nMIT license", used_by=[dict(crate=package)])])
    assert "Copyright Example" in render(data)
    data["licenses"] = []
    try:
        render(data)
    except ValueError:
        pass
    else:
        raise AssertionError("An unbundled dependency must fail")
    package.update(name="ni-file", version="0.0.1")
    data["crates"][0]["license"] = "Unknown"
    assert "NO EXPLICIT REDISTRIBUTION LICENSE FOUND" in render(data)
    package["name"] = "new-unlicensed-dependency"
    try:
        render(data)
    except ValueError:
        pass
    else:
        raise AssertionError("Only the documented unresolved parser is exempt")
    print("License bundle checks passed")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
    elif args.output:
        generate(args.output)
    else:
        parser.error("provide --output DIRECTORY or --self-test")
