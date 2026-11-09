#!/usr/bin/env python3
"""Report framework source drift without modifying repositories."""
import argparse
import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SHA = re.compile(r"^[0-9a-f]{40}$")
SKIP = {".git", "target", "node_modules", ".graft"}


def identity(url):
    return url.removeprefix("git+").split("?")[0].rstrip("/").removesuffix(".git").lower()


def files(root, name):
    import os
    for directory, dirs, names in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP and not d.startswith("."))
        if name in names:
            yield Path(directory) / name


def load_manifest(path):
    data = tomllib.loads(path.read_text())
    families = data["family"]
    repos = set()
    for family in families.values():
        if not SHA.fullmatch(family["revision"]):
            raise ValueError("family revisions must be full lowercase commit SHAs")
        repo = identity(family["repository"])
        if repo in repos or not family["packages"]:
            raise ValueError("each family needs a unique repository and package list")
        repos.add(repo)
    return data


def scan(root, manifest):
    issues, resolved, declarations, tooling = [], [], [], []
    families = manifest["family"]
    aliases = manifest.get("aliases", {})
    owners = {}
    for name, family in families.items():
        for package in family["packages"]:
            owners.setdefault(package, []).append(name)
    repo_names = {identity(f["repository"]): n for n, f in families.items()}
    def issue(kind, **details):
        issues.append({"kind": kind, **details})
    locks = sorted(files(root, "Cargo.lock"))
    if not locks:
        issue("missing-lockfile")
    for lock in locks:
        graph = {}
        for package in tomllib.loads(lock.read_text()).get("package", []):
            name = package["name"]
            source = package.get("source", "path/workspace")
            canonical = aliases.get(name, name)
            source_repo = identity(source.split("#")[0]) if source.startswith("git+") else ""
            family_name = repo_names.get(source_repo)
            if canonical not in owners and not family_name and not name.startswith(("mui-", "moose-")):
                continue
            entry = {"lock": str(lock.relative_to(root)), "package": name, "version": package["version"], "source": source}
            resolved.append(entry)
            graph.setdefault(canonical, set()).add(source)
            if name in aliases:
                issue("custom-alias", package=name, represents=canonical, lock=entry["lock"])
            if not family_name or family_name not in owners.get(canonical, []):
                issue("unregistered-source", **entry)
            elif source.rsplit("#", 1)[-1] != families[family_name]["revision"]:
                issue("revision-drift", family=family_name, **entry)
        for package, sources in graph.items():
            if len(sources) > 1:
                issue("multiple-sources", lock=str(lock.relative_to(root)), package=package, sources=sorted(sources))
    def visit(value, path, manifest_path):
        if isinstance(value, dict):
            if "git" in value and isinstance(value["git"], str) and identity(value["git"]) in repo_names:
                family_name = repo_names[identity(value["git"])]
                revision = value.get("rev")
                entry = {"file": manifest_path, "key": ".".join(path), "family": family_name, "revision": revision}
                declarations.append(entry)
                if revision != families[family_name]["revision"]:
                    issue("declaration-drift", **entry)
            if len(path) >= 2 and path[0] == "patch" and "path" in value:
                package = value.get("package", path[-1])
                if package in owners or package in aliases or identity(path[1]) in repo_names:
                    issue("local-patch", file=manifest_path, key=".".join(path), package=package, path=value["path"])
            for key, child in value.items():
                visit(child, path + [key], manifest_path)
        elif isinstance(value, list):
            for child in value:
                visit(child, path, manifest_path)
    for cargo in sorted(files(root, "Cargo.toml")):
        visit(tomllib.loads(cargo.read_text()), [], str(cargo.relative_to(root)))
    workflow_dir = root / ".github" / "workflows"
    if workflow_dir.is_dir():
        for workflow in sorted(workflow_dir.iterdir()):
            if workflow.suffix not in (".yml", ".yaml"):
                continue
            content = workflow.read_text()
            variables = {}
            for key, value in re.findall(r'^\s*([A-Za-z_][A-Za-z_0-9]*):\s*["\']?([0-9a-f]{40})["\']?\s*$', content, re.M):
                variables.setdefault(key, set()).add(value)
            for line_number, line in enumerate(content.splitlines(), 1):
                if line.lstrip().startswith("#") or not re.search(r'\bcargo(?:\s+\+\S+)?\s+install\b.*\bcargo-moose\b', line):
                    continue
                rev = re.search(r'--rev(?:=|\s+)["\']?([^\s"\']+)', line)
                value = rev.group(1) if rev else ""
                if value.startswith("$"):
                    choices = variables.get(value.strip('${}'), set())
                    value = next(iter(choices)) if len(choices) == 1 else ""
                entry = {"file": str(workflow.relative_to(root)), "line": line_number, "revision": value or None}
                tooling.append(entry)
                if value != families["moose"]["revision"] or identity("https://github.com/Matari-Audio/moose") not in identity(line):
                    issue("tooling-drift", **entry)
    return {"repository": str(root), "lockfiles": [{"path": str(p.relative_to(root)), "sha256": hashlib.sha256(p.read_bytes()).hexdigest()} for p in locks], "resolved": resolved, "declarations": declarations, "tooling": tooling, "issues": issues}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repositories", nargs="+", type=Path)
    parser.add_argument("--manifest", type=Path, default=ROOT / "stack.toml")
    args = parser.parse_args()
    try:
        manifest = load_manifest(args.manifest)
        reports = []
        for repo in args.repositories:
            if not repo.is_dir():
                raise ValueError(f"repository does not exist: {repo}")
            reports.append(scan(repo.resolve(), manifest))
        print(json.dumps({"manifest_status": manifest.get("status"), "manifest_sha256": hashlib.sha256(args.manifest.read_bytes()).hexdigest(), "repositories": reports}, indent=2))
        return int(any(report["issues"] for report in reports))
    except (OSError, ValueError, KeyError, tomllib.TOMLDecodeError) as error:
        print(f"stack-check: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
