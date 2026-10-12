#!/usr/bin/env python3
"""Publish complete nightly snapshots, keeping the newest and one rollback."""
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import sys
import tomllib
import zipfile
from release_notes import generate as release_notes

REPO = os.environ["GH_REPO"]
SHA = os.environ["GITHUB_SHA"]
PLATFORMS = ("linux-x86_64", "windows-x86_64", "macos-arm64", "macos-x86_64")
# Per-arch Mac ZIPs are validated as installer inputs, never uploaded.
PUBLISHED = ("linux-x86_64", "windows-x86_64")


def gh(*args):
    return subprocess.check_output(["gh", *args], stderr=subprocess.PIPE)


def api(path, *args):
    return json.loads(gh("api", f"repos/{REPO}/{path}", *args))


def releases():
    return [r for page in api("releases", "--paginate", "--slurp") for r in page]


def managed_ref(tag):
    return re.fullmatch(r"nightly(?:-previous|-staging)?|legacy-g[0-9a-f]{12}|v\d+\.\d+\.\d+-nightly\.\d{8}\.g[0-9a-f]{12}", tag) is not None


def delete_ref(tag):
    assert managed_ref(tag), "Stable source tags must be preserved"
    # Missing refs can return 422 on DELETE. Prefix matches are not this tag.
    if not any(r["ref"] == "refs/tags/" + tag for r in api("git/matching-refs/tags/" + tag)):
        return
    result = subprocess.run(["gh", "api", f"repos/{REPO}/git/refs/tags/{tag}", "--method", "DELETE"], capture_output=True)
    if result.returncode and b"HTTP 404" not in result.stderr:
        print(result.stderr.decode(), end="")
        raise subprocess.CalledProcessError(result.returncode, result.args, result.stdout, result.stderr)


def verify(release, manifest, manifest_bytes):
    assert release["target_commitish"] == manifest["revision"]
    assert release["name"] == f'KONTRA {manifest["version"]}'
    names = {f"KONTRA-nightly-{p}.zip" for p in PUBLISHED}
    if any(a["name"].endswith(".pkg") for a in manifest["assets"]):
        names |= {"KONTRA-nightly-macos-universal.pkg", "KONTRA-nightly-macos-universal.notarization.json"}
    # Older snapshots also carried per-arch Mac ZIPs.
    assert names <= {a["name"] for a in manifest["assets"]}
    expected = {a["name"]: a for a in manifest["assets"]}
    expected["release-manifest.json"] = dict(size=len(manifest_bytes), sha256=hashlib.sha256(manifest_bytes).hexdigest())
    assert set(expected) == {a["name"] for a in release["assets"]}, "Incomplete release assets"
    for a in release["assets"]:
        e = expected[a["name"]]
        assert a["state"] == "uploaded" and a["size"] == e["size"] and a["digest"] == f'sha256:{e["sha256"]}', a["name"]


def source_tag(tag, revision):
    assert re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", revision), revision
    refs = api("git/matching-refs/tags/" + tag)
    exact = [r for r in refs if r["ref"] == "refs/tags/" + tag]
    if exact:
        obj = exact[0]["object"]
        while obj.get("type") == "tag":
            obj = api("git/tags/" + obj["sha"])["object"]
        assert obj["sha"] == revision, "Source tags must never move"
    else:
        assert revision == SHA == api("git/ref/heads/main")["object"]["sha"], "Never create historical source refs"
        api("git/refs", "--method", "POST", "-f", "ref=refs/tags/" + tag, "-f", "sha=" + revision)


def finish(release, manifest, manifest_bytes):
    verify(release, manifest, manifest_bytes)
    assert not release["draft"]
    tag = f'v{manifest["version"]}'
    assert re.fullmatch(r"v\d+\.\d+\.\d+-nightly\.\d{8}\.g[0-9a-f]{12}", tag), tag
    if release["tag_name"] == "nightly-staging":
        # One-time migration: never ask GITHUB_TOKEN to retag historical commits.
        assert manifest["revision"] == SHA == api("git/ref/heads/main")["object"]["sha"], "Recover staging while its source is still main"
        source_tag(tag, SHA)
        gh("release", "edit", "nightly-staging", "--tag", tag, "--target", SHA, "--prerelease=false", "--latest=true")
        delete_ref("nightly-staging")
        release = next(r for r in releases() if r["tag_name"] == tag)
    assert release["tag_name"] == tag and not release["prerelease"]
    verify(release, manifest, manifest_bytes)
    source_tag(tag, manifest["revision"])
    assert api("releases/latest")["id"] == release["id"], "Complete snapshot must own Latest downloads"
    old = [r for r in releases() if not r["draft"] and r["id"] != release["id"]]
    previous = max(old, key=lambda r: r["published_at"], default=None)
    if previous:
        assets = {a["name"]: a for a in previous["assets"]}
        for platform in PUBLISHED:
            asset = assets[f"KONTRA-nightly-{platform}.zip"]
            assert asset["state"] == "uploaded" and asset["size"] > 0 and re.fullmatch(r"sha256:[0-9a-f]{64}", asset["digest"])
        if "release-manifest.json" in assets:
            old_data = gh("release", "download", previous["tag_name"], "--pattern", "release-manifest.json", "--output", "-")
            verify(previous, json.loads(old_data), old_data)
    # Old records/refs are never renamed or recreated. Delete only after validation.
    for r in old:
        if previous and r["id"] == previous["id"]:
            continue
        gh("release", "delete", r["tag_name"], "--yes", *(["--cleanup-tag"] if managed_ref(r["tag_name"]) else []))
        marker = re.search(r"<!-- kontra-source-tag: (v[\w.\-]+|legacy-g[0-9a-f]{12}) -->", r.get("body") or "")
        if marker and managed_ref(marker[1]) and marker[1] != tag and (not previous or marker[0] not in (previous.get("body") or "")):
            delete_ref(marker[1])


def main():
    files = [Path(f"dist/KONTRA-nightly-{p}.zip") for p in PLATFORMS]
    assert all(p.is_file() and p.stat().st_size for p in files), "Missing platform archive"
    current = releases()
    staging = next((r for r in current if r["tag_name"] == "nightly-staging"), None)
    if staging and not staging["draft"]:
        data = gh("release", "download", "nightly-staging", "--pattern", "release-manifest.json", "--output", "-")
        finish(staging, json.loads(data), data)
    if api("git/ref/heads/main")["object"]["sha"] != SHA:
        print("Superseded by newer main; keeping published downloads.")
        return
    version = tomllib.loads(Path("Cargo.toml").read_text())["package"]["version"]
    assert re.fullmatch(r"\d+\.\d+\.\d+-nightly\.\d{8}\.g[0-9a-f]{12}", version), version
    tag = "v" + version
    # A terminated upload may leave an older versioned draft; only managed drafts.
    for draft in current:
        if draft["draft"] and managed_ref(draft["tag_name"]):
            gh("release", "delete", draft["tag_name"], "--yes", "--cleanup-tag")
    # Recover publication/pruning without editing the existing immutable record.
    existing = next((r for r in releases() if r["tag_name"] == tag), None)
    if existing and not existing["draft"]:
        data = gh("release", "download", tag, "--pattern", "release-manifest.json", "--output", "-")
        finish(existing, json.loads(data), data)
        Path(os.environ["GITHUB_OUTPUT"]).write_text("published=true\n")
        return
    delete_ref("nightly-staging")
    assets, mac_builds = [], []
    for p, platform in zip(files, PLATFORMS):
        notarization = None
        digest = hashlib.sha256(p.read_bytes()).hexdigest()
        assert p.with_suffix(".zip.sha256").read_text().strip() == digest + "  " + p.name, "Archive checksum mismatch: " + p.name
        with zipfile.ZipFile(p) as archive:
            assert archive.testzip() is None, p.name
            prefix = f"KONTRA-nightly-{platform}/"
            if platform.startswith("macos-"):
                binaries = ("KONTRA.clap/Contents/MacOS/KONTRA", "KONTRA.vst3/Contents/MacOS/KONTRA", "KONTRA.app/Contents/MacOS/KONTRA")
            elif platform.startswith("windows-"):
                binaries = ("KONTRA.clap", "KONTRA.vst3/Contents/x86_64-win/KONTRA.vst3", "kontakto-standalone.exe")
            else:
                binaries = ("KONTRA.clap", "KONTRA.vst3/Contents/x86_64-linux/KONTRA.so", "kontakto-standalone")
            for name in (*binaries, "LICENSE", "NOTICE", "THIRD_PARTY.md", "assets/OFL.txt",
                         "docs/LEGAL.md", "licenses/THIRD_PARTY_NOTICES.txt", "licenses/MUI/LICENSE",
                         "licenses/MOOSE/LICENSE", "licenses/MOOSE/LICENSE-MIT",
                         "licenses/MOOSE/LICENSE-APACHE", "licenses/MOOSE/NOTICE"):
                assert archive.getinfo(prefix + name).file_size > 0, (p.name, name)
            notices = archive.read(prefix + "licenses/THIRD_PARTY_NOTICES.txt").decode()
            sources = re.findall(r"^(\S+) (\S+): .*MPL-2\.0.*$", notices, re.MULTILINE)
            assert sources, "Missing MPL inventory"
            for name, source_version in sources:
                assert archive.getinfo(prefix + f"licenses/sources/{name}-{source_version}.crate").file_size > 0, "Missing MPL source"
            assert archive.read(prefix + "SOURCE_COMMIT.txt").decode().strip() == SHA, p.name
            info = [json.loads(archive.read(prefix + name)) for name in ("clap-build-info.json", "vst3-build-info.json", "build-info.json")]
            if platform.startswith("macos-"):
                for bundle in ("KONTRA.clap", "KONTRA.vst3", "KONTRA.app"):
                    plist = plistlib.loads(archive.read(prefix + bundle + "/Contents/Info.plist"))
                    assert plist["CFBundleShortVersionString"] == plist["CFBundleVersion"] == version.split("-", 1)[0]
                    assert plist["KONTRAVersion"] == version
                app = plistlib.loads(archive.read(prefix + "KONTRA.app/Contents/Info.plist"))
                assert app["CFBundlePackageType"] == "APPL"
                assert {t["UTTypeIdentifier"] for t in app["UTImportedTypeDeclarations"]} == {"org.cleveraudio.clap", "com.steinberg.vst3"}
                assert all("com.apple.package" in t["UTTypeConformsTo"] for t in app["UTImportedTypeDeclarations"])
                notarization = json.loads(archive.read(prefix + "notarization.json"))
                assert notarization["status"] == "Accepted" and notarization["stapled"] is True and notarization["signatures_verified"] is True, "Untrusted Mac delivery"
                assert re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", notarization["id"]), "Missing Apple submission ID"
                assert set(notarization["sha256"]) == {"KONTRA.dmg", *binaries}
                for name, product_digest in notarization["sha256"].items():
                    assert hashlib.sha256(archive.read(prefix + name)).hexdigest() == product_digest, "Notarized product changed: " + name
                dmg = archive.read(prefix + "KONTRA.dmg")
                assert len(dmg) > 512 and dmg[-512:-508] == b"koly", "Missing notarized DMG"
        target = {"linux-x86_64": "x86_64-unknown-linux-gnu", "windows-x86_64": "x86_64-pc-windows-msvc", "macos-arm64": "aarch64-apple-darwin", "macos-x86_64": "x86_64-apple-darwin"}[platform]
        assert all(i["version"] == version and i["revision"] == SHA and i["target"] == target and i["profile"] == "release" and {"plugin", "library-access"} <= set(i["features"]) for i in info), p.name
        for i, format in zip(info[:2], ("clap", "vst3")):
            assert set(i["features"]) & {"clap", "vst3", "standalone"} == {format}, p.name
        assert {"clap", "vst3", "standalone"} <= set(info[2]["features"]), p.name
        asset = dict(name=p.name, platform=platform, size=p.stat().st_size, sha256=digest, clap_build=info[0], vst3_build=info[1], standalone_build=info[2])
        if notarization is not None:
            assert (notarization["version"], notarization["revision"], notarization["target"]) == (version, SHA, target), "Wrong notarized build identity"
            asset["notarization"] = notarization
        (mac_builds if platform.startswith("macos-") else assets).append(asset)
    package = Path("dist/KONTRA-nightly-macos-universal.pkg")
    receipt_file = package.with_suffix(".notarization.json")
    assert package.is_file() and receipt_file.is_file(), "Missing universal Mac installer"
    receipt = json.loads(receipt_file.read_text())
    assert package.read_bytes()[:4] == b"xar!", "Invalid Mac installer container"
    assert (receipt["version"], receipt["revision"], receipt["profile"]) == (version, SHA, "release"), "Wrong universal installer identity"
    assert receipt["target"] == "universal-apple-darwin" and receipt["architectures"] == ["arm64", "x86_64"]
    assert receipt["targets"] == ["aarch64-apple-darwin", "x86_64-apple-darwin"]
    assert receipt["status"] == "Accepted" and receipt["stapled"] is True and receipt["signatures_verified"] is True, "Untrusted universal installer"
    assert re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", receipt["id"]), "Missing installer Apple submission ID"
    assert set(receipt["products"]) == {"Library/Audio/Plug-Ins/CLAP/KONTRA.clap/Contents/MacOS/KONTRA", "Library/Audio/Plug-Ins/VST3/KONTRA.vst3/Contents/MacOS/KONTRA", "Applications/KONTRA.app/Contents/MacOS/KONTRA"}
    assert all(re.fullmatch(r"[0-9a-f]{64}", h) for h in receipt["products"].values())
    assert set(receipt["source_builds"]) == {"arm64", "x86_64"}
    for arch in ("arm64", "x86_64"):
        source = next(a for a in mac_builds if a["platform"] == "macos-" + arch)
        assert receipt["source_builds"][arch] == {"clap": source["clap_build"], "vst3": source["vst3_build"], "standalone": source["standalone_build"]}, "Installer source builds differ"
    assert hashlib.sha256(package.read_bytes()).hexdigest() == receipt["package_sha256"], "Universal installer checksum mismatch"
    for path in (package, receipt_file):
        assets.append(dict(name=path.name, platform="macos-universal", size=path.stat().st_size, sha256=hashlib.sha256(path.read_bytes()).hexdigest(), notarization=receipt))
    files = [p for p in files if not p.name.startswith("KONTRA-nightly-macos-")] + [package, receipt_file]
    previous = max((r for r in current if not r["draft"]), key=lambda r: r["published_at"], default=None)
    changelog = release_notes(api, REPO, SHA, version, previous)
    manifest = dict(version=version, revision=SHA, workflow_run=os.environ["GITHUB_RUN_ID"], assets=assets, changelog=changelog)
    data = (json.dumps(manifest, indent=2) + "\n").encode()
    Path("dist/release-manifest.json").write_bytes(data)
    Path("notes.md").write_text(f"""Automated **{version}** snapshot of [{SHA[:12]}](https://github.com/{REPO}/commit/{SHA}).
Source tag: [`v{version}`](https://github.com/{REPO}/tree/v{version}).
<!-- kontra-source-tag: v{version} -->

{changelog}

Every download comes from this source commit. `release-manifest.json` records their sizes and SHA256 checksums; each Linux/Windows archive includes separate `clap-build-info.json`, `vst3-build-info.json` and standalone `build-info.json` with their actual feature sets.
Experimental nightly snapshot, not a stable-quality release. GitHub marks it Latest solely to provide permanent download links.
Contents: CLAP plug-in, VST3 plug-in and standalone application. These builds have not been certified in Windows or macOS DAWs.
Licensing: project-authored code is Apache-2.0; third-party terms apply. Redistribution permission for the required ni-file parser remains unresolved. Library-access decryption is enabled and does not validate ownership or activation. No commercial Kontakt instrument library is supplied. Read the included THIRD_PARTY.md and docs/LEGAL.md before use or redistribution; the notice/source bundle is not legal clearance.
The universal macOS `.pkg` installs both Intel and Apple Silicon CLAP/VST3 plug-ins under `/Library/Audio/Plug-Ins` and the standalone app under `/Applications`. It is signed with Developer ID Installer, accepted by Apple, and carries a validated stapled ticket. Its `KONTRA-nightly-macos-universal.notarization.json` receipt binds both source targets and product/package hashes. It is the only macOS download; both architectures must pass signing, notarization and package validation before it is published.
x86_64 plug-ins require AVX2, FMA and BMI2. Linux requires compatible X11/XCB, XKB, OpenGL/Vulkan and ALSA/JACK system libraries.

The project retains this snapshot and one previous complete release for rollback. Older release records/downloads and managed nightly source tags are removed only after a complete replacement is published. Stable `vX.Y.Z` source tags are preserved.
""")
    try:
        gh("release", "create", tag, *map(str, files), "dist/release-manifest.json", "--draft", "--target", SHA, "--title", "KONTRA " + version, "--notes-file", "notes.md")
        staging = next(r for r in releases() if r["tag_name"] == tag)
        verify(staging, manifest, data)
        if api("git/ref/heads/main")["object"]["sha"] != SHA:
            gh("release", "delete", tag, "--yes", "--cleanup-tag")
            return
        gh("release", "edit", tag, "--draft=false", "--prerelease=false", "--latest=true")
        staging = next(r for r in releases() if r["tag_name"] == tag)
        finish(staging, manifest, data)
    except BaseException:
        # Preserve a published replacement for recovery; remove only failed drafts.
        staging = next((r for r in releases() if r["tag_name"] == tag), None)
        if staging and staging["draft"]:
            gh("release", "delete", tag, "--yes", "--cleanup-tag")
        raise
    Path(os.environ["GITHUB_OUTPUT"]).write_text("published=true\n")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as error:
        if error.stderr:
            print(error.stderr.decode(), file=sys.stderr, end="")
        raise
