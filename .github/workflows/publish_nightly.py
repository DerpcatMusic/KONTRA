#!/usr/bin/env python3
"""Publish complete nightly snapshots, keeping the newest and one rollback."""
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import subprocess
import tomllib
import zipfile

REPO = os.environ["GH_REPO"]
SHA = os.environ["GITHUB_SHA"]
PLATFORMS = ("linux-x86_64", "windows-x86_64", "macos-arm64", "macos-x86_64")


def gh(*args):
    return subprocess.check_output(["gh", *args])


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
    assert {a["name"] for a in manifest["assets"]} == {f"KONTRA-nightly-{p}.zip" for p in PLATFORMS}
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
        api("git/refs", "--method", "POST", "-f", "ref=refs/tags/" + tag, "-f", "sha=" + revision)


def finish(release, manifest, manifest_bytes):
    # Recover a published staging release before starting another upload.
    verify(release, manifest, manifest_bytes)
    assert not release["draft"]
    tag = f'v{manifest["version"]}'
    assert re.fullmatch(r"v\d+\.\d+\.\d+-nightly\.\d{8}\.g[0-9a-f]{12}", tag), tag
    source_tag(tag, manifest["revision"])
    old = [r for r in releases() if not r["draft"] and r["id"] != release["id"]]
    previous = max(old, key=lambda r: r["published_at"], default=None)
    legacy = False
    if previous:
        assets = {a["name"]: a for a in previous["assets"]}
        for platform in PLATFORMS:
            asset = assets[f"KONTRA-nightly-{platform}.zip"]
            assert asset["state"] == "uploaded" and asset["size"] > 0 and re.fullmatch(r"sha256:[0-9a-f]{64}", asset["digest"])
        if "release-manifest.json" in assets:
            old_data = gh("release", "download", previous["tag_name"], "--pattern", "release-manifest.json", "--output", "-")
            verify(previous, json.loads(old_data), old_data)
        else:
            legacy = True
            source = previous["target_commitish"]
            if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", source):
                obj = api("git/ref/tags/" + previous["tag_name"])["object"]
                while obj["type"] == "tag":
                    obj = api("git/tags/" + obj["sha"])["object"]
                source = obj["sha"]
            previous["target_commitish"] = source
            marker = re.search(r"<!-- kontra-source-tag: ([\w.\-]+) -->", previous.get("body") or "")
            legacy_tag = marker[1] if marker else (previous["tag_name"] if previous["tag_name"].startswith("v") else f"legacy-g{source[:12]}")
            source_tag(legacy_tag, source)
            if not marker:
                previous["body"] = (previous.get("body") or "") + f"\n\nLegacy snapshot: embedded version and build identity metadata are unavailable. Source tag: `{legacy_tag}`.\n<!-- kontra-source-tag: {legacy_tag} -->\n"
    # Only prune after the new release is published and every asset verified.
    for r in old:
        if previous and r["id"] == previous["id"]:
            continue
        gh("release", "delete", r["tag_name"], "--yes", *(["--cleanup-tag"] if managed_ref(r["tag_name"]) else []))
        marker = re.search(r"<!-- kontra-source-tag: (v[\w.\-]+|legacy-g[0-9a-f]{12}) -->", r.get("body") or "")
        if marker and managed_ref(marker[1]) and marker[1] != tag and (not previous or marker[0] not in (previous.get("body") or "")):
            delete_ref(marker[1])
    if previous:
        if previous["tag_name"] != "nightly-previous":
            delete_ref("nightly-previous")
        args = ["release", "edit", previous["tag_name"], "--tag", "nightly-previous", "--target", previous["target_commitish"], "--latest=false"]
        if legacy:
            Path("previous-notes.md").write_text(previous["body"])
            title = previous["name"]
            if "(legacy; build metadata unavailable)" not in title:
                title += " (legacy; build metadata unavailable)"
            args += ["--title", title, "--notes-file", "previous-notes.md"]
        gh(*args)
    delete_ref("nightly")
    gh("release", "edit", release["tag_name"], "--tag", "nightly", "--target", manifest["revision"], "--latest=false")
    delete_ref("nightly-staging")


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
    # A rerun of the same source/version must not evict the previous snapshot.
    if any(r["tag_name"] == "nightly" and r["target_commitish"] == SHA and r["name"] == "KONTRA " + version for r in releases()):
        Path(os.environ["GITHUB_OUTPUT"]).write_text("published=true\n")
        return
    if staging and staging["draft"]:
        gh("release", "delete", "nightly-staging", "--yes", "--cleanup-tag")
    delete_ref("nightly-staging")
    assets = []
    for p, platform in zip(files, PLATFORMS):
        digest = hashlib.sha256(p.read_bytes()).hexdigest()
        assert p.with_suffix(".zip.sha256").read_text().strip() == digest + "  " + p.name, "Archive checksum mismatch: " + p.name
        with zipfile.ZipFile(p) as archive:
            assert archive.testzip() is None, p.name
            prefix = f"KONTRA-nightly-{platform}/"
            if platform.startswith("macos-"):
                binaries = ("KONTRA.clap/Contents/MacOS/KONTRA", "KONTRA.vst3/Contents/MacOS/KONTRA", "kontakto-standalone")
            elif platform.startswith("windows-"):
                binaries = ("KONTRA.clap", "KONTRA.vst3/Contents/x86_64-win/KONTRA.vst3", "kontakto-standalone.exe")
            else:
                binaries = ("KONTRA.clap", "KONTRA.vst3/Contents/x86_64-linux/KONTRA.so", "kontakto-standalone")
            for name in (*binaries, "LICENSE", "NOTICE", "THIRD_PARTY.md"):
                assert archive.getinfo(prefix + name).file_size > 0, (p.name, name)
            assert archive.read(prefix + "SOURCE_COMMIT.txt").decode().strip() == SHA, p.name
            info = [json.loads(archive.read(prefix + name)) for name in ("clap-build-info.json", "vst3-build-info.json", "build-info.json")]
            if platform.startswith("macos-"):
                for bundle in ("KONTRA.clap", "KONTRA.vst3"):
                    plist = plistlib.loads(archive.read(prefix + bundle + "/Contents/Info.plist"))
                    assert plist["CFBundleShortVersionString"] == plist["CFBundleVersion"] == version.split("-", 1)[0]
                    assert plist["KONTRAVersion"] == version
        target = {"linux-x86_64": "x86_64-unknown-linux-gnu", "windows-x86_64": "x86_64-pc-windows-msvc", "macos-arm64": "aarch64-apple-darwin", "macos-x86_64": "x86_64-apple-darwin"}[platform]
        assert all(i["version"] == version and i["revision"] == SHA and i["target"] == target and i["profile"] == "release" and {"plugin", "library-access"} <= set(i["features"]) for i in info), p.name
        for i, format in zip(info[:2], ("clap", "vst3")):
            assert set(i["features"]) & {"clap", "vst3", "standalone"} == {format}, p.name
        assert {"clap", "vst3", "standalone"} <= set(info[2]["features"]), p.name
        assets.append(dict(name=p.name, platform=platform, size=p.stat().st_size, sha256=digest, clap_build=info[0], vst3_build=info[1], standalone_build=info[2]))
    manifest = dict(version=version, revision=SHA, workflow_run=os.environ["GITHUB_RUN_ID"], assets=assets)
    data = (json.dumps(manifest, indent=2) + "\n").encode()
    Path("dist/release-manifest.json").write_bytes(data)
    Path("notes.md").write_text(f"""Automated **{version}** snapshot of [{SHA[:12]}](https://github.com/{REPO}/commit/{SHA}).
Source tag: [`v{version}`](https://github.com/{REPO}/tree/v{version}).
<!-- kontra-source-tag: v{version} -->

All four archives come from this source commit. `release-manifest.json` records their sizes and SHA256 checksums; each archive includes separate `clap-build-info.json`, `vst3-build-info.json` and standalone `build-info.json` with their actual feature sets.
Contents: CLAP plug-in, VST3 plug-in and standalone application. These builds have not been certified in Windows or macOS DAWs.
macOS builds are signed ad hoc, not notarized: after unzipping, run `xattr -dr com.apple.quarantine KONTRA.clap KONTRA.vst3 kontakto-standalone`.
x86_64 plug-ins require AVX2, FMA and BMI2. Linux requires Ubuntu 24.04-compatible system libraries.

The project retains this snapshot and one previous complete release for rollback. Older release records/downloads and managed nightly source tags are removed only after a complete replacement is published. Stable `vX.Y.Z` source tags are preserved.
""")
    try:
        gh("release", "create", "nightly-staging", *map(str, files), "dist/release-manifest.json", "--draft", "--prerelease", "--target", SHA, "--title", "KONTRA " + version, "--notes-file", "notes.md")
        staging = next(r for r in releases() if r["tag_name"] == "nightly-staging")
        verify(staging, manifest, data)
        if api("git/ref/heads/main")["object"]["sha"] != SHA:
            gh("release", "delete", "nightly-staging", "--yes", "--cleanup-tag")
            return
        gh("release", "edit", "nightly-staging", "--draft=false", "--latest=false")
        staging["draft"] = False
        finish(staging, manifest, data)
    except BaseException:
        # Preserve a published replacement for recovery; remove only failed drafts.
        staging = next((r for r in releases() if r["tag_name"] == "nightly-staging"), None)
        if staging and staging["draft"]:
            gh("release", "delete", "nightly-staging", "--yes", "--cleanup-tag")
        raise
    Path(os.environ["GITHUB_OUTPUT"]).write_text("published=true\n")


if __name__ == "__main__":
    main()
