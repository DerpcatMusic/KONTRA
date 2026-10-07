#!/usr/bin/env python3
"""Build a source-linked registry; no library contents or access data are read."""
import argparse
import csv
import re
from pathlib import Path


def definitions(path):
    text = path.read_text()
    for match in re.finditer(r"^pub struct (\w+)(?:[^\n{;]*)(?:\{|\([^\n]*\)|;)", text, re.M):
        name = match[1]
        line = text[:match.start()].count("\n") + 1
        if match[0].endswith("{"):
            # Public field declarations only, excluding comments and methods.
            start = match.end()
            end = text.find("\n}", start)
            body = text[start:end]
            fields = [(f[1], f[2].strip(), text[:start + f.start()].count("\n") + 1)
                      for f in re.finditer(r"^\s*pub (\w+): ([^\n]+?)(?:,\s*(?://.*)?)?$", body, re.M)]
        else:
            fields = [("raw", "tuple/unit (see source)", line)]
        versions = re.findall(r"(?:Known Versions|Versions):\s*([^\n]+)", text[:match.start()])
        yield name, fields, versions[-1] if versions else "see dispatch; not declared"


def generate(repo, v1, destination):
    roots = ["vendor/ni-file/src/kontakt", "vendor/ni-file/src/nis", "vendor/ni-file/src/nks",
             "vendor/ni-file/src/nkr", "vendor/ni-file/src/file_container", "crates/sampler-kontakt/src"]
    destination.mkdir(parents=True, exist_ok=True)
    with (destination / "KONTAKT_FIELD_REGISTRY.tsv").open("w") as out:
        writer = csv.writer(out, delimiter="\t", lineterminator="\n")
        writer.writerow(["layer", "record", "versions", "field", "wire_or_view_type", "source", "v1_definition", "upstream"])
        for root in roots:
            for path in sorted((repo / root).rglob("*.rs")):
                rel = path.relative_to(repo)
                old = v1 / rel
                old_text = old.read_text() if old.exists() else ""
                for name, fields, versions in definitions(path):
                    for field, kind, line in fields:
                        writer.writerow([root, name, versions, field, kind, f"{rel}:{line}",
                                         "present" if re.search(r"pub struct " + name + r"\b", old_text) else "absent", "unverified (upstream unavailable)"])
    text = (repo / "vendor/ni-file/src/kontakt/chunk.rs").read_text()
    names = dict((int(i, 16), name) for i, name in re.findall(r"(0x[0-9a-f]+) => KontaktObject::(\w+)", text))
    with (destination / "KONTAKT_ID_REGISTRY.tsv").open("w") as out:
        writer = csv.writer(out, delimiter="\t", lineterminator="\n")
        writer.writerow(["domain", "id", "name", "dispatch", "owner"])
        for i in range(0x65):
            owner = "gpt-format-fxmod" if 0x10 <= i <= 0x25 or i in [0, 7, 8, 9, 10, 11, 12, 13, 0x3a, 0x3b, 0x3c, 0x3f, 0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x4a, 0x4c, 0x4d] or i >= 0x52 else "gpt-format-gaps"
            if i in [4, 5, 0xe, 0xf, 0x26, 0x27, 0x28, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x32, 0x33, 0x34, 0x38, 0x39]: owner = "gpt-format-objects"
            if i in [6, 0x4f, 0x50, 0x51]: owner = "gpt-decipher-persist"
            if i == 0x3d: owner = "gpt-format-legacy"
            writer.writerow(["Kontakt", f"0x{i:02x}", names.get(i, "unmapped/reserved"), "typed wrapper" if f"KontaktObject::{names.get(i, '')}(chunk.try_into()?)" in text else "name/raw only", owner])
        source = (repo / "vendor/ni-file/src/nis/container/data/item_type.rs").read_text()
        for domain, body in re.findall(r'"(NISD|NIK4|RKTR)" => match item_id \{(.*?)\n            \}', source, re.S):
            for i, name in re.findall(r"(0x[0-9a-f]+) => ItemType::(\w+)", body):
                writer.writerow([domain, i, name, "framed; see property source", "gpt-format-gaps" if domain != "RKTR" else "outside Kontakt scope"])


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--v1", type=Path, default=Path("/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v1"))
    parser.add_argument("--out", type=Path, default=Path("docs/architecture-v2"))
    args = parser.parse_args()
    generate(args.repo, args.v1, args.out)
