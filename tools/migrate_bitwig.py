#!/usr/bin/env python3
"""Experimental, explicitly mapped Kontakt VST3 replacement in a new Bitwig copy.

Keep the original. Parameter automation, Kontakt bus assignments and sonic
parity are not translated. Supply a reviewed .kontra-multi with explicit routes.
"""

import argparse
import copy
import hashlib
import io
import json
import re
from pathlib import Path
import struct
import tempfile
import zipfile

from inspect_bitwig import Reader, validate_structure, vst_component
from migrate_project import export_state

KONTRA_ID = b"FC092AF5096DE321B960F4D245F8D72A"


def apply_edits(data, edits):
    for start, end, replacement in sorted(edits, reverse=True):
        data = data[:start] + replacement + data[end:]
    return data


class RewriteReader(Reader):
    def __init__(self, data, start, end, replacements=None):
        super().__init__(data, start, end)
        self.replacements = replacements or {}
        self.edits, self.plugins = [], []

    def obj(self):
        start = self.pos
        cls = int.from_bytes(self.data[start:start + 4], "big")
        super().obj()
        if cls == 1832:  # VST3 device in the verified Bitwig container revision.
            self.plugins.append((start, self.pos))

    def value(self):
        start = self.pos
        tag = self.data[start]
        if tag == 8:
            self.take(1)
            size = self.u32()
            encoding = "utf-16be" if size & 0x80000000 else "utf-8"
            value = self.take((size & 0x7fffffff) * (2 if encoding == "utf-16be" else 1))
            replacement = self.replacements.get(value)
            if replacement is not None:
                self.edits.append((start, self.pos, b"\x08" + struct.pack(">I", len(replacement)) + replacement))
        elif tag == 13:
            self.take(1)
            nested = self.take(self.u32())
            if nested.startswith(b"BtWg") and self.replacements:
                replacement = rewrite_container(nested, self.replacements)
                if replacement != nested:
                    self.edits.append((start, self.pos, b"\x0d" + struct.pack(">I", len(replacement)) + replacement))
        else:
            super().value()


def rewrite_container(data, replacements):
    validate_structure(data)
    start = int(data[16:24], 16)
    boundary = int(data[24:40], 16)
    reader = RewriteReader(data, start, boundary or len(data), replacements)
    while reader.pos < reader.end:
        reader.obj()
    rewritten = apply_edits(data, reader.edits)
    if boundary:
        rewritten = rewritten[:24] + f"{boundary + len(rewritten) - len(data):016x}".encode() + rewritten[40:]
    validate_structure(rewritten)
    return rewritten


def vst_preset(state):
    return (b"VST3\1\0\0\0" + KONTRA_ID + struct.pack("<Q", 48 + len(state)) + state
            + b"List" + struct.pack("<I4sQQ", 1, b"Comp", 48, len(state)))


def migrate(source, output, mappings, exporter, plugin):
    source, output = source.resolve(), output.resolve()
    report_path = output.with_suffix(".migration.json")
    if source == output or output.parent != source.parent or output.suffix.casefold() != ".bwproject":
        raise ValueError("output must be a new .bwproject beside the source")
    if output.exists() or report_path.exists():
        raise ValueError("output or report already exists")
    if not plugin.is_dir() or not plugin.name.endswith(".vst3"):
        raise ValueError("KONTRA VST3 bundle is missing")
    data = source.read_bytes()
    validate_structure(data)
    boundary = int(data[24:40], 16)
    reader = RewriteReader(data, int(data[16:24], 16), boundary)
    reader.obj()
    if reader.pos != boundary:
        raise ValueError("expected exactly one Bitwig project object")
    edits, states, rows = [], {}, []
    with zipfile.ZipFile(source) as archive, tempfile.TemporaryDirectory(prefix="kontra-bitwig-map-") as work:
        for index, (preset_name, multi) in enumerate(mappings.items()):
            entry = "plugin-states/" + preset_name
            preset = archive.read(entry)
            vst_component(preset)
            old_id = preset[8:40]
            if b"kontakt" not in bytes.fromhex(old_id.decode()).lower():
                raise ValueError(f"{entry} is not Kontakt")
            needle = b"\x08" + struct.pack(">I", len(preset_name.encode())) + preset_name.encode()
            candidates = [(a, b) for a, b in reader.plugins if needle in data[a:b]]
            if not candidates:
                raise ValueError(f"no VST3 device references {entry}")
            start, end = min(candidates, key=lambda span: span[1] - span[0])
            if any(not (end <= a or start >= b) for a, b, _ in edits):
                raise ValueError("overlapping/nested mapped devices are unsupported")
            node = data[start:end]
            # Only validated strings inside this device and its cached description change.
            replacements = {old_id: KONTRA_ID}
            # Paths and display names are strings, not arbitrary byte substitutions.
            for match in re.finditer(rb"\x08([\x00-\x7f][\x00-\xff]{3})", node):
                size = int.from_bytes(match[1], "big")
                value = node[match.end():match.end() + size]
                if len(value) != size or size > 4096:
                    continue
                if b"Kontakt" in value and value.endswith(b".vst3\n" + old_id):
                    replacements[value] = str(plugin.resolve()).encode() + b"\n" + KONTRA_ID
                elif b"Kontakt" in value and value.endswith(b".vst3"):
                    replacements[value] = str(plugin.resolve()).encode()
                elif value in (b"Kontakt 7", b"Kontakt 7 Portable", b"Kontakt 8", b"Kontakt 8 Portable"):
                    replacements[value] = b"KONTRA"
            rewritten = RewriteReader(node, 0, len(node), replacements)
            rewritten.obj()
            if not rewritten.edits or old_id not in node:
                raise ValueError("mapped device identity is missing")
            edits.append((start, end, apply_edits(node, rewritten.edits)))
            state, state_report = export_state(exporter, multi, Path(work, f"{index}.state"))
            if any(part["missing_samples"] for part in state_report["imported_parts"]):
                raise ValueError("mapped instruments have missing samples")
            states[entry] = vst_preset(state)
            rows.append({"preset": entry, "mapping": str(multi), "state_export": state_report})
        main = apply_edits(data[:boundary], edits)
        main = main[:24] + f"{len(main):016x}".encode() + main[40:]
        validate_structure(main)
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w", compression=zipfile.ZIP_DEFLATED) as result:
            for info in archive.infolist():
                result.writestr(copy.copy(info), states.get(info.filename, archive.read(info)))
        result_bytes = main + buffer.getvalue()
        with zipfile.ZipFile(io.BytesIO(result_bytes)) as result:
            if result.namelist() != archive.namelist() or result.testzip() is not None:
                raise ValueError("output archive validation failed")
            for info in archive.infolist():
                expected = states.get(info.filename, archive.read(info))
                if result.read(info.filename) != expected:
                    raise ValueError("output state differs from its expected bytes")
    if hashlib.sha256(source.read_bytes()).digest() != hashlib.sha256(data).digest():
        raise ValueError("source changed during migration")
    report = {"status": "experimental_partial_copy", "source": str(source), "output": str(output),
              "source_sha256": hashlib.sha256(data).hexdigest(), "migrated_instances": rows,
              "bitwig_reopen_verified": False,
              "limitations": ["Kontakt parameter automation and static host values are not translated.",
                              "Routing comes from the explicit SavedMulti; original Kontakt bus assignments are not decoded.",
                              "Kontakt sonic parity is unverified; unmapped devices remain unchanged."]}
    with output.open("xb") as destination:
        destination.write(result_bytes)
    with report_path.open("x") as destination:
        json.dump(report, destination, indent=2)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("project", type=Path)
    parser.add_argument("--output-copy", type=Path, required=True)
    parser.add_argument("--map", action="append", required=True, metavar="PRESET.vstpreset=MULTI.kontra-multi")
    parser.add_argument("--state-exporter", type=Path, required=True)
    parser.add_argument("--plugin", type=Path, default=Path.home() / ".vst3/KONTRA.vst3")
    args = parser.parse_args()
    mappings = {}
    for mapping in args.map:
        preset, separator, multi = mapping.partition("=")
        if not separator or preset in mappings or Path(preset).name != preset or not preset.endswith(".vstpreset"):
            parser.error("each mapping needs a unique preset filename and a SavedMulti path")
        mappings[preset] = Path(multi).resolve()
    try:
        print(json.dumps(migrate(args.project, args.output_copy, mappings, args.state_exporter.resolve(), args.plugin), indent=2))
    except (OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        parser.exit(2, f"migrate_bitwig: {error}\n")


if __name__ == "__main__":
    main()
