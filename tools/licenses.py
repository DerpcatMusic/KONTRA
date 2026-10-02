#!/usr/bin/env python3
"""Bundle dependency notices and exact MPL source archives; never grant ni-file rights."""
import argparse
import gzip
import os
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import tarfile

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


def bundle_source(package, sources):
    manifest = Path(package["manifest_path"]).resolve()
    filename = f'{package["name"]}-{package["version"]}.crate'
    if manifest == ROOT / "vendor/symphonia-format-riff/Cargo.toml":
        # Ship the patched source compiled into the binary, not an upstream copy.
        epoch = int(os.environ.get("SOURCE_DATE_EPOCH", "0"))
        def normalize(member):
            member.mtime = epoch
            member.uid = member.gid = 0
            member.uname = member.gname = ""
            member.mode = 0o755 if member.isdir() else 0o644
            member.pax_headers = {}
            return member
        with (sources / filename).open("wb") as raw:
            with gzip.GzipFile(fileobj=raw, filename="", mode="wb", mtime=epoch) as compressed:
                with tarfile.open(fileobj=compressed, mode="w") as archive:
                    archive.add(manifest.parent, arcname=filename.removesuffix(".crate"), filter=normalize)
    else:
        registry = manifest.parents[3]
        archive = registry / "cache" / manifest.parents[1].name / filename
        shutil.copyfile(archive, sources / filename)


def generate(output):
    with tempfile.TemporaryDirectory(prefix="kontra-notices-") as directory:
        raw = Path(directory) / "licenses.json"
        subprocess.run(["cargo-about", "generate", "--locked", "--all-features", "--format", "json",
                        "--config", str(ROOT / "about.toml"), "--output-file", str(raw)],
                       cwd=ROOT, check=True)
        data = json.loads(raw.read_text(encoding="utf-8"))
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
        bundle_source(p, sources)
    (output / "THIRD_PARTY_NOTICES.txt").write_text(notices, encoding="utf-8")
    print(f'Bundled {len(data["crates"])} package entries; ni-file permission remains unresolved.')


def self_test():
    package = dict(id="test", name="test", version="1.0")
    data = dict(crates=[dict(package=package, license="MIT")],
                licenses=[dict(id="MIT", text="Copyright Example\nMIT license", used_by=[dict(crate=package)])])
    assert "Copyright Example" in render(data)
    # cargo-about emits UTF-8, even when Windows' default code page is CP1252.
    from unittest.mock import patch
    with tempfile.TemporaryDirectory(prefix="kontra-notices-encoding-") as directory:
        data["licenses"][0]["text"] = "Copyright Example \u201cUTF-8\u201d\nMIT license"
        def cargo_about(command, **kwargs):
            Path(command[command.index("--output-file") + 1]).write_bytes(
                json.dumps(data, ensure_ascii=False).encode("utf-8"))
        read_text = Path.read_text
        def windows_read(path, encoding=None, **kwargs):
            return read_text(path, encoding=encoding or "cp1252", **kwargs)
        output = Path(directory) / "licenses"
        with patch.object(subprocess, "run", cargo_about), patch.object(Path, "read_text", windows_read):
            generate(output)
        assert "\u201cUTF-8\u201d" in (output / "THIRD_PARTY_NOTICES.txt").read_text(encoding="utf-8")
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
    with tempfile.TemporaryDirectory(prefix="kontra-source-check-") as directory:
        root = Path(directory)
        output = root / "sources"
        output.mkdir()
        vendored = ROOT / "vendor/symphonia-format-riff"
        package = dict(name="symphonia-format-riff", version="0.5.5", manifest_path=str(vendored / "Cargo.toml"))
        bundle_source(package, output)
        with tarfile.open(output / "symphonia-format-riff-0.5.5.crate") as archive:
            for path in vendored.rglob("*"):
                if path.is_file():
                    member = "symphonia-format-riff-0.5.5/" + path.relative_to(vendored).as_posix()
                    assert archive.extractfile(member).read() == path.read_bytes()
        # Independently checked-out architectures must ship identical sources,
        # even when checkout times and filesystem ownership/modes differ.
        copies = []
        with patch.dict(os.environ, {"SOURCE_DATE_EPOCH": "1790962911"}):
            for index in (1, 2):
                # Exercise a noncanonical temporary path, as macOS /var does.
                checkout = root / ".." / root.name / f"checkout-{index}"
                vendor = checkout / "vendor/symphonia-format-riff"
                vendor.mkdir(parents=True)
                for name in ("Cargo.toml", "lib.rs"):
                    path = vendor / name
                    path.write_bytes(b"authored deterministic source\n")
                    os.utime(path, (index * 100, index * 100))
                    path.chmod(0o600 if index == 1 else 0o644)
                with patch.dict(globals(), {"ROOT": checkout.resolve()}):
                    bundle_source(dict(package, manifest_path=str(vendor / "Cargo.toml")), output)
                copies.append((output / "symphonia-format-riff-0.5.5.crate").read_bytes())
        assert copies[0] == copies[1], "Source archive depends on architecture checkout metadata"
        with tarfile.open(output / "symphonia-format-riff-0.5.5.crate") as archive:
            assert all(m.mtime == 1790962911 and m.uid == m.gid == 0 and not m.uname and not m.gname for m in archive)
        # Registry packages keep their exact upstream archive bytes.
        manifest = root / "registry/src/example-index/example-1.0/Cargo.toml"
        manifest.parent.mkdir(parents=True)
        source = root / "registry/cache/example-index/example-1.0.crate"
        source.parent.mkdir(parents=True)
        source.write_bytes(b"authored registry archive")
        bundle_source(dict(name="example", version="1.0", manifest_path=str(manifest)), output)
        assert (output / source.name).read_bytes() == source.read_bytes()
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
