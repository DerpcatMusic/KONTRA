"""Audit byte-identical new-core sources in isolation from legacy workspace members.

Rust Doctor 0.7.0 has no package selector. Preserve the real package manifests,
policy, compiler and build profiles; change only the workspace membership.
"""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

from check_rust_doctor import check


def snapshot(root, destination):
    hashes = {}
    members = sorted(p.parent.relative_to(root).as_posix() for p in (root / "crates").glob("*/Cargo.toml"))
    if not members:
        raise ValueError("No new-core crates found")
    for member in members:
        for source in sorted((root / member).rglob("*")):
            if source.is_file() and "target" not in source.relative_to(root / member).parts:
                relative = source.relative_to(root)
                target = destination / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, target)
                digest = hashlib.sha256(source.read_bytes()).hexdigest()
                if hashlib.sha256(target.read_bytes()).hexdigest() != digest:
                    raise ValueError(f"Snapshot mismatch: {relative}")
                hashes[relative.as_posix()] = digest
    for name in ("rust-toolchain.toml", "rust-doctor.toml"):
        shutil.copyfile(root / name, destination / name)
        hashes[name] = hashlib.sha256((root / name).read_bytes()).hexdigest()
    manifest = '[workspace]\nmembers = ' + json.dumps(members) + '\nresolver = "3"\n'
    keep = False
    for line in (root / "Cargo.toml").read_text().splitlines():
        if line.startswith("["):
            keep = line.startswith("[profile.")
        if keep:
            manifest += line + "\n"
    (destination / "Cargo.toml").write_text(manifest)
    (destination / ".gitignore").write_text("/target/\n")
    return {"members": members, "source_sha256": hashes, "audit_manifest": manifest}


if __name__ == "__main__":
    root = Path(__file__).resolve().parents[2]
    output = Path(sys.argv[1]).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="kontra-core-audit-") as directory:
        directory = Path(directory)
        manifest = snapshot(root, directory)
        manifest["rust_doctor"] = "0.7.0"
        manifest["source_commit"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        manifest["source_sha256"]["Cargo.toml"] = hashlib.sha256((root / "Cargo.toml").read_bytes()).hexdigest()
        manifest["source_sha256"]["Cargo.lock"] = hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest()
        env = {**os.environ, "CARGO_TARGET_DIR": str(directory / "target")}
        # Copy the real lockfile; Cargo prunes unrelated packages while retaining
        # locked versions. New-core dependencies must not be silently upgraded.
        shutil.copyfile(root / "Cargo.lock", directory / "Cargo.lock")
        subprocess.run(["cargo", "metadata", "--offline", "--format-version", "1"],
                       cwd=directory, env=env, stdout=subprocess.DEVNULL, check=True)
        original = tomllib.loads((root / "Cargo.lock").read_text())["package"]
        scoped = tomllib.loads((directory / "Cargo.lock").read_text())["package"]
        locked = {(p["name"], p["version"], p.get("source")) for p in original}
        if any((p["name"], p["version"], p.get("source")) not in locked for p in scoped):
            raise ValueError("Scoped dependency resolution differs from the source lockfile")
        manifest["lockfile_sha256"] = hashlib.sha256((directory / "Cargo.lock").read_bytes()).hexdigest()
        subprocess.run(["git", "init", "-q"], cwd=directory, check=True)
        subprocess.run(["git", "add", "."], cwd=directory, check=True)
        with output.open("w") as report:
            result = subprocess.run(["npx", "-y", "rust-doctor@0.7.0", ".", "--yes", "--scope", "full", "--json"],
                                    cwd=directory, env=env, stdout=report)
        output.with_suffix(".manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        passed, text = check(json.loads(output.read_text()), minimum_score=90)
        print(text, end="")
        if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
            with open(summary, "a") as report:
                report.write(text)
        sys.exit(0 if passed and result.returncode == 0 else 1)
