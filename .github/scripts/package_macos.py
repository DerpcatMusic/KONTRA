#!/usr/bin/env python3
"""Compose two same-source macOS stages into a universal installer payload.

universal-apple-darwin identifies this package, never a Rust compilation target.
Apple signing/notarization is performed separately by package_macos.sh.
"""
import argparse
import json
from pathlib import Path
import plistlib
import re
import shutil
import subprocess

TARGETS = {"arm64": "aarch64-apple-darwin", "x86_64": "x86_64-apple-darwin"}
FORMATS = ("clap", "vst3", "standalone")
BINARIES = {"clap": "KONTRA.clap/Contents/MacOS/KONTRA", "vst3": "KONTRA.vst3/Contents/MacOS/KONTRA", "standalone": "kontakto-standalone"}
MANIFESTS = {"clap": "clap-build-info.json", "vst3": "vst3-build-info.json", "standalone": "build-info.json"}
BUNDLES = {"clap": "Library/Audio/Plug-Ins/CLAP/KONTRA.clap", "vst3": "Library/Audio/Plug-Ins/VST3/KONTRA.vst3", "standalone": "Applications/KONTRA.app"}
REQUIRED = ("LICENSE", "NOTICE", "THIRD_PARTY.md", "assets/OFL.txt", "docs/LEGAL.md")


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def inventory(root):
    result = {}
    for path in sorted(root.rglob("*")):
        require(not path.is_symlink(), f"Symlink in input stage: {path}")
        name = path.relative_to(root).as_posix()
        if path.is_file() and "_CodeSignature" not in path.parts:
            result[name] = path.read_bytes()
    return result


def compose(arm64, x86_64, stage, revision, version, lipo="lipo"):
    require(bool(re.fullmatch(r"[0-9a-f]{40}", revision)), "Expected a full source revision")
    require(bool(re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version)), "Invalid package version")
    require(not stage.exists(), f"Output stage already exists: {stage}")
    sources = {"arm64": arm64, "x86_64": x86_64}
    inventories, manifests = {}, {}
    for arch, root in sources.items():
        files = inventory(root)
        require(files.get("SOURCE_COMMIT.txt", b"").decode().strip() == revision, f"{arch}: source revision mismatch")
        for name in REQUIRED:
            require(name in files and files[name], f"{arch}: missing legal resource {name}")
        require(any(n.startswith("licenses/") for n in files), f"{arch}: missing dependency license bundle")
        manifests[arch] = {}
        for fmt in FORMATS:
            info = json.loads(files[MANIFESTS[fmt]])
            require(info.get("revision") == revision and info.get("version") == version
                    and info.get("target") == TARGETS[arch] and info.get("profile") == "release",
                    f"{arch}/{fmt}: build identity mismatch")
            features = set(info.get("features", []))
            require({"plugin", "library-access", fmt} <= features, f"{arch}/{fmt}: missing required features")
            if fmt != "standalone":
                require(features & set(FORMATS) == {fmt}, f"{arch}/{fmt}: mixed plugin formats")
            manifests[arch][fmt] = info
            binary = root / BINARIES[fmt]
            content = binary.read_bytes()
            for field in ("version", "revision", "target", "source_revision", "build_hash", "import_hash"):
                require(isinstance(info.get(field), str) and info[field].encode() in content,
                        f"{arch}/{fmt}: binary lacks its declared {field}")
            actual = subprocess.check_output([lipo, "-archs", str(binary)], text=True).split()
            require(actual == [arch], f"{arch}/{fmt}: expected one {arch} slice, found {actual}")
            if fmt != "standalone":
                plist = plistlib.loads(files[f"KONTRA.{fmt}/Contents/Info.plist"])
                require(plist.get("CFBundleExecutable") == "KONTRA" and plist.get("CFBundleIdentifier")
                        and plist.get("CFBundlePackageType") == "BNDL", f"{arch}/{fmt}: invalid bundle identity/type")
                require(plist.get("KONTRAVersion") == version
                        and plist.get("CFBundleVersion") == plist.get("CFBundleShortVersionString") == version.split("-", 1)[0],
                        f"{arch}/{fmt}: bundle version mismatch")
        # Signatures and prior distribution containers are not payload resources.
        inventories[arch] = {n: data for n, data in files.items() if n not in set(BINARIES.values()) | set(MANIFESTS.values()) | {"KONTRA.dmg", "notarization.json"}}
    require(inventories["arm64"] == inventories["x86_64"], "Architecture stages have different resources or bundle metadata")
    for fmt in FORMATS:
        a, b = (dict(manifests[arch][fmt]) for arch in TARGETS)
        a.pop("target"); b.pop("target")
        require(a == b, f"{fmt}: architecture build metadata differs beyond target")
    # No embedded code may escape the explicit signing list.
    mach_magic = {bytes.fromhex(h) for h in ("feedface", "cefaedfe", "feedfacf", "cffaedfe", "cafebabe", "bebafeca", "cafebabf", "bfbafeca")}
    for name, data in inventories["arm64"].items():
        require(data[:4] not in mach_magic, f"Unlisted Mach-O resource: {name}")
    stage.mkdir(parents=True)
    payload = stage / "payload"
    for fmt, destination in BUNDLES.items():
        bundle = payload / destination
        if fmt != "standalone":
            shutil.copytree(x86_64 / f"KONTRA.{fmt}", bundle, ignore=shutil.ignore_patterns("_CodeSignature"))
        else:
            (bundle / "Contents/MacOS").mkdir(parents=True)
            (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(dict(
                CFBundleExecutable="KONTRA", CFBundleIdentifier="audio.matari.kontra.standalone",
                CFBundleName="KONTRA", CFBundleDisplayName="KONTRA", CFBundlePackageType="APPL",
                CFBundleVersion=version.split("-", 1)[0], CFBundleShortVersionString=version.split("-", 1)[0],
                KONTRAVersion=version, NSHighResolutionCapable=True)))
        binary = bundle / "Contents/MacOS/KONTRA"
        subprocess.run([lipo, "-create", str(arm64 / BINARIES[fmt]), str(x86_64 / BINARIES[fmt]), "-output", str(binary)], check=True)
        require(set(subprocess.check_output([lipo, "-archs", str(binary)], text=True).split()) == set(TARGETS), f"{fmt}: universal composition failed")
        binary.chmod(0o755)
    identity = dict(version=version, revision=revision, target="universal-apple-darwin", architectures=list(TARGETS),
                    profile="release", features=manifests["arm64"]["standalone"]["features"], source_builds=manifests)
    (stage / "build-info.json").write_text(json.dumps(identity, indent=2) + "\n")
    resources = payload / BUNDLES["standalone"] / "Contents/Resources/KONTRA"
    resources.mkdir(parents=True)
    for name, data in inventories["arm64"].items():
        if name.startswith(("KONTRA.clap/", "KONTRA.vst3/")):
            continue
        destination = resources / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    shutil.copyfile(stage / "build-info.json", resources / "build-info.json")
    for arch in TARGETS:
        for fmt in FORMATS:
            destination = resources / "source-builds" / arch / MANIFESTS[fmt]
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(sources[arch] / MANIFESTS[fmt], destination)
    # Signing-keychain setup may set umask 077; installed resources must still
    # be readable by every plugin host and app user.
    executables = {payload / destination / "Contents/MacOS/KONTRA" for destination in BUNDLES.values()}
    for path in [payload, *payload.rglob("*")]:
        path.chmod(0o755 if path.is_dir() or path in executables else 0o644)
    return identity


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arm64", type=Path, required=True)
    parser.add_argument("--x86-64", type=Path, required=True)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    compose(args.arm64, args.x86_64, args.stage, args.revision, args.version)
